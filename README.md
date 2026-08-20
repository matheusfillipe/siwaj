# siwaj

Should i wear a jacket: ESP32-S3 e-paper weather device with a self-hosted web config page.

Development happens against `docs/research.md` (hardware facts, decisions, plan). Quality gate and all commands live in the `Makefile`.

```
make install      pnpm + uv sync, pre-commit hooks
make quality      full host gate (rust, web, tools)
make fix          autofix pass
make provision    push .env secrets to the device over USB serial
```

Firmware targets (`make firmware-build`, `firmware-flash`, `qemu-smoke`) need `espup` and `cargo-espflash`.
