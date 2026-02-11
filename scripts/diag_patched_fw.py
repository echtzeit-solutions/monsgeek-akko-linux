#!/usr/bin/env python3
"""
Build, flash, and diagnose patched MonsGeek M1 V5 firmware.

Usage:
  python3 diag_patched_fw.py                    # diagnose only (no flash)
  python3 diag_patched_fw.py --flash FILE.bin   # flash + wait + diagnose
  python3 diag_patched_fw.py --build-flash      # build + flash + diagnose
"""

import argparse
import os
import signal
import struct
import subprocess
import sys
import time
from pathlib import Path

try:
    import hid
except ImportError:
    print("ERROR: pip install hidapi", file=sys.stderr)
    sys.exit(1)

VID, PID = 0x3151, 0x5030
USAGE_PAGE_VENDOR = 0xFFFF
USAGE_PAGE_BOOT = 0xFF01
FAIL = "\033[91mFAIL\033[0m"
OK = "\033[92mOK\033[0m"
WARN = "\033[93mWARN\033[0m"
INFO = "\033[94mINFO\033[0m"

SCRIPT_DIR = Path(__file__).parent
PATCH_DIR = SCRIPT_DIR.parent / "firmwares" / "2949-v407" / "patch"
FW_PATCHED = SCRIPT_DIR.parent / "firmwares" / "2949-v407" / "firmware_patched.bin"
FLASH_CMD = ["cargo", "run", "--release", "--", "firmware", "flash", "-y"]
FLASH_CWD = SCRIPT_DIR.parent / "iot_driver_linux"


def section(title):
    print(f"\n{'─'*60}")
    print(f"  {title}")
    print(f"{'─'*60}")


# ── Build ────────────────────────────────────────────────────

def do_build():
    section("0a. Build firmware patch")
    r = subprocess.run(["make", "clean"], cwd=PATCH_DIR, capture_output=True)
    r = subprocess.run(["make"], cwd=PATCH_DIR, capture_output=True, text=True)
    if r.returncode != 0:
        print(f"  [{FAIL}] make failed:\n{r.stderr}")
        return False
    for line in r.stdout.strip().split("\n"):
        print(f"    {line}")

    section("0b. Patch firmware binary")
    r = subprocess.run(["make", "patch"], cwd=PATCH_DIR,
                       capture_output=True, text=True)
    if r.returncode != 0:
        print(f"  [{FAIL}] make patch failed:\n{r.stderr}")
        return False
    for line in r.stdout.strip().split("\n"):
        print(f"    {line}")

    if not FW_PATCHED.exists():
        print(f"  [{FAIL}] {FW_PATCHED} not found after build")
        return False
    print(f"  [{OK}] Built: {FW_PATCHED} ({FW_PATCHED.stat().st_size} bytes)")
    return True


# ── Flash ────────────────────────────────────────────────────

def do_flash(fw_path: Path):
    section("0c. Flash firmware")
    if not fw_path.exists():
        print(f"  [{FAIL}] {fw_path} not found")
        return False

    print(f"  Firmware: {fw_path} ({fw_path.stat().st_size} bytes)")

    # Use Rust CLI flash command
    r = subprocess.run(
        FLASH_CMD + [str(fw_path)],
        capture_output=True, text=True, timeout=30,
        cwd=FLASH_CWD,
    )
    for line in (r.stdout + r.stderr).strip().split("\n"):
        print(f"    {line}")
    if r.returncode != 0:
        print(f"  [{FAIL}] Flash failed (exit {r.returncode})")
        return False

    print(f"  [{OK}] Flash complete, waiting for re-enumeration...")

    # Wait for device to re-appear in normal mode
    for i in range(20):
        time.sleep(1)
        devs = hid.enumerate(VID, PID)
        if devs:
            print(f"  [{OK}] Device re-enumerated after {i+1}s")
            time.sleep(2)  # extra settle time for kernel drivers
            return True
        sys.stdout.write(".")
        sys.stdout.flush()

    print(f"\n  [{FAIL}] Device did not re-enumerate within 20s")
    return False


