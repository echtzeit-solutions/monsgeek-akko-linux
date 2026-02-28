#!/usr/bin/env python3
"""
Hook configuration for dongle firmware battery HID patch.

Defines hooks and binary patches for exposing keyboard battery level
via the dongle's USB HID interface.

Usage:
    python3 hooks.py generate    # Generate hooks_gen.S
    python3 hooks.py patch       # Apply trampolines + binary patches
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

HOOKS = [
    Hook(
        name="usb_init",
        target=0x080069D8,         # usb_init — populate descriptors before enumeration
        handler="handle_usb_init",
        mode="before",
        displace=4,                # push {r3,lr} + movs r1,#1 — 4 bytes, safe
    ),
    Hook(
        name="hid_class_setup",
        target=0x080071B4,         # hid_class_setup_handler
        handler="handle_hid_setup",
        mode="filter",
        displace=4,                # PUSH.W {r4-r10,lr} — 4 bytes, safe
    ),
    Hook(
        name="rf_packet_dispatch",
        target=0x080059FC,         # rf_packet_dispatch — push battery changes to host
        handler="handle_rf_dispatch",
        mode="before",
        displace=4,                # PUSH.W {r4-r8,lr} — 4 bytes, safe
    ),
]

# ── Binary patches ───────────────────────────────────────────────────────────
# Build-time patches for battery HID descriptor support on the dongle.

BINARY_PATCHES = [
    BinaryPatch(0x080072C6, b'\xAB', b'\xD9',
                "IF1 rdesc length CMP cap: 171→217"),
    BinaryPatch(0x080072CA, b'\xAB', b'\xD9',
                "IF1 rdesc length MOV cap: 171→217"),
    BinaryPatch(0x080073C8, struct.pack('<I', 0x200001EC), b'',
                "IF1 rdesc pointer → extended_rdesc",
                symbol='extended_rdesc'),
]

project = PatchProject(
    hooks=HOOKS,
    binary_patches=BINARY_PATCHES,
    firmware_bin=SCRIPT_DIR / ".." / "dfu_dumps" / "dongle_working_256k.bin",
    patched_bin=SCRIPT_DIR / ".." / "dfu_dumps" / "dongle_patched_256k.bin",
    hook_bin=SCRIPT_DIR / "hook.bin",
    elf_path=SCRIPT_DIR / "hook.elf",
    build_dir=SCRIPT_DIR,
    engine_kwargs=dict(
        file_base=0x08000000,
        patch_zone_start=0x0800B000,
        patch_zone_end=0x0800D7FF,
    ),
)

if __name__ == '__main__':
    project.main()
