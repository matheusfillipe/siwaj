# AGENTS.md

Instructions for AI agents working in this repo. Read before editing.

## What this is
Firmware and web config app for the siwaj ("should i wear a jacket") e-paper device: an ESP32-S3-ePaper-1.54 (V2) board that deep sleeps, fetches OpenWeather One Call 4.0, and shows jacket/pullover/shirt plus rain risk on a 200x200 e-paper display.

## Where things live
- Shared contract (config types, thresholds, decision logic, TS binding export): `core/`
- Firmware (esp-idf std Rust, xtensa esp32s3): `firmware/` (excluded from the host workspace; needs `espup`)
- Web config page (vanilla TS, no framework, single minified bundle): `web/`
- Host tooling (serial provisioner, QEMU smoke runner): `tools/` (python, isolated, ruff+pytest bar only)
- Simulation harness (SDL display mirror, render preview): `sim/` (host-only Rust, SDL2 behind the `sdl` feature so CI skips it). Nothing that only exists to drive the emulator belongs in `core/`, which ships to the device.
- The `Makefile` is the single canonical interface for all checks; CI and pre-commit both call it.

## Stack
- Rust workspace for `core/` and `sim/` (host-testable, `ts-rs` generates `web/src/generated/` during `cargo test`)
- `firmware/`: esp-idf-svc/esp-idf-hal, embedded-graphics + epd-waveshare (epd1in54_v2), NVS (encrypted)
- `web/`: TypeScript + esbuild via pnpm, zero runtime dependencies, bundle embedded into the firmware binary
- `tools/`: uv-managed python 3.12, pyserial

All web/tooling commands go through the `Makefile`; do not call pnpm/npm/uv/cargo directly.

## Commands (Makefile is SSoT)
`make help` lists every target. The ones that matter:

