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

/// Return sorted row-major indices for all keys matching a predicate.
///
/// The predicate receives `(row_major_index, key_name)` for each non-empty key.
fn keys_matching(pred: impl Fn(usize, &str) -> bool) -> Vec<usize> {
    let mut result = Vec::new();
    for (col_major_idx, &name) in M1_V5_HE_KEY_NAMES.iter().enumerate() {
        if col_major_idx >= MATRIX_LEN || name.is_empty() {
            continue;
        }
        let col = col_major_idx / ROWS;
        let row = col_major_idx % ROWS;
        let row_major = row * COLS + col;
        if pred(row_major, name) {
            result.push(row_major);
        }
    }
    result.sort_unstable();
    result
}

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
///
/// **Group selectors** (return multiple indices):
/// - `all` — every physical key
/// - `row0`..`row5` — all keys in a row
/// - `col0`..`col15` — all keys in a column
/// - `letters` — A-Z
/// - `frow` — F1-F12
/// - `numbers` — 1-0 number row
/// - `modifiers` — Shift, Ctrl, Alt, Win, Fn
/// - `Q..U` — key range (same row, inclusive)
/// - `#10..#30` — index range (inclusive)
///
/// **Single key selectors**:
/// - Key name: `F1`, `Esc`, `A`
/// - Row,col pair: `0,5`
/// - Matrix index: `#42`
///
/// Returns sorted row-major matrix indices.
pub fn parse_key_target(s: &str) -> Result<Vec<usize>, String> {
    let lower = s.to_ascii_lowercase();

    // --- Group selectors ---
    if lower == "all" {
        return Ok(keys_matching(|_, _| true));
    }
    if lower == "letters" {
        return Ok(keys_matching(|_, name| {
            name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic()
        }));
    }
    if lower == "frow" {
        return Ok(keys_matching(|_, name| {
            name.starts_with('F')
                && name.len() >= 2
                && name[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n))
        }));
    }
    if lower == "numbers" {
        return Ok(keys_matching(|_, name| {
            name.len() == 1 && name.as_bytes()[0].is_ascii_digit()
        }));
    }
    if lower == "modifiers" {
        const MODS: &[&str] = &[
            "LShift", "RShift", "LCtrl", "RCtrl", "LAlt", "RAlt", "LWin", "Fn",
        ];
        return Ok(keys_matching(|_, name| {
            MODS.iter().any(|m| m.eq_ignore_ascii_case(name))
        }));
    }

    // row<N>
    if let Some(n_s) = lower.strip_prefix("row") {
        if let Ok(row) = n_s.parse::<usize>() {
            if row < ROWS {
                return Ok(keys_matching(|idx, _| idx / COLS == row));
            }
            return Err(format!("row out of range: {row} (0-{max})", max = ROWS - 1));
        }
    }

    // col<N>
    if let Some(n_s) = lower.strip_prefix("col") {
        if let Ok(col) = n_s.parse::<usize>() {
            if col < COLS {
                return Ok(keys_matching(|idx, _| idx % COLS == col));
            }
            return Err(format!("col out of range: {col} (0-{max})", max = COLS - 1));
        }
    }

    // --- Range selectors (contain "..") ---
    if let Some((left, right)) = s.split_once("..") {
        // #N..#M — index range
        if let (Some(l_s), Some(r_s)) = (left.strip_prefix('#'), right.strip_prefix('#')) {
            let l: usize = l_s.parse().map_err(|_| format!("invalid index: {l_s}"))?;
            let r: usize = r_s.parse().map_err(|_| format!("invalid index: {r_s}"))?;
            if l > r {
                return Err(format!("index range is empty: #{l}..#{r}"));
            }
            let end = r.min(MATRIX_LEN - 1);
            // Only include indices that have a physical key
            return Ok(keys_matching(|idx, _| idx >= l && idx <= end));
        }

        // Key..Key — same-row range
        let (l_row, l_col) = key_name_to_pos(left).ok_or_else(|| format!("unknown key: {left}"))?;
        let (r_row, r_col) =
            key_name_to_pos(right).ok_or_else(|| format!("unknown key: {right}"))?;
        if l_row != r_row {
            return Err(format!(
                "range keys must be on the same row: {left} (row {l_row}) vs {right} (row {r_row})"
            ));
        }
        let (min_col, max_col) = if l_col <= r_col {
            (l_col, r_col)
        } else {
            (r_col, l_col)
        };
        let row = l_row as usize;
        return Ok(keys_matching(|idx, _| {
            idx / COLS == row && {
                let c = idx % COLS;
                c >= min_col as usize && c <= max_col as usize
            }
        }));
    }

    // --- Single-key selectors ---

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

    #[test]
    fn test_all() {
        let all = parse_key_target("all").unwrap();
        // There are ~82 physical keys (non-empty names in first 96 positions)
        assert!(all.len() > 70 && all.len() < 96, "got {} keys", all.len());
        // Should be sorted
        assert!(all.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn test_row0() {
        let row = parse_key_target("row0").unwrap();
        // Row 0: Esc, F1-F12, Del, Vol+ — varies by layout
        assert!(!row.is_empty());
        // All indices should be in 0..16
        assert!(row.iter().all(|&i| i < COLS));
    }

    #[test]
    fn test_col0() {
        let col = parse_key_target("col0").unwrap();
        // Col 0: Esc, `, Tab, Caps, LShift, LCtrl
        assert_eq!(col.len(), 6);
        assert_eq!(col, vec![0, 16, 32, 48, 64, 80]);
    }

    #[test]
    fn test_letters() {
        let letters = parse_key_target("letters").unwrap();
        assert_eq!(letters.len(), 26);
        // Should be sorted row-major
        assert!(letters.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn test_frow() {
        let frow = parse_key_target("frow").unwrap();
        assert_eq!(frow.len(), 12);
        // F1 is at row 0, col 1 → index 1
        assert_eq!(frow[0], 1);
        // F12 is at row 0, col 12 → index 12
        assert_eq!(frow[11], 12);
    }

    #[test]
    fn test_numbers() {
        let numbers = parse_key_target("numbers").unwrap();
        assert_eq!(numbers.len(), 10);
        // "1" at row 1, col 1 → 17; "0" at row 1, col 10 → 26
        assert_eq!(numbers[0], 17);
        assert_eq!(numbers[9], 26);
    }

    #[test]
    fn test_modifiers() {
        let mods = parse_key_target("modifiers").unwrap();
        // LShift, RShift, LCtrl, RCtrl, LAlt, RAlt, LWin, Fn = 8
        assert_eq!(mods.len(), 8);
    }

    #[test]
    fn test_key_range_q_to_u() {
        // Q(row2,col1) .. U(row2,col7) — same row
        let range = parse_key_target("Q..U").unwrap();
        // Should include Q, W, E, R, T, Y, U = 7 keys (all in row 2, cols 1-7)
        assert_eq!(range.len(), 7);
        // Q at row 2, col 1 → 33
        assert_eq!(range[0], 33);
    }

    #[test]
    fn test_key_range_different_rows() {
        assert!(parse_key_target("Q..A").is_err());
    }

    #[test]
    fn test_index_range() {
        let range = parse_key_target("#10..#20").unwrap();
        // Only non-empty keys in index range 10..=20
        assert!(!range.is_empty());
        assert!(range.iter().all(|&i| i >= 10 && i <= 20));
    }

    #[test]
    fn test_existing_selectors_still_work() {
        // Single key name
        assert_eq!(parse_key_target("Esc").unwrap(), vec![0]);
        // Row,col
        assert_eq!(parse_key_target("0,1").unwrap(), vec![1]);
        // Index
        assert_eq!(parse_key_target("#1").unwrap(), vec![1]);
    }
}
