#!/usr/bin/env python3
"""
Generate C headers and linker scripts from Ghidra-exported symbols JSON.

Reads the output of ExportSymbols.java and produces:
  - fw_{name}.h          — Extern header (proper declarations, for flash patches)
  - fw_{name}_macro.h    — Macro header (function pointers, for SRAM shellcode)
  - fw_symbols.ld        — Linker symbol definitions (used with extern header)
  - patch.ld             — Linker script with computed patch zone

Usage:
  python3 generate_patch_files.py symbols.json -o patch/
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


# AT32F405 sector erase granularity
SECTOR_SIZE = 2048

# Address ranges for classification
SRAM_BASE = 0x20000000
SRAM_END = 0x2FFFFFFF
PERIPH_BASE = 0x40000000
PERIPH_END = 0x5FFFFFFF
FLASH_BASE = 0x08000000


@dataclass
class MemBlock:
    name: str
    start: int
    end: int
    size: int
    perms: str
    initialized: bool


@dataclass
class Function:
    name: str
    addr: int
    size: int
    ret: str
    cc: str
    params: list[dict[str, str]]


@dataclass
class Label:
    name: str
    addr: int
    primary: bool
    data_type: str | None = None
    data_size: int | None = None


@dataclass
class StructType:
    name: str
    size: int
    category: str
    fields: list[dict[str, Any]]


@dataclass
class EnumType:
    name: str
    size: int
    category: str
    members: list[dict[str, Any]]


@dataclass
class SymbolDB:
    program_name: str = ""
    image_base: int = 0
    arch: str = ""
    endian: str = ""
    pointer_size: int = 4
    memory_blocks: list[MemBlock] = field(default_factory=list)
    functions: list[Function] = field(default_factory=list)
    labels: list[Label] = field(default_factory=list)
    structs: list[StructType] = field(default_factory=list)
    enums: list[EnumType] = field(default_factory=list)


def parse_addr(s: str) -> int:
    """Parse hex address string like '0x08005000' or '08005000'."""
    return int(s, 16)


def load_symbols(path: Path) -> SymbolDB:
    """Load symbols JSON into a SymbolDB."""
    data = json.loads(path.read_text())
    db = SymbolDB()

    prog = data["program"]
    db.program_name = prog["name"]
    db.image_base = parse_addr(prog["image_base"])
    db.arch = prog["arch"]
    db.endian = prog.get("endian", "little")
    db.pointer_size = prog.get("pointer_size", 4)

    for b in data.get("memory_blocks", []):
        db.memory_blocks.append(MemBlock(
            name=b["name"],
            start=parse_addr(b["start"]),
            end=parse_addr(b["end"]),
            size=b["size"],
            perms=b["perms"],
            initialized=b["initialized"],
        ))

    for f in data.get("functions", []):
        db.functions.append(Function(
            name=f["name"],
            addr=parse_addr(f["addr"]),
            size=f.get("size", 0),
            ret=f.get("ret", "void"),
            cc=f.get("cc", "default"),
            params=f.get("params", []),
        ))

    for l in data.get("labels", []):
        db.labels.append(Label(
            name=l["name"],
            addr=parse_addr(l["addr"]),
            primary=l.get("primary", True),
            data_type=l.get("data_type"),
            data_size=l.get("data_size"),
        ))

    for s in data.get("structs", []):
        db.structs.append(StructType(
            name=s["name"],
            size=s["size"],
            category=s.get("category", ""),
            fields=s.get("fields", []),
        ))

    for e in data.get("enums", []):
        db.enums.append(EnumType(
            name=e["name"],
            size=e["size"],
            category=e.get("category", ""),
            members=e.get("members", []),
        ))

    return db


def find_code_block(db: SymbolDB) -> MemBlock | None:
    """Find the largest executable flash block (the firmware code region)."""
    candidates = [b for b in db.memory_blocks
                  if 'x' in b.perms and b.start >= FLASH_BASE and b.start < SRAM_BASE]
    if not candidates:
        return None
    return max(candidates, key=lambda b: b.size)


def find_config_blocks(db: SymbolDB, code_block: MemBlock) -> list[MemBlock]:
    """Find flash blocks after the code block (config, keymaps, etc.)."""
    return sorted(
        [b for b in db.memory_blocks
         if b.start > code_block.end and b.start >= FLASH_BASE and b.start < SRAM_BASE],
        key=lambda b: b.start,
    )


def is_sram(addr: int) -> bool:
    return SRAM_BASE <= addr <= SRAM_END


def is_periph(addr: int) -> bool:
    return PERIPH_BASE <= addr <= PERIPH_END


def is_flash(addr: int) -> bool:
    return FLASH_BASE <= addr < SRAM_BASE


def addr_in_block(addr: int, block: MemBlock) -> bool:
    return block.start <= addr <= block.end


def align_up(val: int, alignment: int) -> int:
    return (val + alignment - 1) & ~(alignment - 1)


# --- Ghidra type to C type mapping ---

GHIDRA_TO_C = {
    "byte": "uint8_t",
    "ubyte": "uint8_t",
    "sbyte": "int8_t",
    "char": "char",
    "uchar": "uint8_t",
    "short": "int16_t",
    "ushort": "uint16_t",
    "word": "uint16_t",
    "int": "int32_t",
    "uint": "uint32_t",
    "dword": "uint32_t",
    "long": "int32_t",
    "ulong": "uint32_t",
    "longlong": "int64_t",
    "ulonglong": "uint64_t",
    "float": "float",
    "double": "double",
    "void": "void",
    "bool": "bool",
    "undefined": "uint8_t",
    "undefined1": "uint8_t",
    "undefined2": "uint16_t",
    "undefined4": "uint32_t",
    "undefined8": "uint64_t",
    "pointer": "void *",
    "pointer32": "void *",
    "addr": "void *",
}


def ghidra_type_to_c(gtype: str) -> str:
    """Convert a Ghidra display type name to a C type."""
    if not gtype:
        return "uint8_t"
    # Handle pointer types: "type *"
    if gtype.endswith(" *"):
        inner = ghidra_type_to_c(gtype[:-2].strip())
        return f"{inner} *"
    # Handle array types: "type[N]"
    m = re.match(r"^(.+)\[(\d+)\]$", gtype)
    if m:
        return ghidra_type_to_c(m.group(1).strip())
    # Direct map
    low = gtype.lower()
    if low in GHIDRA_TO_C:
        return GHIDRA_TO_C[low]
    # Standard C types pass through
    if re.match(r"^u?int\d+_t$", gtype):
        return gtype
    # Struct/enum names pass through
    return gtype


def format_c_type_for_size(data_size: int | None) -> str:
    """Pick a volatile C type based on data size."""
    if data_size is None or data_size <= 1:
        return "uint8_t"
    if data_size == 2:
        return "uint16_t"
    if data_size == 4:
        return "uint32_t"
    return "uint8_t"


def is_scalar_type(ctype: str) -> bool:
    """Return True if the type represents a scalar (not a pointer/array)."""
    return ctype in ("uint8_t", "uint16_t", "uint32_t", "uint64_t",
                     "int8_t", "int16_t", "int32_t", "int64_t",
                     "float", "double", "char", "bool")


# --- Label classification (shared between both header modes) ---

@dataclass
class ClassifiedLabels:
    flash: list[Label] = field(default_factory=list)
    rom_data: list[Label] = field(default_factory=list)
    ram: list[Label] = field(default_factory=list)
    periph: list[Label] = field(default_factory=list)
    other: list[Label] = field(default_factory=list)


def classify_labels(db: SymbolDB, code_block: MemBlock | None,
                    config_blocks: list[MemBlock]) -> ClassifiedLabels:
    """Sort labels into flash/rom/ram/periph/other buckets."""
    cl = ClassifiedLabels()
    for label in sorted(db.labels, key=lambda l: l.addr):
        addr = label.addr
        if is_sram(addr):
            cl.ram.append(label)
        elif is_periph(addr):
            cl.periph.append(label)
        elif is_flash(addr):
            in_config = any(addr_in_block(addr, cb) for cb in config_blocks)
            in_code = code_block and addr_in_block(addr, code_block)
            if in_config:
                cl.flash.append(label)
            elif in_code:
                cl.rom_data.append(label)
            else:
                cl.other.append(label)
        else:
            cl.other.append(label)
    return cl


def label_c_type(label: Label) -> tuple[str, bool]:
    """Return (c_type, is_scalar) for a label based on Ghidra metadata."""
    has_explicit_type = label.data_type is not None or label.data_size is not None
    if label.data_type:
        ctype = ghidra_type_to_c(label.data_type)
    elif label.data_size is not None:
        ctype = format_c_type_for_size(label.data_size)
    else:
        ctype = "uint8_t"
    return ctype, has_explicit_type and is_scalar_type(ctype)


def format_struct_field(fld: dict[str, Any]) -> tuple[str, str]:
    """Return (c_type, array_suffix) for a struct field.

    Array types like 'byte[126]' become ('uint8_t', '[126]').
    Scalar types like 'uint' become ('uint32_t', '').
    """
    ftype_raw = fld.get("type", "uint8_t")
    m = re.match(r"^(.+)\[(\d+)\]$", ftype_raw)
    if m:
        base = ghidra_type_to_c(m.group(1).strip())
        return base, f"[{m.group(2)}]"
    return ghidra_type_to_c(ftype_raw), ""


def _topo_sort_structs(structs: list[StructType]) -> list[StructType]:
    """Topologically sort structs so dependencies (field types) come first.

    Falls back to alphabetical order for structs with no inter-dependencies.
    """
    struct_names = {s.name for s in structs}
    by_name = {s.name: s for s in structs}

    # Build dependency graph: s depends on types of its fields
    deps: dict[str, set[str]] = {s.name: set() for s in structs}
    for s in structs:
        for fld in s.fields:
            ftype_raw = fld.get("type", "")
            # Strip array suffix and pointer stars
            base = re.sub(r"\[.*\]$", "", ftype_raw).strip().rstrip(" *")
            if base in struct_names and base != s.name:
                deps[s.name].add(base)

    # Kahn's algorithm
    result: list[str] = []
    no_deps = sorted([n for n, d in deps.items() if not d])
    while no_deps:
        n = no_deps.pop(0)
        result.append(n)
        for m in sorted(deps):
            if n in deps[m]:
                deps[m].discard(n)
                if not deps[m] and m not in result:
                    no_deps.append(m)
                    no_deps.sort()
    # Append any remaining (cycles) alphabetically
    for s in sorted(deps):
        if s not in result:
            result.append(s)

    return [by_name[n] for n in result]


def emit_structs_and_enums(db: SymbolDB, emit) -> None:
    """Emit struct and enum typedefs (shared between both header modes)."""
    if db.structs:
        emit("")
        emit("/* " + "\u2500" * 2 + " Data types (structs) " + "\u2500" * 49 + " */")
        for s in _topo_sort_structs(db.structs):
            emit("")
            emit(f"typedef struct __attribute__((packed)) {{")
            for fld in s.fields:
                fname = fld.get("name", "")
                fsize = fld.get("size", 1)
                foffset = fld.get("offset", 0)
                if not fname:
                    fname = f"_offset_{foffset:#x}"
                ctype, arr = format_struct_field(fld)
                emit(f"    {ctype} {fname}{arr};  /* offset {foffset:#x}, {fsize}B */")
            emit(f"}} {s.name};  /* {s.size} bytes */")

    if db.enums:
        emit("")
        emit("/* " + "\u2500" * 2 + " Data types (enums) " + "\u2500" * 51 + " */")
        for e in sorted(db.enums, key=lambda e: e.name):
            emit("")
            emit(f"typedef enum {{")
            for i, m in enumerate(sorted(e.members, key=lambda m: m["value"])):
                comma = "," if i < len(e.members) - 1 else ""
                emit(f"    {m['name']} = {m['value']:#x}{comma}")
            emit(f"}} {e.name};")


# --- Macro header generation (for SRAM shellcode) ---

def generate_macro_header(db: SymbolDB, header_name: str | None = None) -> str:
    """Generate the macro-based C header (function pointers + address casts)."""
    code_block = find_code_block(db)
    config_blocks = find_config_blocks(db, code_block) if code_block else []
    cl = classify_labels(db, code_block, config_blocks)

    # Derive guard name
    prog_clean = re.sub(r"[^a-zA-Z0-9]", "_", db.program_name).upper()
    guard = f"FW_{prog_clean}_H"
    if header_name:
        guard = re.sub(r"[^a-zA-Z0-9]", "_", header_name.replace(".h", "")).upper() + "_H"

    lines: list[str] = []

    def emit(s: str = "") -> None:
        lines.append(s)

    # --- Preamble ---
    emit(f"/* Auto-generated from Ghidra project '{db.program_name}'. Do not edit manually.")
    emit(f" * Macro header — addresses inlined as casts.  Use for SRAM shellcode where")
    emit(f" * linker symbol resolution (BL) can't reach firmware flash. */")
    emit(f"#ifndef {guard}")
    emit(f"#define {guard}")
    emit("")
    emit("#include <stdint.h>")
    emit("#include <stdbool.h>")

    # --- Memory layout ---
    emit("")
    emit("/* " + "\u2500" * 2 + " Memory layout " + "\u2500" * 55 + " */")
    for b in db.memory_blocks:
        emit(f"/* {b.name:20s}  {b.start:#010x} - {b.end:#010x}  "
             f"{b.size:>7d}B  {b.perms:3s}  {'init' if b.initialized else 'uninit'} */")

    # --- Structs/enums first, so types are available for cast expressions ---
    emit_structs_and_enums(db, emit)

    if cl.flash:
        emit("")
        emit("/* " + "\u2500" * 2 + " Flash regions " + "\u2500" * 55 + " */")
        emit("")
        max_name = max(len(l.name) for l in cl.flash)
        for label in cl.flash:
            pad = " " * (max_name - len(label.name))
            emit(f"#define {label.name}{pad}  "
                 f"((volatile uint8_t *){label.addr:#010x})")

    if cl.rom_data:
        emit("")
        emit("/* " + "\u2500" * 2 + " ROM data (firmware flash) " + "\u2500" * 44 + " */")
        emit("")
        max_name = max(len(l.name) for l in cl.rom_data)
        for label in cl.rom_data:
            ctype = "uint8_t"
            if label.data_type:
                ctype = ghidra_type_to_c(label.data_type)
            pad = " " * (max_name - len(label.name))
            emit(f"#define {label.name}{pad}  "
                 f"((const {ctype} *){label.addr:#010x})")

    # --- RAM globals ---
    if cl.ram:
        emit("")
        emit("/* " + "\u2500" * 2 + " RAM globals " + "\u2500" * 57 + " */")
        emit("")
        max_name = max(len(l.name) for l in cl.ram)
        for label in cl.ram:
            ctype, scalar = label_c_type(label)
            pad = " " * (max_name - len(label.name))
            if scalar:
                emit(f"#define {label.name}{pad}  "
                     f"(*(volatile {ctype} *){label.addr:#010x})")
            else:
                emit(f"#define {label.name}{pad}  "
                     f"((volatile {ctype} *){label.addr:#010x})")

    # --- MMIO registers ---
    if cl.periph:
        emit("")
        emit("/* " + "\u2500" * 2 + " MMIO registers " + "\u2500" * 54 + " */")
        emit("")
        max_name = max(len(l.name) for l in cl.periph)
        for label in cl.periph:
            pad = " " * (max_name - len(label.name))
            emit(f"#define {label.name}{pad}  "
                 f"(*(volatile uint32_t *){label.addr:#010x})")

    # --- Other labels ---
    if cl.other:
        emit("")
        emit("/* " + "\u2500" * 2 + " Other labels " + "\u2500" * 56 + " */")
        emit("")
        max_name = max(len(l.name) for l in cl.other)
        for label in cl.other:
            pad = " " * (max_name - len(label.name))
            emit(f"#define {label.name}{pad}  {label.addr:#010x}")

    # --- Firmware functions ---
    # Group functions by category based on name prefix patterns
    funcs = sorted(db.functions, key=lambda f: f.addr)
    if funcs:
        emit("")
        emit("/* " + "\u2500" * 2 + " Firmware functions (Thumb) " + "\u2500" * 43 + " */")
        emit("")
        max_name = max(len(f.name) for f in funcs)
        for func in funcs:
            sig = format_func_signature(func)
            pad = " " * (max_name - len(func.name))
            emit(f"#define {func.name}{pad}  (({sig})({func.addr:#010x} | 1))")

    # --- Patch zone info ---
    if code_block and config_blocks:
        code_end = code_block.end + 1
        first_config = config_blocks[0].start
        patch_origin = align_up(code_end, SECTOR_SIZE)
        patch_length = first_config - patch_origin
        if patch_length > 0:
            emit("")
            emit("/* " + "\u2500" * 2 + " Patch zone " + "\u2500" * 58 + " */")
            emit("")
            emit(f"#define PATCH_ZONE_START  {patch_origin:#010x}")
            emit(f"#define PATCH_ZONE_END    {first_config - 1:#010x}")
            emit(f"#define PATCH_ZONE_SIZE   {patch_length}  /* {patch_length // 1024} KB */")

    emit("")
    emit(f"#endif /* {guard} */")
    emit("")

    return "\n".join(lines)


def format_func_signature(func: Function) -> str:
    """Format a function's C type signature: ret_type (*)(param_types).

    Ghidra exports params with types like 'undefined4' (= 4-byte register arg)
    which map to uint32_t — perfectly valid for calling convention purposes.
    We emit these as proper C types so the function can be called by name.
    """
    # For return types, "undefined" means Ghidra hasn't determined it → use void
    if func.ret.startswith("undefined"):
        ret = "void"
    else:
        ret = ghidra_type_to_c(func.ret)
    params = func.params

    if not params:
        return f"{ret} (*)(void)"

    param_strs = []
    for p in params:
        ptype = ghidra_type_to_c(p.get("type", "undefined"))
        pname = p.get("name", "")
        if pname and pname != "param_1" and not pname.startswith("in_"):
            param_strs.append(f"{ptype} {pname}")
        else:
            param_strs.append(ptype)

    return f"{ret} (*)({', '.join(param_strs)})"


def func_ret_type(func: Function) -> str:
    """Return the C return type for a function."""
    if func.ret.startswith("undefined"):
        return "void"
    return ghidra_type_to_c(func.ret)


def format_func_declaration(func: Function) -> str:
    """Format a function as a C declaration: ret_type name(params);"""
    ret = func_ret_type(func)
    params = func.params

    if not params:
        return f"{ret} {func.name}(void)"

    param_strs = []
    for p in params:
        ptype = ghidra_type_to_c(p.get("type", "undefined"))
        pname = p.get("name", "")
        if pname and pname != "param_1" and not pname.startswith("in_"):
            param_strs.append(f"{ptype} {pname}")
        else:
            param_strs.append(ptype)

    return f"{ret} {func.name}({', '.join(param_strs)})"


# --- Extern header generation (for flash patches) ---

def generate_extern_header(db: SymbolDB, header_name: str) -> str:
    """Generate extern-declaration header for use with linker-resolved symbols.

    Functions become regular C declarations; data labels become extern variables.
    Addresses are provided by a companion fw_symbols.ld file.
    """
    code_block = find_code_block(db)
    config_blocks = find_config_blocks(db, code_block) if code_block else []
    cl = classify_labels(db, code_block, config_blocks)

    guard = re.sub(r"[^a-zA-Z0-9]", "_", header_name.replace(".h", "")).upper() + "_H"

    lines: list[str] = []

    def emit(s: str = "") -> None:
        lines.append(s)

    emit(f"/* Auto-generated from Ghidra project '{db.program_name}'. Do not edit manually.")
    emit(f" * Extern header — link with fw_symbols.ld to resolve addresses.")
    emit(f" * For flash patches where BL can reach firmware code directly. */")
    emit(f"#ifndef {guard}")
    emit(f"#define {guard}")
    emit("")
    emit("#include <stdint.h>")
    emit("#include <stdbool.h>")

    # Build set of function names to suppress duplicate label declarations
    func_names = {f.name for f in db.functions}

    # --- Memory layout (same comment block) ---
    emit("")
    emit("/* " + "\u2500" * 2 + " Memory layout " + "\u2500" * 55 + " */")
    for b in db.memory_blocks:
        emit(f"/* {b.name:20s}  {b.start:#010x} - {b.end:#010x}  "
             f"{b.size:>7d}B  {b.perms:3s}  {'init' if b.initialized else 'uninit'} */")

    # --- Structs/enums first, so types are available for extern declarations ---
    emit_structs_and_enums(db, emit)

    # Struct type names that are defined in this header (available for use)
    defined_types = ({s.name for s in db.structs} | {e.name for e in db.enums}
                     | set(GHIDRA_TO_C.values()) | {"bool"})

    def extern_safe_type(ctype: str) -> str:
        """Fall back to uint8_t for struct types that aren't defined."""
        base = ctype.rstrip(" *").strip()
        if base in defined_types or re.match(r"^u?int\d+_t$", base):
            return ctype
        return "uint8_t"

    # --- Flash config regions ---
    flash_out = [l for l in cl.flash if l.name not in func_names]
    if flash_out:
        emit("")
        emit("/* " + "\u2500" * 2 + " Flash regions " + "\u2500" * 55 + " */")
        for label in flash_out:
            emit(f"extern volatile uint8_t {label.name}[];")

    # --- ROM data ---
    rom_out = [l for l in cl.rom_data if l.name not in func_names]
    if rom_out:
        emit("")
        emit("/* " + "\u2500" * 2 + " ROM data (firmware flash) " + "\u2500" * 44 + " */")
        for label in rom_out:
            ctype = "uint8_t"
            if label.data_type:
                ctype = extern_safe_type(ghidra_type_to_c(label.data_type))
            emit(f"extern const {ctype} {label.name}[];")

    # --- RAM globals ---
    ram_out = [l for l in cl.ram if l.name not in func_names]
    if ram_out:
        emit("")
        emit("/* " + "\u2500" * 2 + " RAM globals " + "\u2500" * 57 + " */")
        for label in ram_out:
            ctype, scalar = label_c_type(label)
            ctype = extern_safe_type(ctype)
            scalar = scalar and is_scalar_type(ctype)
            if scalar:
                emit(f"extern volatile {ctype} {label.name};")
            else:
                emit(f"extern volatile {ctype} {label.name}[];")

    # --- MMIO registers ---
    periph_out = [l for l in cl.periph if l.name not in func_names]
    if periph_out:
        emit("")
        emit("/* " + "\u2500" * 2 + " MMIO registers " + "\u2500" * 54 + " */")
        for label in periph_out:
            emit(f"extern volatile uint32_t {label.name};")

    # --- Firmware functions ---
    funcs = sorted(db.functions, key=lambda f: f.addr)
    if funcs:
        emit("")
        emit("/* " + "\u2500" * 2 + " Firmware functions " + "\u2500" * 50 + " */")
        for func in funcs:
            emit(f"{format_func_declaration(func)};")

    # --- Patch zone info ---
    if code_block and config_blocks:
        code_end = code_block.end + 1
        first_config = config_blocks[0].start
        patch_origin = align_up(code_end, SECTOR_SIZE)
        patch_length = first_config - patch_origin
        if patch_length > 0:
            emit("")
            emit("/* " + "\u2500" * 2 + " Patch zone " + "\u2500" * 58 + " */")
            emit("")
            emit(f"#define PATCH_ZONE_START  {patch_origin:#010x}")
            emit(f"#define PATCH_ZONE_END    {first_config - 1:#010x}")
            emit(f"#define PATCH_ZONE_SIZE   {patch_length}  /* {patch_length // 1024} KB */")

    emit("")
    emit(f"#endif /* {guard} */")
    emit("")

    return "\n".join(lines)


