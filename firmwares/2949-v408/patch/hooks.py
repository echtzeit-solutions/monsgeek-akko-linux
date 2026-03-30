#!/usr/bin/env python3
"""
Hook configuration for MonsGeek M1 V5 firmware v408 patches.

Ported from v407 hooks.py with addresses adjusted for v408.
Address shifts verified by byte-matching v407 instruction prologues in v408.

Shift zones:
  < 0x08012d40:    0    (before new bt_flags code)
  0x08012d40-0x08015e31: +36   (bt_flags + depth_mon changes)
  >= 0x08015e31:  +140  (LED + USB descriptor changes)

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
# All hooks targeting the v408 firmware.
# Handler functions are defined in handlers.c / handlers.S (symlinked from v407).

HOOKS = [
    Hook(
        name="vendor_dispatch",
        target=0x08013328,         # vendor_command_dispatch (v407: 0x08013304, +36)
        handler="handle_vendor_cmd",
        mode="filter",
        displace=4,                # push {r4-r10,lr} — 4 bytes, safe
    ),
    Hook(
        name="hid_class_setup",
        target=0x08014770,         # hid_class_setup_handler (v407: 0x0801474C, +36)
        handler="handle_hid_setup",
        mode="filter",
        displace=4,                # push {r4,lr} + ldr r2,[r0] — 2x 16-bit, safe
    ),
    Hook(
        name="usb_connect",
        target=0x0801871C,         # usb_otg_device_connect (v407: 0x08018690, +140)
        handler="handle_usb_connect",
        mode="filter",
        displace=4,                # ldr.w r1,[r0,#0x804] — 4 bytes, safe
    ),
    Hook(
        name="battery_monitor",
        target=0x080169E8,         # battery_level_monitor (v407: 0x0801695C, +140)
        handler="battery_monitor_before_hook",
        mode="before",
        displace=4,                # push {r4-r8,lr} — 4 bytes, safe
    ),
    Hook(
        name="dongle_reports",
        target=0x0801754C,         # build_dongle_reports (v407: 0x080174C0, +140)
        handler="dongle_reports_before_hook",
        mode="before",
        displace=4,                # push {r4-r12,lr} — 4 bytes, safe
    ),
    Hook(
        name="wireless_sleep",
        target=0x08016C0C,         # wireless_sleep_loop (v407: 0x08016B80, +140)
        handler="wireless_sleep_before_hook",
        mode="before",
        displace=4,                # push {r3-r11,lr} — 4 bytes (wide Thumb2), safe
    ),
    Hook(
        name="usb_suspend",
        target=0x08014330,         # usb_suspend_handler (v407: 0x0801430C, +36)
        handler="usb_suspend_before_hook",
        mode="before",
        displace=4,                # push {r3-r11,lr} — 4 bytes (wide Thumb2), safe
    ),
]

# ── Binary patches ───────────────────────────────────────────────────────────
# Same patches as v407, addresses shifted for v408.

BINARY_PATCHES = [
    BinaryPatch(0x08014820, b'\xAB', b'\xD9',
                "IF1 rdesc length CMP cap: 171→217"),  # v407: 0x080147FC, +36
    BinaryPatch(0x08014824, b'\xAB', b'\xD9',
                "IF1 rdesc length MOV cap: 171→217"),  # v407: 0x08014800, +36
    BinaryPatch(0x08014880, struct.pack('<I', 0x20000318), b'',
                "IF1 rdesc pointer → extended_rdesc",   # v407: 0x0801485C, +36
                symbol='extended_rdesc'),
    # Depth monitoring: remove 8KHz-over-wireless gate
    BinaryPatch(0x0801282A, b'\x06\x28\x08\xbf\xbd\xe8\xf0\x81',
                b'\x00\xbf\x00\xbf\x00\xbf\x00\xbf',
                "depth monitor: NOP 8KHz gate (CMP+IT+POP → 4×NOP)"),  # v407: same, shift 0
    # Depth monitoring: remove BT-only gate
    BinaryPatch(0x08012836, b'\x01\x28\x18\xbf\xbd\xe8\xf0\x81',
                b'\x00\xbf\x00\xbf\x00\xbf\x00\xbf',
                "depth monitor: NOP BT-only gate (CMP+IT+POP → 4×NOP)"),  # v407: same, shift 0
    # Consumer over dongle: NOP block 3's action path
    BinaryPatch(0x080124FA,
                b'\x08\x48\x01\x80\x81\x70\x20\x78\x40\xf0\x04\x00\x20\x70',
                b'\x00\xbf\x00\xbf\x00\xbf\x00\xbf\x00\xbf\x00\xbf\x00\xbf',
                "hid_report_check_send blk3: NOP consumer zero+bitmap (7×NOP)"),  # v407: same, shift 0
    # Boot-time config validation
    BinaryPatch(0x08012376, bytes.fromhex('3b4c2078'), b'',
                "config_load_all: validate profile_id+led_effect_mode after flash read",
                bl_symbol='validate_config_after_load'),  # v407: same, shift 0
    # LED overlay blend
    BinaryPatch(0x08016234, bytes.fromhex('eff716fa'), b'',
                "firmware_main: overlay blend in frame→DMA memcpy",
                bl_symbol='led_overlay_memcpy_and_blend'),  # v407: 0x080161A8, +140
]

# ── Memory map ───────────────────────────────────────────────────────────────
SRAM_LANDMARKS = [
    ("g_systick_saved_ctrl",  "HID + descriptors"),
    ("g_desct_device",        "Kbd state"),
    ("g_led_dma_buf",         "LED DMA buf"),
    ("g_led_frame_buf",       "LED frame buf"),
    ("g_per_key_state",       "Per-key + WS2812"),
    ("g_mag_engine_state",    "Mag engine"),
    ("g_usb_device",          "USB + RF + config"),
    ("g_vendor_cmd_buffer",   "Vendor cmd buf"),
    ("g_led_anim_state",      "LED anim state"),
]

project = PatchProject(
    hooks=HOOKS,
    binary_patches=BINARY_PATCHES,
    firmware_bin=SCRIPT_DIR / ".." / "firmware_2949_v408.bin",
    patched_bin=SCRIPT_DIR / ".." / "firmware_patched.bin",
    hook_bin=SCRIPT_DIR / "hook.bin",
    elf_path=SCRIPT_DIR / "hook.elf",
    build_dir=SCRIPT_DIR,
    engine_kwargs=dict(file_base=0x08005000),
    sram_landmarks=SRAM_LANDMARKS,
    flash_size=256 * 1024,
    sram_size=96 * 1024,
)

if __name__ == '__main__':
    project.main()
