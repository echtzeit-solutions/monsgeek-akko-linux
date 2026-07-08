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

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// Customization narrowing for the Key Mapping table.
#[derive(Clone, Copy, PartialEq)]
pub(in crate::tui) enum KmState {
    All,
    Customized,
    Default,
}

impl KmState {
    pub fn cycle(self) -> Self {
        match self {
            Self::All => Self::Customized,
            Self::Customized => Self::Default,
            Self::Default => Self::All,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Customized => "Customized",
            Self::Default => "Default",
        }
    }
}

/// Which keys the table shows, by three independent narrowings.
#[derive(Clone, Copy)]
pub(in crate::tui) struct KeyMappingFilter {
    /// Layer: Both = all; L0/L1 = keys with a non-default binding on that keymatrix
    /// layer; Fn = keys with an Fn binding.
    pub layer: RemapLayerView,
    pub state: KmState,
    /// Mode narrowing (None = any).
    pub mode: Option<KeyMode>,
}

impl Default for KeyMappingFilter {
    fn default() -> Self {
        Self {
            layer: RemapLayerView::Both,
            state: KmState::All,
            mode: None,
        }
    }
}

impl KeyMappingFilter {
    pub fn matches(&self, r: &KeyRow) -> bool {
        let state_ok = match self.state {
            KmState::All => true,
            KmState::Customized => r.is_customized(),
            KmState::Default => !r.is_customized(),
        };
        let layer_ok = match self.layer {
            RemapLayerView::Both => true,
            RemapLayerView::L0 => r.output_remapped[0],
            RemapLayerView::L1 => r.output_remapped[1],
            RemapLayerView::Fn => r.fn_action.is_some(),
        };
        state_ok && layer_ok && self.mode.is_none_or(|m| r.mode == m)
    }

    /// True when any narrowing is active.
    pub fn is_active(&self) -> bool {
        self.layer != RemapLayerView::Both || self.state != KmState::All || self.mode.is_some()
    }

    pub fn mode_label(&self) -> &'static str {
        self.mode.map_or("Any", |m| m.label())
    }
}

/// Cycle the mode filter: None → Normal → … → SnapTap → None.
pub(in crate::tui) fn cycle_mode_filter(cur: Option<KeyMode>) -> Option<KeyMode> {
    match cur {
        None => KeyMode::ALL.first().copied(),
        Some(m) => {
            let i = KeyMode::ALL.iter().position(|&x| x == m).unwrap_or(0);
            KeyMode::ALL.get(i + 1).copied()
        }
    }
}

/// Indices into `app.key_rows` that pass the current filter.
pub(in crate::tui) fn visible_indices(app: &App) -> Vec<usize> {
    app.key_rows
        .iter()
        .enumerate()
        .filter(|(_, r)| app.key_mapping_filter.matches(r))
        .map(|(i, _)| i)
        .collect()
}

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

    let filter = app.key_mapping_filter;
    let visible = visible_indices(app);

    // Header / legend.
    let filter_style = if filter.is_active() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            format!("{}/{} keys", visible.len(), app.key_rows.len()),
            Style::default().fg(Color::Green),
        ),
        Span::raw("   filter "),
        Span::styled(
            format!(
                "[layer:{} state:{} mode:{}]",
                filter.layer.label(),
                filter.state.label(),
                filter.mode_label()
            ),
            filter_style,
        ),
        Span::styled(
            "  (f: filter  ↑↓: select  Enter: edit  g: global)",
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
    let view = filter.layer;
    let selected = app.key_mapping_selected;

    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(vi, &ri)| {
            let r = &app.key_rows[ri];
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
            if vi == selected {
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

    let content_height = (visible.len() + 1) as u16;
    let content_size = Size::new(inner.width, content_height);
    let mut scroll_view =
        ScrollView::new(content_size).horizontal_scrollbar_visibility(ScrollbarVisibility::Never);
    scroll_view.render_widget(table, Rect::new(0, 0, inner.width, content_height));
    f.render_stateful_widget(scroll_view, inner, &mut app.scroll_state);
}

/// The `f` filter popup: three cyclable fields (layer / state / mode).
pub(in crate::tui) fn render_key_mapping_filter(f: &mut Frame, app: &App, area: Rect) {
    let filter = app.key_mapping_filter;
    let field = app.key_mapping_filter_field;
    let rows = [
        ("Layer", filter.layer.label()),
        ("State", filter.state.label()),
        ("Mode", filter.mode_label()),
    ];

    let w = 44u16.min(area.width);
    let h = (rows.len() as u16 + 4).min(area.height);
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(w)) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w,
        h,
    );
    f.render_widget(Clear, popup);

    let mut lines = vec![Line::from("")];
    for (i, (label, val)) in rows.iter().enumerate() {
        let val_style = if i == field {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::Cyan)
        };
        lines.push(Line::from(vec![
            Span::raw(format!("  {label:<7}")),
            Span::styled(format!(" ‹ {val} › "), val_style),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "  ↑↓ field   ←→ change   Esc/Enter close",
        Style::default().fg(Color::DarkGray),
    )));

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Filter")
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        popup,
    );
}

/// Cycle the filter's currently-selected field by `dir` (+1 / -1).
pub(in crate::tui) fn cycle_filter_field(app: &mut App, forward: bool) {
    match app.key_mapping_filter_field {
        0 => {
            // Layer cycles through the RemapLayerView order both directions.
            app.key_mapping_filter.layer = if forward {
                app.key_mapping_filter.layer.cycle()
            } else {
                app.key_mapping_filter.layer.cycle().cycle().cycle()
            };
        }
        1 => {
            app.key_mapping_filter.state = if forward {
                app.key_mapping_filter.state.cycle()
            } else {
                app.key_mapping_filter.state.cycle().cycle()
            };
        }
        _ => {
            app.key_mapping_filter.mode = cycle_mode_filter(app.key_mapping_filter.mode);
            let _ = forward;
        }
    }
    // Keep the selection in range after the visible set changes.
    let n = visible_indices(app).len();
    if app.key_mapping_selected >= n {
        app.key_mapping_selected = n.saturating_sub(1);
    }
}
