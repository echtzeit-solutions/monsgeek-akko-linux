// Triggers Tab — trigger settings, key modes, edit modal, keyboard layout view
//
// All triggers-specific types, rendering, and App methods.

use ratatui::{prelude::*, widgets::*};
use std::collections::VecDeque;
use throbber_widgets_tui::Throbber;
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::key_action::KeyAction;
use crate::keymap::Layer;
use crate::protocol::hid;
use crate::tui::widgets::PopupSelect;
use crate::TriggerSettings;
use monsgeek_keyboard::{
    DksAction, DksBinding, DksCombo, DksConfig, DksPhase, KeyMode, KeyTriggerSettings, ModeByte,
    Precision,
};

use super::super::shared::{AsyncResult, LoadState, SpinnerConfig, TriggerViewMode};
use super::super::App;
use super::depth::get_key_label;

// ============================================================================
// Types
// ============================================================================

/// Trigger edit modal target - what we're editing
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::tui) enum TriggerEditTarget {
    /// Edit global settings (applies to all keys)
    Global,
    /// Edit specific key settings
    PerKey { key_index: usize },
}

/// Editable field in trigger settings
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::tui) enum TriggerField {
    Actuation,
    Release,
    RtPress,
    RtLift,
    TopDeadzone,
    BottomDeadzone,
    Mode,
    RapidTrigger,
    ModTapTime,
    SnapTapPartner,
    DksTravel,
    DksBinding,
    DksBindingKey,
    DksAct0,
    DksAct1,
    DksAct2,
    DksAct3,
}

