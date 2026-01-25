#!/bin/bash
# MonsGeek M1 V5 HE Keyboard Driver - Test Runner
# Usage: ./run_tests.sh [category] [test_id]
#   category: build, install, cli, tui, bpf, transport, all
#   test_id: specific test (e.g., A1, C2.1) or 'all'

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
DRIVER_DIR="$PROJECT_DIR/iot_driver_linux"
BPF_DIR="$PROJECT_DIR/akko-hid-bpf"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Test results
PASSED=0
FAILED=0
SKIPPED=0
declare -a FAILURES=()

# Binary paths
IOT_DRIVER="${IOT_DRIVER:-$DRIVER_DIR/target/release/iot_driver}"
AKKO_LOADER="${AKKO_LOADER:-$BPF_DIR/target/release/akko-loader}"

# Test configuration
SKIP_HARDWARE=${SKIP_HARDWARE:-0}
VERBOSE=${VERBOSE:-0}

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_pass() { echo -e "${GREEN}[PASS]${NC} $*"; (( ++PASSED )) || true; }
log_fail() { echo -e "${RED}[FAIL]${NC} $*"; (( ++FAILED )) || true; FAILURES+=("$1"); }
log_skip() { echo -e "${YELLOW}[SKIP]${NC} $*"; (( ++SKIPPED )) || true; }
log_section() { echo -e "\n${BLUE}=== $* ===${NC}\n"; }

# Check if hardware is available
check_hardware() {
    if [[ $SKIP_HARDWARE -eq 1 ]]; then
        return 1
    fi
    # Check for MonsGeek VID 0x3151
    if lsusb 2>/dev/null | grep -q "3151:"; then
        return 0
    fi
    return 1
}

# Check if running as root
check_root() {
    [[ $EUID -eq 0 ]]
}

run_test() {
    local id="$1"
    local name="$2"
    local cmd="$3"
    local expect="${4:-0}"  # Expected exit code

    if [[ $VERBOSE -eq 1 ]]; then
        log_info "Running: $cmd"
    fi

    local output
    local exit_code=0
    output=$(eval "$cmd" 2>&1) || exit_code=$?

    if [[ $exit_code -eq $expect ]]; then
        log_pass "$id: $name"
    else
        log_fail "$id: $name"
        if [[ $VERBOSE -eq 1 ]]; then
            echo "  Expected exit: $expect, got: $exit_code"
            echo "  Output: $output" | head -5
        fi
    fi
    # Always return 0 to not trigger set -e; failures tracked via FAILED counter
    return 0
}

run_test_output() {
    local id="$1"
    local name="$2"
    local cmd="$3"
    local grep_pattern="$4"

    if [[ $VERBOSE -eq 1 ]]; then
        log_info "Running: $cmd"
    fi

    local output
    local exit_code=0
    output=$(eval "$cmd" 2>&1) || exit_code=$?

    if [[ $exit_code -eq 0 ]] && echo "$output" | grep -qE "$grep_pattern"; then
        log_pass "$id: $name"
    else
        log_fail "$id: $name"
        if [[ $VERBOSE -eq 1 ]]; then
            echo "  Pattern: $grep_pattern"
            echo "  Output: $output" | head -5
        fi
    fi
    # Always return 0 to not trigger set -e; failures tracked via FAILED counter
    return 0
}

# ============================================================================
# Build Tests (Category A)
# ============================================================================

test_build() {
    log_section "Build Tests (Category A)"

    # A1: Clean build
    run_test "A1" "Clean build (release)" \
        "cd '$PROJECT_DIR' && make clean-driver 2>/dev/null; make driver"

    # A2: Debug build
    run_test "A2" "Debug build" \
        "cd '$PROJECT_DIR' && make driver-debug"

    # A3: BPF loader build
    run_test "A3" "BPF loader build" \
        "cd '$PROJECT_DIR' && make bpf"

    # A4: eBPF build (nightly) - may fail without nightly
    if rustup show 2>/dev/null | grep -q nightly; then
        run_test "A4" "eBPF build (nightly)" \
            "cd '$PROJECT_DIR' && make bpf-ebpf"
    else
        log_skip "A4: eBPF build (nightly not installed)"
    fi

    # A5: Format check
    run_test "A5" "Format check" \
        "cd '$PROJECT_DIR' && make fmt && git diff --exit-code -- '$DRIVER_DIR'"

    # A6: Clippy lint
    run_test "A6" "Clippy lint" \
        "cd '$PROJECT_DIR' && make check"

    # A7: Unit tests
    run_test "A7" "Unit tests" \
        "cd '$PROJECT_DIR' && make test"
}

