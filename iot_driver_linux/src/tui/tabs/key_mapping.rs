// Key Mapping Tab — unified per-key view across the keymatrix + magnetism tables.
//
// A single scrollable table of every physical key showing its mode, output(s)
// across layers, actuation, and mode-specific values. Replaces the separate
// Remaps and Triggers tabs (a key's outputs and its mode/actuation are stored in
// overlapping tables — DKS output combos even live in the keymatrix layers).

use ratatui::{prelude::*, widgets::*};

use crate::keymap::KeyRow;
use crate::tui::RemapLayerView;
use monsgeek_keyboard::{KeyMode, ModeByte};
use monsgeek_transport::protocol::matrix;

use super::super::shared::LoadState;
use super::super::App;

/// List vs. keyboard-layout presentation for the Key Mapping tab.
#[derive(Clone, Copy, PartialEq, Default)]
pub(in crate::tui) enum KeyMappingView {
    #[default]
    List,
    Layout,
}

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

/// Physical key class narrowing. Alphanumeric = a single-character label that is an
/// ASCII letter or digit (A–Z, 0–9); Other = everything else (Esc, Tab, modifiers,
/// symbols, navigation, …).
#[derive(Clone, Copy, PartialEq, Default)]
pub(in crate::tui) enum KmClass {
    #[default]
    All,
    Alphanumeric,
    Other,
}

impl KmClass {
    pub fn cycle(self) -> Self {
        match self {
            Self::All => Self::Alphanumeric,
            Self::Alphanumeric => Self::Other,
            Self::Other => Self::All,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Alphanumeric => "Alnum",
            Self::Other => "Other",
        }
    }
    /// A key is alphanumeric iff its physical label is exactly one ASCII letter/digit.
    fn is_alnum(r: &KeyRow) -> bool {
        let mut chars = r.position.chars();
        matches!((chars.next(), chars.next()), (Some(c), None) if c.is_ascii_alphanumeric())
    }
    pub fn matches(self, r: &KeyRow) -> bool {
        match self {
            Self::All => true,
            Self::Alphanumeric => Self::is_alnum(r),
            Self::Other => !Self::is_alnum(r),
        }
    }
}

/// Row ordering for the Key Mapping table.
#[derive(Clone, Copy, PartialEq, Default)]
pub(in crate::tui) enum KmSort {
    /// Physical reading order: top row L→R, then the next row down, … (Esc,F1,F2…;
    /// `,1,2…; Tab,Q,W…). The stored index is column-major, so we re-key on
    /// (index % 6, index / 6) = (physical row, physical column).
    #[default]
    Layout,
    /// Alphabetical by physical key label (case-insensitive).
    Alpha,
}

impl KmSort {
    pub fn cycle(self) -> Self {
        match self {
            Self::Layout => Self::Alpha,
            Self::Alpha => Self::Layout,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Layout => "Layout",
            Self::Alpha => "A–Z",
        }
    }
}

/// Which keys the table shows, by four independent narrowings.
#[derive(Clone, Copy)]
pub(in crate::tui) struct KeyMappingFilter {
    /// Layer: Both = all; L0/L1 = keys with a non-default binding on that keymatrix
    /// layer; Fn = keys with an Fn binding.
    pub layer: RemapLayerView,
    pub state: KmState,
    /// Mode narrowing (None = any).
    pub mode: Option<KeyMode>,
    /// Physical key class (alphanumeric / other).
    pub class: KmClass,
}

