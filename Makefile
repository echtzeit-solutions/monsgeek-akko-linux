# MonsGeek/Akko Keyboard Linux Driver
# Top-level Makefile

CARGO ?= cargo
INSTALL ?= install
PREFIX ?= /usr/local
UDEV_RULES_DIR ?= /etc/udev/rules.d
SYSTEMD_DIR ?= /etc/systemd/system
BIN_DIR ?= $(PREFIX)/bin
LIB_DIR ?= $(PREFIX)/lib/akko

# Project directories
DRIVER_DIR := iot_driver_linux
BPF_DIR := akko-hid-bpf

# Binary names
DRIVER_BIN := iot_driver
LOADER_BIN := akko-loader

.PHONY: all driver driver-debug bpf clean clean-driver clean-bpf \
        install install-driver install-udev install-bpf install-systemd install-all \
        uninstall uninstall-driver uninstall-bpf \
        test check fmt help \
        install-tray uninstall-tray run-tray

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
	cd $(BPF_DIR) && cargo +nightly build -p akko-ebpf --release \
		-Z build-std=core --target bpfel-unknown-none

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

## Install driver binary (must run 'make driver' first)
install-driver:
	@test -f $(DRIVER_DIR)/target/release/$(DRIVER_BIN) || \
		{ echo "Error: Binary not found. Run 'make driver' first (as regular user)."; exit 1; }
	$(INSTALL) -D -m 755 $(DRIVER_DIR)/target/release/$(DRIVER_BIN) $(BIN_DIR)/$(DRIVER_BIN)
	@echo "Installed $(DRIVER_BIN) to $(BIN_DIR)"

## Install udev rules
install-udev:
	@echo "Installing udev rules..."
	$(INSTALL) -D -m 644 udev/99-monsgeek.rules $(UDEV_RULES_DIR)/99-monsgeek.rules
	udevadm control --reload-rules
	udevadm trigger --subsystem-match=hidraw --subsystem-match=input
	@echo "Udev rules installed. You may need to reconnect your keyboard."

## Install BPF loader binary (must run 'make bpf' first)
install-bpf:
	@test -f $(BPF_DIR)/target/release/$(LOADER_BIN) || \
		{ echo "Error: Loader not found. Run 'make bpf' first (as regular user)."; exit 1; }
	$(INSTALL) -D -m 755 $(BPF_DIR)/target/release/$(LOADER_BIN) $(BIN_DIR)/$(LOADER_BIN)
	@echo "Installed $(LOADER_BIN) to $(BIN_DIR)"
	@# Install pre-built eBPF object if available
	@if [ -f $(BPF_DIR)/akko-ebpf/target/bpfel-unknown-none/release/akko-ebpf ]; then \
		$(INSTALL) -D -m 644 $(BPF_DIR)/akko-ebpf/target/bpfel-unknown-none/release/akko-ebpf \
			$(LIB_DIR)/akko-ebpf.bpf.o; \
		echo "Installed eBPF object to $(LIB_DIR)"; \
	fi

## Install systemd service for BPF auto-load
install-systemd:
	@echo "Installing systemd service..."
	sed 's|/usr/local|$(PREFIX)|g' systemd/akko-bpf-battery.service > /tmp/akko-bpf-battery.service
	$(INSTALL) -D -m 644 /tmp/akko-bpf-battery.service $(SYSTEMD_DIR)/akko-bpf-battery.service
	rm /tmp/akko-bpf-battery.service
	systemctl daemon-reload
	@echo "Systemd service installed. BPF loader will auto-start on device plug-in."

## Install driver + udev rules (standard install)
install: install-driver install-udev
	@echo ""
	@echo "Installation complete!"
	@echo "Run '$(DRIVER_BIN) --help' to get started."
	@echo ""
	@echo "For HID-BPF battery support (2.4GHz dongle), run:"
	@echo "  make bpf && sudo make install-bpf install-systemd"

## Install everything (driver + BPF + systemd)
install-all: install-driver install-udev install-bpf install-systemd
	@echo ""
	@echo "Full installation complete!"

## Uninstall driver
uninstall-driver:
	rm -f $(BIN_DIR)/$(DRIVER_BIN)
	rm -f $(UDEV_RULES_DIR)/99-monsgeek.rules
	udevadm control --reload-rules
	@echo "Driver uninstalled."

## Uninstall BPF components
uninstall-bpf:
	-systemctl stop akko-bpf-battery.service 2>/dev/null
	-systemctl disable akko-bpf-battery.service 2>/dev/null
	rm -f $(SYSTEMD_DIR)/akko-bpf-battery.service
	systemctl daemon-reload
	rm -f $(BIN_DIR)/$(LOADER_BIN)
	rm -rf $(LIB_DIR)
	-rm -rf /sys/fs/bpf/akko
	@echo "BPF components uninstalled."

## Uninstall everything
uninstall: uninstall-driver uninstall-bpf
	@echo "All components uninstalled."

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
	@echo "  install-bpf     Install BPF loader"
	@echo "  install-systemd Install systemd service for BPF auto-load"
	@echo "  uninstall       Remove all installed files"
	@echo ""
	@echo "Tray app targets (KDE Plasma system tray):"
	@echo "  run-tray       Run tray app for testing"
	@echo "  install-tray   Install tray app + autostart"
	@echo "  uninstall-tray Remove tray app"
	@echo ""
	@echo "Variables:"
	@echo "  PREFIX=$(PREFIX)"
	@echo "  BIN_DIR=$(BIN_DIR)"
	@echo "  UDEV_RULES_DIR=$(UDEV_RULES_DIR)"
