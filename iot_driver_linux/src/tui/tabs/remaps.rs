// Remaps Tab — binding editor, layer filter, macro events
//
// All remaps-specific types, rendering, and helpers.

use ratatui::{prelude::*, widgets::*};
use throbber_widgets_tui::Throbber;

use crate::key_action::KeyAction;
use crate::keymap::Layer;
use crate::protocol::hid::{key_name, keycode_to_char as hid_keycode_to_char};

use super::super::shared::{LoadState, MacroEvent, MacroSlot};
use super::super::App;

// ============================================================================
// Types
// ============================================================================

/// Layer filter for the Remaps tab.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(in crate::tui) enum RemapLayerView {
    #[default]
    Both,
    L0,
    L1,
    Fn,
}

impl RemapLayerView {
    pub fn cycle(self) -> Self {
        match self {
            Self::Both => Self::L0,
            Self::L0 => Self::L1,
            Self::L1 => Self::Fn,
            Self::Fn => Self::Both,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Both => "All",
            Self::L0 => "L0",
            Self::L1 => "L1",
            Self::Fn => "Fn",
        }
    }

    pub fn matches(self, layer: Layer) -> bool {
        match self {
            Self::Both => true,
            Self::L0 => layer == Layer::Base,
            Self::L1 => layer == Layer::Layer1,
            Self::Fn => layer == Layer::Fn,
        }
    }
}

/// Which binding type is selected in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tui) enum BindingType {
    Disabled,
    Key,
    Combo,
    Mouse,
    Consumer,
    Macro,
    Gamepad,
    Fn,
    LedControl,
}

impl BindingType {
    pub const ALL: &[BindingType] = &[
        BindingType::Disabled,
        BindingType::Key,
        BindingType::Combo,
        BindingType::Mouse,
        BindingType::Consumer,
        BindingType::Macro,
        BindingType::Gamepad,
        BindingType::Fn,
        BindingType::LedControl,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Key => "Key",
            Self::Combo => "Modifier Combo",
            Self::Mouse => "Mouse",
            Self::Consumer => "Media/Consumer",
            Self::Macro => "Macro",
            Self::Gamepad => "Gamepad",
            Self::Fn => "Fn",
            Self::LedControl => "LED Control",
        }
    }

    pub fn from_action(action: &KeyAction) -> Self {
        match action {
            KeyAction::Disabled => Self::Disabled,
            KeyAction::Key(_) => Self::Key,
            KeyAction::Combo { .. } => Self::Combo,
            KeyAction::Mouse(_) => Self::Mouse,
            KeyAction::Consumer(_) => Self::Consumer,
            KeyAction::Macro { .. } => Self::Macro,
            KeyAction::Gamepad(_) => Self::Gamepad,
            KeyAction::Fn => Self::Fn,
            KeyAction::LedControl { .. } => Self::LedControl,
            KeyAction::SpecialFn { .. }
            | KeyAction::ProfileSwitch { .. }
            | KeyAction::ConnectionMode { .. }
            | KeyAction::Knob { .. }
            | KeyAction::Unknown { .. } => Self::Disabled,
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }
}

/// Which field is focused in the binding editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tui) enum BindingField {
    Type,
    Filter,
    KeyList,
    Mods,
    MacroSlot,
    MacroKind,
    MacroText,
    MacroEvents,
    Value,
}

/// Which part of a macro event is being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tui) enum MacroEventField {
    Action,
    Key,
    Delay,
}

/// Focus model for the remaps tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::tui) enum RemapFocus {
    #[default]
    List,
    Editor,
}

// ============================================================================
// Constants
// ============================================================================

pub(in crate::tui) const CONSUMER_KEYS: &[(u16, &str)] = &[
    (0x00B5, "Next Track"),
    (0x00B6, "Previous Track"),
    (0x00B7, "Stop"),
    (0x00CD, "Play/Pause"),
    (0x00E2, "Mute"),
    (0x00E9, "Volume Up"),
    (0x00EA, "Volume Down"),
    (0x006F, "Brightness Up"),
    (0x0070, "Brightness Down"),
    (0x018A, "Mail"),
    (0x0192, "Calculator"),
    (0x0194, "My Computer"),
    (0x0221, "Search"),
    (0x0223, "Browser Home"),
];

pub(in crate::tui) const LED_CONTROLS: &[([u8; 3], &str)] = &[
    ([2, 1, 0], "Brightness Up"),
    ([2, 2, 0], "Brightness Down"),
    ([3, 1, 0], "Speed Up"),
    ([3, 2, 0], "Speed Down"),
];

