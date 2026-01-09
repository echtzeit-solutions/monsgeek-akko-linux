#!/bin/bash
# Unload Akko dongle HID-BPF program
# This script should be owned by root with setuid: chown root:root && chmod 4755

set -e

# Check if loaded
if ! bpftool struct_ops list 2>/dev/null | grep -q "akko_dongle"; then
    echo "BPF program not loaded"
    exit 0
fi

echo "Unloading HID-BPF program..."
bpftool struct_ops unregister name akko_dongle

echo "Done"