impl Default for KeyMappingFilter {
    fn default() -> Self {
        Self {
            layer: RemapLayerView::Both,
            state: KmState::All,
            mode: None,
            class: KmClass::All,
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
        state_ok && layer_ok && self.class.matches(r) && self.mode.is_none_or(|m| r.mode == m)
    }

    /// True when any narrowing is active.
    pub fn is_active(&self) -> bool {
        self.layer != RemapLayerView::Both
            || self.state != KmState::All
            || self.mode.is_some()
            || self.class != KmClass::All
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

/// Indices into `app.key_rows` that pass the current filter, ordered by the active sort.
pub(in crate::tui) fn visible_indices(app: &App) -> Vec<usize> {
    let mut v: Vec<usize> = app
        .key_rows
        .iter()
        .enumerate()
        .filter(|(_, r)| app.key_mapping_filter.matches(r))
        .map(|(i, _)| i)
        .collect();
    match app.key_mapping_sort {
        KmSort::Layout => v.sort_by_key(|&i| {
            let idx = app.key_rows[i].index as u16;
            (idx % 6, idx / 6)
        }),
        KmSort::Alpha => {
            v.sort_by(|&a, &b| {
                app.key_rows[a]
                    .position
                    .to_ascii_lowercase()
                    .cmp(&app.key_rows[b].position.to_ascii_lowercase())
            });
        }
    }
    v
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

/// Mode-specific "Extra" cell (DKS travel + active combos / ModTap time / SnapTap).
fn extra_text(row: &KeyRow, factor: f32) -> String {
    match row.mode {
        KeyMode::DynamicKeystroke => {
            let travel = format!("↧{:.2}mm", row.dks_travel as f32 / factor);
            // A binding fires only if it has an output and at least one phase action.
            let active = (0..4)
                .filter(|&i| {
                    !matches!(row.outputs[i], crate::key_action::KeyAction::Disabled)
                        && row.dks_modes[i] != 0
                })
                .count();
            if active == 0 {
                format!("{travel} ∅")
            } else {
                format!("{travel} ×{active}")
            }
        }
        KeyMode::ModTap => format!("{}ms", row.modtap_ms),
        KeyMode::SnapTap => row
            .snaptap_partner
            .map(|p| format!("↔{}", matrix::key_name(p)))
            .unwrap_or_else(|| "unbound".into()),
        _ => "·".into(),
    }
}

pub(in crate::tui) fn render_key_mapping(f: &mut Frame, app: &mut App, area: Rect) {
    match app.key_mapping_view {
        KeyMappingView::Layout => render_key_mapping_layout(f, app, area),
        KeyMappingView::List => render_key_mapping_list(f, app, area),
    }
}

fn render_key_mapping_list(f: &mut Frame, app: &mut App, area: Rect) {
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
                "[layer:{} state:{} mode:{} class:{}]",
                filter.layer.label(),
                filter.state.label(),
                filter.mode_label(),
                filter.class.label(),
            ),
            filter_style,
        ),
        Span::raw("  sort "),
        Span::styled(
            app.key_mapping_sort.label(),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            "  (f: filter  s: sort  v: layout  Enter: edit  g: global)",
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
    let selected = app.key_mapping_selected;

    let rows: Vec<Row> = visible
        .iter()
        .map(|&ri| {
            let r = &app.key_rows[ri];
            let mode_str = ModeByte::new(r.mode, r.rapid_trigger).to_string();
            let cells = vec![
                Cell::from(format!("{:3}", r.index)),
                Cell::from(r.position),
                Cell::from(mode_str).style(Style::default().fg(mode_color(r.mode))),
                Cell::from(output_text(r, RemapLayerView::L0)),
                Cell::from(output_text(r, RemapLayerView::L1)),
                Cell::from(output_text(r, RemapLayerView::Fn)),
                Cell::from(format!("{:.2}", r.actuation as f32 / factor)),
                Cell::from(format!("{:.2}", r.release as f32 / factor)),
                Cell::from(extra_text(r, factor)),
            ];
            let row = Row::new(cells);
            if !r.is_customized() {
                row.style(Style::default().fg(Color::DarkGray))
            } else {
                row
            }
        })
        .collect();

    let header_row = Row::new(vec![
        "#", "Key", "Mode", "Base", "L1", "Fn", "Act", "Rel", "Extra",
    ])
    .style(
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Cyan),
    );

    let widths = [
        Constraint::Length(4),
        Constraint::Length(8),
        Constraint::Length(13),
        Constraint::Min(14),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(5),
        Constraint::Length(5),
        Constraint::Length(13),
    ];
    // A stateful Table keeps the header row sticky and scrolls the body to keep the
    // selection visible.
    let table = Table::new(rows, widths)
        .header(header_row)
        .block(Block::default().borders(Borders::ALL))
        .row_highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = TableState::default();
    // `.then` (lazy) — an empty filter result must not evaluate `visible.len() - 1`,
    // which would underflow and panic.
    state.select((!visible.is_empty()).then(|| selected.min(visible.len() - 1)));
    f.render_stateful_widget(table, chunks[1], &mut state);
}

/// Keyboard-shaped view: every key drawn at its matrix position, colored by mode.
/// Filtered-out keys are dimmed (the whole board stays visible); the selected key
/// (a filter match) is highlighted.
fn render_key_mapping_layout(f: &mut Frame, app: &mut App, area: Rect) {
    let visible = visible_indices(app);
    let filter = app.key_mapping_filter;
    let sel_pos = visible
        .get(app.key_mapping_selected)
        .map(|&ri| app.key_rows[ri].index);
    let visible_set: std::collections::HashSet<u8> =
        visible.iter().map(|&ri| app.key_rows[ri].index).collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(3)])
        .split(area);

