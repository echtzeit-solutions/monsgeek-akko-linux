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
    # Bug #1 (dongle sub-type): Fixed in dongle_reports_before_hook —
    # reroutes encoder consumer data from bit 0x04 (sub=3/mouse) to
    # bit 0x20 (sub=1/consumer) with correct Report ID 3.
    #
    # Bug #2 (hid_report_check_send block 1): checks buffer at 0x20000054;
    # when non-zero, zeros it AND sets bit 0x01 (keyboard sub=0) — spurious
    # keyboard report.  Fix: NOP the STRB that writes bitmap |= 0x01.
    BinaryPatch(0x080124B2, b'\x20\x70', b'\x00\xbf',
                "hid_report_check_send blk1: NOP bitmap |= 0x01"),
    #
    # Bug #3 (hid_report_check_send block 2): checks consumer_report_t at
    # 0x2000004C; when non-zero, zeros it via dma_struct_default THEN sets
    # bit 0x20.  If build_dongle_reports' timer gate deferred the send,
    # the consumer data is wiped before it can be read.  This is a stock
    # firmware bug (never triggered because encoder goes through 6KRO/bit
    # 0x04, not consumer_report_t).  Fix: NOP the entire action path
    # (ldr+bl+ldrb+orr+strb = 14 bytes → 7×NOP).  Our hook handles cleanup.
    BinaryPatch(0x080124DA,
                b'\x0e\x48\x01\xf0\x26\xfc\x20\x78\x40\xf0\x20\x00\x20\x70',
                b'\x00\xbf\x00\xbf\x00\xbf\x00\xbf\x00\xbf\x00\xbf\x00\xbf',
                "hid_report_check_send blk2: NOP consumer zero+bitmap (7×NOP)"),
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
