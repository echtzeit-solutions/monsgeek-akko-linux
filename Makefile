# MonsGeek/Akko Keyboard Linux Driver
# Top-level Makefile

CARGO ?= cargo
INSTALL ?= install
# DESTDIR stages the install under a directory instead of writing to the live system
# (deb/rpm/AUR set it to their package root). When it is non-empty, every recipe below
# skips its side-effects (udevadm/systemctl/desktop-db reloads, network DB refresh) so a
# packaging `make install-all DESTDIR=... PREFIX=/usr` needs no custom steps — the reloads
# belong in the package's postinst/postrm scripts.
DESTDIR ?=
PREFIX ?= /usr/local
BIN_DIR ?= $(PREFIX)/bin
LIB_DIR ?= $(PREFIX)/lib/akko
DATA_DIR ?= $(PREFIX)/share/akko
APP_DIR ?= $(PREFIX)/share/applications
# udev rules and systemd units live in fixed system search paths, NOT under PREFIX
# (systemd/udev never scan /usr/local). Packagers can override for split layouts.
UDEV_RULES_DIR ?= /usr/lib/udev/rules.d
SYSTEMD_DIR ?= /usr/lib/systemd/system

# Project directories
DRIVER_DIR := iot_driver_linux
BPF_DIR := akko-hid-bpf

# Binary names
DRIVER_BIN := iot_driver
JOYSTICK_BIN := monsgeek-joystick
LOADER_BIN := akko-loader

.PHONY: all driver driver-debug bpf clean clean-driver clean-bpf \
        install install-driver install-udev install-desktop install-bpf install-systemd install-all \
        uninstall uninstall-driver uninstall-bpf \
        test check fmt help \
        install-tray uninstall-tray run-tray \
        install-dev-sudoers uninstall-dev-sudoers \
        update-device-db update-device-db-full add-vendor-driver install-data uninstall-data \
        test-integration test-cli test-bpf test-all

# Tray app directory
TRAY_DIR := plasma-tray
TRAY_INSTALL_DIR := $(PREFIX)/share/akko-keyboard/tray
AUTOSTART_DIR := $(HOME)/.config/autostart

# Default target
all: driver

# ============================================================================
# Build Targets
# ============================================================================

## Build driver release binary
driver:
	cd $(DRIVER_DIR) && $(CARGO) build --release

## Build driver debug binary
driver-debug:
	cd $(DRIVER_DIR) && $(CARGO) build

## Build BPF loader (akko-ebpf requires nightly + special target)
bpf:
	cd $(BPF_DIR) && $(CARGO) build -p akko-loader --release

## Build BPF eBPF program (requires nightly toolchain)
bpf-ebpf:
	cd $(BPF_DIR)/akko-ebpf && RUSTFLAGS="-C debuginfo=2 -C link-arg=--btf" \
		cargo +nightly build --release -Z build-std=core --target bpfel-unknown-none

## Run tests
test:
	cd $(DRIVER_DIR) && $(CARGO) test --workspace --features firmware-api

## Run clippy lints
check:
	cd $(DRIVER_DIR) && $(CARGO) clippy --workspace --features firmware-api -- -D warnings

## Check formatting (for CI)
fmt-check:
	cd $(DRIVER_DIR) && $(CARGO) fmt --all --check

## Format code
fmt:
	cd $(DRIVER_DIR) && $(CARGO) fmt --all

## Clean all build artifacts
clean: clean-driver clean-bpf

clean-driver:
	cd $(DRIVER_DIR) && $(CARGO) clean

clean-bpf:
	cd $(BPF_DIR) && $(CARGO) clean

# ============================================================================
# Install Targets (require sudo, run 'make driver' first as regular user)
# ============================================================================

