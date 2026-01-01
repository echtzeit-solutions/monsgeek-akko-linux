#!/bin/bash
# Helper script to run Wine IOT driver with optional strace

WINEPREFIX=/home/florian/src-misc/monsgeek-m1-v5-tmr/wine_iot
IOT_EXE=/home/florian/src-misc/monsgeek-m1-v5-tmr/iot_driver.exe
LOG_DIR=/tmp

# Kill any existing servers on port 3814
pkill -f "target/debug/iot_driver" 2>/dev/null
pkill -f "target/release/iot_driver" 2>/dev/null
pkill -f "wine.*iot_driver" 2>/dev/null
sleep 1

# Check if port is free
if ss -tlnp 2>/dev/null | grep -q ":3814 "; then
    echo "ERROR: Port 3814 still in use"
    ss -tlnp | grep ":3814"
    exit 1
fi

case "${1:-run}" in
    strace)
        echo "Running Wine IOT driver with strace (HID file access)..."
        WINEPREFIX="$WINEPREFIX" WINEDEBUG=-all strace -f -e openat,ioctl -o "$LOG_DIR/wine_strace.log" \
            wine "$IOT_EXE" 2>"$LOG_DIR/wine_iot.log" &
        PID=$!
        echo "PID: $PID"
        echo "Strace log: $LOG_DIR/wine_strace.log"
        echo "Wine log: $LOG_DIR/wine_iot.log"
        ;;
    strace-hid)
        echo "Running Wine IOT driver with strace (filtering for hid/hidraw)..."
        WINEPREFIX="$WINEPREFIX" WINEDEBUG=-all strace -f -e openat,ioctl \
            wine "$IOT_EXE" 2>"$LOG_DIR/wine_iot.log" 2>&1 | grep -i "hid\|raw" &
        PID=$!
        echo "PID: $PID"
        ;;
    debug)
        echo "Running Wine IOT driver with HID debug..."
        WINEPREFIX="$WINEPREFIX" WINEDEBUG=+hid,+hidp \
            wine "$IOT_EXE" 2>&1 | tee "$LOG_DIR/wine_iot.log" &
        PID=$!
        echo "PID: $PID"
        ;;
    run)
        echo "Running Wine IOT driver..."
        WINEPREFIX="$WINEPREFIX" WINEDEBUG=-all \
            wine "$IOT_EXE" > "$LOG_DIR/wine_iot.log" 2>&1 &
        PID=$!
        echo "PID: $PID"
        echo "Log: $LOG_DIR/wine_iot.log"
        ;;
    test)
        echo "Testing gRPC connection to Wine IOT driver..."
        python3 - <<'PYEOF'
import socket
import time

# gRPC-Web request for watchDevList
request = bytes([
    0x00,  # not compressed
    0x00, 0x00, 0x00, 0x00,  # 0 byte message (Empty)
])

headers = (
    "POST /OnlineDebug/watchDevList HTTP/1.1\r\n"
    "Host: 127.0.0.1:3814\r\n"
    "Content-Type: application/grpc-web\r\n"
    "Accept: application/grpc-web\r\n"
    "Content-Length: 5\r\n"
    "\r\n"
)

sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.settimeout(5)
try:
    sock.connect(('127.0.0.1', 3814))
    sock.sendall(headers.encode() + request)
    response = sock.recv(4096)
    print("Response:")
    print(response.decode('utf-8', errors='replace'))
except Exception as e:
    print(f"Error: {e}")
finally:
    sock.close()
PYEOF
        ;;
    *)
        echo "Usage: $0 [run|strace|strace-hid|debug|test]"
        echo "  run        - Run normally (default)"
        echo "  strace     - Run with strace logging file access"
        echo "  strace-hid - Run with strace filtering for HID access"
        echo "  debug      - Run with Wine HID debug output"
        echo "  test       - Test gRPC connection"
        exit 1
        ;;
esac

echo ""
echo "Wait a moment for server to start, then run: $0 test"
