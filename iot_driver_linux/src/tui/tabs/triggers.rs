// Triggers Tab — trigger settings, key modes, edit modal, keyboard layout view
//
// All triggers-specific types, rendering, and App methods.

use ratatui::{prelude::*, widgets::*};
use std::collections::VecDeque;
use throbber_widgets_tui::Throbber;
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::{key_mode, magnetism, TriggerSettings};
use monsgeek_keyboard::{KeyMode, KeyTriggerSettings, Precision};

use super::super::shared::{AsyncResult, LoadState, SpinnerConfig, TriggerViewMode};
use super::super::App;

// ============================================================================
// Types
// ============================================================================

/// Trigger edit modal target - what we're editing
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TriggerEditTarget {
    /// Edit global settings (applies to all keys)
    Global,
    /// Edit specific key settings
    PerKey { key_index: usize },
}

/// Editable field in trigger settings
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TriggerField {
    Actuation,
    Release,
    RtPress,
    RtLift,
    TopDeadzone,
    BottomDeadzone,
    Mode,
}

impl TriggerField {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Actuation => "Actuation",
            Self::Release => "Release",
            Self::RtPress => "RT Press",
            Self::RtLift => "RT Lift",
            Self::TopDeadzone => "Top DZ",
            Self::BottomDeadzone => "Bottom DZ",
            Self::Mode => "Mode",
        }
    }

    pub(crate) fn all() -> &'static [TriggerField] {
        &[
            Self::Actuation,
            Self::Release,
            Self::RtPress,
            Self::RtLift,
            Self::TopDeadzone,
            Self::BottomDeadzone,
            Self::Mode,
        ]
    }

    /// Get spinner configuration for this field (None for Mode which is cycled)
    pub(crate) fn spinner_config(&self) -> Option<SpinnerConfig> {
        match self {
            Self::Actuation | Self::Release => Some(SpinnerConfig {
                min: 0.1,
                max: 4.0,
                step: 0.05,
                step_coarse: 0.2,
                decimals: 2,
                unit: "mm",
            }),
            Self::RtPress | Self::RtLift => Some(SpinnerConfig {
                min: 0.1,
                max: 2.0,
                step: 0.05,
                step_coarse: 0.1,
                decimals: 2,
                unit: "mm",
            }),
            Self::TopDeadzone | Self::BottomDeadzone => Some(SpinnerConfig {
                min: 0.0,
                max: 1.0,
                step: 0.05,
                step_coarse: 0.1,
                decimals: 2,
                unit: "mm",
            }),
            Self::Mode => None, // Mode is cycled, not a spinner
        }
    }
}

/// Trigger edit modal state
#[derive(Debug, Clone)]
pub(crate) struct TriggerEditModal {
    /// What we're editing (global or per-key)
    pub target: TriggerEditTarget,
    /// Currently focused field
    pub field_index: usize,
    /// Depth history for the chart (samples over time)
    pub depth_history: VecDeque<f32>,
    /// Key to filter depth reports (None = show all active keys)
    pub depth_filter: Option<usize>,
    /// Current values being edited
    pub actuation_mm: f32,
    pub release_mm: f32,
    pub rt_press_mm: f32,
    pub rt_lift_mm: f32,
    pub top_dz_mm: f32,
    pub bottom_dz_mm: f32,
    pub mode: u8,
}

impl TriggerEditModal {
    /// Create modal for editing global settings
    pub(crate) fn new_global(triggers: &TriggerSettings, precision: Precision) -> Self {
        let factor = precision.factor() as f32;
        Self {
            target: TriggerEditTarget::Global,
            field_index: 0,
            depth_history: VecDeque::with_capacity(100),
            depth_filter: None,
            actuation_mm: triggers.press_travel.first().copied().unwrap_or(0) as f32 / factor,
            release_mm: triggers.lift_travel.first().copied().unwrap_or(0) as f32 / factor,
            rt_press_mm: triggers.rt_press.first().copied().unwrap_or(0) as f32 / factor,
            rt_lift_mm: triggers.rt_lift.first().copied().unwrap_or(0) as f32 / factor,
            top_dz_mm: triggers.top_deadzone.first().copied().unwrap_or(0) as f32 / factor,
            bottom_dz_mm: triggers.bottom_deadzone.first().copied().unwrap_or(0) as f32 / factor,
            mode: triggers.key_modes.first().copied().unwrap_or(0),
        }
    }

