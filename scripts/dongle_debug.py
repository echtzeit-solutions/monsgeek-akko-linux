#!/usr/bin/env python3
"""
Akko/MonsGeek 2.4GHz Dongle Debug Tool

Auto-detects dongle hidraw devices and monitors all HID traffic.
Dumps everything with timestamps and hex + parsed output.

VID: 0x3151, PID: 0x5038
"""

import os
import sys
import time
import fcntl
import select
import argparse
from pathlib import Path
from dataclasses import dataclass
from typing import Optional, List, Dict
from datetime import datetime

VID = 0x3151
PID = 0x5038

# Command codes
COMMANDS = {
    0x03: "SET_LEDPARAM",
    0x80: "GET_PROFILE",
    0x82: "GET_LEDPARAM",
    0x83: "GET_SLEDPARAM",
    0x8F: "GET_USB_VERSION",
    0xA0: "GET_DEBOUNCE",
    0x61: "GET_FEATURE_LIST",
    0x12: "SET_USERGIF",
    0x1B: "SET_MAGNETISM_REPORT",
    0x0C: "SET_USERPIC",
    0x01: "PROFILE_CHANGE",
    0x0F: "MAGNETISM_STATUS",
    0x1D: "MAGNETISM_MODE",
    0x88: "BATTERY_STATUS",
}

# HID ioctls
def _IOC(dir, type, nr, size):
    return (dir << 30) | (size << 16) | (ord(type) << 8) | nr

HIDIOCSFEATURE = lambda size: _IOC(3, 'H', 0x06, size)
HIDIOCGFEATURE = lambda size: _IOC(3, 'H', 0x07, size)


@dataclass
class HidrawDevice:
    """Represents a hidraw device."""
    path: str
    hid_id: str
    rdesc: bytes
    fd: int = -1

    @property
    def short_id(self) -> str:
        """Short identifier for display."""
        return self.hid_id.split('.')[-1]

    @property
    def interface_type(self) -> str:
        """Identify interface type from descriptor."""
        if len(self.rdesc) < 3:
            return "unknown"

        if self.rdesc[0:3] == bytes([0x05, 0x01, 0x09]):
            usage = self.rdesc[3] if len(self.rdesc) > 3 else 0
            if usage == 0x06:
                return "keyboard"
            elif usage == 0x02:
                return "mouse"
            elif usage == 0x80:
                return "system"
            return f"desktop-{usage:02x}"
        elif self.rdesc[0:3] == bytes([0x06, 0xFF, 0xFF]):
            if b'\xB1' in self.rdesc:
                return "feature"
            elif b'\x81' in self.rdesc:
                return "vendor-in"
            return "vendor"
        elif self.rdesc[0:3] == bytes([0x05, 0x0C, 0x09]):
            # Consumer control - but check for vendor section too
            if b'\x06\xFF\xFF' in self.rdesc:
                return "multi+vendor"
            return "consumer"

        return "unknown"

    @property
    def has_vendor_input(self) -> bool:
        idx = self.rdesc.find(b'\x06\xFF\xFF')
        if idx >= 0:
            after = self.rdesc[idx:]
            return b'\x81\x02' in after or b'\x81\x00' in after
        return False

    @property
    def has_vendor_feature(self) -> bool:
        idx = self.rdesc.find(b'\x06\xFF\xFF')
        if idx >= 0:
            after = self.rdesc[idx:]
            return b'\xB1\x02' in after or b'\xB1\x00' in after
        return False


