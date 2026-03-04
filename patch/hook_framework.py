#!/usr/bin/env python3
"""
Firmware hook framework for AT32F405 (Cortex-M4 Thumb-2) targets.

Automates the generation of function hooks:
  - Reads displaced instruction bytes from firmware
  - Validates they are safe to relocate (no PC-relative ops)
  - Generates assembly stubs: displaced instruction + call to handler + jump-back
  - Manages patch zone allocation for multiple hooks
  - Applies B.W trampolines to the firmware binary

Parameterized for reuse across different firmware targets (keyboard, dongle, etc.).

Usage:
    from hook_framework import HookEngine, Hook, PatchProject, BinaryPatch

    project = PatchProject(
        hooks=[Hook(name="my_hook", target=0x0801474C, handler="my_handler")],
        binary_patches=[BinaryPatch(0x080147FC, b'\\xAB', b'\\xD9', "length cap")],
        firmware_bin="../firmware.bin",
        patched_bin="../firmware_patched.bin",
        hook_bin="hook.bin",
        elf_path="hook.elf",
        build_dir=".",
        engine_kwargs=dict(file_base=0x08005000,
                           patch_zone_start=0x08025800,
                           patch_zone_end=0x08027FFF),
    )
    project.main()
"""

from __future__ import annotations

import struct
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import NamedTuple


# ── Thumb-2 instruction analysis ─────────────────────────────────────────────

def is_thumb2_32bit(hw1: int) -> bool:
    """Check if a Thumb halfword starts a 32-bit instruction."""
    # 32-bit instructions: first halfword has bits [15:11] in {0b11101, 0b11110, 0b11111}
    top5 = (hw1 >> 11) & 0x1F
    return top5 in (0b11101, 0b11110, 0b11111)


def decode_instructions(data: bytes, base_addr: int) -> list[dict]:
    """Decode Thumb instructions from raw bytes. Returns list of instruction info dicts."""
    insns = []
    pos = 0
    while pos < len(data):
        hw1 = struct.unpack_from('<H', data, pos)[0]
        if is_thumb2_32bit(hw1) and pos + 4 <= len(data):
            hw2 = struct.unpack_from('<H', data, pos + 2)[0]
            insns.append({
                'addr': base_addr + pos,
                'size': 4,
                'bytes': data[pos:pos + 4],
                'hw1': hw1,
                'hw2': hw2,
                'encoding': f'{hw1:04X} {hw2:04X}',
            })
            pos += 4
        else:
            insns.append({
                'addr': base_addr + pos,
                'size': 2,
                'bytes': data[pos:pos + 2],
                'hw1': hw1,
                'hw2': None,
                'encoding': f'{hw1:04X}',
            })
            pos += 2
    return insns


def check_pc_relative(insn: dict) -> str | None:
    """
    Check if a Thumb instruction uses PC-relative addressing.
    Returns a description string if PC-relative, None if safe to relocate.
    """
    hw1 = insn['hw1']

    if insn['size'] == 2:
        # 16-bit instructions
        top5 = (hw1 >> 11) & 0x1F
        top8 = (hw1 >> 8) & 0xFF

        # LDR Rt, [PC, #imm] (01001 xxx xxxxxxxx)
        if top5 == 0b01001:
            return "LDR Rt,[PC,#imm8] (16-bit literal pool load)"

        # ADR Rd, label (10100 xxx xxxxxxxx)
        if top5 == 0b10100:
            return "ADR Rd,label (16-bit PC-relative)"

        # B<cond> (1101 cccc xxxxxxxx) - conditional branch, NOT 0b1110/0b1111
        if (top8 >> 4) == 0xD and ((top8 & 0xF) < 0xE):
            return f"B<cond> (16-bit conditional branch, cond={top8 & 0xF:#x})"

        # B (unconditional short) (11100 xxxxxxxxxxx)
        if top5 == 0b11100:
            return "B (16-bit unconditional branch)"

        # CBZ/CBNZ (1011 x0x1 xxxxxxxx)
        if (hw1 & 0xF500) == 0xB100:
            return "CBZ/CBNZ (compare and branch)"

    else:
        # 32-bit instructions
        hw2 = insn['hw2']

        # B.W / BL / BLX (11110 S xxxxxxxxxx  1x xxx xxxxxxxxxxx)
        if (hw1 & 0xF800) == 0xF000 and (hw2 & 0x8000) == 0x8000:
            link = (hw2 >> 14) & 1
            if link:
                return "BL (32-bit branch with link)"
            else:
                blx = not ((hw2 >> 12) & 1)
                if blx:
                    return "BLX (32-bit branch with link and exchange)"
                return "B.W (32-bit unconditional branch)"

        # LDR Rt, [PC, #imm] (11111 000 x1011111 xxxxxxxxxxxxxxxx)
        # Encoding: hw1 = 1111100x U1011111, hw2 = Rt:imm12
        if (hw1 & 0xFF7F) == 0xF85F:
            return "LDR.W Rt,[PC,#imm] (32-bit literal pool load)"

        # ADR.W (11110 x10 xxxx 1111) — ADD/SUB from PC
        if (hw1 & 0xFB0F) == 0xF20F or (hw1 & 0xFB0F) == 0xF2AF:
            return "ADR.W (32-bit PC-relative address)"

        # TBB/TBH (11101000 1101 xxxx  xxxx xxxx 000x xxxx)
        if (hw1 & 0xFFF0) == 0xE8D0 and (hw2 & 0xFFE0) == 0xF000:
            return "TBB/TBH (table branch)"

    return None


def validate_displaced(insns: list[dict]) -> list[str]:
    """Validate that all instructions can be safely displaced. Returns list of errors."""
    errors = []
    for insn in insns:
        reason = check_pc_relative(insn)
        if reason:
            errors.append(
                f"  0x{insn['addr']:08X} [{insn['encoding']}]: PC-relative — {reason}")
    return errors


# ── B.W encoding ─────────────────────────────────────────────────────────────

