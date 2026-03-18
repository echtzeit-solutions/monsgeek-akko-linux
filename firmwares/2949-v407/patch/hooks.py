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
    Hook(
        name="dongle_reports",
        target=0x080174C0,         # build_dongle_reports
        handler="dongle_reports_before_hook",
        mode="before",
        displace=4,                # push {r4-r12,lr} — 4 bytes, safe
    ),
    # LED overlay: BL-patch the frame→DMA memcpy so our blend function runs every frame.
    # No hook needed — rgb_led_animate and led_render_frame run normally.
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
    # Depth monitoring: remove 8KHz-over-wireless gate in send_depth_monitor_report
    # Original: CMP r0,#6; IT EQ; POP.EQ — bails if 8KHz and not USB Full Speed
    # Patched: 4× NOP — allows depth reporting at any polling rate
    BinaryPatch(0x0801282A, b'\x06\x28\x08\xbf\xbd\xe8\xf0\x81',
                b'\x00\xbf\x00\xbf\x00\xbf\x00\xbf',
                "depth monitor: NOP 8KHz gate (CMP+IT+POP → 4×NOP)"),
    # Depth monitoring: remove BT-only gate in send_depth_monitor_report
    # Original: CMP r0,#1; IT NE; POP.W NE {r4-r8,pc} — bails if not BT
    # Patched: 4× NOP — allows depth reporting in all modes (BT, 2.4G, USB)
    BinaryPatch(0x08012836, b'\x01\x28\x18\xbf\xbd\xe8\xf0\x81',
                b'\x00\xbf\x00\xbf\x00\xbf\x00\xbf',
                "depth monitor: NOP BT-only gate (CMP+IT+POP → 4×NOP)"),
    # ── Consumer over dongle ──────────────────────────────────────────────
    # Stock firmware bug: hid_report_check_send block 3 zeros the consumer
    # buffer (0x20000027) AND sets bitmap |= 0x04.  build_dongle_reports
    # then reads zeros instead of the actual consumer usage code.  The
    # timer gate in build_dongle_reports makes this worse — it defers the
    # send, giving block 3 even more time to wipe the data.
    #
    # Fix: NOP block 3's action path (zero + bitmap set, 14 bytes → 7×NOP).
    # With this NOP, keycode_dispatch case 3 is the only writer of both the
    # consumer buffer and bitmap bit 0x04.  build_dongle_reports reads the
    # actual data.  dongle_reports_before_hook handles auto-release: after
    # a press is sent, it zeros the buffer and re-sets 0x04 to guarantee a
    # release report on the next cycle.
    BinaryPatch(0x080124FA,
                b'\x08\x48\x01\x80\x81\x70\x20\x78\x40\xf0\x04\x00\x20\x70',
                b'\x00\xbf\x00\xbf\x00\xbf\x00\xbf\x00\xbf\x00\xbf\x00\xbf',
                "hid_report_check_send blk3: NOP consumer zero+bitmap (7×NOP)"),
    # ── Boot-time config validation (bugs 4-5 from oob_hazards.txt) ──────
    # config_load_all: replace ldr r4,[pc,#0xEC]; ldrb r0,[r4,#0] with
    # BL validate_config_after_load (returns r4=g_fw_config, r0=profile_id)
    BinaryPatch(0x08012376, bytes.fromhex('3b4c2078'), b'',
                "config_load_all: validate profile_id+led_effect_mode after flash read",
                bl_symbol='validate_config_after_load'),
    # ── LED overlay blend ────────────────────────────────────────────────
    # Replace memcpy(dma_buf, frame_buf, 0x7b0) in firmware_main's scan loop
    # with our blend function that copies then applies the additive overlay.
    BinaryPatch(0x080161A8, bytes.fromhex('eff75cfa'), b'',
                "firmware_main: overlay blend in frame→DMA memcpy",
                bl_symbol='led_overlay_memcpy_and_blend'),
]

# ── Memory map ───────────────────────────────────────────────────────────────
# Flash regions auto-derived from symbols.json memory_blocks.
# SRAM regions resolved from Ghidra labels (symbol name → display name).
# Addresses come from the export — no hardcoded addresses needed.

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
    firmware_bin=SCRIPT_DIR / ".." / "firmware_reconstructed.bin",
    patched_bin=SCRIPT_DIR / ".." / "firmware_patched.bin",
    hook_bin=SCRIPT_DIR / "hook.bin",
    elf_path=SCRIPT_DIR / "hook.elf",
    build_dir=SCRIPT_DIR,
    engine_kwargs=dict(file_base=0x08005000),
    sram_landmarks=SRAM_LANDMARKS,
    flash_size=256 * 1024,   # AT32F405RCT7
    sram_size=96 * 1024,     # 0x20000000-0x20017FFF
    memmap_svg=SCRIPT_DIR / ".." / ".." / ".." / "docs" / "memmap-keyboard.svg",
)

if __name__ == '__main__':
    project.main()
