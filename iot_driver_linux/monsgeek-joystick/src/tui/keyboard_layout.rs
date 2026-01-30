//! Keyboard layout visualization widget
//!
//! Displays a visual keyboard layout with key depths and mapping highlights.

use crate::tui::app::App;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Key names for M1 V5 HE matrix positions (column-major, 21x6).
/// Each column has 6 rows (positions 0-5, 6-11, etc.)
///
/// These are abbreviated for TUI display width (e.g. "LSh" instead of "LShift").
/// The authoritative full key names are in `iot_driver::profile::builtin::M1_V5_HE_KEY_NAMES`.
const KEY_NAMES: &[&str] = &[
    // Col 0 (0-5): Esc column
    "Esc", "`", "Tab", "Caps", "LSh", "LCt", // Col 1 (6-11): F1/1/Q/A column
    "F1", "1", "Q", "A", "", "LWin", // Col 2 (12-17): F2/2/W/S/Z column
    "F2", "2", "W", "S", "Z", "LAlt", // Col 3 (18-23): F3/3/E/D/X column
    "F3", "3", "E", "D", "X", "", // Col 4 (24-29): F4/4/R/F/C column
    "F4", "4", "R", "F", "C", "", // Col 5 (30-35): F5/5/T/G/V column
    "F5", "5", "T", "G", "V", "", // Col 6 (36-41): F6/6/Y/H/B/Space column
    "F6", "6", "Y", "H", "B", "Spc", // Col 7 (42-47): F7/7/U/J/N column
    "F7", "7", "U", "J", "N", "", // Col 8 (48-53): F8/8/I/K/M column
    "F8", "8", "I", "K", "M", "", // Col 9 (54-59): F9/9/O/L/,/RAlt column
    "F9", "9", "O", "L", ",", "RAlt", // Col 10 (60-65): F10/0/P/;/./Fn column
    "F10", "0", "P", ";", ".", "Fn", // Col 11 (66-71): F11/-/[/'/RCtrl column
    "F11", "-", "[", "'", "/", "RCt", // Col 12 (72-77): F12/=/]/RShift/Left column
    "F12", "=", "]", "", "RSh", "<-", // Col 13 (78-83): Del/Bksp/\/Enter/Up/Down column
    "Del", "Bks", "\\", "Ent", "^", "v", // Col 14 (84-89): Nav cluster
    "", "Hom", "PgU", "PgD", "End", "->", // Col 15 (90-95): Media keys
    "V+", "V-", "Mt", "", "", "", // Remaining (96-125): extra positions
    "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "",
    "", "", "", "", "", "",
];

/// Number of columns in the keyboard layout
const LAYOUT_COLS: usize = 16;
/// Number of rows in the keyboard layout
const LAYOUT_ROWS: usize = 6;

/// Get key name for a matrix position
pub fn get_key_name(index: u8) -> &'static str {
    KEY_NAMES.get(index as usize).copied().unwrap_or("")
}

/// Render the keyboard layout
pub fn render_keyboard_layout(frame: &mut Frame, app: &App, area: Rect) {
    // Calculate key cell dimensions
    let key_width = 4u16;
    let key_height = 2u16;

    // Get mapped key indices for highlighting
    let mapped_keys = app.config.mapped_key_indices();

    // Render each key
    for pos in 0..(LAYOUT_COLS * LAYOUT_ROWS) {
        let col = pos / LAYOUT_ROWS;
        let row = pos % LAYOUT_ROWS;

        let key_name = KEY_NAMES.get(pos).copied().unwrap_or("");
        if key_name.is_empty() {
            continue;
        }

        // Calculate screen position
        let x = area.x + (col as u16 * key_width);
        let y = area.y + (row as u16 * key_height);

        // Skip if outside area
        if x + key_width > area.x + area.width || y + key_height > area.y + area.height {
            continue;
        }

        let key_rect = Rect::new(x, y, key_width, key_height);
        let pos_u8 = pos as u8;

        // Determine key style
        let is_mapped = mapped_keys.contains(&pos_u8);
        let is_pressed = app.pressed_key == Some(pos_u8);
        let depth = app.mapper.get_key_depth(pos_u8);

        // Color based on state
        let (fg, bg, border_color) = if is_pressed && depth > 0.1 {
            // Currently pressed - show depth intensity
            let intensity = (depth / 4.0).clamp(0.0, 1.0);
            let green = (50.0 + intensity * 205.0) as u8;
            (Color::Black, Color::Rgb(0, green, 0), Color::Green)
        } else if is_mapped {
            // Mapped key
            (Color::Yellow, Color::Reset, Color::Yellow)
        } else {
            // Normal key
            (Color::White, Color::Reset, Color::DarkGray)
        };

        let style = Style::default().fg(fg).bg(bg);
        let border_style = Style::default().fg(border_color);

        // Truncate key name to fit
        let display_name: String = key_name.chars().take(3).collect();

        let key_block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style);

        let key_text = Paragraph::new(display_name).style(style).block(key_block);

        frame.render_widget(key_text, key_rect);
    }

    // Show depth values for mapped keys below the layout
    let depth_y = area.y + (LAYOUT_ROWS as u16 * key_height) + 1;
    if depth_y < area.y + area.height {
        let mut depth_spans = Vec::new();
        for &key_idx in &mapped_keys {
            let name = get_key_name(key_idx);
            let depth = app.mapper.get_key_depth(key_idx);
            if !name.is_empty() {
                if !depth_spans.is_empty() {
                    depth_spans.push(Span::raw("  "));
                }
                depth_spans.push(Span::styled(
                    format!("{}:{:.1}", name, depth),
                    Style::default().fg(if depth > 0.1 {
                        Color::Green
                    } else {
                        Color::DarkGray
                    }),
                ));
            }
        }
        if !depth_spans.is_empty() {
            let depth_line = ratatui::text::Line::from(depth_spans);
            let depth_area = Rect::new(area.x, depth_y, area.width, 1);
            frame.render_widget(Paragraph::new(depth_line), depth_area);
        }
    }
}

/// Find key index by name (case-insensitive)
pub fn find_key_by_name(name: &str) -> Option<u8> {
    let name_lower = name.to_lowercase();
    for (i, &key_name) in KEY_NAMES.iter().enumerate() {
        if key_name.to_lowercase() == name_lower {
            return Some(i as u8);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_names() {
        assert_eq!(get_key_name(0), "Esc");
        assert_eq!(get_key_name(9), "A");
        assert_eq!(get_key_name(14), "W");
        assert_eq!(get_key_name(15), "S");
        assert_eq!(get_key_name(21), "D");
    }

    #[test]
    fn test_find_key() {
        assert_eq!(find_key_by_name("W"), Some(14));
        assert_eq!(find_key_by_name("w"), Some(14));
        assert_eq!(find_key_by_name("A"), Some(9));
        assert_eq!(find_key_by_name("S"), Some(15));
        assert_eq!(find_key_by_name("D"), Some(21));
    }
}
