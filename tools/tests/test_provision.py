from dataclasses import dataclass
from pathlib import Path

import pytest

from siwaj.provision import detect_port, load_env


@dataclass
class FakePort:
    device: str
    vid: int | None


def write_env(tmp_path: Path, content: str) -> Path:
    env = tmp_path / ".env"
    env.write_text(content, encoding="utf-8")
    return env


def test_load_env_skips_comments_blanks_and_empty(tmp_path: Path) -> None:
    env = write_env(
        tmp_path,
        "# comment\n\nOPENWEATHER_API_KEY=abc123\nEMPTY=\nSPACED = spaced \n",
    )
    assert load_env(env) == {"OPENWEATHER_API_KEY": "abc123", "SPACED": "spaced"}


def test_load_env_empty_file(tmp_path: Path) -> None:
    env = write_env(tmp_path, "")
    assert load_env(env) == {}


def fake_comports(monkeypatch: pytest.MonkeyPatch, *ports: FakePort) -> None:
    monkeypatch.setattr("siwaj.provision.serial.tools.list_ports.comports", lambda: list(ports))


def test_detect_port_ignores_non_board_usb_devices(monkeypatch: pytest.MonkeyPatch) -> None:
    """A monitor's control channel enumerates as a usbmodem too, and picking it
    hands espflash a port that answers nothing."""
    fake_comports(
        monkeypatch,
        FakePort("/dev/cu.usbmodemABC1234567892", 0x043E),
        FakePort("/dev/cu.Bluetooth-Incoming-Port", None),
        FakePort("/dev/cu.usbmodem101", 0x303A),
    )
    assert detect_port() == "/dev/cu.usbmodem101"


def test_detect_port_none_when_no_board(monkeypatch: pytest.MonkeyPatch) -> None:
    fake_comports(monkeypatch, FakePort("/dev/cu.usbmodemABC1234567892", 0x043E))
    assert detect_port() is None


def test_detect_port_none_when_two_boards(monkeypatch: pytest.MonkeyPatch) -> None:
    fake_comports(
        monkeypatch,
        FakePort("/dev/cu.usbmodem101", 0x303A),
        FakePort("/dev/cu.usbserial-1", 0x10C4),
    )
    assert detect_port() is None