    /// Create modal for editing a specific key
    pub(crate) fn new_per_key(
        key_index: usize,
        triggers: &TriggerSettings,
        precision: Precision,
    ) -> Self {
        let factor = precision.factor() as f32;
        Self {
            target: TriggerEditTarget::PerKey { key_index },
            field_index: 0,
            depth_history: VecDeque::with_capacity(100),
            depth_filter: Some(key_index),
            actuation_mm: triggers.press_travel.get(key_index).copied().unwrap_or(0) as f32
                / factor,
            release_mm: triggers.lift_travel.get(key_index).copied().unwrap_or(0) as f32 / factor,
            rt_press_mm: triggers.rt_press.get(key_index).copied().unwrap_or(0) as f32 / factor,
            rt_lift_mm: triggers.rt_lift.get(key_index).copied().unwrap_or(0) as f32 / factor,
            top_dz_mm: triggers.top_deadzone.get(key_index).copied().unwrap_or(0) as f32 / factor,
            bottom_dz_mm: triggers
                .bottom_deadzone
                .get(key_index)
                .copied()
                .unwrap_or(0) as f32
                / factor,
            mode: triggers.key_modes.get(key_index).copied().unwrap_or(0),
        }
    }

    pub(crate) fn current_field(&self) -> TriggerField {
        TriggerField::all()[self.field_index]
    }

    pub(crate) fn next_field(&mut self) {
        self.field_index = (self.field_index + 1) % TriggerField::all().len();
    }

    pub(crate) fn prev_field(&mut self) {
        self.field_index = if self.field_index == 0 {
            TriggerField::all().len() - 1
        } else {
            self.field_index - 1
        };
    }

    /// Get the current value for the selected field
    pub(crate) fn current_value(&self) -> f32 {
        match self.current_field() {
            TriggerField::Actuation => self.actuation_mm,
            TriggerField::Release => self.release_mm,
            TriggerField::RtPress => self.rt_press_mm,
            TriggerField::RtLift => self.rt_lift_mm,
            TriggerField::TopDeadzone => self.top_dz_mm,
            TriggerField::BottomDeadzone => self.bottom_dz_mm,
            TriggerField::Mode => self.mode as f32,
        }
    }

    /// Set the value for the selected field
    pub(crate) fn set_current_value(&mut self, value: f32) {
        match self.current_field() {
            TriggerField::Actuation => self.actuation_mm = value,
            TriggerField::Release => self.release_mm = value,
            TriggerField::RtPress => self.rt_press_mm = value,
            TriggerField::RtLift => self.rt_lift_mm = value,
            TriggerField::TopDeadzone => self.top_dz_mm = value,
            TriggerField::BottomDeadzone => self.bottom_dz_mm = value,
            TriggerField::Mode => {} // Mode is cycled, not set directly
        }
    }

    /// Increment the current field value (using spinner config)
    pub(crate) fn increment_current(&mut self, coarse: bool) {
        if let Some(config) = self.current_field().spinner_config() {
            let new_value = config.increment(self.current_value(), coarse);
            self.set_current_value(new_value);
        } else if self.current_field() == TriggerField::Mode {
            self.cycle_mode();
        }
    }

    /// Decrement the current field value (using spinner config)
    pub(crate) fn decrement_current(&mut self, coarse: bool) {
        if let Some(config) = self.current_field().spinner_config() {
            let new_value = config.decrement(self.current_value(), coarse);
            self.set_current_value(new_value);
        } else if self.current_field() == TriggerField::Mode {
            self.cycle_mode_reverse();
        }
    }

    /// Cycle mode forward: Normal -> RT -> DKS -> SnapTap -> Normal
    pub(crate) fn cycle_mode(&mut self) {
        self.mode = match self.mode & 0x7F {
            0 => 0x80,                       // Normal -> RT
            _ if self.mode & 0x80 != 0 => 2, // RT -> DKS
            2 => 7,                          // DKS -> SnapTap
            7 => 0,                          // SnapTap -> Normal
            _ => 0,                          // Unknown -> Normal
        };
    }

    /// Cycle mode backward: Normal <- RT <- DKS <- SnapTap <- Normal
    pub(crate) fn cycle_mode_reverse(&mut self) {
        self.mode = match self.mode & 0x7F {
            0 if self.mode & 0x80 != 0 => 0, // RT -> Normal
            0 => 7,                          // Normal -> SnapTap
            2 => 0x80,                       // DKS -> RT
            7 => 2,                          // SnapTap -> DKS
            _ => 0,                          // Unknown -> Normal
        };
    }

    /// Add a depth sample to history
    pub(crate) fn push_depth(&mut self, depth_mm: f32) {
        if self.depth_history.len() >= 100 {
            self.depth_history.pop_front();
        }
        self.depth_history.push_back(depth_mm);
    }
}

// ============================================================================
// App methods
// ============================================================================

impl App {
    /// Load trigger settings (tab 2).
    /// Spawns a background task to avoid blocking the UI.
    pub(in crate::tui) fn load_triggers(&mut self) {
        let Some(keyboard) = self.keyboard.clone() else {
            return;
        };

        self.loading.triggers = LoadState::Loading;
        let tx = self.gen_sender();
        tokio::spawn(async move {
            let result = keyboard
                .get_all_triggers()
                .map(|triggers| TriggerSettings {
                    press_travel: triggers.press_travel,
                    lift_travel: triggers.lift_travel,
                    rt_press: triggers.rt_press,
                    rt_lift: triggers.rt_lift,
                    key_modes: triggers.key_modes,
                    bottom_deadzone: triggers.bottom_deadzone,
                    top_deadzone: triggers.top_deadzone,
                })
                .map_err(|e| e.to_string());
            tx.send(AsyncResult::Triggers(result));
        });
    }

