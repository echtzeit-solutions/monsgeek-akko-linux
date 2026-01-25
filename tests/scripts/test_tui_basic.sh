#!/bin/bash
# TUI Basic Tests
# Tests TUI launch and basic key navigation using tmux
#
# Prerequisites:
#   - tmux installed
#   - MonsGeek keyboard connected

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"
DRIVER_DIR="$PROJECT_DIR/iot_driver_linux"
IOT_DRIVER="${IOT_DRIVER:-$DRIVER_DIR/target/release/iot_driver}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_pass() { echo -e "${GREEN}[PASS]${NC} $*"; }
log_fail() { echo -e "${RED}[FAIL]${NC} $*"; }
log_info() { echo -e "${YELLOW}[INFO]${NC} $*"; }

SESSION_NAME="monsgeek-tui-test"
TIMEOUT=5

cleanup() {
    tmux kill-session -t "$SESSION_NAME" 2>/dev/null || true
}
trap cleanup EXIT

if ! [[ -x "$IOT_DRIVER" ]]; then
    echo "Error: iot_driver not found at $IOT_DRIVER"
    exit 1
fi

if ! command -v tmux &>/dev/null; then
    echo "Error: tmux required for TUI tests"
    echo "Install with: sudo apt install tmux"
    exit 1
fi

# Check hardware
if ! lsusb | grep -q "3151:"; then
    echo "Error: MonsGeek keyboard not connected"
    exit 1
fi

echo "TUI Basic Tests"
echo "==============="
echo ""

# Create tmux session
log_info "Creating tmux session..."
cleanup
tmux new-session -d -s "$SESSION_NAME" -x 120 -y 40

# Start TUI
log_info "Starting TUI..."
tmux send-keys -t "$SESSION_NAME" "$IOT_DRIVER tui" Enter
sleep 2

# Capture initial screen
CAPTURE=$(tmux capture-pane -t "$SESSION_NAME" -p 2>/dev/null)

# D1: Check TUI launched
if echo "$CAPTURE" | grep -qE "MonsGeek|Device|Profile|LED|Tab"; then
    log_pass "D1: TUI launched successfully"
else
    log_fail "D1: TUI failed to launch"
    echo "Captured output:"
    echo "$CAPTURE" | head -20
    exit 1
fi

# D2: Tab navigation
log_info "Testing tab navigation..."
for i in {1..5}; do
    tmux send-keys -t "$SESSION_NAME" Tab
    sleep 0.5
done
CAPTURE2=$(tmux capture-pane -t "$SESSION_NAME" -p 2>/dev/null)
if [[ "$CAPTURE2" != "$CAPTURE" ]]; then
    log_pass "D2: Tab navigation works"
else
    log_fail "D2: Tab navigation not working"
fi

# D3-D8: Check each tab exists (based on labels in UI)
for tab in "Device" "LED" "Depth" "Triggers" "Options" "Macros"; do
    if echo "$CAPTURE" | grep -q "$tab"; then
        log_pass "Tab visible: $tab"
    else
        log_info "Tab not visible in capture: $tab (may be in different view)"
    fi
done

# D9: Test quit
log_info "Testing quit (q key)..."
tmux send-keys -t "$SESSION_NAME" q
sleep 1

# Check if process exited
if ! tmux list-panes -t "$SESSION_NAME" -F '#{pane_current_command}' 2>/dev/null | grep -q iot_driver; then
    log_pass "D9: TUI exited cleanly"
else
    log_fail "D9: TUI did not exit"
fi

echo ""
echo "TUI basic tests complete!"
echo ""
echo "For interactive testing, run:"
echo "  $IOT_DRIVER tui"
