"""Serial provisioner: pushes .env secrets into the device's encrypted NVS REPL."""

import argparse
import os
import sys
import time
from pathlib import Path

import serial
import serial.tools.list_ports

BAUD = 115200
SETTLE_SECONDS = 2.0
TIMEOUT_SECONDS = 10.0


def load_env(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        if value.strip():
            values[key.strip()] = value.strip()
    return values


def send(conn: serial.Serial, command: str, timeout: float = TIMEOUT_SECONDS) -> str:
    """One exchange with the device's REPL: write a line, read until it answers
    OK or ERR. Returns the reply, empty when the device stayed silent."""
    conn.write(f"{command}\n".encode())
    deadline = time.time() + timeout
    reply = ""
    while time.time() < deadline:
        reply += conn.read_until(b"\n").decode(errors="replace")
        if "OK" in reply or "ERR" in reply:
            return reply.strip()
    return reply.strip()


def provision(port: str, env_path: Path) -> int:
    secrets = load_env(env_path)
    if not secrets:
        print(f"no populated variables in {env_path}", file=sys.stderr)
        return 1

    conn = serial.serial_for_url(port, BAUD, timeout=1)
    try:
        time.sleep(SETTLE_SECONDS)
        conn.reset_input_buffer()
        for key, value in secrets.items():
            reply = send(conn, f"set {key} {value}")
            if "OK" in reply:
                print(f"set {key}: ok")
            elif "ERR" in reply:
                print(f"set {key}: {reply}", file=sys.stderr)
                return 1
            else:
                print(f"set {key}: no reply from device", file=sys.stderr)
                return 1
    finally:
        conn.close()
    print("provisioned", len(secrets), "secrets")
    return 0


"""USB vendors the board can appear under: Espressif's own on-die USB, and the
UART bridges other revisions carry. Matching the vendor keeps unrelated
usbmodem devices (a monitor's control channel, a phone) out of the running."""
BOARD_VENDOR_IDS = frozenset(
    {
        0x303A,  # Espressif USB-serial-JTAG
        0x10C4,  # Silicon Labs CP210x
        0x1A86,  # WCH CH34x
        0x0403,  # FTDI
    }
)


def board_ports() -> list[str]:
    return [p.device for p in serial.tools.list_ports.comports() if p.vid in BOARD_VENDOR_IDS]


def detect_port() -> str | None:
    """The board enumerates under a different name on every host, so the port
    is discovered rather than assumed. Ambiguity is the caller's to resolve."""
    candidates = board_ports()
    return candidates[0] if len(candidates) == 1 else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", default=os.environ.get("SIWAJ_PORT"))
    parser.add_argument("--env", type=Path, default=Path(__file__).resolve().parents[3] / ".env")
    args = parser.parse_args()
    if not args.env.exists():
        print(f"missing {args.env}; copy .env.example to .env first", file=sys.stderr)
        return 1
    port = args.port or detect_port()
    if port is None:
        seen = [
            f"{p.device} (vid {p.vid:#06x})" if p.vid else p.device
            for p in serial.tools.list_ports.comports()
        ]
        print(f"no single board to provision; pass --port. seen: {seen}", file=sys.stderr)
        return 1
    print(f"provisioning over {port}")
    return provision(port, args.env)


if __name__ == "__main__":
    raise SystemExit(main())
