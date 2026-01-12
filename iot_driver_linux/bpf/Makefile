# HID-BPF Makefile for Akko/MonsGeek keyboard battery integration
#
# This builds the Rust-based BPF program and loader.
# The BPF program hooks into the keyboard's HID interface to:
# - Fix Report ID quirk (Report 5 returns as Report 0)
# - On-demand F7 battery refresh when UPower queries the battery

.PHONY: all clean akko-loader akko-ebpf install

all: akko-ebpf akko-loader

# Build Rust BPF program
# Requires: rustup +nightly, bpf-linker
akko-ebpf:
	@echo "Building Rust BPF program (requires nightly + bpf-linker)..."
	cd ../akko-ebpf && cargo +nightly build --release
	cp ../akko-ebpf/target/bpfel-unknown-none/release/akko-ebpf \
		hid_battery_support/akko-loader-rs/akko-ebpf.bpf.o
	@echo "Built: hid_battery_support/akko-loader-rs/akko-ebpf.bpf.o"

# Build Rust loader (uses local Aya fork)
akko-loader:
	@echo "Building Rust akko-loader..."
	cd hid_battery_support/akko-loader-rs && cargo build --release
	@echo "Built: hid_battery_support/akko-loader-rs/target/release/akko-loader"

clean:
	cd hid_battery_support/akko-loader-rs && cargo clean
	cd ../akko-ebpf && cargo clean
	rm -f hid_battery_support/akko-loader-rs/akko-ebpf.bpf.o

# Install to system (requires sudo) - see main Makefile for full install
install: akko-ebpf akko-loader
	install -D -m 755 hid_battery_support/akko-loader-rs/target/release/akko-loader /usr/local/bin/akko-loader
	install -D -m 644 hid_battery_support/akko-loader-rs/akko-ebpf.bpf.o /usr/local/lib/akko/akko-ebpf.bpf.o
	@echo "Installed akko-loader and BPF object"
