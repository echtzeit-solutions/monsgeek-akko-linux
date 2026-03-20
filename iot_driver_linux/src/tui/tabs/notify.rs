// Notify Tab — effect editor, daemon control, hardware preview
//
// All notify-specific types, rendering, and input handling.

use crossterm::event::KeyCode;
use ratatui::{prelude::*, widgets::*};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::effect::{
    required_variables, resolve as resolve_effect, EffectDef, EffectLibrary, KeyframeDef, NumOrVar,
    ResolvedEffect,
};
use crate::led_stream::{apply_power_budget, send_overlay_diff};

use super::super::shared::AsyncResult;
use super::super::App;

// ============================================================================
// Types
// ============================================================================

/// Notify tab state — effect editor, daemon control, preview.
pub(crate) struct NotifyTabState {
    // Daemon
    pub daemon_running: bool,
    pub daemon_cancel: Option<Arc<AtomicBool>>,
    pub daemon_handle: Option<tokio::task::JoinHandle<()>>,
    pub daemon_error: Option<String>,

    // Effect library (loaded from effects.toml)
    pub effects: Option<EffectLibrary>,
    pub effect_names: Vec<String>,
    pub selected_effect: usize,

    // Editor
    pub focus: NotifyFocus,
    pub selected_keyframe: usize,
    pub selected_field: usize, // 0=timing, 1=value, 2=easing
    pub editing: bool,
    pub edit_input: String,
    pub selected_var: usize,
    pub var_values: std::collections::BTreeMap<String, String>,

    // Preview
    pub resolved: Option<ResolvedEffect>,
    pub preview_start: Instant,
    pub preview_on_hardware: bool,
    pub prev_hw_frame: [(u8, u8, u8); 96],

    // D-Bus notification list
    pub notifications: Vec<(u64, String, String, String, i32)>,
    pub last_notif_poll: Instant,

    // Labels for preview grid
    pub labels: Vec<String>,

    pub dirty: bool,
}

