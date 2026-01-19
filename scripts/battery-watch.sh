#!/bin/bash
# Battery monitor script for charger testing
# Prints full vendor response every second to observe byte changes
# Usage: ./battery-watch.sh [interval_seconds]

INTERVAL=${1:-1}
DRIVER="iot_driver"

# Check if driver is available
if ! command -v "$DRIVER" &> /dev/null; then
    # Try local build
    if [ -x "./target/release/iot_driver" ]; then
        DRIVER="./target/release/iot_driver"
    elif [ -x "./target/debug/iot_driver" ]; then
        DRIVER="./target/debug/iot_driver"
    else
        echo "Error: iot_driver not found. Build with 'cargo build --release' first."
        exit 1
    fi
fi

echo "Battery Monitor - Press Ctrl+C to stop"
echo "Polling interval: ${INTERVAL}s"
echo "Driver: $DRIVER"
echo ""
echo "Watching for changes in vendor response bytes..."
echo "Try plugging/unplugging the charger to see if any bytes change."
echo ""
echo "========================================"

$DRIVER battery --hex --watch "$INTERVAL"
