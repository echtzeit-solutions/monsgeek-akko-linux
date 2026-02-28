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
        """Print a summary of all hooks and patch zone usage."""
        used = self._alloc_ptr - self.patch_zone_start
        lines = [
            f"",
            f"Patch zone usage: {used} / {self.patch_zone_size} bytes "
            f"({used * 100 // self.patch_zone_size}%)",
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

    def main(self) -> None:
        if len(sys.argv) < 2:
            print(f"Usage: {sys.argv[0]} <validate|generate|patch>")
            sys.exit(1)

        cmd = sys.argv[1]
        if cmd == "validate":
            self.cmd_validate()
        elif cmd == "generate":
            self.cmd_generate()
        elif cmd == "patch":
            self.cmd_patch()
        else:
            print(f"Unknown command: {cmd}")
            sys.exit(1)
