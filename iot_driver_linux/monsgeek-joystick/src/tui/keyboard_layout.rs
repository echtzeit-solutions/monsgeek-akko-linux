//! Keyboard layout visualization widget
//!
//! Displays a visual keyboard layout with key depths and mapping highlights.
//! Key names are sourced from `monsgeek_transport::protocol::matrix` — the
//! single source of truth for matrix position → key name mapping.

use crate::tui::app::App;
use monsgeek_transport::protocol::matrix;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Number of columns in the visual keyboard layout (cols 0-14, no media keys)
const LAYOUT_COLS: usize = 15;
/// Number of rows in the keyboard layout
const LAYOUT_ROWS: usize = 6;

/// Get display-ready key name for a matrix position.
///
/// Delegates to `matrix::key_name()` and maps `"?"` to `""` for unused slots.
pub fn get_key_name(index: u8) -> &'static str {
    let name = matrix::key_name(index);
    if name == "?" {
        ""
    } else {
        name
    }
}

/// Render the keyboard layout
pub fn render_keyboard_layout(frame: &mut Frame, app: &App, area: Rect) {
    // Calculate key cell dimensions
    let key_width = 4u16;
    let key_height = 2u16;

    // Get mapped key indices for highlighting
    let mapped_keys = app.config.mapped_key_indices();

    // Render each key (columns 0-14 only, skip media columns 90-95)
    for pos in 0..(LAYOUT_COLS * LAYOUT_ROWS) {
        let col = pos / LAYOUT_ROWS;
        let row = pos % LAYOUT_ROWS;

        let key_name = get_key_name(pos as u8);
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

        // Truncate key name to fit cell width
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

/// Find key index by name (case-insensitive).
///
/// Delegates to the matrix module's canonical lookup.
pub fn find_key_by_name(name: &str) -> Option<u8> {
    matrix::key_index_from_name(name)
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
    fn test_unused_slots_empty() {
        // "?" positions in the matrix should map to empty string for display
        assert_eq!(get_key_name(23), ""); // Col 3 row 5 — unused
        assert_eq!(get_key_name(29), ""); // Col 4 row 5 — unused
        assert_eq!(matrix::key_name(23), "?");
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
