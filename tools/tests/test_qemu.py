import struct
from pathlib import Path

from siwaj.qemu import PARTITION_TABLE_OFFSET, nvs_partitions, refresh_device_image

FLASH_LEN = 0x40000
NVS_OFFSET = 0x9000
NVS_SIZE = 0x1000
APP_OFFSET = 0x20000


def entry(ptype: int, subtype: int, offset: int, size: int, label: bytes) -> bytes:
    return (
        b"\xaa\x50"
        + bytes([ptype, subtype])
        + struct.pack("<II", offset, size)
        + label.ljust(16, b"\x00")
        + struct.pack("<I", 0)
    )


def image(app_byte: int, nvs_byte: int) -> bytes:
    flash = bytearray(b"\xff" * FLASH_LEN)
    table = (
        entry(0x01, 0x02, NVS_OFFSET, NVS_SIZE, b"nvs")
        + entry(0x01, 0x04, NVS_OFFSET + NVS_SIZE, NVS_SIZE, b"nvs_key")
        + entry(0x00, 0x00, APP_OFFSET, 0x1000, b"factory")
    )
    flash[PARTITION_TABLE_OFFSET : PARTITION_TABLE_OFFSET + len(table)] = table
    flash[NVS_OFFSET : NVS_OFFSET + NVS_SIZE] = bytes([nvs_byte]) * NVS_SIZE
    flash[APP_OFFSET : APP_OFFSET + 0x1000] = bytes([app_byte]) * 0x1000
    return bytes(flash)


def test_nvs_partitions_reads_only_nvs_entries() -> None:
    assert nvs_partitions(image(0xAA, 0xBB)) == {
        "nvs": (NVS_OFFSET, NVS_SIZE),
        "nvs_key": (NVS_OFFSET + NVS_SIZE, NVS_SIZE),
    }


def test_refresh_takes_new_app_and_keeps_stored_nvs(tmp_path: Path) -> None:
    device = tmp_path / "device.bin"
    device.write_bytes(image(app_byte=0x11, nvs_byte=0xBB))
    fresh = tmp_path / "fresh.bin"
    fresh.write_bytes(image(app_byte=0x22, nvs_byte=0xFF))

    note = refresh_device_image(fresh, device)

    merged = device.read_bytes()
    assert merged[APP_OFFSET] == 0x22, "the rebuilt firmware must actually boot"
    assert merged[NVS_OFFSET] == 0xBB, "stored config must survive a rebuild"
    assert "nvs" in note


def test_refresh_without_prior_state_copies_the_image(tmp_path: Path) -> None:
    device = tmp_path / "device.bin"
    fresh = tmp_path / "fresh.bin"
    fresh.write_bytes(image(app_byte=0x22, nvs_byte=0xFF))

    assert refresh_device_image(fresh, device) == "new device"
    assert device.read_bytes() == fresh.read_bytes()