    pub(in crate::tui) fn set_all_key_modes(&mut self, mode: u8) {
        let key_count = self
            .triggers
            .as_ref()
            .map(|t| t.key_modes.len())
            .unwrap_or(0);
        if key_count == 0 {
            return;
        }
        // TODO: Implement bulk key mode setting in keyboard interface
        // For now, update locally
        let modes: Vec<u8> = vec![mode; key_count];
        if let Some(ref mut triggers) = self.triggers {
            triggers.key_modes = modes;
        }
        self.status_msg = format!("Set all keys to {}", magnetism::mode_name(mode));
    }

    /// Set mode for a single key (used in layout view)
    pub(in crate::tui) fn set_single_key_mode(&mut self, key_index: usize, mode: u8) {
        let valid = self
            .triggers
            .as_ref()
            .map(|t| key_index < t.key_modes.len())
            .unwrap_or(false);
        if !valid {
            self.status_msg = format!("Invalid key index: {key_index}");
            return;
        }
        // TODO: Implement single key mode setting in keyboard interface
        if let Some(ref mut triggers) = self.triggers {
            triggers.key_modes[key_index] = mode;
        }
        let key_name = get_key_label(self, key_index);
        self.status_msg = format!(
            "Key {} ({}) set to {}",
            key_index,
            key_name,
            magnetism::mode_name(mode)
        );
    }

    /// Set key mode - dispatches to single or all based on view mode
    pub(in crate::tui) fn set_key_mode(&mut self, mode: u8) {
        if self.trigger_view_mode == TriggerViewMode::Layout {
            self.set_single_key_mode(self.trigger_selected_key, mode);
        } else {
            self.set_all_key_modes(mode);
        }
    }

    /// Toggle trigger view mode between List and Layout
    pub(in crate::tui) fn toggle_trigger_view(&mut self) {
        self.trigger_view_mode = match self.trigger_view_mode {
            TriggerViewMode::List => TriggerViewMode::Layout,
            TriggerViewMode::Layout => TriggerViewMode::List,
        };
        self.status_msg = format!("Trigger view: {:?}", self.trigger_view_mode);
    }

    /// Open trigger edit modal for global settings
    pub(in crate::tui) fn open_trigger_edit_global(&mut self) {
        if let Some(ref triggers) = self.triggers {
            let modal = TriggerEditModal::new_global(triggers, self.precision);
            self.trigger_edit_modal = Some(modal);
            // Enable depth monitoring for the modal
            if !self.depth_monitoring {
                if let Some(ref keyboard) = self.keyboard {
                    let _ = keyboard.start_magnetism_report();
                }
                self.depth_monitoring = true;
            }
            self.status_msg = "Editing global triggers (press keys to see depth)".to_string();
        } else {
            self.status_msg = "No trigger data loaded".to_string();
        }
    }

    /// Open trigger edit modal for a specific key
    pub(in crate::tui) fn open_trigger_edit_key(&mut self, key_index: usize) {
        if let Some(ref triggers) = self.triggers {
            let modal = TriggerEditModal::new_per_key(key_index, triggers, self.precision);
            self.trigger_edit_modal = Some(modal);
            // Enable depth monitoring for the modal
            if !self.depth_monitoring {
                if let Some(ref keyboard) = self.keyboard {
                    let _ = keyboard.start_magnetism_report();
                }
                self.depth_monitoring = true;
            }
            let key_name = get_key_label(self, key_index);
            self.status_msg = format!(
                "Editing key {} ({}) - press it to see depth",
                key_index, key_name
            );
        } else {
            self.status_msg = "No trigger data loaded".to_string();
        }
    }

    /// Close trigger edit modal without saving
    pub(in crate::tui) fn close_trigger_edit_modal(&mut self) {
        self.trigger_edit_modal = None;
        self.status_msg = "Edit cancelled".to_string();
    }

