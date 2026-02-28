#!/usr/bin/env python3
"""
Hook configuration for MonsGeek M1 V5 firmware patches.

Defines all hooks and generates the assembly stubs + patched firmware.

Usage:
    python3 hooks.py generate    # Generate hooks_gen.S
    python3 hooks.py patch       # Apply trampolines to firmware (after make compiles .S)
    python3 hooks.py validate    # Just validate hook points
"""

import struct
import sys
from pathlib import Path

# Shared hook framework at repo root
sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent.parent / "patch"))
from hook_framework import BinaryPatch, Hook, PatchProject

SCRIPT_DIR = Path(__file__).parent

# ── Hook definitions ─────────────────────────────────────────────────────────
# All hooks targeting the v407 firmware.
# Handler functions are defined in handlers.S (user code).

HOOKS = [
    Hook(
        name="vendor_dispatch",
        target=0x08013304,         # vendor_command_dispatch
        handler="handle_vendor_cmd",
        mode="filter",
        displace=4,                # push {r4-r10,lr} — 4 bytes, safe
    ),
    Hook(
        name="hid_class_setup",
        target=0x0801474C,         # hid_class_setup_handler
        handler="handle_hid_setup",
        mode="filter",
        displace=4,                # push {r4,lr} + ldr r2,[r0] — 2x 16-bit, safe
    ),
    Hook(
        name="usb_connect",
        target=0x08018690,         # usb_otg_device_connect
        handler="handle_usb_connect",
        mode="filter",
        displace=4,                # ldr.w r1,[r0,#0x804] — 4 bytes, safe
    ),
    Hook(
        name="battery_monitor",
        target=0x0801695C,         # battery_level_monitor
        handler="battery_monitor_before_hook",
        mode="before",
        displace=4,                # push {r4-r8,lr} — 4 bytes, safe
    ),
    # LED streaming: no hook for rgb_led_animate or led_render_frame (both start with
    # PC-relative LDR; can't displace). We set led_effect_mode=0 when streaming so
    # rgb_led_animate returns immediately. Commit copies stream_frame_buf to frame+DMA.
]

# ── Binary patches ───────────────────────────────────────────────────────────
# Build-time patches for battery HID descriptor support.
# Redirect hid_class_setup_handler to read from extended_rdesc buffer
# (with battery descriptor appended) instead of the original IF1 report
# descriptor in SRAM.

BINARY_PATCHES = [
    BinaryPatch(0x080147FC, b'\xAB', b'\xD9',
                "IF1 rdesc length CMP cap: 171→217"),
    BinaryPatch(0x08014800, b'\xAB', b'\xD9',
                "IF1 rdesc length MOV cap: 171→217"),
    BinaryPatch(0x0801485C, struct.pack('<I', 0x20000318), b'',
                "IF1 rdesc pointer → extended_rdesc",
                symbol='extended_rdesc'),
    # Depth monitoring: allow 2.4GHz dongle (was BT-only gate)
    # send_depth_monitor_report @ 0x08012804 checks connection type at +0x26:
    #   CMP r0, #1 (BT only) → CMP r0, #0 (any wireless: BT=1 or 2.4G=2)
    BinaryPatch(0x08012836, b'\x01', b'\x00',
                "depth monitor: CMP #1 (BT-only) → CMP #0 (any wireless)"),
]

project = PatchProject(
    hooks=HOOKS,
    binary_patches=BINARY_PATCHES,
    firmware_bin=SCRIPT_DIR / ".." / "firmware_reconstructed.bin",
    patched_bin=SCRIPT_DIR / ".." / "firmware_patched.bin",
    hook_bin=SCRIPT_DIR / "hook.bin",
    elf_path=SCRIPT_DIR / "hook.elf",
    build_dir=SCRIPT_DIR,
    engine_kwargs=dict(file_base=0x08005000),
)

if __name__ == '__main__':
    project.main()
