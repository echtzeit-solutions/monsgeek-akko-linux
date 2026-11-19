//! `peek` / `poke`: read and write device memory over the vendor channel.
//!
//! Needs a patched firmware with the peek/poke capability (see
//! `docs/FIRMWARE_PATCH.md`). The firmware only touches flash, SRAM, the system
//! control block and the known register blocks.

use super::{CmdCtx, CommandResult, with_ry_keyboard};
use monsgeek_keyboard::RyKeyboard;
use monsgeek_keyboard::memory::Width;
use std::fmt::Write as _;

/// Parse a number as hex (`0x…`, `_` allowed between digits) or decimal.
fn parse_u32(text: &str) -> Result<u32, String> {
    let cleaned: String = text.trim().chars().filter(|&c| c != '_').collect();
    let parsed = match cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => cleaned.parse(),
    };
    parsed.map_err(|_| format!("'{text}' is not a number (use decimal or 0x… hex, up to 32 bits)"))
}

fn resolve_width(addr: u32, requested: Option<u8>) -> Result<Width, String> {
    match requested {
        None => Ok(Width::default_for(addr)),
        Some(n) => Width::from_bytes(n).ok_or_else(|| format!("width must be 1, 2 or 4, not {n}")),
    }
}

fn require_peek_poke(kb: &RyKeyboard) -> CommandResult {
    if kb.supports_peek_poke()? {
        return Ok(());
    }
    Err(
        "this firmware does not support peek/poke; flash a patched build that includes it \
         (see docs/FIRMWARE_PATCH.md)"
            .into(),
    )
}

/// A hex dump, one line per 16 bytes: the address, the data in `width`-byte
/// groups (little-endian values, as the CPU sees them), and for byte dumps the
/// printable characters.
fn format_dump(addr: u32, bytes: &[u8], width: Width) -> String {
    let w = width.bytes();
    let mut out = String::new();
    for (line_no, line) in bytes.chunks(16).enumerate() {
        let line_addr = addr.wrapping_add((line_no * 16) as u32);
        let _ = write!(out, "{line_addr:08x} ");
        for (i, group) in line.chunks(w).enumerate() {
            // an extra gap in the middle of a byte dump, as hexdump -C does
            if w == 1 && i == 8 {
                out.push(' ');
            }
            out.push(' ');
            for b in group.iter().rev() {
                let _ = write!(out, "{b:02x}");
            }
        }
        if w == 1 {
            // pad a short last line so the text column lines up
            let missing = 16 - line.len();
            out.push_str(&" ".repeat(missing * 3 + usize::from(line.len() <= 8)));
            out.push_str("  |");
            out.extend(line.iter().map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            }));
            out.push('|');
        }
        out.push('\n');
    }
    out
}

pub fn peek(ctx: &CmdCtx, addr: &str, len: &str, width: Option<u8>) -> CommandResult {
    let addr = parse_u32(addr)?;
    let len = parse_u32(len)? as usize;
    let width = resolve_width(addr, width)?;

    with_ry_keyboard(ctx, |kb| {
        require_peek_poke(kb)?;
        let bytes = kb.peek(addr, len, width)?;
        print!("{}", format_dump(addr, &bytes, width));
        Ok(())
    })
}

