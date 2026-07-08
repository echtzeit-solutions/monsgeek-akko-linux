// Triggers Tab — trigger settings, key modes, edit modal, keyboard layout view
//
// All triggers-specific types, rendering, and App methods.

use ratatui::{prelude::*, widgets::*};
use std::collections::VecDeque;

use crate::key_action::KeyAction;
use crate::keymap::Layer;
use crate::protocol::hid;
use crate::tui::widgets::PopupSelect;
use crate::TriggerSettings;
use monsgeek_keyboard::{
    DksAction, DksBinding, DksCombo, DksConfig, DksPhase, KeyMode, KeyTriggerSettings, ModeByte,
    Precision,
};

use super::super::shared::{AsyncResult, LoadState, SpinnerConfig};
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
    OutputLayer,
    Output,
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
    /// The key's emitted output (keymatrix layer 0). Shown for every mode except
    /// DKS, where the four combo slots below are the output.
    const OUTPUT: &'static [TriggerField] = &[Self::OutputLayer, Self::Output];
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
        // The output binding (keymatrix layer 0). In DKS mode the combo slots below
        // are the output, so it's omitted there.
        if base != KeyMode::DynamicKeystroke {
            Self::append_fields(&mut fields, Self::OUTPUT);
        }

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
            Self::OutputLayer => "Layer",
            Self::Output => "Output",
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
            | Self::OutputLayer
            | Self::Output
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
    /// Current output per layer `[Base, Layer1, Fn]`.
    pub outputs: [KeyAction; 3],
    /// Matrix position whose default keycode equals each layer's output (picker preselect).
    pub output_keys: [Option<u8>; 3],
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
    /// The key's emitted output per layer `[Base, Layer1, Fn]` (non-DKS modes).
    pub outputs: [KeyAction; 3],
    /// Matrix position whose default keycode was chosen per layer (for the picker).
    pub output_keys: [Option<u8>; 3],
    /// Snapshot of `outputs` at open, so save only writes layers that changed.
    pub outputs_orig: [KeyAction; 3],
    /// Which output layer the `Output`/`Layer` fields target: 0=Base, 1=Layer1, 2=Fn.
    pub output_layer: usize,
    /// Open output-key picker.
    pub output_picker: Option<PopupSelect<Option<u8>>>,
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
            outputs: [KeyAction::Disabled; 3],
            output_keys: [None; 3],
            outputs_orig: [KeyAction::Disabled; 3],
            output_layer: 0,
            output_picker: None,
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
            outputs: prefetch.outputs,
            output_keys: prefetch.output_keys,
            outputs_orig: prefetch.outputs,
            output_layer: 0,
            output_picker: None,
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
            | TriggerField::OutputLayer
            | TriggerField::Output
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
            | TriggerField::OutputLayer
            | TriggerField::Output
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
            TriggerField::OutputLayer => self.cycle_output_layer(true),
            TriggerField::Output => self.open_output_picker(),
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
            TriggerField::OutputLayer => self.cycle_output_layer(false),
            TriggerField::Output => self.open_output_picker(),
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

    /// Names of the three output layers, indexed by `output_layer`.
    pub(in crate::tui) fn output_layer_name(&self) -> &'static str {
        ["Base", "Layer1", "Fn"][self.output_layer.min(2)]
    }

    /// Cycle the output-layer selector (Base → Layer1 → Fn → Base).
    pub(in crate::tui) fn cycle_output_layer(&mut self, forward: bool) {
        self.output_layer = if forward {
            (self.output_layer + 1) % 3
        } else {
            (self.output_layer + 2) % 3
        };
    }

    /// Open the output-key picker for the current output layer. "(none)" is offered
    /// on the overlay layers (Layer1 / Fn) — they're transparent when empty — but
    /// never on Base, where an empty entry would silence the key.
    pub(in crate::tui) fn open_output_picker(&mut self) {
        let mut items: Vec<(String, Option<u8>)> = Vec::new();
        if self.output_layer != 0 {
            items.push(("(none)".to_string(), None));
        }
        items.extend(self.key_choices.iter().map(|(l, i)| (l.clone(), Some(*i))));
        let mut picker = PopupSelect::new(format!("{} output", self.output_layer_name()), items);
        let current = self.output_keys[self.output_layer];
        picker.select_where(|&p| p == current);
        self.output_picker = Some(picker);
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
        // Current output per layer [Base, Layer1, Fn]. Base/Layer1 come from
        // keymatrix layers 0/1; Fn from the separate Fn table.
        let resolve = |bytes: [u8; 4]| {
            let action = KeyAction::from_config_bytes(bytes);
            let hid = match action {
                KeyAction::Key(c) => c,
                KeyAction::Combo { key, .. } => key,
                _ => 0,
            };
            let key = key_choices
                .iter()
                .map(|(_, i)| *i)
                .find(|&idx| self.key_output_hid(idx) == hid);
            (action, key)
        };
        let kb = self.keyboard.as_ref();
        let base_bytes = kb
            .and_then(|kb| kb.get_key_config_at_layer(0, 0, key_index as u8).ok())
            .unwrap_or([0; 4]);
        let l1_bytes = kb
            .and_then(|kb| kb.get_key_config_at_layer(0, 1, key_index as u8).ok())
            .unwrap_or([0; 4]);
        let fn_bytes = kb
            .and_then(|kb| kb.get_fn_keymatrix(0, 0, 8).ok())
            .and_then(|m| {
                m.get(key_index * 4..key_index * 4 + 4)
                    .map(|s| [s[0], s[1], s[2], s[3]])
            })
            .unwrap_or([0; 4]);
        let (o0, k0) = resolve(base_bytes);
        let (o1, k1) = resolve(l1_bytes);
        let (o2, k2) = resolve(fn_bytes);
        let outputs = [o0, o1, o2];
        let output_keys = [k0, k1, k2];
        if let Some(ref triggers) = self.triggers {
            let modal = TriggerEditModal::new_per_key(
                key_index,
                triggers,
                self.precision,
                PerKeyEditPrefetch {
                    modtap_ms,
                    snaptap_partner,
                    key_choices,
                    outputs,
                    output_keys,
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
                    if self.loading.key_mapping != LoadState::NotLoaded {
                        self.load_key_mapping();
                    }
                } else {
                    self.status_msg = format!("Errors: {}", errors.join(", "));
                }
            }
            TriggerEditTarget::PerKey { key_index } => {
                // Per-key uses the same u16 precision as the bulk table.
                let mode_byte = ModeByte::from_u8(modal.mode);
                let settings = KeyTriggerSettings {
                    key_index: key_index as u8,
                    actuation: (modal.actuation_mm * factor) as u16,
                    deactuation: (modal.release_mm * factor) as u16,
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
                        } else {
                            // Per-layer output bindings (non-DKS). Write only layers that
                            // changed. Base (0) must never be all-zero — that silences the
                            // key — and Base/Layer1 (keymatrix) commit via the settling
                            // combo path; Fn goes to the separate Fn table (SET_FN). The
                            // overlay layers treat an empty entry as transparent.
                            for layer in 0..3usize {
                                if modal.outputs[layer] == modal.outputs_orig[layer] {
                                    continue;
                                }
                                let bytes = modal.outputs[layer].to_config_bytes();
                                if layer == 0 && bytes == [0, 0, 0, 0] {
                                    continue;
                                }
                                let res = if layer < 2 {
                                    match DksCombo::from_config_bytes(bytes) {
                                        Some(combo) => keyboard.set_dks_combo_binding(
                                            0,
                                            key,
                                            layer as u8,
                                            combo,
                                            true,
                                        ),
                                        None => keyboard.set_key_config(0, key, layer as u8, bytes),
                                    }
                                } else {
                                    keyboard.set_key_config(0, key, 2, bytes)
                                };
                                if let Err(e) = res {
                                    extra.push(format!("output L{layer}: {e}"));
                                }
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
                        if self.loading.key_mapping != LoadState::NotLoaded {
                            self.load_key_mapping();
                        }
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
    } else if let Some(picker) = &modal.output_picker {
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
            TriggerField::OutputLayer => (modal.output_layer_name().to_string(), ""),
            TriggerField::Output => (modal.outputs[modal.output_layer].to_string(), ""),
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
                    | TriggerField::OutputLayer
                    | TriggerField::Output
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
