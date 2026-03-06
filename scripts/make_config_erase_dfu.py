#!/usr/bin/env python3
"""Generate a DfuSe .dfu file that erases the config area (factory reset).

Usage: python make_config_erase_dfu.py [output.dfu]

The resulting .dfu file targets address 0x08028000 with 2KB of 0xFF,
which resets the keyboard config to factory defaults without touching
the bootloader or firmware.

Flash with: dfu-util -a 0 -d 2e3c:df11 -D config_erase.dfu
"""

import struct
import sys


def crc32_dfuse(data: bytes) -> int:
    """CRC-32 used by DfuSe (standard CRC-32/ISO-3309, same as zlib)."""
    import binascii
    return binascii.crc32(data) & 0xFFFFFFFF


def make_dfuse(target_addr: int, payload: bytes,
               vid: int = 0x2E3C, pid: int = 0xDF11,
               target_name: str = "config_erase") -> bytes:
    """Build a DfuSe v1 .dfu file with one target, one element."""

    # --- Image Element ---
    element = struct.pack('<II', target_addr, len(payload)) + payload

    # --- Target Prefix ---
    # 6s  = "Target"
    # B   = alt setting (0)
    # I   = target named (1 = yes)
    # 255s = target name (null-padded)
    # I   = target size (total bytes of all elements)
    # I   = number of elements
    target_name_bytes = target_name.encode('ascii')[:255].ljust(255, b'\x00')
    target_prefix = struct.pack('<6sBI255sII',
                                b'Target', 0, 1,
                                target_name_bytes,
                                len(element), 1)

    # --- DfuSe Prefix ---
    # 5s = "DfuSe"
    # B  = version (1)
    # I  = total file size (filled after suffix)
    # B  = number of targets
    image_size = 11 + len(target_prefix) + len(element) + 16  # +16 for suffix
    prefix = struct.pack('<5sBIB', b'DfuSe', 1, image_size, 1)

    # --- DFU Suffix (16 bytes) ---
    # H = bcdDevice
    # H = idProduct
    # H = idVendor
    # H = bcdDFU (0x0100 for DFU 1.0)
    # 3s = "UFD" (DFU suffix signature, stored little-endian)
    # B  = suffix length (16)
    # I  = CRC (over entire file including suffix, with CRC field = 0)
    body = prefix + target_prefix + element
    suffix_no_crc = struct.pack('<HHHH3sB',
                                0xFFFF, pid, vid, 0x0100,
                                b'UFD', 16)
    # DFU suffix CRC: bitwise NOT of CRC-32 over everything except the CRC field
    crc = ~crc32_dfuse(body + suffix_no_crc) & 0xFFFFFFFF
    suffix = suffix_no_crc + struct.pack('<I', crc)

    return body + suffix


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "config_erase.dfu"

    CONFIG_ADDR = 0x08028000
    CONFIG_SIZE = 2048  # 2KB = one flash page

    payload = b'\xFF' * CONFIG_SIZE
    dfu = make_dfuse(CONFIG_ADDR, payload)

    with open(out, 'wb') as f:
        f.write(dfu)

    print(f"Written {out} ({len(dfu)} bytes)")
    print(f"  Target: 0x{CONFIG_ADDR:08X}, {CONFIG_SIZE} bytes of 0xFF")
    print(f"  Flash:  dfu-util -a 0 -d 2e3c:df11 -D {out}")


if __name__ == '__main__':
    main()
