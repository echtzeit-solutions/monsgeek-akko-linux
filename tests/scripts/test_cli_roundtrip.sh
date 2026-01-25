#!/bin/bash
# CLI Setting Roundtrip Tests
# Tests set commands by verifying the get commands reflect changes

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"
DRIVER_DIR="$PROJECT_DIR/iot_driver_linux"
IOT_DRIVER="${IOT_DRIVER:-$DRIVER_DIR/target/release/iot_driver}"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

log_pass() { echo -e "${GREEN}[PASS]${NC} $*"; }
log_fail() { echo -e "${RED}[FAIL]${NC} $*"; exit 1; }

if ! [[ -x "$IOT_DRIVER" ]]; then
    echo "Error: iot_driver not found at $IOT_DRIVER"
    exit 1
fi

# Check hardware
if ! lsusb | grep -q "3151:"; then
    echo "Error: MonsGeek keyboard not connected"
    exit 1
fi

echo "CLI Roundtrip Tests"
echo "==================="
echo ""

# Save original settings
echo "Saving original settings..."
ORIG_PROFILE=$($IOT_DRIVER profile 2>/dev/null | grep -oE '[0-3]' | head -1) || ORIG_PROFILE=0
ORIG_DEBOUNCE=$($IOT_DRIVER debounce 2>/dev/null | grep -oE '[0-9]+' | head -1) || ORIG_DEBOUNCE=5

cleanup() {
    echo ""
    echo "Restoring original settings..."
    $IOT_DRIVER set-profile "$ORIG_PROFILE" 2>/dev/null || true
    $IOT_DRIVER set-debounce "$ORIG_DEBOUNCE" 2>/dev/null || true
}
trap cleanup EXIT

# Test: Profile roundtrip
echo ""
echo "Test: Profile roundtrip"
for p in 0 1 2 3; do
    $IOT_DRIVER set-profile "$p"
    sleep 0.3
    got=$($IOT_DRIVER profile | grep -oE '[0-3]' | head -1)
    if [[ "$got" == "$p" ]]; then
        log_pass "Profile $p"
    else
        log_fail "Profile $p: expected $p, got $got"
    fi
done

# Test: Debounce roundtrip
echo ""
echo "Test: Debounce roundtrip"
for d in 0 5 10 25 50; do
    $IOT_DRIVER set-debounce "$d"
    sleep 0.3
    got=$($IOT_DRIVER debounce | grep -oE '[0-9]+' | head -1)
    if [[ "$got" == "$d" ]]; then
        log_pass "Debounce ${d}ms"
    else
        log_fail "Debounce ${d}ms: expected $d, got $got"
    fi
done

# Test: Polling rate roundtrip
echo ""
echo "Test: Polling rate roundtrip"
for rate in 125 250 500 1000; do
    $IOT_DRIVER set-rate "$rate"
    sleep 0.5
    got=$($IOT_DRIVER rate | grep -oE '[0-9]+' | head -1)
    if [[ "$got" == "$rate" ]]; then
        log_pass "Rate ${rate}Hz"
    else
        log_fail "Rate ${rate}Hz: expected $rate, got $got"
    fi
done

echo ""
echo "All roundtrip tests passed!"
