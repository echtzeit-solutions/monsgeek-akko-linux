#!/usr/bin/env python3
"""
Load Akko dongle HID-BPF program with proper hid_id setup.

The HID-BPF struct_ops requires setting hid_id before registration.
This loader:
1. Finds the matching HID device (VID 3151, PID 5038)
2. Extracts the numeric hid_id from the device path
3. Uses bpftool to load with the correct hid_id

Must be run as root.
"""

import os
import sys
import struct
import subprocess
from pathlib import Path

VID = 0x3151
PID = 0x5038
BPF_OBJ = Path("/home/florian/src-misc/monsgeek-m1-v5-tmr/iot_driver_linux/bpf/akko_dongle.bpf.o")

def find_hid_devices(vid: int, pid: int) -> list[tuple[str, int]]:
    """Find HID devices matching VID:PID, return (name, hid_id) tuples."""
    devices = []
    hid_path = Path("/sys/bus/hid/devices")

    for dev in hid_path.iterdir():
        # Format: BBBB:VVVV:PPPP.IIII (e.g., 0003:3151:5038.00B6)
        name = dev.name
        parts = name.split(":")
        if len(parts) != 3:
            continue

        try:
            dev_vid = int(parts[1], 16)
            pid_id = parts[2].split(".")
            dev_pid = int(pid_id[0], 16)
            dev_id = int(pid_id[1], 16)
        except (ValueError, IndexError):
            continue

        if dev_vid == vid and dev_pid == pid:
            devices.append((name, dev_id))

    return devices


def get_rdesc_size(dev_name: str) -> int:
    """Get the report descriptor size for a HID device."""
    rdesc_path = Path(f"/sys/bus/hid/devices/{dev_name}/report_descriptor")
    try:
        return rdesc_path.stat().st_size
    except FileNotFoundError:
        return 0


def check_already_loaded() -> bool:
    """Check if our BPF program is already loaded."""
    result = subprocess.run(
        ["bpftool", "struct_ops", "list"],
        capture_output=True,
        text=True
    )
    return "akko_dongle" in result.stdout


def unload_bpf():
    """Unload the BPF program if loaded."""
    subprocess.run(
        ["bpftool", "struct_ops", "unregister", "name", "akko_dongle"],
        capture_output=True
    )


def main():
    if os.geteuid() != 0:
        print("Error: Must run as root")
        sys.exit(1)

    if not BPF_OBJ.exists():
        print(f"Error: BPF object not found at {BPF_OBJ}")
        print("Run 'make' in iot_driver_linux/bpf/ first")
        sys.exit(1)

    # Find matching devices
    devices = find_hid_devices(VID, PID)
    if not devices:
        print(f"Error: No HID devices found with VID {VID:04x} PID {PID:04x}")
        sys.exit(1)

    print(f"Found {len(devices)} matching device(s):")
    for name, hid_id in devices:
        rdesc_size = get_rdesc_size(name)
        print(f"  {name}: hid_id={hid_id} (0x{hid_id:04x}), rdesc_size={rdesc_size}")

    # Find the vendor Feature report interface (small descriptor starting with 06 FF FF)
    target = None
    for name, hid_id in devices:
        rdesc_size = get_rdesc_size(name)
        if 20 <= rdesc_size <= 24:
            # Read first 3 bytes to check for vendor page
            rdesc_path = Path(f"/sys/bus/hid/devices/{name}/report_descriptor")
            with open(rdesc_path, "rb") as f:
                header = f.read(3)
            if header == b'\x06\xff\xff':
                target = (name, hid_id)
                break

    if not target:
        print("Error: Could not find vendor Feature report interface")
        print("Expected: ~20 byte descriptor starting with 06 FF FF")
        sys.exit(1)

    name, hid_id = target
    print(f"\nTarget device: {name} (hid_id={hid_id})")

    # Check if already loaded
    if check_already_loaded():
        print("BPF program already loaded, unloading first...")
        unload_bpf()

    # The problem is that bpftool struct_ops register doesn't let us set hid_id
    # before loading. The HID-BPF subsystem expects this to be set.
    #
    # Looking at the kernel code, the hid_id field in the struct_ops map
    # needs to be populated before the struct_ops is registered.
    #
    # For now, let's try a workaround: use bpftool with BTF to patch the map.
    # Actually, this is complex - let's try with libbpf directly via ctypes
    # or use the udev-hid-bpf approach.
    #
    # Alternative: Try loading and see if the kernel probe function can
    # match by VID/PID from the .hid_bpf_config section.

    print(f"\nLoading BPF program for hid_id={hid_id}...")

    # Try direct registration first - kernel may use .hid_bpf_config for matching
    result = subprocess.run(
        ["bpftool", "struct_ops", "register", str(BPF_OBJ)],
        capture_output=True,
        text=True
    )

    if result.returncode != 0:
        print(f"bpftool failed: {result.stderr}")

        # The issue is we can't set hid_id. Let's try a different approach:
        # Use skeleton-like loading via raw libbpf calls
        print("\nNote: bpftool struct_ops doesn't support setting hid_id.")
        print("This requires a proper loader using libbpf or aya.")
        print("\nAlternative: Use udev-hid-bpf framework")
        print("  Install: https://gitlab.freedesktop.org/libevdev/udev-hid-bpf")
        sys.exit(1)

    print("Success!")

    # Wait for kernel to reprobe
    import time
    time.sleep(1)

    # Check for power_supply
    ps_path = Path("/sys/class/power_supply")
    for ps in ps_path.iterdir():
        if "hid" in ps.name.lower() and "3151" in ps.name:
            print(f"\npower_supply created: {ps.name}")
            capacity = (ps / "capacity").read_text().strip() if (ps / "capacity").exists() else "N/A"
            status = (ps / "status").read_text().strip() if (ps / "status").exists() else "N/A"
            print(f"  capacity: {capacity}%")
            print(f"  status: {status}")
            break
    else:
        print("\nWarning: power_supply not created - check dmesg")


if __name__ == "__main__":
    main()