def encode_thumb2_bw(from_addr: int, to_addr: int) -> bytes:
    """Encode a Thumb-2 B.W (unconditional branch, 4 bytes)."""
    offset = to_addr - (from_addr + 4)

    if offset < -(1 << 24) or offset >= (1 << 24):
        raise ValueError(f"B.W offset {offset:#x} out of range (±16MB)")
    if offset & 1:
        raise ValueError(f"B.W target must be halfword-aligned (offset={offset:#x})")

    S = (offset >> 24) & 1
    imm10 = (offset >> 12) & 0x3FF
    imm11 = (offset >> 1) & 0x7FF
    I1 = (offset >> 23) & 1
    I2 = (offset >> 22) & 1
    J1 = (~(I1 ^ S)) & 1
    J2 = (~(I2 ^ S)) & 1

    hw1 = (0b11110 << 11) | (S << 10) | imm10
    hw2 = (0b10 << 14) | (J1 << 13) | (1 << 12) | (J2 << 11) | imm11

    return struct.pack('<HH', hw1, hw2)


# ── Inline assembly helpers ──────────────────────────────────────────────────

def bytes_to_asm_words(data: bytes, comment: str = "") -> str:
    """Convert raw bytes to .short/.word directives for GNU as."""
    lines = []
    if comment:
        lines.append(f"    /* {comment} */")
    pos = 0
    while pos < len(data):
        hw = struct.unpack_from('<H', data, pos)[0]
        if is_thumb2_32bit(hw) and pos + 4 <= len(data):
            word = struct.unpack_from('<I', data, pos)[0]
            lines.append(f"    .word 0x{word:08X}")
            pos += 4
        else:
            lines.append(f"    .short 0x{hw:04X}")
            pos += 2
    return '\n'.join(lines)


# ── Hook definition ──────────────────────────────────────────────────────────

@dataclass
class Hook:
    """
    Definition of a single function hook.

    Attributes:
        name:       Unique identifier for this hook (used in generated labels).
        target:     Flash address of the function to hook.
        handler:    Label of the user's handler function (defined in user .S/.c file).
                    The handler is called with all original registers intact.
                    It should return with:
                      r0 = 0  → continue to original function (jump-back)
                      r0 != 0 → skip original, return from hook stub
        displace:   Number of bytes to displace at target (default 4 = one B.W).
                    Must be >= 4, instruction-aligned. Read from firmware automatically.
        mode:       Hook mode:
                    "filter" — call handler, if r0==0 jump-back, else return
                    "before" — always call handler then jump-back
                    "replace" — handler IS the new function, no jump-back generated
    """
    name: str
    target: int
    handler: str
    displace: int = 4
    mode: str = "filter"

    # Populated by the engine
    _displaced_bytes: bytes = field(default=b'', repr=False)
    _displaced_insns: list = field(default_factory=list, repr=False)
    _stub_addr: int = 0
    _stub_size: int = 0


# ── Hook engine ──────────────────────────────────────────────────────────────