pub(in crate::tui) const MODIFIER_LIST: &[(u8, &str)] = &[
    (crate::key_action::mods::LCTRL, "LCtrl"),
    (crate::key_action::mods::LSHIFT, "LShift"),
    (crate::key_action::mods::LALT, "LAlt"),
    (crate::key_action::mods::LGUI, "LGUI"),
    (crate::key_action::mods::RCTRL, "RCtrl"),
    (crate::key_action::mods::RSHIFT, "RShift"),
    (crate::key_action::mods::RALT, "RAlt"),
    (crate::key_action::mods::RGUI, "RGUI"),
];

// ============================================================================
// Helpers
// ============================================================================

pub(in crate::tui) fn all_hid_keys() -> Vec<(u8, &'static str)> {
    let mut keys = Vec::new();
    for code in 0x04..=0x73u8 {
        let name = key_name(code);
        if name == "?" || name == "F13-F24" {
            continue;
        }
        keys.push((code, name));
    }
    // F13-F24 individually
    for n in 13..=24u8 {
        let code = 0x68 + (n - 13);
        let name = match n {
            13 => "F13",
            14 => "F14",
            15 => "F15",
            16 => "F16",
            17 => "F17",
            18 => "F18",
            19 => "F19",
            20 => "F20",
            21 => "F21",
            22 => "F22",
            23 => "F23",
            24 => "F24",
            _ => unreachable!(),
        };
        keys.push((code, name));
    }
    // Modifier keys
    for code in 0xE0..=0xE7u8 {
        let name = key_name(code);
        if name != "?" {
            keys.push((code, name));
        }
    }
    keys.sort_by_key(|&(_, name)| name);
    keys
}

/// Try to reconstruct typed text from macro events
pub(in crate::tui) fn text_preview_from_events(events: &[monsgeek_keyboard::MacroEvent]) -> String {
    let mut result = String::new();
    let mut shift_held = false;

    for evt in events {
        // Track shift state
        if evt.keycode == 0xE1 || evt.keycode == 0xE5 {
            shift_held = evt.is_down;
            continue;
        }

        // Only process key-down events
        if !evt.is_down {
            continue;
        }

        if let Some(c) = hid_keycode_to_char(evt.keycode, shift_held) {
            result.push(c);
        }
    }

    if result.is_empty() && !events.is_empty() {
        format!("{} events", events.len())
    } else {
        result
    }
}

// ============================================================================
// BindingEditor
// ============================================================================

/// Inline binding editor (always visible in the right panel when a key is selected).
pub(in crate::tui) struct BindingEditor {
    pub binding_type: BindingType,
    pub field: BindingField,
    // Key type
    pub key_list_index: usize,
    pub key_filter: String,
    // Combo type
    pub combo_key_index: usize,
    pub combo_mods: u8,
    pub combo_mod_cursor: usize,
    pub combo_key_filter: String,
    // Mouse
    pub mouse_button: u8,
    // Consumer
    pub consumer_index: usize,
    pub consumer_filter: String,
    // Macro
    pub macro_slot: u8,
    pub macro_kind: u8,
    pub macro_text: String,
    pub macro_events: Vec<MacroEvent>,
    pub macro_event_cursor: usize,
    pub macro_event_field: MacroEventField,
    pub macro_repeat: u16,
    // Gamepad
    pub gamepad_button: u8,
    // LED Control
    pub led_control_index: usize,
    // State
    pub dirty: bool,
}

impl BindingEditor {
    pub fn new() -> Self {
        Self {
            binding_type: BindingType::Disabled,
            field: BindingField::Type,
            key_list_index: 0,
            key_filter: String::new(),
            combo_key_index: 0,
            combo_mods: 0,
            combo_mod_cursor: 0,
            combo_key_filter: String::new(),
            mouse_button: 1,
            consumer_index: 0,
            consumer_filter: String::new(),
            macro_slot: 0,
            macro_kind: 0,
            macro_text: String::new(),
            macro_events: Vec::new(),
            macro_event_cursor: 0,
            macro_event_field: MacroEventField::Action,
            macro_repeat: 1,
            gamepad_button: 1,
            led_control_index: 0,
            dirty: false,
        }
    }