impl Default for NotifyTabState {
    fn default() -> Self {
        Self {
            daemon_running: false,
            daemon_cancel: None,
            daemon_handle: None,
            daemon_error: None,
            effects: None,
            effect_names: Vec::new(),
            selected_effect: 0,
            focus: NotifyFocus::EffectList,
            selected_keyframe: 0,
            selected_field: 0,
            editing: false,
            edit_input: String::new(),
            selected_var: 0,
            var_values: std::collections::BTreeMap::new(),
            resolved: None,
            preview_start: Instant::now(),
            preview_on_hardware: false,
            prev_hw_frame: [(0, 0, 0); 96],
            notifications: Vec::new(),
            last_notif_poll: Instant::now(),
            labels: crate::effect::preview::build_labels(),
            dirty: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum NotifyFocus {
    #[default]
    EffectList,
    KeyframeList,
    FieldEdit,
    VarEdit,
}

/// Known easing function names for the cycling editor.
const EASING_NAMES: &[&str] = &[
    "Linear",
    "Hold",
    "EaseIn",
    "EaseOut",
    "EaseInOut",
    "EaseInCubic",
    "EaseOutCubic",
    "EaseInOutCubic",
    "EaseInQuart",
    "EaseOutQuart",
    "EaseInOutQuart",
    "EaseInQuint",
    "EaseOutQuint",
    "EaseInOutQuint",
];

/// Extract the default value for a variable from an effect definition.
fn extract_var_default(def: &EffectDef, var_name: &str) -> Option<String> {
    // Check effect-level color
    if let Some(ref c) = def.color {
        if let Some(var_ref) = c.strip_prefix('$') {
            if let Some((name, default)) = var_ref.split_once(':') {
                if name == var_name {
                    return Some(default.to_string());
                }
            }
        }
    }
    // Check keyframe timing and colors
    for kf in &def.keyframes {
        if let Some(ref c) = kf.color {
            if let Some(var_ref) = c.strip_prefix('$') {
                if let Some((name, default)) = var_ref.split_once(':') {
                    if name == var_name {
                        return Some(default.to_string());
                    }
                }
            }
        }
        for nov in [&kf.t, &kf.d].into_iter().flatten() {
            if let NumOrVar::Var(s) = nov {
                if let Some(var_ref) = s.strip_prefix('$') {
                    if let Some((name, default)) = var_ref.split_once(':') {
                        if name == var_name {
                            return Some(default.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

// ============================================================================
// Input Handling
// ============================================================================

pub(in crate::tui) fn handle_notify_input(app: &mut App, key: KeyCode) {
    use KeyCode::*;
    let ns = &mut app.notify;

    match ns.focus {
        NotifyFocus::EffectList => match key {
            Up | Char('k') => {
                if ns.selected_effect > 0 {
                    ns.selected_effect -= 1;
                    ns.selected_keyframe = 0;
                    app.notify_recompute_preview();
                }
            }
            Down | Char('j') => {
                if ns.selected_effect + 1 < ns.effect_names.len() {
                    ns.selected_effect += 1;
                    ns.selected_keyframe = 0;
                    app.notify_recompute_preview();
                }
            }
            Enter => {
                ns.focus = NotifyFocus::KeyframeList;
                ns.selected_keyframe = 0;
                ns.selected_field = 0;
            }
            Char('n') => {
                app.notify_toggle_daemon();
            }
            Char('p') => {
                let ns = &mut app.notify;
                if ns.daemon_running {
                    app.status_msg =
                        "Hardware preview disabled while daemon is running".to_string();
                } else {
                    ns.preview_on_hardware = !ns.preview_on_hardware;
                    if !ns.preview_on_hardware {
                        // Clear overlay
                        if let Some(ref kb) = app.keyboard {
                            let _ = kb.stream_led_release();
                        }
                        ns.prev_hw_frame = [(0, 0, 0); 96];
                    }
                    app.status_msg = if ns.preview_on_hardware {
                        "Hardware preview ON".to_string()
                    } else {
                        "Hardware preview OFF".to_string()
                    };
                }
            }
            Char('s') => {
                if let Some(ref lib) = app.notify.effects {
                    match lib.save_default() {
                        Ok(()) => {
                            app.notify.dirty = false;
                            app.status_msg = "Saved effects.toml".to_string();
                        }
                        Err(e) => app.status_msg = format!("Save failed: {e}"),
                    }
                }
            }
            _ => {}
        },
        NotifyFocus::KeyframeList => match key {
            Up | Char('k') => {
                if ns.selected_keyframe > 0 {
                    ns.selected_keyframe -= 1;
                }
            }
            Down | Char('j') => {
                let kf_count = current_effect_keyframes(ns).len();
                if ns.selected_keyframe + 1 < kf_count {
                    ns.selected_keyframe += 1;
                }
            }
            Left | Char('h') => {
                if ns.selected_field > 0 {
                    ns.selected_field -= 1;
                }
            }
            Right | Char('l') => {
                if ns.selected_field < 2 {
                    ns.selected_field += 1;
                }
            }
            Enter => {
                // Start editing the selected field
                let kfs = current_effect_keyframes(ns);
                if let Some(kf) = kfs.get(ns.selected_keyframe) {
                    ns.edit_input = match ns.selected_field {
                        0 => {
                            // Timing (d or t)
                            if let Some(ref d) = kf.d {
                                format!("{d}")
                            } else if let Some(ref t) = kf.t {
                                format!("{t}")
                            } else {
                                String::new()
                            }
                        }
                        1 => format!("{}", kf.v),
                        2 => kf.easing.clone(),
                        _ => String::new(),
                    };
                    ns.focus = NotifyFocus::FieldEdit;
                    ns.editing = true;
                }
            }
            Tab => {
                // Jump to variable editing
                ns.focus = NotifyFocus::VarEdit;
                ns.selected_var = 0;
            }
            Char('a') => {
                // Add keyframe after current
                if let Some(ref mut lib) = app.notify.effects {
                    let name = app.notify.effect_names[app.notify.selected_effect].clone();
                    if let Some(def) = lib.effects.get_mut(&name) {
                        let new_kf = KeyframeDef {
                            t: None,
                            d: Some(NumOrVar::Num(500.0)),
                            v: 1.0,
                            color: None,
                            easing: "Linear".to_string(),
                        };
                        let idx = (app.notify.selected_keyframe + 1).min(def.keyframes.len());
                        def.keyframes.insert(idx, new_kf);
                        app.notify.selected_keyframe = idx;
                        app.notify.dirty = true;
                        app.notify_recompute_preview();
                    }
                }
            }
            Char('x') | Delete => {
                // Delete current keyframe
                if let Some(ref mut lib) = app.notify.effects {
                    let name = app.notify.effect_names[app.notify.selected_effect].clone();
                    if let Some(def) = lib.effects.get_mut(&name) {
                        if def.keyframes.len() > 1 {
                            let idx = app.notify.selected_keyframe.min(def.keyframes.len() - 1);
                            def.keyframes.remove(idx);
                            if app.notify.selected_keyframe >= def.keyframes.len() {
                                app.notify.selected_keyframe =
                                    def.keyframes.len().saturating_sub(1);
                            }
                            app.notify.dirty = true;
                            app.notify_recompute_preview();
                        }
                    }
                }
            }
            Esc => {
                ns.focus = NotifyFocus::EffectList;
            }
            _ => {}
        },
        NotifyFocus::FieldEdit => match key {
            Esc => {
                ns.focus = if ns.selected_field == 3 {
                    NotifyFocus::VarEdit
                } else {
                    NotifyFocus::KeyframeList
                };
                ns.editing = false;
            }
            Enter if app.notify.selected_field == 3 => {
                // Save variable value
                let keys: Vec<_> = app.notify.var_values.keys().cloned().collect();
                if let Some(key_name) = keys.get(app.notify.selected_var) {
                    app.notify
                        .var_values
                        .insert(key_name.clone(), app.notify.edit_input.clone());
                }
                app.notify.focus = NotifyFocus::VarEdit;
                app.notify.editing = false;
                app.notify_recompute_preview();
            }
            Enter => {
                // Apply edit
                if let Some(ref mut lib) = app.notify.effects {
                    let name = app.notify.effect_names[app.notify.selected_effect].clone();
                    if let Some(def) = lib.effects.get_mut(&name) {
                        if let Some(kf) = def.keyframes.get_mut(app.notify.selected_keyframe) {
                            match app.notify.selected_field {
                                0 => {
                                    // Timing
                                    let val = parse_num_or_var(&app.notify.edit_input);
                                    if kf.d.is_some() {
                                        kf.d = Some(val);
                                    } else {
                                        kf.t = Some(val);
                                    }
                                }
                                1 => {
                                    if let Ok(v) = app.notify.edit_input.parse::<f64>() {
                                        kf.v = v.clamp(0.0, 1.0);
                                    }
                                }
                                2 => {
                                    kf.easing = app.notify.edit_input.clone();
                                }
                                _ => {}
                            }
                            app.notify.dirty = true;
                        }
                    }
                }
                app.notify.focus = NotifyFocus::KeyframeList;
                app.notify.editing = false;
                app.notify_recompute_preview();
            }
            Left if app.notify.selected_field == 2 => {
                // Cycle easing backward
                if let Some(idx) = EASING_NAMES
                    .iter()
                    .position(|&n| n == app.notify.edit_input)
                {
                    let new_idx = if idx == 0 {
                        EASING_NAMES.len() - 1
                    } else {
                        idx - 1
                    };
                    app.notify.edit_input = EASING_NAMES[new_idx].to_string();
                }
            }
            Right if app.notify.selected_field == 2 => {
                // Cycle easing forward
                if let Some(idx) = EASING_NAMES
                    .iter()
                    .position(|&n| n == app.notify.edit_input)
                {
                    let new_idx = (idx + 1) % EASING_NAMES.len();
                    app.notify.edit_input = EASING_NAMES[new_idx].to_string();
                }
            }
            Backspace => {
                app.notify.edit_input.pop();
            }
            Char(c) => {
                app.notify.edit_input.push(c);
            }
            _ => {}
        },
        NotifyFocus::VarEdit => match key {
            Esc => {
                ns.focus = NotifyFocus::KeyframeList;
            }
            Up | Char('k') => {
                if ns.selected_var > 0 {
                    ns.selected_var -= 1;
                }
            }
            Down | Char('j') => {
                let count = ns.var_values.len();
                if ns.selected_var + 1 < count {
                    ns.selected_var += 1;
                }
            }
            Enter => {
                // Start inline editing of the selected variable value
                let keys: Vec<_> = ns.var_values.keys().cloned().collect();
                if let Some(key_name) = keys.get(ns.selected_var) {
                    ns.edit_input = ns.var_values.get(key_name).cloned().unwrap_or_default();
                    ns.editing = true;
                    ns.focus = NotifyFocus::FieldEdit;
                    // Repurpose selected_field=3 for var editing
                    ns.selected_field = 3;
                }
            }
            Backspace if ns.editing => {
                ns.edit_input.pop();
            }
            Char(c) if ns.editing => {
                ns.edit_input.push(c);
            }
            _ => {}
        },
    }
}

/// Parse a string into NumOrVar (for editing keyframe timing values).
fn parse_num_or_var(s: &str) -> NumOrVar {
    if s.starts_with('$') {
        NumOrVar::Var(s.to_string())
    } else if let Ok(n) = s.parse::<f64>() {
        NumOrVar::Num(n)
    } else {
        NumOrVar::Var(s.to_string())
    }
}

/// Get the keyframes of the currently selected effect.
pub(in crate::tui) fn current_effect_keyframes(ns: &NotifyTabState) -> &[KeyframeDef] {
    let Some(ref lib) = ns.effects else {
        return &[];
    };
    let Some(name) = ns.effect_names.get(ns.selected_effect) else {
        return &[];
    };
    let Some(def) = lib.get(name) else { return &[] };
    &def.keyframes
}

// ============================================================================
// App methods (notify-specific)
// ============================================================================

impl App {
    pub(in crate::tui) fn notify_recompute_preview(&mut self) {
        let ns = &mut self.notify;
        let Some(ref lib) = ns.effects else { return };
        let Some(name) = ns.effect_names.get(ns.selected_effect) else {
            return;
        };
        let Some(def) = lib.get(name) else { return };

        // Populate var_values with defaults for any missing required variables
        let required = required_variables(def);
        for var in &required {
            if !ns.var_values.contains_key(var) {
                // Try to extract default from the definition
                let default_val = extract_var_default(def, var).unwrap_or_default();
                ns.var_values.insert(var.clone(), default_val);
            }
        }
        // Remove vars not in the required set
        ns.var_values.retain(|k, _| required.iter().any(|r| r == k));

        match resolve_effect(def, &ns.var_values) {
            Ok(resolved) => {
                ns.resolved = Some(resolved);
                ns.preview_start = Instant::now();
            }
            Err(_) => {
                ns.resolved = None;
            }
        }
    }

    pub(in crate::tui) fn notify_toggle_daemon(&mut self) {
        if self.notify.daemon_running {
            // Stop
            if let Some(ref cancel) = self.notify.daemon_cancel {
                cancel.store(false, std::sync::atomic::Ordering::SeqCst);
            }
            self.status_msg = "Stopping daemon...".to_string();
        } else {
            // Start — need a keyboard handle
            let Some(ref kb) = self.keyboard else {
                self.status_msg = "Connect to a device first".to_string();
                return;
            };
            let kb = Arc::clone(kb);
            let running = Arc::new(AtomicBool::new(true));
            let cancel = Arc::clone(&running);
            let tx = self.gen_sender();

            let handle = tokio::spawn(async move {
                let result = crate::notify::daemon::run_with_cancel(kb, 30, 400, running).await;
                tx.send(AsyncResult::NotifyDaemonStopped(
                    result.map_err(|e| e.to_string()),
                ));
            });

            self.notify.daemon_running = true;
            self.notify.daemon_cancel = Some(cancel);
            self.notify.daemon_handle = Some(handle);
            self.notify.daemon_error = None;
            self.status_msg = "Notify daemon started".to_string();
        }
    }

    pub(in crate::tui) fn notify_poll_list(&mut self) {
        if !self.notify.daemon_running {
            return;
        }
        if self.notify.last_notif_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.notify.last_notif_poll = Instant::now();
        let tx = self.gen_sender();
        tokio::spawn(async move {
            let list: Vec<(u64, String, String, String, i32)> =
                match zbus::Connection::session().await {
                    Ok(conn) => {
                        let result = zbus::proxy::Builder::<zbus::Proxy<'_>>::new(&conn)
                            .destination("org.monsgeek.Notify1")
                            .and_then(|b| b.path("/org/monsgeek/Notify1"))
                            .and_then(|b| b.interface("org.monsgeek.Notify1"));
                        if let Ok(builder) = result {
                            if let Ok(p) = builder.build().await {
                                p.call_method("List", &())
                                    .await
                                    .ok()
                                    .and_then(|r| r.body().deserialize().ok())
                                    .unwrap_or_default()
                            } else {
                                Vec::new()
                            }
                        } else {
                            Vec::new()
                        }
                    }
                    Err(_) => Vec::new(),
                };
            tx.send(AsyncResult::NotifyList(list));
        });
    }

    pub(in crate::tui) fn notify_tick(&mut self) {
        if self.tab != 4 {
            return;
        }
        // Poll D-Bus for active notifications
        self.notify_poll_list();

        // Hardware preview
        if self.notify.preview_on_hardware && !self.notify.daemon_running {
            if let (Some(ref resolved), Some(ref kb)) = (&self.notify.resolved, &self.keyboard) {
                let elapsed_ms = self.notify.preview_start.elapsed().as_secs_f64() * 1000.0;
                let rgb = resolved.evaluate(elapsed_ms);
                let mut frame = [(rgb.r, rgb.g, rgb.b); 96];
                apply_power_budget(&mut frame, 400);
                if frame != self.notify.prev_hw_frame {
                    let _ = send_overlay_diff(kb, &self.notify.prev_hw_frame, &frame);
                    self.notify.prev_hw_frame = frame;
                }
            }
        }
    }
}

// ============================================================================
// Rendering
// ============================================================================

pub(in crate::tui) fn render_notify(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_notify_left(f, app, chunks[0]);
    render_notify_right(f, app, chunks[1]);
}

fn render_notify_left(f: &mut Frame, app: &App, area: Rect) {
    let ns = &app.notify;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Daemon status
            Constraint::Min(8),    // Effects list
            Constraint::Min(8),    // Keyframe editor
        ])
        .split(area);

    // ── Daemon status ──
    let daemon_status = if ns.daemon_running {
        Span::styled("● Running", Style::default().fg(Color::Green))
    } else if let Some(ref err) = ns.daemon_error {
        Span::styled(format!("● Error: {err}"), Style::default().fg(Color::Red))
    } else {
        Span::styled("○ Stopped", Style::default().fg(Color::DarkGray))
    };
    let hw_preview = if ns.preview_on_hardware {
        Span::styled(" [HW]", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };
    let dirty = if ns.dirty {
        Span::styled(" [modified]", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };
    let daemon_line = Line::from(vec![
        Span::raw("Daemon: "),
        daemon_status,
        hw_preview,
        dirty,
        Span::styled(
            "  [n]toggle [p]hw [s]save",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let daemon_block = Paragraph::new(daemon_line).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Notify Daemon"),
    );
    f.render_widget(daemon_block, chunks[0]);

    // ── Effects list ──
    let items: Vec<ListItem> = ns
        .effect_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let lib = ns.effects.as_ref().unwrap();
            let def = lib.get(name).unwrap();
            let desc = def
                .description
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(30)
                .collect::<String>();
            let style = if i == ns.selected_effect {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:12}", name), style),
                Span::styled(desc, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let highlight = if ns.focus == NotifyFocus::EffectList {
        Style::default().bg(Color::DarkGray)
    } else {
        Style::default()
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Effects [↑↓ Enter]"),
        )
        .highlight_style(highlight);

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(ns.selected_effect));
    f.render_stateful_widget(list, chunks[1], &mut list_state);

    // ── Keyframe editor ──
    render_keyframe_editor(f, app, chunks[2]);
}

fn render_keyframe_editor(f: &mut Frame, app: &App, area: Rect) {
    let ns = &app.notify;
    let kfs = current_effect_keyframes(ns);

    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(4)])
        .split(area);

    // Keyframes
    let mut lines: Vec<Line> = Vec::new();
    for (i, kf) in kfs.iter().enumerate() {
        let is_selected = ns.focus == NotifyFocus::KeyframeList && i == ns.selected_keyframe;
        let prefix = if is_selected { ">" } else { " " };

        let timing = if let Some(ref d) = kf.d {
            format!("d={d}")
        } else if let Some(ref t) = kf.t {
            format!("t={t}")
        } else {
            "?".to_string()
        };

        let field_style = |field_idx: usize| {
            if is_selected && ns.selected_field == field_idx {
                if ns.focus == NotifyFocus::FieldEdit {
                    Style::default().fg(Color::Black).bg(Color::Yellow)
                } else {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::UNDERLINED)
                }
            } else {
                Style::default()
            }
        };

        lines.push(Line::from(vec![
            Span::styled(
                prefix,
                if is_selected {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                },
            ),
            Span::raw(format!("KF{i}: ")),
            Span::styled(format!("{:14}", timing), field_style(0)),
            Span::raw(" v="),
            Span::styled(format!("{:.2}", kf.v), field_style(1)),
            Span::raw(" "),
            Span::styled(kf.easing.clone(), field_style(2)),
        ]));
    }

    if kfs.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no keyframes — solid effect)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    if ns.focus == NotifyFocus::FieldEdit && ns.selected_field < 3 {
        lines.push(Line::from(vec![
            Span::styled("  Edit: ", Style::default().fg(Color::Yellow)),
            Span::styled(&ns.edit_input, Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ]));
    }

    let kf_block = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Keyframes [Enter:edit a:add x:del Tab:vars Esc:back]"),
    );
    f.render_widget(kf_block, inner_chunks[0]);

    // Variables
    let var_lines: Vec<Line> = ns
        .var_values
        .iter()
        .enumerate()
        .map(|(i, (k, v))| {
            let is_sel = ns.focus == NotifyFocus::VarEdit && i == ns.selected_var;
            let style = if is_sel {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(format!("  {k} = "), style),
                Span::styled(
                    v.as_str(),
                    if is_sel && ns.editing {
                        Style::default().bg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::Cyan)
                    },
                ),
            ])
        })
        .collect();

    let var_block = Paragraph::new(if var_lines.is_empty() {
        vec![Line::from(Span::styled(
            "  (no variables)",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        var_lines
    })
    .block(Block::default().borders(Borders::ALL).title("Variables"));
    f.render_widget(var_block, inner_chunks[1]);
}

fn render_notify_right(f: &mut Frame, app: &App, area: Rect) {
    let ns = &app.notify;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Preview grid
            Constraint::Length(4), // Sparkline
            Constraint::Min(5),    // Notification list
        ])
        .split(area);

    // ── Preview grid (6 rows × 16 cols) ──
    render_preview_grid(f, app, chunks[0]);

    // ── Brightness sparkline ──
    render_brightness_sparkline(f, app, chunks[1]);

    // ── Active notifications ──
    let notif_items: Vec<ListItem> = if ns.notifications.is_empty() {
        vec![ListItem::new(Span::styled(
            if ns.daemon_running {
                "No active notifications"
            } else {
                "Daemon not running"
            },
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        ns.notifications
            .iter()
            .map(|(id, key, source, effect, prio)| {
                // Resolve "0,1,16" indices to key names like "Esc,F1,~"
                let key_short: String = {
                    let names: Vec<&str> = key
                        .split(',')
                        .filter_map(|s| s.trim().parse::<usize>().ok())
                        .filter_map(|idx| {
                            ns.labels
                                .get(idx)
                                .map(|l| l.trim())
                                .filter(|l| !l.is_empty())
                        })
                        .collect();
                    let joined = names.join(",");
                    if joined.len() > 14 {
                        format!("{}…", &joined[..joined.floor_char_boundary(13)])
                    } else {
                        joined
                    }
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{id:3} "), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{key_short:14} "), Style::default().fg(Color::Cyan)),
                    Span::styled(format!("{effect:10} "), Style::default().fg(Color::Yellow)),
                    Span::styled(format!("p={prio}"), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!(" ({source})"), Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect()
    };
    let notif_list = List::new(notif_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Active Notifications"),
    );
    f.render_widget(notif_list, chunks[2]);
}

fn render_preview_grid(f: &mut Frame, app: &App, area: Rect) {
    use crate::notify::keymap::{COLS, ROWS};

    let ns = &app.notify;
    let inner = Block::default()
        .borders(Borders::ALL)
        .title("Preview")
        .inner(area);
    f.render_widget(
        Block::default().borders(Borders::ALL).title("Preview"),
        area,
    );

    if inner.height < ROWS as u16 || inner.width < (COLS * 5) as u16 {
        let msg = Paragraph::new("(too small)").style(Style::default().fg(Color::DarkGray));
        f.render_widget(msg, inner);
        return;
    }

    // Evaluate current effect
    let (r, g, b) = if let Some(ref resolved) = ns.resolved {
        let elapsed_ms = ns.preview_start.elapsed().as_secs_f64() * 1000.0;
        let rgb = resolved.evaluate(elapsed_ms);
        (rgb.r, rgb.g, rgb.b)
    } else {
        (40, 40, 40)
    };

    for row in 0..ROWS {
        for col in 0..COLS {
            let idx = row * COLS + col;
            let label = &ns.labels[idx];
            let x = inner.x + (col * 5) as u16;
            let y = inner.y + row as u16;

            if y >= inner.y + inner.height || x + 5 > inner.x + inner.width {
                continue;
            }

            let (fg, bg) = if label.trim().is_empty() {
                (Color::DarkGray, Color::Rgb(20, 20, 20))
            } else if ns.resolved.is_some() {
                let lum = (r as u16 + g as u16 + b as u16) / 3;
                let fg = if lum > 128 {
                    Color::Black
                } else {
                    Color::White
                };
                (fg, Color::Rgb(r, g, b))
            } else {
                (Color::DarkGray, Color::Rgb(40, 40, 40))
            };

            let cell =
                Paragraph::new(format!("{:^5}", label)).style(Style::default().fg(fg).bg(bg));
            f.render_widget(
                cell,
                Rect {
                    x,
                    y,
                    width: 5,
                    height: 1,
                },
            );
        }
    }
}

fn render_brightness_sparkline(f: &mut Frame, app: &App, area: Rect) {
    let ns = &app.notify;

    // Compute brightness curve: 64 samples across one cycle
    let data: Vec<u64> = if let Some(ref resolved) = ns.resolved {
        let dur = resolved.duration_ms.max(1.0);
        (0..64)
            .map(|i| {
                let t = (i as f64 / 64.0) * dur;
                let rgb = resolved.evaluate(t);
                // Max channel as brightness proxy
                let b = rgb.r.max(rgb.g).max(rgb.b);
                (b as f64 / 255.0 * 64.0) as u64
            })
            .collect()
    } else {
        vec![0; 64]
    };

    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::ALL).title("Brightness"))
        .data(&data)
        .max(64)
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(sparkline, area);
}
