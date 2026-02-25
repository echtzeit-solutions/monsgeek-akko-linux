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
import subprocess
import sys
from pathlib import Path

# Shared hook framework at repo root
sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent.parent / "patch"))
from hook_framework import HookEngine, Hook

SCRIPT_DIR = Path(__file__).parent
FILE_BASE = 0x08000000  # full 256KB dump including bootloader

FIRMWARE_IN = SCRIPT_DIR / ".." / "dfu_dumps" / "dongle_working_256k.bin"
FIRMWARE_OUT = SCRIPT_DIR / ".." / "dfu_dumps" / "dongle_patched_256k.bin"
HOOKS_ASM = SCRIPT_DIR / "hooks_gen.S"
HOOK_BIN = SCRIPT_DIR / "hook.bin"
HOOK_ELF = SCRIPT_DIR / "hook.elf"

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
]


def build_engine() -> HookEngine:
    engine = HookEngine(FIRMWARE_IN,
                        file_base=FILE_BASE,
                        patch_zone_start=0x0800B000,
                        patch_zone_end=0x0800D7FF)
    for hook in HOOKS:
        engine.add_hook(hook)
    return engine


def cmd_validate():
    engine = build_engine()
    print(engine.summary())
    print("\nAll hook points validated OK.")


def cmd_generate():
    engine = build_engine()
    engine.generate(HOOKS_ASM)
    print(engine.summary())
    print(f"\nGenerated: {HOOKS_ASM}")
    print(f"Now define handlers in handlers.S, then run: make")


def fix_stub_addresses(engine: HookEngine, symbols: dict[str, int]) -> None:
    """Fix hook stub addresses using actual ELF symbol addresses."""
    for hook in engine.hooks:
        sym = f"_hook_{hook.name}_stub"
        if sym in symbols:
            actual = symbols[sym]
            if hook._stub_addr != actual:
                print(f"  Fix {hook.name} stub: "
                      f"0x{hook._stub_addr:08X} → 0x{actual:08X}")
                hook._stub_addr = actual


def read_elf_symbols() -> dict[str, int]:
    """Read all symbol addresses from hook.elf via nm."""
    if not HOOK_ELF.exists():
        return {}
    result = subprocess.run(
        ['arm-none-eabi-nm', str(HOOK_ELF)],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        return {}
    symbols = {}
    for line in result.stdout.strip().split('\n'):
        parts = line.strip().split()
        if len(parts) == 3:
            symbols[parts[2]] = int(parts[0], 16)
    return symbols


def apply_binary_patches(fw: bytearray, symbols: dict[str, int]) -> None:
    """Apply build-time binary patches to the dongle firmware.

    Patches in hid_class_setup_handler (0x080071B4):
      0x080072C6: cmp r0, #0xAB → cmp r0, #0xD9  (length cap 171→217)
      0x080072CA: movs r0, #0xAB → movs r0, #0xD9 (length cap 171→217)
      0x080073C8: .word 0x200001EC → .word <extended_rdesc>  (literal pool)
    """
    extended_rdesc_addr = symbols.get('extended_rdesc')
    if extended_rdesc_addr is None:
        print("ERROR: 'extended_rdesc' symbol not found in hook.elf. "
              "Make sure it is non-static in handlers.c.", file=sys.stderr)
        sys.exit(1)

    patches = [
        # (flash_addr, old_byte, new_byte, description)
        (0x080072C6, 0xAB, 0xD9, "IF1 rdesc length CMP cap: 171→217"),
        (0x080072CA, 0xAB, 0xD9, "IF1 rdesc length MOV cap: 171→217"),
    ]

    for flash_addr, old, new, desc in patches:
        off = flash_addr - FILE_BASE
        if fw[off] != old:
            print(f"WARNING: byte at 0x{flash_addr:08X} is 0x{fw[off]:02X}, "
                  f"expected 0x{old:02X}. Already patched?", file=sys.stderr)
        else:
            fw[off] = new
            print(f"  Patch: 0x{flash_addr:08X} [0x{old:02X}→0x{new:02X}] {desc}")

    # Patch literal pool: IF1 report descriptor pointer → extended_rdesc
    litpool_addr = 0x080073C8
    litpool_off = litpool_addr - FILE_BASE
    old_ptr = struct.unpack_from('<I', fw, litpool_off)[0]
    if old_ptr != 0x200001EC:
        print(f"WARNING: literal pool at 0x{litpool_addr:08X} is 0x{old_ptr:08X}, "
              f"expected 0x200001EC. Already patched?", file=sys.stderr)
    struct.pack_into('<I', fw, litpool_off, extended_rdesc_addr)
    print(f"  Patch: 0x{litpool_addr:08X} [0x{old_ptr:08X}→0x{extended_rdesc_addr:08X}] "
          f"IF1 rdesc pointer → extended_rdesc")


def cmd_patch():
    engine = build_engine()
    if not HOOK_BIN.exists():
        print(f"ERROR: {HOOK_BIN} not found. Run 'make' first.", file=sys.stderr)
        sys.exit(1)

    symbols = read_elf_symbols()
    fix_stub_addresses(engine, symbols)
    engine.patch(FIRMWARE_OUT, HOOK_BIN)

    # Apply binary patches to the already-written output
    fw = bytearray(FIRMWARE_OUT.read_bytes())
    apply_binary_patches(fw, symbols)
    FIRMWARE_OUT.write_bytes(fw)
    print(f"Binary patches applied to {FIRMWARE_OUT}")

    print(engine.summary())


def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <validate|generate|patch>")
        sys.exit(1)

    cmd = sys.argv[1]
    if cmd == "validate":
        cmd_validate()
    elif cmd == "generate":
        cmd_generate()
    elif cmd == "patch":
        cmd_patch()
    else:
        print(f"Unknown command: {cmd}")
        sys.exit(1)


if __name__ == '__main__':
    main()
