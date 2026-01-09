#!/usr/bin/env python3
"""
Test F7 command exactly as Windows does it.

From USB capture analysis:
- Windows sends SET_REPORT: [f7 00 00 00...] to Report ID 0, Interface 2
- Windows then GET_REPORT: Returns [00 5f 00 00 01 01 01...] where 5f=95% battery

This test replicates that exact sequence.
"""

import os
import sys
import fcntl
import time
from pathlib import Path

VID = 0x3151
PID = 0x5038

def _IOC(dir, type, nr, size):
    return (dir << 30) | (size << 16) | (ord(type) << 8) | nr

HIDIOCSFEATURE = lambda size: _IOC(3, 'H', 0x06, size)
HIDIOCGFEATURE = lambda size: _IOC(3, 'H', 0x07, size)


def find_feature_device():
    """Find the vendor feature device (interface 2)."""
    hid_base = Path("/sys/bus/hid/devices")

    for hid_dir in hid_base.iterdir():
        parts = hid_dir.name.split(':')
        if len(parts) != 3:
            continue
        try:
            vid = int(parts[1], 16)
            pid_part = parts[2].split('.')[0]
            pid = int(pid_part, 16)
        except ValueError:
            continue

        if vid != VID or pid != PID:
            continue

        # Check report descriptor for vendor feature
        rdesc_path = hid_dir / "report_descriptor"
        if not rdesc_path.exists():
            continue
        rdesc = rdesc_path.read_bytes()

        # Looking for vendor page (0x06, 0xFF, 0xFF) with feature report (0xB1)
        if b'\x06\xFF\xFF' in rdesc and b'\xB1' in rdesc:
            hidraw_dir = hid_dir / "hidraw"
            if hidraw_dir.exists():
                for hidraw in hidraw_dir.iterdir():
                    path = f"/dev/{hidraw.name}"
                    return path, len(rdesc)

    return None, 0


def test_f7_command():
    """Test the F7 command exactly as Windows does."""

    path, rdesc_len = find_feature_device()
    if not path:
        print("No feature device found!")
        return False

    print(f"Found feature interface: {path} ({rdesc_len} bytes)")

    try:
        fd = os.open(path, os.O_RDWR)
    except OSError as e:
        print(f"Cannot open {path}: {e}")
        return False

    # Windows protocol from USB capture:
    # SET_REPORT (bRequest=0x09): wValue=0x0300 (Feature, Report ID 0), wIndex=2, wLength=64
    # Data: f7 00 00 00 00 00 00 00... (64 bytes)

    # Test 1: SET + GET with Report ID 0 (exactly like Windows)
    print("\nTest 1: F7 command with Report ID 0 (Windows method)")
    print("-" * 50)

    # SET: F7 command at byte[1], Report ID 0 at byte[0]
    set_buf = bytearray(65)  # hidraw adds report ID at [0]
    set_buf[0] = 0x00  # Report ID 0
    set_buf[1] = 0xF7  # F7 command
    # Rest is zeros (as in Windows capture)

    print(f"Sending SET: {' '.join(f'{b:02x}' for b in set_buf[:16])}...")
    try:
        fcntl.ioctl(fd, HIDIOCSFEATURE(65), bytes(set_buf))
        print("SET OK")
    except OSError as e:
        print(f"SET failed: {e}")
        os.close(fd)
        return False

    # Small delay (Windows has ~258 bytes / USB frame delays between SET and GET)
    time.sleep(0.05)

    # GET: Read Report ID 0
    get_buf = bytearray(65)
    get_buf[0] = 0x00  # Report ID 0

    print(f"Sending GET with Report ID 0...")
    try:
        fcntl.ioctl(fd, HIDIOCGFEATURE(65), get_buf)
        resp = bytes(get_buf)
        hex_resp = ' '.join(f'{b:02x}' for b in resp[:16])
        print(f"GET response: {hex_resp}")

        # Check if we got battery data
        if resp[1] > 0 and resp[1] <= 100:
            print(f"\n*** BATTERY FOUND: {resp[1]}% ***")
            print(f"    Online: {resp[4]}, Charging: {resp[5]}, Connected: {resp[6]}")
            os.close(fd)
            return True
        else:
            print("No valid battery data (byte[1] is 0 or >100)")
    except OSError as e:
        print(f"GET failed: {e}")

    # Test 2: Try without the SET first (maybe dongle caches battery?)
    print("\nTest 2: Just GET Report ID 0 (no SET)")
    print("-" * 50)

    get_buf = bytearray(65)
    get_buf[0] = 0x00

    try:
        fcntl.ioctl(fd, HIDIOCGFEATURE(65), get_buf)
        resp = bytes(get_buf)
        hex_resp = ' '.join(f'{b:02x}' for b in resp[:16])
        print(f"GET response: {hex_resp}")

        if resp[1] > 0 and resp[1] <= 100:
            print(f"\n*** BATTERY FOUND: {resp[1]}% ***")
    except OSError as e:
        print(f"GET failed: {e}")

    # Test 3: Try Report ID 5 (what we tried before)
    print("\nTest 3: GET Report ID 5 (alternative)")
    print("-" * 50)

    get_buf = bytearray(65)
    get_buf[0] = 0x05  # Report ID 5

    try:
        fcntl.ioctl(fd, HIDIOCGFEATURE(65), get_buf)
        resp = bytes(get_buf)
        hex_resp = ' '.join(f'{b:02x}' for b in resp[:16])
        print(f"GET response: {hex_resp}")

        if resp[1] > 0 and resp[1] <= 100:
            print(f"\n*** BATTERY FOUND: {resp[1]}% ***")
    except OSError as e:
        print(f"GET failed: {e}")

    # Test 4: Try multiple rapid SET+GET cycles (in case dongle needs "wake up")
    print("\nTest 4: Multiple SET+GET cycles (5 rounds)")
    print("-" * 50)

    for i in range(5):
        set_buf = bytearray(65)
        set_buf[0] = 0x00
        set_buf[1] = 0xF7

        try:
            fcntl.ioctl(fd, HIDIOCSFEATURE(65), bytes(set_buf))
        except OSError:
            pass

        time.sleep(0.02)

        get_buf = bytearray(65)
        get_buf[0] = 0x00

        try:
            fcntl.ioctl(fd, HIDIOCGFEATURE(65), get_buf)
            resp = bytes(get_buf)
            battery = resp[1]
            if battery > 0 and battery <= 100:
                print(f"  Round {i+1}: BATTERY = {battery}%")
                os.close(fd)
                return True
            else:
                status = "zeros" if all(b == 0 for b in resp[:8]) else f"data={resp[1]:02x}"
                print(f"  Round {i+1}: {status}")
        except OSError as e:
            print(f"  Round {i+1}: error - {e}")

    os.close(fd)

    print("\n" + "=" * 50)
    print("CONCLUSION: Dongle not responding to F7 command on Linux")
    print("This may be a driver/kernel difference in how GET_FEATURE_REPORT works")
    print("=" * 50)

    return False


if __name__ == '__main__':
    success = test_f7_command()
    sys.exit(0 if success else 1)
