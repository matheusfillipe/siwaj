# AGENTS.md

Instructions for AI agents working in this repo. Read before editing.

## What this is
Firmware and web config app for the siwaj ("should i wear a jacket") e-paper device: an ESP32-S3-ePaper-1.54 (V2) board that deep sleeps, fetches OpenWeather One Call 3.0, and shows shirt/pullover/jacket plus rain risk on a 200x200 e-paper display.

## Where things live
- Research, hardware facts, decisions, full plan: `docs/research.md`
- Shared contract (config types, thresholds, decision logic, TS binding export): `core/`
- Firmware (esp-idf std Rust, xtensa esp32s3): `firmware/` (excluded from the host workspace; needs `espup`)
- Web config page (vanilla TS, no framework, single minified bundle): `web/`
- Host tooling (serial provisioner, QEMU smoke runner): `tools/` (python, isolated, ruff+pytest bar only)
- The `Makefile` is the single canonical interface for all checks; CI and pre-commit both call it.

## Stack
- Rust workspace for `core/` (host-testable, `ts-rs` generates `web/src/generated/` during `cargo test`)
- `firmware/`: esp-idf-svc/esp-idf-hal, embedded-graphics + epd-waveshare (epd1in54_v2), NVS (encrypted)
- `web/`: TypeScript + esbuild via pnpm, zero runtime dependencies, bundle embedded into the firmware binary
- `tools/`: uv-managed python 3.12, pyserial

All web/tooling commands go through the `Makefile`; do not call pnpm/npm/uv/cargo directly.

## Commands (Makefile is SSoT)
- `make install` pnpm + uv sync plus pre-commit hooks
- `make quality` the full host gate: cargo fmt/clippy/test, web typecheck + bundle, tools ruff + pytest
- `make fix` autofix pass (cargo fmt/clippy --fix, ruff format)
- `make firmware-build` / `firmware-flash` / `firmware-monitor` device targets (need espup + cargo-espflash)
- `make provision` push `.env` secrets into the device NVS over USB serial
- `make qemu-smoke` boot the release ELF under the Espressif QEMU fork

## Before considering work complete
1. Run `make quality`.
2. Fix all failures.
3. Do not weaken or remove quality checks to make them pass.
4. Do not leave TODO stubs in `core/` (shared contract must stay exact); firmware TODOs are milestone-tracked in `docs/research.md`.

## Secrets
`OPENWEATHER_API_KEY` lives only in `.env` (gitignored) and in the device's encrypted NVS, provisioned via `make provision`. Never embed it in firmware, web assets, tests, or docs.

## Code style
- Rust: no comments unless a non-obvious why; fail loudly; prefer `siwaj-core` types everywhere over redefining shapes.
- TS: strict mode, `noUncheckedIndexedAccess`, hand-rolled DOM code, no frameworks, no runtime deps.
- The JSON wire format is camelCase (serde rename_all on the Rust side, generated TS matches).
- Python tools: imports at top, no bare `except`, ruff-clean; quality bar is lower by design, do not let tooling concerns leak into core/firmware/web.

## Conventions
- `revision` in `Config` is the user-config version: the device bumps it on every accepted POST `/api/config`. Web clients sync localStorage against it (client newer than unconfigured device means re-flash happened: client pushes).
- `schemaVersion` is the payload shape version; `siwaj-core::migrate` is the single gate. Bump it only with a migration path there.
- Clothing decision: `feels_like < low -> jacket`, `< mid -> pullover`, `< high -> shirt`, else t-shirt. Rain risk: minutely precip >= 0.1mm within the hour OR hourly pop >= configured threshold.
- Board is V2 (8MB flash / 8MB octal PSRAM). GPIO17 (battery rail) must be high with `gpio_hold` through deep sleep or the board never wakes on battery.
- Prose (docs, commits): declarative, terse, no em dashes.

## Hygiene
Never commit secrets, build artifacts (`web/dist`, `web/src/generated`, `target/`), or the `.rev-ok` marker. `.gitignore` covers them; keep it that way.