    /// Save trigger edit modal changes
    pub(in crate::tui) fn save_trigger_edit_modal(&mut self) {
        let modal = match self.trigger_edit_modal.take() {
            Some(m) => m,
            None => return,
        };

        let Some(ref keyboard) = self.keyboard else {
            self.status_msg = "No keyboard connected".to_string();
            return;
        };

        let precision = self.precision;
        let factor = precision.factor() as f32;

        match modal.target {
            TriggerEditTarget::Global => {
                // Apply all global settings
                let actuation_raw = (modal.actuation_mm * factor) as u16;
                let release_raw = (modal.release_mm * factor) as u16;
                let rt_press_raw = (modal.rt_press_mm * factor) as u16;
                let rt_lift_raw = (modal.rt_lift_mm * factor) as u16;
                let top_dz_raw = (modal.top_dz_mm * factor) as u16;
                let bottom_dz_raw = (modal.bottom_dz_mm * factor) as u16;

                let mut errors = Vec::new();

                if let Err(e) = keyboard.set_actuation_all_u16(actuation_raw) {
                    errors.push(format!("actuation: {e}"));
                }
                if let Err(e) = keyboard.set_release_all_u16(release_raw) {
                    errors.push(format!("release: {e}"));
                }
                if let Err(e) = keyboard.set_rt_press_all_u16(rt_press_raw) {
                    errors.push(format!("rt_press: {e}"));
                }
                if let Err(e) = keyboard.set_rt_lift_all_u16(rt_lift_raw) {
                    errors.push(format!("rt_lift: {e}"));
                }
                if let Err(e) = keyboard.set_top_deadzone_all_u16(top_dz_raw) {
                    errors.push(format!("top_dz: {e}"));
                }
                if let Err(e) = keyboard.set_bottom_deadzone_all_u16(bottom_dz_raw) {
                    errors.push(format!("bottom_dz: {e}"));
                }

                if errors.is_empty() {
                    self.status_msg = format!(
                        "Global triggers saved: act={:.2}mm rel={:.2}mm",
                        modal.actuation_mm, modal.release_mm
                    );
                    // Reload triggers to reflect changes
                    self.load_triggers();
                } else {
                    self.status_msg = format!("Errors: {}", errors.join(", "));
                }
            }
            TriggerEditTarget::PerKey { key_index } => {
                // Per-key uses u8 values with factor of 10 (0.1mm precision)
                let settings = KeyTriggerSettings {
                    key_index: key_index as u8,
                    actuation: (modal.actuation_mm * 10.0) as u8,
                    deactuation: (modal.release_mm * 10.0) as u8,
                    mode: KeyMode::from_u8(modal.mode),
                };

                match keyboard.set_key_trigger(&settings) {
                    Ok(()) => {
                        let key_name = get_key_label(self, key_index);
                        self.status_msg = format!(
                            "Key {} ({}) saved: act={:.1}mm rel={:.1}mm mode={:?}",
                            key_index,
                            key_name,
                            modal.actuation_mm,
                            modal.release_mm,
                            settings.mode
                        );
                        // Reload triggers to reflect changes
                        self.load_triggers();
                    }
                    Err(e) => {
                        self.status_msg = format!("Failed to save key {}: {}", key_index, e);
                    }
                }
            }
        }
    }

    /// Navigate to next valid key in layout view (Tab key)
    #[allow(dead_code)]
    pub(in crate::tui) fn layout_key_next(&mut self) {
        let max_key = self
            .triggers
            .as_ref()
            .map(|t| t.key_modes.len().saturating_sub(1))
            .unwrap_or(125);

        // Find next non-empty key
        for next in (self.trigger_selected_key + 1)..=max_key {
            if self.is_valid_key_position(next) {
                self.trigger_selected_key = next;
                return;
            }
        }
    }

    /// Navigate to previous valid key in layout view (Shift+Tab key)
    #[allow(dead_code)]
    pub(in crate::tui) fn layout_key_prev(&mut self) {
        if self.trigger_selected_key == 0 {
            return;
        }

        // Find previous non-empty key
        for prev in (0..self.trigger_selected_key).rev() {
            if self.is_valid_key_position(prev) {
                self.trigger_selected_key = prev;
                return;
            }
        }
    }

    /// Move up one row in keyboard layout
    pub(in crate::tui) fn layout_key_up(&mut self) {
        let col = self.trigger_selected_key / 6;
        let row = self.trigger_selected_key % 6;
        if row > 0 {
            let new_pos = col * 6 + (row - 1);
            if self.is_valid_key_position(new_pos) {
                self.trigger_selected_key = new_pos;
            }
        }
    }

    /// Move down one row in keyboard layout
    pub(in crate::tui) fn layout_key_down(&mut self) {
        let col = self.trigger_selected_key / 6;
        let row = self.trigger_selected_key % 6;
        if row < 5 {
            let new_pos = col * 6 + (row + 1);
            if new_pos < self.matrix_size && self.is_valid_key_position(new_pos) {
                self.trigger_selected_key = new_pos;
            }
        }
    }

    /// Move left one column in keyboard layout
    pub(in crate::tui) fn layout_key_left(&mut self) {
        let col = self.trigger_selected_key / 6;
        let row = self.trigger_selected_key % 6;
        if col > 0 {
            let new_pos = (col - 1) * 6 + row;
            if self.is_valid_key_position(new_pos) {
                self.trigger_selected_key = new_pos;
            }
        }
    }

