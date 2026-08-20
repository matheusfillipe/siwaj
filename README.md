# siwaj

Should i wear a jacket: ESP32-S3 e-paper weather device with a self-hosted web config page.

Development happens against `docs/research.md` (hardware facts, decisions, plan). Quality gate and all commands live in the `Makefile`.

```
make help           list all targets
make install        pnpm + uv sync, pre-commit hooks
make check          host checks (rust, web, tools)
make check-firmware firmware clippy, both targets
make test-e2e       end-to-end suite on the emulated device
make ci-local       replay the CI workflow locally with act
make provision      push .env secrets to the device over USB serial
```

Firmware targets (`make firmware-build`, `firmware-flash`, `qemu-smoke`) need `espup` and `cargo-espflash`.
