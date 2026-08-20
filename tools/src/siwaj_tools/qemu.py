"""QEMU runner: smoke-tests the firmware image or serves it as a dev device.

smoke: boot, wait for the expect line, terminate. CI-shaped.
serve: boot detached; HTTP forwarded to 127.0.0.1:<http_port>, serial
       exposed as a TCP socket for `make provision`/monitoring.
"""

import argparse
import contextlib
import os
import queue
import shutil
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import TextIO

REPO_ROOT = Path(__file__).resolve().parents[3]
QEMU_CANDIDATES = [
    REPO_ROOT / "tools" / "bin" / "qemu" / "bin" / "qemu-system-xtensa",
]
QEMU_SYSTEM_XTENSA = "qemu-system-xtensa"
MACHINE_DEFAULT = "esp32"
EXPECT_LINE = "siwaj awake"
BOOT_TIMEOUT_SECONDS = 180.0
RUN_DIR = REPO_ROOT / "firmware" / "target-esp32" / "qemu-dev"

SERIAL_TCP_PORT = 47653
HTTP_PORT = 47652


def resolve_qemu() -> str | None:
    for candidate in QEMU_CANDIDATES:
        if candidate.is_file():
            return str(candidate)
    return shutil.which(QEMU_SYSTEM_XTENSA)


def base_cmd(image: Path, machine: str, http_port: int, serial: list[str]) -> list[str]:
    qemu = resolve_qemu()
    if qemu is None:
        raise RuntimeError(
            "qemu-system-xtensa not found; run `make qemu-install` "
            "(downloads the Espressif QEMU fork into tools/bin/qemu)"
        )
    return [
        qemu,
        "-machine",
        machine,
        "-nic",
        f"user,model=open_eth,hostfwd=tcp:127.0.0.1:{http_port}-:80",
        "-drive",
        f"file={image},if=mtd,format=raw",
        *serial,
        "-display",
        "none",
    ]


def kill_process_group(proc: subprocess.Popen) -> None:
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        proc.kill()
    with contextlib.suppress(subprocess.TimeoutExpired):
        proc.wait(timeout=5)


def smoke(image: Path, machine: str, expect: str, timeout: float) -> int:
    if not image.is_file():
        print(f"flash image {image} not found; run `make firmware-image`", file=sys.stderr)
        return 1
    cmd = base_cmd(image, machine, HTTP_PORT, ["-serial", "mon:stdio"])
    proc = subprocess.Popen(
        cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, start_new_session=True
    )
    deadline = time.monotonic() + timeout
    assert proc.stdout is not None
    # a blocking line-iterator would sleep past the deadline on a silent boot;
    # a reader thread lets the main loop enforce the total-time bound
    lines: queue.Queue[str] = queue.Queue()

    def reader(stream: TextIO) -> None:
        for line in stream:
            lines.put(line)

    threading.Thread(target=reader, args=(proc.stdout,), daemon=True).start()

    found = False
    timed_out = False
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                timed_out = True
                break
            try:
                line = lines.get(timeout=min(remaining, 1.0))
            except queue.Empty:
                continue
            print(line, end="")
            if expect in line:
                found = True
                break
    finally:
        kill_process_group(proc)
    if found:
        print(f"smoke: '{expect}' seen, PASS")
        return 0
    reason = "timed out" if timed_out else "output ended"
    print(f"smoke: '{expect}' not seen ({reason} within {timeout}s), FAIL", file=sys.stderr)
    return 1


def http_up(http_port: int) -> bool:
    import urllib.error
    import urllib.request

    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{http_port}/api/config", timeout=3) as resp:
            return resp.status == 200
    except (urllib.error.URLError, OSError):
        return False


def serve(image: Path, machine: str, expect: str, timeout: float) -> int:
    del expect  # readiness is detected by polling the forwarded HTTP endpoint
    if not image.is_file():
        print(f"flash image {image} not found; run `make firmware-image`", file=sys.stderr)
        return 1
    RUN_DIR.mkdir(parents=True, exist_ok=True)
    pid_file = RUN_DIR / "qemu.pid"
    log_file = RUN_DIR / "qemu.log"
    device_image = RUN_DIR / "device.bin"
    stop(pid_file)

    # QEMU writes NVS straight into the flash image: run a working copy so the
    # build artifact stays pristine and device state persists across restarts
    shutil.copyfile(image, device_image)

    cmd = base_cmd(
        device_image,
        machine,
        HTTP_PORT,
        [
            "-chardev",
            f"socket,id=ser0,host=127.0.0.1,port={SERIAL_TCP_PORT},server=on,wait=off",
            "-serial",
            "chardev:ser0",
        ],
    )
    with log_file.open("w") as log:
        proc = subprocess.Popen(
            cmd,
            stdout=log,
            stderr=subprocess.STDOUT,
            stdin=subprocess.DEVNULL,
            start_new_session=True,
        )
    pid_file.write_text(str(proc.pid))

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            print(f"qemu exited early (rc={proc.returncode}); see {log_file}", file=sys.stderr)
            return 1
        if http_up(HTTP_PORT):
            print(f"qemu dev device up (pid {proc.pid})")
            print(f"  web ui  : http://127.0.0.1:{HTTP_PORT}")
            print(f"  serial  : socket://127.0.0.1:{SERIAL_TCP_PORT} (make qemu-provision)")
            print(f"  log     : {log_file}")
            print("  stop    : make qemu-stop")
            return 0
        time.sleep(2)
    print(
        f"http://127.0.0.1:{HTTP_PORT} not answering within {timeout}s; see {log_file}",
        file=sys.stderr,
    )
    stop(pid_file)
    return 1


def stop(pid_file: Path) -> int:
    if not pid_file.is_file():
        return 0
    pid = int(pid_file.read_text().strip())
    with contextlib.suppress(ProcessLookupError, PermissionError):
        os.killpg(os.getpgid(pid), signal.SIGKILL)
    pid_file.unlink(missing_ok=True)
    print("qemu dev device stopped")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["smoke", "serve", "stop"])
    parser.add_argument("image", type=Path, nargs="?", help="merged flash image")
    parser.add_argument("--machine", default=MACHINE_DEFAULT, help="QEMU machine type")
    parser.add_argument("--expect", default=EXPECT_LINE, help="serial line marking a good boot")
    parser.add_argument("--timeout", type=float, default=BOOT_TIMEOUT_SECONDS)
    args = parser.parse_args()

    if args.command == "stop":
        return stop(RUN_DIR / "qemu.pid")
    if args.image is None:
        parser.error("smoke/serve need a flash image path")
    if args.command == "smoke":
        return smoke(args.image, args.machine, args.expect, args.timeout)
    return serve(args.image, args.machine, args.expect, args.timeout)


if __name__ == "__main__":
    raise SystemExit(main())