class HookEngine:
    """
    Manages multiple firmware hooks: validation, assembly generation, and patching.

    Args:
        firmware_path:    Path to the firmware binary.
        file_base:        Flash address corresponding to file offset 0.
        patch_zone_start: First usable flash address in the patch zone.
        patch_zone_end:   Last usable byte address in the patch zone.
    """

    def __init__(self, firmware_path: str | Path, *,
                 file_base: int = 0x08005000,
                 patch_zone_start: int = 0x08025800,
                 patch_zone_end: int = 0x08027FFF):
        self.firmware_path = Path(firmware_path)
        self.fw = bytearray(self.firmware_path.read_bytes())
        self.hooks: list[Hook] = []

        self.file_base = file_base
        self.patch_zone_start = patch_zone_start
        self.patch_zone_end = patch_zone_end
        self.patch_zone_size = patch_zone_end - patch_zone_start + 1

        self._alloc_ptr = patch_zone_start
        print(f"Loaded firmware: {len(self.fw)} bytes (0x{len(self.fw):X})")
        print(f"Patch zone: 0x{patch_zone_start:08X}–0x{patch_zone_end:08X} "
              f"({self.patch_zone_size} bytes)")

    def flash_to_offset(self, addr: int) -> int:
        """Convert real flash address to firmware file offset."""
        return addr - self.file_base

    def offset_to_flash(self, off: int) -> int:
        """Convert firmware file offset to real flash address."""
        return off + self.file_base

    def add_hook(self, hook: Hook) -> None:
        """Add a hook and validate the displaced instructions."""
        # Read displaced bytes from firmware
        off = self.flash_to_offset(hook.target)
        if off < 0 or off + hook.displace > len(self.fw):
            raise ValueError(f"Hook '{hook.name}': target 0x{hook.target:08X} "
                             f"outside firmware range")

        hook._displaced_bytes = bytes(self.fw[off:off + hook.displace])
        hook._displaced_insns = decode_instructions(hook._displaced_bytes, hook.target)

        # Validate total decoded size matches displace count
        total_decoded = sum(i['size'] for i in hook._displaced_insns)
        if total_decoded != hook.displace:
            raise ValueError(
                f"Hook '{hook.name}': instruction boundary mismatch at "
                f"0x{hook.target:08X}. Requested {hook.displace} bytes but "
                f"decoded {total_decoded}. Adjust displace= to an instruction boundary.")

        # Check for PC-relative instructions
        errors = validate_displaced(hook._displaced_insns)
        if errors:
            msg = (f"Hook '{hook.name}': cannot safely displace instructions "
                   f"at 0x{hook.target:08X}:\n" + '\n'.join(errors) +
                   "\nPick a different hook point or increase displace= "
                   "to cover the PC-relative instruction + its literal pool usage.")
            raise ValueError(msg)

        # Estimate stub size for allocation
        if hook.mode == "replace":
            hook._stub_size = 8
        elif hook.mode == "before":
            hook._stub_size = hook.displace + 4 + 8 + 8
        else:  # filter
            hook._stub_size = hook.displace + 32 + 16
        # Round up to 4-byte alignment
        hook._stub_size = (hook._stub_size + 3) & ~3

        # Allocate in patch zone
        hook._stub_addr = self._alloc_ptr
        self._alloc_ptr += hook._stub_size
        if self._alloc_ptr > self.patch_zone_end + 1:
            raise ValueError(
                f"Hook '{hook.name}': patch zone exhausted "
                f"(need 0x{self._alloc_ptr - self.patch_zone_start:X} bytes, "
                f"have {self.patch_zone_size})")

        self.hooks.append(hook)

        insn_desc = ', '.join(f"[{i['encoding']}]" for i in hook._displaced_insns)
        print(f"  Hook '{hook.name}': 0x{hook.target:08X} → stub@0x{hook._stub_addr:08X} "
              f"(mode={hook.mode}, displace={hook.displace}B: {insn_desc})")

    def generate(self, output_path: str | Path, extra_asm: str = "") -> str:
        """
        Generate the assembly source file with all hook stubs.

        Args:
            output_path: Where to write the generated .S file.
            extra_asm: Additional assembly to include (user handler code).

        Returns:
            The generated assembly source as a string.
        """
        lines = [
            "/* Auto-generated by hook_framework.py — do not edit manually. */",
            "",
            "    .syntax unified",
            "    .cpu    cortex-m4",
            "    .thumb",
            "",
        ]

        for hook in self.hooks:
            lines.extend(self._gen_stub(hook))

        if extra_asm:
            lines.append("")
            lines.append("/* ── User handler code ─────────────────── */")
            lines.append(extra_asm)

        lines.append("")
        src = '\n'.join(lines)

        Path(output_path).write_text(src)
        print(f"Generated: {output_path} ({len(src)} bytes, {len(self.hooks)} hooks)")
        return src

    def _gen_stub(self, hook: Hook) -> list[str]:
        """Generate assembly lines for a single hook stub."""
        target = hook.target
        jumpback = target + hook.displace
        jumpback_thumb = jumpback | 1  # Thumb bit for bx

        lines = [
            f"/* ── Hook: {hook.name} ──────────────────── */",
            f"/* Target: 0x{target:08X}, displaced: {hook.displace} bytes */",
            f"/* Jump-back: 0x{jumpback:08X} */",
            "",
            f"    .section .text.hook_{hook.name}, \"ax\", %progbits",
            f"    .global _hook_{hook.name}_stub",
            f"    .thumb_func",
            f"    .type _hook_{hook.name}_stub, %function",
            f"",
            f"_hook_{hook.name}_stub:",
        ]

        if hook.mode == "replace":
            lines += [
                f"    b.w {hook.handler}",
                f"",
                f"    .size _hook_{hook.name}_stub, . - _hook_{hook.name}_stub",
                "",
            ]
            return lines

        if hook.mode == "before":
            lines += [
                f"    /* Save lr so handler can use bl freely */",
                f"    push {{lr}}",
                f"    bl {hook.handler}",
                f"    pop {{lr}}",
                f"",
                f"    /* Execute displaced instructions */",
                bytes_to_asm_words(hook._displaced_bytes,
                                   hook._displaced_bytes.hex()),
                f"",
                f"    /* Jump back to original function + {hook.displace} */",
                f"    ldr r12, =0x{jumpback_thumb:08X}",
                f"    bx  r12",
                f"",
                f"    .size _hook_{hook.name}_stub, . - _hook_{hook.name}_stub",
                "",
            ]
            return lines

        # mode == "filter" (default)
        lines += [
            f"    /* 1. Call filter handler (preserving all regs) */",
            f"    push {{r0-r3, r12, lr}}",
            f"    bl {hook.handler}",
            f"    cmp r0, #0",
            f"    pop {{r0-r3, r12, lr}}   /* restore (does NOT affect flags) */",
            f"",
            f"    /* 2. If handler returned 0 → continue to original */",
            f"    beq .L_{hook.name}_passthrough",
            f"",
            f"    /* Handler intercepted — original function never ran. */",
            f"    /* Handler should have written its response; just return to caller. */",
            f"    bx  lr",
            f"",
            f".L_{hook.name}_passthrough:",
            f"    /* 3. Execute displaced instructions, then continue original */",
            bytes_to_asm_words(hook._displaced_bytes,
                               hook._displaced_bytes.hex()),
            f"",
            f"    /* Jump back to original function + {hook.displace} */",
            f"    ldr r12, =0x{jumpback_thumb:08X}",
            f"    bx  r12",
            f"",
            f"    .size _hook_{hook.name}_stub, . - _hook_{hook.name}_stub",
            "",
        ]
        return lines

    def patch(self, output_path: str | Path, hook_bin_path: str | Path | None = None) -> None:
        """
        Apply all hooks to the firmware and write the patched binary.

        If hook_bin_path is provided, splice it into the patch zone.
        Then write B.W trampolines for each hook.
        """
        patched = bytearray(self.fw)

        # Pad firmware to patch zone start
        patch_zone_file_off = self.flash_to_offset(self.patch_zone_start)
        if len(patched) < patch_zone_file_off:
            patched.extend(b'\xff' * (patch_zone_file_off - len(patched)))

        # Splice compiled hook binary if provided
        if hook_bin_path:
            hook_bin = Path(hook_bin_path).read_bytes()
            # Extend or overwrite at patch zone
            end_off = patch_zone_file_off + len(hook_bin)
            if end_off > len(patched):
                patched.extend(b'\xff' * (end_off - len(patched)))
            patched[patch_zone_file_off:patch_zone_file_off + len(hook_bin)] = hook_bin
            print(f"Spliced hook binary: {len(hook_bin)} bytes at "
                  f"0x{self.patch_zone_start:08X}")

        # Write B.W trampolines for each hook
        for hook in self.hooks:
            off = self.flash_to_offset(hook.target)
            bw = encode_thumb2_bw(hook.target, hook._stub_addr)

            # Verify original bytes are still intact (not already patched)
            current = patched[off:off + 4]
            if current != hook._displaced_bytes[:4]:
                print(f"WARNING: Hook '{hook.name}': bytes at 0x{hook.target:08X} "
                      f"changed ({current.hex()} != {hook._displaced_bytes[:4].hex()}). "
                      f"Already patched?")

            patched[off:off + 4] = bw
            print(f"  Trampoline: 0x{hook.target:08X} → B.W 0x{hook._stub_addr:08X} "
                  f"({bw.hex()})")

        Path(output_path).write_bytes(patched)
        print(f"Wrote: {output_path} ({len(patched)} bytes)")

    def summary(self) -> str:
        """Print a summary of all hooks."""
        lines = [
            f"",
            f"Hooks ({len(self.hooks)}):",
        ]
        for h in self.hooks:
            lines.append(
                f"  {h.name:24s}  0x{h.target:08X} → stub@0x{h._stub_addr:08X}  "
                f"mode={h.mode}  displace={h.displace}B  handler={h.handler}")
        return '\n'.join(lines)