# --- Linker symbol definitions ---

def generate_symbols_ld(db: SymbolDB) -> str:
    """Generate linker symbol definitions for all firmware addresses.

    Functions get Thumb addresses (bit 0 set) so BL generates correct
    Thumb interworking.  Data symbols get raw addresses.
    """
    code_block = find_code_block(db)
    config_blocks = find_config_blocks(db, code_block) if code_block else []
    cl = classify_labels(db, code_block, config_blocks)

    lines: list[str] = []

    def emit(s: str = "") -> None:
        lines.append(s)

    emit(f"/* Auto-generated from Ghidra project '{db.program_name}'. Do not edit manually.")
    emit(f" * Firmware symbol addresses — use with fw_*_extern.h.")
    emit(f" * Link via: ld -T patch.ld -T fw_symbols.ld ... */")

    # Function names take priority — skip labels that duplicate function names
    func_names = {f.name for f in db.functions}

    # --- Functions (Thumb bit set) ---
    funcs = sorted(db.functions, key=lambda f: f.addr)
    if funcs:
        emit("")
        emit("/* " + "\u2500" * 2 + " Firmware functions (Thumb, bit 0 set) " + "\u2500" * 31 + " */")
        max_name = max(len(f.name) for f in funcs)
        for func in funcs:
            pad = " " * (max_name - len(func.name))
            emit(f"{func.name}{pad} = {func.addr | 1:#010x};")

    # --- Flash config labels ---
    flash_out = [l for l in cl.flash if l.name not in func_names]
    if flash_out:
        emit("")
        emit("/* " + "\u2500" * 2 + " Flash regions " + "\u2500" * 55 + " */")
        max_name = max(len(l.name) for l in flash_out)
        for label in flash_out:
            pad = " " * (max_name - len(label.name))
            emit(f"{label.name}{pad} = {label.addr:#010x};")

    # --- ROM data ---
    rom_out = [l for l in cl.rom_data if l.name not in func_names]
    if rom_out:
        emit("")
        emit("/* " + "\u2500" * 2 + " ROM data (firmware flash) " + "\u2500" * 44 + " */")
        max_name = max(len(l.name) for l in rom_out)
        for label in rom_out:
            pad = " " * (max_name - len(label.name))
            emit(f"{label.name}{pad} = {label.addr:#010x};")

    # --- RAM globals ---
    ram_out = [l for l in cl.ram if l.name not in func_names]
    if ram_out:
        emit("")
        emit("/* " + "\u2500" * 2 + " RAM globals " + "\u2500" * 57 + " */")
        max_name = max(len(l.name) for l in ram_out)
        for label in ram_out:
            pad = " " * (max_name - len(label.name))
            emit(f"{label.name}{pad} = {label.addr:#010x};")

    # --- MMIO registers ---
    periph_out = [l for l in cl.periph if l.name not in func_names]
    if periph_out:
        emit("")
        emit("/* " + "\u2500" * 2 + " MMIO registers " + "\u2500" * 54 + " */")
        max_name = max(len(l.name) for l in periph_out)
        for label in periph_out:
            pad = " " * (max_name - len(label.name))
            emit(f"{label.name}{pad} = {label.addr:#010x};")

    # --- Other labels ---
    other_out = [l for l in cl.other if l.name not in func_names]
    if other_out:
        emit("")
        emit("/* " + "\u2500" * 2 + " Other labels " + "\u2500" * 56 + " */")
        max_name = max(len(l.name) for l in other_out)
        for label in other_out:
            pad = " " * (max_name - len(label.name))
            emit(f"{label.name}{pad} = {label.addr:#010x};")

    emit("")
    return "\n".join(lines)