    /// Move right one column in keyboard layout
    pub(in crate::tui) fn layout_key_right(&mut self) {
        let col = self.trigger_selected_key / 6;
        let row = self.trigger_selected_key % 6;
        if col < 20 {
            // 21 columns total
            let new_pos = (col + 1) * 6 + row;
            if new_pos < self.matrix_size && self.is_valid_key_position(new_pos) {
                self.trigger_selected_key = new_pos;
            }
        }
    }

    /// Check if a matrix position has an active key
    pub(in crate::tui) fn is_valid_key_position(&self, pos: usize) -> bool {
        if pos >= self.matrix_size {
            return false;
        }
        let name = get_key_label(self, pos);
        !name.is_empty() && name != "?"
    }
}

// ============================================================================
// Render functions
// ============================================================================

pub(in crate::tui) fn render_trigger_settings(f: &mut Frame, app: &mut App, area: Rect) {
    // Check loading state first
    if app.loading.triggers == LoadState::Loading {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Trigger Settings [v: toggle view, ↑/↓: select, ←/→: adjust]");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let throbber = Throbber::default()
            .label("Loading trigger settings...")
            .throbber_style(Style::default().fg(Color::Yellow));
        f.render_stateful_widget(throbber, inner, &mut app.throbber_state.clone());
        return;
    }

    if app.triggers.is_none() {
        let msg = if app.loading.triggers == LoadState::Error {
            "Failed to load trigger settings"
        } else {
            "No trigger settings loaded"
        };
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(msg, Style::default().fg(Color::Red))),
            Line::from(""),
            Line::from("Press 'r' to load trigger settings from device"),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Trigger Settings"),
        );
        f.render_widget(help, area);
        return;
    }

    match app.trigger_view_mode {
        TriggerViewMode::List => render_trigger_list(f, app, area),
        TriggerViewMode::Layout => render_trigger_layout(f, app, area),
    }
}

