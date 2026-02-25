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
import subprocess
import sys
from pathlib import Path

# Shared hook framework at repo root
sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent.parent / "patch"))
from hook_framework import HookEngine, Hook

SCRIPT_DIR = Path(__file__).parent
FILE_BASE = 0x08005000  # keyboard firmware file → flash address offset

FIRMWARE_IN = SCRIPT_DIR / ".." / "firmware_reconstructed.bin"
FIRMWARE_OUT = SCRIPT_DIR / ".." / "firmware_patched.bin"
HOOKS_ASM = SCRIPT_DIR / "hooks_gen.S"
HOOK_BIN = SCRIPT_DIR / "hook.bin"
HOOK_ELF = SCRIPT_DIR / "hook.elf"

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


def build_engine() -> HookEngine:
    engine = HookEngine(FIRMWARE_IN)
    for hook in HOOKS:
        engine.add_hook(hook)
    return engine


def cmd_validate():
    engine = build_engine()
    print(engine.summary())
    print("\nAll hook points validated OK.")


def cmd_generate():
    engine = build_engine()

    # Generate stubs — handlers are in a separate file (handlers.S)
    engine.generate(HOOKS_ASM)
    print(engine.summary())
    print(f"\nGenerated: {HOOKS_ASM}")
    print(f"Now define handlers in handlers.S, then run: make")


def fix_stub_addresses(engine: HookEngine, symbols: dict[str, int]) -> None:
    """Fix hook stub addresses using actual ELF symbol addresses.

    The framework estimates stub sizes for allocation, but the linker may
    place sections at different offsets.  We use the resolved symbol table
    to update each hook before encoding B.W trampolines.
    """
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
    """Apply build-time binary patches to the firmware.

    These patches redirect the original hid_class_setup_handler to read
    from our extended_rdesc buffer (with battery descriptor appended)
    instead of the original IF1 report descriptor in SRAM.

    Patched locations in hid_class_setup_handler (0x0801474C):
      0x080147FC: cmp r2, #0xAB  → cmp r2, #0xD9   (length cap 171→217)
      0x08014800: mov r2, #0xAB  → mov r2, #0xD9   (length cap 171→217)
      0x0801485C: .word 0x20000318 → .word <extended_rdesc>  (literal pool)
    """
    extended_rdesc_addr = symbols.get('extended_rdesc')
    if extended_rdesc_addr is None:
        print("ERROR: 'extended_rdesc' symbol not found in hook.elf. "
              "Make sure it is non-static in handlers.c.", file=sys.stderr)
        sys.exit(1)

    patches = [
        # (flash_addr, old_byte, new_byte, description)
        (0x080147FC, 0xAB, 0xD9, "IF1 rdesc length CMP cap: 171→217"),
        (0x08014800, 0xAB, 0xD9, "IF1 rdesc length MOV cap: 171→217"),
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
    litpool_addr = 0x0801485C
    litpool_off = litpool_addr - FILE_BASE
    old_ptr = struct.unpack_from('<I', fw, litpool_off)[0]
    if old_ptr != 0x20000318:
        print(f"WARNING: literal pool at 0x{litpool_addr:08X} is 0x{old_ptr:08X}, "
              f"expected 0x20000318. Already patched?", file=sys.stderr)
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
