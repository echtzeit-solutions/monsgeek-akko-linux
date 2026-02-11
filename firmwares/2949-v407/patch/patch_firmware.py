#!/usr/bin/env python3
"""
Splice hook.bin into firmware_reconstructed.bin and patch the branch.

This script:
1. Reads the stock firmware (132736 bytes)
2. Pads to offset 0x20800 (= real flash 0x08025800 = patch zone)
3. Splices in the compiled hook.bin at that offset
4. Encodes a Thumb-2 B.W instruction at offset 0xe304
   (= vendor_command_dispatch) targeting real address 0x08025800
5. Writes firmware_patched.bin

Address mapping:
  Ghidra addr  = file_offset + 0x08000000
  Real flash   = Ghidra addr + 0x5000
  file_offset  = Ghidra addr - 0x08000000
"""

import struct
import sys
from pathlib import Path

# Paths
SCRIPT_DIR = Path(__file__).parent
FIRMWARE_IN = SCRIPT_DIR / ".." / "firmware_reconstructed.bin"
HOOK_BIN = SCRIPT_DIR / "hook.bin"
FIRMWARE_OUT = SCRIPT_DIR / ".." / "firmware_patched.bin"

# Addresses (file offsets, i.e., Ghidra - 0x08000000)
VENDOR_CMD_DISPATCH_OFFSET = 0x0000E304
PATCH_ZONE_OFFSET = 0x00020800

# Real flash addresses (for B.W encoding)
VENDOR_CMD_DISPATCH_REAL = 0x08013304
PATCH_ZONE_REAL = 0x08025800

# Original prologue bytes (push {r4-r10,lr})
ORIGINAL_PROLOGUE = bytes.fromhex("2de9f047")


def encode_thumb2_branch(from_addr: int, to_addr: int) -> bytes:
    """Encode a Thumb-2 B.W (unconditional branch) instruction.

    Thumb-2 B.W encoding (T4, 4 bytes):
      halfword1: 11110 S imm10
      halfword2: 10 J1 1 J2 imm11

    The signed offset is computed from PC (= from_addr + 4):
      offset = to_addr - (from_addr + 4)

    offset bits: S:I1:I2:imm10:imm11:0 (25-bit signed, LSB always 0)
      where I1 = NOT(J1 XOR S), I2 = NOT(J2 XOR S)
    """
    offset = to_addr - (from_addr + 4)

    # B.W can encode +/- 16MB (24-bit signed * 2)
    if offset < -(1 << 24) or offset >= (1 << 24):
        raise ValueError(f"Branch offset {offset:#x} out of range for B.W")

    if offset & 1:
        raise ValueError(f"Branch target must be halfword-aligned (offset={offset:#x})")

    # Extract sign and magnitude bits from the 25-bit signed offset
    # offset = S:I1:I2:imm10[9:0]:imm11[10:0]:0
    S = (offset >> 24) & 1
    imm10 = (offset >> 12) & 0x3FF
    imm11 = (offset >> 1) & 0x7FF

    # I1 and I2: derived from bits 23 and 22 of offset
    bit23 = (offset >> 23) & 1
    bit22 = (offset >> 22) & 1

    # J1 = NOT(I1 XOR S) where I1 = bit23
    # But actually: I1 = NOT(J1 XOR S), so J1 = NOT(I1 XOR S)
    # I1 = bit23 (from the offset)
    # Wait, let me be precise:
    #   The encoding stores J1, J2.
    #   offset[24] = S
    #   offset[23] = I1 = NOT(J1 XOR S)  =>  J1 = NOT(I1 XOR S)
    #   offset[22] = I2 = NOT(J2 XOR S)  =>  J2 = NOT(I2 XOR S)
    I1 = bit23
    I2 = bit22
    J1 = (~(I1 ^ S)) & 1
    J2 = (~(I2 ^ S)) & 1

    hw1 = (0b11110 << 11) | (S << 10) | imm10
    hw2 = (0b10 << 14) | (J1 << 13) | (1 << 12) | (J2 << 11) | imm11

    # Thumb-2 is stored little-endian: hw1 first, then hw2
    return struct.pack("<HH", hw1, hw2)


def main() -> int:
    # Read inputs
    if not FIRMWARE_IN.exists():
        print(f"ERROR: {FIRMWARE_IN} not found", file=sys.stderr)
        return 1
    if not HOOK_BIN.exists():
        print(f"ERROR: {HOOK_BIN} not found (run 'make' first)", file=sys.stderr)
        return 1

    fw = bytearray(FIRMWARE_IN.read_bytes())
    hook = HOOK_BIN.read_bytes()

    print(f"Stock firmware: {len(fw)} bytes (0x{len(fw):x})")
    print(f"Hook binary:    {len(hook)} bytes (0x{len(hook):x})")

    # Verify original prologue is intact
    prologue = fw[VENDOR_CMD_DISPATCH_OFFSET:VENDOR_CMD_DISPATCH_OFFSET + 4]
    if prologue != ORIGINAL_PROLOGUE:
        print(
            f"ERROR: Expected prologue {ORIGINAL_PROLOGUE.hex()} at offset "
            f"0x{VENDOR_CMD_DISPATCH_OFFSET:x}, found {prologue.hex()}",
            file=sys.stderr,
        )
        return 1
    print(f"Original prologue verified at 0x{VENDOR_CMD_DISPATCH_OFFSET:x}: {prologue.hex()}")

    # Pad firmware to patch zone offset
    if len(fw) > PATCH_ZONE_OFFSET:
        print(
            f"ERROR: Firmware already extends past patch zone "
            f"(size=0x{len(fw):x} > 0x{PATCH_ZONE_OFFSET:x})",
            file=sys.stderr,
        )
        return 1

    pad_size = PATCH_ZONE_OFFSET - len(fw)
    fw.extend(b"\xff" * pad_size)  # 0xFF = erased flash
    print(f"Padded {pad_size} bytes (0xFF) to reach patch zone at 0x{PATCH_ZONE_OFFSET:x}")

    # Splice in hook binary
    fw.extend(hook)
    print(f"Spliced hook at 0x{PATCH_ZONE_OFFSET:x}, new size: {len(fw)} bytes (0x{len(fw):x})")

    # Encode and patch the B.W trampoline
    branch = encode_thumb2_branch(VENDOR_CMD_DISPATCH_REAL, PATCH_ZONE_REAL)
    print(f"B.W encoding: {branch.hex()} (from 0x{VENDOR_CMD_DISPATCH_REAL:08x} to 0x{PATCH_ZONE_REAL:08x})")

    # Verify: decode the offset back
    offset = PATCH_ZONE_REAL - (VENDOR_CMD_DISPATCH_REAL + 4)
    print(f"  Branch offset: {offset} (0x{offset:x})")

    fw[VENDOR_CMD_DISPATCH_OFFSET:VENDOR_CMD_DISPATCH_OFFSET + 4] = branch
    print(f"Patched B.W at offset 0x{VENDOR_CMD_DISPATCH_OFFSET:x}")

    # Write output
    FIRMWARE_OUT.write_bytes(fw)
    print(f"\nWrote {FIRMWARE_OUT} ({len(fw)} bytes)")
    print(f"  Patch zone:  0x{PATCH_ZONE_OFFSET:x} - 0x{PATCH_ZONE_OFFSET + len(hook):x}")
    print(f"  Trampoline:  0x{VENDOR_CMD_DISPATCH_OFFSET:x} (B.W -> 0x{PATCH_ZONE_REAL:08x})")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
