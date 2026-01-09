#!/bin/bash
# Battery monitor wrapper - run with sudo or add to sudoers
# Updates test_power module parameters for UPower integration

DRIVER="/home/florian/src-misc/monsgeek-m1-v5-tmr/iot_driver_linux/target/release/iot_driver"

if [ ! -f "$DRIVER" ]; then
    echo "Error: iot_driver not found at $DRIVER"
    exit 1
fi

# Check if test_power is loaded
if [ ! -d /sys/module/test_power ]; then
    echo "Loading test_power module..."
    modprobe test_power
fi

exec "$DRIVER" battery-monitor "$@"
