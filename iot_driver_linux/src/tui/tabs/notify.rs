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

use super::super::shared::AsyncResult;
use super::super::App;

// ============================================================================
// Types
// ============================================================================

/// Notify tab state — effect editor, daemon control, preview.
pub(in crate::tui) struct NotifyTabState {
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

    // D-Bus notification list
    pub notifications: Vec<(u64, String, String, String, i32)>,
    pub last_notif_poll: Instant,

    // Labels for preview grid
    pub labels: Vec<String>,

    /// LED power budget in mA (0 = unlimited, step 100)
    pub power_budget: u32,

    /// Shared slot info: daemon writes effect name + resolved, TUI reads for display/sparklines.
    pub slot_info: crate::anim::SharedSlotInfo,

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
            notifications: Vec::new(),
            last_notif_poll: Instant::now(),
            labels: crate::effect::preview::build_labels(),
            power_budget: 400,
            slot_info: Arc::new(std::sync::Mutex::new(crate::anim::SlotInfo::default())),
            dirty: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(in crate::tui) enum NotifyFocus {
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
                }
                app.notify_recompute_preview();
            }
            Down | Char('j') => {
                if ns.selected_effect + 1 < ns.effect_names.len() {
                    ns.selected_effect += 1;
                    ns.selected_keyframe = 0;
                }
                app.notify_recompute_preview();
            }
            Enter => {
                ns.focus = NotifyFocus::KeyframeList;
                ns.selected_keyframe = 0;
                ns.selected_field = 0;
            }
            Char('p') => {
                // Always program preview to keyboard
                app.notify_recompute_preview();
                app.notify_program_preview();
            }
            Char('s') => {
                app.notify_toggle_daemon();
            }
            Char('w') => {
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
            Right | Char('>') | Char('.') => {
                let ns = &mut app.notify;
                ns.power_budget = ns.power_budget.saturating_add(100);
                app.status_msg = if ns.power_budget == 0 {
                    "Power budget: unlimited".to_string()
                } else {
                    format!("Power budget: {}mA", ns.power_budget)
                };
            }
            Left | Char('<') | Char(',') => {
                let ns = &mut app.notify;
                ns.power_budget = ns.power_budget.saturating_sub(100);
                app.status_msg = if ns.power_budget == 0 {
                    "Power budget: unlimited".to_string()
                } else {
                    format!("Power budget: {}mA", ns.power_budget)
                };
            }
            Char('c') => {
                if let Some(ref kb) = app.keyboard {
                    let _ = kb.anim_clear();
                    app.status_msg = "Cleared all animations".to_string();
                    app.notify.preview_on_hardware = false;
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

            let power_budget = self.notify.power_budget;
            let labels = Arc::clone(&self.notify.slot_info);
            let handle = tokio::spawn(async move {
                let result =
                    crate::notify::daemon::run_with_cancel(kb, power_budget, running, labels).await;
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

    /// Poll animation engine status periodically.
    /// Polls every 500ms on the notify tab, or every 2s on other tabs if overlay is active.
    pub(in crate::tui) fn poll_anim_status(&mut self) {
        let has_patch = self
            .patch_info
            .as_ref()
            .is_some_and(|p| p.capabilities.contains(&"anim_engine"));
        if !has_patch {
            return;
        }
        let interval = if self.tab == 4 {
            self.anim_poll_interval // 500ms on notify tab
        } else if self
            .anim_snapshot
            .as_ref()
            .is_some_and(|s| s.overlay_active())
        {
            Duration::from_secs(2) // 2s if overlay active on other tabs
        } else {
            return; // don't poll if not on notify tab and no overlay
        };
        if self.last_anim_poll.elapsed() < interval {
            return;
        }
        self.last_anim_poll = Instant::now();

        let Some(keyboard) = self.keyboard.clone() else {
            return;
        };
        let tx = self.gen_sender();
        tokio::spawn(async move {
            let engine = crate::anim::AnimEngine::new(keyboard);
            let result = engine.query_full();
            tx.send(AsyncResult::AnimStatus(result));
        });
    }

    pub(in crate::tui) fn notify_tick(&mut self) {
        if self.tab != 4 {
            return;
        }
        // Poll D-Bus for active notifications
        self.notify_poll_list();
    }

    /// Program the current preview effect to the animation engine (slot 7, QWERTY row).
    pub(in crate::tui) fn notify_program_preview(&mut self) {
        let Some(ref resolved) = self.notify.resolved else {
            self.status_msg = "No effect resolved".to_string();
            return;
        };
        let Some(ref kb) = self.keyboard else {
            self.status_msg = "No device connected".to_string();
            return;
        };

        let compiled = match resolved.compile_for_firmware(127, false) {
            Some(c) => c,
            None => {
                self.status_msg = "Effect has no keyframes".to_string();
                return;
            }
        };

        // Cancel previous preview
        let _ = kb.anim_cancel(7);

        // Program slot 7 with max priority so it wins over daemon animations
        if let Err(e) = kb.anim_define(
            7,
            compiled.flags,
            compiled.priority,
            compiled.duration_ticks,
            &compiled.keyframes,
        ) {
            self.status_msg = format!("Preview failed: {e}");
            return;
        }

        // Assign QWERTY row (matrix indices 33-44), no phase offset
        let keys: Vec<(u8, u8)> = (33..=44).map(|idx| (idx, 0)).collect();
        if let Err(e) = kb.anim_assign(7, &keys) {
            self.status_msg = format!("Preview assign failed: {e}");
            return;
        }

        // Cache slot info for TUI display + sparkline
        let effect_name = self
            .notify
            .effect_names
            .get(self.notify.selected_effect)
            .cloned()
            .unwrap_or_default();
        if let Some(ref resolved) = self.notify.resolved {
            self.notify.slot_info.lock().unwrap().set(
                7,
                crate::anim::SlotEntry {
                    effect_name,
                    resolved: resolved.clone(),
                    compiled,
                },
            );
        }

        self.notify.preview_on_hardware = true;
        self.status_msg = "Preview playing on QWERTY row (on-device)".to_string();
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
    let power_str = if ns.power_budget == 0 {
        "unlimited".to_string()
    } else {
        format!("{}mA", ns.power_budget)
    };
    let daemon_line = Line::from(vec![
        Span::raw("Daemon: "),
        daemon_status,
        hw_preview,
        Span::styled(
            format!("  pwr:{power_str}"),
            Style::default().fg(Color::DarkGray),
        ),
        dirty,
        Span::styled(
            "  [p]play [s]service [w]save [c]clear [<>]pwr",
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Preview grid
            Constraint::Length(4), // Sparkline
            Constraint::Min(4),    // Animation engine + keys (merged)
        ])
        .split(area);

    render_preview_grid(f, app, chunks[0]);
    render_animation_curve(
        f,
        chunks[1],
        app.notify.resolved.as_ref(),
        "Brightness",
        0.0,
        0.0,
    );
    render_anim_status(f, app, chunks[2]);
}

fn render_anim_status(f: &mut Frame, app: &App, area: Rect) {
    let snap = app.anim_snapshot.as_ref();

    // Render block border
    let title = snap
        .map(|s| format!("Engine ({})", s.active_count()))
        .unwrap_or_else(|| "Engine".to_string());
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.width < 4 {
        return;
    }

    let Some(snap) = snap else {
        let msg = if app.notify.daemon_running {
            "Daemon running (no anim engine)"
        } else {
            "No animation engine"
        };
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    };

    // Interpolate frame_count for smooth sparkline scrolling
    let elapsed_since_poll = app.anim_snapshot_time.elapsed().as_secs_f64();
    let interp_frames = snap.frame_count() as f64 + elapsed_since_poll * crate::anim::TICK_RATE_HZ;

    // Lay out content line by line within `inner`
    let mut y = inner.y;
    let max_y = inner.y + inner.height;

    let active_defs: Vec<_> = snap.defs().iter().filter(|d| d.key_count > 0).collect();
    if active_defs.is_empty() && y < max_y {
        f.render_widget(
            Paragraph::new("(idle)").style(Style::default().fg(Color::DarkGray)),
            Rect::new(inner.x, y, inner.width, 1),
        );
        return;
    }

    for d in &active_defs {
        if y >= max_y {
            break;
        }

        let mode = match (d.is_one_shot(), d.is_rainbow()) {
            (true, true) => "1s+rbw",
            (true, false) => "1shot",
            (false, true) => "rainbow",
            (false, false) => "loop",
        };

        // Get cached slot info (effect name + resolved effect)
        let slot_entry = app
            .notify
            .slot_info
            .lock()
            .ok()
            .and_then(|si| si.get(d.id).cloned());

        let mut spans = vec![Span::styled(
            format!("def[{}]", d.id),
            Style::default().fg(Color::Yellow),
        )];
        if let Some(ref entry) = slot_entry {
            spans.push(Span::styled(
                format!(" {}", entry.effect_name),
                Style::default().fg(Color::Green),
            ));
        }
        spans.extend([
            Span::styled(format!(" {}KF ", d.num_kf), Style::default()),
            Span::styled(
                format!("p={} ", d.priority),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(mode, Style::default().fg(Color::Magenta)),
        ]);
        f.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;

        // Animation curve (2 rows high) — uses same widget as brightness preview
        if y + 1 < max_y && d.duration_ticks > 0 {
            let dur_ticks = d.duration_ticks as f64;
            let resolved = slot_entry.as_ref().map(|e| &e.resolved);
            let now_ticks = interp_frames % dur_ticks;

            let curve_area = Rect::new(inner.x, y, inner.width, 2);
            render_animation_curve(
                f,
                // No border for inline engine curves — render directly
                Rect::new(
                    curve_area.x,
                    curve_area.y,
                    curve_area.width,
                    curve_area.height,
                ),
                resolved,
                "",
                now_ticks,
                dur_ticks,
            );
            y += 2;
        }

        if let Some(keys) = snap.keys.get(&d.id) {
            // Key list per phase group
            let labels = &app.notify.labels;
            let mut groups: std::collections::BTreeMap<u8, Vec<u8>> =
                std::collections::BTreeMap::new();
            for k in keys {
                groups.entry(k.phase_offset).or_default().push(k.strip_idx);
            }
            for (&phase, strip_indices) in &groups {
                if y >= max_y {
                    break;
                }
                let keys_str: String = strip_indices
                    .iter()
                    .map(|&s| {
                        let l = crate::notify::keymap::strip_to_label(s, labels).trim();
                        if l.is_empty() {
                            format!("#{s}")
                        } else {
                            l.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let phase_ms = phase as u32 * 8 * crate::anim::MS_PER_TICK as u32;
                let phase_label = if phase == 0 {
                    "+0 ".to_string()
                } else {
                    format!("+{phase_ms}ms ")
                };
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(phase_label, Style::default().fg(Color::DarkGray)),
                        Span::styled(keys_str, Style::default().fg(Color::Cyan)),
                    ])),
                    Rect::new(inner.x, y, inner.width, 1),
                );
                y += 1;
            }
        }
    }
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

    // QWERTY preview row: matrix indices 33-44 (row 2)
    const PREVIEW_ROW: usize = 2;
    const PREVIEW_START: usize = PREVIEW_ROW * COLS;
    const PREVIEW_END: usize = PREVIEW_START + 12; // Q through ]

    for row in 0..ROWS {
        for col in 0..COLS {
            let idx = row * COLS + col;
            let label = &ns.labels[idx];
            let x = inner.x + (col * 5) as u16;
            let y = inner.y + row as u16;

            if y >= inner.y + inner.height || x + 5 > inner.x + inner.width {
                continue;
            }

            let in_preview = (PREVIEW_START..PREVIEW_END).contains(&idx);
            let (fg, bg) = if label.trim().is_empty() {
                (Color::DarkGray, Color::Rgb(20, 20, 20))
            } else if in_preview && ns.resolved.is_some() {
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

/// Render an animation curve with interpolated colors.
/// Each column samples the resolved effect at that time offset and renders
/// a bar whose height = brightness and color = the actual RGB color.
/// `phase_offset_ticks` shifts the time for each column (0 for simple preview).
/// `time_offset_ticks` is the current playback position (0 for static view).
fn render_animation_curve(
    f: &mut Frame,
    area: Rect,
    resolved: Option<&ResolvedEffect>,
    title: &str,
    time_offset_ticks: f64,
    dur_ticks: f64,
) {
    let inner = if title.is_empty() {
        area
    } else {
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(area);
        f.render_widget(block, area);
        inner
    };

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let Some(resolved) = resolved else {
        return;
    };

    let w = inner.width as usize;
    let h = inner.height as usize;

    // Use ticks if provided, otherwise use duration_ms directly
    let use_ticks = dur_ticks > 0.0;

    // Unicode block characters for sub-cell resolution (eighths)
    const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    for col in 0..w {
        let t_ms = if use_ticks {
            let wall_ticks = time_offset_ticks + (col as f64 / w as f64) * dur_ticks;
            (wall_ticks % dur_ticks) * crate::anim::MS_PER_TICK
        } else {
            (col as f64 / w as f64) * resolved.duration_ms.max(1.0)
        };

        let rgb = resolved.evaluate(t_ms);
        let brightness = rgb.r.max(rgb.g).max(rgb.b) as f64 / 255.0;
        let color = Color::Rgb(rgb.r, rgb.g, rgb.b);

        // Height in sub-cells (eighths of a cell)
        let bar_eighths = (brightness * (h * 8) as f64).round() as usize;

        // Render bottom-up
        for row in 0..h {
            let cell_bottom = row * 8; // eighths from bottom
            let fill = bar_eighths.saturating_sub(cell_bottom).min(8);
            let ch = BLOCKS[fill];
            let y = inner.y + (h - 1 - row) as u16;
            let x = inner.x + col as u16;

            let style = if fill == 8 {
                // Full block: color the background
                Style::default().bg(color).fg(color)
            } else if fill > 0 {
                // Partial block: colored foreground
                Style::default().fg(color)
            } else {
                Style::default()
            };

            f.render_widget(
                Paragraph::new(ch.to_string()).style(style),
                Rect::new(x, y, 1, 1),
            );
        }
    }
}
