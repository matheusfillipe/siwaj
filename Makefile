PNPM := pnpm --dir web
UV := uv run --project tools

.DEFAULT_GOAL := help
.PHONY: help install fix precommit \
        check check-firmware \
        core-format core-format-check core-lint core-lint-fix core-test core-ts core-snapshots core-preview demo \
        web-typecheck web-bundle web-watch \
        tools-format tools-format-check tools-lint tools-test \
        firmware-partitions build build-qemu firmware-image firmware-flash firmware-monitor \
        qemu-install qemu-image qemu-smoke qemu-run qemu-display qemu-stop qemu-reset \
        qemu-charge qemu-unplug qemu-sleep qemu-button qemu-provision provision test-e2e \
        ci-host-checks ci-emulator-tests ci-local ci-local-plan \
        all clean

help: ## list available targets
	@grep -E '^[a-zA-Z0-9_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-18s %s\n", $$1, $$2}'

install: ## install dependencies (pnpm, uv) and git hooks
	$(PNPM) install
	uv sync --project tools
	uv run --project tools pre-commit install

fix: ## autofix formatting and lint issues (cargo fmt/clippy, ruff)
	cargo fmt --all
	cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged -- -D warnings || cargo clippy --workspace --all-targets -- -D warnings
	cd tools && uv run ruff format src tests && uv run ruff check --fix src tests

precommit: fix ## hook entry: same as fix

# --- checks (verify, never produce artifacts) ---

check: core-format-check core-lint core-test web-typecheck web-bundle tools-format-check tools-lint tools-test ## run all host checks (the pre-commit gate)

check-firmware: firmware-partitions ## run clippy -D warnings on both firmware targets (esp32s3 device + esp32 emulator)
	cd firmware && $(FW_ENV) cargo clippy --release -- -D warnings
	cd firmware && MCU=esp32 $(FW_ENV_QEMU) cargo clippy --release --target xtensa-esp32-espidf --target-dir target-esp32 -- -D warnings

core-format:
	cargo fmt --all

core-format-check:
	cargo fmt --all --check

core-lint:
	cargo clippy --workspace --all-targets -- -D warnings

core-test:
	cargo test --workspace

core-ts: core-test ## regenerate the TypeScript bindings (side effect of the tests)

core-snapshots: ## regenerate render fixtures after an intentional layout/icon change
	cd core && UPDATE_SNAPSHOTS=1 cargo test --test render_test

core-preview: ## open a window cycling the four garments (needs SDL2: brew install sdl2)
	LIBRARY_PATH=/opt/homebrew/lib cargo run -p siwaj-core --example preview --features preview

demo: core-preview ## alias for core-preview

web-typecheck:
	$(PNPM) run typecheck

web-bundle:
	$(PNPM) run build
	cp web/styles.css web/dist/styles.css
	gzip -9 -kf web/dist/app.js web/dist/styles.css

web-watch:
	$(PNPM) run watch

tools-format:
	cd tools && uv run ruff format src tests && uv run ruff check --fix src tests

tools-format-check:
	cd tools && uv run ruff format --check src tests

tools-lint:
	cd tools && uv run ruff check src tests

tools-test:
	cd tools && uv run pytest

# --- firmware builds (produce artifacts, never run checks) ---

# esp-idf resolves the custom partition CSV against its generated project dir,
# so the filename must be absolute; generated per checkout, never committed
firmware-partitions:
	printf 'CONFIG_PARTITION_TABLE_CUSTOM_FILENAME="%s/partitions.csv"\n' "$$PWD/firmware" > firmware/sdkconfig.partitions
	printf 'CONFIG_PARTITION_TABLE_CUSTOM_FILENAME="%s/partitions.esp32.csv"\n' "$$PWD/firmware" > firmware/sdkconfig.partitions-esp32

FW_ENV := ESP_IDF_SDKCONFIG_DEFAULTS="$$PWD/sdkconfig.defaults;$$PWD/sdkconfig.partitions" ESP_IDF_SDKCONFIG="$$PWD/sdkconfig"
FW_ENV_QEMU := ESP_IDF_SDKCONFIG_DEFAULTS="$$PWD/sdkconfig.defaults;$$PWD/sdkconfig.partitions-esp32;$$PWD/sdkconfig.esp32" ESP_IDF_SDKCONFIG="$$PWD/target-esp32/sdkconfig"

build: web-bundle firmware-partitions ## build the esp32s3 device firmware (release)
	cd firmware && $(FW_ENV) cargo build --release

build-qemu: qemu-image ## build the esp32 emulator flash image

firmware-image: build ## build and merge the device flash image (8MB)
	cd firmware && $(FW_ENV) cargo espflash save-image --release --chip esp32s3 --merge --skip-padding target/siwaj-flash.bin
	truncate -s 8M firmware/target/siwaj-flash.bin