impl TriggerField {
    const CORE_TRAVEL: &'static [TriggerField] = &[Self::Actuation, Self::Release];
    const RT_SENSITIVITY: &'static [TriggerField] = &[Self::RtPress, Self::RtLift];
    const DEADZONES: &'static [TriggerField] = &[Self::TopDeadzone, Self::BottomDeadzone];
    const MODE_CONTROLS: &'static [TriggerField] = &[Self::Mode, Self::RapidTrigger];
    const MODTAP: &'static [TriggerField] = &[Self::ModTapTime];
    const SNAPTAP: &'static [TriggerField] = &[Self::SnapTapPartner];
    const DKS: &'static [TriggerField] = &[
        Self::DksBinding,
        Self::DksBindingKey,
        Self::DksAct0,
        Self::DksAct1,
        Self::DksAct2,
        Self::DksAct3,
    ];

    fn append_fields(out: &mut Vec<TriggerField>, slice: &'static [TriggerField]) {
        out.extend_from_slice(slice);
    }

    /// Bulk all-keys edit — only fields that `save_trigger_edit_modal` writes globally.
    fn global_fields() -> Vec<TriggerField> {
        let mut fields = Vec::new();
        Self::append_fields(&mut fields, Self::CORE_TRAVEL);
        Self::append_fields(&mut fields, Self::RT_SENSITIVITY);
        Self::append_fields(&mut fields, Self::DEADZONES);
        fields
    }

    /// Per-key fields visible for the current base mode (and RT flag).
    ///
    /// Mode / Rapid Trigger are always first so changing mode refreshes the list below.
    fn per_key_fields(mode_byte: u8) -> Vec<TriggerField> {
        let base = KeyMode::from_u8(mode_byte);
        let rt_on = mode_byte & ModeByte::RT_FLAG != 0;
        let mut fields = Vec::new();
        Self::append_fields(&mut fields, Self::MODE_CONTROLS);

        match base {
            KeyMode::Normal => {
                Self::append_fields(&mut fields, Self::CORE_TRAVEL);
                if rt_on {
                    Self::append_fields(&mut fields, Self::RT_SENSITIVITY);
                }
                Self::append_fields(&mut fields, Self::DEADZONES);
            }
            KeyMode::DynamicKeystroke => {
                Self::append_fields(&mut fields, &[Self::DksTravel, Self::Actuation]);
                if rt_on {
                    Self::append_fields(&mut fields, Self::RT_SENSITIVITY);
                }
                Self::append_fields(&mut fields, Self::DKS);
            }
            KeyMode::ModTap => {
                Self::append_fields(&mut fields, Self::MODTAP);
                if rt_on {
                    Self::append_fields(&mut fields, Self::RT_SENSITIVITY);
                }
            }
            KeyMode::SnapTap => {
                Self::append_fields(&mut fields, Self::SNAPTAP);
                if rt_on {
                    Self::append_fields(&mut fields, Self::RT_SENSITIVITY);
                }
            }
            // Toggle output is configured in the keymap; vendor hides travel/DZ for TGL.
            KeyMode::ToggleHold | KeyMode::ToggleDots => {
                if rt_on {
                    Self::append_fields(&mut fields, Self::RT_SENSITIVITY);
                }
            }
            KeyMode::Unknown(_) => {
                if rt_on {
                    Self::append_fields(&mut fields, Self::RT_SENSITIVITY);
                }
            }
        }
        fields
    }

    /// Column label, with mode-specific names where the same field means something else.
    pub(in crate::tui) fn label_for(self, mode_byte: u8) -> &'static str {
        match (self, KeyMode::from_u8(mode_byte)) {
            (Self::Actuation, KeyMode::DynamicKeystroke) => "DKS Full Depth",
            _ => self.label(),
        }
    }

    pub(in crate::tui) fn label(&self) -> &'static str {
        match self {
            Self::Actuation => "Actuation",
            Self::Release => "Release",
            Self::RtPress => "RT Press",
            Self::RtLift => "RT Lift",
            Self::TopDeadzone => "Top DZ",
            Self::BottomDeadzone => "Bottom DZ",
            Self::Mode => "Mode",
            Self::RapidTrigger => "Rapid Trig",
            Self::ModTapTime => "MT Time",
            Self::SnapTapPartner => "SnapTap Key",
            Self::DksTravel => "DKS Trigger Pt",
            Self::DksBinding => "DKS Binding",
            Self::DksBindingKey => "DKS Output",
            Self::DksAct0 => DksPhase::PressShallow.short_label(),
            Self::DksAct1 => DksPhase::PressFull.short_label(),
            Self::DksAct2 => DksPhase::ReleaseFull.short_label(),
            Self::DksAct3 => DksPhase::ReleaseShallow.short_label(),
        }
    }

    /// Get spinner configuration for this field (None for Mode which is cycled)
    pub(in crate::tui) fn spinner_config(&self) -> Option<SpinnerConfig> {
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
            Self::ModTapTime => Some(SpinnerConfig {
                min: 0.0,
                max: 2550.0,
                step: 10.0,
                step_coarse: 100.0,
                decimals: 0,
                unit: "ms",
            }),
            Self::DksTravel => Some(SpinnerConfig {
                min: 0.1,
                max: 4.0,
                step: 0.05,
                step_coarse: 0.2,
                decimals: 2,
                unit: "mm",
            }),
            // Mode / SnapTapPartner / DKS pickers open popups; RapidTrigger toggles.
            Self::Mode
            | Self::RapidTrigger
            | Self::SnapTapPartner
            | Self::DksBinding
            | Self::DksBindingKey
            | Self::DksAct0
            | Self::DksAct1
            | Self::DksAct2
            | Self::DksAct3 => None,
        }
    }

    fn dks_action_index(self) -> Option<usize> {
        match self {
            Self::DksAct0 => Some(0),
            Self::DksAct1 => Some(1),
            Self::DksAct2 => Some(2),
            Self::DksAct3 => Some(3),
            _ => None,
        }
    }
}

/// DKS fields loaded when opening a per-key trigger edit modal.
#[derive(Debug, Clone)]
pub(in crate::tui) struct DksEditState {
    pub travel_mm: f32,
    pub bindings: [DksBinding; 4],
    pub binding_keys: [Option<u8>; 4],
}

/// Per-key sub-configs fetched from the device before opening the edit modal.
#[derive(Debug, Clone)]
pub(in crate::tui) struct PerKeyEditPrefetch {
    pub modtap_ms: u16,
    pub snaptap_partner: Option<u8>,
    pub key_choices: Vec<(String, u8)>,
    pub dks: DksEditState,
}