# ── Binary patch definition ──────────────────────────────────────────────────


class BinaryPatch(NamedTuple):
    """A single binary patch to apply to the firmware.

    For byte patches: addr is the flash address, old_bytes/new_bytes are
    the expected and replacement byte strings (e.g. b'\\xAB' → b'\\xD9').

    For symbol-resolved patches: set symbol to an ELF symbol name.
    old_bytes is the expected original word (4 bytes LE), and new_bytes
    is ignored — the symbol's address is written instead.
    """
    addr: int
    old_bytes: bytes
    new_bytes: bytes
    desc: str
    symbol: str | None = None


class MemoryRegion(NamedTuple):
    """A memory region for the memory map visualization."""
    name: str       # "Bootloader", "Firmware code", etc.
    start: int      # absolute address
    end: int        # last byte (inclusive)
    style: str      # "code" | "free" | "patch" | "config" | "stack"


# ── Memory map SVG rendering ─────────────────────────────────────────────────


def _fmt_size(size: int) -> str:
    """Format a byte size as KiB (e.g. 10240 → '10.0 KiB', 5508 → '5.4 KiB')."""
    kib = size / 1024
    if kib >= 1:
        return f"{kib:.1f} KiB" if kib != int(kib) else f"{int(kib)} KiB"
    return f"{size} B"


def _fmt_addr(addr: int) -> str:
    """Format an address as 0x08XXXXXX or 0x2000XXXX."""
    return f"0x{addr:08X}"


