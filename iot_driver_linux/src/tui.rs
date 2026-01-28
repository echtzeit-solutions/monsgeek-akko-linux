// MonsGeek M1 V5 HE TUI Application
// Real-time monitoring and settings configuration

use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind,
        MouseButton, MouseEventKind,
    },
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use futures::StreamExt;
use ratatui::{prelude::*, widgets::*};
use std::cell::Cell as StdCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, stdout};
use std::sync::Arc;
use std::time::{Duration, Instant};
use throbber_widgets_tui::{Throbber, ThrobberState, BRAILLE_SIX};
use tokio::sync::{broadcast, mpsc};
use tui_scrollview::{ScrollView, ScrollViewState, ScrollbarVisibility};

use std::path::PathBuf;

// Use shared library
use crate::firmware_api::FirmwareCheckResult;
use crate::hid::BatteryInfo;
use crate::power_supply::{
    find_dongle_battery_power_supply, find_hid_battery_power_supply, read_kernel_battery,
};
use crate::{cmd, devices, key_mode, magnetism, DeviceInfo, TriggerSettings};

// Keyboard abstraction layer - using async interface directly
use monsgeek_keyboard::{
    KeyboardInterface, KeyboardOptions as KbOptions, LedMode, LedParams, Precision, RgbColor,
    SleepTimeSettings, TimestampedEvent, VendorEvent,
};
use monsgeek_transport::HidDiscovery;

/// Battery data source
#[derive(Debug, Clone)]
enum BatterySource {
    /// Kernel power_supply sysfs (via eBPF filter)
    Kernel(PathBuf),
    /// Direct vendor protocol (HID feature report)
    Vendor,
}

/// Application state
struct App {
    info: DeviceInfo,
    tab: usize,
    selected: usize,
    key_depths: Vec<f32>,
    depth_monitoring: bool,
    status_msg: String,
    connected: bool,
    device_name: String,
    key_count: u8,
    has_sidelight: bool,
    has_magnetism: bool,
    // Trigger settings
    triggers: Option<TriggerSettings>,
    trigger_scroll: usize,
    trigger_view_mode: TriggerViewMode,
    trigger_selected_key: usize, // Selected key in layout view
    precision: Precision,
    // Keyboard options
    options: Option<KeyboardOptions>,
    // Sleep time settings (loaded separately, merged into options)
    sleep_settings: Option<SleepTimeSettings>,
    // Macro editor state
    macros: Vec<MacroSlot>,
    macro_selected: usize,
    macro_editing: bool,
    macro_edit_text: String,
    // Key depth visualization
    depth_view_mode: DepthViewMode,
    depth_history: Vec<VecDeque<(f64, f32)>>, // Per-key history (timestamp, depth_mm)
    active_keys: HashSet<usize>,              // Keys with recent activity
    selected_keys: HashSet<usize>,            // Keys selected for time series view
    depth_cursor: usize,                      // Cursor for key selection
    max_observed_depth: f32,                  // Max depth observed during session (for bar scaling)
    depth_last_update: Vec<Instant>,          // Last update time per key (for stale detection)
    // Battery status (for 2.4GHz dongle)
    battery: Option<BatteryInfo>,
    battery_source: Option<BatterySource>,
    last_battery_check: Instant,
    is_wireless: bool,
    // Help popup
    show_help: bool,
    // Keyboard interface (async, wrapped in Arc for spawning tasks)
    keyboard: Option<Arc<KeyboardInterface>>,
    // Event receiver for low-latency EP2 notifications (with timestamps)
    event_rx: Option<broadcast::Receiver<TimestampedEvent>>,
    loading: LoadingStates,
    throbber_state: ThrobberState,
    // Async result channel (sender for spawned tasks)
    result_tx: mpsc::UnboundedSender<AsyncResult>,
    // Hex color input
    hex_editing: bool,
    hex_input: String,
    hex_target: HexColorTarget,
    // Firmware check result
    firmware_check: Option<FirmwareCheckResult>,
    // Mouse hit areas (updated during render via interior mutability)
    tab_bar_area: StdCell<Rect>,
    content_area: StdCell<Rect>,
    // Scroll view state for content area
    scroll_state: ScrollViewState,
    // Trigger edit modal
    trigger_edit_modal: Option<TriggerEditModal>,
}

/// Which color is being edited with hex input
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum HexColorTarget {
    #[default]
    MainLed,
    SideLed,
}

/// Macro slot data
#[derive(Debug, Clone, Default)]
struct MacroSlot {
    events: Vec<MacroEvent>,
    repeat_count: u16,
    text_preview: String,
}

/// Single macro event
#[derive(Debug, Clone)]
struct MacroEvent {
    keycode: u8,
    is_down: bool,
    delay_ms: u8,
}

/// Keyboard options state
#[derive(Debug, Clone, Default)]
struct KeyboardOptions {
    os_mode: u8,
    fn_layer: u8,
    anti_mistouch: bool,
    rt_stability: u8,
    wasd_swap: bool,
    // Sleep time settings (all in seconds, 0 = disabled)
    idle_bt: u16,
    idle_24g: u16,
    deep_bt: u16,
    deep_24g: u16,
}

/// Sleep time field identifier for updates
#[derive(Debug, Clone, Copy, PartialEq)]
enum SleepField {
    IdleBt,
    Idle24g,
    DeepBt,
    Deep24g,
}

/// Key depth visualization mode
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum DepthViewMode {
    #[default]
    BarChart, // Bar chart of all active keys
    TimeSeries, // Time series graph of selected keys
}

/// Trigger settings view mode
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum TriggerViewMode {
    #[default]
    List, // Scrollable list of keys
    Layout, // Visual keyboard layout
}

/// Configuration for a spinner (numeric value with left/right adjustment)
/// Reusable across different tabs and modals
#[derive(Debug, Clone, Copy)]
struct SpinnerConfig {
    /// Minimum value
    min: f32,
    /// Maximum value
    max: f32,
    /// Step size for normal adjustment
    step: f32,
    /// Step size when shift is held (coarse adjustment)
    step_coarse: f32,
    /// Number of decimal places to display
    decimals: u8,
    /// Unit suffix (e.g., "mm", "%", "")
    unit: &'static str,
}

impl SpinnerConfig {
    /// Increment value by step (or coarse step if shift held)
    fn increment(&self, value: f32, coarse: bool) -> f32 {
        let step = if coarse { self.step_coarse } else { self.step };
        (value + step).min(self.max)
    }

    /// Decrement value by step (or coarse step if shift held)
    fn decrement(&self, value: f32, coarse: bool) -> f32 {
        let step = if coarse { self.step_coarse } else { self.step };
        (value - step).max(self.min)
    }

    /// Increment u8 value (for RGB components)
    fn increment_u8(&self, value: u8, coarse: bool) -> u8 {
        let step = if coarse { self.step_coarse } else { self.step } as u8;
        value.saturating_add(step).min(self.max as u8)
    }

    /// Decrement u8 value (for RGB components)
    fn decrement_u8(&self, value: u8, coarse: bool) -> u8 {
        let step = if coarse { self.step_coarse } else { self.step } as u8;
        value.saturating_sub(step).max(self.min as u8)
    }

    /// Format value for display
    fn format(&self, value: f32) -> String {
        match self.decimals {
            0 => format!("{:.0}", value),
            1 => format!("{:.1}", value),
            _ => format!("{:.2}", value),
        }
    }
}

/// Spinner config for RGB color components (0-255)
const RGB_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 255.0,
    step: 1.0,
    step_coarse: 10.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for LED brightness (0-4)
const BRIGHTNESS_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 4.0,
    step: 1.0,
    step_coarse: 1.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for LED speed (0-4)
const SPEED_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 4.0,
    step: 1.0,
    step_coarse: 1.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for Fn layer (0-3)
const FN_LAYER_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 3.0,
    step: 1.0,
    step_coarse: 1.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for RT stability (0-125, step 25)
const RT_STABILITY_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 125.0,
    step: 25.0,
    step_coarse: 25.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for sleep time in seconds (0-3600, step 60s, coarse 300s)
const SLEEP_TIME_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 3600.0,
    step: 60.0,
    step_coarse: 300.0,
    decimals: 0,
    unit: "s",
};

/// Trigger edit modal target - what we're editing
#[derive(Debug, Clone, Copy, PartialEq)]
enum TriggerEditTarget {
    /// Edit global settings (applies to all keys)
    Global,
    /// Edit specific key settings
    PerKey { key_index: usize },
}

/// Editable field in trigger settings
#[derive(Debug, Clone, Copy, PartialEq)]
enum TriggerField {
    Actuation,
    Release,
    RtPress,
    RtLift,
    TopDeadzone,
    BottomDeadzone,
    Mode,
}

impl TriggerField {
    fn label(&self) -> &'static str {
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