pub fn poke(ctx: &CmdCtx, addr: &str, value: &str, width: Option<u8>, yes: bool) -> CommandResult {
    let addr = parse_u32(addr)?;
    let value = parse_u32(value)?;
    let width = resolve_width(addr, width)?;

    if !yes {
        return Err(format!(
            "poke writes device memory: {value:#x} ({} byte{}) to {addr:#010x}. \
             Pass --yes to do it.",
            width.bytes(),
            if width.bytes() == 1 { "" } else { "s" }
        )
        .into());
    }

    with_ry_keyboard(ctx, |kb| {
        require_peek_poke(kb)?;
        let readback = kb.poke(addr, width, value)?;
        println!(
            "wrote {value:#x} ({} byte{}) to {addr:#010x}",
            width.bytes(),
            if width.bytes() == 1 { "" } else { "s" }
        );
        if addr < 0x4000_0000 {
            println!("read back {readback:#x}");
        } else {
            println!("(registers are not read back; peek it to check)");
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_parse_as_hex_or_decimal() {
        assert_eq!(parse_u32("0x20000000"), Ok(0x2000_0000));
        assert_eq!(parse_u32("0X2000_0000"), Ok(0x2000_0000));
        assert_eq!(parse_u32("64"), Ok(64));
        assert_eq!(parse_u32(" 0xff "), Ok(255));
        assert_eq!(parse_u32("0xFFFFFFFF"), Ok(u32::MAX));
    }

    #[test]
    fn bad_numbers_are_refused() {
        for bad in [
            "",
            "0x",
            "0x100000000",
            "-1",
            "kbd_state",
            "20000000h",
            "4294967296",
        ] {
            assert!(parse_u32(bad).is_err(), "{bad:?} should not parse");
        }
        assert!(parse_u32("nope").unwrap_err().contains("'nope'"));
    }

    #[test]
    fn width_defaults_follow_the_address() {
        assert_eq!(resolve_width(0x2000_0000, None), Ok(Width::Byte));
        assert_eq!(resolve_width(0x4002_0410, None), Ok(Width::Word));
        assert_eq!(resolve_width(0x2000_0000, Some(2)), Ok(Width::Half));
        assert!(resolve_width(0x2000_0000, Some(3)).is_err());
        assert!(resolve_width(0x2000_0000, Some(0)).is_err());
    }

    #[test]
    fn byte_dump_has_address_hex_and_text() {
        let bytes: Vec<u8> = (0x40..0x60).collect();
        let dump = format_dump(0x2000_0100, &bytes, Width::Byte);
        let lines: Vec<&str> = dump.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "20000100  40 41 42 43 44 45 46 47  48 49 4a 4b 4c 4d 4e 4f  |@ABCDEFGHIJKLMNO|"
        );
        assert!(lines[1].starts_with("20000110  50 51 52 53"));
        assert!(lines[1].ends_with("|PQRSTUVWXYZ[\\]^_|"));
    }

    #[test]
    fn short_byte_dump_keeps_the_text_column_aligned() {
        let full = format_dump(0, &[0x41; 16], Width::Byte);
        for len in [1usize, 5, 8, 9, 15] {
            let short = format_dump(0, &vec![0x41; len], Width::Byte);
            let bar = |s: &str| s.find('|').unwrap();
            assert_eq!(bar(&short), bar(&full), "len {len}");
        }
    }

    #[test]
    fn unprintable_bytes_show_as_dots() {
        let dump = format_dump(0, &[0x00, 0x1f, 0x7f, 0xff, b'A', b' '], Width::Byte);
        assert!(dump.trim_end().ends_with("|....A |"), "{dump}");
    }

    #[test]
    fn word_dump_shows_little_endian_values() {
        // 0xDEADBEEF stored LSB first, then 0x12345678
        let bytes = [0xEF, 0xBE, 0xAD, 0xDE, 0x78, 0x56, 0x34, 0x12];
        let dump = format_dump(0x4002_0410, &bytes, Width::Word);
        assert_eq!(dump, "40020410  deadbeef 12345678\n");
    }

    #[test]
    fn halfword_dump_groups_in_twos_and_addresses_advance_per_line() {
        let bytes: Vec<u8> = (0..20).collect();
        let dump = format_dump(0x2000_0000, &bytes, Width::Half);
        let lines: Vec<&str> = dump.lines().collect();
        assert_eq!(
            lines[0],
            "20000000  0100 0302 0504 0706 0908 0b0a 0d0c 0f0e"
        );
        assert_eq!(lines[1], "20000010  1110 1312");
    }

    #[test]
    fn dump_addresses_wrap_instead_of_panicking() {
        let dump = format_dump(0xFFFF_FFF0, &[0u8; 32], Width::Byte);
        assert!(dump.lines().nth(1).unwrap().starts_with("00000000 "));
    }
}