# ── Diagnostics ──────────────────────────────────────────────

def check_usb_enumeration():
    section("1. USB Enumeration")
    devs = hid.enumerate(VID, PID)
    if not devs:
        print(f"  [{FAIL}] Device not found (VID=0x{VID:04X} PID=0x{PID:04X})")
        return False, {}

    interfaces = {}
    for d in devs:
        iface = d.get("interface_number", -1)
        if iface not in interfaces:
            interfaces[iface] = []
        interfaces[iface].append(d)

    for iface in sorted(interfaces.keys()):
        usages = [f"0x{d.get('usage_page',0):04X}" for d in interfaces[iface]]
        paths = set(os.path.basename(d.get("path", b"").decode("utf-8", errors="replace"))
                     for d in interfaces[iface])
        print(f"  IF{iface}: usage_pages=[{', '.join(usages)}] hidraw=[{','.join(sorted(paths))}]")

    has_if2 = 2 in interfaces
    if2_ffff = any(d.get("usage_page") == 0xFFFF for d in interfaces.get(2, []))
    print(f"  IF2 vendor present: {'[' + OK + ']' if if2_ffff else '[' + FAIL + ']'}")
    return True, interfaces


def check_hidraw_mapping():
    section("2. HID Device → hidraw Mapping")
    hid_dir = Path("/sys/bus/hid/devices")
    for entry in sorted(hid_dir.iterdir()):
        if "3151" in entry.name and "5030" in entry.name:
            hidraw_entries = list((entry / "hidraw").iterdir()) if (entry / "hidraw").exists() else []
            hidraw_names = [e.name for e in hidraw_entries]
            driver_link = entry / "driver"
            driver = os.path.basename(os.readlink(str(driver_link))) if driver_link.is_symlink() else "?"
            print(f"  {entry.name}: hidraw=[{','.join(hidraw_names)}] driver={driver}")


def check_descriptors():
    section("3. USB Descriptors (wDescriptorLength)")
    for usb_dev in Path("/sys/bus/usb/devices").iterdir():
        idv = usb_dev / "idVendor"
        idp = usb_dev / "idProduct"
        if not (idv.exists() and idp.exists()):
            continue
        try:
            if idv.read_text().strip() != "3151" or idp.read_text().strip() != "5030":
                continue
        except OSError:
            continue

        desc_path = usb_dev / "descriptors"
        if not desc_path.exists():
            continue
        data = desc_path.read_bytes()

        # Parse USB descriptors sequentially
        pos = 0
        current_iface = -1
        hid_descs = []
        while pos < len(data):
            bLen = data[pos]
            if bLen < 2 or pos + bLen > len(data):
                break
            bType = data[pos + 1]
            if bType == 4 and bLen >= 4:  # Interface descriptor
                current_iface = data[pos + 2]
            elif bType == 0x21 and bLen >= 9:  # HID descriptor
                wDescLen = struct.unpack_from("<H", data, pos + 7)[0]
                hid_descs.append((current_iface, wDescLen))
            pos += bLen

        for iface, wlen in hid_descs:
            status = OK if (iface == 1 and wlen == 195) or iface != 1 else WARN
            note = ""
            if iface == 1:
                if wlen == 195:
                    note = " (extended: 171 + 24 battery)"
                elif wlen == 171:
                    note = " (original, no battery)"
                else:
                    note = f" (unexpected!)"
            print(f"  [{status}] IF{iface}: wDescriptorLength={wlen}{note}")
        return

    print(f"  [{WARN}] USB device not found in sysfs")


def check_power_supply():
    section("4. Power Supply (kernel battery driver)")
    ps_dir = Path("/sys/class/power_supply")
    found = None
    for entry in ps_dir.iterdir():
        if "3151" in entry.name and "5030" in entry.name:
            found = entry
            break

    if not found:
        print(f"  [{WARN}] No power_supply for 3151:5030")
        print(f"         (Kernel didn't find battery descriptor in IF1)")
        return

    print(f"  [{OK}] Found: {found.name}")
    for attr in ["type", "present", "scope", "model_name", "capacity", "status"]:
        path = found / attr
        if path.exists():
            try:
                val = path.read_text().strip()
                is_err = attr in ("capacity", "status") and val == ""
                tag = FAIL if is_err else OK
                val_str = "ENODATA (empty)" if is_err else val
                print(f"    [{tag}] {attr}: {val_str}")
            except OSError as e:
                print(f"    [{FAIL}] {attr}: {e}")


