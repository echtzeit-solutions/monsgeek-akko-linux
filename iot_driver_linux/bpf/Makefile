# HID-BPF Makefile for Akko dongle battery integration

CLANG ?= clang
CC ?= gcc
BPFTOOL ?= bpftool

# Compiler flags for BPF
BPF_CFLAGS := -g -O2 -target bpf
BPF_CFLAGS += -D__TARGET_ARCH_x86
BPF_CFLAGS += -I.
BPF_CFLAGS += -I/usr/include

# Compiler flags for loader
LOADER_CFLAGS := -g -O2 -Wall
LOADER_LDFLAGS := -lbpf

# Source files - only the working version by default
SRCS := akko_dongle.bpf.c
OBJS := $(SRCS:.bpf.c=.bpf.o)

.PHONY: all clean vmlinux option_a option_b option_b_wq akko-loader akko-loader-rs akko-ebpf

all: $(OBJS) loader

# Generate vmlinux.h from kernel BTF (run once)
vmlinux:
	$(BPFTOOL) btf dump file /sys/kernel/btf/vmlinux format c > vmlinux.h

%.bpf.o: %.bpf.c vmlinux.h hid_bpf.h hid_bpf_helpers.h
	$(CLANG) $(BPF_CFLAGS) -c $< -o $@

akko_dongle.skel.h: akko_dongle.bpf.o
	$(BPFTOOL) gen skeleton $< > $@

loader: loader.c akko_dongle.skel.h
	$(CC) $(LOADER_CFLAGS) -o $@ loader.c $(LOADER_LDFLAGS)

clean:
	rm -f $(OBJS) loader akko_dongle.skel.h

# Build Option A (keyboard inject) - run from hid_battery_support/option_a_keyboard_inject/
option_a:
	@echo "Building Option A in hid_battery_support/option_a_keyboard_inject/"
	cd hid_battery_support/option_a_keyboard_inject && \
	$(CLANG) $(BPF_CFLAGS) -I../common -c akko_keyboard_battery.bpf.c -o akko_keyboard_battery.bpf.o && \
	$(BPFTOOL) gen skeleton akko_keyboard_battery.bpf.o > akko_keyboard_battery.skel.h && \
	$(CC) $(LOADER_CFLAGS) -o loader_kb loader_kb.c $(LOADER_LDFLAGS)

# Build Option B (vendor interface with periodic F7)
option_b:
	@echo "Building Option B in hid_battery_support/option_b_bidirectional/"
	cd hid_battery_support/option_b_bidirectional && \
	$(CLANG) $(BPF_CFLAGS) -I../common -c akko_bidirectional.bpf.c -o akko_bidirectional.bpf.o && \
	$(BPFTOOL) gen skeleton akko_bidirectional.bpf.o > akko_bidirectional.skel.h && \
	$(CC) $(LOADER_CFLAGS) -o loader_bidir loader_bidir.c $(LOADER_LDFLAGS)

# Build Option B WQ (experimental bpf_wq version)
option_b_wq:
	@echo "Building Option B WQ (experimental) in hid_battery_support/option_b_wq_experimental/"
	cd hid_battery_support/option_b_wq_experimental && \
	$(CLANG) $(BPF_CFLAGS) -I../common -c akko_wq.bpf.c -o akko_wq.bpf.o && \
	$(BPFTOOL) gen skeleton akko_wq.bpf.o > akko_wq.skel.h && \
	$(CC) $(LOADER_CFLAGS) -o loader_wq loader_wq.c $(LOADER_LDFLAGS)

# Build unified loader with all strategies
# Note: Requires skeletons to exist (run option_a, option_b, option_b_wq first if needed)
akko-loader:
	@echo "Building unified akko-loader..."
	@echo "Note: Requires skeleton headers. Run 'make option_a option_b option_b_wq' first if missing."
	cd hid_battery_support && \
	$(CC) $(LOADER_CFLAGS) -I. \
		-o akko-loader akko_loader.c $(LOADER_LDFLAGS)
	@echo "Built: hid_battery_support/akko-loader"
	@echo ""
	@echo "Usage: sudo ./hid_battery_support/akko-loader -s <keyboard|vendor|wq> [-v] [-d]"

# Build Rust-based akko-loader (uses local Aya fork)
akko-loader-rs:
	@echo "Building Rust akko-loader..."
	cd hid_battery_support/akko-loader-rs && cargo build --release
	@echo "Built: hid_battery_support/akko-loader-rs/target/release/akko-loader"

# Build Rust BPF program (experimental)
# Requires: rustup +nightly, bpf-linker
akko-ebpf:
	@echo "Building Rust BPF program (requires nightly + bpf-linker)..."
	cd ../akko-ebpf && cargo +nightly build --release
	cp ../akko-ebpf/target/bpfel-unknown-none/release/akko-ebpf \
		hid_battery_support/akko-loader-rs/akko-ebpf.bpf.o
	@echo "Built: hid_battery_support/akko-loader-rs/akko-ebpf.bpf.o"

# Install to system location (requires sudo)
install: $(OBJS)
	install -D -m 644 akko_dongle.bpf.o /usr/share/akko-keyboard/akko_dongle.bpf.o

.PHONY: test
test: akko_dongle.bpf.o
	@echo "BPF object compiled successfully"
	@file $<
	@$(BPFTOOL) prog load $< /sys/fs/bpf/akko_test type struct_ops 2>&1 || true
	@$(BPFTOOL) prog show name akko 2>/dev/null || echo "Load test: needs root"
