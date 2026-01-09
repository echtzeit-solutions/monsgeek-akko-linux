#!/bin/bash
# Load Akko dongle HID-BPF program for battery integration
# This script should be owned by root with setuid: chown root:root && chmod 4755

set -e

BPF_OBJ="/home/florian/src-misc/monsgeek-m1-v5-tmr/iot_driver_linux/bpf/akko_dongle.bpf.o"

# Check if BPF object exists
if [ ! -f "$BPF_OBJ" ]; then
    echo "Error: BPF object not found at $BPF_OBJ"
    echo "Run 'make' in iot_driver_linux/bpf/ first"
    exit 1
fi

# Check if dongle is connected
if ! ls /sys/bus/hid/devices/ 2>/dev/null | grep -q "3151:5038"; then
    echo "Error: Akko dongle (3151:5038) not found"
    exit 1
fi

# Check if already loaded
if bpftool struct_ops list 2>/dev/null | grep -q "akko_dongle"; then
    echo "BPF program already loaded"
    exit 0
fi

echo "Loading HID-BPF program..."
bpftool struct_ops register "$BPF_OBJ"

# Wait for kernel to reprobe device
sleep 1

# Check if power_supply was created
if ls /sys/class/power_supply/ 2>/dev/null | grep -q "hid-.*3151.*5038"; then
    echo "Success! power_supply entry created"
    ls /sys/class/power_supply/ | grep hid
else
    echo "Warning: power_supply not created - check dmesg"
fi
