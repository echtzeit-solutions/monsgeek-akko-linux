//! Key name → matrix position lookup for the M1 V5 HE 16×6 LED grid.
//!
//! The LED streaming protocol uses row-major positions: `pos = row * 16 + col`.
//! The firmware's key name table (`M1_V5_HE_KEY_NAMES`) is column-major:
//! `index = col * 6 + row`. This module bridges the two.

use crate::profile::M1_V5_HE_KEY_NAMES;

/// Matrix dimensions (must match led_stream.rs)
pub const COLS: usize = 16;
pub const ROWS: usize = 6;
pub const MATRIX_LEN: usize = COLS * ROWS; // 96

/// Convert a key name to its (row, col) position in the 16×6 LED grid.
///
/// Key names are case-insensitive. Accepts the canonical names from
/// `M1_V5_HE_KEY_NAMES` plus common aliases.
pub fn key_name_to_pos(name: &str) -> Option<(u8, u8)> {
    // Try aliases first
    let canonical = match name.to_ascii_lowercase().as_str() {
        "escape" => "Esc",
        "backspace" | "bs" => "Bksp",
        "delete" => "Del",
        "capslock" | "capslk" => "Caps",
        "lshift" | "leftshift" | "left_shift" => "LShift",
        "rshift" | "rightshift" | "right_shift" => "RShift",
        "lctrl" | "leftctrl" | "left_ctrl" | "leftcontrol" => "LCtrl",
        "rctrl" | "rightctrl" | "right_ctrl" | "rightcontrol" => "RCtrl",
        "lalt" | "leftalt" | "left_alt" => "LAlt",
        "ralt" | "rightalt" | "right_alt" => "RAlt",
        "lwin" | "leftwin" | "super" | "lsuper" | "lgui" | "meta" => "LWin",
        "spacebar" | "spc" => "Space",
        "pgup" | "pageup" | "page_up" => "PgUp",
        "pgdn" | "pagedown" | "page_down" | "pgdown" => "PgDn",
        "volumeup" | "volup" | "vol+" => "Vol+",
        "volumedown" | "voldown" | "vol-" => "Vol-",
        "backtick" | "grave" | "tilde" => "`",
        "minus" => "-",
        "equal" | "equals" | "plus" => "=",
        "leftbracket" | "lbracket" => "[",
        "rightbracket" | "rbracket" => "]",
        "backslash" | "bslash" => "\\",
        "semicolon" => ";",
        "apostrophe" | "quote" => "'",
        "comma" => ",",
        "period" | "dot" => ".",
        "slash" | "fwdslash" => "/",
        "return" | "ret" => "Enter",
        "up" | "uparrow" => "Up",
        "down" | "downarrow" => "Down",
        "left" | "leftarrow" => "Left",
        "right" | "rightarrow" => "Right",
        _ => "", // no alias match, fall through
    };

    let search = if canonical.is_empty() {
        name
    } else {
        canonical
    };

    // M1_V5_HE_KEY_NAMES is column-major: index = col * 6 + row
    // Find the index, then convert to (row, col)
    for (idx, &key_name) in M1_V5_HE_KEY_NAMES.iter().enumerate() {
        if idx >= MATRIX_LEN {
            break;
        }
        if key_name.is_empty() {
            continue;
        }
        if key_name.eq_ignore_ascii_case(search) {
            let col = idx / ROWS;
            let row = idx % ROWS;
            return Some((row as u8, col as u8));
        }
    }

    None
}

/// Convert a (row, col) position to the row-major matrix index for LED streaming.
pub fn pos_to_matrix_index(row: u8, col: u8) -> usize {
    row as usize * COLS + col as usize
}

/// Parse a key target string. Accepts:
/// - Key name: "F1", "Esc", "A"
/// - Row,col pair: "0,5" or "row=0,col=5"
/// - Matrix index: "#42"
///
/// Returns row-major matrix indices.
pub fn parse_key_target(s: &str) -> Result<Vec<usize>, String> {
    // Check for comma-separated row,col
    if let Some((row_s, col_s)) = s.split_once(',') {
        let row: u8 = row_s
            .trim()
            .parse()
            .map_err(|_| format!("invalid row: {row_s}"))?;
        let col: u8 = col_s
            .trim()
            .parse()
            .map_err(|_| format!("invalid col: {col_s}"))?;
        if (row as usize) < ROWS && (col as usize) < COLS {
            return Ok(vec![pos_to_matrix_index(row, col)]);
        } else {
            return Err(format!("position out of range: {row},{col}"));
        }
    }

    // Check for matrix index (#N)
    if let Some(idx_s) = s.strip_prefix('#') {
        let idx: usize = idx_s
            .parse()
            .map_err(|_| format!("invalid index: {idx_s}"))?;
        if idx < MATRIX_LEN {
            return Ok(vec![idx]);
        } else {
            return Err(format!("index out of range: {idx}"));
        }
    }

    // Try key name
    if let Some((row, col)) = key_name_to_pos(s) {
        return Ok(vec![pos_to_matrix_index(row, col)]);
    }

    Err(format!("unknown key: {s}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_esc_position() {
        // Esc is at column-major index 0: col=0, row=0
        assert_eq!(key_name_to_pos("Esc"), Some((0, 0)));
        assert_eq!(key_name_to_pos("esc"), Some((0, 0)));
        assert_eq!(key_name_to_pos("escape"), Some((0, 0)));
    }

    #[test]
    fn test_f1_position() {
        // F1 is at column-major index 6: col=1, row=0
        assert_eq!(key_name_to_pos("F1"), Some((0, 1)));
    }

    #[test]
    fn test_space_position() {
        // Space is at column-major index 41: col=6, row=5
        assert_eq!(key_name_to_pos("Space"), Some((5, 6)));
    }

    #[test]
    fn test_enter_position() {
        // Enter is at column-major index 81: col=13, row=3
        assert_eq!(key_name_to_pos("Enter"), Some((3, 13)));
    }

    #[test]
    fn test_matrix_index() {
        // F1 at (0,1) → index 1 in row-major
        assert_eq!(pos_to_matrix_index(0, 1), 1);
        // Space at (5,6) → index 86
        assert_eq!(pos_to_matrix_index(5, 6), 86);
    }

    #[test]
    fn test_parse_key_target_name() {
        assert_eq!(parse_key_target("F1").unwrap(), vec![1]);
    }

    #[test]
    fn test_parse_key_target_rowcol() {
        assert_eq!(parse_key_target("0,1").unwrap(), vec![1]);
    }

    #[test]
    fn test_parse_key_target_index() {
        assert_eq!(parse_key_target("#42").unwrap(), vec![42]);
    }
}
