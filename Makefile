PNPM := pnpm --dir web
UV := uv run --project tools

.DEFAULT_GOAL := quality
.PHONY: install fix precommit core-format core-format-check core-lint core-lint-fix core-test core-ts core-snapshots core-preview demo web-typecheck web-bundle web-watch tools-format tools-format-check tools-lint tools-test firmware-build firmware-image firmware-flash firmware-monitor provision qemu-install qemu-image qemu-smoke qemu-run qemu-stop qemu-provision build build-release build-qemu test-e2e quality clean

install:
	$(PNPM) install
	uv sync --project tools
	uv run --project tools pre-commit install

precommit: fix

fix: core-format core-lint-fix tools-format

# --- core (rust workspace; tests also regenerate web/src/generated TS) ---

core-format:
	cargo fmt --all

core-format-check:
	cargo fmt --all --check

core-lint:
	cargo clippy --workspace --all-targets -- -D warnings

core-lint-fix:
	cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged -- -D warnings || cargo clippy --workspace --all-targets -- -D warnings

core-test:
	cargo test --workspace

core-ts: core-test

# regenerates render fixtures after an intentional layout/icon change
core-snapshots:
	cd core && UPDATE_SNAPSHOTS=1 cargo test --test render_test

# interactive window cycling through the four garments (needs SDL2: brew install sdl2)
# LIBRARY_PATH: sdl2-sys does not query pkg-config, so point clang at Homebrew's libdir
core-preview:
	LIBRARY_PATH=/opt/homebrew/lib cargo run -p siwaj-core --example preview --features preview

# build fresh and open the live emulated 200x200 e-paper window
demo: core-preview

# --- web (vanilla TS; needs `make core-ts` first for generated bindings) ---

web-typecheck:
	$(PNPM) run typecheck

web-bundle:
	$(PNPM) run build
	cp web/styles.css web/dist/styles.css
	gzip -9 -kf web/dist/app.js web/dist/styles.css

web-watch:
	$(PNPM) run watch

# --- tools (python, isolated; quality bar is ruff + pytest only) ---

tools-format:
	cd tools && uv run ruff format src tests && uv run ruff check --fix src tests

tools-format-check:
	cd tools && uv run ruff format --check src tests

tools-lint:
	cd tools && uv run ruff check src tests

tools-test:
	cd tools && uv run pytest

# --- firmware (needs espup xtensa toolchain; not part of host quality gate) ---

QEMU_VERSION := 9.2.2
QEMU_DATE := 20260417
QEMU_URL := https://github.com/espressif/qemu/releases/download/esp-develop-$(QEMU_VERSION)-$(QEMU_DATE)/qemu-xtensa-softmmu-esp_develop_$(QEMU_VERSION)_$(QEMU_DATE)-aarch64-apple-darwin.tar.xz
QEMU_BIN := tools/bin/qemu/bin/qemu-system-xtensa
FLASH_IMAGE := firmware/target/siwaj-flash.bin

firmware-build: web-bundle
	cd firmware && cargo build --release

# host gate + both firmware targets: everything that must compile before a commit
build: quality firmware-build qemu-image

build-release: firmware-build

firmware-image: firmware-build
	cd firmware && cargo espflash save-image --release --chip esp32s3 --merge --skip-padding target/siwaj-flash.bin
	truncate -s 8M firmware/target/siwaj-flash.bin

firmware-flash: firmware-build
	cd firmware && ESP_IDF_SDKCONFIG_DEFAULTS="$$PWD/sdkconfig.defaults;$$PWD/sdkconfig.secure" cargo espflash flash --release --monitor

firmware-monitor:
	cd firmware && cargo espflash monitor

qemu-install:
	@mkdir -p tools/bin
	@test -x $(QEMU_BIN) || { curl -sL "$(QEMU_URL)" | tar -xJ -C tools/bin --strip-components=1; }
	$(QEMU_BIN) --version | head -1

# The esp32 QEMU machine emulates OpenETH (the esp32s3 machine hangs on the
# macOS arm64 prebuilt, espressif/qemu#99). Isolated target dir + sdkconfig so
# the esp32s3 device build is never disturbed.
QEMU_IMAGE := firmware/target-esp32/siwaj-smoke.bin

build-qemu: qemu-image

qemu-image: qemu-install web-bundle
	cd firmware && MCU=esp32 ESP_IDF_SDKCONFIG=$$PWD/sdkconfig.esp32 cargo espflash save-image --release --chip esp32 --target xtensa-esp32-espidf --target-dir target-esp32 --flash-size 4mb --merge --skip-padding target-esp32/siwaj-smoke.bin
	truncate -s 4M $(QEMU_IMAGE)

# CI-shaped: boot, wait for the banner, terminate
qemu-smoke: qemu-image
	$(UV) python -m siwaj_tools.qemu smoke $(QEMU_IMAGE)

# dev device: boot detached; web ui on http://127.0.0.1:47652
qemu-run: qemu-image
	$(UV) python -m siwaj_tools.qemu serve $(QEMU_IMAGE) --expect 'config mode: serving'

qemu-stop:
	$(UV) python -m siwaj_tools.qemu stop

# push .env secrets into the emulated device's serial REPL
qemu-provision:
	$(UV) python -m siwaj_tools.provision --port socket://127.0.0.1:47653

# push .env secrets into the real device over USB serial
provision:
	$(UV) python -m siwaj_tools.provision

# automated end-to-end: boots the emulated device, provisions secrets from
# .env (when present), saves a config, verifies persistence and geocoding
test-e2e: qemu-image
	$(UV) python -m siwaj_tools.e2e

# --- gates ---

quality: core-format-check core-lint core-test web-typecheck web-bundle tools-format-check tools-lint tools-test
	@echo "quality gate passed"

clean:
	cargo clean
	rm -rf web/dist
	cd tools && uv run ruff clean 2>/dev/null || true