# ============================================================================
# Installation Tests (Category B)
# ============================================================================

test_install() {
    log_section "Installation Tests (Category B)"

    if ! check_root; then
        log_skip "B: Installation tests require root (run with sudo or set SKIP_INSTALL=1)"
        return
    fi

    local PREFIX="/tmp/monsgeek-test"
    local BIN_DIR="$PREFIX/bin"
    local UDEV_DIR="$PREFIX/udev"

    # Clean test prefix
    rm -rf "$PREFIX"
    mkdir -p "$BIN_DIR" "$UDEV_DIR"

    # B1: Install driver
    run_test "B1" "Install driver binary" \
        "cd '$PROJECT_DIR' && make install-driver PREFIX='$PREFIX' BIN_DIR='$BIN_DIR'"

    # Check binary exists
    if [[ -x "$BIN_DIR/iot_driver" ]]; then
        log_pass "B1.1: Binary is executable"
    else
        log_fail "B1.1: Binary not found or not executable"
    fi

    # B2: Install udev (to temp dir)
    run_test "B2" "Install udev rules" \
        "cp '$PROJECT_DIR/udev/99-monsgeek.rules' '$UDEV_DIR/'"

    # B3: Udev rules valid
    if grep -q "3151" "$UDEV_DIR/99-monsgeek.rules"; then
        log_pass "B3: Udev rules contain MonsGeek VID"
    else
        log_fail "B3: Udev rules missing VID"
    fi

    # B4-B6: Skip systemd tests in temp install
    log_skip "B4-B6: Systemd tests (require actual system install)"

    # B7: Uninstall
    rm -rf "$PREFIX"
    if [[ ! -d "$PREFIX" ]]; then
        log_pass "B7: Cleanup complete"
    else
        log_fail "B7: Cleanup failed"
    fi
}

# ============================================================================
# CLI Functional Tests (Category C)
# ============================================================================

test_cli() {
    log_section "CLI Functional Tests (Category C)"

    if ! [[ -x "$IOT_DRIVER" ]]; then
        log_skip "C: Driver binary not found at $IOT_DRIVER"
        return
    fi

    # C0: Help and version
    run_test "C0.1" "Help output" "$IOT_DRIVER --help"
    run_test_output "C0.2" "Version output" "$IOT_DRIVER --version" "iot_driver [0-9]"

    if ! check_hardware; then
        log_skip "C1-C5: Hardware tests (keyboard not connected or SKIP_HARDWARE=1)"
        return
    fi

    # C1: Device Discovery
    log_info "C1: Device Discovery"
    run_test_output "C1.1" "List devices" "$IOT_DRIVER list" "3151|HID|hidraw"
    run_test_output "C1.2" "Device info" "$IOT_DRIVER info" "Device ID|Firmware"
    run_test_output "C1.3" "All info" "$IOT_DRIVER all" "Profile|LED|Debounce"

    # C2: Settings Get
    log_info "C2: Settings Get"
    run_test "C2.1" "Get profile" "$IOT_DRIVER profile"
    run_test "C2.2" "Get LED" "$IOT_DRIVER led"
    run_test "C2.3" "Get debounce" "$IOT_DRIVER debounce"
    run_test "C2.4" "Get polling rate" "$IOT_DRIVER rate"
    run_test "C2.5" "Get options" "$IOT_DRIVER options"
    run_test "C2.6" "Get sleep" "$IOT_DRIVER sleep"
    run_test "C2.7" "Get triggers" "$IOT_DRIVER triggers"
    run_test "C2.8" "Get features" "$IOT_DRIVER features"

    # C3: Settings Set (requires hardware, careful with changes)
    log_info "C3: Settings Set (may modify keyboard settings)"

    # Save current profile
    local current_profile
    current_profile=$($IOT_DRIVER profile 2>/dev/null | grep -oE '[0-3]' | head -1) || current_profile=0

    # Test set profile (then restore)
    local test_profile=$(( (current_profile + 1) % 4 ))
    run_test "C3.1a" "Set profile to $test_profile" "$IOT_DRIVER set-profile $test_profile"
    run_test "C3.1b" "Restore profile to $current_profile" "$IOT_DRIVER set-profile $current_profile"

    # Test debounce (non-destructive range)
    run_test "C3.2" "Set debounce" "$IOT_DRIVER set-debounce 5"

    # Test polling rate
    run_test "C3.3" "Set polling rate" "$IOT_DRIVER set-rate 1000"

    # C4: LED Animations (visual tests, just check no crash)
    log_info "C4: LED Animations (quick test)"
    run_test "C4.1" "Modes list" "$IOT_DRIVER modes"

    # C5: Battery (dongle only)
    log_info "C5: Battery"
    # Check if dongle is connected (VID:PID 3151:5038)
    if lsusb 2>/dev/null | grep -q "3151:5038"; then
        run_test "C5.1" "Get battery" "$IOT_DRIVER battery --quiet"
    else
        log_skip "C5: Battery tests (dongle not connected)"
    fi
}