## Install driver + joystick binaries (must run 'make driver' first)
install-driver:
	@test -f $(DRIVER_DIR)/target/release/$(DRIVER_BIN) || \
		{ echo "Error: Binary not found. Run 'make driver' first (as regular user)."; exit 1; }
	$(INSTALL) -D -m 755 $(DRIVER_DIR)/target/release/$(DRIVER_BIN) $(DESTDIR)$(BIN_DIR)/$(DRIVER_BIN)
	@echo "Installed $(DRIVER_BIN) to $(BIN_DIR)"
	@# monsgeek-joystick is a workspace member built by the same `make driver`.
	@if [ -f $(DRIVER_DIR)/target/release/$(JOYSTICK_BIN) ]; then \
		$(INSTALL) -D -m 755 $(DRIVER_DIR)/target/release/$(JOYSTICK_BIN) $(DESTDIR)$(BIN_DIR)/$(JOYSTICK_BIN); \
		echo "Installed $(JOYSTICK_BIN) to $(BIN_DIR)"; \
	fi

## Install udev rules
install-udev:
	@echo "Installing udev rules..."
	$(INSTALL) -D -m 644 udev/99-monsgeek.rules $(DESTDIR)$(UDEV_RULES_DIR)/99-monsgeek.rules
	@if [ -z "$(DESTDIR)" ]; then \
		udevadm control --reload-rules; \
		udevadm trigger --subsystem-match=hidraw --subsystem-match=input; \
		echo "Udev rules installed. You may need to reconnect your keyboard."; \
	else \
		echo "Staged udev rules under DESTDIR; reload with 'udevadm control --reload-rules' in postinst."; \
	fi

## Install BPF loader binary + eBPF object (must run 'make bpf bpf-ebpf' first)
install-bpf:
	@test -f $(BPF_DIR)/target/release/$(LOADER_BIN) || \
		{ echo "Error: Loader not found. Run 'make bpf' first (as regular user)."; exit 1; }
	$(INSTALL) -D -m 755 $(BPF_DIR)/target/release/$(LOADER_BIN) $(DESTDIR)$(BIN_DIR)/$(LOADER_BIN)
	@echo "Installed $(LOADER_BIN) to $(BIN_DIR)"
	@if [ -f $(BPF_DIR)/akko-ebpf/target/bpfel-unknown-none/release/akko-ebpf ]; then \
		$(INSTALL) -D -m 644 $(BPF_DIR)/akko-ebpf/target/bpfel-unknown-none/release/akko-ebpf \
			$(DESTDIR)$(LIB_DIR)/akko-ebpf.bpf.o; \
		echo "Installed eBPF object to $(LIB_DIR)"; \
	else \
		echo "WARNING: eBPF object missing — run 'make bpf-ebpf' first, or battery-over-BPF will not work." >&2; \
	fi

## Install systemd service for BPF auto-load
install-systemd:
	@echo "Installing systemd service..."
	$(INSTALL) -d $(DESTDIR)$(SYSTEMD_DIR)
	sed 's|/usr/local|$(PREFIX)|g' systemd/akko-bpf-battery.service \
		> $(DESTDIR)$(SYSTEMD_DIR)/akko-bpf-battery.service
	chmod 644 $(DESTDIR)$(SYSTEMD_DIR)/akko-bpf-battery.service
	@if [ -z "$(DESTDIR)" ]; then \
		systemctl daemon-reload; \
		echo "Systemd service installed. BPF loader will auto-start on device plug-in."; \
	else \
		echo "Staged systemd unit under DESTDIR; reload with 'systemctl daemon-reload' in postinst."; \
	fi

## Install the XDG desktop entry. Its app id (solutions.echtzeit.akko_keyboard_driver) is what
## the ScreenCast portal needs to register the app: KDE then shows a name in the
## screen-share picker/tray, and the saved restore token is namespaced to it so
## screen-reactive mode stops re-prompting. Without this file the portal rejects
## the app id with "App info not found".
install-desktop:
	$(INSTALL) -D -m 644 $(DRIVER_DIR)/packaging/solutions.echtzeit.akko_keyboard_driver.desktop \
		$(DESTDIR)$(APP_DIR)/solutions.echtzeit.akko_keyboard_driver.desktop
	@if [ -z "$(DESTDIR)" ]; then \
		update-desktop-database $(APP_DIR) 2>/dev/null || true; \
	fi
	@echo "Installed desktop entry to $(APP_DIR)"