/// Trigger edit modal state
#[derive(Debug, Clone)]
pub(in crate::tui) struct TriggerEditModal {
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
    /// Full mode byte (base mode in low 7 bits, RT flag in `0x80`)
    pub mode: u8,
    /// Mod-Tap tap-vs-hold decision time in ms (per-key only)
    pub modtap_ms: u16,
    /// Snap-Tap partner key index, if bound (per-key only)
    pub snaptap_partner: Option<u8>,
    /// `(label, key_index)` choices for the Snap-Tap partner picker
    pub key_choices: Vec<(String, u8)>,
    /// Open base-mode picker, when the user is choosing a mode
    pub mode_picker: Option<PopupSelect<KeyMode>>,
    /// Open Snap-Tap partner picker, when choosing a partner key (`None` = unbound)
    pub key_picker: Option<PopupSelect<Option<u8>>>,
    /// DKS trigger-point travel in mm (per-key only; R1/R4 shallow depth)
    pub dks_travel_mm: f32,
    /// Four DKS output bindings (per-key only)
    pub dks_bindings: [DksBinding; 4],
    /// Which DKS binding row (0–3) is being edited
    pub dks_binding_index: usize,
    /// Matrix key index bound to each binding's primary output (for the picker UI)
    pub dks_binding_keys: [Option<u8>; 4],
    /// Open DKS output-key picker for the current binding
    pub dks_key_picker: Option<PopupSelect<Option<u8>>>,
    /// Open DKS action picker: `(DksPhase index, picker)`
    pub dks_action_picker: Option<(usize, PopupSelect<DksAction>)>,
}

fn format_dks_combo(combo: DksCombo) -> String {
    [combo.skey, combo.key, combo.key2]
        .iter()
        .filter(|&&c| c != 0)
        .map(|&c| hid::key_name(c).to_string())
        .collect::<Vec<_>>()
        .join("+")
}