    /// Initialize from a KeyAction (and optionally macros data).
    pub fn from_action(action: &KeyAction, macros: &[MacroSlot]) -> Self {
        let mut ed = Self::new();
        ed.binding_type = BindingType::from_action(action);
        match *action {
            KeyAction::Key(code) => {
                let keys = all_hid_keys();
                ed.key_list_index = keys.iter().position(|&(c, _)| c == code).unwrap_or(0);
            }
            KeyAction::Combo { mods, key } => {
                ed.combo_mods = mods;
                let keys = all_hid_keys();
                ed.combo_key_index = keys.iter().position(|&(c, _)| c == key).unwrap_or(0);
            }
            KeyAction::Mouse(btn) => {
                ed.mouse_button = btn;
            }
            KeyAction::Consumer(code) => {
                ed.consumer_index = CONSUMER_KEYS
                    .iter()
                    .position(|&(c, _)| c == code)
                    .unwrap_or(0);
            }
            KeyAction::Macro { index, kind } => {
                ed.macro_slot = index;
                ed.macro_kind = kind;
                if let Some(slot) = macros.get(index as usize) {
                    ed.macro_events = slot.events.clone();
                    ed.macro_repeat = slot.repeat_count.max(1);
                    if !slot.text_preview.is_empty() && !slot.text_preview.contains("events") {
                        ed.macro_text = slot.text_preview.clone();
                    }
                }
            }
            KeyAction::Gamepad(btn) => {
                ed.gamepad_button = btn;
            }
            KeyAction::LedControl { data } => {
                ed.led_control_index = LED_CONTROLS
                    .iter()
                    .position(|&(d, _)| d == data)
                    .unwrap_or(0);
            }
            _ => {}
        }
        ed
    }

    /// Build a KeyAction from the current editor state.
    pub fn to_action(&self) -> KeyAction {
        match self.binding_type {
            BindingType::Disabled => KeyAction::Disabled,
            BindingType::Key => {
                let keys = self.filtered_key_list();
                if let Some(&(code, _)) = keys.get(self.key_list_index) {
                    KeyAction::Key(code)
                } else {
                    let keys = all_hid_keys();
                    KeyAction::Key(keys.first().map(|&(c, _)| c).unwrap_or(0x04))
                }
            }
            BindingType::Combo => {
                let keys = self.filtered_combo_key_list();
                let key = keys
                    .get(self.combo_key_index)
                    .map(|&(c, _)| c)
                    .unwrap_or_else(|| all_hid_keys().first().map(|&(c, _)| c).unwrap_or(0x04));
                if self.combo_mods == 0 {
                    KeyAction::Key(key)
                } else {
                    KeyAction::Combo {
                        mods: self.combo_mods,
                        key,
                    }
                }
            }
            BindingType::Mouse => KeyAction::Mouse(self.mouse_button),
            BindingType::Consumer => {
                let list = self.filtered_consumer_list();
                let code = list
                    .get(self.consumer_index)
                    .map(|&(c, _)| c)
                    .unwrap_or(CONSUMER_KEYS[0].0);
                KeyAction::Consumer(code)
            }
            BindingType::Macro => KeyAction::Macro {
                index: self.macro_slot,
                kind: self.macro_kind,
            },
            BindingType::Gamepad => KeyAction::Gamepad(self.gamepad_button),
            BindingType::Fn => KeyAction::Fn,
            BindingType::LedControl => {
                let data = LED_CONTROLS
                    .get(self.led_control_index)
                    .map(|&(d, _)| d)
                    .unwrap_or([2, 1, 0]);
                KeyAction::LedControl { data }
            }
        }
    }

    /// Returns the ordered list of fields visible for the current binding type.
    pub fn visible_fields(&self) -> Vec<BindingField> {
        match self.binding_type {
            BindingType::Disabled | BindingType::Fn => vec![BindingField::Type],
            BindingType::Key => vec![
                BindingField::Type,
                BindingField::Filter,
                BindingField::KeyList,
            ],
            BindingType::Combo => vec![
                BindingField::Type,
                BindingField::Mods,
                BindingField::Filter,
                BindingField::KeyList,
            ],
            BindingType::Mouse | BindingType::Gamepad | BindingType::LedControl => {
                vec![BindingField::Type, BindingField::Value]
            }
            BindingType::Consumer => {
                vec![
                    BindingField::Type,
                    BindingField::Filter,
                    BindingField::KeyList,
                ]
            }
            BindingType::Macro => vec![
                BindingField::Type,
                BindingField::MacroSlot,
                BindingField::MacroKind,
                BindingField::MacroText,
                BindingField::MacroEvents,
            ],
        }
    }

    pub fn next_field(&mut self) {
        let fields = self.visible_fields();
        if let Some(idx) = fields.iter().position(|&f| f == self.field) {
            if idx + 1 < fields.len() {
                self.field = fields[idx + 1];
            }
        }
    }

    pub fn prev_field(&mut self) {
        let fields = self.visible_fields();
        if let Some(idx) = fields.iter().position(|&f| f == self.field) {
            if idx > 0 {
                self.field = fields[idx - 1];
            }
        }
    }

