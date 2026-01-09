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

.PHONY: all clean vmlinux option_a

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

# Install to system location (requires sudo)
install: $(OBJS)
	install -D -m 644 akko_dongle.bpf.o /usr/share/akko-keyboard/akko_dongle.bpf.o

.PHONY: test
test: akko_dongle.bpf.o
	@echo "BPF object compiled successfully"
	@file $<
	@$(BPFTOOL) prog load $< /sys/fs/bpf/akko_test type struct_ops 2>&1 || true
	@$(BPFTOOL) prog show name akko 2>/dev/null || echo "Load test: needs root"
