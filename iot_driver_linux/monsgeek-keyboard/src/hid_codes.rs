//! HID keycode utilities

/// Convert a character to HID keycode
/// Returns (keycode, needs_shift) or None if unsupported
pub fn char_to_hid(ch: char) -> Option<(u8, bool)> {
    match ch {
        // Letters (a-z lowercase, A-Z needs shift)
        'a'..='z' => Some((0x04 + (ch as u8 - b'a'), false)),
        'A'..='Z' => Some((0x04 + (ch as u8 - b'A'), true)),
        // Numbers
        '1'..='9' => Some((0x1E + (ch as u8 - b'1'), false)),
        '0' => Some((0x27, false)),
        // Special characters (unshifted)
        ' ' => Some((0x2C, false)), // Space
        '-' => Some((0x2D, false)),
        '=' => Some((0x2E, false)),
        '[' => Some((0x2F, false)),
        ']' => Some((0x30, false)),
        '\\' => Some((0x31, false)),
        ';' => Some((0x33, false)),
        '\'' => Some((0x34, false)),
        '`' => Some((0x35, false)),
        ',' => Some((0x36, false)),
        '.' => Some((0x37, false)),
        '/' => Some((0x38, false)),
        '\n' => Some((0x28, false)), // Enter
        '\t' => Some((0x2B, false)), // Tab
        // Shifted characters
        '!' => Some((0x1E, true)), // Shift+1
        '@' => Some((0x1F, true)), // Shift+2
        '#' => Some((0x20, true)), // Shift+3
        '$' => Some((0x21, true)), // Shift+4
        '%' => Some((0x22, true)), // Shift+5
        '^' => Some((0x23, true)), // Shift+6
        '&' => Some((0x24, true)), // Shift+7
        '*' => Some((0x25, true)), // Shift+8
        '(' => Some((0x26, true)), // Shift+9
        ')' => Some((0x27, true)), // Shift+0
        '_' => Some((0x2D, true)), // Shift+-
        '+' => Some((0x2E, true)), // Shift+=
        '{' => Some((0x2F, true)), // Shift+[
        '}' => Some((0x30, true)), // Shift+]
        '|' => Some((0x31, true)), // Shift+\
        ':' => Some((0x33, true)), // Shift+;
        '"' => Some((0x34, true)), // Shift+'
        '~' => Some((0x35, true)), // Shift+`
        '<' => Some((0x36, true)), // Shift+,
        '>' => Some((0x37, true)), // Shift+.
        '?' => Some((0x38, true)), // Shift+/
        _ => None,
    }
}