def find_dongle_devices() -> List[HidrawDevice]:
    """Find all hidraw devices for the Akko dongle."""
    devices = []
    hid_base = Path("/sys/bus/hid/devices")

    if not hid_base.exists():
        return devices

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

        hidraw_dir = hid_dir / "hidraw"
        if not hidraw_dir.exists():
            continue

        for hidraw in hidraw_dir.iterdir():
            if hidraw.name.startswith("hidraw"):
                hidraw_path = f"/dev/{hidraw.name}"
                rdesc_path = hid_dir / "report_descriptor"
                rdesc = rdesc_path.read_bytes() if rdesc_path.exists() else b''

                devices.append(HidrawDevice(
                    path=hidraw_path,
                    hid_id=hid_dir.name,
                    rdesc=rdesc
                ))

    devices.sort(key=lambda d: d.path)
    return devices


def parse_cmd(data: bytes) -> str:
    """Parse command/data and return description."""
    if not data:
        return "empty"

    # Handle report ID prefix
    offset = 0
    if len(data) > 1 and data[0] in (0x00, 0x05):
        offset = 1

    if offset >= len(data):
        return f"ReportID={data[0]:02x}"

    cmd = data[offset]
    name = COMMANDS.get(cmd, f"CMD_{cmd:02X}")

    # Parse specific commands
    if cmd == 0x88 and len(data) >= offset + 5:
        # Battery status
        battery = data[offset + 3]
        flags = data[offset + 4]
        online = "online" if (flags & 0x01) else "offline"
        charging = ", charging" if (flags & 0x02) else ""
        return f"{name}: {battery}% {online}{charging}"

    if cmd == 0x8F and len(data) >= offset + 10:
        # USB version response
        if data[offset + 1] == 0x8F:  # Echo
            device_id = int.from_bytes(data[offset + 2:offset + 6], 'little')
            version = int.from_bytes(data[offset + 8:offset + 10], 'little')
            v_str = f"{(version >> 8) & 0xF}.{(version >> 4) & 0xF}.{version & 0xF}"
            return f"{name}: id={device_id:08x} ver={v_str}"

    if cmd == 0x82 and len(data) >= offset + 9:
        # LED params
        if data[offset + 1] == 0x82:  # Echo
            mode = data[offset + 2]
            bright = data[offset + 3]
            speed = data[offset + 4]
            r, g, b = data[offset + 6], data[offset + 7], data[offset + 8]
            return f"{name}: mode={mode} bright={bright} speed={speed} RGB=({r},{g},{b})"

    if cmd == 0x1B and len(data) >= offset + 2:
        # Magnetism/key depth data
        return f"KEY_DEPTH: {len(data) - offset - 1} bytes"

    if cmd == 0x0F and len(data) >= offset + 3:
        # Magnetism status
        started = data[offset + 1] == 1
        return f"MAGNETISM: {'started' if started else 'stopped'}"

    return name


def timestamp() -> str:
    """Get timestamp string."""
    return datetime.now().strftime("%H:%M:%S.%f")[:-3]


def hexdump_short(data: bytes, max_bytes: int = 24) -> str:
    """Short hex dump."""
    hex_str = data[:max_bytes].hex()
    # Add spaces every 2 chars
    spaced = ' '.join(hex_str[i:i + 2] for i in range(0, len(hex_str), 2))
    if len(data) > max_bytes:
        spaced += "..."
    return spaced


def calc_checksum(buf: bytes, end_idx: int = 7) -> int:
    """Calculate Bit7 checksum."""
    total = sum(buf[1:end_idx + 1])
    return (255 - (total & 0xFF)) & 0xFF


def send_command(fd: int, cmd: int, data: bytes = b'') -> bool:
    """Send a feature report command."""
    buf = bytearray(65)
    buf[0] = 0x00  # Report ID
    buf[1] = cmd
    for i, b in enumerate(data):
        if i + 2 < 65:
            buf[i + 2] = b
    buf[8] = calc_checksum(buf)

    try:
        fcntl.ioctl(fd, HIDIOCSFEATURE(65), bytes(buf))
        return True
    except OSError:
        return False


def read_feature(fd: int) -> Optional[bytes]:
    """Read a feature report."""
    try:
        buf = bytearray(65)
        buf[0] = 0x00
        fcntl.ioctl(fd, HIDIOCGFEATURE(65), buf)
        return bytes(buf)
    except OSError:
        return None


