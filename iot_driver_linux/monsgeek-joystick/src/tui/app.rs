//! TUI application state

use crate::config::{AxisId, JoystickConfig};
use crate::mapper::AxisMapper;
use std::path::PathBuf;

/// Current UI mode / tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// View live axis values and key depths
    Live,
    /// Configure axis mappings
    Configure,
    /// Interactive calibration
    Calibrate,
}

/// What element is currently selected in configure mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedElement {
    /// Selecting which axis to configure
    AxisList(usize),
    /// Editing a specific axis property
    AxisProperty(usize, AxisProperty),
    /// Selecting a key on the keyboard layout
    KeyboardKey,
}

/// Which property of an axis is being edited
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisProperty {
    Enabled,
    MappingMode,
    PositiveKey,
    NegativeKey,
    Deadzone,
    Curve,
}

/// Keyboard connection status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardStatus {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

/// Joystick status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoystickStatus {
    NotCreated,
    Active,
    Error,
}

/// Main application state
pub struct App {
    /// Current UI mode
    pub mode: AppMode,
    /// Configuration
    pub config: JoystickConfig,
    /// Config file path
    pub config_path: PathBuf,
    /// Unsaved changes flag
    pub config_dirty: bool,
    /// Axis mapper with current key depths
    pub mapper: AxisMapper,
    /// Current selection in configure mode
    pub selected: SelectedElement,
    /// Keyboard connection status
    pub keyboard_status: KeyboardStatus,
    /// Virtual joystick status
    pub joystick_status: JoystickStatus,
    /// Status message (for errors/info)
    pub status_message: Option<String>,
    /// Currently pressed key (for key selection)
    pub pressed_key: Option<u8>,
    /// Waiting for key press (in key selection mode)
    pub awaiting_key_press: bool,
    /// Show help overlay
    pub show_help: bool,
    /// Should quit
    pub should_quit: bool,
    /// Precision factor for depth conversion (depth_raw / precision_factor → mm)
    pub precision_factor: f64,
}

impl App {
    /// Create a new app with given config
    pub fn new(config: JoystickConfig, config_path: PathBuf) -> Self {
        Self {
            mode: AppMode::Live,
            config,
            config_path,
            config_dirty: false,
            mapper: AxisMapper::new(),
            selected: SelectedElement::AxisList(0),
            keyboard_status: KeyboardStatus::Disconnected,
            joystick_status: JoystickStatus::NotCreated,
            status_message: None,
            pressed_key: None,
            awaiting_key_press: false,
            show_help: false,
            should_quit: false,
            precision_factor: 100.0,
        }
    }

    /// Switch to a different mode
    pub fn set_mode(&mut self, mode: AppMode) {
        self.mode = mode;
        // Reset selection when changing modes
        if mode == AppMode::Configure {
            self.selected = SelectedElement::AxisList(0);
        }
    }

    /// Get list of enabled axis IDs from config
    pub fn enabled_axes(&self) -> Vec<AxisId> {
        self.config
            .axes
            .iter()
            .filter(|a| a.enabled)
            .map(|a| a.id)
            .collect()
    }

    /// Update a key depth value
    pub fn update_key_depth(&mut self, key_index: u8, depth_mm: f32) {
        self.mapper.update_key_depth(key_index, depth_mm);
        self.pressed_key = Some(key_index);
    }

    /// Mark config as changed
    pub fn mark_dirty(&mut self) {
        self.config_dirty = true;
    }

    /// Save config to file
    pub fn save_config(&mut self) -> anyhow::Result<()> {
        self.config.save(&self.config_path)?;
        self.config_dirty = false;
        self.status_message = Some("Config saved".to_string());
        Ok(())
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        match &self.selected {
            SelectedElement::AxisList(idx) => {
                if *idx > 0 {
                    self.selected = SelectedElement::AxisList(idx - 1);
                }
            }
            SelectedElement::AxisProperty(axis_idx, prop) => {
                let props = axis_properties();
                if let Some(i) = props.iter().position(|p| p == prop) {
                    if i > 0 {
                        self.selected = SelectedElement::AxisProperty(*axis_idx, props[i - 1]);
                    } else {
                        self.selected = SelectedElement::AxisList(*axis_idx);
                    }
                }
            }
            SelectedElement::KeyboardKey => {}
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        match &self.selected {
            SelectedElement::AxisList(idx) => {
                if *idx + 1 < self.config.axes.len() {
                    self.selected = SelectedElement::AxisList(idx + 1);
                }
            }
            SelectedElement::AxisProperty(axis_idx, prop) => {
                let props = axis_properties();
                if let Some(i) = props.iter().position(|p| p == prop) {
                    if i + 1 < props.len() {
                        self.selected = SelectedElement::AxisProperty(*axis_idx, props[i + 1]);
                    }
                }
            }
            SelectedElement::KeyboardKey => {}
        }
    }

    /// Enter into selected item (drill down)
    pub fn select_enter(&mut self) {
        match &self.selected {
            SelectedElement::AxisList(idx) => {
                let props = axis_properties();
                if !props.is_empty() {
                    self.selected = SelectedElement::AxisProperty(*idx, props[0]);
                }
            }
            SelectedElement::AxisProperty(_, AxisProperty::PositiveKey)
            | SelectedElement::AxisProperty(_, AxisProperty::NegativeKey) => {
                self.awaiting_key_press = true;
                self.selected = SelectedElement::KeyboardKey;
            }
            _ => {}
        }
    }

    /// Go back (escape)
    pub fn select_back(&mut self) {
        match &self.selected {
            SelectedElement::AxisProperty(idx, _) => {
                self.selected = SelectedElement::AxisList(*idx);
            }
            SelectedElement::KeyboardKey => {
                self.awaiting_key_press = false;
                self.selected = SelectedElement::AxisList(0);
            }
            _ => {}
        }
    }

    /// Toggle current boolean property or cycle enum
    pub fn toggle_current(&mut self) {
        if let SelectedElement::AxisProperty(idx, prop) = self.selected {
            if let Some(axis) = self.config.axes.get_mut(idx) {
                if prop == AxisProperty::Enabled {
                    axis.enabled = !axis.enabled;
                    self.mark_dirty();
                }
            }
        }
    }

    /// Adjust current numeric property
    pub fn adjust_current(&mut self, delta: f32) {
        if let SelectedElement::AxisProperty(idx, prop) = self.selected {
            if let Some(axis) = self.config.axes.get_mut(idx) {
                match prop {
                    AxisProperty::Deadzone => {
                        axis.calibration.deadzone_percent =
                            (axis.calibration.deadzone_percent + delta).clamp(0.0, 50.0);
                        self.mark_dirty();
                    }
                    AxisProperty::Curve => {
                        axis.calibration.curve_exponent =
                            (axis.calibration.curve_exponent + delta * 0.1).clamp(0.5, 3.0);
                        self.mark_dirty();
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Get list of editable axis properties
fn axis_properties() -> Vec<AxisProperty> {
    vec![
        AxisProperty::Enabled,
        AxisProperty::MappingMode,
        AxisProperty::PositiveKey,
        AxisProperty::NegativeKey,
        AxisProperty::Deadzone,
        AxisProperty::Curve,
    ]
}
