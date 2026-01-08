# MonsGeek/Akko Keyboard Linux Driver
# Top-level Makefile

CARGO ?= cargo
INSTALL ?= install
UDEV_RULES_DIR ?= /etc/udev/rules.d
BIN_DIR ?= /usr/local/bin
BPF_DIR ?= /usr/share/akko-keyboard

# Rust project directory
RUST_DIR := iot_driver_linux
BPF_SRC_DIR := $(RUST_DIR)/bpf

# Binary name
BINARY := iot_driver

.PHONY: all build build-debug clean install install-udev install-bin uninstall \
        bpf install-bpf clean-bpf install-tray uninstall-tray run-tray help test check

# Tray app directory
TRAY_DIR := plasma-tray
TRAY_INSTALL_DIR := /usr/share/akko-keyboard/tray
AUTOSTART_DIR := $(HOME)/.config/autostart

# Default target
all: build

# ============================================================================
# Rust Build Targets
# ============================================================================

## Build release binary
build:
	cd $(RUST_DIR) && $(CARGO) build --release

## Build debug binary
build-debug:
	cd $(RUST_DIR) && $(CARGO) build

## Run tests
test:
	cd $(RUST_DIR) && $(CARGO) test

## Run clippy lints
check:
	cd $(RUST_DIR) && $(CARGO) clippy --release

## Clean build artifacts
clean:
	cd $(RUST_DIR) && $(CARGO) clean

# ============================================================================
# Installation Targets (require sudo)
# ============================================================================

## Install udev rules for HID device access
install-udev:
	@echo "Installing udev rules..."
	$(INSTALL) -D -m 644 $(RUST_DIR)/udev/99-monsgeek.rules $(UDEV_RULES_DIR)/99-monsgeek.rules
	udevadm control --reload-rules
	udevadm trigger
	@echo "Udev rules installed. You may need to reconnect your keyboard."

## Install binary to /usr/local/bin
install-bin: build
	@echo "Installing binary..."
	$(INSTALL) -D -m 755 $(RUST_DIR)/target/release/$(BINARY) $(BIN_DIR)/$(BINARY)
	@echo "Installed $(BINARY) to $(BIN_DIR)"

## Install everything (binary + udev rules)
install: install-bin install-udev
	@echo ""
	@echo "Installation complete!"
	@echo "Run '$(BINARY) --help' to get started."

## Uninstall everything
uninstall:
	@echo "Removing installed files..."
	rm -f $(BIN_DIR)/$(BINARY)
	rm -f $(UDEV_RULES_DIR)/99-monsgeek.rules
	rm -rf $(BPF_DIR)
	udevadm control --reload-rules
	@echo "Uninstalled."

# ============================================================================
# BPF Targets (optional, for battery integration via HID-BPF)
# ============================================================================

## Build BPF objects
bpf:
	@if [ -d "$(BPF_SRC_DIR)" ]; then \
		echo "Building BPF objects..."; \
		$(MAKE) -C $(BPF_SRC_DIR); \
	else \
		echo "BPF source directory not found: $(BPF_SRC_DIR)"; \
		exit 1; \
	fi

## Install BPF objects
install-bpf: bpf
	@echo "Installing BPF objects..."
	$(INSTALL) -D -m 644 $(BPF_SRC_DIR)/akko_dongle.bpf.o $(BPF_DIR)/akko_dongle.bpf.o
	@echo "BPF object installed to $(BPF_DIR)"

## Clean BPF build artifacts
clean-bpf:
	@if [ -d "$(BPF_SRC_DIR)" ]; then \
		$(MAKE) -C $(BPF_SRC_DIR) clean; \
	fi

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
	@echo "Run 'make run-tray' to test now."

## Uninstall tray app
uninstall-tray:
	@echo "Removing tray app..."
	rm -rf $(TRAY_INSTALL_DIR)
	rm -f $(AUTOSTART_DIR)/akko-tray.desktop
	@echo "Tray app uninstalled."

# ============================================================================
# Help
# ============================================================================

## Show this help
help:
	@echo "MonsGeek/Akko Keyboard Linux Driver"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Build targets:"
	@echo "  build        Build release binary (default)"
	@echo "  build-debug  Build debug binary"
	@echo "  test         Run tests"
	@echo "  check        Run clippy lints"
	@echo "  clean        Clean build artifacts"
	@echo ""
	@echo "Install targets (require sudo):"
	@echo "  install      Install binary + udev rules"
	@echo "  install-bin  Install binary only"
	@echo "  install-udev Install udev rules only"
	@echo "  uninstall    Remove all installed files"
	@echo ""
	@echo "BPF targets (optional, for HID-BPF battery integration):"
	@echo "  bpf          Build BPF objects"
	@echo "  install-bpf  Install BPF objects"
	@echo "  clean-bpf    Clean BPF build artifacts"
	@echo ""
	@echo "Tray app targets (KDE Plasma system tray):"
	@echo "  run-tray       Run tray app for testing"
	@echo "  install-tray   Install tray app + autostart"
	@echo "  uninstall-tray Remove tray app"
	@echo ""
	@echo "Variables:"
	@echo "  BIN_DIR=$(BIN_DIR)"
	@echo "  UDEV_RULES_DIR=$(UDEV_RULES_DIR)"
	@echo "  BPF_DIR=$(BPF_DIR)"