    fn all() -> &'static [TriggerField] {
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
    fn spinner_config(&self) -> Option<SpinnerConfig> {
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
struct TriggerEditModal {
    /// What we're editing (global or per-key)
    target: TriggerEditTarget,
    /// Currently focused field
    field_index: usize,
    /// Depth history for the chart (samples over time)
    depth_history: VecDeque<f32>,
    /// Key to filter depth reports (None = show all active keys)
    depth_filter: Option<usize>,
    /// Current values being edited
    actuation_mm: f32,
    release_mm: f32,
    rt_press_mm: f32,
    rt_lift_mm: f32,
    top_dz_mm: f32,
    bottom_dz_mm: f32,
    mode: u8,
}

impl TriggerEditModal {
    /// Create modal for editing global settings
    fn new_global(triggers: &TriggerSettings, precision: Precision) -> Self {
        let factor = precision.factor() as f32;
        // Decode first key's u16 values as representative global values
        let decode_u16 = |data: &[u8], idx: usize| -> u16 {
            if idx * 2 + 1 < data.len() {
                u16::from_le_bytes([data[idx * 2], data[idx * 2 + 1]])
            } else {
                0
            }
        };

        Self {
            target: TriggerEditTarget::Global,
            field_index: 0,
            depth_history: VecDeque::with_capacity(100),
            depth_filter: None,
            actuation_mm: decode_u16(&triggers.press_travel, 0) as f32 / factor,
            release_mm: decode_u16(&triggers.lift_travel, 0) as f32 / factor,
            rt_press_mm: decode_u16(&triggers.rt_press, 0) as f32 / factor,
            rt_lift_mm: decode_u16(&triggers.rt_lift, 0) as f32 / factor,
            top_dz_mm: decode_u16(&triggers.top_deadzone, 0) as f32 / factor,
            bottom_dz_mm: decode_u16(&triggers.bottom_deadzone, 0) as f32 / factor,
            mode: triggers.key_modes.first().copied().unwrap_or(0),
        }
    }

    /// Create modal for editing a specific key
    fn new_per_key(key_index: usize, triggers: &TriggerSettings, precision: Precision) -> Self {
        let factor = precision.factor() as f32;
        let decode_u16 = |data: &[u8], idx: usize| -> u16 {
            if idx * 2 + 1 < data.len() {
                u16::from_le_bytes([data[idx * 2], data[idx * 2 + 1]])
            } else {
                0
            }
        };

        Self {
            target: TriggerEditTarget::PerKey { key_index },
            field_index: 0,
            depth_history: VecDeque::with_capacity(100),
            depth_filter: Some(key_index),
            actuation_mm: decode_u16(&triggers.press_travel, key_index) as f32 / factor,
            release_mm: decode_u16(&triggers.lift_travel, key_index) as f32 / factor,
            rt_press_mm: decode_u16(&triggers.rt_press, key_index) as f32 / factor,
            rt_lift_mm: decode_u16(&triggers.rt_lift, key_index) as f32 / factor,
            top_dz_mm: decode_u16(&triggers.top_deadzone, key_index) as f32 / factor,
            bottom_dz_mm: decode_u16(&triggers.bottom_deadzone, key_index) as f32 / factor,
            mode: triggers.key_modes.get(key_index).copied().unwrap_or(0),
        }
    }

    fn current_field(&self) -> TriggerField {
        TriggerField::all()[self.field_index]
    }

    fn next_field(&mut self) {
        self.field_index = (self.field_index + 1) % TriggerField::all().len();
    }

    fn prev_field(&mut self) {
        self.field_index = if self.field_index == 0 {
            TriggerField::all().len() - 1
        } else {
            self.field_index - 1
        };
    }

    /// Get the current value for the selected field
    fn current_value(&self) -> f32 {
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
    fn set_current_value(&mut self, value: f32) {
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
    fn increment_current(&mut self, coarse: bool) {
        if let Some(config) = self.current_field().spinner_config() {
            let new_value = config.increment(self.current_value(), coarse);
            self.set_current_value(new_value);
        } else if self.current_field() == TriggerField::Mode {
            self.cycle_mode();
        }
    }

    /// Decrement the current field value (using spinner config)
    fn decrement_current(&mut self, coarse: bool) {
        if let Some(config) = self.current_field().spinner_config() {
            let new_value = config.decrement(self.current_value(), coarse);
            self.set_current_value(new_value);
        } else if self.current_field() == TriggerField::Mode {
            self.cycle_mode_reverse();
        }
    }

    /// Cycle mode forward: Normal -> RT -> DKS -> SnapTap -> Normal
    fn cycle_mode(&mut self) {
        self.mode = match self.mode & 0x7F {
            0 => 0x80,                       // Normal -> RT
            _ if self.mode & 0x80 != 0 => 2, // RT -> DKS
            2 => 7,                          // DKS -> SnapTap
            7 => 0,                          // SnapTap -> Normal
            _ => 0,                          // Unknown -> Normal
        };
    }

    /// Cycle mode backward: Normal <- RT <- DKS <- SnapTap <- Normal
    fn cycle_mode_reverse(&mut self) {
        self.mode = match self.mode & 0x7F {
            0 if self.mode & 0x80 != 0 => 0, // RT -> Normal
            0 => 7,                          // Normal -> SnapTap
            2 => 0x80,                       // DKS -> RT
            7 => 2,                          // SnapTap -> DKS
            _ => 0,                          // Unknown -> Normal
        };
    }

    /// Add a depth sample to history
    fn push_depth(&mut self, depth_mm: f32) {
        if self.depth_history.len() >= 100 {
            self.depth_history.pop_front();
        }
        self.depth_history.push_back(depth_mm);
    }
}

/// Loading state for async data fetching
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum LoadState {
    #[default]
    NotLoaded,
    Loading,
    Loaded,
    Error,
}

/// Track loading state per HID query group
#[derive(Debug, Clone, Default)]
struct LoadingStates {
    // Device info queries (tab 0/1)
    usb_version: LoadState, // device_id + version
    profile: LoadState,
    debounce: LoadState,
    polling_rate: LoadState,
    led_params: LoadState, // all main LED fields
    side_led_params: LoadState,
    kb_options_info: LoadState, // fn_layer + wasd_swap for info display
    precision: LoadState,
    sleep_time: LoadState,
    firmware_check: LoadState, // server firmware version check
    // Other tabs
    triggers: LoadState, // tab 3
    options: LoadState,  // tab 4
    macros: LoadState,   // tab 5
}

/// Async result from background keyboard operations
/// These are sent from spawned tasks to the main event loop
#[allow(dead_code)] // Macros and SetComplete reserved for future use
enum AsyncResult {
    // Device info results
    DeviceIdAndVersion(Result<(u32, monsgeek_keyboard::FirmwareVersion), String>),
    Profile(Result<u8, String>),
    Debounce(Result<u8, String>),
    PollingRate(Result<u16, String>),
    LedParams(Result<LedParams, String>),
    SideLedParams(Result<LedParams, String>),
    KbOptions(Result<KbOptions, String>),
    Precision(Result<Precision, String>),
    SleepTime(Result<SleepTimeSettings, String>),
    FirmwareCheck(FirmwareCheckResult),
    // Other tab results
    Triggers(Result<TriggerSettings, String>),
    Options(Result<KbOptions, String>),
    Macros(Result<Vec<MacroSlot>, String>),
    // Battery status (from keyboard API)
    Battery(Result<BatteryInfo, String>),
    // Operation completion (for set operations)
    SetComplete(String, Result<(), String>), // (field_name, result)
}

/// History length for time series (samples)
const DEPTH_HISTORY_LEN: usize = 100;

// ============================================================================
// Help System - Self-documenting keybindings
// ============================================================================

/// Context in which a keybind is active
#[derive(Debug, Clone, Copy, PartialEq)]
enum KeyContext {
    Global,   // Available everywhere
    Info,     // Device Info tab (0)
    Led,      // LED Settings tab (1)
    Depth,    // Key Depth tab (2)
    Triggers, // Trigger Settings tab (3)
    Macros,   // Macros tab (5)
}

/// A single keybinding definition
struct Keybind {
    keys: &'static str,
    description: &'static str,
    context: KeyContext,
}

/// All TUI keybindings - single source of truth
const TUI_KEYBINDS: &[Keybind] = &[
    // Global keybindings
    Keybind {
        keys: "q / Esc",
        description: "Quit application",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "? / F1",
        description: "Toggle this help",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Tab",
        description: "Next tab",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Shift+Tab",
        description: "Previous tab",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "↑ / k",
        description: "Navigate up",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "↓ / j",
        description: "Navigate down",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "← / h",
        description: "Navigate left / decrease",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "→ / l",
        description: "Navigate right / increase",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "r",
        description: "Refresh device info",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "c",
        description: "Connect to device",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "m",
        description: "Toggle depth monitoring",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Ctrl+1-4",
        description: "Switch profile 1-4",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "PgUp/PgDn",
        description: "Fast scroll (15 items)",
        context: KeyContext::Global,
    },
    // Info tab
    Keybind {
        keys: "u",
        description: "Check for firmware updates",
        context: KeyContext::Info,
    },
    // LED tab
    Keybind {
        keys: "p",
        description: "Apply LED settings",
        context: KeyContext::Led,
    },
    Keybind {
        keys: "Shift+←/→",
        description: "Adjust by ±10",
        context: KeyContext::Led,
    },
    // Depth tab
    Keybind {
        keys: "v",
        description: "Toggle visualization mode",
        context: KeyContext::Depth,
    },
    Keybind {
        keys: "x",
        description: "Clear depth data",
        context: KeyContext::Depth,
    },
    Keybind {
        keys: "Space",
        description: "Pause/resume monitoring",
        context: KeyContext::Depth,
    },
    // Triggers tab
    Keybind {
        keys: "v",
        description: "Toggle list/layout view",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "Enter / e",
        description: "Edit selected key",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "g",
        description: "Edit global (all keys)",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "n / N",
        description: "Normal mode (key/all)",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "t / T",
        description: "RT mode (key/all)",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "d / D",
        description: "DKS mode (key/all)",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "s / S",
        description: "SnapTap mode (key/all)",
        context: KeyContext::Triggers,
    },
    // Macros tab
    Keybind {
        keys: "e",
        description: "Edit selected macro",
        context: KeyContext::Macros,
    },
    Keybind {
        keys: "c",
        description: "Clear selected macro",
        context: KeyContext::Macros,
    },
];

/// Physical keyboard shortcuts from the manual
const KEYBOARD_SHORTCUTS: &[(&str, &str)] = &[
    // Profile switching
    ("Fn+F9", "Profile 1"),
    ("Fn+F10", "Profile 2"),
    ("Fn+F11", "Profile 3"),
    ("Fn+F12", "Profile 4"),
    // LED controls
    ("Fn+\\", "Cycle 7 colors + RGB"),
    ("Fn+↑", "Brightness up"),
    ("Fn+↓", "Brightness down"),
    ("Fn+←", "LED speed down"),
    ("Fn+→", "LED speed up"),
    ("Fn+=", "LED settings"),
    ("Fn+L", "LED mode cycle"),
    ("Fn+Home", "Effect 1-5"),
    ("Fn+PgUp", "Effect 6-10"),
    ("Fn+End", "Effect 11-15"),
    ("Fn+PgDn", "Effect 16-20"),
    // Connection modes
    ("Fn+E/R/T", "Bluetooth 1/2/3 (long=pair)"),
    ("Fn+Y", "2.4GHz mode (long=pair)"),
    // Utility
    ("Fn+Space", "Battery check"),
    ("Fn+W", "WASD/Arrow swap"),
    ("Fn+L_Win", "Win key lock"),
    ("Fn+I", "Insert"),
    ("Fn+P", "Print Screen"),
    ("Fn+C", "Calculator"),
    // Media (Windows)
    ("Fn+F1", "File Explorer"),
    ("Fn+F2", "Mail"),
    ("Fn+F3", "Browser"),
    ("Fn+F4", "Lock PC"),
    ("Fn+F5", "Display off"),
    ("Fn+F6/F8", "Play/Pause"),
    ("Fn+F7", "Volume down"),
    ("Fn+M", "Mute"),
    ("Fn+<", "Volume down"),
    ("Fn+>", "Volume up"),
];

impl App {
    fn new() -> (Self, mpsc::UnboundedReceiver<AsyncResult>) {
        let (result_tx, result_rx) = mpsc::unbounded_channel();
        let app = Self {
            info: DeviceInfo::default(),
            tab: 0,
            selected: 0,
            key_depths: Vec::new(),
            depth_monitoring: false,
            status_msg: String::new(),
            connected: false,
            device_name: String::new(),
            key_count: 0,
            has_sidelight: false,
            has_magnetism: false,
            triggers: None,
            trigger_scroll: 0,
            trigger_view_mode: TriggerViewMode::default(),
            trigger_selected_key: 0,
            precision: Precision::default(),
            options: None,
            sleep_settings: None,
            macros: Vec::new(),
            macro_selected: 0,
            macro_editing: false,
            macro_edit_text: String::new(),
            // Key depth visualization
            depth_view_mode: DepthViewMode::default(),
            depth_history: Vec::new(),
            active_keys: HashSet::new(),
            selected_keys: HashSet::new(),
            depth_cursor: 0,
            max_observed_depth: 0.1, // Will grow as keys are pressed
            depth_last_update: Vec::new(),
            // Battery status
            battery: None,
            battery_source: None,
            last_battery_check: Instant::now(),
            is_wireless: false,
            // Help popup
            show_help: false,
            // Keyboard interface (wrapped in Arc for spawning tasks)
            keyboard: None,
            // Event receiver (subscribed on connect)
            event_rx: None,
            loading: LoadingStates::default(),
            throbber_state: ThrobberState::default(),
            result_tx,
            // Hex color input
            hex_editing: false,
            hex_input: String::new(),
            hex_target: HexColorTarget::default(),
            // Firmware check
            firmware_check: None,
            // Mouse hit areas (updated during render)
            tab_bar_area: StdCell::new(Rect::default()),
            content_area: StdCell::new(Rect::default()),
            // Scroll view state
            scroll_state: ScrollViewState::new(),
            // Trigger edit modal
            trigger_edit_modal: None,
        };
        (app, result_rx)
    }

    async fn connect(&mut self) -> Result<(), String> {
        // Use async device discovery with smart probing
        let discovery = HidDiscovery::new();

        // Probe all devices to find which ones actually respond
        // This handles cases where dongle is plugged in but keyboard is on BT
        let transport = discovery
            .open_preferred()
            .await
            .map_err(|e| format!("Failed to open device: {e}"))?;

        let transport_info = transport.device_info().clone();
        let vid = transport_info.vid;
        let pid = transport_info.pid;

        // Look up device info from database, falling back to hardcoded or defaults
        let (key_count, has_magnetism, has_sidelight, device_name) =
            if let Some(info) = devices::get_device_info(vid, pid) {
                (
                    if info.key_count > 0 {
                        info.key_count
                    } else {
                        98
                    },
                    info.has_magnetism,
                    info.has_sidelight,
                    info.display_name,
                )
            } else {
                // Fallback for unknown devices
                let name = transport_info
                    .product_name
                    .clone()
                    .unwrap_or_else(|| format!("Device {vid:04x}:{pid:04x}"));
                (98, true, false, name) // Default: assume 98 keys, magnetism, no sidelight
            };

        let keyboard = Arc::new(KeyboardInterface::new(transport, key_count, has_magnetism));
        let is_wireless = keyboard.is_wireless();

        // Subscribe to low-latency event notifications
        self.event_rx = keyboard.subscribe_events();

        self.device_name = device_name;
        self.key_count = key_count;
        self.has_sidelight = has_sidelight;
        self.has_magnetism = has_magnetism;
        self.is_wireless = is_wireless;
        self.keyboard = Some(keyboard);

        // Initialize key depths array based on actual key count
        self.key_depths = vec![0.0; self.key_count as usize];
        // Initialize depth history for time series
        self.depth_history =
            vec![VecDeque::with_capacity(DEPTH_HISTORY_LEN); self.key_count as usize];
        // Initialize last update times (set to past so they don't show as active)
        self.depth_last_update = vec![Instant::now(); self.key_count as usize];
        self.active_keys.clear();
        self.selected_keys.clear();

        // Detect battery source (kernel power_supply if eBPF loaded, else vendor)
        if self.is_wireless {
            self.battery_source = if let Some(path) = find_hid_battery_power_supply(vid, pid) {
                Some(BatterySource::Kernel(path))
            } else {
                Some(BatterySource::Vendor)
            };
        }

        self.connected = true;

        // Load battery status immediately for wireless devices
        if self.is_wireless {
            self.refresh_battery();
            // Show warning if keyboard is idle/sleeping
            if self.battery.as_ref().map(|b| b.idle).unwrap_or(false) {
                self.status_msg =
                    "Keyboard sleeping - press a key to wake before querying".to_string();
            } else {
                self.status_msg = format!("Connected to {}", self.device_name);
            }
        } else {
            self.status_msg = format!("Connected to {}", self.device_name);
        }

        Ok(())
    }

    /// Load all device info (all queries for tabs 0/1)
    /// Spawns background tasks to avoid blocking the UI
    fn load_device_info(&mut self) {
        let Some(keyboard) = self.keyboard.clone() else {
            return;
        };

        // Mark all as loading
        self.loading.usb_version = LoadState::Loading;
        self.loading.profile = LoadState::Loading;
        self.loading.debounce = LoadState::Loading;
        self.loading.polling_rate = LoadState::Loading;
        self.loading.led_params = LoadState::Loading;
        self.loading.side_led_params = LoadState::Loading;
        self.loading.kb_options_info = LoadState::Loading;
        self.loading.precision = LoadState::Loading;
        self.loading.sleep_time = LoadState::Loading;

        // Spawn background tasks for each query
        let tx = self.result_tx.clone();

        // Device ID + Version (combined query)
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = match (kb.get_device_id().await, kb.get_version().await) {
                    (Ok(id), Ok(ver)) => Ok((id, ver)),
                    (Err(e), _) | (_, Err(e)) => Err(e.to_string()),
                };
                let _ = tx.send(AsyncResult::DeviceIdAndVersion(result));
            });
        }

        // Profile
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_profile().await.map_err(|e| e.to_string());
                let _ = tx.send(AsyncResult::Profile(result));
            });
        }

        // Debounce
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_debounce().await.map_err(|e| e.to_string());
                let _ = tx.send(AsyncResult::Debounce(result));
            });
        }

        // Polling rate
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb
                    .get_polling_rate()
                    .await
                    .map(|r| r as u16)
                    .map_err(|e| e.to_string());
                let _ = tx.send(AsyncResult::PollingRate(result));
            });
        }

        // LED params
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_led_params().await.map_err(|e| e.to_string());
                let _ = tx.send(AsyncResult::LedParams(result));
            });
        }

        // Side LED params
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_side_led_params().await.map_err(|e| e.to_string());
                let _ = tx.send(AsyncResult::SideLedParams(result));
            });
        }

        // KB options
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_kb_options().await.map_err(|e| e.to_string());
                let _ = tx.send(AsyncResult::KbOptions(result));
            });
        }

        // Feature list
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_precision().await.map_err(|e| e.to_string());
                let _ = tx.send(AsyncResult::Precision(result));
            });
        }

        // Sleep time
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_sleep_time().await.map_err(|e| e.to_string());
                let _ = tx.send(AsyncResult::SleepTime(result));
            });
        }
    }

    /// Load trigger settings (tab 3)
    /// Spawns a background task to avoid blocking the UI
    fn load_triggers(&mut self) {
        let Some(keyboard) = self.keyboard.clone() else {
            return;
        };

        self.loading.triggers = LoadState::Loading;
        let tx = self.result_tx.clone();
        tokio::spawn(async move {
            let result = keyboard
                .get_all_triggers()
                .await
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
            let _ = tx.send(AsyncResult::Triggers(result));
        });
    }

    fn refresh_battery(&mut self) {
        if !self.is_wireless {
            return;
        }

        // Re-detect battery source (allows hot-switching when eBPF loads/unloads)
        self.battery_source = if let Some(path) = find_dongle_battery_power_supply() {
            Some(BatterySource::Kernel(path))
        } else {
            Some(BatterySource::Vendor)
        };

        match &self.battery_source {
            Some(BatterySource::Kernel(path)) => {
                // Read from kernel power_supply sysfs (synchronous, fast)
                self.battery = read_kernel_battery(path);
            }
            Some(BatterySource::Vendor) => {
                // Query battery via keyboard API (async)
                let Some(keyboard) = self.keyboard.clone() else {
                    return;
                };
                let tx = self.result_tx.clone();
                tokio::spawn(async move {
                    let result = keyboard
                        .get_battery()
                        .await
                        .map(|kb_info| BatteryInfo {
                            level: kb_info.level,
                            online: kb_info.online,
                            charging: kb_info.charging,
                            idle: kb_info.idle,
                        })
                        .map_err(|e| e.to_string());
                    let _ = tx.send(AsyncResult::Battery(result));
                });
            }
            None => {}
        }
        self.last_battery_check = Instant::now();
    }

    /// Check for firmware updates from server
    fn check_firmware(&mut self) {
        if !self.connected || self.loading.firmware_check == LoadState::Loading {
            return;
        }

        let device_id = self.info.device_id;
        let local_version = self.info.version;
        let tx = self.result_tx.clone();

        self.loading.firmware_check = LoadState::Loading;
        self.status_msg = "Checking for firmware updates...".to_string();

        tokio::spawn(async move {
            use crate::firmware_api::{check_firmware, ApiError};

            let result = match check_firmware(device_id).await {
                Ok(response) => FirmwareCheckResult::from_response(&response, local_version),
                Err(ApiError::ServerError(500, _)) => {
                    // 500 error means device not in database = no update available
                    FirmwareCheckResult::not_in_database()
                }
                Err(e) => FirmwareCheckResult {
                    server_version: None,
                    has_update: false,
                    download_path: None,
                    message: format!("Check failed: {e}"),
                },
            };

            let _ = tx.send(AsyncResult::FirmwareCheck(result));
        });
    }

    async fn toggle_depth_monitoring(&mut self) {
        self.depth_monitoring = !self.depth_monitoring;
        if let Some(ref keyboard) = self.keyboard {
            if self.depth_monitoring {
                let _ = keyboard.start_magnetism_report().await;
            } else {
                let _ = keyboard.stop_magnetism_report().await;
            }
        }
        self.status_msg = if self.depth_monitoring {
            "Key depth monitoring ENABLED".to_string()
        } else {
            "Key depth monitoring DISABLED".to_string()
        };
    }

    async fn set_led_mode(&mut self, mode: u8) {
        if let Some(ref keyboard) = self.keyboard {
            let _ = keyboard
                .set_led(
                    mode,
                    self.info.led_brightness,
                    4 - self.info.led_speed.min(4),
                    self.info.led_r,
                    self.info.led_g,
                    self.info.led_b,
                    self.info.led_dazzle,
                )
                .await;
        }
        self.info.led_mode = mode;
        self.status_msg = format!("LED mode: {}", cmd::led_mode_name(mode));
    }

    async fn set_brightness(&mut self, brightness: u8) {
        let brightness = brightness.min(4);
        if let Some(ref keyboard) = self.keyboard {
            let _ = keyboard
                .set_led(
                    self.info.led_mode,
                    brightness,
                    4 - self.info.led_speed.min(4),
                    self.info.led_r,
                    self.info.led_g,
                    self.info.led_b,
                    self.info.led_dazzle,
                )
                .await;
        }
        self.info.led_brightness = brightness;
        self.status_msg = format!("Brightness: {brightness}/4");
    }

    async fn set_speed(&mut self, speed: u8) {
        let speed = speed.min(4);
        if let Some(ref keyboard) = self.keyboard {
            let _ = keyboard
                .set_led(
                    self.info.led_mode,
                    self.info.led_brightness,
                    speed,
                    self.info.led_r,
                    self.info.led_g,
                    self.info.led_b,
                    self.info.led_dazzle,
                )
                .await;
        }
        self.info.led_speed = 4 - speed;
        self.status_msg = format!("Speed: {speed}/4");
    }

    async fn set_profile(&mut self, profile: u8) {
        if let Some(ref keyboard) = self.keyboard {
            if keyboard.set_profile(profile).await.is_ok() {
                self.info.profile = profile;
                self.status_msg = format!("Profile {} active", profile + 1);
                // Reload device info after profile switch
                self.load_device_info();
            } else {
                self.status_msg = "Failed to set profile".to_string();
            }
        }
    }

    async fn set_color(&mut self, r: u8, g: u8, b: u8) {
        if let Some(ref keyboard) = self.keyboard {
            let _ = keyboard
                .set_led(
                    self.info.led_mode,
                    self.info.led_brightness,
                    4 - self.info.led_speed.min(4),
                    r,
                    g,
                    b,
                    self.info.led_dazzle,
                )
                .await;
        }
        self.info.led_r = r;
        self.info.led_g = g;
        self.info.led_b = b;
        self.status_msg = format!("Color: #{r:02X}{g:02X}{b:02X}");
    }

    async fn toggle_dazzle(&mut self) {
        let new_dazzle = !self.info.led_dazzle;
        if let Some(ref keyboard) = self.keyboard {
            let _ = keyboard
                .set_led(
                    self.info.led_mode,
                    self.info.led_brightness,
                    4 - self.info.led_speed.min(4),
                    self.info.led_r,
                    self.info.led_g,
                    self.info.led_b,
                    new_dazzle,
                )
                .await;
        }
        self.info.led_dazzle = new_dazzle;
        self.status_msg = format!("Dazzle: {}", if new_dazzle { "ON" } else { "OFF" });
    }

    /// Start hex color input mode
    fn start_hex_input(&mut self, target: HexColorTarget) {
        self.hex_editing = true;
        self.hex_target = target;
        // Pre-fill with current color
        let (r, g, b) = match target {
            HexColorTarget::MainLed => (self.info.led_r, self.info.led_g, self.info.led_b),
            HexColorTarget::SideLed => (self.info.side_r, self.info.side_g, self.info.side_b),
        };
        self.hex_input = format!("{r:02X}{g:02X}{b:02X}");
        self.status_msg = "Type hex color, Enter to apply, Esc to cancel".to_string();
    }

    /// Cancel hex input mode
    fn cancel_hex_input(&mut self) {
        self.hex_editing = false;
        self.hex_input.clear();
        self.status_msg.clear();
    }

    /// Apply hex color input
    async fn apply_hex_input(&mut self) {
        if let Some((r, g, b)) = parse_hex_color(&self.hex_input) {
            match self.hex_target {
                HexColorTarget::MainLed => self.set_color(r, g, b).await,
                HexColorTarget::SideLed => self.set_side_color(r, g, b).await,
            }
        } else {
            self.status_msg = format!("Invalid hex color: {}", self.hex_input);
        }
        self.hex_editing = false;
        self.hex_input.clear();
    }

    // Side LED methods
    async fn set_side_mode(&mut self, mode: u8) {
        if let Some(ref keyboard) = self.keyboard {
            let params = LedParams {
                mode: LedMode::from_u8(mode).unwrap_or(LedMode::Off),
                brightness: self.info.side_brightness,
                speed: 4 - self.info.side_speed.min(4),
                color: RgbColor::new(self.info.side_r, self.info.side_g, self.info.side_b),
                direction: if self.info.side_dazzle { 7 } else { 8 },
            };
            let _ = keyboard.set_side_led_params(&params).await;
        }
        self.info.side_mode = mode;
        self.status_msg = format!("Side LED mode: {}", cmd::led_mode_name(mode));
    }

    async fn set_side_brightness(&mut self, brightness: u8) {
        let brightness = brightness.min(4);
        if let Some(ref keyboard) = self.keyboard {
            let params = LedParams {
                mode: LedMode::from_u8(self.info.side_mode).unwrap_or(LedMode::Off),
                brightness,
                speed: 4 - self.info.side_speed.min(4),
                color: RgbColor::new(self.info.side_r, self.info.side_g, self.info.side_b),
                direction: if self.info.side_dazzle { 7 } else { 8 },
            };
            let _ = keyboard.set_side_led_params(&params).await;
        }
        self.info.side_brightness = brightness;
        self.status_msg = format!("Side brightness: {brightness}/4");
    }

    async fn set_side_speed(&mut self, speed: u8) {
        let speed = speed.min(4);
        if let Some(ref keyboard) = self.keyboard {
            let params = LedParams {
                mode: LedMode::from_u8(self.info.side_mode).unwrap_or(LedMode::Off),
                brightness: self.info.side_brightness,
                speed,
                color: RgbColor::new(self.info.side_r, self.info.side_g, self.info.side_b),
                direction: if self.info.side_dazzle { 7 } else { 8 },
            };
            let _ = keyboard.set_side_led_params(&params).await;
        }
        self.info.side_speed = 4 - speed;
        self.status_msg = format!("Side speed: {speed}/4");
    }

    async fn set_side_color(&mut self, r: u8, g: u8, b: u8) {
        if let Some(ref keyboard) = self.keyboard {
            let params = LedParams {
                mode: LedMode::from_u8(self.info.side_mode).unwrap_or(LedMode::Off),
                brightness: self.info.side_brightness,
                speed: 4 - self.info.side_speed.min(4),
                color: RgbColor::new(r, g, b),
                direction: if self.info.side_dazzle { 7 } else { 8 },
            };
            let _ = keyboard.set_side_led_params(&params).await;
        }
        self.info.side_r = r;
        self.info.side_g = g;
        self.info.side_b = b;
        self.status_msg = format!("Side color: #{r:02X}{g:02X}{b:02X}");
    }

    async fn toggle_side_dazzle(&mut self) {
        let new_dazzle = !self.info.side_dazzle;
        if let Some(ref keyboard) = self.keyboard {
            let params = LedParams {
                mode: LedMode::from_u8(self.info.side_mode).unwrap_or(LedMode::Off),
                brightness: self.info.side_brightness,
                speed: 4 - self.info.side_speed.min(4),
                color: RgbColor::new(self.info.side_r, self.info.side_g, self.info.side_b),
                direction: if new_dazzle { 7 } else { 8 },
            };
            let _ = keyboard.set_side_led_params(&params).await;
        }
        self.info.side_dazzle = new_dazzle;
        self.status_msg = format!("Side dazzle: {}", if new_dazzle { "ON" } else { "OFF" });
    }

    async fn set_all_key_modes(&mut self, mode: u8) {
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
    async fn set_single_key_mode(&mut self, key_index: usize, mode: u8) {
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
        let key_name = get_key_label(key_index);
        self.status_msg = format!(
            "Key {} ({}) set to {}",
            key_index,
            key_name,
            magnetism::mode_name(mode)
        );
    }

    /// Set key mode - dispatches to single or all based on view mode
    async fn set_key_mode(&mut self, mode: u8) {
        if self.trigger_view_mode == TriggerViewMode::Layout {
            self.set_single_key_mode(self.trigger_selected_key, mode)
                .await;
        } else {
            self.set_all_key_modes(mode).await;
        }
    }

    async fn apply_per_key_color(&mut self) {
        let (r, g, b) = (self.info.led_r, self.info.led_g, self.info.led_b);
        if let Some(ref keyboard) = self.keyboard {
            if keyboard
                .set_all_keys_color(RgbColor::new(r, g, b), 0)
                .await
                .is_ok()
            {
                self.info.led_mode = 25; // Per-Key Color mode
                self.status_msg = format!("Per-key color set: #{r:02X}{g:02X}{b:02X}");
            } else {
                self.status_msg = "Failed to set per-key colors".to_string();
            }
        }
    }

    /// Load keyboard options (tab 4)
    /// Spawns a background task to avoid blocking the UI
    fn load_options(&mut self) {
        let Some(keyboard) = self.keyboard.clone() else {
            return;
        };

        self.loading.options = LoadState::Loading;
        let tx = self.result_tx.clone();
        tokio::spawn(async move {
            let result = keyboard.get_kb_options().await.map_err(|e| e.to_string());
            let _ = tx.send(AsyncResult::Options(result));
        });
    }

    async fn save_options(&mut self) {
        if let Some(ref opts) = self.options {
            if let Some(ref keyboard) = self.keyboard {
                let kb_opts = KbOptions {
                    os_mode: opts.os_mode,
                    fn_layer: opts.fn_layer,
                    anti_mistouch: opts.anti_mistouch,
                    rt_stability: opts.rt_stability,
                    wasd_swap: opts.wasd_swap,
                };
                if keyboard.set_kb_options(&kb_opts).await.is_ok() {
                    self.status_msg = "Options saved".to_string();
                } else {
                    self.status_msg = "Failed to save options".to_string();
                }
            }
        }
    }

    async fn set_fn_layer(&mut self, layer: u8) {
        let layer = layer.min(3);
        if let Some(ref mut opts) = self.options {
            opts.fn_layer = layer;
        }
        self.save_options().await;
        self.status_msg = format!("Fn layer: {layer}");
    }

    async fn toggle_wasd_swap(&mut self) {
        let new_val = self.options.as_ref().map(|o| !o.wasd_swap).unwrap_or(false);
        if let Some(ref mut opts) = self.options {
            opts.wasd_swap = new_val;
        }
        self.save_options().await;
        self.status_msg = format!("WASD swap: {}", if new_val { "ON" } else { "OFF" });
    }

    async fn toggle_anti_mistouch(&mut self) {
        let new_val = self
            .options
            .as_ref()
            .map(|o| !o.anti_mistouch)
            .unwrap_or(false);
        if let Some(ref mut opts) = self.options {
            opts.anti_mistouch = new_val;
        }
        self.save_options().await;
        self.status_msg = format!("Anti-mistouch: {}", if new_val { "ON" } else { "OFF" });
    }

    async fn set_rt_stability(&mut self, value: u8) {
        let value = value.min(125);
        if let Some(ref mut opts) = self.options {
            opts.rt_stability = value;
        }
        self.save_options().await;
        self.status_msg = format!("RT stability: {value}ms");
    }

    /// Update a single sleep time value with validation (deep >= idle)
    async fn update_sleep_time(&mut self, field: SleepField, delta: i32) {
        let Some(ref mut opts) = self.options else {
            return;
        };

        // Get current value and compute new value
        let current = match field {
            SleepField::IdleBt => opts.idle_bt,
            SleepField::Idle24g => opts.idle_24g,
            SleepField::DeepBt => opts.deep_bt,
            SleepField::Deep24g => opts.deep_24g,
        };

        // Apply delta with bounds (0 to 3600 seconds = 0 to 1 hour)
        let new_val = (current as i32 + delta).clamp(0, 3600) as u16;

        // Validate: deep sleep must be >= idle sleep for same mode
        // When increasing idle, also increase deep if needed
        // When decreasing deep, also decrease idle if needed
        match field {
            SleepField::IdleBt => {
                opts.idle_bt = new_val;
                if opts.deep_bt < new_val && new_val > 0 {
                    opts.deep_bt = new_val;
                }
            }
            SleepField::Idle24g => {
                opts.idle_24g = new_val;
                if opts.deep_24g < new_val && new_val > 0 {
                    opts.deep_24g = new_val;
                }
            }
            SleepField::DeepBt => {
                // Deep must be >= idle (unless disabled)
                let min_val = if new_val == 0 { 0 } else { opts.idle_bt };
                opts.deep_bt = new_val.max(min_val);
            }
            SleepField::Deep24g => {
                let min_val = if new_val == 0 { 0 } else { opts.idle_24g };
                opts.deep_24g = new_val.max(min_val);
            }
        }

        // Update display value
        self.info.sleep_seconds = opts.idle_bt;

        // Send to keyboard
        if let Some(ref keyboard) = self.keyboard {
            let settings =
                SleepTimeSettings::new(opts.idle_bt, opts.idle_24g, opts.deep_bt, opts.deep_24g);
            if keyboard.set_sleep_time(&settings).await.is_ok() {
                let field_name = match field {
                    SleepField::IdleBt => "BT Idle",
                    SleepField::Idle24g => "2.4G Idle",
                    SleepField::DeepBt => "BT Deep",
                    SleepField::Deep24g => "2.4G Deep",
                };
                let value = match field {
                    SleepField::IdleBt => opts.idle_bt,
                    SleepField::Idle24g => opts.idle_24g,
                    SleepField::DeepBt => opts.deep_bt,
                    SleepField::Deep24g => opts.deep_24g,
                };
                self.status_msg = format!(
                    "{}: {}",
                    field_name,
                    SleepTimeSettings::format_duration(value)
                );
            } else {
                self.status_msg = "Failed to set sleep time".to_string();
            }
        }
    }

    /// Load macros (tab 5) - placeholder, macros not yet implemented
    fn load_macros(&mut self) {
        self.loading.macros = LoadState::Loading;
        // Macros not yet implemented in keyboard interface
        self.macros = vec![MacroSlot::default(); 8];
        self.loading.macros = LoadState::Loaded;
        self.status_msg = format!("Loaded {} macro slots", self.macros.len());
    }

    /// Process async result from background tasks
    fn process_async_result(&mut self, result: AsyncResult) {
        match result {
            AsyncResult::DeviceIdAndVersion(Ok((device_id, ver))) => {
                self.info.device_id = device_id;
                self.info.version = ver.raw;
                self.loading.usb_version = LoadState::Loaded;
            }
            AsyncResult::DeviceIdAndVersion(Err(_)) => {
                self.loading.usb_version = LoadState::Error;
            }
            AsyncResult::Profile(Ok(p)) => {
                self.info.profile = p;
                self.loading.profile = LoadState::Loaded;
            }
            AsyncResult::Profile(Err(_)) => {
                self.loading.profile = LoadState::Error;
            }
            AsyncResult::Debounce(Ok(d)) => {
                self.info.debounce = d;
                self.loading.debounce = LoadState::Loaded;
            }
            AsyncResult::Debounce(Err(_)) => {
                self.loading.debounce = LoadState::Error;
            }
            AsyncResult::PollingRate(Ok(rate)) => {
                self.info.polling_rate = rate;
                self.loading.polling_rate = LoadState::Loaded;
            }
            AsyncResult::PollingRate(Err(_)) => {
                self.loading.polling_rate = LoadState::Error;
            }
            AsyncResult::LedParams(Ok(params)) => {
                self.info.led_mode = params.mode as u8;
                self.info.led_brightness = params.brightness;
                self.info.led_speed = params.speed;
                self.info.led_dazzle = params.direction == 7; // DAZZLE_ON=7
                self.info.led_r = params.color.r;
                self.info.led_g = params.color.g;
                self.info.led_b = params.color.b;
                self.loading.led_params = LoadState::Loaded;
            }
            AsyncResult::LedParams(Err(_)) => {
                self.loading.led_params = LoadState::Error;
            }
            AsyncResult::SideLedParams(Ok(params)) => {
                self.info.side_mode = params.mode as u8;
                self.info.side_brightness = params.brightness;
                self.info.side_speed = params.speed;
                self.info.side_dazzle = params.direction == 7;
                self.info.side_r = params.color.r;
                self.info.side_g = params.color.g;
                self.info.side_b = params.color.b;
                self.loading.side_led_params = LoadState::Loaded;
            }
            AsyncResult::SideLedParams(Err(_)) => {
                self.loading.side_led_params = LoadState::Error;
            }
            AsyncResult::KbOptions(Ok(opts)) => {
                self.info.fn_layer = opts.fn_layer;
                self.info.wasd_swap = opts.wasd_swap;
                self.loading.kb_options_info = LoadState::Loaded;
            }
            AsyncResult::KbOptions(Err(_)) => {
                self.loading.kb_options_info = LoadState::Error;
            }
            AsyncResult::Precision(Ok(precision)) => {
                self.precision = precision;
                self.loading.precision = LoadState::Loaded;
            }
            AsyncResult::Precision(Err(_)) => {
                self.loading.precision = LoadState::Error;
            }
            AsyncResult::SleepTime(Ok(settings)) => {
                // Store full sleep time settings
                self.info.sleep_seconds = settings.idle_bt; // For info display
                self.sleep_settings = Some(settings);
                self.loading.sleep_time = LoadState::Loaded;
                // Update options if already loaded
                if let Some(ref mut opts) = self.options {
                    if let Some(ref s) = self.sleep_settings {
                        opts.idle_bt = s.idle_bt;
                        opts.idle_24g = s.idle_24g;
                        opts.deep_bt = s.deep_bt;
                        opts.deep_24g = s.deep_24g;
                    }
                }
            }
            AsyncResult::SleepTime(Err(_)) => {
                self.loading.sleep_time = LoadState::Error;
            }
            AsyncResult::FirmwareCheck(result) => {
                self.firmware_check = Some(result.clone());
                self.loading.firmware_check = LoadState::Loaded;
                self.status_msg = result.message;
            }
            AsyncResult::Triggers(Ok(triggers)) => {
                self.triggers = Some(triggers);
                self.loading.triggers = LoadState::Loaded;
                self.status_msg = "Trigger settings loaded".to_string();
            }
            AsyncResult::Triggers(Err(_)) => {
                self.loading.triggers = LoadState::Error;
                self.status_msg = "Failed to load trigger settings".to_string();
            }
            AsyncResult::Options(Ok(opts)) => {
                // Get sleep settings if already loaded, otherwise use defaults
                let sleep = self.sleep_settings.unwrap_or_default();
                self.options = Some(KeyboardOptions {
                    os_mode: opts.os_mode,
                    fn_layer: opts.fn_layer,
                    anti_mistouch: opts.anti_mistouch,
                    rt_stability: opts.rt_stability,
                    wasd_swap: opts.wasd_swap,
                    idle_bt: sleep.idle_bt,
                    idle_24g: sleep.idle_24g,
                    deep_bt: sleep.deep_bt,
                    deep_24g: sleep.deep_24g,
                });
                self.loading.options = LoadState::Loaded;
                self.status_msg = "Keyboard options loaded".to_string();
            }
            AsyncResult::Options(Err(_)) => {
                self.loading.options = LoadState::Error;
                self.status_msg = "Failed to load options".to_string();
            }
            AsyncResult::Macros(Ok(macros)) => {
                self.macros = macros;
                self.loading.macros = LoadState::Loaded;
                self.status_msg = format!("Loaded {} macro slots", self.macros.len());
            }
            AsyncResult::Macros(Err(_)) => {
                self.loading.macros = LoadState::Error;
                self.status_msg = "Failed to load macros".to_string();
            }
            AsyncResult::SetComplete(field, Ok(())) => {
                self.status_msg = format!("{field} updated");
            }
            AsyncResult::SetComplete(field, Err(e)) => {
                self.status_msg = format!("Failed to set {field}: {e}");
            }
            // Battery is read synchronously via feature report, not used currently
            AsyncResult::Battery(Ok(info)) => {
                self.battery = Some(info);
            }
            AsyncResult::Battery(Err(e)) => {
                self.status_msg = format!("Battery read failed: {e}");
            }
        }
    }

    /// Get current spinner character for inline display
    fn spinner_char(&self) -> &'static str {
        let idx = self.throbber_state.index() as usize % BRAILLE_SIX.symbols.len();
        BRAILLE_SIX.symbols[idx]
    }

    fn set_macro_text(&mut self, _index: usize, _text: &str, _delay_ms: u8, _repeat: u16) {
        // Macros not yet implemented in keyboard interface
        self.status_msg = "Macro setting not yet implemented".to_string();
    }

    fn clear_macro(&mut self, _index: usize) {
        // Macros not yet implemented in keyboard interface
        self.status_msg = "Macro clearing not yet implemented".to_string();
    }

    fn read_input_reports(&mut self) {
        if !self.depth_monitoring {
            return;
        }

        // Depth events are now handled via handle_vendor_notification from the transport layer.
        // This function handles stale key cleanup and history updates on tick.

        let now = Instant::now();
        // Long timeout as fallback - primary release detection is via depth < 0.05 threshold
        // This only catches truly stale keys (e.g., missed release reports)
        let stale_timeout = Duration::from_secs(2);

        // Reset keys that haven't been updated recently (considered released)
        let stale_keys: Vec<usize> = self
            .active_keys
            .iter()
            .filter(|&&k| {
                k < self.depth_last_update.len()
                    && now.duration_since(self.depth_last_update[k]) > stale_timeout
            })
            .copied()
            .collect();
        for key_idx in stale_keys {
            if key_idx < self.key_depths.len() {
                self.key_depths[key_idx] = 0.0;
            }
            if key_idx < self.depth_history.len() {
                self.depth_history[key_idx].clear();
            }
            self.active_keys.remove(&key_idx);
        }
        // Note: depth_history is now updated in handle_depth_event with real timestamps
    }

    /// Handle a vendor notification and update app state
    fn handle_vendor_notification(&mut self, timestamp: f64, event: VendorEvent) {
        match event {
            VendorEvent::Wake => {
                self.status_msg = "Keyboard wake".to_string();
            }
            VendorEvent::ProfileChange { profile } => {
                self.info.profile = profile;
                self.status_msg = format!("Profile {} (via Fn key)", profile + 1);
            }
            VendorEvent::LedEffectMode { effect_id } => {
                self.info.led_mode = effect_id;
                self.status_msg = format!(
                    "LED mode: {} ({})",
                    effect_id,
                    crate::cmd::led_mode_name(effect_id)
                );
            }
            VendorEvent::LedEffectSpeed { speed } => {
                // Speed is stored inverted (0=fast, 4=slow in protocol)
                self.info.led_speed = 4 - speed.min(4);
                self.status_msg = format!("LED speed: {}/4", speed);
            }
            VendorEvent::BrightnessLevel { level } => {
                self.info.led_brightness = level;
                self.status_msg = format!("Brightness: {}/4", level);
            }
            VendorEvent::LedColor { color } => {
                // Map color index to RGB (7 preset colors + custom)
                let (r, g, b) = match color {
                    0 => (255, 0, 0),     // Red
                    1 => (255, 128, 0),   // Orange
                    2 => (255, 255, 0),   // Yellow
                    3 => (0, 255, 0),     // Green
                    4 => (0, 255, 255),   // Cyan
                    5 => (0, 0, 255),     // Blue
                    6 => (128, 0, 255),   // Purple
                    7 => (255, 255, 255), // White (rainbow/dazzle)
                    _ => (255, 255, 255), // Default white
                };
                self.info.led_r = r;
                self.info.led_g = g;
                self.info.led_b = b;
                // Color index 7 typically means dazzle/rainbow mode
                self.info.led_dazzle = color == 7;
                self.status_msg = format!("LED color: #{:02X}{:02X}{:02X}", r, g, b);
            }
            VendorEvent::WinLockToggle { locked } => {
                self.status_msg =
                    format!("Win key: {}", if locked { "LOCKED" } else { "unlocked" });
            }
            VendorEvent::WasdSwapToggle { swapped } => {
                self.info.wasd_swap = swapped;
                self.status_msg = format!(
                    "WASD/Arrows: {}",
                    if swapped { "SWAPPED" } else { "normal" }
                );
            }
            VendorEvent::BacklightToggle => {
                self.status_msg = "Backlight toggled".to_string();
            }
            VendorEvent::DialModeToggle => {
                self.status_msg = "Dial mode toggled".to_string();
            }
            VendorEvent::FnLayerToggle { layer } => {
                self.status_msg = format!("Fn layer: {}", layer);
            }
            VendorEvent::SettingsAck { started } => {
                // Settings ACK is low-level, only show in debug
                if started {
                    tracing::debug!("Settings change started");
                } else {
                    tracing::debug!("Settings change complete");
                }
            }
            VendorEvent::MagnetismStart | VendorEvent::MagnetismStop => {
                // Handled separately by depth monitoring
            }
            VendorEvent::KeyDepth {
                key_index,
                depth_raw,
            } => {
                self.handle_depth_event(key_index, depth_raw, timestamp);
            }
            VendorEvent::BatteryStatus {
                level,
                charging,
                online,
            } => {
                self.battery = Some(crate::hid::BatteryInfo {
                    level,
                    charging,
                    online,
                    idle: false,
                });
            }
            VendorEvent::UnknownKbFunc { category, action } => {
                self.status_msg = format!("KB func: cat={} action={}", category, action);
            }
            VendorEvent::MouseReport {
                buttons,
                x,
                y,
                wheel,
            } => {
                // Mouse reports from keyboard's built-in mouse function
                tracing::debug!(
                    "Mouse: buttons={:#04x} x={} y={} wheel={}",
                    buttons,
                    x,
                    y,
                    wheel
                );
            }
            VendorEvent::Unknown(data) => {
                tracing::debug!("Unknown notification: {:02X?}", data);
            }
        }
    }

    /// Handle a depth event from the keyboard (coalesced from event loop)
    fn handle_depth_event(&mut self, key_index: u8, depth_raw: u16, timestamp: f64) {
        let precision = self.precision.factor() as f32;
        let depth_mm = depth_raw as f32 / precision;
        let key_index = key_index as usize;

        if key_index < self.key_depths.len() {
            // Update current depth (for bar chart)
            self.key_depths[key_index] = depth_mm;

            // Feed depth to modal if open and matching filter
            if let Some(ref mut modal) = self.trigger_edit_modal {
                let should_sample = match modal.depth_filter {
                    Some(filter_key) => filter_key == key_index,
                    None => true, // No filter = sample all keys (use max)
                };
                if should_sample {
                    modal.push_depth(depth_mm);
                }
            }

            // Update timestamp for this key (for stale detection)
            if key_index < self.depth_last_update.len() {
                self.depth_last_update[key_index] = Instant::now();
            }

            // Track max observed depth for bar chart scaling
            if depth_mm > self.max_observed_depth {
                self.max_observed_depth = depth_mm;
            }

            // Mark key as active when pressed, remove when fully released
            if depth_mm > 0.1 {
                self.active_keys.insert(key_index);
                // Push to timestamped history (for time series)
                if key_index < self.depth_history.len() {
                    let history = &mut self.depth_history[key_index];
                    if history.len() >= DEPTH_HISTORY_LEN {
                        history.pop_front();
                    }
                    history.push_back((timestamp, depth_mm));
                }
            } else if depth_mm < 0.05 {
                // Only remove when depth is very close to rest position
                // This handles the "key released" report from keyboard
                self.active_keys.remove(&key_index);
                // Clear time series history for this key
                if key_index < self.depth_history.len() {
                    self.depth_history[key_index].clear();
                }
            }
            // Note: depths between 0.05-0.1 keep current state (hysteresis)
        }
    }

    /// Toggle view mode for depth tab
    fn toggle_depth_view(&mut self) {
        self.depth_view_mode = match self.depth_view_mode {
            DepthViewMode::BarChart => DepthViewMode::TimeSeries,
            DepthViewMode::TimeSeries => DepthViewMode::BarChart,
        };
        self.status_msg = format!("Depth view: {:?}", self.depth_view_mode);
    }

    /// Toggle selection of a key for time series view
    fn toggle_key_selection(&mut self, key_index: usize) {
        if self.selected_keys.contains(&key_index) {
            self.selected_keys.remove(&key_index);
        } else if self.selected_keys.len() < 8 {
            // Limit to 8 selected keys for readability
            self.selected_keys.insert(key_index);
        }
    }

    /// Clear depth history and active keys
    fn clear_depth_data(&mut self) {
        for history in &mut self.depth_history {
            history.clear();
        }
        self.active_keys.clear();
        for depth in &mut self.key_depths {
            *depth = 0.0;
        }
        self.status_msg = "Depth data cleared".to_string();
    }

    /// Toggle trigger view mode between List and Layout
    fn toggle_trigger_view(&mut self) {
        self.trigger_view_mode = match self.trigger_view_mode {
            TriggerViewMode::List => TriggerViewMode::Layout,
            TriggerViewMode::Layout => TriggerViewMode::List,
        };
        self.status_msg = format!("Trigger view: {:?}", self.trigger_view_mode);
    }

    /// Open trigger edit modal for global settings
    async fn open_trigger_edit_global(&mut self) {
        if let Some(ref triggers) = self.triggers {
            let modal = TriggerEditModal::new_global(triggers, self.precision);
            self.trigger_edit_modal = Some(modal);
            // Enable depth monitoring for the modal
            if !self.depth_monitoring {
                if let Some(ref keyboard) = self.keyboard {
                    let _ = keyboard.start_magnetism_report().await;
                }
                self.depth_monitoring = true;
            }
            self.status_msg = "Editing global triggers (press keys to see depth)".to_string();
        } else {
            self.status_msg = "No trigger data loaded".to_string();
        }
    }

    /// Open trigger edit modal for a specific key
    async fn open_trigger_edit_key(&mut self, key_index: usize) {
        if let Some(ref triggers) = self.triggers {
            let modal = TriggerEditModal::new_per_key(key_index, triggers, self.precision);
            self.trigger_edit_modal = Some(modal);
            // Enable depth monitoring for the modal
            if !self.depth_monitoring {
                if let Some(ref keyboard) = self.keyboard {
                    let _ = keyboard.start_magnetism_report().await;
                }
                self.depth_monitoring = true;
            }
            let key_name = get_key_label(key_index);
            self.status_msg = format!(
                "Editing key {} ({}) - press it to see depth",
                key_index, key_name
            );
        } else {
            self.status_msg = "No trigger data loaded".to_string();
        }
    }

    /// Close trigger edit modal without saving
    fn close_trigger_edit_modal(&mut self) {
        self.trigger_edit_modal = None;
        self.status_msg = "Edit cancelled".to_string();
    }

    /// Save trigger edit modal changes
    async fn save_trigger_edit_modal(&mut self) {
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

                if let Err(e) = keyboard.set_actuation_all_u16(actuation_raw).await {
                    errors.push(format!("actuation: {e}"));
                }
                if let Err(e) = keyboard.set_release_all_u16(release_raw).await {
                    errors.push(format!("release: {e}"));
                }
                if let Err(e) = keyboard.set_rt_press_all_u16(rt_press_raw).await {
                    errors.push(format!("rt_press: {e}"));
                }
                if let Err(e) = keyboard.set_rt_lift_all_u16(rt_lift_raw).await {
                    errors.push(format!("rt_lift: {e}"));
                }
                if let Err(e) = keyboard.set_top_deadzone_all_u16(top_dz_raw).await {
                    errors.push(format!("top_dz: {e}"));
                }
                if let Err(e) = keyboard.set_bottom_deadzone_all_u16(bottom_dz_raw).await {
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
                let settings = monsgeek_keyboard::KeyTriggerSettings {
                    key_index: key_index as u8,
                    actuation: (modal.actuation_mm * 10.0) as u8,
                    deactuation: (modal.release_mm * 10.0) as u8,
                    mode: monsgeek_keyboard::KeyMode::from_u8(modal.mode),
                };

                match keyboard.set_key_trigger(&settings).await {
                    Ok(()) => {
                        let key_name = get_key_label(key_index);
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
    fn layout_key_next(&mut self) {
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
    fn layout_key_prev(&mut self) {
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
    fn layout_key_up(&mut self) {
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
    fn layout_key_down(&mut self) {
        let col = self.trigger_selected_key / 6;
        let row = self.trigger_selected_key % 6;
        if row < 5 {
            let new_pos = col * 6 + (row + 1);
            if new_pos < 126 && self.is_valid_key_position(new_pos) {
                self.trigger_selected_key = new_pos;
            }
        }
    }

    /// Move left one column in keyboard layout
    fn layout_key_left(&mut self) {
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
    fn layout_key_right(&mut self) {
        let col = self.trigger_selected_key / 6;
        let row = self.trigger_selected_key % 6;
        if col < 20 {
            // 21 columns total
            let new_pos = (col + 1) * 6 + row;
            if new_pos < 126 && self.is_valid_key_position(new_pos) {
                self.trigger_selected_key = new_pos;
            }
        }
    }

    /// Check if a matrix position has an active key
    fn is_valid_key_position(&self, pos: usize) -> bool {
        if pos >= 126 {
            return false;
        }
        let name = get_key_label(pos);
        !name.is_empty() && name != "?"
    }
}

/// Parse hex color string (supports #RRGGBB, RRGGBB formats)
fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Run the TUI - called via 'iot_driver tui' command
pub async fn run() -> io::Result<()> {
    use crossterm::event::KeyModifiers;

    // Setup terminal
    enable_raw_mode()?;
    stdout()
        .execute(EnterAlternateScreen)?
        .execute(EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let (mut app, mut result_rx) = App::new();

    // Try to connect
    if let Err(e) = app.connect().await {
        app.status_msg = e;
    } else {
        // Skip loading if keyboard is sleeping (queries will fail/timeout)
        let is_idle = app.battery.as_ref().map(|b| b.idle).unwrap_or(false);
        if !is_idle {
            // Load device info (TUI starts on tab 0) - spawns background tasks
            app.load_device_info();
        }
    }

    // Set up async event stream
    let mut event_stream = EventStream::new();
    let mut tick_interval = tokio::time::interval(Duration::from_millis(100));

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        tokio::select! {
            // Handle async results from background tasks
            Some(result) = result_rx.recv() => {
                app.process_async_result(result);
            }
            // Handle terminal events
            maybe_event = event_stream.next() => {
                if let Some(Ok(Event::Key(key))) = maybe_event {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    // Handle macro editing mode first
                    if app.macro_editing {
                        match key.code {
                            KeyCode::Esc => {
                                app.macro_editing = false;
                                app.macro_edit_text.clear();
                                app.status_msg = "Edit cancelled".to_string();
                            }
                            KeyCode::Enter => {
                                if !app.macro_edit_text.is_empty() {
                                    let text = app.macro_edit_text.clone();
                                    app.set_macro_text(app.macro_selected, &text, 10, 1);
                                }
                                app.macro_editing = false;
                                app.macro_edit_text.clear();
                            }
                            KeyCode::Backspace => {
                                app.macro_edit_text.pop();
                            }
                            KeyCode::Char(c) => {
                                if app.macro_edit_text.len() < 50 {
                                    app.macro_edit_text.push(c);
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Help popup handling
                    if app.show_help {
                        match key.code {
                            KeyCode::Char('?') | KeyCode::Esc | KeyCode::F(1) => {
                                app.show_help = false;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Hex color input mode
                    if app.hex_editing {
                        match key.code {
                            KeyCode::Esc => app.cancel_hex_input(),
                            KeyCode::Enter => app.apply_hex_input().await,
                            KeyCode::Backspace => {
                                app.hex_input.pop();
                            }
                            KeyCode::Char(c) if c.is_ascii_hexdigit() => {
                                if app.hex_input.len() < 6 {
                                    app.hex_input.push(c.to_ascii_uppercase());
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Trigger edit modal - uses spinners with Left/Right to adjust values
                    if app.trigger_edit_modal.is_some() {
                        let coarse = key.modifiers.contains(KeyModifiers::SHIFT);
                        match key.code {
                            KeyCode::Esc => app.close_trigger_edit_modal(),
                            KeyCode::Enter => app.save_trigger_edit_modal().await,
                            KeyCode::Tab | KeyCode::Down => {
                                if let Some(ref mut modal) = app.trigger_edit_modal {
                                    modal.next_field();
                                }
                            }
                            KeyCode::BackTab | KeyCode::Up => {
                                if let Some(ref mut modal) = app.trigger_edit_modal {
                                    modal.prev_field();
                                }
                            }
                            KeyCode::Left | KeyCode::Char('h') => {
                                if let Some(ref mut modal) = app.trigger_edit_modal {
                                    modal.decrement_current(coarse);
                                }
                            }
                            KeyCode::Right | KeyCode::Char('l') => {
                                if let Some(ref mut modal) = app.trigger_edit_modal {
                                    modal.increment_current(coarse);
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }

                    match key.code {
                        // Help toggle
                        KeyCode::Char('?') | KeyCode::F(1) => {
                            app.show_help = true;
                        }
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Tab => {
                            app.tab = (app.tab + 1) % 6;
                            app.selected = 0;
                            app.trigger_scroll = 0;
                            app.scroll_state = ScrollViewState::new();
                            // Auto-load when entering tabs
                            if app.tab == 3 && app.loading.triggers == LoadState::NotLoaded {
                                app.load_triggers();
                            } else if app.tab == 4 && app.loading.options == LoadState::NotLoaded {
                                app.load_options();
                            } else if app.tab == 5 && app.loading.macros == LoadState::NotLoaded {
                                app.load_macros();
                            }
                        }
                        KeyCode::BackTab => {
                            app.tab = if app.tab == 0 { 5 } else { app.tab - 1 };
                            app.selected = 0;
                            app.trigger_scroll = 0;
                            app.scroll_state = ScrollViewState::new();
                            // Auto-load when entering tabs
                            if app.tab == 3 && app.loading.triggers == LoadState::NotLoaded {
                                app.load_triggers();
                            } else if app.tab == 4 && app.loading.options == LoadState::NotLoaded {
                                app.load_options();
                            } else if app.tab == 5 && app.loading.macros == LoadState::NotLoaded {
                                app.load_macros();
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.tab == 2 && app.depth_view_mode == DepthViewMode::BarChart {
                                let row_starts = [0, 15, 30, 43, 56];
                                if let Some(row) = row_starts.iter().rposition(|&s| s <= app.depth_cursor) {
                                    if row > 0 {
                                        let col = app.depth_cursor - row_starts[row];
                                        let prev_row_start = row_starts[row - 1];
                                        let prev_row_size = row_starts[row] - prev_row_start;
                                        app.depth_cursor = prev_row_start + col.min(prev_row_size - 1);
                                    }
                                }
                            } else if app.tab == 3 {
                                if app.trigger_view_mode == TriggerViewMode::Layout {
                                    app.layout_key_up();
                                } else {
                                    // List view: move selection up
                                    if app.trigger_selected_key > 0 {
                                        app.trigger_selected_key -= 1;
                                        // Keep selection visible in scroll window
                                        if app.trigger_selected_key < app.trigger_scroll {
                                            app.trigger_scroll = app.trigger_selected_key;
                                        }
                                    }
                                }
                            } else if app.tab == 5 {
                                if app.macro_selected > 0 {
                                    app.macro_selected -= 1;
                                }
                            } else if app.selected > 0 {
                                app.selected -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.tab == 2 && app.depth_view_mode == DepthViewMode::BarChart {
                                let row_starts = [0, 15, 30, 43, 56, 66];
                                if let Some(row) = row_starts.iter().rposition(|&s| s <= app.depth_cursor) {
                                    if row < 4 {
                                        let col = app.depth_cursor - row_starts[row];
                                        let next_row_start = row_starts[row + 1];
                                        let next_row_size = row_starts[row + 2] - next_row_start;
                                        app.depth_cursor = next_row_start + col.min(next_row_size - 1);
                                    }
                                }
                            } else if app.tab == 3 {
                                if app.trigger_view_mode == TriggerViewMode::Layout {
                                    app.layout_key_down();
                                } else {
                                    // List view: move selection down
                                    let max_key = app.triggers.as_ref()
                                        .map(|t| t.key_modes.len().saturating_sub(1))
                                        .unwrap_or(0);
                                    if app.trigger_selected_key < max_key {
                                        app.trigger_selected_key += 1;
                                        // Keep selection visible in scroll window (assume ~15 visible rows)
                                        let visible_rows = 15usize;
                                        if app.trigger_selected_key >= app.trigger_scroll + visible_rows {
                                            app.trigger_scroll = app.trigger_selected_key.saturating_sub(visible_rows - 1);
                                        }
                                    }
                                }
                            } else if app.tab == 5 {
                                if app.macro_selected < app.macros.len().saturating_sub(1) {
                                    app.macro_selected += 1;
                                }
                            } else {
                                app.selected += 1;
                            }
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            if app.tab == 2 && app.depth_view_mode == DepthViewMode::BarChart {
                                if app.depth_cursor > 0 {
                                    app.depth_cursor -= 1;
                                }
                            } else if app.tab == 3 && app.trigger_view_mode == TriggerViewMode::Layout {
                                app.layout_key_left();
                            } else if app.tab == 1 {
                                let coarse = key.modifiers.contains(KeyModifiers::SHIFT);
                                match app.selected {
                                    0 => app.set_led_mode(app.info.led_mode.saturating_sub(1)).await,
                                    1 => app.set_brightness(BRIGHTNESS_SPINNER.decrement_u8(app.info.led_brightness, coarse)).await,
                                    2 => {
                                        let current = 4 - app.info.led_speed.min(4);
                                        app.set_speed(SPEED_SPINNER.decrement_u8(current, coarse)).await;
                                    }
                                    3 => { let r = RGB_SPINNER.decrement_u8(app.info.led_r, coarse); app.set_color(r, app.info.led_g, app.info.led_b).await; }
                                    4 => { let g = RGB_SPINNER.decrement_u8(app.info.led_g, coarse); app.set_color(app.info.led_r, g, app.info.led_b).await; }
                                    5 => { let b = RGB_SPINNER.decrement_u8(app.info.led_b, coarse); app.set_color(app.info.led_r, app.info.led_g, b).await; }
                                    7 => app.toggle_dazzle().await,
                                    // Side LED controls (only if device has sidelight)
                                    9 if app.has_sidelight => app.set_side_mode(app.info.side_mode.saturating_sub(1)).await,
                                    10 if app.has_sidelight => app.set_side_brightness(BRIGHTNESS_SPINNER.decrement_u8(app.info.side_brightness, coarse)).await,
                                    11 if app.has_sidelight => {
                                        let current = 4 - app.info.side_speed.min(4);
                                        app.set_side_speed(SPEED_SPINNER.decrement_u8(current, coarse)).await;
                                    }
                                    12 if app.has_sidelight => { let r = RGB_SPINNER.decrement_u8(app.info.side_r, coarse); app.set_side_color(r, app.info.side_g, app.info.side_b).await; }
                                    13 if app.has_sidelight => { let g = RGB_SPINNER.decrement_u8(app.info.side_g, coarse); app.set_side_color(app.info.side_r, g, app.info.side_b).await; }
                                    14 if app.has_sidelight => { let b = RGB_SPINNER.decrement_u8(app.info.side_b, coarse); app.set_side_color(app.info.side_r, app.info.side_g, b).await; }
                                    16 if app.has_sidelight => app.toggle_side_dazzle().await,
                                    _ => {}
                                }
                            } else if app.tab == 4 {
                                let coarse = key.modifiers.contains(KeyModifiers::SHIFT);
                                if let Some(ref opts) = app.options.clone() {
                                    match app.selected {
                                        0 => app.set_fn_layer(FN_LAYER_SPINNER.decrement_u8(opts.fn_layer, coarse)).await,
                                        1 => app.toggle_wasd_swap().await,
                                        2 => app.toggle_anti_mistouch().await,
                                        3 => app.set_rt_stability(RT_STABILITY_SPINNER.decrement_u8(opts.rt_stability, coarse)).await,
                                        // Sleep time sliders
                                        4 => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::IdleBt, -step).await; }
                                        5 => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::Idle24g, -step).await; }
                                        6 => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::DeepBt, -step).await; }
                                        7 => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::Deep24g, -step).await; }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            if app.tab == 2 && app.depth_view_mode == DepthViewMode::BarChart {
                                let max_key = app.key_depths.len().min(66).saturating_sub(1);
                                if app.depth_cursor < max_key {
                                    app.depth_cursor += 1;
                                }
                            } else if app.tab == 3 && app.trigger_view_mode == TriggerViewMode::Layout {
                                app.layout_key_right();
                            } else if app.tab == 1 {
                                let coarse = key.modifiers.contains(KeyModifiers::SHIFT);
                                match app.selected {
                                    0 => app.set_led_mode((app.info.led_mode + 1).min(cmd::LED_MODE_MAX)).await,
                                    1 => app.set_brightness(BRIGHTNESS_SPINNER.increment_u8(app.info.led_brightness, coarse)).await,
                                    2 => {
                                        let current = 4 - app.info.led_speed.min(4);
                                        app.set_speed(SPEED_SPINNER.increment_u8(current, coarse)).await;
                                    }
                                    3 => { let r = RGB_SPINNER.increment_u8(app.info.led_r, coarse); app.set_color(r, app.info.led_g, app.info.led_b).await; }
                                    4 => { let g = RGB_SPINNER.increment_u8(app.info.led_g, coarse); app.set_color(app.info.led_r, g, app.info.led_b).await; }
                                    5 => { let b = RGB_SPINNER.increment_u8(app.info.led_b, coarse); app.set_color(app.info.led_r, app.info.led_g, b).await; }
                                    7 => app.toggle_dazzle().await,
                                    // Side LED controls (only if device has sidelight)
                                    9 if app.has_sidelight => app.set_side_mode((app.info.side_mode + 1).min(cmd::LED_MODE_MAX)).await,
                                    10 if app.has_sidelight => app.set_side_brightness(BRIGHTNESS_SPINNER.increment_u8(app.info.side_brightness, coarse)).await,
                                    11 if app.has_sidelight => {
                                        let current = 4 - app.info.side_speed.min(4);
                                        app.set_side_speed(SPEED_SPINNER.increment_u8(current, coarse)).await;
                                    }
                                    12 if app.has_sidelight => { let r = RGB_SPINNER.increment_u8(app.info.side_r, coarse); app.set_side_color(r, app.info.side_g, app.info.side_b).await; }
                                    13 if app.has_sidelight => { let g = RGB_SPINNER.increment_u8(app.info.side_g, coarse); app.set_side_color(app.info.side_r, g, app.info.side_b).await; }
                                    14 if app.has_sidelight => { let b = RGB_SPINNER.increment_u8(app.info.side_b, coarse); app.set_side_color(app.info.side_r, app.info.side_g, b).await; }
                                    16 if app.has_sidelight => app.toggle_side_dazzle().await,
                                    _ => {}
                                }
                            } else if app.tab == 4 {
                                let coarse = key.modifiers.contains(KeyModifiers::SHIFT);
                                if let Some(ref opts) = app.options.clone() {
                                    match app.selected {
                                        0 => app.set_fn_layer(FN_LAYER_SPINNER.increment_u8(opts.fn_layer, coarse)).await,
                                        1 => app.toggle_wasd_swap().await,
                                        2 => app.toggle_anti_mistouch().await,
                                        3 => app.set_rt_stability(RT_STABILITY_SPINNER.increment_u8(opts.rt_stability, coarse)).await,
                                        // Sleep time sliders
                                        4 => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::IdleBt, step).await; }
                                        5 => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::Idle24g, step).await; }
                                        6 => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::DeepBt, step).await; }
                                        7 => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::Deep24g, step).await; }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        KeyCode::Char('r') => {
                            // Re-check battery/idle state before refresh
                            if app.is_wireless {
                                app.refresh_battery();
                            }
                            let is_idle = app.is_wireless && app.battery.as_ref().map(|b| b.idle).unwrap_or(false);
                            if is_idle {
                                app.status_msg = "Keyboard sleeping - press a key to wake before querying".to_string();
                            } else {
                                app.status_msg = "Refreshing...".to_string();
                                app.load_device_info();
                                if app.tab == 3 { app.load_triggers(); }
                                else if app.tab == 4 { app.load_options(); }
                                else if app.tab == 5 { app.load_macros(); }
                            }
                        }
                        KeyCode::Char('u') if app.tab == 0 => {
                            app.check_firmware();
                        }
                        KeyCode::Enter if app.tab == 1 => {
                            if app.selected == 6 {
                                app.start_hex_input(HexColorTarget::MainLed);
                            } else if app.selected == 15 && app.has_sidelight {
                                app.start_hex_input(HexColorTarget::SideLed);
                            }
                        }
                        KeyCode::Char('#') if app.tab == 1 => {
                            if app.selected >= 3 && app.selected <= 6 {
                                app.start_hex_input(HexColorTarget::MainLed);
                            } else if app.has_sidelight && app.selected >= 12 && app.selected <= 15 {
                                app.start_hex_input(HexColorTarget::SideLed);
                            }
                        }
                        KeyCode::Char(c) if app.tab == 1 && (app.selected == 6 || (app.selected == 15 && app.has_sidelight)) && c.is_ascii_hexdigit() => {
                            let target = if app.selected == 6 { HexColorTarget::MainLed } else { HexColorTarget::SideLed };
                            app.start_hex_input(target);
                            app.hex_input.clear();
                            app.hex_input.push(c.to_ascii_uppercase());
                        }
                        KeyCode::Char('e') if app.tab == 5 => {
                            if !app.macros.is_empty() {
                                app.macro_editing = true;
                                app.macro_edit_text.clear();
                                let m = &app.macros[app.macro_selected];
                                if !m.text_preview.is_empty() && !m.text_preview.contains("events") {
                                    app.macro_edit_text = m.text_preview.clone();
                                }
                                app.status_msg = format!("Editing macro {} - type text and press Enter", app.macro_selected);
                            }
                        }
                        KeyCode::Char('c') if app.tab == 5 => {
                            if !app.macros.is_empty() {
                                app.clear_macro(app.macro_selected);
                            }
                        }
                        KeyCode::Char('m') => {
                            app.toggle_depth_monitoring().await;
                        }
                        KeyCode::Char('c') => {
                            if let Err(e) = app.connect().await {
                                app.status_msg = e;
                            } else {
                                // Skip loading if keyboard is sleeping
                                let is_idle = app.battery.as_ref().map(|b| b.idle).unwrap_or(false);
                                if !is_idle {
                                    app.load_device_info();
                                }
                            }
                        }
                        KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::CONTROL) => app.set_profile(0).await,
                        KeyCode::Char('2') if key.modifiers.contains(KeyModifiers::CONTROL) => app.set_profile(1).await,
                        KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::CONTROL) => app.set_profile(2).await,
                        KeyCode::Char('4') if key.modifiers.contains(KeyModifiers::CONTROL) => app.set_profile(3).await,
                        KeyCode::PageUp => {
                            if app.tab == 3 {
                                app.trigger_scroll = app.trigger_scroll.saturating_sub(15);
                            }
                        }
                        KeyCode::PageDown => {
                            if app.tab == 3 {
                                let max_scroll = app.triggers.as_ref()
                                    .map(|t| t.key_modes.len().saturating_sub(15))
                                    .unwrap_or(0);
                                app.trigger_scroll = (app.trigger_scroll + 15).min(max_scroll);
                            }
                        }
                        KeyCode::Char('n') if app.tab == 3 => app.set_key_mode(magnetism::MODE_NORMAL).await,
                        KeyCode::Char('N') if app.tab == 3 => app.set_all_key_modes(magnetism::MODE_NORMAL).await,
                        KeyCode::Char('t') if app.tab == 3 => app.set_key_mode(magnetism::MODE_RAPID_TRIGGER).await,
                        KeyCode::Char('T') if app.tab == 3 => app.set_all_key_modes(magnetism::MODE_RAPID_TRIGGER).await,
                        KeyCode::Char('d') if app.tab == 3 => app.set_key_mode(magnetism::MODE_DKS).await,
                        KeyCode::Char('D') if app.tab == 3 => app.set_all_key_modes(magnetism::MODE_DKS).await,
                        KeyCode::Char('s') if app.tab == 3 => app.set_key_mode(magnetism::MODE_SNAPTAP).await,
                        KeyCode::Char('S') if app.tab == 3 => app.set_all_key_modes(magnetism::MODE_SNAPTAP).await,
                        KeyCode::Char('p') if app.tab == 1 => app.apply_per_key_color().await,
                        KeyCode::Char('v') if app.tab == 2 => app.toggle_depth_view(),
                        KeyCode::Char('v') if app.tab == 3 => app.toggle_trigger_view(),
                        KeyCode::Enter if app.tab == 3 => {
                            // Open trigger edit modal for selected key (both views)
                            app.open_trigger_edit_key(app.trigger_selected_key).await;
                        }
                        KeyCode::Char('e') if app.tab == 3 => {
                            // 'e' also opens edit modal for selected key
                            app.open_trigger_edit_key(app.trigger_selected_key).await;
                        }
                        KeyCode::Char('g') if app.tab == 3 => {
                            // 'g' opens global edit modal
                            app.open_trigger_edit_global().await;
                        }
                        KeyCode::Char('x') if app.tab == 2 => app.clear_depth_data(),
                        KeyCode::Char(' ') if app.tab == 2 => {
                            if app.depth_view_mode == DepthViewMode::BarChart {
                                app.toggle_key_selection(app.depth_cursor);
                                let label = get_key_label(app.depth_cursor);
                                if app.selected_keys.contains(&app.depth_cursor) {
                                    app.status_msg = format!("Selected Key {label} for time series");
                                } else {
                                    app.status_msg = format!("Deselected Key {label}");
                                }
                            }
                        }
                        _ => {}
                    }
                } else if let Some(Ok(Event::Mouse(mouse))) = maybe_event {
                    // Handle mouse events
                    let pos = Position::new(mouse.column, mouse.row);
                    let tab_bar = app.tab_bar_area.get();
                    let content = app.content_area.get();

                    match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        // Check if click is in tab bar area
                        if tab_bar.contains(pos) {
                            // Calculate which tab was clicked
                            // Tabs render with border (1 char), then " Tab1 │ Tab2 │ ..."
                            let tab_names = ["Device Info", "LED Settings", "Key Depth", "Triggers", "Options", "Macros"];
                            let inner_x = mouse.column.saturating_sub(tab_bar.x + 1); // Account for border
                            let mut tab_pos = 1u16; // Initial padding
                            for (i, name) in tab_names.iter().enumerate() {
                                let tab_width = name.len() as u16;
                                if inner_x >= tab_pos && inner_x < tab_pos + tab_width {
                                    let old_tab = app.tab;
                                    app.tab = i;
                                    app.selected = 0;
                                    app.trigger_scroll = 0;
                                    app.scroll_state = ScrollViewState::new();
                                    // Auto-load when entering tabs
                                    if app.tab == 3 && app.loading.triggers == LoadState::NotLoaded {
                                        app.load_triggers();
                                    } else if app.tab == 4 && app.loading.options == LoadState::NotLoaded {
                                        app.load_options();
                                    } else if app.tab == 5 && app.loading.macros == LoadState::NotLoaded {
                                        app.load_macros();
                                    }
                                    if old_tab != app.tab {
                                        app.status_msg = format!("Switched to tab {}", i);
                                    }
                                    break;
                                }
                                tab_pos += tab_width + 3; // Tab width + " │ " separator
                            }
                        }

                        // Check if click is in content area
                        if content.contains(pos) {
                            // Row within content area (accounting for any border)
                            let content_row = (mouse.row.saturating_sub(content.y + 1)) as usize;
                            match app.tab {
                                1 => {
                                    // LED Settings - items in the list
                                    if content_row < 17 {
                                        app.selected = content_row;
                                    }
                                }
                                4 => {
                                    // Options tab - 5 items (0-4)
                                    if content_row < 5 {
                                        app.selected = content_row;
                                    }
                                }
                                5 => {
                                    // Macros tab - select macro slot
                                    if content_row < app.macros.len() {
                                        app.macro_selected = content_row;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    MouseEventKind::ScrollUp if content.contains(pos) => {
                        app.scroll_state.scroll_up();
                    }
                    MouseEventKind::ScrollDown if content.contains(pos) => {
                        app.scroll_state.scroll_down();
                    }
                    _ => {}
                    }
                } else if let Some(Ok(Event::Resize(_, _))) = maybe_event {
                    // Resize is handled automatically by ratatui on next draw
                }
            }

            // Handle keyboard EP2 events - low-latency channel from dedicated reader thread
            // This wakes immediately when events arrive (not tick-based)
            // Event coalescing: drain all pending events before redraw, keeping only latest depth per key
            result = async {
                match &mut app.event_rx {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                // Collect depth events for coalescing (key_index -> (timestamp, depth_raw))
                let mut pending_depths: HashMap<u8, (f64, u16)> = HashMap::new();
                // Collect non-depth events to process after draining
                let mut other_events: Vec<(f64, VendorEvent)> = Vec::new();
                let mut channel_closed = false;

                match result {
                    Ok(ts) => {
                        if let VendorEvent::KeyDepth { key_index, depth_raw } = ts.event {
                            pending_depths.insert(key_index, (ts.timestamp, depth_raw));
                        } else {
                            other_events.push((ts.timestamp, ts.event));
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!("Event receiver lagged by {} events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!("Event channel closed");
                        channel_closed = true;
                    }
                }

                // Drain remaining events without blocking (coalesce depth events by key)
                if !channel_closed {
                    if let Some(ref mut rx) = app.event_rx {
                        loop {
                            match rx.try_recv() {
                                Ok(ts) => {
                                    if let VendorEvent::KeyDepth { key_index, depth_raw } = ts.event {
                                        // Keep only latest depth per key
                                        pending_depths.insert(key_index, (ts.timestamp, depth_raw));
                                    } else {
                                        other_events.push((ts.timestamp, ts.event));
                                    }
                                }
                                Err(broadcast::error::TryRecvError::Empty) => break,
                                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                                    tracing::debug!("Event receiver lagged by {} events (drain)", n);
                                }
                                Err(broadcast::error::TryRecvError::Closed) => {
                                    channel_closed = true;
                                    break;
                                }
                            }
                        }
                    }
                }

                if channel_closed {
                    app.event_rx = None;
                }

                // Process non-depth events first (order preserved)
                for (timestamp, event) in other_events {
                    app.handle_vendor_notification(timestamp, event);
                }

                // Process coalesced depth events (one per key)
                for (key_index, (timestamp, depth_raw)) in pending_depths {
                    app.handle_depth_event(key_index, depth_raw, timestamp);
                }
            }

            // Handle tick updates
            _ = tick_interval.tick() => {
                // Advance spinner animation
                app.throbber_state.calc_next();

                // Read depth reports (handles stale key cleanup internally)
                app.read_input_reports();

                // Refresh battery every 30 seconds for wireless devices
                if app.is_wireless && app.last_battery_check.elapsed() >= Duration::from_secs(30) {
                    let was_idle = app.battery.as_ref().map(|b| b.idle).unwrap_or(false);
                    app.refresh_battery();
                    let is_idle = app.battery.as_ref().map(|b| b.idle).unwrap_or(false);

                    if is_idle {
                        app.status_msg = "Keyboard sleeping - press a key to wake before querying".to_string();
                    } else if was_idle {
                        // Keyboard just woke up - load device info now
                        app.status_msg = "Keyboard awake - loading settings...".to_string();
                        app.load_device_info();
                    }
                }
            }
        }
    }

    // Cleanup - stop magnetism reporting
    if app.depth_monitoring {
        if let Some(ref keyboard) = app.keyboard {
            let _ = keyboard.stop_magnetism_report().await;
        }
    }
    disable_raw_mode()?;
    stdout()
        .execute(DisableMouseCapture)?
        .execute(LeaveAlternateScreen)?;
    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Length(3), // Tabs
            Constraint::Min(10),   // Content
            Constraint::Length(3), // Status bar
        ])
        .split(f.area());

    // Title - show device name if connected, otherwise generic title
    let title_text = if app.connected && !app.device_name.is_empty() {
        format!("{} - Configuration Tool", app.device_name)
    } else {
        "MonsGeek/Akko Keyboard - Configuration Tool".to_string()
    };
    let title = Paragraph::new(title_text)
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Tabs
    let tabs = Tabs::new(vec![
        "Device Info",
        "LED Settings",
        "Key Depth",
        "Triggers",
        "Options",
        "Macros",
    ])
    .select(app.tab)
    .style(Style::default().fg(Color::White))
    .highlight_style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Tabs [Tab/Shift+Tab]"),
    );
    f.render_widget(tabs, chunks[1]);

    // Store areas for mouse hit testing (using interior mutability)
    app.tab_bar_area.set(chunks[1]);
    app.content_area.set(chunks[2]);

    // Content based on tab
    match app.tab {
        0 => render_device_info(f, app, chunks[2]),
        1 => render_led_settings(f, app, chunks[2]),
        2 => render_depth_monitor(f, app, chunks[2]),
        3 => render_trigger_settings(f, app, chunks[2]),
        4 => render_options(f, app, chunks[2]),
        5 => render_macros(f, app, chunks[2]),
        _ => {}
    }

    // Status bar
    let status_color = if app.connected {
        Color::Green
    } else {
        Color::Red
    };
    let conn_status = if app.connected {
        "Connected"
    } else {
        "Disconnected"
    };
    let profile_str = if app.connected {
        format!(" Profile {}", app.info.profile + 1)
    } else {
        String::new()
    };

    // Battery status for wireless devices
    let battery_str = if app.is_wireless {
        if let Some(ref batt) = app.battery {
            let icon = if batt.charging {
                "⚡"
            } else if batt.level > 75 {
                "█"
            } else if batt.level > 50 {
                "▆"
            } else if batt.level > 25 {
                "▃"
            } else {
                "▁"
            };
            // Show source indicator: (k)ernel or (v)endor
            let src = match &app.battery_source {
                Some(BatterySource::Kernel(_)) => "k",
                Some(BatterySource::Vendor) => "v",
                None => "?",
            };
            // Show idle indicator when keyboard is sleeping
            let idle_str = if batt.idle { " zzz" } else { "" };
            format!(" {}{}%({src}){idle_str}", icon, batt.level)
        } else {
            " ?%".to_string()
        }
    } else {
        String::new()
    };

    let monitoring_str = if app.depth_monitoring {
        " MONITORING"
    } else {
        ""
    };

    let status_text = if app.hex_editing {
        format!(
            " [{}{}{}] Enter hex color: #{} | Esc:Cancel Enter:Apply",
            conn_status, profile_str, battery_str, app.hex_input
        )
    } else {
        format!(
            " [{}{}{}] {} | ?:Help q:Quit{}",
            conn_status, profile_str, battery_str, app.status_msg, monitoring_str
        )
    };
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(status_color))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(status, chunks[3]);

    // Help popup (renders on top)
    if app.show_help {
        render_help_popup(f, f.area());
    }

    // Trigger edit modal (renders on top)
    if app.trigger_edit_modal.is_some() {
        render_trigger_edit_modal(f, app, f.area());
    }
}

/// Render help popup with all keybindings
fn render_help_popup(f: &mut Frame, area: Rect) {
    // Calculate popup size (80% width, 80% height)
    let popup_width = (area.width as f32 * 0.85) as u16;
    let popup_height = (area.height as f32 * 0.85) as u16;
    let popup_x = (area.width - popup_width) / 2;
    let popup_y = (area.height - popup_height) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the popup
    f.render_widget(Clear, popup_area);

    // Split into two columns: TUI shortcuts and Keyboard shortcuts
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(popup_area);

    // Left column: TUI Keybindings
    let mut tui_lines: Vec<Line> = vec![Line::from(Span::styled(
        "── Global ──",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];

    let mut current_context = KeyContext::Global;
    for kb in TUI_KEYBINDS {
        if kb.context != current_context {
            current_context = kb.context;
            let section_name = match current_context {
                KeyContext::Global => "Global",
                KeyContext::Info => "Info Tab",
                KeyContext::Led => "LED Tab",
                KeyContext::Depth => "Depth Tab",
                KeyContext::Triggers => "Triggers Tab",
                KeyContext::Macros => "Macros Tab",
            };
            tui_lines.push(Line::from(""));
            tui_lines.push(Line::from(Span::styled(
                format!("── {section_name} ──"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        tui_lines.push(Line::from(vec![
            Span::styled(format!("{:14}", kb.keys), Style::default().fg(Color::Cyan)),
            Span::raw(" "),
            Span::raw(kb.description),
        ]));
    }

    let tui_help = Paragraph::new(tui_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" TUI Shortcuts [? to close] ")
                .title_style(
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(tui_help, columns[0]);

    // Right column: Physical Keyboard Shortcuts
    let mut kb_lines: Vec<Line> = vec![Line::from(Span::styled(
        "── Profiles ──",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];

    let sections = [
        (0, 4, "Profiles"),
        (4, 12, "LED Controls"),
        (12, 14, "Connection"),
        (14, 19, "Utility"),
        (19, 29, "Media (Win)"),
    ];

    for (idx, (start, end, name)) in sections.into_iter().enumerate() {
        if idx > 0 {
            kb_lines.push(Line::from(""));
            kb_lines.push(Line::from(Span::styled(
                format!("── {name} ──"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        for (key, desc) in &KEYBOARD_SHORTCUTS[start..end] {
            kb_lines.push(Line::from(vec![
                Span::styled(format!("{key:14}"), Style::default().fg(Color::Magenta)),
                Span::raw(" "),
                Span::raw(*desc),
            ]));
        }
    }

    let kb_help = Paragraph::new(kb_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Physical Keyboard (Fn+key) ")
                .title_style(
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(kb_help, columns[1]);
}

/// Render trigger edit modal with depth chart
fn render_trigger_edit_modal(f: &mut Frame, app: &App, area: Rect) {
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
            let key_name = get_key_label(key_index);
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

fn render_device_info(f: &mut Frame, app: &mut App, area: Rect) {
    let info = &app.info;
    let loading = &app.loading;
    let spinner = app.spinner_char();

    // Helper to create value span based on loading state
    let value_span = |state: LoadState, value: String, color: Color| -> Span<'static> {
        match state {
            LoadState::NotLoaded => {
                Span::styled("-".to_string(), Style::default().fg(Color::DarkGray))
            }
            LoadState::Loading => {
                Span::styled(spinner.to_string(), Style::default().fg(Color::Yellow))
            }
            LoadState::Loaded => Span::styled(value, Style::default().fg(color)),
            LoadState::Error => Span::styled("!".to_string(), Style::default().fg(Color::Red)),
        }
    };

    let text = vec![
        // Device name and key count are from device definition, not async loaded
        Line::from(vec![
            Span::raw("Device:         "),
            Span::styled(
                app.device_name.clone(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("Key Count:      "),
            Span::styled(
                format!("{}", app.key_count),
                Style::default().fg(Color::Green),
            ),
        ]),
        // USB version info (device_id + version)
        Line::from(vec![
            Span::raw("Device ID:      "),
            value_span(
                loading.usb_version,
                format!("{} (0x{:04X})", info.device_id, info.device_id),
                Color::Yellow,
            ),
        ]),
        Line::from(vec![
            Span::raw("Firmware:       "),
            value_span(
                loading.usb_version,
                format!("v{:X}", info.version),
                Color::Yellow,
            ),
        ]),
        // Firmware update check
        Line::from(vec![
            Span::raw("Update:         "),
            match loading.firmware_check {
                LoadState::NotLoaded => Span::styled(
                    "[u] Check".to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
                LoadState::Loading => {
                    Span::styled(spinner.to_string(), Style::default().fg(Color::Yellow))
                }
                LoadState::Loaded => {
                    if let Some(ref result) = app.firmware_check {
                        let color = if result.has_update {
                            Color::Yellow
                        } else {
                            Color::Green
                        };
                        Span::styled(result.message.clone(), Style::default().fg(color))
                    } else {
                        Span::styled("-".to_string(), Style::default().fg(Color::DarkGray))
                    }
                }
                LoadState::Error => Span::styled("!".to_string(), Style::default().fg(Color::Red)),
            },
        ]),
        // Profile
        Line::from(vec![
            Span::raw("Profile:        "),
            value_span(
                loading.profile,
                format!("{} (1-4)", info.profile + 1),
                Color::Cyan,
            ),
        ]),
        // Debounce
        Line::from(vec![
            Span::raw("Debounce:       "),
            value_span(
                loading.debounce,
                format!("{} ms", info.debounce),
                Color::Cyan,
            ),
        ]),
        // Polling rate
        Line::from(vec![
            Span::raw("Polling Rate:   "),
            value_span(
                loading.polling_rate,
                if info.polling_rate > 0 {
                    crate::protocol::polling_rate::name(info.polling_rate)
                } else {
                    "N/A".to_string()
                },
                Color::Cyan,
            ),
        ]),
        // KB options (fn_layer, wasd_swap)
        Line::from(vec![
            Span::raw("Fn Layer:       "),
            value_span(
                loading.kb_options_info,
                format!("{}", info.fn_layer),
                Color::Cyan,
            ),
        ]),
        Line::from(vec![
            Span::raw("WASD Swap:      "),
            value_span(
                loading.kb_options_info,
                if info.wasd_swap { "Yes" } else { "No" }.to_string(),
                Color::Cyan,
            ),
        ]),
        // Precision
        Line::from(vec![
            Span::raw("Precision:      "),
            value_span(
                loading.precision,
                app.precision.as_str().to_string(),
                Color::Green,
            ),
        ]),
        // Sleep time
        Line::from(vec![
            Span::raw("Sleep:          "),
            value_span(
                loading.sleep_time,
                format!(
                    "{} sec ({} min)",
                    info.sleep_seconds,
                    info.sleep_seconds / 60
                ),
                Color::Cyan,
            ),
        ]),
        Line::from(""),
        // LED params
        Line::from(vec![
            Span::raw("LED Mode:       "),
            value_span(
                loading.led_params,
                format!("{} ({})", info.led_mode, cmd::led_mode_name(info.led_mode)),
                Color::Magenta,
            ),
        ]),
        Line::from(vec![
            Span::raw("LED Color:      "),
            if loading.led_params == LoadState::Loaded {
                Span::styled(
                    format!(
                        "RGB({}, {}, {}) #{:02X}{:02X}{:02X}",
                        info.led_r, info.led_g, info.led_b, info.led_r, info.led_g, info.led_b
                    ),
                    Style::default().fg(Color::Rgb(info.led_r, info.led_g, info.led_b)),
                )
            } else {
                value_span(loading.led_params, String::new(), Color::Magenta)
            },
        ]),
        Line::from(vec![
            Span::raw("Brightness:     "),
            value_span(
                loading.led_params,
                format!("{}/4", info.led_brightness),
                Color::Magenta,
            ),
        ]),
        Line::from(vec![
            Span::raw("Speed:          "),
            value_span(
                loading.led_params,
                format!("{}/4", 4 - info.led_speed.min(4)),
                Color::Magenta,
            ),
        ]),
    ];

    // Render the block border first
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Device Information [r to refresh]");
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    // Create paragraph without block (rendered separately)
    let content_height = text.len() as u16;
    let para = Paragraph::new(text);

    // Use ScrollView for narrow terminals
    let content_size = Size::new(inner_area.width, content_height);
    let mut scroll_view =
        ScrollView::new(content_size).horizontal_scrollbar_visibility(ScrollbarVisibility::Never);
    scroll_view.render_widget(para, Rect::new(0, 0, inner_area.width, content_height));
    f.render_stateful_widget(scroll_view, inner_area, &mut app.scroll_state);
}

fn render_led_settings(f: &mut Frame, app: &mut App, area: Rect) {
    let info = &app.info;
    let speed = 4 - info.led_speed.min(4);

    // Helper to create RGB bar visualization
    let rgb_bar = |val: u8| -> String {
        let bars = (val as usize * 16 / 255).min(16);
        format!("{:3} {}", val, "█".repeat(bars))
    };

    // Build main LED settings items
    let mut items: Vec<ListItem> = vec![
        ListItem::new(Line::from(vec![
            Span::raw("Mode:       "),
            Span::styled(
                format!(
                    "< {} ({}) >",
                    info.led_mode,
                    cmd::led_mode_name(info.led_mode)
                ),
                Style::default().fg(Color::Yellow),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Brightness: "),
            Span::styled(
                format!(
                    "< {}/4 >  {}",
                    info.led_brightness,
                    "█".repeat(info.led_brightness as usize)
                ),
                Style::default().fg(Color::Yellow),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Speed:      "),
            Span::styled(
                format!("< {}/4 >  {}", speed, "█".repeat(speed as usize)),
                Style::default().fg(Color::Yellow),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Red:        "),
            Span::styled(
                format!("< {} >", rgb_bar(info.led_r)),
                Style::default().fg(Color::Red),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Green:      "),
            Span::styled(
                format!("< {} >", rgb_bar(info.led_g)),
                Style::default().fg(Color::Green),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Blue:       "),
            Span::styled(
                format!("< {} >", rgb_bar(info.led_b)),
                Style::default().fg(Color::Blue),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Color:      "),
            if app.hex_editing && app.hex_target == HexColorTarget::MainLed {
                // Show editable textbox
                Span::styled(
                    format!("████████ [#{}_]", app.hex_input),
                    Style::default()
                        .fg(Color::Rgb(info.led_r, info.led_g, info.led_b))
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                // Show static preview
                Span::styled(
                    format!(
                        "████████ [#{:02X}{:02X}{:02X}]",
                        info.led_r, info.led_g, info.led_b
                    ),
                    Style::default().fg(Color::Rgb(info.led_r, info.led_g, info.led_b)),
                )
            },
            Span::styled("  Enter to edit", Style::default().fg(Color::DarkGray)),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Dazzle:     "),
            Span::styled(
                if info.led_dazzle {
                    "< ON (rainbow) >"
                } else {
                    "< OFF >"
                },
                Style::default().fg(if info.led_dazzle {
                    Color::Magenta
                } else {
                    Color::Gray
                }),
            ),
        ])),
    ];

    // Side LED section - only show if device has sidelight
    if app.has_sidelight {
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "─── Side LEDs (Side Lights) ───",
            Style::default().fg(Color::DarkGray),
        )])));
        items.push(ListItem::new(Line::from(vec![
            Span::raw("Mode:       "),
            Span::styled(
                format!(
                    "< {} ({}) >",
                    info.side_mode,
                    cmd::led_mode_name(info.side_mode)
                ),
                Style::default().fg(Color::Cyan),
            ),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::raw("Brightness: "),
            Span::styled(
                format!(
                    "< {}/4 >  {}",
                    info.side_brightness,
                    "█".repeat(info.side_brightness as usize)
                ),
                Style::default().fg(Color::Cyan),
            ),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::raw("Speed:      "),
            Span::styled(
                format!(
                    "< {}/4 >  {}",
                    4 - info.side_speed.min(4),
                    "█".repeat((4 - info.side_speed.min(4)) as usize)
                ),
                Style::default().fg(Color::Cyan),
            ),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::raw("Red:        "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_r)),
                Style::default().fg(Color::Red),
            ),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::raw("Green:      "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_g)),
                Style::default().fg(Color::Green),
            ),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::raw("Blue:       "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_b)),
                Style::default().fg(Color::Blue),
            ),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::raw("Color:      "),
            if app.hex_editing && app.hex_target == HexColorTarget::SideLed {
                // Show editable textbox
                Span::styled(
                    format!("████████ [#{}_]", app.hex_input),
                    Style::default()
                        .fg(Color::Rgb(info.side_r, info.side_g, info.side_b))
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                // Show static preview
                Span::styled(
                    format!(
                        "████████ [#{:02X}{:02X}{:02X}]",
                        info.side_r, info.side_g, info.side_b
                    ),
                    Style::default().fg(Color::Rgb(info.side_r, info.side_g, info.side_b)),
                )
            },
            Span::styled("  Enter to edit", Style::default().fg(Color::DarkGray)),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::raw("Dazzle:     "),
            Span::styled(
                if info.side_dazzle {
                    "< ON (rainbow) >"
                } else {
                    "< OFF >"
                },
                Style::default().fg(if info.side_dazzle {
                    Color::Magenta
                } else {
                    Color::Gray
                }),
            ),
        ])));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("LED Settings [←/→ adjust, ↑/↓ select, p=per-key mode]"),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = ListState::default();
    state.select(Some(app.selected.min(16)));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_depth_monitor(f: &mut Frame, app: &mut App, area: Rect) {
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    // Status and mode indicator
    let mode_str = match app.depth_view_mode {
        DepthViewMode::BarChart => "Bar Chart",
        DepthViewMode::TimeSeries => "Time Series",
    };
    let status_text = if app.depth_monitoring {
        vec![
            Line::from(vec![
                Span::styled(
                    "MONITORING ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("| View: "),
                Span::styled(mode_str, Style::default().fg(Color::Cyan)),
                Span::raw(" | Active keys: "),
                Span::styled(
                    format!("{}", app.active_keys.len()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(" | Selected: "),
                Span::styled(
                    format!("{}", app.selected_keys.len()),
                    Style::default().fg(Color::Magenta),
                ),
            ]),
            Line::from("Press 'v' to switch view, Space to select key, 'x' to clear data"),
        ]
    } else {
        vec![
            Line::from(Span::styled(
                "Key depth monitoring is OFF - press 'm' to start",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
        ]
    };
    let status = Paragraph::new(status_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Monitor Status"),
    );
    f.render_widget(status, inner[0]);

    // Main visualization area
    match app.depth_view_mode {
        DepthViewMode::BarChart => render_depth_bar_chart(f, app, inner[1]),
        DepthViewMode::TimeSeries => render_depth_time_series(f, app, inner[1]),
    }

    // Help bar
    let help_text = if app.depth_monitoring {
        match app.depth_view_mode {
            DepthViewMode::BarChart => "m:Stop  v:TimeSeries  ↑↓←→:Navigate  Space:Select  x:Clear",
            DepthViewMode::TimeSeries => "m:Stop  v:BarChart  Space:Deselect  x:Clear",
        }
    } else {
        "m:Start monitoring  v:Switch view"
    };
    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(help, inner[2]);
}

/// Get key label for display - use device profile matrix key names
fn get_key_label(index: usize) -> String {
    use crate::profile::builtin::M1V5HeProfile;
    use crate::profile::DeviceProfile;

    // Use builtin profile for key name lookup
    static PROFILE: std::sync::OnceLock<M1V5HeProfile> = std::sync::OnceLock::new();
    let profile = PROFILE.get_or_init(M1V5HeProfile::new);
    profile.matrix_key_name(index as u8).to_string()
}

fn render_depth_bar_chart(f: &mut Frame, app: &mut App, area: Rect) {
    // Use max observed depth for consistent scaling (minimum 0.1mm)
    let max_depth = app.max_observed_depth.max(0.1);

    // Show all keys with non-zero depth as a single row of bars
    // Use raw depth values (scaled to u64 for display) - BarChart handles scaling via .max()
    let mut bar_data: Vec<(String, u64)> = Vec::new();

    for (i, &depth) in app.key_depths.iter().enumerate() {
        if depth > 0.01 || app.active_keys.contains(&i) {
            // Convert mm to 0.01mm units for integer display
            let depth_raw = (depth * 100.0) as u64;
            let label = get_key_label(i);
            bar_data.push((label, depth_raw));
        }
    }

    // If no active keys, show placeholder
    if bar_data.is_empty() {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "No keys pressed",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from("Press keys to see their depth"),
        ];
        let para = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Key Depths (max: {max_depth:.1}mm)")),
        );
        f.render_widget(para, area);
        return;
    }

    // Convert to references for BarChart
    let bar_refs: Vec<(&str, u64)> = bar_data.iter().map(|(s, v)| (s.as_str(), *v)).collect();

    // Max in same units as data (0.01mm)
    let max_raw = (max_depth * 100.0) as u64;

    let chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Key Depths (max: {max_depth:.2}mm)")),
        )
        .max(max_raw)
        .bar_width(3)
        .bar_gap(1)
        .bar_style(Style::default().fg(Color::Cyan))
        .value_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::DIM),
        )
        .data(&bar_refs);

    f.render_widget(chart, area);
}

fn render_depth_time_series(f: &mut Frame, app: &mut App, area: Rect) {
    // Find all keys with history data (any non-empty history)
    let mut active_keys: Vec<usize> = app
        .depth_history
        .iter()
        .enumerate()
        .filter(|(_, h)| !h.is_empty())
        .map(|(i, _)| i)
        .collect();
    active_keys.sort();

    // Limit to first 8 keys for readability
    active_keys.truncate(8);

    if active_keys.is_empty() {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "No key activity recorded yet",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from("Press keys while monitoring to see their depth over time"),
        ];
        let para = Paragraph::new(text)
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title("Time Series"));
        f.render_widget(para, area);
        return;
    }

    // Colors for different keys
    let colors = [
        Color::Cyan,
        Color::Yellow,
        Color::Green,
        Color::Magenta,
        Color::Red,
        Color::Blue,
        Color::LightCyan,
        Color::LightYellow,
    ];

    // Calculate time bounds from all histories (timestamps are in seconds since start)
    let time_window = 5.0; // Show 5 seconds of history
    let mut time_min = f64::MAX;
    let mut time_max = f64::MIN;
    for &key_idx in &active_keys {
        if let Some(history) = app.depth_history.get(key_idx) {
            for &(t, _) in history.iter() {
                if t < time_min {
                    time_min = t;
                }
                if t > time_max {
                    time_max = t;
                }
            }
        }
    }

    // If no valid timestamps, show empty
    if time_min == f64::MAX || time_max == f64::MIN {
        time_min = 0.0;
        time_max = time_window;
    }

    // Show a rolling window of the last N seconds
    let x_max = time_max;
    let x_min = (time_max - time_window).max(time_min);

    // Build datasets for Chart widget
    let mut datasets: Vec<Dataset> = Vec::new();
    let mut all_data: Vec<Vec<(f64, f64)>> = Vec::new();

    for (color_idx, &key_idx) in active_keys.iter().enumerate() {
        if key_idx < app.depth_history.len() {
            let history = &app.depth_history[key_idx];
            // Use actual timestamps as X-axis, filter to visible window
            let data: Vec<(f64, f64)> = history
                .iter()
                .filter(|(t, _)| *t >= x_min)
                .map(|&(t, depth)| (t, depth as f64))
                .collect();
            all_data.push(data);

            let color = colors[color_idx % colors.len()];
            let label = get_key_label(key_idx);
            datasets.push(
                Dataset::default()
                    .name(label)
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(color)),
            );
        }
    }

    // Set data references
    let datasets: Vec<Dataset> = datasets
        .into_iter()
        .zip(all_data.iter())
        .map(|(ds, data)| ds.data(data))
        .collect();

    // Build legend string
    let legend: String = active_keys
        .iter()
        .enumerate()
        .map(|(i, &k)| {
            let color_char = match i {
                0 => "C",
                1 => "Y",
                2 => "G",
                3 => "M",
                4 => "R",
                5 => "B",
                6 => "c",
                7 => "y",
                _ => "?",
            };
            format!("[{}]K{}", color_char, get_key_label(k))
        })
        .collect::<Vec<_>>()
        .join(" ");

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Time Series: {legend}")),
        )
        .x_axis(
            Axis::default()
                .title("Time (s)")
                .style(Style::default().fg(Color::Gray))
                .bounds([x_min, x_max])
                .labels(vec![
                    Span::raw(format!("{:.1}", x_min)),
                    Span::raw(format!("{:.1}", (x_min + x_max) / 2.0)),
                    Span::raw(format!("{:.1}", x_max)),
                ]),
        )
        .y_axis(
            Axis::default()
                .title("mm")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, 4.5])
                .labels(vec![
                    Span::raw("0"),
                    Span::raw("1"),
                    Span::raw("2"),
                    Span::raw("3"),
                    Span::raw("4"),
                ]),
        );

    f.render_widget(chart, area);
}

fn render_trigger_settings(f: &mut Frame, app: &mut App, area: Rect) {
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
        // Decode first key values
        let decode_u16 = |data: &[u8], idx: usize| -> u16 {
            if idx * 2 + 1 < data.len() {
                u16::from_le_bytes([data[idx * 2], data[idx * 2 + 1]])
            } else {
                0
            }
        };
        let first_press = decode_u16(&triggers.press_travel, 0);
        let first_lift = decode_u16(&triggers.lift_travel, 0);
        let first_rt_press = decode_u16(&triggers.rt_press, 0);
        let first_rt_lift = decode_u16(&triggers.rt_lift, 0);
        let first_mode = triggers.key_modes.first().copied().unwrap_or(0);
        let num_keys = triggers
            .key_modes
            .len()
            .min(triggers.press_travel.len() / 2);

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
        let decode_u16 = |data: &[u8], idx: usize| -> u16 {
            if idx * 2 + 1 < data.len() {
                u16::from_le_bytes([data[idx * 2], data[idx * 2 + 1]])
            } else {
                0
            }
        };

        let num_keys = triggers
            .key_modes
            .len()
            .min(triggers.press_travel.len() / 2);

        // Build ALL rows for table (ScrollView handles viewport)
        let selected_key = app.trigger_selected_key;
        let rows: Vec<Row> = (0..num_keys)
            .map(|i| {
                let press = decode_u16(&triggers.press_travel, i);
                let lift = decode_u16(&triggers.lift_travel, i);
                let rt_p = decode_u16(&triggers.rt_press, i);
                let rt_l = decode_u16(&triggers.rt_lift, i);
                let mode = triggers.key_modes.get(i).copied().unwrap_or(0);
                let key_name = get_key_label(i);
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
    for pos in 0..126 {
        let col = pos / 6;
        let row = pos % 6;

        // Skip positions outside visible area or empty keys
        let key_name = get_key_label(pos);
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
        let key_name = get_key_label(pos);

        let decode_u16 = |data: &[u8], idx: usize| -> u16 {
            if idx * 2 + 1 < data.len() {
                u16::from_le_bytes([data[idx * 2], data[idx * 2 + 1]])
            } else {
                0
            }
        };

        let press = decode_u16(&triggers.press_travel, pos);
        let lift = decode_u16(&triggers.lift_travel, pos);
        let rt_press = decode_u16(&triggers.rt_press, pos);
        let rt_lift = decode_u16(&triggers.rt_lift, pos);
        let mode = triggers.key_modes.get(pos).copied().unwrap_or(0);
        let bottom_dz = decode_u16(&triggers.bottom_deadzone, pos);
        let top_dz = decode_u16(&triggers.top_deadzone, pos);

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

fn render_options(f: &mut Frame, app: &mut App, area: Rect) {
    if let Some(ref opts) = app.options {
        let os_mode_str = match opts.os_mode {
            0 => "Windows",
            1 => "macOS",
            2 => "Linux",
            _ => "Unknown",
        };

        let items: Vec<ListItem> = vec![
            ListItem::new(Line::from(vec![
                Span::raw("Fn Layer:       "),
                Span::styled(
                    format!("< {} >", opts.fn_layer),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled("  (0-3)", Style::default().fg(Color::DarkGray)),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("WASD Swap:      "),
                Span::styled(
                    if opts.wasd_swap { "< ON >" } else { "< OFF >" },
                    Style::default().fg(if opts.wasd_swap {
                        Color::Green
                    } else {
                        Color::Gray
                    }),
                ),
                Span::styled(
                    "  (swap WASD/Arrow keys)",
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("Anti-Mistouch:  "),
                Span::styled(
                    if opts.anti_mistouch {
                        "< ON >"
                    } else {
                        "< OFF >"
                    },
                    Style::default().fg(if opts.anti_mistouch {
                        Color::Green
                    } else {
                        Color::Gray
                    }),
                ),
                Span::styled(
                    "  (prevent accidental key presses)",
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("RT Stability:   "),
                Span::styled(
                    format!("< {}ms >", opts.rt_stability),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    "  (0-125ms, delay for stability)",
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
            // Sleep time settings (4 sliders)
            ListItem::new(Line::from(vec![
                Span::raw("BT Idle:        "),
                Span::styled(
                    format!("< {} >", SleepTimeSettings::format_duration(opts.idle_bt)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    "  (Bluetooth idle timeout)",
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("2.4G Idle:      "),
                Span::styled(
                    format!("< {} >", SleepTimeSettings::format_duration(opts.idle_24g)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    "  (2.4GHz idle timeout)",
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("BT Deep Sleep:  "),
                Span::styled(
                    format!("< {} >", SleepTimeSettings::format_duration(opts.deep_bt)),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    "  (must be ≥ BT idle)",
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("2.4G Deep Sleep:"),
                Span::styled(
                    format!("< {} >", SleepTimeSettings::format_duration(opts.deep_24g)),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    "  (must be ≥ 2.4G idle)",
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
            ListItem::new(Line::from("")),
            ListItem::new(Line::from(vec![Span::styled(
                "Read-Only Info:",
                Style::default().add_modifier(Modifier::BOLD),
            )])),
            ListItem::new(Line::from(vec![
                Span::raw("OS Mode:        "),
                Span::styled(os_mode_str, Style::default().fg(Color::Magenta)),
            ])),
        ];

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Keyboard Options [←/→ adjust, ↑/↓ select, Enter to toggle]"),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        let mut state = ListState::default();
        state.select(Some(app.selected.min(7))); // 8 items: FN, WASD, AntiMis, RT, 4x sleep
        f.render_stateful_widget(list, area, &mut state);
    } else if app.loading.options == LoadState::Loading {
        // Show loading spinner
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Keyboard Options");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let throbber = Throbber::default()
            .label("Loading options...")
            .throbber_style(Style::default().fg(Color::Yellow));
        f.render_stateful_widget(throbber, inner, &mut app.throbber_state.clone());
    } else {
        let msg = if app.loading.options == LoadState::Error {
            "Failed to load options"
        } else {
            "No options loaded"
        };
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(msg, Style::default().fg(Color::Red))),
            Line::from(""),
            Line::from("Press 'r' to load keyboard options from device"),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Keyboard Options"),
        );
        f.render_widget(help, area);
    }
}

fn render_macros(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::protocol::hid::key_name;

    // Check loading state first
    if app.loading.macros == LoadState::Loading {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Macros [e: edit, c: clear, ↑/↓: select]");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let throbber = Throbber::default()
            .label("Loading macros...")
            .throbber_style(Style::default().fg(Color::Yellow));
        f.render_stateful_widget(throbber, inner, &mut app.throbber_state.clone());
        return;
    }

    // Split into macro list (left) and detail/edit area (right)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Left panel: Macro list
    if app.macros.is_empty() {
        let msg = if app.loading.macros == LoadState::Error {
            "Failed to load macros"
        } else {
            "No macros loaded"
        };
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(msg, Style::default().fg(Color::Yellow))),
            Line::from(""),
            Line::from("Press 'r' to load macros from device"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Macros"));
        f.render_widget(help, chunks[0]);
    } else {
        let items: Vec<ListItem> = app
            .macros
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let status = if m.events.is_empty() {
                    Span::styled("(empty)", Style::default().fg(Color::DarkGray))
                } else {
                    Span::styled(
                        format!("{} (x{})", &m.text_preview, m.repeat_count),
                        Style::default().fg(Color::Green),
                    )
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("M{i}: "), Style::default().fg(Color::Cyan)),
                    status,
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Macros [↑/↓ select]"),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        let mut state = ListState::default();
        state.select(Some(
            app.macro_selected.min(app.macros.len().saturating_sub(1)),
        ));
        f.render_stateful_widget(list, chunks[0], &mut state);
    }

    // Right panel: Detail view or edit mode
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(7)])
        .split(chunks[1]);

    if app.macro_editing {
        // Edit mode
        let edit_text = Paragraph::new(vec![
            Line::from(""),
            Line::from(vec![
                Span::raw("Text: "),
                Span::styled(
                    &app.macro_edit_text,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("█", Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Type your macro text, then press Enter to save",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "Press Escape to cancel",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Edit Macro (typing mode)"),
        );
        f.render_widget(edit_text, right_chunks[0]);
    } else if !app.macros.is_empty() && app.macro_selected < app.macros.len() {
        // Detail view
        let m = &app.macros[app.macro_selected];
        let mut lines = vec![
            Line::from(vec![Span::styled(
                format!("Macro {}", app.macro_selected),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::raw("Repeat: "),
                Span::styled(
                    format!("{}", m.repeat_count),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(vec![
                Span::raw("Events: "),
                Span::styled(
                    format!("{}", m.events.len()),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(""),
        ];

        // Show events (up to 10)
        if !m.events.is_empty() {
            lines.push(Line::from(Span::styled(
                "Events:",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            for (i, evt) in m.events.iter().take(12).enumerate() {
                let arrow = if evt.is_down { "↓" } else { "↑" };
                let delay_str = if evt.delay_ms > 0 {
                    format!(" +{}ms", evt.delay_ms)
                } else {
                    String::new()
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{i:2}: "), Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        arrow,
                        Style::default().fg(if evt.is_down {
                            Color::Green
                        } else {
                            Color::Red
                        }),
                    ),
                    Span::raw(" "),
                    Span::styled(key_name(evt.keycode), Style::default().fg(Color::Yellow)),
                    Span::styled(delay_str, Style::default().fg(Color::DarkGray)),
                ]));
            }
            if m.events.len() > 12 {
                lines.push(Line::from(Span::styled(
                    format!("... and {} more", m.events.len() - 12),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "(empty)",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let detail = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Macro Details"),
        );
        f.render_widget(detail, right_chunks[0]);
    } else {
        let empty = Paragraph::new("Select a macro").block(
            Block::default()
                .borders(Borders::ALL)
                .title("Macro Details"),
        );
        f.render_widget(empty, right_chunks[0]);
    }

    // Help text at bottom
    let help_lines = if app.macro_editing {
        vec![Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(" Save  "),
            Span::styled("Escape", Style::default().fg(Color::Yellow)),
            Span::raw(" Cancel"),
        ])]
    } else {
        vec![Line::from(vec![
            Span::styled("e", Style::default().fg(Color::Yellow)),
            Span::raw(" Edit macro  "),
            Span::styled("c", Style::default().fg(Color::Yellow)),
            Span::raw(" Clear macro  "),
            Span::styled("r", Style::default().fg(Color::Yellow)),
            Span::raw(" Refresh"),
        ])]
    };
    let help = Paragraph::new(help_lines)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title("Keys"));
    f.render_widget(help, right_chunks[1]);
}