impl TriggerEditModal {
    /// Create modal for editing global settings
    pub(in crate::tui) fn new_global(triggers: &TriggerSettings, precision: Precision) -> Self {
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
            modtap_ms: 0,
            snaptap_partner: None,
            key_choices: Vec::new(),
            mode_picker: None,
            key_picker: None,
            dks_travel_mm: 0.0,
            dks_bindings: [DksBinding::default(); 4],
            dks_binding_index: 0,
            dks_binding_keys: [None; 4],
            dks_key_picker: None,
            dks_action_picker: None,
        }
    }

    /// Create modal for editing a specific key. `prefetch` is loaded by the
    /// caller (needs device access for Mod-Tap, Snap-Tap, and DKS).
    pub(in crate::tui) fn new_per_key(
        key_index: usize,
        triggers: &TriggerSettings,
        precision: Precision,
        prefetch: PerKeyEditPrefetch,
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
            modtap_ms: prefetch.modtap_ms,
            snaptap_partner: prefetch.snaptap_partner,
            key_choices: prefetch.key_choices,
            mode_picker: None,
            key_picker: None,
            dks_travel_mm: prefetch.dks.travel_mm,
            dks_bindings: prefetch.dks.bindings,
            dks_binding_index: 0,
            dks_binding_keys: prefetch.dks.binding_keys,
            dks_key_picker: None,
            dks_action_picker: None,
        }
    }

    /// Fields shown for this modal, filtered to what applies to the current mode.
    pub(in crate::tui) fn visible_fields(&self) -> Vec<TriggerField> {
        match self.target {
            TriggerEditTarget::Global => TriggerField::global_fields(),
            TriggerEditTarget::PerKey { .. } => TriggerField::per_key_fields(self.mode),
        }
    }

    /// Keep focus on `preferred` after the visible field list changes (e.g. mode switch).
    fn clamp_field_index(&mut self, preferred: TriggerField) {
        let fields = self.visible_fields();
        self.field_index = fields
            .iter()
            .position(|&f| f == preferred)
            .or_else(|| fields.iter().position(|&f| f == TriggerField::Mode))
            .unwrap_or(0);
    }

    pub(in crate::tui) fn current_field(&self) -> TriggerField {
        let fields = self.visible_fields();
        fields
            .get(self.field_index)
            .copied()
            .unwrap_or(TriggerField::Mode)
    }

    pub(in crate::tui) fn next_field(&mut self) {
        let len = self.visible_fields().len().max(1);
        self.field_index = (self.field_index + 1) % len;
    }

    pub(in crate::tui) fn prev_field(&mut self) {
        let len = self.visible_fields().len().max(1);
        self.field_index = if self.field_index == 0 {
            len - 1
        } else {
            self.field_index - 1
        };
    }

    /// Get the current value for the selected spinner field (0.0 for
    /// non-spinner fields, which are edited by other means).
    pub(in crate::tui) fn current_value(&self) -> f32 {
        match self.current_field() {
            TriggerField::Actuation => self.actuation_mm,
            TriggerField::Release => self.release_mm,
            TriggerField::RtPress => self.rt_press_mm,
            TriggerField::RtLift => self.rt_lift_mm,
            TriggerField::TopDeadzone => self.top_dz_mm,
            TriggerField::BottomDeadzone => self.bottom_dz_mm,
            TriggerField::ModTapTime => self.modtap_ms as f32,
            TriggerField::DksTravel => self.dks_travel_mm,
            TriggerField::Mode
            | TriggerField::RapidTrigger
            | TriggerField::SnapTapPartner
            | TriggerField::DksBinding
            | TriggerField::DksBindingKey
            | TriggerField::DksAct0
            | TriggerField::DksAct1
            | TriggerField::DksAct2
            | TriggerField::DksAct3 => 0.0,
        }
    }

    /// Set the value for the selected spinner field.
    pub(in crate::tui) fn set_current_value(&mut self, value: f32) {
        match self.current_field() {
            TriggerField::Actuation => self.actuation_mm = value,
            TriggerField::Release => self.release_mm = value,
            TriggerField::RtPress => self.rt_press_mm = value,
            TriggerField::RtLift => self.rt_lift_mm = value,
            TriggerField::TopDeadzone => self.top_dz_mm = value,
            TriggerField::BottomDeadzone => self.bottom_dz_mm = value,
            TriggerField::ModTapTime => self.modtap_ms = value.clamp(0.0, 2550.0) as u16,
            TriggerField::DksTravel => self.dks_travel_mm = value,
            TriggerField::Mode
            | TriggerField::RapidTrigger
            | TriggerField::SnapTapPartner
            | TriggerField::DksBinding
            | TriggerField::DksBindingKey
            | TriggerField::DksAct0
            | TriggerField::DksAct1
            | TriggerField::DksAct2
            | TriggerField::DksAct3 => {}
        }
    }

    /// Increment the current field: spinner up, or open a picker / flip the RT
    /// flag for the non-spinner fields.
    pub(in crate::tui) fn increment_current(&mut self, coarse: bool) {
        match self.current_field() {
            TriggerField::Mode => self.open_mode_picker(),
            TriggerField::SnapTapPartner => self.open_key_picker(),
            TriggerField::DksBindingKey => self.open_dks_key_picker(),
            TriggerField::DksBinding => self.cycle_dks_binding(true),
            field if field.dks_action_index().is_some() => {
                self.open_dks_action_picker(field.dks_action_index().unwrap());
            }
            TriggerField::RapidTrigger => self.toggle_rapid_trigger(),
            field => {
                if let Some(config) = field.spinner_config() {
                    let new_value = config.increment(self.current_value(), coarse);
                    self.set_current_value(new_value);
                }
            }
        }
    }

    /// Decrement the current field: spinner down, or open a picker / flip the RT
    /// flag for the non-spinner fields.
    pub(in crate::tui) fn decrement_current(&mut self, coarse: bool) {
        match self.current_field() {
            TriggerField::Mode => self.open_mode_picker(),
            TriggerField::SnapTapPartner => self.open_key_picker(),
            TriggerField::DksBindingKey => self.open_dks_key_picker(),
            TriggerField::DksBinding => self.cycle_dks_binding(false),
            field if field.dks_action_index().is_some() => {
                self.open_dks_action_picker(field.dks_action_index().unwrap());
            }
            TriggerField::RapidTrigger => self.toggle_rapid_trigger(),
            field => {
                if let Some(config) = field.spinner_config() {
                    let new_value = config.decrement(self.current_value(), coarse);
                    self.set_current_value(new_value);
                }
            }
        }
    }

    fn cycle_dks_binding(&mut self, forward: bool) {
        if forward {
            self.dks_binding_index = (self.dks_binding_index + 1) % 4;
        } else {
            self.dks_binding_index = (self.dks_binding_index + 3) % 4;
        }
    }

    /// Flip the Rapid-Trigger (`0x80`) flag, preserving the base mode.
    pub(in crate::tui) fn toggle_rapid_trigger(&mut self) {
        let preferred = TriggerField::RapidTrigger;
        self.mode ^= ModeByte::RT_FLAG;
        self.clamp_field_index(preferred);
    }

    /// Open the base-mode popup selector, preselected to the current base mode.
    pub(in crate::tui) fn open_mode_picker(&mut self) {
        let current = KeyMode::from_u8(self.mode);
        let items: Vec<(String, KeyMode)> = KeyMode::ALL
            .iter()
            .map(|&m| (m.label().to_string(), m))
            .collect();
        let mut picker = PopupSelect::new("Mode", items);
        picker.select_where(|&m| m == current);
        self.mode_picker = Some(picker);
    }

    /// Apply the picker's selection to the base mode (keeping the RT flag) and
    /// close it.
    pub(in crate::tui) fn confirm_mode_picker(&mut self) {
        if let Some(picker) = self.mode_picker.take() {
            if let Some(&base) = picker.selected() {
                let rt = self.mode & ModeByte::RT_FLAG != 0;
                let preferred = self.current_field();
                self.mode = ModeByte::new(base, rt).to_u8();
                self.clamp_field_index(preferred);
            }
        }
    }

    /// Open the Snap-Tap partner picker, preselected to the current partner.
    pub(in crate::tui) fn open_key_picker(&mut self) {
        let mut items = vec![("(none)".to_string(), None)];
        items.extend(self.key_choices.iter().map(|(l, i)| (l.clone(), Some(*i))));
        let mut picker = PopupSelect::new("SnapTap partner", items);
        let current = self.snaptap_partner;
        picker.select_where(|&p| p == current);
        self.key_picker = Some(picker);
    }

    /// Apply the partner-picker selection and close it.
    pub(in crate::tui) fn confirm_key_picker(&mut self) {
        if let Some(picker) = self.key_picker.take() {
            if let Some(&partner) = picker.selected() {
                self.snaptap_partner = partner;
            }
        }
    }

    /// Open the DKS output-key picker for the current slot.
    pub(in crate::tui) fn open_dks_key_picker(&mut self) {
        let mut items = vec![("(none)".to_string(), None)];
        items.extend(self.key_choices.iter().map(|(l, i)| (l.clone(), Some(*i))));
        let mut picker = PopupSelect::new(
            format!("DKS binding {} output", self.dks_binding_index + 1),
            items,
        );
        let current = self.dks_binding_keys[self.dks_binding_index];
        picker.select_where(|&p| p == current);
        self.dks_key_picker = Some(picker);
    }

    /// Open the DKS action picker for one travel checkpoint on the current slot.
    pub(in crate::tui) fn open_dks_action_picker(&mut self, action_idx: usize) {
        let current = self.dks_bindings[self.dks_binding_index].phase_actions[action_idx];
        let items: Vec<(String, DksAction)> = [
            DksAction::None,
            DksAction::SingleTrigger,
            DksAction::ContinuousUntilNext,
            DksAction::ContinuousAcross,
        ]
        .into_iter()
        .map(|a| (a.to_string(), a))
        .collect();
        let phase = DksPhase::from_index(action_idx).unwrap_or(DksPhase::PressShallow);
        let mut picker = PopupSelect::new(
            format!(
                "DKS binding {} {}",
                self.dks_binding_index + 1,
                phase.short_label()
            ),
            items,
        );
        picker.select_where(|&a| a == current);
        self.dks_action_picker = Some((action_idx, picker));
    }

    /// Apply the DKS action-picker selection and close it.
    pub(in crate::tui) fn confirm_dks_action_picker(&mut self) {
        if let Some((idx, picker)) = self.dks_action_picker.take() {
            if let Some(&action) = picker.selected() {
                self.dks_bindings[self.dks_binding_index].phase_actions[idx] = action;
            }
        }
    }

    /// Add a depth sample to history
    pub(in crate::tui) fn push_depth(&mut self, depth_mm: f32) {
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
        let Some(keyboard) = self.keyboard.as_ref() else {
            self.status_msg = "No keyboard connected".to_string();
            return;
        };
        match keyboard.set_mode_all(ModeByte::from_u8(mode)) {
            Ok(()) => {
                self.status_msg = format!("Set all keys to {}", ModeByte::from_u8(mode));
                self.load_triggers();
            }
            Err(e) => self.status_msg = format!("Failed to set mode: {e}"),
        }
    }

    /// Set mode for a single key (used in layout view). The single-key protocol
    /// bundles actuation/release with the mode, so re-send the key's current
    /// travel (converted from stored precision units to 0.1 mm) alongside.
    pub(in crate::tui) fn set_single_key_mode(&mut self, key_index: usize, mode: u8) {
        let Some(triggers) = self.triggers.as_ref() else {
            self.status_msg = "Triggers not loaded".to_string();
            return;
        };
        if key_index >= triggers.key_modes.len() {
            self.status_msg = format!("Invalid key index: {key_index}");
            return;
        }
        let factor = self.precision.factor() as f32;
        let to_u8_mm = |raw: u16| ((raw as f32 / factor) * 10.0) as u8;
        let mode_byte = ModeByte::from_u8(mode);
        let settings = KeyTriggerSettings {
            key_index: key_index as u8,
            actuation: to_u8_mm(triggers.press_travel.get(key_index).copied().unwrap_or(0)),
            deactuation: to_u8_mm(triggers.lift_travel.get(key_index).copied().unwrap_or(0)),
            mode: mode_byte.base,
            rapid_trigger: mode_byte.rapid_trigger,
        };
        let Some(keyboard) = self.keyboard.as_ref() else {
            self.status_msg = "No keyboard connected".to_string();
            return;
        };
        if let Err(e) = keyboard.set_key_trigger(&settings) {
            self.status_msg = format!("Failed to set key mode: {e}");
            return;
        }
        self.load_triggers();
        let key_name = get_key_label(self, key_index);
        self.status_msg = format!(
            "Key {} ({}) set to {}",
            key_index,
            key_name,
            ModeByte::from_u8(mode)
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

    /// Build `(label, key_index)` choices for the Snap-Tap partner picker,
    /// covering every named key.
    fn key_choices(&self) -> Vec<(String, u8)> {
        let count = self
            .triggers
            .as_ref()
            .map(|t| t.key_modes.len())
            .unwrap_or(0)
            .min(u8::MAX as usize);
        (0..count)
            .filter_map(|i| {
                let name = get_key_label(self, i);
                (!name.is_empty()).then_some((name, i as u8))
            })
            .collect()
    }

    /// Default/base-layer HID code a matrix key would emit (remap-aware).
    pub(in crate::tui) fn key_output_hid(&self, key_index: u8) -> u8 {
        for entry in &self.remaps {
            if entry.index == key_index && entry.layer == Layer::Base {
                return match entry.action {
                    KeyAction::Key(code) => code,
                    KeyAction::Combo { key, .. } => key,
                    _ => continue,
                };
            }
        }
        hid::key_code_from_name(monsgeek_transport::protocol::matrix::key_name(key_index))
            .unwrap_or(0)
    }

    /// Best-effort reverse lookup: which matrix key emits the combo's primary HID.
    fn dks_binding_key_for_combo(
        &self,
        combo: DksCombo,
        key_choices: &[(String, u8)],
    ) -> Option<u8> {
        let hid_code = combo.key.max(combo.skey).max(combo.key2);
        if hid_code == 0 {
            return None;
        }
        key_choices
            .iter()
            .map(|(_, i)| *i)
            .find(|&idx| self.key_output_hid(idx) == hid_code)
    }

    /// Open trigger edit modal for a specific key
    pub(in crate::tui) fn open_trigger_edit_key(&mut self, key_index: usize) {
        if self.triggers.is_none() {
            self.status_msg = "No trigger data loaded".to_string();
            return;
        }
        // Best-effort fetch of the per-key sub-configs (Mod-Tap time, Snap-Tap
        // partner) that live outside the bulk trigger snapshot.
        let (modtap_ms, snaptap_partner) = match self.keyboard.as_ref() {
            Some(kb) => {
                let mt = kb
                    .get_modtap_times()
                    .ok()
                    .and_then(|v| v.get(key_index).copied())
                    .unwrap_or(0);
                let sp = kb
                    .get_snaptap_binds()
                    .ok()
                    .and_then(|v| v.get(key_index).copied())
                    .filter(|&p| p != monsgeek_keyboard::SNAPTAP_UNBOUND);
                (mt, sp)
            }
            None => (0, None),
        };
        let key_choices = self.key_choices();
        let factor = self.precision.factor() as f32;
        let dks = match self.keyboard.as_ref() {
            Some(kb) => kb
                .get_dks_config(key_index as u8)
                .map(|cfg| {
                    let mut binding_keys = [None; 4];
                    for (i, binding) in cfg.bindings.iter().enumerate() {
                        binding_keys[i] =
                            self.dks_binding_key_for_combo(binding.combo, &key_choices);
                    }
                    DksEditState {
                        travel_mm: cfg.trigger_point_travel_raw as f32 / factor,
                        bindings: cfg.bindings,
                        binding_keys,
                    }
                })
                .unwrap_or(DksEditState {
                    travel_mm: 0.7,
                    bindings: [DksBinding::default(); 4],
                    binding_keys: [None; 4],
                }),
            None => DksEditState {
                travel_mm: 0.7,
                bindings: [DksBinding::default(); 4],
                binding_keys: [None; 4],
            },
        };
        if let Some(ref triggers) = self.triggers {
            let modal = TriggerEditModal::new_per_key(
                key_index,
                triggers,
                self.precision,
                PerKeyEditPrefetch {
                    modtap_ms,
                    snaptap_partner,
                    key_choices,
                    dks,
                },
            );
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
                let mode_byte = ModeByte::from_u8(modal.mode);
                let settings = KeyTriggerSettings {
                    key_index: key_index as u8,
                    actuation: (modal.actuation_mm * 10.0) as u8,
                    deactuation: (modal.release_mm * 10.0) as u8,
                    mode: mode_byte.base,
                    rapid_trigger: mode_byte.rapid_trigger,
                };

                match keyboard.set_key_trigger(&settings) {
                    Ok(()) => {
                        // Apply the mode-specific sub-configs alongside the base
                        // trigger. Mod-Tap time is only meaningful in Mod-Tap
                        // mode; Snap-Tap pairing only in Snap-Tap mode.
                        let key = key_index as u8;
                        let mut extra = Vec::new();
                        if mode_byte.base == KeyMode::ModTap {
                            if let Err(e) = keyboard.set_modtap_time(key, modal.modtap_ms) {
                                extra.push(format!("mt_time: {e}"));
                            }
                        }
                        if mode_byte.base == KeyMode::SnapTap {
                            let res = match modal.snaptap_partner {
                                Some(partner) => keyboard.set_snaptap_pair(key, partner),
                                None => keyboard.clear_snaptap(key),
                            };
                            if let Err(e) = res {
                                extra.push(format!("snaptap: {e}"));
                            }
                        }
                        if mode_byte.base == KeyMode::DynamicKeystroke {
                            let travel_raw = (modal.dks_travel_mm * factor) as u16;
                            let config = DksConfig {
                                trigger_point_travel_raw: travel_raw,
                                bindings: modal.dks_bindings,
                            };
                            if let Err(e) =
                                keyboard.set_dks_config(key, &config, Some(mode_byte.rapid_trigger))
                            {
                                extra.push(format!("dks: {e}"));
                            }
                        }
                        let key_name = get_key_label(self, key_index);
                        self.status_msg = if extra.is_empty() {
                            format!(
                                "Key {} ({}) saved: act={:.1}mm rel={:.1}mm mode={}",
                                key_index,
                                key_name,
                                modal.actuation_mm,
                                modal.release_mm,
                                ModeByte::new(settings.mode, settings.rapid_trigger),
                            )
                        } else {
                            format!("Key {key_index} saved with errors: {}", extra.join(", "))
                        };
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
                    ModeByte::from_u8(first_mode).to_string(),
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
                    Cell::from(ModeByte::from_u8(mode).to_string()),
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

        let parsed = ModeByte::from_u8(mode);
        let mode_color = if parsed.rapid_trigger && parsed.base == KeyMode::Normal {
            Color::Yellow // plain Rapid Trigger
        } else {
            match parsed.base {
                KeyMode::Normal => Color::White,
                KeyMode::DynamicKeystroke => Color::Magenta,
                KeyMode::ModTap => Color::Green,
                KeyMode::ToggleHold | KeyMode::ToggleDots => Color::Blue,
                KeyMode::SnapTap => Color::Cyan,
                KeyMode::Unknown(_) => Color::Gray,
            }
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
                    ModeByte::from_u8(mode).to_string(),
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
    let popup_height = (area.height as f32 * 0.85).min(38.0) as u16;
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
            Constraint::Min(5),    // Depth chart
            Constraint::Min(18),   // Fields (per-key modal has many rows)
            Constraint::Length(2), // Help line
        ])
        .split(inner);

    // Render depth chart
    render_modal_depth_chart(f, modal, app, chunks[0]);

    // Render editable fields
    render_modal_fields(f, modal, chunks[1]);

    // Render help line
    let help_text = "Tab/↑↓: field | ←/→: adjust/toggle | Enter: save | Esc: cancel";
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    f.render_widget(help, chunks[2]);

    // Overlay a picker if open. The renderer only has `&App`, so clone the small
    // picker to satisfy the stateful-widget `&mut` requirement.
    if let Some(picker) = &modal.mode_picker {
        picker.clone().render(f, popup_area);
    } else if let Some(picker) = &modal.key_picker {
        picker.clone().render(f, popup_area);
    } else if let Some(picker) = &modal.dks_key_picker {
        picker.clone().render(f, popup_area);
    } else if let Some((_, picker)) = &modal.dks_action_picker {
        picker.clone().render(f, popup_area);
    }
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
    let fields = modal.visible_fields();
    let mut lines: Vec<Line> = Vec::new();

    for (i, field) in fields.iter().enumerate() {
        let is_selected = i == modal.field_index;
        let label = format!("{:12}", field.label_for(modal.mode));

        // Get value and unit from spinner config, or special handling for the
        // Mode (base name), Rapid-Trigger (on/off) and Snap-Tap partner fields.
        let (value, unit) = match field {
            TriggerField::Mode => (KeyMode::from_u8(modal.mode).label().to_string(), ""),
            TriggerField::RapidTrigger => {
                let on = modal.mode & ModeByte::RT_FLAG != 0;
                ((if on { "On" } else { "Off" }).to_string(), "")
            }
            TriggerField::SnapTapPartner => {
                let label = match modal.snaptap_partner {
                    Some(idx) => modal
                        .key_choices
                        .iter()
                        .find(|(_, i)| *i == idx)
                        .map(|(l, _)| l.clone())
                        .unwrap_or_else(|| format!("key {idx}")),
                    None => "(none)".to_string(),
                };
                (label, "")
            }
            TriggerField::DksBinding => (format!("{} / 4", modal.dks_binding_index + 1), ""),
            TriggerField::DksBindingKey => {
                let binding = modal.dks_binding_index;
                let label = if let Some(idx) = modal.dks_binding_keys[binding] {
                    modal
                        .key_choices
                        .iter()
                        .find(|(_, i)| *i == idx)
                        .map(|(l, _)| l.clone())
                        .unwrap_or_else(|| format_dks_combo(modal.dks_bindings[binding].combo))
                } else if modal.dks_bindings[binding].combo.is_empty() {
                    "(none)".to_string()
                } else {
                    format_dks_combo(modal.dks_bindings[binding].combo)
                };
                (label, "")
            }
            TriggerField::DksAct0
            | TriggerField::DksAct1
            | TriggerField::DksAct2
            | TriggerField::DksAct3 => {
                let idx = field.dks_action_index().unwrap();
                (
                    modal.dks_bindings[modal.dks_binding_index].phase_actions[idx].to_string(),
                    "",
                )
            }
            _ => {
                let config = field.spinner_config().expect("spinner field");
                let val = match field {
                    TriggerField::Actuation => modal.actuation_mm,
                    TriggerField::Release => modal.release_mm,
                    TriggerField::RtPress => modal.rt_press_mm,
                    TriggerField::RtLift => modal.rt_lift_mm,
                    TriggerField::TopDeadzone => modal.top_dz_mm,
                    TriggerField::BottomDeadzone => modal.bottom_dz_mm,
                    TriggerField::ModTapTime => modal.modtap_ms as f32,
                    TriggerField::DksTravel => modal.dks_travel_mm,
                    TriggerField::Mode
                    | TriggerField::RapidTrigger
                    | TriggerField::SnapTapPartner
                    | TriggerField::DksBinding
                    | TriggerField::DksBindingKey
                    | TriggerField::DksAct0
                    | TriggerField::DksAct1
                    | TriggerField::DksAct2
                    | TriggerField::DksAct3 => unreachable!(),
                };
                (config.format(val), config.unit)
            }
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