- `make install` pnpm + uv sync plus pre-commit hooks
- `make check` all host checks (cargo fmt/clippy/test, web typecheck + bundle, tools ruff + pytest); the pre-commit gate
- `make check-firmware` clippy `-D warnings` on both firmware targets (esp32s3 + QEMU esp32)
- `make build` / `make build-qemu` build the device firmware / the emulator flash image (build only, no checks)
- `make test-e2e` automated end-to-end on the emulated device (boot, provision, config flow, geocode, persistence)
- `make qemu-smoke` CI-shaped boot check; `make qemu-run`/`qemu-stop`/`qemu-provision` interactive emulated device (web ui on http://127.0.0.1:47652, `.env` auto-loaded, state persisted in `firmware/target-esp32/qemu-dev/device.bin`); `make qemu-display` opens an SDL window mirroring the emulated e-paper live
- `make ci-host-checks` / `make ci-emulator-tests` exactly what each CI job runs (host checks; firmware clippy + boot smoke + device e2e); `make ci-local` replays the workflow locally with act (`ci-local-plan` for a dry run)
- `make demo` interactive emulated e-paper window
- `make firmware-flash` / `firmware-monitor` / `provision` real-device targets (need espup + cargo-espflash)

All commands go through the Makefile. Never call cargo/pnpm/uv/qemu binaries directly, and never build or redirect output by hand; add or extend a make target instead.

## Before considering work complete
1. Run `make check` (host) and `make check-firmware` (firmware).
2. Fix all failures.
3. Do not weaken or remove checks to make them pass.
4. Do not leave TODO stubs in `core/` (shared contract must stay exact).
5. Emulator affordances stay out of the device build. `strings firmware/target/xtensa-esp32s3-espidf/release/siwaj` must not mention `api/sim`, `api/frame`, or the serial button and sleep replies.

## Secrets
`OPENWEATHER_API_KEY` lives only in `.env` (gitignored) and in the device's encrypted NVS, provisioned via `make provision`. Never embed it in firmware, web assets, tests, or docs.

## Code style
- Rust: no comments unless a non-obvious why; fail loudly; prefer `siwaj-core` types everywhere over redefining shapes.
- TS: strict mode, `noUncheckedIndexedAccess`, hand-rolled DOM code, no frameworks, no runtime deps.
- The JSON wire format is camelCase (serde rename_all on the Rust side, generated TS matches). Inbound envelopes carry `deny_unknown_fields`: a payload from another shape is refused whole rather than half-applied.
- Python tools: imports at top, no bare `except`, ruff-clean; quality bar is lower by design, do not let tooling concerns leak into core/firmware/web.

## Rust standards
- Shared logic lives in `core/` with host tests; firmware modules stay thin hardware shells. If a change is testable off-device, it belongs in core.
- One error protocol per crate: `ConfigError` in core (the contract), `anyhow` everywhere in firmware. No `Result<_, String>` seams; `Context`/`bail` at the source, `e.to_string()` only at the wire.
- Presence vs corruption are different states: `Result<Option<T>>` for persistence reads; never collapse "unconfigured", "unreadable", and "invalid" into one `None`.
- Closed domains are enums, not strings (`SecretKey`, not `&str` keys; `TimeOfDay`, not `(u8, u8)`). Wire envelopes are typed in core with ts-rs, never hand-mirrored JSON in the web client.
- Sentinel values are banned: a failed lookup is an error, not `(0.0, 0.0)`; NVS/JSON/migration failures carry a stage-identifying message.
- Every `unsafe` block carries a `// SAFETY:` comment saying why it is sound. GPIO steals and deep-sleep register calls included.
- `make check-firmware` clippy `-D warnings` on both firmware targets is part of every change, like `make check` for host code. Fix warnings, never silence them.
- Render changes ship with updated snapshot fixtures (`make core-snapshots`) when intentional; a changed diff without a fixture update is a bug.
- Firmware wake paths must always terminate in deep sleep; an error path that leaves the radio on is a battery bug, not a style issue. The s3 weather cycle runs under a 120s watchdog thread that force-sleeps a hung cycle, and wifi bring-up plus HTTP carry their own deadlines.
- New public API surface on `Board`/`Store`/`Secrets` starts private; widen only with a consumer in the same change.
- Dependencies: firmware declares only what firmware names (shared crates enter via the `siwaj-core` path dependency); unused deps are removed when the compiler stops naming them.

## Conventions
- `revision` in `Config` is the user-config version: the device bumps it on every accepted POST `/api/config`; the real device (esp32s3) then restarts into weather mode (3s grace so the response flushes), while the esp32 emulator build just keeps serving with the new config. Web clients sync localStorage against it (client newer than unconfigured device means re-flash happened: client pushes). The UI never shows revisions.
- `schemaVersion` is the payload shape version; `siwaj-core::migrate` is the single gate and accepts only the current version. A bump makes stored configs unreadable, which drops the device into config mode for a fresh setup: that is the migration story, so change the shape freely and bump.
- Clothing decision: `feels_like < low -> jacket`, `< high -> pullover`, else shirt. Two thresholds, three garments, both inside `THRESHOLD_MIN_C..=THRESHOLD_MAX_C`; the config page draws its axis to those same bounds and `Config::validate` is what keeps the two in step. Rain risk: minutely precip >= 0.1mm within the hour OR hourly pop >= configured threshold; it draws the streaks under the cloud and never changes the garment.
- Geocoding returns `region` and `country` alongside the fix; they ride in `Location` so the page can show which Springfield it resolved to.
- Board is V2 (8MB flash / 8MB octal PSRAM). GPIO17 (battery rail) must be high with `gpio_hold` through deep sleep or the board never wakes on battery.
- esp32 build = QEMU-only variant (config-mode + OpenETH); esp32s3 build = the real device. Target-specific code is `#[cfg]`-gated; keep both warning-free (`make build`).
- One Call 4.0 serves each resolution from its own endpoint, so a weather cycle spends two requests: `timeline/1h` for feels-like, next-hour pop and the zone offset, `timeline/1min` for the precipitation trace. `core::weather::parse_hourly` then `merge_minutely` fold both into one `Snapshot`; `timeline/1h`'s `data[0]` is the current hour bucket, so feels-like is that hour's value. Budget two calls per wake against the account's daily cap.
- `GET /api/weather` runs a live One Call fetch with the stored config: device-side debugging aid and the e2e's weather probe. A 401 from upstream means the account's "One Call by Call" plan is not activated yet, not a firmware bug.
- The emulator has no panel, so the esp32 build runs a frame loop instead: the same fetch and view the s3 cycle draws, published into memory. `GET /api/frame` hands out those 5000 bytes (packed 1bpp, `render::FRAME_BYTES`) with `X-Siwaj-Frame: live|offline`, and `make qemu-display` draws them, and puts the bench controls on keys: c flips the charger over `/api/sim`, b and s drive the serial button and sleep, so the window is the whole workbench. Both are `#[cfg(esp32)]`, so the frame loop, the endpoint, and the cached frame exist only in the emulator build.
- Config mode serves only while it is being used: `siwaj_core::CONFIG_MODE_IDLE` after the last request the server is dropped, and the s3 deep sleeps into the next weather cycle. Requests count as activity from `serve_gz`/`serve_json`, so a new handler stays awake by default; `GET /api/status` opts out through `write_json` because a page watching the clock must not stop it reaching zero. Editing sends nothing on its own, so the page refetches its config while someone works.
- The emulator cannot deep sleep under QEMU, so it parks on the serial REPL instead: `make qemu-sleep` ends the window now and `make qemu-button` plays the BOOT button the s3 reads from GPIO0. Serial answers when HTTP does not, which is what makes it the channel that can wake a device that stopped serving. Both commands are `#[cfg(esp32)]`.
- `POST /api/sim` (`#[cfg(esp32)]`, `SimInputs` in `firmware/src/frame.rs`) drives the sense lines QEMU has no hardware for: `make qemu-charge` / `make qemu-unplug` flip the charging bolt. A flipped switch redraws from the weather already in hand, so it costs no upstream call. The device reads `Board::battery` instead and never takes a sense reading from the network.
- Charging on the real board is inferred from rail voltage at or above `Battery::EXTERNAL_POWER`, which only a charger can hold. A charge that has not lifted the rail that far still reads as running on battery; a proper answer needs a VBUS or charger STAT line.
- A cycle whose fetch fails still renders, as the offline face; `Config::next_fetch_delay` then retries on `OFFLINE_RETRY` instead of the full refresh interval, so a device recovers once upstream does. The face travels with the frame because an offline frame is a well-formed frame and would otherwise pass every shape check.
- NVS keys are capped at 15 chars: `Secrets` maps `.env` names to short keys (`ow_key`, `wifi_ssid`, `wifi_pass`).
- QEMU writes device state into `firmware/target-esp32/qemu-dev/device.bin`; delete it to reset the emulated device (the e2e does this every run).
- Prose (docs, commits): declarative, terse, no em dashes.

## Hygiene
Never commit secrets, build artifacts (`web/dist`, `web/src/generated`, `target/`), or the `.rev-ok` marker. `.gitignore` covers them; keep it that way.
