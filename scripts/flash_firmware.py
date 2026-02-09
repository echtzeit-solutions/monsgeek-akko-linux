#!/usr/bin/env python3
"""
Flash firmware to MonsGeek M1 V5 TMR keyboard via RY bootloader protocol.

Protocol (reverse-engineered from ry_upgrade.exe + uhid_dummy_device.py):
1. Open normal device: VID=0x3151, PID=0x5030, usage_page=0xFFFF (IF2)
2. Send ENTER_BOOTLOADER: SET_REPORT [0x7F, 0x55, 0xAA, 0x55, 0xAA, ...]
3. Wait for re-enumeration (device reboots as PID=0x502A, usage_page=0xFF01)
4. Send FW_TRANSFER_START: [0xBA, 0xC0, chunk_count_lo, chunk_count_hi, size_lo, size_mid, size_hi, ...]
5. Read ack (GET_REPORT)
6. Send firmware in 64-byte chunks
7. Send FW_TRANSFER_COMPLETE: [0xBA, 0xC2, ...]
8. Device reboots to normal mode

Usage:
  sudo python3 flash_firmware.py firmware_patched.bin
  sudo python3 flash_firmware.py --dry-run firmware_patched.bin  # verify protocol only
  sudo python3 flash_firmware.py --skip-bootloader firmware_patched.bin  # already in bootloader
"""

import argparse
import sys
import time
from pathlib import Path

try:
    import hid
except ImportError:
    print("ERROR: hidapi not found. Install with: pip install hidapi", file=sys.stderr)
    sys.exit(1)

# USB IDs
VID = 0x3151
PID_NORMAL = 0x5030
PID_BOOTLOADER = 0x502A

# HID usage pages
USAGE_PAGE_VENDOR = 0xFFFF   # Normal mode IF2
USAGE_PAGE_BOOT = 0xFF01     # Bootloader mode

# Report size (no report ID in bootloader mode)
REPORT_SIZE = 64
CHUNK_SIZE = 64


def find_device(vid: int, pid: int, usage_page: int | None = None) -> dict | None:
    """Find a HID device matching VID/PID and optional usage page."""
    for dev in hid.enumerate(vid, pid):
        if usage_page is None or dev.get("usage_page") == usage_page:
            return dev
    return None


def open_device(vid: int, pid: int, usage_page: int | None = None) -> hid.Device:
    """Open a HID device, retrying briefly for enumeration delays."""
    dev_info = find_device(vid, pid, usage_page)
    if dev_info is None:
        raise RuntimeError(
            f"Device not found: VID=0x{vid:04x} PID=0x{pid:04x}"
            + (f" usage_page=0x{usage_page:04x}" if usage_page else "")
        )
    d = hid.Device(path=dev_info["path"])
    return d


def build_vendor_report(cmd: int, data: bytes = b"") -> bytes:
    """Build a 65-byte vendor feature report with Bit7 checksum.

    Layout: [report_id=0x00] [cmd] [data...] [checksum at byte 8] [zeros...]
    Checksum = 0xFF - sum(bytes 1-7) & 0xFF, placed at byte 8.
    """
    buf = bytearray(REPORT_SIZE + 1)  # 65 bytes
    buf[0] = 0x00  # report ID
    buf[1] = cmd
    for i, b in enumerate(data[:6]):
        buf[2 + i] = b
    checksum_sum = sum(buf[1:8]) & 0xFF
    buf[8] = (0xFF - checksum_sum) & 0xFF
    return bytes(buf)


def enter_bootloader(dev: hid.Device) -> None:
    """Send ISP_PREPARE + ENTER_BOOTLOADER commands."""
    # ISP_PREPARE (0xC5, param 0x3A) — tells firmware to prepare for update
    dev.send_feature_report(build_vendor_report(0xC5, b"\x3A"))
    print("Sent ISP_PREPARE (0xC5)")
    time.sleep(0.05)

    # ENTER_BOOTLOADER (0x7F + 55AA55AA magic + Bit7 checksum)
    dev.send_feature_report(build_vendor_report(0x7F, b"\x55\xAA\x55\xAA"))
    print("Sent ENTER_BOOTLOADER (0x7F + 55AA55AA + checksum)")