# --- Linker script generation ---

def generate_linker_script(db: SymbolDB) -> str | None:
    """Generate the linker script content. Returns None if no patch zone found."""
    code_block = find_code_block(db)
    if not code_block:
        print("WARNING: No executable flash block found, skipping linker script",
              file=sys.stderr)
        return None

    config_blocks = find_config_blocks(db, code_block)
    if not config_blocks:
        print("WARNING: No config blocks found after code, skipping linker script",
              file=sys.stderr)
        return None

    code_end = code_block.end + 1
    first_config = config_blocks[0].start
    patch_origin = align_up(code_end, SECTOR_SIZE)
    patch_length = first_config - patch_origin

    if patch_length <= 0:
        print("WARNING: No gap between code and config blocks", file=sys.stderr)
        return None

    lines: list[str] = []

    def emit(s: str = "") -> None:
        lines.append(s)

    emit(f"/* Auto-generated from Ghidra project '{db.program_name}'. Do not edit manually.")
    emit(f" * Patch zone: gap between firmware code and config region.")
    emit(f" * Aligned to {SECTOR_SIZE}-byte sector boundary (AT32F405 erase granularity). */")
    # PATCH_SRAM: scratch SRAM above all known firmware globals.
    # Firmware SRAM usage ends around 0x20009046; we start at 0x20009800 for safety.
    sram_origin = 0x20009800
    sram_length = 1024

    emit("MEMORY {")
    emit(f"    PATCH (rx)      : ORIGIN = {patch_origin:#010x}, LENGTH = {patch_length}")
    emit(f"    PATCH_SRAM (rw) : ORIGIN = {sram_origin:#010x}, LENGTH = {sram_length}")
    emit("}")
    emit("SECTIONS {")
    emit("    .text : {")
    emit("        *(.text*)")
    emit("    } > PATCH")
    emit("    .rodata : {")
    emit("        *(.rodata*)")
    emit("    } > PATCH")
    emit("    .bss (NOLOAD) : {")
    emit("        *(.bss*)")
    emit("        *(COMMON)")
    emit("    } > PATCH_SRAM")
    emit("    /DISCARD/ : {")
    emit("        *(.ARM.*)")
    emit("        *(.comment)")
    emit("        *(.note*)")
    emit("        *(.data*)")
    emit("    }")
    emit("}")
    emit("")

    return "\n".join(lines)


