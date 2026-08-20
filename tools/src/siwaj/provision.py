"""Serial provisioner: pushes .env secrets into the device's encrypted NVS REPL."""

import argparse
import os
import sys
import time
from pathlib import Path

import serial

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
            conn.write(f"set {key} {value}\n".encode())
            deadline = time.time() + TIMEOUT_SECONDS
            reply = ""
            while time.time() < deadline:
                reply += conn.read_until(b"\n").decode(errors="replace")
                if "OK" in reply:
                    print(f"set {key}: ok")
                    break
                if "ERR" in reply:
                    print(f"set {key}: {reply.strip()}", file=sys.stderr)
                    return 1
            else:
                print(f"set {key}: no reply from device", file=sys.stderr)
                return 1
    finally:
        conn.close()
    print("provisioned", len(secrets), "secrets")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", default=os.environ.get("SIWAJ_PORT", "/dev/cu.usbmodem101"))
    parser.add_argument("--env", type=Path, default=Path(__file__).resolve().parents[3] / ".env")
    args = parser.parse_args()
    if not args.env.exists():
        print(f"missing {args.env}; copy .env.example to .env first", file=sys.stderr)
        return 1
    return provision(args.port, args.env)


if __name__ == "__main__":
    raise SystemExit(main())
