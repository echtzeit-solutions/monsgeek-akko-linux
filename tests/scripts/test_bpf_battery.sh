#!/bin/bash
# BPF Battery Driver Tests
# Tests the HID-BPF battery driver for 2.4GHz dongle
#
# Prerequisites:
#   - Kernel 6.12+ (HID-BPF struct_ops support)
#   - akko-loader built
#   - MonsGeek 2.4GHz dongle connected
#   - Root privileges

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"
BPF_DIR="$PROJECT_DIR/akko-hid-bpf"
AKKO_LOADER="${AKKO_LOADER:-$BPF_DIR/target/release/akko-loader}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_pass() { echo -e "${GREEN}[PASS]${NC} $*"; }
log_fail() { echo -e "${RED}[FAIL]${NC} $*"; }
log_info() { echo -e "${YELLOW}[INFO]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }

# Cleanup on exit
cleanup() {
    if [[ ${BPF_LOADED:-0} -eq 1 ]]; then
        log_info "Cleaning up BPF..."
        "$AKKO_LOADER" unload 2>/dev/null || true
    fi
}
trap cleanup EXIT

echo "BPF Battery Driver Tests"
echo "========================"
echo ""

# Check root
if [[ $EUID -ne 0 ]]; then
    echo "Error: This script requires root privileges"
    echo "Run with: sudo $0"
    exit 1
fi

# Check loader exists
if ! [[ -x "$AKKO_LOADER" ]]; then
    echo "Error: akko-loader not found at $AKKO_LOADER"
    echo "Build with: cd $PROJECT_DIR && make bpf"
    exit 1
fi

# Check kernel version
KERNEL=$(uname -r)
MAJOR=$(echo "$KERNEL" | cut -d. -f1)
MINOR=$(echo "$KERNEL" | cut -d. -f2)

log_info "Kernel: $KERNEL"

if [[ $MAJOR -lt 6 ]] || { [[ $MAJOR -eq 6 ]] && [[ $MINOR -lt 12 ]]; }; then
    echo "Error: Kernel $KERNEL does not support HID-BPF struct_ops"
    echo "Requires kernel 6.12+"
    exit 1
fi


# Check for dongle
if ! lsusb | grep -q "3151:5038"; then
    echo "Error: MonsGeek 2.4GHz dongle not detected (VID:PID 3151:5038)"
    echo "Connect the dongle and ensure keyboard is in 2.4GHz mode"
    exit 1
fi
log_pass "Dongle detected"

echo ""
echo "Running tests..."
echo ""

# E1: Load BPF
log_info "E1: Loading BPF..."
if "$AKKO_LOADER" load 2>&1; then
    log_pass "E1: BPF loaded"
    BPF_LOADED=1
else
    log_fail "E1: BPF load failed"
    echo ""
    echo "This may be due to:"
    echo "  - Missing BTF support"
    echo "  - BPF already loaded"
    echo "  - aya/aya-obj version mismatch"
    exit 1
fi

# E2: Verify pin
log_info "E2: Verifying BPF pin..."
if [[ -d /sys/fs/bpf/akko ]]; then
    log_pass "E2: BPF pin exists at /sys/fs/bpf/akko"
    ls -la /sys/fs/bpf/akko/
else
    log_fail "E2: BPF pin directory missing"
fi

# E3: Power supply entry
log_info "E3: Checking power supply..."
POWER_SUPPLY=""
for ps in /sys/class/power_supply/hid-*; do
    if [[ -d "$ps" ]]; then
        POWER_SUPPLY="$ps"
        break
    fi
done

if [[ -n "$POWER_SUPPLY" ]]; then
    log_pass "E3: Power supply found: $(basename "$POWER_SUPPLY")"
else
    log_fail "E3: No power supply entry found"
    log_info "Available power supplies:"
    ls /sys/class/power_supply/ || true
fi

# E4: Read capacity
if [[ -n "$POWER_SUPPLY" ]] && [[ -f "$POWER_SUPPLY/capacity" ]]; then
    log_info "E4: Reading battery capacity..."
    CAPACITY=$(cat "$POWER_SUPPLY/capacity")
    if [[ "$CAPACITY" =~ ^[0-9]+$ ]] && [[ $CAPACITY -ge 0 ]] && [[ $CAPACITY -le 100 ]]; then
        log_pass "E4: Battery capacity: ${CAPACITY}%"
    else
        log_fail "E4: Invalid capacity: $CAPACITY"
    fi

    # Show additional info
    log_info "Power supply details:"
    for attr in status type scope model_name; do
        if [[ -f "$POWER_SUPPLY/$attr" ]]; then
            echo "  $attr: $(cat "$POWER_SUPPLY/$attr")"
        fi
    done
else
    log_fail "E4: Cannot read capacity"
fi

# E5: Desktop integration (check upower)
log_info "E5: Checking desktop integration..."
if command -v upower &>/dev/null; then
    if upower -e 2>/dev/null | grep -q "hid"; then
        log_pass "E5: UPower sees device"
        upower -i "$(upower -e | grep hid | head -1)" 2>/dev/null | head -10 || true
    else
        log_warn "E5: UPower does not see device (may be normal)"
    fi
else
    log_info "E5: upower not installed (skip)"
fi

# E6: Unload BPF
log_info "E6: Unloading BPF..."
if "$AKKO_LOADER" unload 2>&1; then
    log_pass "E6: BPF unloaded"
    BPF_LOADED=0
else
    log_fail "E6: BPF unload failed"
fi

# Verify unload
if [[ ! -d /sys/fs/bpf/akko ]]; then
    log_pass "E6.1: BPF pin removed"
else
    log_warn "E6.1: BPF pin still exists"
fi

# E7/E8: Systemd tests require service installed
echo ""
log_info "E7-E8: Systemd tests require installed service"
log_info "Run: sudo make install-systemd"
log_info "Then replug dongle to test auto-load"

echo ""
echo "BPF tests complete!"