# --- Main ---

def main() -> int:
    parser = argparse.ArgumentParser(
        description="Generate C headers and linker scripts from Ghidra symbols JSON.")
    parser.add_argument("symbols_json", metavar="symbols.json",
                        help="Path to exported Ghidra symbols JSON")
    parser.add_argument("-o", "--output-dir", default=".",
                        help="Output directory (default: .)")
    parser.add_argument("--header-name",
                        help="Override macro header filename (default: fw_{name}.h)")
    parser.add_argument("--no-linker", action="store_true",
                        help="Skip linker script generation")
    parser.add_argument("--no-header", action="store_true",
                        help="Skip C header generation")
    args = parser.parse_args()

    symbols_path = Path(args.symbols_json)
    if not symbols_path.exists():
        print(f"ERROR: {symbols_path} not found", file=sys.stderr)
        return 1

    db = load_symbols(symbols_path)
    out_dir = Path(args.output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    print(f"Loaded: {db.program_name} (base {db.image_base:#010x}, {db.arch})")
    print(f"  {len(db.memory_blocks)} memory blocks, "
          f"{len(db.functions)} functions, "
          f"{len(db.labels)} labels, "
          f"{len(db.structs)} structs, "
          f"{len(db.enums)} enums")

    if not args.no_header:
        # Derive header filename from program name
        if args.header_name:
            macro_fname = args.header_name
        else:
            safe_name = re.sub(r"[^a-zA-Z0-9]", "_", db.program_name).lower()
            macro_fname = f"fw_{safe_name}.h"

        # Extern header (default — for flash patches with linker symbols)
        extern_content = generate_extern_header(db, macro_fname)
        extern_path = out_dir / macro_fname
        extern_path.write_text(extern_content)
        print(f"Wrote {extern_path}")

        # Macro header (for SRAM shellcode where BL can't reach)
        macro_suffix_fname = macro_fname.replace(".h", "_macro.h")
        macro_content = generate_macro_header(db, macro_suffix_fname)
        macro_path = out_dir / macro_suffix_fname
        macro_path.write_text(macro_content)
        print(f"Wrote {macro_path}")

        # Linker symbol definitions (companion to extern header)
        sym_ld_content = generate_symbols_ld(db)
        sym_ld_path = out_dir / "fw_symbols.ld"
        sym_ld_path.write_text(sym_ld_content)
        print(f"Wrote {sym_ld_path}")

    if not args.no_linker:
        ld_content = generate_linker_script(db)
        if ld_content:
            ld_path = out_dir / "patch.ld"
            ld_path.write_text(ld_content)
            print(f"Wrote {ld_path}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