def wait_for_bootloader(timeout: float = 10.0) -> hid.Device:
    """Wait for the bootloader device to appear after reboot."""
    print(f"Waiting up to {timeout:.0f}s for bootloader device (PID=0x{PID_BOOTLOADER:04x})...")
    start = time.monotonic()
    while time.monotonic() - start < timeout:
        dev_info = find_device(VID, PID_BOOTLOADER, USAGE_PAGE_BOOT)
        if dev_info is not None:
            # Small delay for device to stabilize
            time.sleep(0.5)
            d = hid.Device(path=dev_info["path"])
            print(f"Bootloader device found after {time.monotonic() - start:.1f}s")
            return d
        time.sleep(0.3)
    raise RuntimeError("Timeout waiting for bootloader device")


def flash_firmware(dev: hid.Device, firmware: bytes) -> None:
    """Flash firmware via RY bootloader protocol."""
    total_size = len(firmware)
    chunk_count = (total_size + CHUNK_SIZE - 1) // CHUNK_SIZE

    print(f"Firmware: {total_size} bytes, {chunk_count} chunks of {CHUNK_SIZE} bytes")

    # Step 1: Send FW_TRANSFER_START
    # Format: [0xBA, 0xC0, chunk_count_lo, chunk_count_hi, size_lo, size_mid, size_hi, 0, ...]
    start_buf = bytearray(REPORT_SIZE)
    start_buf[0] = 0xBA
    start_buf[1] = 0xC0  # FW_TRANSFER_START marker
    start_buf[2] = chunk_count & 0xFF
    start_buf[3] = (chunk_count >> 8) & 0xFF
    start_buf[4] = total_size & 0xFF
    start_buf[5] = (total_size >> 8) & 0xFF
    start_buf[6] = (total_size >> 16) & 0xFF

    dev.send_feature_report(bytes(start_buf))
    print(f"Sent FW_TRANSFER_START: chunks={chunk_count}, size={total_size}")

    # Read ack
    ack = dev.get_feature_report(0, REPORT_SIZE + 1)
    print(f"Ack: {ack[:8].hex() if ack else 'empty'}")

    # Step 2: Send firmware chunks
    for i in range(chunk_count):
        offset = i * CHUNK_SIZE
        chunk = firmware[offset:offset + CHUNK_SIZE]

        # Pad last chunk if needed
        if len(chunk) < CHUNK_SIZE:
            chunk = chunk + b"\xff" * (CHUNK_SIZE - len(chunk))

        dev.send_feature_report(bytes(chunk))

        # Progress indicator
        if (i + 1) % 100 == 0 or i == chunk_count - 1:
            pct = (i + 1) * 100 // chunk_count
            print(f"  Chunk {i + 1}/{chunk_count} ({pct}%)")

    # Step 3: Send FW_TRANSFER_COMPLETE
    complete_buf = bytearray(REPORT_SIZE)
    complete_buf[0] = 0xBA
    complete_buf[1] = 0xC2  # FW_TRANSFER_COMPLETE marker
    # Bytes 8-11: total size (u32 LE)
    complete_buf[8] = total_size & 0xFF
    complete_buf[9] = (total_size >> 8) & 0xFF
    complete_buf[10] = (total_size >> 16) & 0xFF
    complete_buf[11] = (total_size >> 24) & 0xFF

    dev.send_feature_report(bytes(complete_buf))
    print("Sent FW_TRANSFER_COMPLETE")

    # Read final ack
    try:
        ack = dev.get_feature_report(0, REPORT_SIZE + 1)
        print(f"Final ack: {ack[:8].hex() if ack else 'empty'}")
    except Exception:
        print("Device disconnected (expected — rebooting to normal mode)")