def render_memmap_svg(
    flash_col: list[MemoryRegion],
    sram_col: list[MemoryRegion],
    flash_size: int,
    sram_size: int,
    title: str = "Memory Map",
) -> str:
    """Render a memory map SVG with flash and SRAM columns side by side.

    Each column is a list of MemoryRegion (sorted, no gaps — caller fills gaps
    as 'free' regions). Regions are drawn proportionally to their size within
    each column.

    Returns the SVG as a string.
    """
    STYLE_COLORS = {
        "code":   ("#4682B4", "#FFFFFF"),  # steel blue, white text
        "free":   ("#E8E8E8", "#888888"),  # light gray, gray text
        "patch":  ("#E8820C", "#FFFFFF"),  # orange, white text
        "patch_used": ("#E8820C", "#FFFFFF"),
        "patch_free": ("#F5C882", "#886620"),  # light orange, dark text
        "config": ("#4CAF50", "#FFFFFF"),  # green, white text
        "stack":  ("#B39DDB", "#333333"),  # light purple, dark text
    }

    col_w = 260
    label_w = 110  # space for address labels
    gap = 60       # gap between columns
    top_margin = 50
    bottom_margin = 80
    max_col_h = 700  # max pixel height per column
    min_region_h = 20  # minimum region height in px

    def layout_column(regions: list[MemoryRegion], total_size: int) -> list[tuple[MemoryRegion, float]]:
        """Assign pixel heights to regions, with minimum height enforcement."""
        if not regions:
            return []

        # Calculate proportional heights with minimum enforcement.
        # Scale only the non-minimum regions to fit max_col_h.
        raw: list[tuple[MemoryRegion, float]] = []
        for r in regions:
            prop_h = (r.end - r.start + 1) / total_size * max_col_h
            raw.append((r, max(min_region_h, prop_h)))

        total_h = sum(h for _, h in raw)
        if total_h > max_col_h:
            # Only scale regions that are above min_region_h
            fixed = sum(min_region_h for _, h in raw if h <= min_region_h)
            scalable = total_h - fixed
            target = max_col_h - fixed
            if scalable > 0 and target > 0:
                scale = target / scalable
                raw = [(r, min_region_h if h <= min_region_h else h * scale)
                       for r, h in raw]
        return raw

    flash_layout = layout_column(flash_col, flash_size)
    sram_layout = layout_column(sram_col, sram_size)

    flash_h = sum(h for _, h in flash_layout)
    sram_h = sum(h for _, h in sram_layout)
    content_h = max(flash_h, sram_h)

    total_w = label_w + col_w + gap + label_w + col_w + label_w
    total_h = top_margin + content_h + bottom_margin

    lines = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {total_w} {total_h}"'
        f' width="{total_w}" height="{total_h}">',
        '<style>',
        '  text { font-family: "Consolas", "Monaco", monospace; }',
        '  .title { font-size: 16px; font-weight: bold; fill: #333; }',
        '  .col-title { font-size: 13px; font-weight: bold; fill: #555; }',
        '  .region-name { font-size: 9px; }',
        '  .region-size { font-size: 8px; opacity: 0.8; }',
        '  .addr { font-size: 8px; fill: #666; }',
        '  .legend-text { font-size: 10px; fill: #333; }',
        '  rect.region { stroke: #666; stroke-width: 0.5; }',
        '  pattern#hatch { patternUnits: userSpaceOnUse; }',
        '</style>',
        '<defs>',
        '  <pattern id="hatch" width="6" height="6" patternTransform="rotate(45)"'
        '   patternUnits="userSpaceOnUse">',
        '    <line x1="0" y1="0" x2="0" y2="6" stroke="#CCC" stroke-width="1.5"/>',
        '  </pattern>',
        '  <pattern id="hatch-orange" width="6" height="6" patternTransform="rotate(45)"'
        '   patternUnits="userSpaceOnUse">',
        '    <rect width="6" height="6" fill="#F5C882"/>',
        '    <line x1="0" y1="0" x2="0" y2="6" stroke="#D4981C" stroke-width="1.5"/>',
        '  </pattern>',
        '  <linearGradient id="stack-grad" x1="0" y1="0" x2="0" y2="1">',
        '    <stop offset="0%" stop-color="#7E57C2" stop-opacity="1"/>',
        '    <stop offset="100%" stop-color="#7E57C2" stop-opacity="0"/>',
        '  </linearGradient>',
        '</defs>',
        f'<text x="{total_w / 2}" y="30" text-anchor="middle" class="title">{title}</text>',
    ]

    def draw_column(regions_layout, x_label, x_rect, y_start, col_title, base_addr, total_size):
        lines.append(
            f'<text x="{x_rect + col_w / 2}" y="{y_start - 8}" '
            f'text-anchor="middle" class="col-title">'
            f'{col_title} ({_fmt_size(total_size)})</text>'
        )

        y = y_start
        for region, h in regions_layout:
            fill, text_color = STYLE_COLORS.get(region.style, ("#DDD", "#333"))
            region_size = region.end - region.start + 1

            # Region rectangle
            if region.style == "free":
                lines.append(
                    f'<rect x="{x_rect}" y="{y:.1f}" width="{col_w}" height="{h:.1f}"'
                    f' fill="url(#hatch)" class="region"/>'
                )
            elif region.style == "patch_free":
                lines.append(
                    f'<rect x="{x_rect}" y="{y:.1f}" width="{col_w}" height="{h:.1f}"'
                    f' fill="url(#hatch-orange)" class="region"/>'
                )
            elif region.style == "stack":
                # Hatch underneath, gradient on top fading to transparent
                lines.append(
                    f'<rect x="{x_rect}" y="{y:.1f}" width="{col_w}" height="{h:.1f}"'
                    f' fill="url(#hatch)" class="region"/>'
                )
                lines.append(
                    f'<rect x="{x_rect}" y="{y:.1f}" width="{col_w}" height="{h:.1f}"'
                    f' fill="url(#stack-grad)" stroke="none"/>'
                )
            else:
                lines.append(
                    f'<rect x="{x_rect}" y="{y:.1f}" width="{col_w}" height="{h:.1f}"'
                    f' fill="{fill}" class="region"/>'
                )

            # Address label (left of rect)
            lines.append(
                f'<text x="{x_label}" y="{y + 11:.1f}" class="addr"'
                f' text-anchor="end">{_fmt_addr(region.start)}</text>'
            )

            # Region name + size (centered in rect)
            size_str = _fmt_size(region_size)
            if h >= 32:
                # Two lines: name above, size below
                lines.append(
                    f'<text x="{x_rect + col_w / 2}" y="{y + h / 2 + 1:.1f}"'
                    f' text-anchor="middle" class="region-name"'
                    f' fill="{text_color}">{region.name}</text>'
                )
                lines.append(
                    f'<text x="{x_rect + col_w / 2}" y="{y + h / 2 + 10:.1f}"'
                    f' text-anchor="middle" class="region-size"'
                    f' fill="{text_color}">{size_str}</text>'
                )
            elif h >= 16:
                # Single line: "name (size)"
                lines.append(
                    f'<text x="{x_rect + col_w / 2}" y="{y + h / 2 + 3:.1f}"'
                    f' text-anchor="middle" class="region-name"'
                    f' fill="{text_color}">{region.name} ({size_str})</text>'
                )

            y += h

        # End address
        if regions_layout:
            last = regions_layout[-1][0]
            lines.append(
                f'<text x="{x_label}" y="{y + 11:.1f}" class="addr"'
                f' text-anchor="end">{_fmt_addr(last.end + 1)}</text>'
            )

    # Draw flash column (left)
    flash_x_label = label_w - 4
    flash_x_rect = label_w
    draw_column(flash_layout, flash_x_label, flash_x_rect, top_margin,
                "Flash", 0x08000000, flash_size)

    # Draw SRAM column (right)
    sram_x_label = label_w + col_w + gap + label_w - 4
    sram_x_rect = label_w + col_w + gap + label_w
    draw_column(sram_layout, sram_x_label, sram_x_rect, top_margin,
                "SRAM", 0x20000000, sram_size)

    # Legend
    legend_y = top_margin + content_h + 30
    legend_items = [
        ("code", "Firmware / globals"),
        ("config", "Config / keymaps"),
        ("patch", "Patch (used)"),
        ("patch_free", "Patch (free)"),
        ("stack", "Stack"),
        ("free", "Unused"),
    ]
    legend_x = 20
    for style, label in legend_items:
        fill, _ = STYLE_COLORS.get(style, ("#DDD", "#333"))
        if style == "free":
            lines.append(
                f'<rect x="{legend_x}" y="{legend_y - 10}" width="14" height="14"'
                f' fill="url(#hatch)" stroke="#999" stroke-width="0.5"/>'
            )
        elif style == "patch_free":
            lines.append(
                f'<rect x="{legend_x}" y="{legend_y - 10}" width="14" height="14"'
                f' fill="url(#hatch-orange)" stroke="#999" stroke-width="0.5"/>'
            )
        elif style == "stack":
            lines.append(
                f'<rect x="{legend_x}" y="{legend_y - 10}" width="14" height="14"'
                f' fill="url(#hatch)" stroke="#999" stroke-width="0.5"/>'
            )
            lines.append(
                f'<rect x="{legend_x}" y="{legend_y - 10}" width="14" height="14"'
                f' fill="url(#stack-grad)" stroke="none"/>'
            )
        else:
            lines.append(
                f'<rect x="{legend_x}" y="{legend_y - 10}" width="14" height="14"'
                f' fill="{fill}" stroke="#999" stroke-width="0.5"/>'
            )
        lines.append(
            f'<text x="{legend_x + 20}" y="{legend_y}" class="legend-text">{label}</text>'
        )
        legend_x += len(label) * 7 + 40

    lines.append('</svg>')
    return '\n'.join(lines)


# ── Patch project (reusable CLI scaffold) ────────────────────────────────────