    pub fn adjust_right(&mut self) {
        self.dirty = true;
        match self.field {
            BindingField::Type => {
                let idx = self.binding_type.index();
                let next = (idx + 1) % BindingType::ALL.len();
                self.binding_type = BindingType::ALL[next];
                self.field = BindingField::Type;
            }
            BindingField::Value => match self.binding_type {
                BindingType::Mouse => {
                    self.mouse_button = (self.mouse_button + 1).min(5);
                }
                BindingType::Gamepad => {
                    self.gamepad_button = (self.gamepad_button + 1).min(32);
                }
                BindingType::LedControl => {
                    self.led_control_index = (self.led_control_index + 1) % LED_CONTROLS.len();
                }
                _ => {}
            },
            BindingField::Mods => {
                self.combo_mod_cursor = (self.combo_mod_cursor + 1) % MODIFIER_LIST.len();
            }
            BindingField::MacroSlot => {
                self.macro_slot = (self.macro_slot + 1).min(49);
            }
            BindingField::MacroKind => {
                self.macro_kind = (self.macro_kind + 1) % 3;
            }
            BindingField::MacroEvents => {
                // Adjust the focused sub-field of the selected event
                if let Some(evt) = self.macro_events.get_mut(self.macro_event_cursor) {
                    match self.macro_event_field {
                        MacroEventField::Key => {
                            // Cycle to next HID key
                            let keys = all_hid_keys();
                            if let Some(idx) = keys.iter().position(|&(c, _)| c == evt.keycode) {
                                evt.keycode = keys[(idx + 1) % keys.len()].0;
                            }
                        }
                        MacroEventField::Delay => {
                            evt.delay_ms = evt.delay_ms.saturating_add(5);
                        }
                        MacroEventField::Action => {
                            evt.is_down = !evt.is_down;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn adjust_left(&mut self) {
        self.dirty = true;
        match self.field {
            BindingField::Type => {
                let idx = self.binding_type.index();
                let prev = if idx == 0 {
                    BindingType::ALL.len() - 1
                } else {
                    idx - 1
                };
                self.binding_type = BindingType::ALL[prev];
                self.field = BindingField::Type;
            }
            BindingField::Value => match self.binding_type {
                BindingType::Mouse => {
                    self.mouse_button = self.mouse_button.saturating_sub(1).max(1);
                }
                BindingType::Gamepad => {
                    self.gamepad_button = self.gamepad_button.saturating_sub(1).max(1);
                }
                BindingType::LedControl => {
                    self.led_control_index = if self.led_control_index == 0 {
                        LED_CONTROLS.len() - 1
                    } else {
                        self.led_control_index - 1
                    };
                }
                _ => {}
            },
            BindingField::Mods => {
                self.combo_mod_cursor = if self.combo_mod_cursor == 0 {
                    MODIFIER_LIST.len() - 1
                } else {
                    self.combo_mod_cursor - 1
                };
            }
            BindingField::MacroSlot => {
                self.macro_slot = self.macro_slot.saturating_sub(1);
            }
            BindingField::MacroKind => {
                self.macro_kind = if self.macro_kind == 0 {
                    2
                } else {
                    self.macro_kind - 1
                };
            }
            BindingField::MacroEvents => {
                if let Some(evt) = self.macro_events.get_mut(self.macro_event_cursor) {
                    match self.macro_event_field {
                        MacroEventField::Key => {
                            let keys = all_hid_keys();
                            if let Some(idx) = keys.iter().position(|&(c, _)| c == evt.keycode) {
                                let prev = if idx == 0 { keys.len() - 1 } else { idx - 1 };
                                evt.keycode = keys[prev].0;
                            }
                        }
                        MacroEventField::Delay => {
                            evt.delay_ms = evt.delay_ms.saturating_sub(5);
                        }
                        MacroEventField::Action => {
                            evt.is_down = !evt.is_down;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn scroll_up(&mut self) {
        match self.field {
            BindingField::KeyList => {
                if self.binding_type == BindingType::Consumer {
                    self.consumer_index = self.consumer_index.saturating_sub(1);
                } else if self.binding_type == BindingType::Combo {
                    self.combo_key_index = self.combo_key_index.saturating_sub(1);
                } else {
                    self.key_list_index = self.key_list_index.saturating_sub(1);
                }
            }
            BindingField::MacroEvents => {
                self.macro_event_cursor = self.macro_event_cursor.saturating_sub(1);
            }
            _ => {}
        }
    }

    pub fn scroll_down(&mut self) {
        match self.field {
            BindingField::KeyList => {
                if self.binding_type == BindingType::Consumer {
                    let max = self.filtered_consumer_list().len().saturating_sub(1);
                    if self.consumer_index < max {
                        self.consumer_index += 1;
                    }
                } else if self.binding_type == BindingType::Combo {
                    let max = self.filtered_combo_key_list().len().saturating_sub(1);
                    if self.combo_key_index < max {
                        self.combo_key_index += 1;
                    }
                } else {
                    let max = self.filtered_key_list().len().saturating_sub(1);
                    if self.key_list_index < max {
                        self.key_list_index += 1;
                    }
                }
            }
            BindingField::MacroEvents => {
                let max = self.macro_events.len().saturating_sub(1);
                if self.macro_event_cursor < max {
                    self.macro_event_cursor += 1;
                }
            }
            _ => {}
        }
    }

    pub fn handle_char(&mut self, c: char) {
        match self.field {
            BindingField::Filter => {
                if self.binding_type == BindingType::Combo {
                    self.combo_key_filter.push(c);
                    self.combo_key_index = 0;
                } else if self.binding_type == BindingType::Consumer {
                    self.consumer_filter.push(c);
                    self.consumer_index = 0;
                } else {
                    self.key_filter.push(c);
                    self.key_list_index = 0;
                }
            }
            BindingField::MacroText => {
                self.macro_text.push(c);
                self.regenerate_macro_events_from_text();
                self.dirty = true;
            }
            _ => {}
        }
    }

    pub fn handle_backspace(&mut self) {
        match self.field {
            BindingField::Filter => {
                if self.binding_type == BindingType::Combo {
                    self.combo_key_filter.pop();
                    self.combo_key_index = 0;
                } else if self.binding_type == BindingType::Consumer {
                    self.consumer_filter.pop();
                    self.consumer_index = 0;
                } else {
                    self.key_filter.pop();
                    self.key_list_index = 0;
                }
            }
            BindingField::MacroText => {
                self.macro_text.pop();
                self.regenerate_macro_events_from_text();
                self.dirty = true;
            }
            _ => {}
        }
    }

    pub fn toggle_current_mod(&mut self) {
        if self.field == BindingField::Mods {
            if let Some(&(bit, _)) = MODIFIER_LIST.get(self.combo_mod_cursor) {
                self.combo_mods ^= bit;
                self.dirty = true;
            }
        }
    }

    pub fn filtered_key_list(&self) -> Vec<(u8, &'static str)> {
        let keys = all_hid_keys();
        if self.key_filter.is_empty() {
            return keys;
        }
        let f = self.key_filter.to_ascii_lowercase();
        keys.into_iter()
            .filter(|&(code, name)| {
                name.to_ascii_lowercase().contains(&f) || format!("0x{code:02x}").contains(&f)
            })
            .collect()
    }

    pub fn filtered_combo_key_list(&self) -> Vec<(u8, &'static str)> {
        let keys = all_hid_keys();
        if self.combo_key_filter.is_empty() {
            return keys;
        }
        let f = self.combo_key_filter.to_ascii_lowercase();
        keys.into_iter()
            .filter(|&(code, name)| {
                name.to_ascii_lowercase().contains(&f) || format!("0x{code:02x}").contains(&f)
            })
            .collect()
    }

    pub fn filtered_consumer_list(&self) -> Vec<(u16, &'static str)> {
        if self.consumer_filter.is_empty() {
            return CONSUMER_KEYS.to_vec();
        }
        let f = self.consumer_filter.to_ascii_lowercase();
        CONSUMER_KEYS
            .iter()
            .copied()
            .filter(|&(code, name)| {
                name.to_ascii_lowercase().contains(&f) || format!("0x{code:04x}").contains(&f)
            })
            .collect()
    }

    pub fn add_macro_event(&mut self) {
        // Add a press+release pair for key A with default delay
        self.macro_events.push(MacroEvent {
            keycode: 0x04,
            is_down: true,
            delay_ms: 10,
        });
        self.macro_events.push(MacroEvent {
            keycode: 0x04,
            is_down: false,
            delay_ms: 10,
        });
        self.macro_event_cursor = self.macro_events.len().saturating_sub(2);
        self.macro_text = "(custom)".to_string();
        self.dirty = true;
    }

    pub fn remove_macro_event(&mut self) {
        if !self.macro_events.is_empty() && self.macro_event_cursor < self.macro_events.len() {
            self.macro_events.remove(self.macro_event_cursor);
            if self.macro_event_cursor >= self.macro_events.len() && !self.macro_events.is_empty() {
                self.macro_event_cursor = self.macro_events.len() - 1;
            }
            self.macro_text = "(custom)".to_string();
            self.dirty = true;
        }
    }

    /// Cycle the sub-field focus for macro events (Action -> Key -> Delay -> Action).
    pub fn cycle_macro_event_field(&mut self) {
        self.macro_event_field = match self.macro_event_field {
            MacroEventField::Action => MacroEventField::Key,
            MacroEventField::Key => MacroEventField::Delay,
            MacroEventField::Delay => MacroEventField::Action,
        };
    }

    pub fn macro_events_to_tuples(&self) -> Vec<(u8, bool, u16)> {
        self.macro_events
            .iter()
            .map(|e| (e.keycode, e.is_down, e.delay_ms))
            .collect()
    }

    /// Regenerate macro_events from the text field using char_to_hid.
    fn regenerate_macro_events_from_text(&mut self) {
        use crate::protocol::hid::char_to_hid;
        self.macro_events.clear();
        let delay: u16 = 10;
        for ch in self.macro_text.chars() {
            if let Some((keycode, needs_shift)) = char_to_hid(ch) {
                if needs_shift {
                    self.macro_events.push(MacroEvent {
                        keycode: 0xE1, // LShift
                        is_down: true,
                        delay_ms: 0,
                    });
                }
                self.macro_events.push(MacroEvent {
                    keycode,
                    is_down: true,
                    delay_ms: delay,
                });
                self.macro_events.push(MacroEvent {
                    keycode,
                    is_down: false,
                    delay_ms: delay,
                });
                if needs_shift {
                    self.macro_events.push(MacroEvent {
                        keycode: 0xE1,
                        is_down: false,
                        delay_ms: delay,
                    });
                }
            }
        }
    }
}

// ============================================================================
// Rendering
// ============================================================================

pub(in crate::tui) fn render_remaps(f: &mut Frame, app: &mut App, area: Rect) {
    // Check loading state first
    if app.loading.remaps == LoadState::Loading {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Remaps [Enter: edit, d: reset, f: filter]");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let throbber = Throbber::default()
            .label("Loading remaps...")
            .throbber_style(Style::default().fg(Color::Yellow));
        f.render_stateful_widget(throbber, inner, &mut app.throbber_state.clone());
        return;
    }

    let filtered = app.filtered_remaps();

    // Split into remap list (left) and editor panel (right)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    // Left panel: Remap list
    let filter_label = app.remap_layer_view.label();
    let list_focus = app.remap_focus == RemapFocus::List;
    let list_border = if list_focus {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let list_title = format!("Remaps ({}) [{}]", filtered.len(), filter_label);

    if filtered.is_empty() {
        let msg = if app.loading.remaps == LoadState::Error {
            "Failed to load remaps"
        } else {
            "No remappings found. All keys at defaults."
        };
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(msg, Style::default().fg(Color::Yellow))),
            Line::from(""),
            Line::from("Press 'r' to reload, 'f' to change filter"),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(list_border)
                .title(list_title),
        );
        f.render_widget(help, chunks[0]);
    } else {
        let items: Vec<ListItem> = filtered
            .iter()
            .map(|&remap_idx| {
                let r = &app.remaps[remap_idx];
                let layer_prefix = match r.layer {
                    Layer::Fn => Span::styled("Fn ", Style::default().fg(Color::Yellow)),
                    Layer::Layer1 => Span::styled("L1 ", Style::default().fg(Color::Cyan)),
                    Layer::Base => Span::styled("L0 ", Style::default().fg(Color::DarkGray)),
                };
                let action_style = match r.action {
                    KeyAction::Macro { .. } => Style::default().fg(Color::Magenta),
                    KeyAction::Key(_) | KeyAction::Combo { .. } => {
                        Style::default().fg(Color::Green)
                    }
                    KeyAction::Mouse(_) | KeyAction::Gamepad(_) => {
                        Style::default().fg(Color::Yellow)
                    }
                    _ => Style::default().fg(Color::White),
                };
                ListItem::new(Line::from(vec![
                    layer_prefix,
                    Span::styled(
                        format!("{:<8}", r.position),
                        Style::default().fg(Color::White),
                    ),
                    Span::raw(" -> "),
                    Span::styled(format!("{}", r.action), action_style),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(list_border)
                    .title(list_title),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        let mut state = ListState::default();
        state.select(Some(
            app.remap_selected.min(filtered.len().saturating_sub(1)),
        ));
        f.render_stateful_widget(list, chunks[0], &mut state);
    }

    // Right panel: Binding editor (always visible)
    let editor_focus = app.remap_focus == RemapFocus::Editor;
    let editor_border = if editor_focus {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let ed = &app.binding_editor;

    // Vertical layout: editor content area + help bar
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(2)])
        .split(chunks[1]);

    let mut lines: Vec<Line> = Vec::new();

    // Type selector
    let type_focused = editor_focus && ed.field == BindingField::Type;
    let type_style = if type_focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    lines.push(Line::from(vec![
        Span::raw(" Type:  "),
        Span::styled("< ", type_style),
        Span::styled(ed.binding_type.label(), type_style),
        Span::styled(" >", type_style),
    ]));
    lines.push(Line::from(""));

    // Type-specific fields
    match ed.binding_type {
        BindingType::Disabled | BindingType::Fn => {
            // No extra fields
        }
        BindingType::Key => {
            // Filter
            let filter_focused = editor_focus && ed.field == BindingField::Filter;
            let filter_style = if filter_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::raw(" Filter: "),
                Span::styled(&ed.key_filter, filter_style),
                if filter_focused {
                    Span::styled("\u{2588}", Style::default().fg(Color::White))
                } else {
                    Span::raw("")
                },
            ]));

            // Key list
            let list_focused = editor_focus && ed.field == BindingField::KeyList;
            let keys = ed.filtered_key_list();
            let max_show = (right_chunks[0].height as usize).saturating_sub(6);
            let start = if ed.key_list_index >= max_show {
                ed.key_list_index - max_show + 1
            } else {
                0
            };
            for (i, &(code, name)) in keys.iter().enumerate().skip(start).take(max_show) {
                let is_selected = i == ed.key_list_index;
                let prefix = if is_selected { " > " } else { "   " };
                let style = if is_selected && list_focused {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(name, style),
                    Span::styled(
                        format!(" (0x{code:02X})"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }
        BindingType::Combo => {
            // Modifier grid
            let mods_focused = editor_focus && ed.field == BindingField::Mods;
            let mut mod_spans = vec![Span::raw(" Mods:  ")];
            for (i, &(bit, name)) in MODIFIER_LIST.iter().enumerate() {
                let checked = ed.combo_mods & bit != 0;
                let is_cursor = mods_focused && i == ed.combo_mod_cursor;
                let style = if is_cursor {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if checked {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let mark = if checked { "x" } else { " " };
                mod_spans.push(Span::styled(format!("[{mark}]{name} "), style));
            }
            lines.push(Line::from(mod_spans));

            // Filter
            let filter_focused = editor_focus && ed.field == BindingField::Filter;
            let filter_style = if filter_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::raw(" Filter: "),
                Span::styled(&ed.combo_key_filter, filter_style),
                if filter_focused {
                    Span::styled("\u{2588}", Style::default().fg(Color::White))
                } else {
                    Span::raw("")
                },
            ]));

            // Key list
            let list_focused = editor_focus && ed.field == BindingField::KeyList;
            let keys = ed.filtered_combo_key_list();
            let max_show = (right_chunks[0].height as usize).saturating_sub(7);
            let start = if ed.combo_key_index >= max_show {
                ed.combo_key_index - max_show + 1
            } else {
                0
            };
            for (i, &(code, name)) in keys.iter().enumerate().skip(start).take(max_show) {
                let is_selected = i == ed.combo_key_index;
                let prefix = if is_selected { " > " } else { "   " };
                let style = if is_selected && list_focused {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(name, style),
                    Span::styled(
                        format!(" (0x{code:02X})"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }
        BindingType::Mouse => {
            let focused = editor_focus && ed.field == BindingField::Value;
            let style = if focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::raw(" Button: "),
                Span::styled("< ", style),
                Span::styled(format!("{}", ed.mouse_button), style),
                Span::styled(" >", style),
            ]));
        }
        BindingType::Consumer => {
            // Filter
            let filter_focused = editor_focus && ed.field == BindingField::Filter;
            let filter_style = if filter_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::raw(" Filter: "),
                Span::styled(&ed.consumer_filter, filter_style),
                if filter_focused {
                    Span::styled("\u{2588}", Style::default().fg(Color::White))
                } else {
                    Span::raw("")
                },
            ]));

            // Consumer key list
            let list_focused = editor_focus && ed.field == BindingField::KeyList;
            let keys = ed.filtered_consumer_list();
            let max_show = (right_chunks[0].height as usize).saturating_sub(6);
            let start = if ed.consumer_index >= max_show {
                ed.consumer_index - max_show + 1
            } else {
                0
            };
            for (i, &(code, name)) in keys.iter().enumerate().skip(start).take(max_show) {
                let is_selected = i == ed.consumer_index;
                let prefix = if is_selected { " > " } else { "   " };
                let style = if is_selected && list_focused {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(name, style),
                    Span::styled(
                        format!(" (0x{code:04X})"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }
        BindingType::Macro => {
            // Slot spinner
            let slot_focused = editor_focus && ed.field == BindingField::MacroSlot;
            let slot_style = if slot_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::raw(" Slot:  "),
                Span::styled("< ", slot_style),
                Span::styled(format!("{}", ed.macro_slot), slot_style),
                Span::styled(" >", slot_style),
            ]));

            // Kind spinner
            let kind_focused = editor_focus && ed.field == BindingField::MacroKind;
            let kind_style = if kind_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let kind_label = match ed.macro_kind {
                0 => "Repeat",
                1 => "Toggle",
                2 => "Hold",
                _ => "?",
            };
            lines.push(Line::from(vec![
                Span::raw(" Kind:  "),
                Span::styled("< ", kind_style),
                Span::styled(kind_label, kind_style),
                Span::styled(" >", kind_style),
            ]));

            // Text input
            let text_focused = editor_focus && ed.field == BindingField::MacroText;
            let text_style = if text_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::raw(" Text:  "),
                Span::styled(&ed.macro_text, text_style),
                if text_focused {
                    Span::styled("\u{2588}", Style::default().fg(Color::White))
                } else {
                    Span::raw("")
                },
            ]));

            // Event list
            let events_focused = editor_focus && ed.field == BindingField::MacroEvents;
            lines.push(Line::from(Span::styled(
                " Events:",
                if events_focused {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::White)
                },
            )));

            let max_show = (right_chunks[0].height as usize).saturating_sub(9);
            let start = if ed.macro_event_cursor >= max_show {
                ed.macro_event_cursor - max_show + 1
            } else {
                0
            };
            for (i, evt) in ed
                .macro_events
                .iter()
                .enumerate()
                .skip(start)
                .take(max_show)
            {
                let is_selected = i == ed.macro_event_cursor;
                let prefix = if is_selected { " > " } else { "   " };
                let arrow = if evt.is_down { "\u{2193}" } else { "\u{2191}" };
                let arrow_color = if evt.is_down {
                    Color::Green
                } else {
                    Color::Red
                };

                let key_style = if is_selected
                    && events_focused
                    && ed.macro_event_field == MacroEventField::Key
                {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let delay_style = if is_selected
                    && events_focused
                    && ed.macro_event_field == MacroEventField::Delay
                {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let action_style = if is_selected
                    && events_focused
                    && ed.macro_event_field == MacroEventField::Action
                {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(arrow_color)
                };

                lines.push(Line::from(vec![
                    Span::styled(
                        prefix,
                        if is_selected && events_focused {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default()
                        },
                    ),
                    Span::styled(arrow, action_style),
                    Span::raw(" "),
                    Span::styled(key_name(evt.keycode), key_style),
                    Span::styled(format!("  {}ms", evt.delay_ms), delay_style),
                ]));
            }
            if ed.macro_events.len() > max_show {
                lines.push(Line::from(Span::styled(
                    format!("   ({} total events)", ed.macro_events.len()),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        BindingType::Gamepad => {
            let focused = editor_focus && ed.field == BindingField::Value;
            let style = if focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::raw(" Button: "),
                Span::styled("< ", style),
                Span::styled(format!("{}", ed.gamepad_button), style),
                Span::styled(" >", style),
            ]));
        }
        BindingType::LedControl => {
            let focused = editor_focus && ed.field == BindingField::Value;
            let style = if focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let label = LED_CONTROLS
                .get(ed.led_control_index)
                .map(|&(_, n)| n)
                .unwrap_or("?");
            lines.push(Line::from(vec![
                Span::raw(" Action: "),
                Span::styled("< ", style),
                Span::styled(label, style),
                Span::styled(" >", style),
            ]));
        }
    }

    let editor_title = if let Some(&remap_idx) = filtered.get(app.remap_selected) {
        let r = &app.remaps[remap_idx];
        format!("Edit Binding: {} ({})", r.position, r.layer.name())
    } else {
        "Edit Binding".to_string()
    };

    let editor_block = Block::default()
        .borders(Borders::ALL)
        .border_style(editor_border)
        .title(editor_title);

    let editor_para = Paragraph::new(lines).block(editor_block);
    f.render_widget(editor_para, right_chunks[0]);

    // Help bar
    let help_text = if editor_focus {
        match ed.binding_type {
            BindingType::Macro if ed.field == BindingField::MacroEvents => {
                "\u{2190}\u{2192} adjust  \u{2191}\u{2193} scroll  Tab:field  Space:press/release  a:add  x:del  Enter:save  Esc:back"
            }
            _ => "\u{2190}\u{2192} adjust  \u{2191}\u{2193}/Tab navigate  Space:toggle  Enter:save  Esc:back",
        }
    } else {
        "Enter/\u{2192} edit  d:reset  f:filter  r:refresh"
    };
    let help = Paragraph::new(Line::from(Span::styled(
        help_text,
        Style::default().fg(Color::DarkGray),
    )))
    .alignment(Alignment::Center);
    f.render_widget(help, right_chunks[1]);
}