# ============================================================================
# TUI Functional Tests (Category D)
# ============================================================================

test_tui() {
    log_section "TUI Functional Tests (Category D)"

    if ! [[ -x "$IOT_DRIVER" ]]; then
        log_skip "D: Driver binary not found"
        return
    fi

    if ! check_hardware; then
        log_skip "D: Hardware tests (keyboard not connected)"
        return
    fi

    log_info "TUI tests require manual interaction"
    log_info "Run: $IOT_DRIVER tui"
    log_skip "D1-D9: TUI tests (require manual verification)"
}

# ============================================================================
# BPF Battery Driver Tests (Category E)
# ============================================================================

test_bpf() {
    log_section "BPF Battery Driver Tests (Category E)"

    if ! [[ -x "$AKKO_LOADER" ]]; then
        log_skip "E: akko-loader not found at $AKKO_LOADER"
        return
    fi

    if ! check_root; then
        log_skip "E: BPF tests require root"
        return
    fi

    # Check kernel version
    local kernel_version
    kernel_version=$(uname -r | cut -d. -f1-2)
    local major minor
    major=$(echo "$kernel_version" | cut -d. -f1)
    minor=$(echo "$kernel_version" | cut -d. -f2)

    if [[ $major -lt 6 ]] || { [[ $major -eq 6 ]] && [[ $minor -lt 12 ]]; }; then
        log_skip "E: Kernel $kernel_version < 6.12 (HID-BPF struct_ops not supported)"
        return
    fi

    # E1: Load BPF
    if run_test "E1" "Load BPF" "$AKKO_LOADER load 2>&1"; then
        # E2: Verify pin
        if [[ -d /sys/fs/bpf/akko ]]; then
            log_pass "E2: BPF pin directory exists"
        else
            log_fail "E2: BPF pin directory missing"
        fi

        # E3: Power supply
        if ls /sys/class/power_supply/hid-* 2>/dev/null | head -1; then
            log_pass "E3: Power supply entry exists"

            # E4: Read capacity
            local capacity
            capacity=$(cat /sys/class/power_supply/hid-*/capacity 2>/dev/null | head -1)
            if [[ "$capacity" =~ ^[0-9]+$ ]] && [[ $capacity -ge 0 ]] && [[ $capacity -le 100 ]]; then
                log_pass "E4: Capacity reads $capacity%"
            else
                log_fail "E4: Invalid capacity: $capacity"
            fi
        else
            log_fail "E3: Power supply entry missing"
            log_skip "E4: Capacity (no power supply)"
        fi

        # E5: Desktop integration (skip in automated tests)
        log_skip "E5: Desktop integration (manual verification)"

        # E6: Unload BPF
        run_test "E6" "Unload BPF" "$AKKO_LOADER unload"
    else
        log_skip "E2-E6: BPF tests (load failed)"
    fi

    # E7-E8: Systemd tests
    log_skip "E7-E8: Systemd tests (require installed service)"
}