## Install driver + udev rules (standard install)
install: install-driver install-udev install-desktop install-data
	@echo ""
	@echo "Installation complete!"
	@echo "Run '$(DRIVER_BIN) --help' to get started."
	@echo ""
	@echo "For HID-BPF battery support (2.4GHz dongle), run:"
	@echo "  make bpf && sudo make install-bpf install-systemd"

## Install everything (driver + BPF + systemd)
install-all: install-driver install-udev install-desktop install-data install-bpf install-systemd
	@echo ""
	@echo "Full installation complete!"

## Uninstall driver
uninstall-driver:
	rm -f $(DESTDIR)$(BIN_DIR)/$(DRIVER_BIN)
	rm -f $(DESTDIR)$(BIN_DIR)/$(JOYSTICK_BIN)
	rm -f $(DESTDIR)$(UDEV_RULES_DIR)/99-monsgeek.rules
	rm -f $(DESTDIR)$(APP_DIR)/solutions.echtzeit.akko_keyboard_driver.desktop
	@if [ -z "$(DESTDIR)" ]; then \
		update-desktop-database $(APP_DIR) 2>/dev/null || true; \
		udevadm control --reload-rules; \
	fi
	@echo "Driver uninstalled."

## Uninstall BPF components
uninstall-bpf:
	@if [ -z "$(DESTDIR)" ]; then \
		systemctl stop akko-bpf-battery.service 2>/dev/null || true; \
		systemctl disable akko-bpf-battery.service 2>/dev/null || true; \
	fi
	rm -f $(DESTDIR)$(SYSTEMD_DIR)/akko-bpf-battery.service
	rm -f $(DESTDIR)$(BIN_DIR)/$(LOADER_BIN)
	rm -rf $(DESTDIR)$(LIB_DIR)
	@if [ -z "$(DESTDIR)" ]; then \
		systemctl daemon-reload; \
		rm -rf /sys/fs/bpf/akko; \
	fi
	@echo "BPF components uninstalled."

## Uninstall everything
uninstall: uninstall-driver uninstall-bpf
	@echo "All components uninstalled."

# ============================================================================
# Development Targets
# ============================================================================

# Paths for dev sudoers (must be absolute)
DEV_WRAPPER := $(CURDIR)/scripts/bpf-dev-wrapper.sh
SUDOERS_FILE := /etc/sudoers.d/akko-bpf-dev
# Use SUDO_USER when run with sudo, otherwise fall back to USER
DEV_USER := $(if $(SUDO_USER),$(SUDO_USER),$(USER))

## Install sudoers rule for passwordless BPF dev operations (requires sudo)
## This allows the pre-push hook to verify BPF without password prompts
install-dev-sudoers:
	@echo "Installing development sudoers rule for $(DEV_USER)..."
	@echo "# Allow passwordless BPF operations for development" > /tmp/akko-bpf-dev
	@echo "# Installed by: make install-dev-sudoers" >> /tmp/akko-bpf-dev
	@echo "$(DEV_USER) ALL=(ALL) NOPASSWD: $(DEV_WRAPPER)" >> /tmp/akko-bpf-dev
	$(INSTALL) -m 440 /tmp/akko-bpf-dev $(SUDOERS_FILE)
	@rm /tmp/akko-bpf-dev
	@echo "Sudoers rule installed at $(SUDOERS_FILE)"
	@echo "You can now run: sudo $(DEV_WRAPPER) verify|load|unload"

## Remove development sudoers rule
uninstall-dev-sudoers:
	rm -f $(SUDOERS_FILE)
	@echo "Development sudoers rule removed."

# ============================================================================
# Tray App Targets (KDE Plasma system tray)
# ============================================================================

## Run tray app (for testing)
run-tray:
	cd $(TRAY_DIR) && python3 main.py