def verify_cmd_fb(dev: hid.Device) -> bool:
    """Send vendor command 0xFB and check for 'PATCHED' response."""
    buf = bytearray(REPORT_SIZE + 1)
    buf[0] = 0x00  # report ID
    buf[1] = 0xFB  # our custom command
    # Checksum: make bytes 2-9 sum + cmd = 0xFF
    # cmd_buf layout: [flag, cmd_ack, cmd_id, ...]
    # We set buf[1]=flag area, buf[2]=cmd area... wait.
    # Actually the vendor protocol uses: buf[0]=report_id, then hidapi strips it.
    # The firmware sees: cmd_buf[0]=flag (non-zero=pending), cmd_buf[1]=ack, cmd_buf[2]=cmd_id
    # So we need: buf[0]=report_id, buf[1]=non-zero flag, buf[2]=0x00 (ack area), buf[3]=0xFB (cmd)
    # Actually looking at uhid_dummy_device.py parse_cmd():
    #   if report[0] == 0x00 and len >= 2: return report[1]
    # And the firmware reads cmd_buf[2] as the command ID.
    # In SET_REPORT: buf[0]=report_id(0), then kernel delivers buf[1:] as the report data.
    # The firmware's cmd_buf[0] is the "pending" flag, cmd_buf[2] is cmd_id.
    # So send_feature_report: [report_id=0, pending=1, ack=0, cmd_id=0xFB, ...]
    buf[0] = 0x00
    buf[1] = 0x01  # pending flag
    buf[2] = 0x00  # ack area
    buf[3] = 0xFB  # command ID

    dev.send_feature_report(bytes(buf))
    print("Sent cmd 0xFB")

    time.sleep(0.05)  # give firmware time to process

    resp = dev.get_feature_report(0, REPORT_SIZE + 1)
    if resp and len(resp) > 4:
        # Response should have "PATCHED" starting at byte offset 4 (cmd_buf[3])
        text = bytes(resp[4:12]).split(b"\x00")[0].decode("ascii", errors="replace")
        print(f"Response: '{text}'")
        if text == "PATCHED":
            print("SUCCESS: Firmware patch verified!")
            return True
    print(f"Response bytes: {resp[:16].hex() if resp else 'empty'}")
    return False


def main() -> int:
    parser = argparse.ArgumentParser(description="Flash firmware to MonsGeek keyboard")
    parser.add_argument("firmware", type=Path, help="Firmware binary to flash")
    parser.add_argument("--dry-run", action="store_true", help="Print what would be done")
    parser.add_argument(
        "--skip-bootloader",
        action="store_true",
        help="Device is already in bootloader mode",
    )
    parser.add_argument(
        "--verify-only",
        action="store_true",
        help="Only send cmd 0xFB to verify the patch",
    )
    args = parser.parse_args()

    if args.verify_only:
        print("Verifying patch (sending cmd 0xFB)...")
        dev = open_device(VID, PID_NORMAL, USAGE_PAGE_VENDOR)
        ok = verify_cmd_fb(dev)
        dev.close()
        return 0 if ok else 1

    if not args.firmware.exists():
        print(f"ERROR: {args.firmware} not found", file=sys.stderr)
        return 1

    firmware = args.firmware.read_bytes()
    print(f"Firmware file: {args.firmware} ({len(firmware)} bytes)")

    if args.dry_run:
        chunk_count = (len(firmware) + CHUNK_SIZE - 1) // CHUNK_SIZE
        print(f"[DRY RUN] Would flash {len(firmware)} bytes in {chunk_count} chunks")
        print(f"[DRY RUN] Step 1: Open VID=0x{VID:04x} PID=0x{PID_NORMAL:04x}")
        print(f"[DRY RUN] Step 2: Send ENTER_BOOTLOADER")
        print(f"[DRY RUN] Step 3: Wait for PID=0x{PID_BOOTLOADER:04x}")
        print(f"[DRY RUN] Step 4: Send FW_TRANSFER_START")
        print(f"[DRY RUN] Step 5: Send {chunk_count} chunks")
        print(f"[DRY RUN] Step 6: Send FW_TRANSFER_COMPLETE")
        return 0

    if args.skip_bootloader:
        print("Skipping bootloader entry (--skip-bootloader)")
        boot_dev = wait_for_bootloader(timeout=5.0)
    else:
        # Step 1: Enter bootloader
        print(f"Opening normal device (VID=0x{VID:04x} PID=0x{PID_NORMAL:04x})...")
        normal_dev = open_device(VID, PID_NORMAL, USAGE_PAGE_VENDOR)
        enter_bootloader(normal_dev)
        normal_dev.close()

        # Step 2: Wait for bootloader
        time.sleep(1)  # brief pause before scanning
        boot_dev = wait_for_bootloader()

    # Step 3: Flash
    flash_firmware(boot_dev, firmware)
    boot_dev.close()

    print("\nFlash complete! Device should reboot to normal mode.")
    print("To verify the patch: python3 flash_firmware.py --verify-only dummy")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
