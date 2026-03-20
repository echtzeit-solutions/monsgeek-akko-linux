// Shared types used across multiple TUI tabs

use std::path::PathBuf;

use tokio::sync::mpsc;

use crate::firmware_api::FirmwareCheckResult;
use crate::hid::BatteryInfo;
use crate::keymap::KeyEntry;
use crate::TriggerSettings;
use monsgeek_keyboard::{KeyboardOptions as KbOptions, LedParams, Precision, SleepTimeSettings};
use monsgeek_transport::TransportType;

#[cfg(feature = "notify")]
use crate::effect::EffectLibrary;

/// Map TransportType to a short display name
pub(crate) fn transport_type_name(tt: TransportType) -> &'static str {
    match tt {
        TransportType::HidWired => "usb",
        TransportType::HidDongle => "dongle",
        TransportType::Bluetooth => "bt",
        TransportType::WebRtc => "webrtc",
    }
}

/// Battery data source
#[derive(Debug, Clone)]
pub(crate) enum BatterySource {
    /// Kernel power_supply sysfs (via eBPF filter)
    Kernel(PathBuf),
    /// Direct vendor protocol (HID feature report)
    Vendor,
}

/// Parsed patch info for display
#[derive(Debug, Clone)]
pub(crate) struct PatchInfoData {
    /// Patch name (e.g. "MONSMOD")
    pub name: String,
    /// Patch version
    pub version: u8,
    /// Capability names (e.g. ["battery", "led_stream"])
    pub capabilities: Vec<&'static str>,
}

/// Keyboard options state
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyboardOptions {
    pub os_mode: u8,
    pub fn_layer: u8,
    pub anti_mistouch: bool,
    pub rt_stability: u8,
    pub wasd_swap: bool,
    // Sleep time settings (all in seconds, 0 = disabled)
    pub idle_bt: u16,
    pub idle_24g: u16,
    pub deep_bt: u16,
    pub deep_24g: u16,
}

/// Sleep time field identifier for updates
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SleepField {
    IdleBt,
    Idle24g,
    DeepBt,
    Deep24g,
}

/// Key depth visualization mode
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum DepthViewMode {
    #[default]
    BarChart, // Bar chart of all active keys
    TimeSeries, // Time series graph of selected keys
}

/// Trigger settings view mode
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum TriggerViewMode {
    #[default]
    List, // Scrollable list of keys
    Layout, // Visual keyboard layout
}

/// Configuration for a spinner (numeric value with left/right adjustment)
/// Reusable across different tabs and modals
#[derive(Debug, Clone, Copy)]
pub(crate) struct SpinnerConfig {
    /// Minimum value
    pub min: f32,
    /// Maximum value
    pub max: f32,
    /// Step size for normal adjustment
    pub step: f32,
    /// Step size when shift is held (coarse adjustment)
    pub step_coarse: f32,
    /// Number of decimal places to display
    pub decimals: u8,
    /// Unit suffix (e.g., "mm", "%", "")
    pub unit: &'static str,
}

impl SpinnerConfig {
    /// Increment value by step (or coarse step if shift held)
    pub fn increment(&self, value: f32, coarse: bool) -> f32 {
        let step = if coarse { self.step_coarse } else { self.step };
        (value + step).min(self.max)
    }

    /// Decrement value by step (or coarse step if shift held)
    pub fn decrement(&self, value: f32, coarse: bool) -> f32 {
        let step = if coarse { self.step_coarse } else { self.step };
        (value - step).max(self.min)
    }

    /// Increment u8 value (for RGB components)
    pub fn increment_u8(&self, value: u8, coarse: bool) -> u8 {
        let step = if coarse { self.step_coarse } else { self.step } as u8;
        value.saturating_add(step).min(self.max as u8)
    }

    /// Decrement u8 value (for RGB components)
    pub fn decrement_u8(&self, value: u8, coarse: bool) -> u8 {
        let step = if coarse { self.step_coarse } else { self.step } as u8;
        value.saturating_sub(step).max(self.min as u8)
    }

    /// Format value for display
    pub fn format(&self, value: f32) -> String {
        match self.decimals {
            0 => format!("{:.0}", value),
            1 => format!("{:.1}", value),
            _ => format!("{:.2}", value),
        }
    }
}

/// Spinner config for RGB color components (0-255)
pub(crate) const RGB_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 255.0,
    step: 1.0,
    step_coarse: 10.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for LED brightness (0-4)
