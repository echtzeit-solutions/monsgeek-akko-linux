// Key Mapping Tab — unified per-key view across the keymatrix + magnetism tables.
//
// A single scrollable table of every physical key showing its mode, output(s)
// across layers, actuation, and mode-specific values. Replaces the separate
// Remaps and Triggers tabs (a key's outputs and its mode/actuation are stored in
// overlapping tables — DKS output combos even live in the keymatrix layers).

use ratatui::{prelude::*, widgets::*};
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::keymap::KeyRow;
use crate::tui::RemapLayerView;
use monsgeek_keyboard::{KeyMode, ModeByte};
use monsgeek_transport::protocol::matrix;

use super::super::shared::LoadState;
use super::super::App;

fn mode_color(mode: KeyMode) -> Color {
    match mode {
        KeyMode::Normal => Color::White,
        KeyMode::DynamicKeystroke => Color::Magenta,
        KeyMode::ModTap => Color::Green,
        KeyMode::ToggleHold | KeyMode::ToggleDots => Color::Blue,
        KeyMode::SnapTap => Color::Cyan,
        KeyMode::Unknown(_) => Color::Red,
    }
}

/// The "Output" cell for the focused layer view. In DKS mode the base view shows
/// the four combo slots; otherwise it shows the selected layer's binding.
fn output_text(row: &KeyRow, view: RemapLayerView) -> String {
    let non_empty =
        |a: &crate::key_action::KeyAction| !matches!(a, crate::key_action::KeyAction::Disabled);
    match view {
        RemapLayerView::L1 => {
            if non_empty(&row.outputs[1]) {
                row.outputs[1].to_string()
            } else {
                "·".into()
            }
        }
        RemapLayerView::Fn => row
            .fn_action
            .as_ref()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "·".into()),
        // Both / L0 → base output, or the DKS combo set.
        _ => {
            if row.mode == KeyMode::DynamicKeystroke {
                let combos: Vec<String> = row
                    .outputs
                    .iter()
                    .filter(|a| non_empty(a))
                    .map(|a| a.to_string())
                    .collect();
                if combos.is_empty() {
                    "(dks)".into()
                } else {
                    combos.join(" / ")
                }
            } else {
                row.outputs[0].to_string()
            }
        }
    }
}

/// Mode-specific "Extra" cell (DKS travel / ModTap time / SnapTap partner).
fn extra_text(row: &KeyRow, factor: f32) -> String {
    match row.mode {
        KeyMode::DynamicKeystroke => format!("↧{:.2}mm", row.dks_travel as f32 / factor),
        KeyMode::ModTap => format!("{}ms", row.modtap_ms),
        KeyMode::SnapTap => row
            .snaptap_partner
            .map(|p| format!("↔{}", matrix::key_name(p)))
            .unwrap_or_else(|| "unbound".into()),
        _ => "·".into(),
    }
}

/// Compact layer-occupancy markers, e.g. "0 1 · · F".
fn layer_markers(row: &KeyRow) -> String {
    let mut s = String::new();
    for (l, &set) in row.output_remapped.iter().enumerate() {
        s.push(if set {
            char::from(b'0' + l as u8)
        } else {
            '·'
        });
        s.push(' ');
    }
    s.push(if row.fn_action.is_some() { 'F' } else { '·' });
    s
}

pub(in crate::tui) fn render_key_mapping(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(6)])
        .split(area);

    // Header / legend.
    let customized = app.key_rows.iter().filter(|r| r.is_customized()).count();
    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            format!("{} keys", app.key_rows.len()),
            Style::default().fg(Color::Green),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{customized} customized"),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("   |   Layer view: "),
        Span::styled(
            app.key_mapping_layer_view.label(),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            "  (f: layer  ↑↓: select  Enter: edit  g: global  c: calibrate)",
            Style::default().fg(Color::DarkGray),
        ),
    ])])
    .block(Block::default().borders(Borders::ALL).title("Key Mapping"));
    f.render_widget(header, chunks[0]);

    if app.key_rows.is_empty() {
        let msg = match app.loading.key_mapping {
            LoadState::Loading => "Loading…",
            LoadState::Error => "Failed to load — press r to retry",
            _ => "No key data — press r to load",
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL)),
            chunks[1],
        );
        return;
    }

    let factor = app.precision.factor() as f32;
    let view = app.key_mapping_layer_view;
    let selected = app.key_mapping_selected;

    let rows: Vec<Row> = app
        .key_rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mode_str = ModeByte::new(r.mode, r.rapid_trigger).to_string();
            let cells = vec![
                Cell::from(format!("{:3}", r.index)),
                Cell::from(r.position),
                Cell::from(mode_str).style(Style::default().fg(mode_color(r.mode))),
                Cell::from(output_text(r, view)),
                Cell::from(format!("{:.2}", r.actuation as f32 / factor)),
                Cell::from(format!("{:.2}", r.release as f32 / factor)),
                Cell::from(extra_text(r, factor)),
                Cell::from(layer_markers(r)),
            ];
            let row = Row::new(cells);
            if i == selected {
                row.style(Style::default().bg(Color::Blue).fg(Color::White))
            } else if !r.is_customized() {
                row.style(Style::default().fg(Color::DarkGray))
            } else {
                row
            }
        })
        .collect();

    let header_row = Row::new(vec![
        "#", "Key", "Mode", "Output", "Act", "Rel", "Extra", "Layers",
    ])
    .style(
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Cyan),
    );

    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(chunks[1]);
    f.render_widget(block, chunks[1]);

    let widths = [
        Constraint::Length(4),
        Constraint::Length(8),
        Constraint::Length(13),
        Constraint::Min(16),
        Constraint::Length(5),
        Constraint::Length(5),
        Constraint::Length(11),
        Constraint::Length(11),
    ];
    let table = Table::new(rows, widths).header(header_row);

    let content_height = (app.key_rows.len() + 1) as u16;
    let content_size = Size::new(inner.width, content_height);
    let mut scroll_view =
        ScrollView::new(content_size).horizontal_scrollbar_visibility(ScrollbarVisibility::Never);
    scroll_view.render_widget(table, Rect::new(0, 0, inner.width, content_height));
    f.render_stateful_widget(scroll_view, inner, &mut app.scroll_state);
}
