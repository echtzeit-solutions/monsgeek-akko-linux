#!/bin/bash
# Rebind Akko dongle HID device to trigger descriptor re-parse
# Usage: sudo ./akko-rebind.sh

set -e

DEVICE="0003:3151:5038.00BD"
DRIVER_PATH="/sys/bus/hid/drivers/hid-generic"

# Find the actual device (suffix may change after replug)
ACTUAL_DEVICE=$(ls /sys/bus/hid/devices/ | grep "0003:3151:5038" | tail -1)
if [ -z "$ACTUAL_DEVICE" ]; then
    echo "Error: Akko dongle not found"
    exit 1
fi

echo "Rebinding device: $ACTUAL_DEVICE"

# Unbind from hid-generic
echo "$ACTUAL_DEVICE" > "$DRIVER_PATH/unbind" 2>/dev/null || true
sleep 0.5

# Rebind to hid-generic
echo "$ACTUAL_DEVICE" > "$DRIVER_PATH/bind"
sleep 0.5

echo "Device rebound successfully"

# Check if input device was created
if [ -d "/sys/bus/hid/devices/$ACTUAL_DEVICE/input" ]; then
    echo "Input device created!"
else
    echo "Warning: No input device created"
fi

# Check for power_supply
PS_PATH=$(find /sys/class/power_supply -name "*3151*" 2>/dev/null | head -1)
if [ -n "$PS_PATH" ]; then
    echo "Power supply found: $PS_PATH"
else
    echo "Warning: No power_supply entry found"
fi
