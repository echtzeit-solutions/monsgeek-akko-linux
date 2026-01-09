# Makefile for iot_driver_linux
# Wraps cargo commands and provides installation targets

PREFIX ?= /usr/local
UDEV_DIR ?= /etc/udev/rules.d
SYSTEMD_DIR ?= /etc/systemd/system

.PHONY: all build release clean install install-bin install-udev install-bpf install-systemd uninstall test clippy fmt

all: build

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

# Build and install BPF loader (Option A - recommended)
install-bpf:
	$(MAKE) -C bpf option_a
	install -D -m 755 bpf/hid_battery_support/option_a_keyboard_inject/loader_kb $(PREFIX)/bin/akko-bpf-loader
	@echo "BPF loader installed to $(PREFIX)/bin/akko-bpf-loader"
	@echo "Run with: sudo akko-bpf-loader"

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

# Full install
install: install-bin install-udev
	@echo "Installation complete."
	@echo "Run 'sudo make install-bpf install-systemd' for HID-BPF battery support."

uninstall:
	-systemctl stop akko-bpf-battery.service 2>/dev/null
	-systemctl disable akko-bpf-battery.service 2>/dev/null
	rm -f $(SYSTEMD_DIR)/akko-bpf-battery.service
	systemctl daemon-reload
	rm -f $(PREFIX)/bin/iot_driver
	rm -f $(PREFIX)/bin/akko-bpf-loader
	rm -f $(UDEV_DIR)/99-monsgeek.rules
	udevadm control --reload-rules

# Development helpers
test:
	cargo test

clippy:
	cargo clippy

fmt:
	cargo fmt