## Install tray app
install-tray:
	@echo "Installing tray app..."
	$(INSTALL) -d $(TRAY_INSTALL_DIR)
	$(INSTALL) -m 644 $(TRAY_DIR)/main.py $(TRAY_INSTALL_DIR)/main.py
	$(INSTALL) -m 644 $(TRAY_DIR)/controller.py $(TRAY_INSTALL_DIR)/controller.py
	$(INSTALL) -m 644 $(TRAY_DIR)/main.qml $(TRAY_INSTALL_DIR)/main.qml
	$(INSTALL) -d $(AUTOSTART_DIR)
	$(INSTALL) -m 644 $(TRAY_DIR)/akko-tray.desktop $(AUTOSTART_DIR)/akko-tray.desktop
	@echo "Tray app installed. It will start automatically on login."

## Uninstall tray app
uninstall-tray:
	rm -rf $(TRAY_INSTALL_DIR)
	rm -f $(AUTOSTART_DIR)/akko-tray.desktop
	@echo "Tray app uninstalled."

# ============================================================================
# Device Database Targets
# ============================================================================

## Update device database from app.monsgeek.com
update-device-db:
	@echo "Updating device database from webapp..."
	./scripts/update-device-db.sh

## Update device database including Electron driver (slower)
update-device-db-full:
	@echo "Updating device database (webapp + electron)..."
	./scripts/update-device-db.sh --electron

## Add a locally downloaded vendor driver as a database source, then rebuild.
## Rebranded vendor drivers (WOMIER, Epomaker, ...) are often the only source of data
## for a recently released keyboard.
##   make add-vendor-driver DRIVER=~/Downloads/Womier_SK75.rar VENDOR=womier
add-vendor-driver:
	@test -n "$(DRIVER)" || { echo "Usage: make add-vendor-driver DRIVER=<installer> VENDOR=<tag>"; exit 1; }
	@test -n "$(VENDOR)" || { echo "Usage: make add-vendor-driver DRIVER=<installer> VENDOR=<tag>"; exit 1; }
	./driver_extract/download-and-extract.sh --input "$(DRIVER)" --vendor "$(VENDOR)"
	./scripts/update-device-db.sh --electron --vendor "$(VENDOR)"

## Install device data files. Interactive for a live install: offers to refresh the
## database from the vendor driver first (default Yes), falling back to committed assets.
## When DESTDIR is set (packaging) the refresh is forced off — committed assets install
## with no prompt and no network. Set SKIP_REFRESH=1 to force that for a live install too.
install-data:
	@DATA_DIR="$(DESTDIR)$(DATA_DIR)" INSTALL="$(INSTALL)" \
		SKIP_REFRESH="$(if $(DESTDIR),1,$(SKIP_REFRESH))" \
		./scripts/install-data.sh

## Uninstall device data files
uninstall-data:
	rm -f $(DESTDIR)$(DATA_DIR)/devices.json
	rm -f $(DESTDIR)$(DATA_DIR)/device_matrices.json
	-rmdir $(DESTDIR)$(DATA_DIR) 2>/dev/null || true
	@echo "Device data uninstalled."

# ============================================================================
# Integration Tests (require hardware)
# ============================================================================

# Test directory (anchored to Makefile location)
TEST_DIR := $(CURDIR)/tests

## Run all integration tests (build + cli + transport)
test-integration:
	$(TEST_DIR)/run_tests.sh all

## Run build tests only
test-build:
	$(TEST_DIR)/run_tests.sh build

## Run CLI tests (requires hardware)
test-cli:
	$(TEST_DIR)/run_tests.sh cli

## Run transport tests (requires hardware)
test-transport:
	$(TEST_DIR)/run_tests.sh transport

## Run BPF battery tests (requires root + hardware)
test-bpf:
	$(TEST_DIR)/run_tests.sh bpf

## Run full test suite including root tests
test-all:
	sudo $(TEST_DIR)/run_tests.sh all