def main():
    parser = argparse.ArgumentParser(
        description='Akko/MonsGeek dongle debug - dumps all HID activity',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s                  # Show devices and test GET commands
  %(prog)s -m               # Monitor all interfaces for activity
  %(prog)s -m -t 10         # Monitor for 10 seconds
  %(prog)s --set-led        # Test SET_LEDPARAM command
  %(prog)s --query 0x8f     # Send a specific GET command
  %(prog)s --battery        # Send status request, wait for INPUT response
""")
    parser.add_argument('-m', '--monitor', action='store_true',
                        help='Monitor all interfaces for Input reports')
    parser.add_argument('-t', '--time', type=float, default=5.0,
                        help='Monitor duration in seconds (default: 5)')
    parser.add_argument('--set-led', action='store_true',
                        help='Test SET_LEDPARAM (turns LED green)')
    parser.add_argument('--query', type=str, default=None,
                        help='Send a GET command (hex, e.g. 0x8f)')
    parser.add_argument('--battery', action='store_true',
                        help='Send 0x11 status request and monitor for 0x88 response')
    args = parser.parse_args()

    print("=" * 70)
    print(f"Akko/MonsGeek 2.4GHz Dongle Debug Tool")
    print(f"Target: VID 0x{VID:04X}, PID 0x{PID:04X}")
    print("=" * 70)
    print()

    devices = find_dongle_devices()

    if not devices:
        print("❌ No dongle found! Is the 2.4GHz receiver plugged in?")
        return 1

    print(f"Found {len(devices)} interface(s):")
    feature_dev = None
    input_devs = []

    for dev in devices:
        itype = dev.interface_type
        vendor_f = "F" if dev.has_vendor_feature else "-"
        vendor_i = "I" if dev.has_vendor_input else "-"
        print(f"  {dev.path:14} {dev.short_id} [{vendor_f}{vendor_i}] {itype:12} ({len(dev.rdesc)} bytes)")

        if dev.has_vendor_feature and feature_dev is None:
            feature_dev = dev
        if dev.has_vendor_input or itype in ("multi+vendor", "vendor-in"):
            input_devs.append(dev)

    print()

    # Test GET commands on feature interface
    if feature_dev:
        print("-" * 70)
        print(f"Testing GET commands on {feature_dev.path}")
        print("-" * 70)

        try:
            fd = os.open(feature_dev.path, os.O_RDWR)
            feature_dev.fd = fd

            test_cmds = [
                (0x8F, "GET_USB_VERSION"),
                (0x82, "GET_LEDPARAM"),
                (0x80, "GET_PROFILE"),
            ]

            for cmd, name in test_cmds:
                if send_command(fd, cmd):
                    time.sleep(0.05)
                    resp = read_feature(fd)
                    if resp:
                        is_zero = all(b == 0 for b in resp)
                        echo = resp[1] == cmd if resp else False
                        status = "✓" if echo else ("zeros" if is_zero else "no echo")
                        parsed = parse_cmd(resp) if echo else ""
                        print(f"  {name:20} → {status} {parsed}")
                        if not echo and not is_zero:
                            print(f"      Raw: {hexdump_short(resp)}")
                    else:
                        print(f"  {name:20} → ❌ read failed")
                else:
                    print(f"  {name:20} → ❌ send failed")

            # Test Report ID 5 (battery) on this interface
            print()
            print("  Testing Report ID 5 (battery)...")
            buf = bytearray(65)
            buf[0] = 0x05  # Report ID 5
            try:
                fcntl.ioctl(fd, HIDIOCGFEATURE(65), buf)
                resp = bytes(buf)
                is_zero = all(b == 0 for b in resp[1:8])
                if is_zero:
                    print(f"  BATTERY (RID=5)        → zeros")
                else:
                    battery = resp[1]
                    charging = resp[2]
                    online = resp[3]
                    print(f"  BATTERY (RID=5)        → {battery}% online={online} charging={charging}")
                    print(f"      Raw: {hexdump_short(resp)}")
            except OSError as e:
                print(f"  BATTERY (RID=5)        → ❌ {e}")

        except OSError as e:
            print(f"  Cannot open: {e}")
        print()

    # Also test Report ID 5 on the multi+vendor interface (hidraw2)
    for dev in input_devs:
        print("-" * 70)
        print(f"Testing Report ID 5 (battery) on {dev.path}")
        print("-" * 70)

        try:
            fd = os.open(dev.path, os.O_RDWR)
            buf = bytearray(65)
            buf[0] = 0x05  # Report ID 5
            try:
                fcntl.ioctl(fd, HIDIOCGFEATURE(65), buf)
                resp = bytes(buf)
                is_zero = all(b == 0 for b in resp[1:8])
                if is_zero:
                    print(f"  BATTERY (RID=5)        → zeros")
                else:
                    battery = resp[1]
                    charging = resp[2]
                    online = resp[3]
                    print(f"  BATTERY (RID=5)        → {battery}% online={online} charging={charging}")
                    print(f"      Raw: {hexdump_short(resp)}")
            except OSError as e:
                print(f"  BATTERY (RID=5)        → ❌ {e}")
            os.close(fd)
        except OSError as e:
            print(f"  Cannot open: {e}")
        print()

    # Test SET command
    if args.set_led and feature_dev and feature_dev.fd >= 0:
        print("-" * 70)
        print("Testing SET_LEDPARAM (LED → green)...")
        print("-" * 70)

        # SET_LEDPARAM: cmd=0x03, [mode, speed_inv, bright, opt, r, g, b]
        buf = bytearray(65)
        buf[0] = 0x00
        buf[1] = 0x03  # SET_LEDPARAM
        buf[2] = 1     # mode=constant
        buf[3] = 4     # speed
        buf[4] = 4     # brightness
        buf[5] = 0     # option
        buf[6] = 0     # R
        buf[7] = 255   # G
        buf[8] = 0     # B
        total = sum(buf[1:9])
        buf[9] = (255 - (total & 0xFF)) & 0xFF

        try:
            fcntl.ioctl(feature_dev.fd, HIDIOCSFEATURE(65), bytes(buf))
            print("  ✓ Command sent - check if keyboard LED turned green!")
        except OSError as e:
            print(f"  ❌ Failed: {e}")
        print()

    # Custom query
    if args.query and feature_dev and feature_dev.fd >= 0:
        cmd = int(args.query, 0)
        print("-" * 70)
        print(f"Sending command 0x{cmd:02X}...")
        print("-" * 70)

        if send_command(feature_dev.fd, cmd):
            time.sleep(0.1)
            resp = read_feature(feature_dev.fd)
            if resp:
                print(f"  Response: {hexdump_short(resp, 32)}")
                print(f"  Parsed: {parse_cmd(resp)}")
            else:
                print("  No response")
        else:
            print("  Send failed")
        print()

    # Monitor Input reports
    if args.monitor:
        print("-" * 70)
        print(f"Monitoring all interfaces for {args.time}s...")
        print("Press keys, change settings, etc. to trigger events")
        print("-" * 70)

        # Open all devices for reading
        fds = {}
        for dev in devices:
            try:
                fd = os.open(dev.path, os.O_RDONLY | os.O_NONBLOCK)
                fds[fd] = dev
            except OSError as e:
                print(f"  Cannot open {dev.path}: {e}")

        if fds:
            start = time.time()
            count = 0
            while time.time() - start < args.time:
                ready, _, _ = select.select(list(fds.keys()), [], [], 0.1)
                for fd in ready:
                    try:
                        data = os.read(fd, 65)
                        if data:
                            dev = fds[fd]
                            ts = timestamp()
                            parsed = parse_cmd(data)
                            hex_str = hexdump_short(data, 16)
                            print(f"[{ts}] {dev.path:14} ← {hex_str}")
                            print(f"             {parsed}")
                            count += 1
                    except BlockingIOError:
                        pass

            for fd in fds:
                os.close(fd)

            if count == 0:
                print("  (no activity detected)")
        print()

    # Battery status request test
    if args.battery and feature_dev and input_devs:
        print("-" * 70)
        print("Battery Status Request Test")
        print("Sending 0x11 (status request), monitoring for 0x88 response...")
        print("-" * 70)

        # Open feature device for sending
        try:
            feat_fd = os.open(feature_dev.path, os.O_RDWR)
        except OSError as e:
            print(f"  Cannot open feature device: {e}")
            feat_fd = -1

        # Open input devices for receiving
        input_fds = {}
        for dev in input_devs:
            try:
                fd = os.open(dev.path, os.O_RDONLY | os.O_NONBLOCK)
                input_fds[fd] = dev
            except OSError as e:
                print(f"  Cannot open {dev.path}: {e}")

        if feat_fd >= 0 and input_fds:
            # Send status request (command 0x11)
            print(f"  Sending 0x11 via {feature_dev.path}...")
            if send_command(feat_fd, 0x11):
                print("  ✓ Command sent, waiting for INPUT response...")

                # Monitor for response
                found_battery = False
                start = time.time()
                timeout = 2.0  # 2 second timeout

                while time.time() - start < timeout:
                    ready, _, _ = select.select(list(input_fds.keys()), [], [], 0.1)
                    for fd in ready:
                        try:
                            data = os.read(fd, 65)
                            if data:
                                dev = input_fds[fd]
                                ts = timestamp()
                                hex_str = hexdump_short(data, 24)
                                print(f"  [{ts}] {dev.path} ← {hex_str}")

                                # Check for 0x88 status response
                                # Format: [report_id] 0x88 ... battery_level flags
                                for i, b in enumerate(data):
                                    if b == 0x88 and i + 4 < len(data):
                                        battery = data[i + 3]
                                        flags = data[i + 4]
                                        online = "online" if (flags & 0x01) else "offline"
                                        charging = ", charging" if (flags & 0x02) else ""
                                        print(f"  → BATTERY: {battery}% {online}{charging}")
                                        found_battery = True
                                        break
                        except BlockingIOError:
                            pass

                if not found_battery:
                    print("  ✗ No 0x88 response received within timeout")
                    print("    The keyboard may be asleep or not connected wirelessly")
            else:
                print("  ✗ Failed to send command")

            os.close(feat_fd)
            for fd in input_fds:
                os.close(fd)
        print()

    # Cleanup
    if feature_dev and feature_dev.fd >= 0:
        os.close(feature_dev.fd)

    # Summary
    print("-" * 70)
    print("Summary:")
    print("-" * 70)
    if feature_dev:
        # Quick test
        try:
            fd = os.open(feature_dev.path, os.O_RDWR)
            send_command(fd, 0x8F)
            time.sleep(0.05)
            resp = read_feature(fd)
            os.close(fd)
            if resp and resp[1] == 0x8F:
                print(f"  ✓ Feature interface responding: {feature_dev.path}")
            else:
                print(f"  ⚠ Feature interface sends OK but GET returns zeros")
                print(f"    This is a known dongle firmware limitation.")
                print(f"    SET commands work, GET commands don't return data.")
        except:
            print(f"  ❌ Feature interface error")
    else:
        print("  ❌ No vendor Feature interface found")

    if input_devs:
        print(f"  ℹ Input interfaces: {', '.join(d.path for d in input_devs)}")
        print(f"    Use -m to monitor for spontaneous events (battery, etc.)")

    return 0


if __name__ == '__main__':
    sys.exit(main())
