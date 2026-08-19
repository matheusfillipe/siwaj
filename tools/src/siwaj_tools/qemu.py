"""QEMU smoke runner: boots the merged firmware flash image under the Espressif QEMU fork.

Expects the repo-local QEMU binary (installed by `make qemu-install`) at tools/bin/qemu/bin/.
"""

import argparse
import contextlib
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
QEMU_CANDIDATES = [
    REPO_ROOT / "tools" / "bin" / "qemu" / "bin" / "qemu-system-xtensa",
]
QEMU_SYSTEM_XTENSA = "qemu-system-xtensa"
MACHINE_DEFAULT = "esp32"
EXPECT_LINE = "siwaj awake"
BOOT_TIMEOUT_SECONDS = 180.0


def resolve_qemu() -> str | None:
    for candidate in QEMU_CANDIDATES:
        if candidate.is_file():
            return str(candidate)
    return shutil.which(QEMU_SYSTEM_XTENSA)


def smoke(
    image: Path,
    machine: str,
    expect: str,
    timeout: float,
    hostfwd: int | None,
    keep_running: bool,
) -> int:
    qemu = resolve_qemu()
    if qemu is None:
        print(
            "qemu-system-xtensa not found. Run `make qemu-install` "
            "(downloads the Espressif QEMU fork into tools/bin/qemu).",
            file=sys.stderr,
        )
        return 1
    if not image.is_file():
        print(f"flash image {image} not found. Run `make firmware-image` first.", file=sys.stderr)
        return 1

    cmd = [qemu, "-machine", machine]
    if hostfwd is not None:
        cmd.append("-nic")
        cmd.append(f"user,model=open_eth,hostfwd=tcp:127.0.0.1:{hostfwd}-:80")
    cmd += [
        "-drive",
        f"file={image},if=mtd,format=raw",
        "-serial",
        "mon:stdio",
        "-display",
        "none",
    ]
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )
    deadline = time.monotonic() + timeout
    assert proc.stdout is not None
    found = False
    try:
        for line in proc.stdout:
            print(line, end="")
            if expect in line:
                found = True
                break
            if time.monotonic() > deadline:
                break
    finally:
        if not (found and keep_running):
            kill_process_group(proc)

    if found and keep_running:
        print(
            f"smoke: '{expect}' seen; server running on http://127.0.0.1:{hostfwd} (ctrl-c to stop)"
        )
        with contextlib.suppress(KeyboardInterrupt):
            proc.wait()
        kill_process_group(proc)
        return 0

    if found:
        print(f"smoke: '{expect}' seen, PASS")
        return 0
    print(f"smoke: '{expect}' not seen within {timeout}s, FAIL", file=sys.stderr)
    return 1


def kill_process_group(proc: subprocess.Popen[str]) -> None:
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        proc.kill()
    with contextlib.suppress(subprocess.TimeoutExpired):
        proc.wait(timeout=5)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "image",
        type=Path,
        help="merged flash image (from `make firmware-image`)",
    )
    parser.add_argument("--machine", default=MACHINE_DEFAULT, help="QEMU machine type")
    parser.add_argument("--expect", default=EXPECT_LINE, help="serial line that marks a good boot")
    parser.add_argument("--timeout", type=float, default=BOOT_TIMEOUT_SECONDS)
    parser.add_argument(
        "--hostfwd",
        type=int,
        default=None,
        help="forward this host port to guest 80 via emulated OpenETH",
    )
    parser.add_argument(
        "--keep-running",
        action="store_true",
        help="after the expect line is seen, keep QEMU alive (interactive server)",
    )
    args = parser.parse_args()
    return smoke(
        args.image, args.machine, args.expect, args.timeout, args.hostfwd, args.keep_running
    )


if __name__ == "__main__":
    raise SystemExit(main())