## Run CLI setting roundtrip tests
test-roundtrip:
	$(TEST_DIR)/scripts/test_cli_roundtrip.sh

## Run TUI basic tests
test-tui:
	$(TEST_DIR)/scripts/test_tui_basic.sh

# ============================================================================
# Help
# ============================================================================

help:
	@echo "MonsGeek/Akko Keyboard Linux Driver"
	@echo ""
	@echo "Quick start:"
	@echo "  make driver && sudo make install"
	@echo ""
	@echo "Build targets (run as regular user):"
	@echo "  driver       Build driver release binary (default)"
	@echo "  driver-debug Build driver debug binary"
	@echo "  bpf          Build BPF loader"
	@echo "  bpf-ebpf     Build BPF eBPF program (requires nightly)"
	@echo "  test         Run tests"
	@echo "  check        Run clippy lints"
	@echo "  fmt          Format code"
	@echo "  clean        Clean all build artifacts"
	@echo ""
	@echo "Install targets (run with sudo, after building):"
	@echo "  install         Install driver + udev rules"
	@echo "  install-all     Install everything (driver + BPF + systemd)"
	@echo "  install-driver  Install driver binary only"
	@echo "  install-udev    Install udev rules only"
	@echo "  install-desktop Install XDG desktop entry (needed for screen-share app name)"
	@echo "  install-bpf     Install BPF loader + eBPF object"
	@echo "  install-systemd Install systemd service for BPF auto-load"
	@echo "  uninstall       Remove all installed files"
	@echo ""
	@echo "Packaging (deb/rpm/AUR — no custom install steps needed):"
	@echo "  make driver bpf bpf-ebpf"
	@echo "  make install-all DESTDIR=\"\$$pkgdir\" PREFIX=/usr"
	@echo "  # DESTDIR staged: udevadm/systemctl/desktop-db reloads and the DB refresh"
	@echo "  # are skipped — run them from the package's postinst/postrm instead."
	@echo ""
	@echo "Tray app targets (KDE Plasma system tray):"
	@echo "  run-tray       Run tray app for testing"
	@echo "  install-tray   Install tray app + autostart"
	@echo "  uninstall-tray Remove tray app"
	@echo ""
	@echo "Development targets:"
	@echo "  install-dev-sudoers   Allow passwordless BPF verify/load (for pre-push hook)"
	@echo "  uninstall-dev-sudoers Remove dev sudoers rule"
	@echo ""
	@echo "Device database targets:"
	@echo "  update-device-db      Fetch and extract device data from webapp"
	@echo "  update-device-db-full Also include Electron driver (slower)"
	@echo "  add-vendor-driver     Add a local vendor driver as a source (DRIVER=<file> VENDOR=<tag>)"
	@echo "  install-data          Install device data to $(DATA_DIR) (prompts to refresh first; SKIP_REFRESH=1 to skip)"
	@echo "  uninstall-data        Remove installed device data"
	@echo ""
	@echo "Integration test targets:"
	@echo "  test-integration Run build + cli tests"
	@echo "  test-build       Run build tests only"
	@echo "  test-cli         Run CLI tests (requires hardware)"
	@echo "  test-bpf         Run BPF tests (requires sudo + hardware)"
	@echo "  test-all         Run full suite including root tests"
	@echo "  test-roundtrip   Run CLI setting roundtrip tests"
	@echo "  test-tui         Run TUI basic tests"
	@echo ""
	@echo "Variables (override on the command line):"
	@echo "  DESTDIR=$(DESTDIR)   (staging root for packaging; empty = live install)"
	@echo "  PREFIX=$(PREFIX)"
	@echo "  BIN_DIR=$(BIN_DIR)"
	@echo "  LIB_DIR=$(LIB_DIR)"
	@echo "  DATA_DIR=$(DATA_DIR)"
	@echo "  APP_DIR=$(APP_DIR)"
	@echo "  UDEV_RULES_DIR=$(UDEV_RULES_DIR)"
	@echo "  SYSTEMD_DIR=$(SYSTEMD_DIR)"