firmware-flash: build ## flash the device (needs espup + cargo-espflash; encrypted-NVS sdkconfig applied)
	cd firmware && ESP_IDF_SDKCONFIG_DEFAULTS="$$PWD/sdkconfig.defaults;$$PWD/sdkconfig.partitions;$$PWD/sdkconfig.secure" cargo espflash flash --release --monitor

firmware-monitor:
	cd firmware && cargo espflash monitor

# --- emulated device ---

QEMU_VERSION := 9.2.2
QEMU_DATE := 20260417
QEMU_TAG := esp-develop-$(QEMU_VERSION)-$(QEMU_DATE)
# release assets are per-OS/arch; map this machine onto espressif's naming
ifeq ($(shell uname -s),Darwin)
ifeq ($(shell uname -m),arm64)
QEMU_ARCH := aarch64-apple-darwin
else
QEMU_ARCH := x86_64-apple-darwin
endif
else
ifeq ($(shell uname -m),aarch64)
QEMU_ARCH := aarch64-linux-gnu
else
QEMU_ARCH := x86_64-linux-gnu
endif
endif
QEMU_ASSET := qemu-xtensa-softmmu-esp_develop_$(QEMU_VERSION)_$(QEMU_DATE)-$(QEMU_ARCH)
QEMU_URL := https://github.com/espressif/qemu/releases/download/$(QEMU_TAG)/$(QEMU_ASSET).tar.xz
QEMU_BIN := tools/bin/qemu/bin/qemu-system-xtensa

qemu-install:
	@mkdir -p tools/bin
	@test -x $(QEMU_BIN) || { curl -sL "$(QEMU_URL)" | tar -xJ -C tools/bin; }
	$(QEMU_BIN) --version | head -1

# The esp32 QEMU machine emulates OpenETH (the esp32s3 machine hangs on the
# macOS arm64 prebuilt, espressif/qemu#99). Isolated target dir + sdkconfig so
# the esp32s3 device build is never disturbed.
QEMU_IMAGE := firmware/target-esp32/siwaj-smoke.bin

qemu-image: qemu-install web-bundle firmware-partitions
	cd firmware && MCU=esp32 $(FW_ENV_QEMU) cargo espflash save-image --release --chip esp32 --target xtensa-esp32-espidf --target-dir target-esp32 --flash-size 4mb --merge --skip-padding target-esp32/siwaj-smoke.bin
	truncate -s 4M $(QEMU_IMAGE)

qemu-smoke: qemu-image ## boot the emulator image once and check the boot banner (CI check)
	$(UV) python -m siwaj.qemu smoke $(QEMU_IMAGE)

qemu-run: qemu-image ## boot a detached emulated device; web ui on http://127.0.0.1:47652
	$(UV) python -m siwaj.qemu serve $(QEMU_IMAGE)

qemu-display: ## open an SDL window mirroring the emulated e-paper live (run qemu-run first)
	LIBRARY_PATH=/opt/homebrew/lib cargo run -p siwaj-core --example mirror --features preview

qemu-stop:
	$(UV) python -m siwaj.qemu stop

qemu-reset: qemu-stop ## wipe the emulated device's stored config and secrets (next qemu-run is a first setup)
	rm -f firmware/target-esp32/qemu-dev/device.bin

qemu-charge: ## tell the emulated device the charger is plugged in
	$(UV) python -m siwaj.qemu sim --charging on

qemu-unplug: ## tell the emulated device it is back on battery
	$(UV) python -m siwaj.qemu sim --charging off

qemu-sleep: ## end config mode now; the web ui goes down like the sleeping device
	$(UV) python -m siwaj.qemu sleep

qemu-button: ## press the emulated BOOT button; config mode and the web ui come back
	$(UV) python -m siwaj.qemu button

qemu-provision: ## push .env secrets into the emulated device over its serial REPL
	$(UV) python -m siwaj.provision --port socket://127.0.0.1:47653

provision: ## push .env secrets into the real device over USB serial
	$(UV) python -m siwaj.provision

test-e2e: qemu-image ## run the automated end-to-end suite on the emulated device
	$(UV) python -m siwaj.e2e

# --- CI (the Makefile is the single source of truth for what CI runs) ---

ci-host-checks: check ## everything the CI host job runs (fmt, clippy, tests, typecheck, bundle, ruff, pytest)

ci-emulator-tests: check-firmware qemu-smoke test-e2e ## everything the CI emulator job runs (firmware clippy, boot smoke, device e2e)

ci-local: ## run the real GitHub workflow locally with act (needs Docker)
	act push -W .github/workflows/ci.yml

ci-local-plan: ## parse the workflow with act and list what would run, without executing
	act push -n -W .github/workflows/ci.yml

# --- maintenance ---

all: build build-qemu ## build every firmware artifact

clean: ## remove host build artifacts (cargo target dirs, web bundle)
	cargo clean
	rm -rf web/dist

clean-firmware: ## remove firmware build artifacts (~2.5G; next build recompiles)
	rm -rf firmware/target firmware/target-esp32

clean-sdk: ## remove the esp-idf SDK install in .embuild (~4.1G; next build re-downloads)
	rm -rf firmware/.embuild