# ============================================================================
# Transport Layer Tests (Category F)
# ============================================================================

test_transport() {
    log_section "Transport Layer Tests (Category F)"

    if ! [[ -x "$IOT_DRIVER" ]]; then
        log_skip "F: Driver binary not found"
        return
    fi

    if ! check_hardware; then
        log_skip "F: Hardware tests (keyboard not connected)"
        return
    fi

    # F1-F3: Transport discovery
    local transport
    transport=$($IOT_DRIVER test-transport 2>&1) || true

    if echo "$transport" | grep -qE "HidWired|HidDongle|HidBluetooth"; then
        log_pass "F1-F3: Transport detected: $transport"
    else
        log_fail "F1-F3: Transport detection failed"
    fi

    # F4: Auto-select
    if $IOT_DRIVER info >/dev/null 2>&1; then
        log_pass "F4: Auto-select works"
    else
        log_fail "F4: Auto-select failed"
    fi

    # F5: BT battery (only if BT connected)
    if echo "$transport" | grep -q "Bluetooth"; then
        run_test "F5" "BT battery" "$IOT_DRIVER battery"
    else
        log_skip "F5: BT battery (Bluetooth not connected)"
    fi
}

# ============================================================================
# Main
# ============================================================================

usage() {
    echo "Usage: $0 [OPTIONS] [CATEGORY] [TEST_ID]"
    echo ""
    echo "Categories:"
    echo "  build     Build tests (A1-A7)"
    echo "  install   Installation tests (B1-B7, requires root)"
    echo "  cli       CLI functional tests (C1-C5)"
    echo "  tui       TUI tests (D1-D9, manual)"
    echo "  bpf       BPF battery tests (E1-E8, requires root)"
    echo "  transport Transport layer tests (F1-F5)"
    echo "  all       Run all tests (default)"
    echo ""
    echo "Options:"
    echo "  -v, --verbose       Show command output"
    echo "  -h, --help          Show this help"
    echo "  --skip-hardware     Skip tests requiring keyboard"
    echo ""
    echo "Environment:"
    echo "  IOT_DRIVER=path     Override driver binary path"
    echo "  AKKO_LOADER=path    Override BPF loader path"
    echo "  SKIP_HARDWARE=1     Skip hardware tests"
    echo "  VERBOSE=1           Verbose output"
}

main() {
    local category="all"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -v|--verbose) VERBOSE=1; shift ;;
            -h|--help) usage; exit 0 ;;
            --skip-hardware) SKIP_HARDWARE=1; shift ;;
            build|install|cli|tui|bpf|transport|all) category="$1"; shift ;;
            *) echo "Unknown option: $1"; usage; exit 1 ;;
        esac
    done

    echo "MonsGeek M1 V5 HE Keyboard Driver - Test Suite"
    echo "Date: $(date '+%Y-%m-%d %H:%M:%S')"
    echo "Kernel: $(uname -r)"
    echo "Hardware: $(check_hardware && echo "Detected" || echo "Not detected")"
    echo ""

    case "$category" in
        build) test_build ;;
        install) test_install ;;
        cli) test_cli ;;
        tui) test_tui ;;
        bpf) test_bpf ;;
        transport) test_transport ;;
        all)
            test_build
            test_cli
            test_transport
            # Skip install/bpf without explicit root
            if check_root; then
                test_install
                test_bpf
            else
                log_info "Skipping install/bpf tests (run with sudo for full suite)"
            fi
            test_tui
            ;;
    esac

    # Summary
    log_section "Test Summary"
    echo -e "Passed:  ${GREEN}$PASSED${NC}"
    echo -e "Failed:  ${RED}$FAILED${NC}"
    echo -e "Skipped: ${YELLOW}$SKIPPED${NC}"

    if [[ ${#FAILURES[@]} -gt 0 ]]; then
        echo ""
        echo "Failed tests:"
        for f in "${FAILURES[@]}"; do
            echo "  - $f"
        done
    fi

    # Exit with failure if any tests failed
    [[ $FAILED -eq 0 ]]
}

main "$@"