def hid_op_with_timeout(func, timeout_sec=3):
    """Run a HID operation with a signal-based timeout."""
    result = [None]
    error = [None]

    def handler(signum, frame):
        raise TimeoutError()

    old = signal.signal(signal.SIGALRM, handler)
    try:
        signal.alarm(timeout_sec)
        result[0] = func()
        signal.alarm(0)
    except TimeoutError:
        signal.alarm(0)
        error[0] = "TIMEOUT"
    except Exception as e:
        signal.alarm(0)
        error[0] = str(e)
    finally:
        signal.signal(signal.SIGALRM, old)

    return result[0], error[0]


def check_vendor_commands(interfaces):
    section("5. Vendor Commands (IF2 Feature Reports)")

    if2_dev = None
    for d in interfaces.get(2, []):
        if d.get("usage_page") == 0xFFFF:
            if2_dev = d
            break

    if not if2_dev:
        print(f"  [{FAIL}] No IF2 device with usage_page=0xFFFF")
        return

    path = if2_dev["path"]
    print(f"  Device: {path}")

    try:
        dev = hid.Device(path=path)
    except Exception as e:
        print(f"  [{FAIL}] Open failed: {e}")
        return

    # Test A: Plain GET_REPORT
    print(f"\n  [A] GET_REPORT (read-only)...")
    resp, err = hid_op_with_timeout(lambda: dev.get_feature_report(0, 65))
    if err:
        print(f"  [{FAIL}] {err}")
        if "TIMEOUT" in err:
            print(f"         EP0 is blocked — firmware not responding to Feature Reports.")
            print(f"         Likely: kernel battery GET_REPORT on IF1 is blocking EP0.")
        if "Protocol error" in str(err):
            print(f"         EP0 protocol error — likely STALL or corrupt state.")
        dev.close()
        return
    print(f"  [{OK}] {len(resp)} bytes: {resp[:16].hex()}")

    # Test A2: 0xFD DEBUG_LOG (early, before other commands that might break EP0)
    print(f"\n  [A2] 0xFD DEBUG_LOG (early read)...")
    try:
        ring_data = bytearray()
        log_count = 0
        log_head = 0
        log_buf_size = 0

        for page in range(10):
            buf = bytearray(65)
            buf[0] = 0x00; buf[1] = 0xFD; buf[3] = page
            buf[8] = (0xFF - (0xFD & 0xFF)) & 0xFF
            dev.send_feature_report(bytes(buf))
            time.sleep(0.03)
            resp_log, err_log = hid_op_with_timeout(lambda: dev.get_feature_report(0, 65))
            if err_log:
                print(f"  [{FAIL}] Page {page}: {err_log}")
                break

            if page == 0:
                log_count = struct.unpack_from("<H", resp_log, 2)[0]
                log_head = struct.unpack_from("<H", resp_log, 4)[0]
                log_buf_size = resp_log[6] << 8
                print(f"  [{OK}] count={log_count} head={log_head} buf_size={log_buf_size}")
                if log_count == 0:
                    print(f"  [{INFO}] Log buffer empty")
                    break

            ring_data.extend(resp_log[7:63])

        # Parse log entries (same as section D)
        ring_data = ring_data[:log_buf_size] if log_buf_size else ring_data[:512]
        if log_count > 0 and len(ring_data) > 0:
            start = log_head if log_count >= log_buf_size else 0
            LOG_TYPES = {
                0x01: ("HID_SETUP", 8), 0x02: ("RESULT", 2),
                0x03: ("VENDOR_CMD", 2), 0x04: ("USB_CONNECT", 0),
                0x05: ("EP0_XFER", 6),
            }
            pos = start
            bytes_read = 0
            entries = []
            max_bytes = min(log_count, log_buf_size)
            while bytes_read < max_bytes:
                type_byte = ring_data[pos % log_buf_size]
                if type_byte == 0:
                    break
                info = LOG_TYPES.get(type_byte)
                if info is None:
                    break
                name, payload_len = info
                payload = bytearray()
                for j in range(payload_len):
                    payload.append(ring_data[(pos + 1 + j) % log_buf_size])
                entries.append((type_byte, name, payload))
                pos = (pos + 1 + payload_len) % log_buf_size
                bytes_read += 1 + payload_len

            print(f"  Entries ({len(entries)} parsed, {bytes_read}/{max_bytes} bytes):")
            for i, (typ, name, payload) in enumerate(entries):
                if typ == 0x01:
                    bmReq = payload[0]; bReq = payload[1]
                    wVal = payload[2] | (payload[3] << 8)
                    wIdx = payload[4] | (payload[5] << 8)
                    wLen = payload[6] | (payload[7] << 8)
                    print(f"    [{i:3d}] {name}: bmReq=0x{bmReq:02X} bReq=0x{bReq:02X} "
                          f"wVal=0x{wVal:04X} wIdx={wIdx} wLen={wLen}")
                elif typ == 0x02:
                    r = payload[0]; b = payload[1]
                    tag = "intercept" if r == 1 else ("stall" if r == 2 else ("zlp" if r == 3 else "pass"))
                    print(f"    [{i:3d}] {name}: {tag} bat={b}")
                elif typ == 0x03:
                    print(f"    [{i:3d}] {name}: buf0=0x{payload[0]:02X} cmd=0x{payload[1]:02X}")
                elif typ == 0x04:
                    print(f"    [{i:3d}] {name}")
                elif typ == 0x05:
                    print(f"    [{i:3d}] {name}: {payload.hex()}")
                else:
                    print(f"    [{i:3d}] {name}: {payload.hex()}")

            type_counts = {}
            for _, name, _ in entries:
                type_counts[name] = type_counts.get(name, 0) + 1
            print(f"  Summary: {', '.join(f'{n}={c}' for n, c in sorted(type_counts.items()))}")
    except Exception as e:
        print(f"  [{FAIL}] Early log read: {e}")

    # Test B: 0x8F GET_USB_VERSION
    print(f"\n  [B] 0x8F GET_USB_VERSION...")
    buf = bytearray(65)
    buf[0] = 0x00; buf[1] = 0x8F
    buf[8] = (0xFF - (0x8F & 0xFF)) & 0xFF
    try:
        dev.send_feature_report(bytes(buf))
        time.sleep(0.05)
        resp, err = hid_op_with_timeout(lambda: dev.get_feature_report(0, 65))
        if err:
            print(f"  [{FAIL}] {err}")
        else:
            chip_id = struct.unpack_from("<H", resp, 2)[0] if len(resp) > 3 else 0
            fw_minor = resp[4] if len(resp) > 4 else 0
            fw_major = resp[5] if len(resp) > 5 else 0
            print(f"  [{OK}] chip=0x{chip_id:04X} fw=v{fw_major}{fw_minor:02d}")
            print(f"         raw: {resp[:10].hex()}")
    except Exception as e:
        print(f"  [{FAIL}] {e}")

    # Test C: 0xFB PATCH_INFO
    print(f"\n  [C] 0xFB PATCH_INFO...")
    buf = bytearray(65)
    buf[0] = 0x00; buf[1] = 0xFB
    buf[8] = (0xFF - (0xFB & 0xFF)) & 0xFF
    try:
        dev.send_feature_report(bytes(buf))
        time.sleep(0.05)
        resp, err = hid_op_with_timeout(lambda: dev.get_feature_report(0, 65))
        if err:
            print(f"  [{FAIL}] {err}")
        else:
            magic = (resp[2] << 8 | resp[3]) if len(resp) > 3 else 0
            if magic == 0xCAFE:
                ver = resp[4] if len(resp) > 4 else 0
                caps = struct.unpack_from("<H", resp, 5)[0] if len(resp) > 6 else 0
                name = bytes(resp[7:15]).split(b"\x00")[0].decode("ascii", errors="replace")
                print(f"  [{OK}] PATCHED: magic=0xCAFE ver={ver} caps=0x{caps:04X} name='{name}'")

                # Diagnostics
                if len(resp) > 28:
                    calls = struct.unpack_from("<H", resp, 15)[0]
                    intercepts = struct.unpack_from("<H", resp, 17)[0]
                    last_bmReq = resp[19]
                    last_bReq = resp[20]
                    last_wVal = struct.unpack_from("<H", resp, 21)[0]
                    last_wIdx = struct.unpack_from("<H", resp, 23)[0]
                    last_wLen = struct.unpack_from("<H", resp, 25)[0]
                    last_bat = resp[27]
                    last_result = resp[28]
                    print(f"         diag: calls={calls} intercepts={intercepts}")
                    print(f"         last: bmReq=0x{last_bmReq:02X} bReq=0x{last_bReq:02X} "
                          f"wVal=0x{last_wVal:04X} wIdx=0x{last_wIdx:04X} wLen=0x{last_wLen:04X}")
                    print(f"         battery_level={last_bat} result={last_result}")
                    # Extended battery diagnostics (bytes 30-37)
                    if len(resp) > 36:
                        raw_bat_level = resp[29]
                        raw_charger   = resp[30]
                        raw_debounce  = resp[31]
                        raw_update_ctr= resp[32]
                        raw_bat_raw   = resp[33]
                        raw_indicator = resp[34]
                        adc_ctr = struct.unpack_from("<H", resp, 35)[0]
                        print(f"         kbd_state: battery_level={raw_bat_level} "
                              f"charger={raw_charger} debounce={raw_debounce} "
                              f"update_ctr={raw_update_ctr}")
                        print(f"         kbd_state: battery_raw={raw_bat_raw} "
                              f"indicator_active={raw_indicator} "
                              f"adc_counter={adc_ctr}")
            else:
                print(f"  [{WARN}] Not patched or wrong response (magic=0x{magic:04X})")
                print(f"         raw: {resp[:20].hex()}")
    except Exception as e:
        print(f"  [{FAIL}] {e}")

    # Test D: 0xFD DEBUG_LOG
    print(f"\n  [D] 0xFD DEBUG_LOG...")
    try:
        # Read all 10 pages (covers 560 bytes, more than 512 ring)
        ring_data = bytearray()
        log_count = 0
        log_head = 0
        log_buf_size = 0

        for page in range(10):
            buf = bytearray(65)
            buf[0] = 0x00; buf[1] = 0xFD; buf[3] = page
            buf[8] = (0xFF - (0xFD & 0xFF)) & 0xFF
            dev.send_feature_report(bytes(buf))
            time.sleep(0.03)
            resp, err = hid_op_with_timeout(lambda: dev.get_feature_report(0, 65))
            if err:
                print(f"  [{FAIL}] Page {page}: {err}")
                break

            if page == 0:
                log_count = struct.unpack_from("<H", resp, 2)[0]
                log_head = struct.unpack_from("<H", resp, 4)[0]
                log_buf_size = resp[6] << 8
                print(f"  [{OK}] count={log_count} head={log_head} buf_size={log_buf_size}")

            ring_data.extend(resp[7:63])  # 56 bytes per page

        # Truncate to actual buffer size
        ring_data = ring_data[:log_buf_size] if log_buf_size else ring_data[:512]

        # Parse log entries from the ring buffer
        if log_count > 0:
            # Determine read start: if buffer wrapped, start from head; otherwise from 0
            if log_count >= log_buf_size:
                start = log_head  # buffer full, oldest data is at head
            else:
                start = 0

            LOG_TYPES = {
                0x01: ("HID_SETUP_ENTRY", 8),
                0x02: ("HID_SETUP_RESULT", 2),
                0x03: ("VENDOR_CMD_ENTRY", 2),
                0x04: ("USB_CONNECT", 0),
                0x05: ("EP0_XFER_START", 6),
            }

            pos = start
            bytes_read = 0
            entries = []
            max_bytes = min(log_count, log_buf_size)

            while bytes_read < max_bytes:
                type_byte = ring_data[pos % log_buf_size]
                if type_byte == 0:
                    break  # uninitialized region
                info = LOG_TYPES.get(type_byte)
                if info is None:
                    print(f"    Unknown type 0x{type_byte:02X} at offset {pos}")
                    break
                name, payload_len = info
                payload = bytearray()
                for j in range(payload_len):
                    payload.append(ring_data[(pos + 1 + j) % log_buf_size])
                entries.append((type_byte, name, payload))
                entry_size = 1 + payload_len
                pos = (pos + entry_size) % log_buf_size
                bytes_read += entry_size

            print(f"\n  Log entries ({len(entries)} parsed, {bytes_read}/{max_bytes} bytes):")
            for i, (typ, name, payload) in enumerate(entries):
                if typ == 0x01:  # HID_SETUP_ENTRY
                    bmReq = payload[0]
                    bReq = payload[1]
                    wVal = payload[2] | (payload[3] << 8)
                    wIdx = payload[4] | (payload[5] << 8)
                    wLen = payload[6] | (payload[7] << 8)
                    print(f"    [{i:3d}] {name}: bmReq=0x{bmReq:02X} bReq=0x{bReq:02X} "
                          f"wVal=0x{wVal:04X} wIdx=0x{wIdx:04X} wLen=0x{wLen:04X}")
                elif typ == 0x02:  # HID_SETUP_RESULT
                    result = payload[0]
                    bat = payload[1]
                    tag = "intercept" if result else "passthrough"
                    bat_str = f" bat={bat}" if result or bat else ""
                    print(f"    [{i:3d}] {name}: {tag}{bat_str}")
                elif typ == 0x03:  # VENDOR_CMD_ENTRY
                    print(f"    [{i:3d}] {name}: buf[0]=0x{payload[0]:02X} cmd=0x{payload[1]:02X}")
                elif typ == 0x04:  # USB_CONNECT
                    print(f"    [{i:3d}] {name}")
                elif typ == 0x05:  # EP0_XFER_START
                    buf_addr = payload[0] | (payload[1] << 8)
                    xfer_len = payload[2]
                    udev_addr = payload[3] | (payload[4] << 8)
                    print(f"    [{i:3d}] {name}: buf=0x{buf_addr:04X} len={xfer_len} "
                          f"udev=0x{udev_addr:04X}")
                else:
                    print(f"    [{i:3d}] {name}: {payload.hex()}")

            # Summary counts
            type_counts = {}
            for typ, name, _ in entries:
                type_counts[name] = type_counts.get(name, 0) + 1
            print(f"\n  Summary: {', '.join(f'{n}={c}' for n, c in sorted(type_counts.items()))}")
        else:
            print(f"  [{INFO}] Log buffer empty (count=0)")

    except Exception as e:
        print(f"  [{FAIL}] {e}")

    dev.close()


def main():
    parser = argparse.ArgumentParser(description="MonsGeek patched firmware diagnostics")
    parser.add_argument("--flash", type=Path, metavar="FILE",
                        help="Flash firmware before diagnosing")
    parser.add_argument("--build-flash", action="store_true",
                        help="Build patch, flash firmware_patched.bin, then diagnose")
    args = parser.parse_args()

    print("MonsGeek M1 V5 TMR — Patched Firmware Diagnostics")
    print(f"{'='*60}")

    if args.build_flash:
        if not do_build():
            return 1
        if not do_flash(FW_PATCHED):
            return 1
    elif args.flash:
        if not do_flash(args.flash):
            return 1

    found, interfaces = check_usb_enumeration()
    if not found:
        return 1

    check_hidraw_mapping()
    check_descriptors()
    check_power_supply()
    check_vendor_commands(interfaces)

    print(f"\n{'='*60}")
    print("Done.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