class PatchProject:
    """Reusable scaffold for a firmware hook project.

    Encapsulates hooks, binary patches, file paths, and CLI commands
    (validate / generate / patch).  Each firmware target (keyboard, dongle)
    creates a PatchProject with its own configuration and calls .main().
    """

    def __init__(
        self,
        hooks: list[Hook],
        binary_patches: list[BinaryPatch],
        firmware_bin: str | Path,
        patched_bin: str | Path,
        hook_bin: str | Path,
        elf_path: str | Path,
        build_dir: str | Path,
        engine_kwargs: dict | None = None,
        flash_regions: list[MemoryRegion] | None = None,
        sram_regions: list[MemoryRegion] | None = None,
        sram_landmarks: list[tuple[str, str]] | None = None,
        flash_size: int | None = None,
        sram_size: int | None = None,
        initial_sp: int | None = None,
        memmap_svg: str | Path | None = None,
    ) -> None:
        self.hooks = hooks
        self.binary_patches = binary_patches
        self.firmware_bin = Path(firmware_bin)
        self.patched_bin = Path(patched_bin)
        self.hook_bin = Path(hook_bin)
        self.elf_path = Path(elf_path)
        self.build_dir = Path(build_dir)
        self.hooks_asm = self.build_dir / "hooks_gen.S"
        self.engine_kwargs = engine_kwargs or {}
        self.flash_regions = flash_regions
        self.sram_regions = sram_regions
        self.sram_landmarks = sram_landmarks
        self.flash_size = flash_size
        self.sram_size = sram_size
        self.initial_sp = initial_sp
        self.memmap_svg = Path(memmap_svg) if memmap_svg else None

    def _build_engine(self) -> HookEngine:
        engine = HookEngine(self.firmware_bin, **self.engine_kwargs)
        for hook in self.hooks:
            engine.add_hook(hook)
        return engine

    def read_elf_symbols(self) -> dict[str, int]:
        """Read all symbol addresses from hook.elf via nm."""
        if not self.elf_path.exists():
            return {}
        result = subprocess.run(
            ['arm-none-eabi-nm', str(self.elf_path)],
            capture_output=True, text=True,
        )
        if result.returncode != 0:
            return {}
        symbols: dict[str, int] = {}
        for line in result.stdout.strip().split('\n'):
            parts = line.strip().split()
            if len(parts) == 3:
                symbols[parts[2]] = int(parts[0], 16)
        return symbols

    def size_report(self, engine: HookEngine) -> str:
        """Report flash and SRAM usage from the compiled ELF."""
        if not self.elf_path.exists():
            return "  (no ELF — run make first)"

        result = subprocess.run(
            ['arm-none-eabi-size', '-A', str(self.elf_path)],
            capture_output=True, text=True,
        )
        if result.returncode != 0:
            return f"  (arm-none-eabi-size failed: {result.stderr.strip()})"

        # Parse section sizes from `size -A` output
        flash_sections: dict[str, int] = {}
        sram_sections: dict[str, int] = {}
        for line in result.stdout.strip().split('\n'):
            parts = line.split()
            if len(parts) < 3 or not parts[1].isdigit():
                continue
            name, size_s, addr_s = parts[0], int(parts[1]), int(parts[2])
            if size_s == 0:
                continue
            if addr_s >= 0x20000000:
                sram_sections[name] = size_s
            elif addr_s >= engine.patch_zone_start:
                flash_sections[name] = size_s

        flash_total = sum(flash_sections.values())
        sram_total = sum(sram_sections.values())
        sram_limit = self.engine_kwargs.get('patch_sram_size', 0)

        # Try to read PATCH_SRAM length from patch.ld MEMORY section
        if not sram_limit:
            patch_ld = self.build_dir / "patch.ld"
            if patch_ld.exists():
                import re
                m = re.search(r'PATCH_SRAM\s*\([^)]*\)\s*:\s*ORIGIN\s*=\s*\S+\s*,\s*LENGTH\s*=\s*(\d+)',
                              patch_ld.read_text())
                if m:
                    sram_limit = int(m.group(1))

        lines = [
            "",
            f"Flash: {flash_total} / {engine.patch_zone_size} bytes "
            f"({flash_total * 100 // engine.patch_zone_size}%)",
        ]
        # Detail flash sections
        for name, size in sorted(flash_sections.items(), key=lambda x: -x[1]):
            lines.append(f"  {name:12s} {size:5d}")

        sram_pct = f" ({sram_total * 100 // sram_limit}%)" if sram_limit else ""
        sram_cap = f" / {sram_limit}" if sram_limit else ""
        lines.append(f"SRAM:  {sram_total}{sram_cap} bytes{sram_pct}")
        for name, size in sorted(sram_sections.items(), key=lambda x: -x[1]):
            lines.append(f"  {name:12s} {size:5d}")

        return '\n'.join(lines)

    def fix_stub_addresses(self, engine: HookEngine, symbols: dict[str, int]) -> None:
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

    def apply_binary_patches(self, fw: bytearray, symbols: dict[str, int]) -> None:
        """Apply build-time binary patches to the firmware."""
        file_base = self.engine_kwargs.get('file_base', 0x08005000)

        for patch in self.binary_patches:
            off = patch.addr - file_base

            if patch.symbol is not None:
                # Word-sized symbol-resolved patch
                resolved = symbols.get(patch.symbol)
                if resolved is None:
                    print(f"ERROR: '{patch.symbol}' symbol not found in hook.elf. "
                          f"Make sure it is non-static in handlers.c.",
                          file=sys.stderr)
                    sys.exit(1)
                old_val = struct.unpack_from('<I', fw, off)[0]
                expected = struct.unpack('<I', patch.old_bytes)[0]
                if old_val != expected:
                    print(f"WARNING: word at 0x{patch.addr:08X} is "
                          f"0x{old_val:08X}, expected 0x{expected:08X}. "
                          f"Already patched?", file=sys.stderr)
                struct.pack_into('<I', fw, off, resolved)
                print(f"  Patch: 0x{patch.addr:08X} "
                      f"[0x{old_val:08X}→0x{resolved:08X}] {patch.desc}")
            else:
                # Byte-level patch
                for i, (old_b, new_b) in enumerate(
                    zip(patch.old_bytes, patch.new_bytes)
                ):
                    if fw[off + i] != old_b:
                        print(f"WARNING: byte at 0x{patch.addr + i:08X} is "
                              f"0x{fw[off + i]:02X}, expected 0x{old_b:02X}. "
                              f"Already patched?", file=sys.stderr)
                    else:
                        fw[off + i] = new_b
                old_hex = patch.old_bytes.hex()
                new_hex = patch.new_bytes.hex()
                print(f"  Patch: 0x{patch.addr:08X} "
                      f"[0x{old_hex}→0x{new_hex}] {patch.desc}")

    def cmd_validate(self) -> None:
        engine = self._build_engine()
        print(engine.summary())
        print("\nAll hook points validated OK.")

    def cmd_generate(self) -> None:
        engine = self._build_engine()
        engine.generate(self.hooks_asm)
        print(engine.summary())
        print(f"\nGenerated: {self.hooks_asm}")
        print(f"Now define handlers in handlers.S, then run: make")

    def cmd_patch(self) -> None:
        engine = self._build_engine()
        if not self.hook_bin.exists():
            print(f"ERROR: {self.hook_bin} not found. Run 'make' first.",
                  file=sys.stderr)
            sys.exit(1)

        symbols = self.read_elf_symbols()
        self.fix_stub_addresses(engine, symbols)
        engine.patch(self.patched_bin, self.hook_bin)

        # Apply binary patches to the already-written output
        fw = bytearray(self.patched_bin.read_bytes())
        self.apply_binary_patches(fw, symbols)
        self.patched_bin.write_bytes(fw)
        print(f"Binary patches applied to {self.patched_bin}")

        print(engine.summary())
        print(self.size_report(engine))

    def _parse_patch_ld(self) -> tuple[int, int, int, int] | None:
        """Parse patch.ld for PATCH and PATCH_SRAM origins/lengths.

        Returns (flash_origin, flash_len, sram_origin, sram_len) or None.
        """
        import re
        patch_ld = self.build_dir / "patch.ld"
        if not patch_ld.exists():
            return None
        text = patch_ld.read_text()

        def parse_mem(name: str) -> tuple[int, int] | None:
            m = re.search(
                rf'{name}\s*\([^)]*\)\s*:\s*ORIGIN\s*=\s*(0x[\da-fA-F]+)\s*,\s*LENGTH\s*=\s*(\d+)',
                text)
            if m:
                return int(m.group(1), 16), int(m.group(2))
            return None

        flash = parse_mem('PATCH')
        sram = parse_mem('PATCH_SRAM')
        if flash and sram:
            return (*flash, *sram)
        return None

    def _elf_section_sizes(self) -> dict[str, tuple[int, int]]:
        """Read section names → (size, addr) from ELF via arm-none-eabi-size -A."""
        if not self.elf_path.exists():
            return {}
        result = subprocess.run(
            ['arm-none-eabi-size', '-A', str(self.elf_path)],
            capture_output=True, text=True,
        )
        if result.returncode != 0:
            return {}
        sections: dict[str, tuple[int, int]] = {}
        for line in result.stdout.strip().split('\n'):
            parts = line.split()
            if len(parts) >= 3 and parts[1].isdigit():
                sections[parts[0]] = (int(parts[1]), int(parts[2]))
        return sections

    def cmd_memmap(self) -> None:
        """Generate a memory map SVG showing flash and SRAM layout."""
        import json

        patch_info = self._parse_patch_ld()
        if not patch_info:
            print("ERROR: cannot parse patch.ld", file=sys.stderr)
            sys.exit(1)
        patch_flash_origin, patch_flash_len, patch_sram_origin, patch_sram_len = patch_info

        # Get ELF usage to split patch zones into used/free
        elf_sections = self._elf_section_sizes()
        patch_flash_used = 0
        patch_sram_used = 0
        for _name, (size, addr) in elf_sections.items():
            if size == 0:
                continue
            if addr >= patch_flash_origin and addr < patch_flash_origin + patch_flash_len:
                patch_flash_used += size
            elif addr >= patch_sram_origin and addr < patch_sram_origin + patch_sram_len:
                patch_sram_used += size

        # ── Build flash column ──
        flash_col: list[MemoryRegion] = []
        symbols_path = self.build_dir / "symbols.json"

        if self.flash_regions:
            # Manual flash regions provided
            flash_col = list(self.flash_regions)
        elif symbols_path.exists():
            # Auto-derive from symbols.json memory_blocks
            with open(symbols_path) as f:
                syms = json.load(f)

            flash_blocks = [b for b in syms.get('memory_blocks', [])
                            if int(b['start'], 16) >= 0x08000000
                            and int(b['start'], 16) < 0x20000000]

            # If no block starts at 0x08000000, add bootloader
            # (keyboard Ghidra project only has the app, not the bootloader)
            if flash_blocks and int(flash_blocks[0]['start'], 16) > 0x08000000:
                bl_end = int(flash_blocks[0]['start'], 16) - 1
                flash_col.append(MemoryRegion(
                    "Bootloader", 0x08000000, bl_end, "code"))

            # Read firmware binary to trim code blocks to actual content
            file_base = self.engine_kwargs.get('file_base', 0x08005000)
            fw_data = None
            if self.firmware_bin.exists():
                fw_data = self.firmware_bin.read_bytes()

            for block in flash_blocks:
                start = int(block['start'], 16)
                end = int(block['end'], 16)
                name = block['name']
                # Ghidra's default "ram" name isn't descriptive
                if name == 'ram':
                    name = "Firmware code"
                # Classify style by block properties and name
                if 'unused' in name or name == 'patch_zone':
                    style = "free"
                elif not block.get('initialized') or 'x' not in block.get('perms', ''):
                    style = "config"
                else:
                    style = "code"

                # Trim code blocks to actual content (scan for trailing 0xFF)
                if style == "code" and fw_data is not None:
                    file_off_start = start - file_base
                    file_off_end = end - file_base
                    if 0 <= file_off_start < len(fw_data):
                        actual_end = min(file_off_end, len(fw_data) - 1)
                        # Scan backwards for last non-0xFF byte
                        trimmed_end = file_off_start - 1
                        for off in range(actual_end, file_off_start - 1, -1):
                            if fw_data[off] != 0xFF:
                                trimmed_end = off
                                break
                        if trimmed_end < file_off_start:
                            # Entire block is 0xFF — mark as free
                            style = "free"
                            name = "(unused)"
                        else:
                            # Trim end to actual content (align to 16)
                            end = file_base + ((trimmed_end + 16) & ~15) - 1

                flash_col.append(MemoryRegion(name, start, end, style))
        else:
            print("WARNING: no symbols.json and no flash_regions — "
                  "flash column will only show patch zone", file=sys.stderr)

        # Insert patch zone into flash column (split into used/free)
        patch_flash_end = patch_flash_origin + patch_flash_len - 1
        # Remove any existing region that overlaps with patch zone
        flash_col = [r for r in flash_col
                     if r.end < patch_flash_origin or r.start > patch_flash_end]

        if patch_flash_used > 0:
            used_end = patch_flash_origin + patch_flash_used - 1
            flash_col.append(MemoryRegion(
                "Patch (used)", patch_flash_origin, used_end, "patch_used"))
            if patch_flash_used < patch_flash_len:
                flash_col.append(MemoryRegion(
                    "Patch (free)", used_end + 1, patch_flash_end, "patch_free"))
        else:
            flash_col.append(MemoryRegion(
                "Patch zone", patch_flash_origin, patch_flash_end, "patch_free"))

        flash_col.sort(key=lambda r: r.start)

        # Clip to actual flash size (dump files may extend beyond chip capacity)
        flash_size = self.flash_size or 256 * 1024
        flash_end_addr = 0x08000000 + flash_size - 1
        flash_col = [r for r in flash_col if r.start <= flash_end_addr]
        if flash_col and flash_col[-1].end > flash_end_addr:
            r = flash_col[-1]
            flash_col[-1] = MemoryRegion(r.name, r.start, flash_end_addr, r.style)

        # ── Determine stack pointer (needed for SRAM layout) ──
        sp = self.initial_sp
        if sp is None and symbols_path.exists():
            with open(symbols_path) as f:
                syms = json.load(f)
            for label in syms.get('labels', []):
                if label['name'] == 'g_stack_top':
                    sp = int(label['addr'], 16)
                    break

        # ── Build SRAM column ──
        sram_base = 0x20000000
        sram_end_addr = sram_base + (self.sram_size or 0x18000) - 1
        sram_col: list[MemoryRegion] = []

        if self.sram_landmarks and symbols_path.exists():
            # Resolve landmark labels from symbols.json
            with open(symbols_path) as f:
                syms = json.load(f)
            label_addrs = {l['name']: int(l['addr'], 16)
                           for l in syms.get('labels', [])}

            resolved = []
            for label_name, display_name in self.sram_landmarks:
                addr = label_addrs.get(label_name)
                if addr is None:
                    print(f"WARNING: SRAM landmark '{label_name}' not found "
                          f"in symbols.json", file=sys.stderr)
                    continue
                resolved.append((addr, display_name))
            resolved.sort(key=lambda x: x[0])

            # Build regions: each landmark runs until the next one.
            for i, (addr, name) in enumerate(resolved):
                if i + 1 < len(resolved):
                    end = resolved[i + 1][0] - 1
                else:
                    end = patch_sram_origin - 1
                sram_col.append(MemoryRegion(name, addr, end, "code"))
        elif self.sram_regions:
            sram_col = list(self.sram_regions)

        # Insert patch SRAM (split into used/free)
        patch_sram_end = patch_sram_origin + patch_sram_len - 1
        sram_col = [r for r in sram_col
                    if r.end < patch_sram_origin or r.start > patch_sram_end]

        if patch_sram_used > 0:
            used_end = patch_sram_origin + patch_sram_used - 1
            sram_col.append(MemoryRegion(
                "Patch (used)", patch_sram_origin, used_end, "patch_used"))
            if patch_sram_used < patch_sram_len:
                sram_col.append(MemoryRegion(
                    "Patch (free)", used_end + 1, patch_sram_end, "patch_free"))
        else:
            sram_col.append(MemoryRegion(
                "Patch SRAM", patch_sram_origin, patch_sram_end, "patch_free"))

        sram_col.sort(key=lambda r: r.start)

        # ── Insert stack region ──
        # ARM Cortex-M stack grows downward from initial SP.
        # We don't know the exact depth, so show a fixed estimate
        # (2 KB) below SP.  The gradient fades out at the bottom
        # to convey the uncertain lower bound.
        STACK_ESTIMATE = 2048
        if sp and sp > sram_base:
            stack_bottom = sp - STACK_ESTIMATE
            # Clamp to not overlap existing regions
            for r in sram_col:
                if r.end >= stack_bottom and r.end < sp:
                    stack_bottom = r.end + 1
            if stack_bottom < sp:
                sram_col.append(MemoryRegion(
                    "Stack", stack_bottom, sp - 1, "stack"))
                sram_col.sort(key=lambda r: r.start)

        # ── Fill gaps with 'free' regions ──
        sram_size = self.sram_size or 96 * 1024
        flash_base = 0x08000000

        flash_col = self._fill_gaps(flash_col, flash_base, flash_end_addr)
        sram_col = self._fill_gaps(sram_col, sram_base, sram_end_addr)

        # ── Render ──
        title = self.firmware_bin.stem.replace('_', ' ').title()
        svg = render_memmap_svg(flash_col, sram_col, flash_size, sram_size, title)
        out_path = self.memmap_svg or (self.build_dir / "memmap.svg")
        out_path.write_text(svg)
        print(f"Wrote: {out_path}")

        # Summary
        print(f"Flash patch: {patch_flash_used} / {patch_flash_len} bytes "
              f"({patch_flash_used * 100 // patch_flash_len}%)")
        print(f"SRAM patch:  {patch_sram_used} / {patch_sram_len} bytes "
              f"({patch_sram_used * 100 // patch_sram_len if patch_sram_len else 0}%)")
        if sp:
            print(f"Stack top:   {_fmt_addr(sp)}")

    @staticmethod
    def _fill_gaps(regions: list[MemoryRegion], base: int, end: int) -> list[MemoryRegion]:
        """Insert 'free' regions to fill gaps between declared regions."""
        if not regions:
            return [MemoryRegion("(unused)", base, end, "free")]

        filled: list[MemoryRegion] = []
        cursor = base

        for r in regions:
            if r.start > cursor:
                filled.append(MemoryRegion("(unused)", cursor, r.start - 1, "free"))
            filled.append(r)
            cursor = r.end + 1

        if cursor <= end:
            filled.append(MemoryRegion("(unused)", cursor, end, "free"))

        return filled

    def main(self) -> None:
        if len(sys.argv) < 2:
            print(f"Usage: {sys.argv[0]} <validate|generate|patch|memmap>")
            sys.exit(1)

        cmd = sys.argv[1]
        if cmd == "validate":
            self.cmd_validate()
        elif cmd == "generate":
            self.cmd_generate()
        elif cmd == "patch":
            self.cmd_patch()
        elif cmd == "memmap":
            self.cmd_memmap()
        else:
            print(f"Unknown command: {cmd}")
            sys.exit(1)
