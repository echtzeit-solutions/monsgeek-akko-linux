# Makefile for iot_driver_linux
# Wraps cargo commands and provides installation targets

PREFIX ?= /usr/local
UDEV_DIR ?= /etc/udev/rules.d
SYSTEMD_DIR ?= /etc/systemd/system

.PHONY: all build release clean install install-bin install-udev install-bpf install-systemd install-all uninstall test clippy fmt help

all: release

help:
	@echo "MonsGeek/Akko Linux Driver - Build and Install"
	@echo ""
	@echo "Build targets:"
	@echo "  make              - Build release binary"
	@echo "  make build        - Build debug binary"
	@echo "  make release      - Build release binary"
	@echo "  make clean        - Clean build artifacts"
	@echo ""
	@echo "Install targets (require sudo):"
	@echo "  make install      - Install driver + udev rules"
	@echo "  make install-all  - Install everything (driver + BPF + systemd)"
	@echo "  make install-bin  - Install driver binary only"
	@echo "  make install-udev - Install udev rules"
	@echo "  make install-bpf  - Build and install BPF battery support"
	@echo "  make install-systemd - Install systemd service for auto-load"
	@echo "  make uninstall    - Remove all installed files"
	@echo ""
	@echo "Development:"
	@echo "  make test         - Run tests"
	@echo "  make clippy       - Run clippy linter"
	@echo "  make fmt          - Format code"
	@echo ""
	@echo "Variables:"
	@echo "  PREFIX=$(PREFIX)"
	@echo "  UDEV_DIR=$(UDEV_DIR)"
	@echo "  SYSTEMD_DIR=$(SYSTEMD_DIR)"

build:
	cargo build

release:
	cargo build --release

clean:
	cargo clean
	$(MAKE) -C bpf clean

# Install udev rules (requires sudo)
install-udev:
	install -D -m 644 udev/99-monsgeek.rules $(UDEV_DIR)/99-monsgeek.rules
	udevadm control --reload-rules
	udevadm trigger --subsystem-match=hidraw --subsystem-match=input
	@echo "Udev rules installed. You may need to replug the device."

# Build and install Rust BPF loader + BPF program
install-bpf:
	$(MAKE) -C bpf all
	install -D -m 755 bpf/hid_battery_support/akko-loader-rs/target/release/akko-loader $(PREFIX)/bin/akko-loader
	install -D -m 644 bpf/hid_battery_support/akko-loader-rs/akko-ebpf.bpf.o $(PREFIX)/lib/akko/akko-ebpf.bpf.o
	@echo "BPF loader installed to $(PREFIX)/bin/akko-loader"
	@echo "Run with: sudo akko-loader status"

# Install systemd service for auto-start on device plug-in
install-systemd:
	sed 's|/usr/local|$(PREFIX)|g' systemd/akko-bpf-battery.service > /tmp/akko-bpf-battery.service
	install -D -m 644 /tmp/akko-bpf-battery.service $(SYSTEMD_DIR)/akko-bpf-battery.service
	rm /tmp/akko-bpf-battery.service
	systemctl daemon-reload
	@echo "Systemd service installed. BPF loader will auto-start on device plug-in."

# Install binary
install-bin: release
	install -D -m 755 target/release/iot_driver $(PREFIX)/bin/iot_driver

# Full install (driver + udev)
install: install-bin install-udev
	@echo "Installation complete."
	@echo "Run 'sudo make install-bpf install-systemd' for HID-BPF battery support."

# Full install with BPF battery support
install-all: install-bin install-udev install-bpf install-systemd
	@echo "Full installation complete (driver + BPF battery support)."

uninstall:
	-systemctl stop akko-bpf-battery.service 2>/dev/null
	-systemctl disable akko-bpf-battery.service 2>/dev/null
	rm -f $(SYSTEMD_DIR)/akko-bpf-battery.service
	systemctl daemon-reload
	rm -f $(PREFIX)/bin/iot_driver
	rm -f $(PREFIX)/bin/akko-loader
	rm -f $(PREFIX)/bin/akko-bpf-loader
	rm -rf $(PREFIX)/lib/akko
	rm -f $(UDEV_DIR)/99-monsgeek.rules
	udevadm control --reload-rules
	# Remove pinned BPF links
	-rm -rf /sys/fs/bpf/akko

# Development helpers
test:
	cargo test

clippy:
	cargo clippy

fmt:
	cargo fmt