/// Render trigger settings as a list view
fn render_trigger_list(f: &mut Frame, app: &mut App, area: Rect) {
    // Split into summary and detail areas
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Summary
            Constraint::Min(10),   // Key list
        ])
        .split(area);

    // Summary section
    let factor = app.precision.factor() as f32;
    let precision_str = app.precision.as_str();

    let summary = if let Some(ref triggers) = app.triggers {
        let first_press = triggers.press_travel.first().copied().unwrap_or(0);
        let first_lift = triggers.lift_travel.first().copied().unwrap_or(0);
        let first_rt_press = triggers.rt_press.first().copied().unwrap_or(0);
        let first_rt_lift = triggers.rt_lift.first().copied().unwrap_or(0);
        let first_mode = triggers.key_modes.first().copied().unwrap_or(0);
        let num_keys = triggers.key_modes.len().min(triggers.press_travel.len());

        vec![
            Line::from(vec![
                Span::styled("Precision: ", Style::default().fg(Color::Gray)),
                Span::styled(precision_str, Style::default().fg(Color::Green)),
                Span::raw("  |  "),
                Span::styled("Keys: ", Style::default().fg(Color::Gray)),
                Span::styled(format!("{num_keys}"), Style::default().fg(Color::Green)),
                Span::raw("  |  "),
                Span::styled("View: List", Style::default().fg(Color::Yellow)),
                Span::styled(" (v: layout)", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "Global Settings ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled("[g to edit all keys]", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::raw("  Actuation: "),
                Span::styled(
                    format!("{:.2}mm", first_press as f32 / factor),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw("  |  Release: "),
                Span::styled(
                    format!("{:.2}mm", first_lift as f32 / factor),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            Line::from(vec![
                Span::raw("  RT Press: "),
                Span::styled(
                    format!("{:.2}mm", first_rt_press as f32 / factor),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("   |  RT Release: "),
                Span::styled(
                    format!("{:.2}mm", first_rt_lift as f32 / factor),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(vec![
                Span::raw("  Mode: "),
                Span::styled(
                    magnetism::mode_name(first_mode),
                    Style::default().fg(Color::Magenta),
                ),
            ]),
        ]
    } else {
        vec![
            Line::from(Span::styled(
                "No trigger data loaded",
                Style::default().fg(Color::Red),
            )),
            Line::from(""),
            Line::from("Press 'r' to load trigger settings from device"),
        ]
    };

    let summary_block = Paragraph::new(summary).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Trigger Settings Summary"),
    );
    f.render_widget(summary_block, chunks[0]);

    // Key list section with ScrollView
    if let Some(ref triggers) = app.triggers {
        let num_keys = triggers.key_modes.len().min(triggers.press_travel.len());

        // Build ALL rows for table (ScrollView handles viewport)
        let selected_key = app.trigger_selected_key;
        let rows: Vec<Row> = (0..num_keys)
            .map(|i| {
                let press = triggers.press_travel.get(i).copied().unwrap_or(0);
                let lift = triggers.lift_travel.get(i).copied().unwrap_or(0);
                let rt_p = triggers.rt_press.get(i).copied().unwrap_or(0);
                let rt_l = triggers.rt_lift.get(i).copied().unwrap_or(0);
                let mode = triggers.key_modes.get(i).copied().unwrap_or(0);
                let key_name = get_key_label(app, i);
                let is_selected = i == selected_key;

                let row = Row::new(vec![
                    Cell::from(format!("{i:3}")),
                    Cell::from(if key_name.is_empty() {
                        "-".to_string()
                    } else {
                        key_name
                    }),
                    Cell::from(format!("{:.2}", press as f32 / factor)),
                    Cell::from(format!("{:.2}", lift as f32 / factor)),
                    Cell::from(format!("{:.2}", rt_p as f32 / factor)),
                    Cell::from(format!("{:.2}", rt_l as f32 / factor)),
                    Cell::from(magnetism::mode_name(mode)),
                ]);

                if is_selected {
                    row.style(Style::default().bg(Color::Blue).fg(Color::White))
                } else {
                    row
                }
            })
            .collect();

        let header = Row::new(vec!["#", "Key", "Act", "Rel", "RT↓", "RT↑", "Mode"]).style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        );

        // Render the block border first
        let block = Block::default().borders(Borders::ALL).title(format!(
            "Per-Key [{num_keys} keys] ↑↓:Select  Enter:Edit  g:Global  v:Layout"
        ));
        let inner_area = block.inner(chunks[1]);
        f.render_widget(block, chunks[1]);

        // Create table without block (rendered separately)
        let table = Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Length(7),
                Constraint::Length(6),
                Constraint::Length(6),
                Constraint::Length(6),
                Constraint::Length(6),
                Constraint::Length(14),
            ],
        )
        .header(header);

        // Use ScrollView for smooth scrolling - content height = rows + 1 for header
        let content_height = (num_keys + 1) as u16;
        let content_width = inner_area.width;
        let content_size = Size::new(content_width, content_height);

        let mut scroll_view = ScrollView::new(content_size)
            .horizontal_scrollbar_visibility(ScrollbarVisibility::Never);
        scroll_view.render_widget(table, Rect::new(0, 0, content_width, content_height));
        f.render_stateful_widget(scroll_view, inner_area, &mut app.scroll_state);
    } else {
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "Controls:",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from("  r - Reload trigger settings from device"),
            Line::from("  v - Toggle layout/list view"),
            Line::from("  ↑/↓ - Scroll through keys"),
            Line::from(""),
            Line::from(Span::styled(
                "Mode Switching (all keys):",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from("  n - Normal mode"),
            Line::from("  t - Rapid Trigger mode"),
            Line::from("  d - DKS mode"),
            Line::from("  s - SnapTap mode"),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Per-Key Settings"),
        );
        f.render_widget(help, chunks[1]);
    }
}

/// Render trigger settings as a keyboard layout view
fn render_trigger_layout(f: &mut Frame, app: &mut App, area: Rect) {
    // Split into layout area and detail area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // Keyboard layout
            Constraint::Length(9), // Selected key details
        ])
        .split(area);

    let factor = app.precision.factor() as f32;

    // Render keyboard layout
    let layout_area = chunks[0];
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Keyboard Layout [↑↓←→:Move  Enter:Edit  v:List  n/t/d/s:Mode]");
    let inner = block.inner(layout_area);

    f.render_widget(block, layout_area);

    // Calculate key cell dimensions
    // Layout is 16 main columns + nav cluster (5 more columns) = ~21 columns
    // 6 rows
    let key_width = 5u16; // Width of each key cell
    let key_height = 2u16; // Height of each key cell

    // Draw each key in the matrix (column-major order: 21 cols × 6 rows)
    for pos in 0..app.matrix_size {
        let col = pos / 6;
        let row = pos % 6;

        // Skip positions outside visible area or empty keys
        let key_name = get_key_label(app, pos);
        if key_name.is_empty() || key_name == "?" {
            continue;
        }

        // Calculate screen position
        let x = inner.x + (col as u16 * key_width);
        let y = inner.y + (row as u16 * key_height);

        // Skip if outside area
        if x + key_width > inner.x + inner.width || y + key_height > inner.y + inner.height {
            continue;
        }

        let key_rect = Rect::new(x, y, key_width, key_height);

        // Determine key style based on selection and mode
        let is_selected = pos == app.trigger_selected_key;
        let mode = app
            .triggers
            .as_ref()
            .and_then(|t| t.key_modes.get(pos).copied())
            .unwrap_or(0);

        let mode_color = match key_mode::base_mode(mode) {
            0 => Color::White,     // Normal
            0x80 => Color::Yellow, // RT
            2 => Color::Magenta,   // DKS
            7 => Color::Cyan,      // SnapTap
            _ => Color::Gray,
        };

        let style = if is_selected {
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(mode_color)
        };

        // Truncate key name to fit
        let display_name: String = key_name.chars().take(4).collect();

        // Create a mini block for each key
        let key_block = Block::default()
            .borders(Borders::ALL)
            .border_style(if is_selected {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            });

        let key_text = Paragraph::new(display_name)
            .style(style)
            .alignment(Alignment::Center)
            .block(key_block);

        f.render_widget(key_text, key_rect);
    }

    // Render selected key details
    if let Some(ref triggers) = app.triggers {
        let pos = app.trigger_selected_key;
        let key_name = get_key_label(app, pos);

        let press = triggers.press_travel.get(pos).copied().unwrap_or(0);
        let lift = triggers.lift_travel.get(pos).copied().unwrap_or(0);
        let rt_press = triggers.rt_press.get(pos).copied().unwrap_or(0);
        let rt_lift = triggers.rt_lift.get(pos).copied().unwrap_or(0);
        let mode = triggers.key_modes.get(pos).copied().unwrap_or(0);
        let bottom_dz = triggers.bottom_deadzone.get(pos).copied().unwrap_or(0);
        let top_dz = triggers.top_deadzone.get(pos).copied().unwrap_or(0);

        let details = vec![
            Line::from(vec![
                Span::styled(format!("Key {pos}: "), Style::default().fg(Color::Gray)),
                Span::styled(
                    &key_name,
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  |  Mode: "),
                Span::styled(
                    magnetism::mode_name(mode),
                    Style::default().fg(Color::Magenta),
                ),
            ]),
            Line::from(vec![
                Span::raw("Actuation: "),
                Span::styled(
                    format!("{:.2}mm", press as f32 / factor),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw("  |  Release: "),
                Span::styled(
                    format!("{:.2}mm", lift as f32 / factor),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            Line::from(vec![
                Span::raw("RT Press: "),
                Span::styled(
                    format!("{:.2}mm", rt_press as f32 / factor),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("   |  RT Release: "),
                Span::styled(
                    format!("{:.2}mm", rt_lift as f32 / factor),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(vec![
                Span::raw("Deadzone: Bottom "),
                Span::styled(
                    format!("{:.2}mm", bottom_dz as f32 / factor),
                    Style::default().fg(Color::Green),
                ),
                Span::raw("  |  Top "),
                Span::styled(
                    format!("{:.2}mm", top_dz as f32 / factor),
                    Style::default().fg(Color::Green),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "n/t/d/s: Set this key  |  N/T/D/S: Set ALL keys",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let detail_block = Paragraph::new(details).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Selected Key Details [{key_name}]")),
        );
        f.render_widget(detail_block, chunks[1]);
    } else {
        let help = Paragraph::new("Press 'r' to load trigger settings").block(
            Block::default()
                .borders(Borders::ALL)
                .title("Selected Key Details"),
        );
        f.render_widget(help, chunks[1]);
    }
}

/// Render trigger edit modal with depth chart
pub(in crate::tui) fn render_trigger_edit_modal(f: &mut Frame, app: &App, area: Rect) {
    let modal = match &app.trigger_edit_modal {
        Some(m) => m,
        None => return,
    };

    // Calculate popup size (70% width, 80% height)
    let popup_width = (area.width as f32 * 0.70).min(80.0) as u16;
    let popup_height = (area.height as f32 * 0.80).min(30.0) as u16;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the popup
    f.render_widget(Clear, popup_area);

    // Title based on target
    let title = match modal.target {
        TriggerEditTarget::Global => " Edit Global Trigger Settings ".to_string(),
        TriggerEditTarget::PerKey { key_index } => {
            let key_name = get_key_label(app, key_index);
            format!(" Edit Key {} ({}) ", key_index, key_name)
        }
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(title)
        .title_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    // Split into chart area and fields area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),    // Depth chart
            Constraint::Length(9), // Fields
            Constraint::Length(2), // Help line
        ])
        .split(inner);

    // Render depth chart
    render_modal_depth_chart(f, modal, app, chunks[0]);

    // Render editable fields
    render_modal_fields(f, modal, chunks[1]);

    // Render help line
    let help_text = "Tab/↑↓: navigate | 0-9.: edit | m: cycle mode | Enter: save | Esc: cancel";
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    f.render_widget(help, chunks[2]);
}

/// Render the depth chart within the modal
fn render_modal_depth_chart(f: &mut Frame, modal: &TriggerEditModal, app: &App, area: Rect) {
    use ratatui::widgets::{Axis, Chart, Dataset, GraphType};

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Key Depth ")
        .title_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Build depth data from history
    let depth_data: Vec<(f64, f64)> = modal
        .depth_history
        .iter()
        .enumerate()
        .map(|(i, &d)| (i as f64, d as f64))
        .collect();

    // Also get current depth for filtered key or max active key
    let current_depth = if let Some(key_idx) = modal.depth_filter {
        app.key_depths.get(key_idx).copied().unwrap_or(0.0)
    } else {
        // Show max depth across all active keys
        app.key_depths.iter().copied().fold(0.0f32, |a, b| a.max(b))
    };

    // Create threshold lines
    let max_samples = 100.0;
    let actuation_line: Vec<(f64, f64)> = vec![
        (0.0, modal.actuation_mm as f64),
        (max_samples, modal.actuation_mm as f64),
    ];
    let release_line: Vec<(f64, f64)> = vec![
        (0.0, modal.release_mm as f64),
        (max_samples, modal.release_mm as f64),
    ];
    let top_dz_line: Vec<(f64, f64)> = vec![
        (0.0, modal.top_dz_mm as f64),
        (max_samples, modal.top_dz_mm as f64),
    ];
    let bottom_dz_line: Vec<(f64, f64)> = vec![
        (0.0, (4.0 - modal.bottom_dz_mm) as f64),
        (max_samples, (4.0 - modal.bottom_dz_mm) as f64),
    ];

    let mut datasets = vec![
        // Depth trace
        Dataset::default()
            .name("Depth")
            .marker(ratatui::symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::White))
            .data(&depth_data),
        // Actuation threshold
        Dataset::default()
            .name("Act")
            .marker(ratatui::symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Yellow))
            .data(&actuation_line),
        // Release threshold
        Dataset::default()
            .name("Rel")
            .marker(ratatui::symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Cyan))
            .data(&release_line),
    ];

    // Only show deadzone lines if non-zero
    if modal.top_dz_mm > 0.01 {
        datasets.push(
            Dataset::default()
                .name("TopDZ")
                .marker(ratatui::symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(Color::Green))
                .data(&top_dz_line),
        );
    }
    if modal.bottom_dz_mm > 0.01 {
        datasets.push(
            Dataset::default()
                .name("BotDZ")
                .marker(ratatui::symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(Color::Red))
                .data(&bottom_dz_line),
        );
    }

    // Current depth indicator
    let depth_str = format!("{:.2}mm", current_depth);

    let chart = Chart::new(datasets)
        .x_axis(
            Axis::default()
                .title("Time")
                .style(Style::default().fg(Color::DarkGray))
                .bounds([0.0, max_samples]),
        )
        .y_axis(
            Axis::default()
                .title(depth_str)
                .style(Style::default().fg(Color::DarkGray))
                .labels(vec![
                    Span::raw("0"),
                    Span::raw("1"),
                    Span::raw("2"),
                    Span::raw("3"),
                    Span::raw("4"),
                ])
                .bounds([0.0, 4.0]),
        );

    f.render_widget(chart, inner);
}

/// Render the editable fields in the modal using spinner style
fn render_modal_fields(f: &mut Frame, modal: &TriggerEditModal, area: Rect) {
    let fields = TriggerField::all();
    let mut lines: Vec<Line> = Vec::new();

    for (i, field) in fields.iter().enumerate() {
        let is_selected = i == modal.field_index;
        let label = format!("{:12}", field.label());

        // Get value and unit from spinner config, or special handling for Mode
        let (value, unit) = if let Some(config) = field.spinner_config() {
            let val = match field {
                TriggerField::Actuation => modal.actuation_mm,
                TriggerField::Release => modal.release_mm,
                TriggerField::RtPress => modal.rt_press_mm,
                TriggerField::RtLift => modal.rt_lift_mm,
                TriggerField::TopDeadzone => modal.top_dz_mm,
                TriggerField::BottomDeadzone => modal.bottom_dz_mm,
                TriggerField::Mode => 0.0, // Won't reach here
            };
            (config.format(val), config.unit)
        } else {
            (magnetism::mode_name(modal.mode).to_string(), "")
        };

        // Spinner-style display: < value > when selected, just value when not
        let display_value = if is_selected {
            format!("< {} >", value)
        } else {
            format!("  {}  ", value)
        };

        let label_style = Style::default().fg(Color::Gray);
        let value_style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let unit_style = Style::default().fg(Color::DarkGray);

        let mut spans = vec![
            Span::raw("  "),
            Span::styled(label, label_style),
            Span::styled(display_value, value_style),
        ];
        if !unit.is_empty() {
            spans.push(Span::styled(format!(" {}", unit), unit_style));
        }

        lines.push(Line::from(spans));
    }

    // Add help text at bottom
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  ←/→", Style::default().fg(Color::Cyan)),
        Span::raw(" adjust  "),
        Span::styled("Shift", Style::default().fg(Color::Cyan)),
        Span::raw(" coarse  "),
        Span::styled("↑/↓", Style::default().fg(Color::Cyan)),
        Span::raw(" select  "),
        Span::styled("Enter", Style::default().fg(Color::Green)),
        Span::raw(" save  "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(" cancel"),
    ]));

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

// ============================================================================
// Private helpers
// ============================================================================

/// Get key label for display - use device profile matrix key names
fn get_key_label(app: &App, index: usize) -> String {
    app.matrix_key_names
        .get(index)
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_default()
}