    let block = Block::default().borders(Borders::ALL).title(format!(
        "Key Mapping — layout  [{}/{} keys]  (v: list  ←↑↓→: move  Enter: edit  f: filter  s: sort  g: global)",
        visible.len(),
        app.key_rows.len(),
    ));
    let inner = block.inner(chunks[0]);
    f.render_widget(block, chunks[0]);

    let (key_w, key_h) = (5u16, 2u16);
    for r in &app.key_rows {
        if r.position.is_empty() || r.position == "?" {
            continue;
        }
        let col = r.index as u16 / 6;
        let row = r.index as u16 % 6;
        let x = inner.x + col * key_w;
        let y = inner.y + row * key_h;
        if x + key_w > inner.x + inner.width || y + key_h > inner.y + inner.height {
            continue;
        }
        let selected = Some(r.index) == sel_pos;
        let text_style = if selected {
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else if !visible_set.contains(&r.index) {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(mode_color(r.mode))
        };
        let border_style = if selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let name: String = r.position.chars().take(4).collect();
        let cell = Paragraph::new(name)
            .style(text_style)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style),
            );
        f.render_widget(cell, Rect::new(x, y, key_w, key_h));
    }

    // Detail line for the selected key.
    let factor = app.precision.factor() as f32;
    let detail = if let Some(&ri) = visible.get(app.key_mapping_selected) {
        let r = &app.key_rows[ri];
        Line::from(vec![
            Span::styled(
                format!("{} ", r.position),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                ModeByte::new(r.mode, r.rapid_trigger).to_string(),
                Style::default().fg(mode_color(r.mode)),
            ),
            Span::raw(format!(
                "  → {}   act {:.2} / rel {:.2}mm   {}",
                output_text(r, filter.layer),
                r.actuation as f32 / factor,
                r.release as f32 / factor,
                extra_text(r, factor),
            )),
        ])
    } else {
        Line::from(Span::styled(
            "(no matching key)",
            Style::default().fg(Color::DarkGray),
        ))
    };
    f.render_widget(
        Paragraph::new(detail).block(Block::default().borders(Borders::ALL).title("Selected")),
        chunks[1],
    );
}

/// Move the layout selection one grid step; `dcol`/`drow` in {-1,0,1}. Snaps to
/// the nearest visible key in that direction (same column for up/down, same row
/// for left/right).
pub(in crate::tui) fn layout_move(app: &mut App, dcol: i32, drow: i32) {
    let visible = visible_indices(app);
    let Some(&cur_ri) = visible.get(app.key_mapping_selected) else {
        return;
    };
    let cur = app.key_rows[cur_ri].index as i32;
    let (col, row) = (cur / 6, cur % 6);
    // Search outward in the requested direction for the next visible key.
    for step in 1..=21i32 {
        let (c, r) = (col + dcol * step, row + drow * step);
        if !(0..21).contains(&c) || !(0..6).contains(&r) {
            break;
        }
        let target = (c * 6 + r) as u8;
        if let Some(vi) = visible
            .iter()
            .position(|&ri| app.key_rows[ri].index == target)
        {
            app.key_mapping_selected = vi;
            return;
        }
    }
}

/// The `f` filter popup: three cyclable fields (layer / state / mode).
pub(in crate::tui) fn render_key_mapping_filter(f: &mut Frame, app: &App, area: Rect) {
    let filter = app.key_mapping_filter;
    let field = app.key_mapping_filter_field;
    let rows = [
        ("Layer", filter.layer.label()),
        ("State", filter.state.label()),
        ("Mode", filter.mode_label()),
        ("Class", filter.class.label()),
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
        2 => {
            app.key_mapping_filter.mode = cycle_mode_filter(app.key_mapping_filter.mode);
            let _ = forward;
        }
        _ => {
            app.key_mapping_filter.class = if forward {
                app.key_mapping_filter.class.cycle()
            } else {
                app.key_mapping_filter.class.cycle().cycle()
            };
        }
    }
    // Keep the selection in range after the visible set changes.
    let n = visible_indices(app).len();
    if app.key_mapping_selected >= n {
        app.key_mapping_selected = n.saturating_sub(1);
    }
}
