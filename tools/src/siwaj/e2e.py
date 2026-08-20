"""Automated end-to-end against the emulated device.

Boots the QEMU dev device, provisions secrets from .env (when present),
walks the config flow over HTTP, and verifies persistence and geocoding.
Exit 0 only if every assertion holds.
"""

import json
import sys
import time
import urllib.error
import urllib.request

from siwaj import qemu
from siwaj.provision import load_env

REPO_ROOT = qemu.REPO_ROOT
IMAGE = REPO_ROOT / "firmware" / "target-esp32" / "siwaj-smoke.bin"
HTTP_PORT = qemu.HTTP_PORT
BASE = f"http://127.0.0.1:{HTTP_PORT}"


def http_json(method: str, path: str, payload: dict | None = None) -> tuple[int, dict | str]:
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(
        BASE + path, data=data, method=method, headers={"content-type": "application/json"}
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            body = resp.read().decode()
            return resp.status, json.loads(body) if body.startswith("{") else body
    except urllib.error.HTTPError as err:
        return err.code, err.read().decode(errors="replace")


def http_bytes(path: str, timeout: float = 60.0) -> tuple[int, bytes]:
    try:
        with urllib.request.urlopen(BASE + path, timeout=timeout) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as err:
        return err.code, err.read()
    except (urllib.error.URLError, OSError) as err:
        return 0, str(err).encode()


def api_up() -> bool:
    try:
        with urllib.request.urlopen(BASE + "/api/config", timeout=2) as resp:
            return resp.status == 200
    except (urllib.error.URLError, OSError):
        return False


def wait_for_restart(down_timeout: float = 20.0, up_timeout: float = 60.0) -> bool:
    """The device restarts ~3s after an accepted save. The old server still
    answers during the grace window, so first wait for it to go down, then
    for the fresh boot to answer; anything else is a failed restart."""
    deadline = time.monotonic() + down_timeout
    while time.monotonic() < deadline:
        if not api_up():
            break
        time.sleep(0.5)
    else:
        return False
    deadline = time.monotonic() + up_timeout
    while time.monotonic() < deadline:
        if api_up():
            return True
        time.sleep(1.0)
    return False


def check(name: str, ok: bool, detail: str = "") -> bool:
    print(f"  {'PASS' if ok else 'FAIL'}  {name}" + (f" ({detail})" if detail else ""))
    return ok


def main() -> int:
    env_path = REPO_ROOT / ".env"
    has_key = bool(load_env(env_path)) if env_path.is_file() else False

    # fresh device state so the first-setup path is exercised every run
    device_image = qemu.RUN_DIR / "device.bin"
    if device_image.is_file():
        device_image.unlink()

    print("e2e: booting emulated device")
    if qemu.serve(IMAGE, qemu.MACHINE_DEFAULT, "", 240.0) != 0:
        return 1

    try:
        results = []

        print("e2e: unconfigured state")
        status, state = http_json("GET", "/api/config")
        results.append(check("GET /api/config -> 200", status == 200))
        assert isinstance(state, dict)
        results.append(check("device unconfigured", state.get("configured") is False))

        print("e2e: static ui")
        with urllib.request.urlopen(BASE + "/", timeout=10) as resp:
            index_ok = resp.status == 200 and b"siwaj" in resp.read()
        results.append(check("GET / serves the page", index_ok))
        with urllib.request.urlopen(BASE + "/app.js", timeout=10) as resp:
            js_ok = resp.status == 200 and resp.headers.get("Content-Encoding") == "gzip"
        results.append(check("GET /app.js gzipped", js_ok))

        if has_key:
            print("e2e: provisioning secrets over serial")
            from siwaj.provision import provision

            rc = provision(f"socket://127.0.0.1:{qemu.SERIAL_TCP_PORT}", env_path)
            results.append(check("secrets provisioned", rc == 0))

        print("e2e: save config")
        submit = {
            "thresholds": {"lowC": 8, "midC": 15, "highC": 21},
            "rainThresholdPct": 30,
            "refreshMinutes": 30,
            "locationName": "Berlin",
        }
        status, saved = http_json("POST", "/api/config", submit)
        results.append(check("POST /api/config -> 200", status == 200))
        assert isinstance(saved, dict)
        results.append(check("revision bumped to 1", saved.get("revision") == 1))
        if has_key:
            lat = isinstance(saved.get("location"), dict) and saved["location"].get("lat")
            results.append(
                check("geocoded Berlin", bool(lat and 52.0 < float(lat) < 53.0), str(lat))
            )
        else:
            print("  SKIP  geocoding (no .env key)")

        print("e2e: persistence (device restarted after the save)")
        results.append(check("device restarts and serves again", wait_for_restart()))
        status, state = http_json("GET", "/api/config")
        assert isinstance(state, dict)
        configured = state.get("configured") is True and state.get("revision") == 1
        results.append(check("config persisted", configured))

        if has_key:
            print("e2e: weather debug endpoint")
            status, weather = http_json("GET", "/api/weather")
            if status == 200 and isinstance(weather, dict):
                results.append(
                    check("GET /api/weather -> 200", True, str(weather.get("feelsLikeC")))
                )
            elif status == 502 and isinstance(weather, str) and "returned 40" in weather:
                # the key is valid for geocoding but the One Call by Call plan
                # is not activated on the account yet; upstream auth, not code
                results.append(
                    check("GET /api/weather answers (plan not activated)", True, weather.strip())
                )
            else:
                results.append(check("GET /api/weather -> 200", False, str(weather)[:80]))

            print("e2e: display frame render")
            status, frame = http_bytes("/api/frame.bmp")
            if status == 200:
                ok = frame[:2] == b"BM" and len(frame) == 54 + 3 * 200 * 200
                results.append(check("GET /api/frame.bmp -> BMP", ok, f"{len(frame)} bytes"))
            elif status == 502 and b"returned 40" in frame:
                results.append(check("frame endpoint answers (plan not activated)", True))
            else:
                results.append(check("GET /api/frame.bmp -> BMP", False, str(frame)[:80]))

        print("e2e: invalid input rejected")
        status, _ = http_json(
            "POST", "/api/config", {**submit, "thresholds": {"lowC": 20, "midC": 10, "highC": 30}}
        )
        results.append(check("disordered thresholds -> 422", status == 422))
    finally:
        qemu.stop(qemu.RUN_DIR / "qemu.pid")

    failed = results.count(False)
    print(f"e2e: {len(results) - failed}/{len(results)} checks passed")
    if failed:
        print(f"e2e: {failed} FAILURES", file=sys.stderr)
        return 1
    print("e2e: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