pub(crate) const BRIGHTNESS_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 4.0,
    step: 1.0,
    step_coarse: 1.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for LED speed (0-4)
pub(crate) const SPEED_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 4.0,
    step: 1.0,
    step_coarse: 1.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for debounce (0-25, step 1, coarse 5)
pub(crate) const DEBOUNCE_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 25.0,
    step: 1.0,
    step_coarse: 5.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for profile (0-3)
pub(crate) const PROFILE_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 3.0,
    step: 1.0,
    step_coarse: 1.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for Fn layer (0-3)
pub(crate) const FN_LAYER_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 3.0,
    step: 1.0,
    step_coarse: 1.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for RT stability (0-125, step 25)
pub(crate) const RT_STABILITY_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 125.0,
    step: 25.0,
    step_coarse: 25.0,
    decimals: 0,
    unit: "",
};

/// Spinner config for sleep time in seconds (0-3600, step 60s, coarse 300s)
pub(crate) const SLEEP_TIME_SPINNER: SpinnerConfig = SpinnerConfig {
    min: 0.0,
    max: 3600.0,
    step: 60.0,
    step_coarse: 300.0,
    decimals: 0,
    unit: "s",
};

/// Loading state for async data fetching
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum LoadState {
    #[default]
    NotLoaded,
    Loading,
    Loaded,
    Error,
}

/// Track loading state per HID query group
#[derive(Debug, Clone, Default)]
pub(crate) struct LoadingStates {
    // Device info queries (tab 0/1)
    pub usb_version: LoadState, // device_id + version
    pub profile: LoadState,
    pub debounce: LoadState,
    pub polling_rate: LoadState,
    pub led_params: LoadState, // all main LED fields
    pub side_led_params: LoadState,
    pub kb_options_info: LoadState, // fn_layer + wasd_swap for info display
    pub precision: LoadState,
    pub sleep_time: LoadState,
    pub patch_info: LoadState,
    pub dongle_patch_info: LoadState,
    pub firmware_check: LoadState, // server firmware version check
    // Other tabs
    pub triggers: LoadState, // tab 3
    pub options: LoadState,  // tab 4
    pub remaps: LoadState,   // tab 5 (remap list)
    pub macros: LoadState,   // macro slots (loaded from remap tab)
}

/// Macro slot data
#[derive(Debug, Clone, Default)]
pub(crate) struct MacroSlot {
    pub events: Vec<MacroEvent>,
    pub repeat_count: u16,
    pub text_preview: String,
}

/// Single macro event
#[derive(Debug, Clone)]
pub(crate) struct MacroEvent {
    pub keycode: u8,
    pub is_down: bool,
    pub delay_ms: u16,
}

/// Async result from background keyboard operations
/// These are sent from spawned tasks to the main event loop
#[allow(dead_code)] // Macros and SetComplete reserved for future use
pub(crate) enum AsyncResult {
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
    PatchInfo(Result<PatchInfoData, String>),
    DonglePatchInfo(Result<PatchInfoData, String>),
    FirmwareCheck(FirmwareCheckResult),
    // Other tab results
    Triggers(Result<TriggerSettings, String>),
    Options(Result<KbOptions, String>),
    Remaps(Result<Vec<KeyEntry>, String>),
    Macros(Result<Vec<MacroSlot>, String>),
    // Battery status (from keyboard API)
    Battery(Result<BatteryInfo, String>),
    // Operation completion (for set operations)
    SetComplete(String, Result<(), String>), // (field_name, result)
    // Notify tab
    #[cfg(feature = "notify")]
    NotifyEffectsLoaded(Result<EffectLibrary, String>),
    #[cfg(feature = "notify")]
    NotifyDaemonStopped(Result<(), String>),
    #[cfg(feature = "notify")]
    NotifyList(Vec<(u64, String, String, String, i32)>),
}

/// An async result tagged with the device generation it was produced for.
pub(crate) struct GenerationalResult {
    pub generation: u64,
    pub result: AsyncResult,
}

/// A sender that automatically tags results with a device generation.
#[derive(Clone)]
pub(crate) struct GenSender {
    pub tx: mpsc::UnboundedSender<GenerationalResult>,
    pub generation: u64,
}

impl GenSender {
    pub fn send(&self, result: AsyncResult) {
        let _ = self.tx.send(GenerationalResult {
            generation: self.generation,
            result,
        });
    }
}

/// History length for time series (samples)
pub(crate) const DEPTH_HISTORY_LEN: usize = 100;
