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
use std::collections::{HashSet, VecDeque};
use std::io::{self, stdout};
use std::sync::Arc;
use std::time::{Duration, Instant};
use throbber_widgets_tui::{Throbber, ThrobberState, BRAILLE_SIX};
use tokio::sync::mpsc;

use std::path::PathBuf;

// Use shared library
use crate::firmware_api::FirmwareCheckResult;
use crate::hid::BatteryInfo;
use crate::power_supply::{find_hid_battery_power_supply, read_kernel_battery};
use crate::{cmd, devices, hal, key_mode, magnetism, DeviceInfo, TriggerSettings};

// Keyboard abstraction layer - using async interface directly
use monsgeek_keyboard::{
    KeyboardInterface, KeyboardOptions as KbOptions, LedMode, LedParams, RgbColor,
};
use monsgeek_transport::{DeviceDiscovery, HidDiscovery};

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
    input_device: Option<hidapi::HidDevice>, // Separate INPUT interface for depth reports
    info: DeviceInfo,
    tab: usize,
    selected: usize,
    key_depths: Vec<f32>,
    depth_monitoring: bool,
    status_msg: String,
    connected: bool,
    device_name: String,
    key_count: u8,
    // Trigger settings
    triggers: Option<TriggerSettings>,
    trigger_scroll: usize,
    trigger_view_mode: TriggerViewMode,
    trigger_selected_key: usize, // Selected key in layout view
    precision_factor: f32,
    // Keyboard options
    options: Option<KeyboardOptions>,
    // Macro editor state
    macros: Vec<MacroSlot>,
    macro_selected: usize,
    macro_editing: bool,
    macro_edit_text: String,
    // Key depth visualization
    depth_view_mode: DepthViewMode,
    depth_history: Vec<VecDeque<f32>>, // Per-key history for time series
    active_keys: HashSet<usize>,       // Keys with recent activity
    selected_keys: HashSet<usize>,     // Keys selected for time series view
    depth_cursor: usize,               // Cursor for key selection
    depth_sample_idx: usize,           // Global sample counter for time axis
    max_observed_depth: f32,           // Max depth observed during session (for bar scaling)
    depth_last_update: Vec<Instant>,   // Last update time per key (for stale detection)
    // Battery status (for 2.4GHz dongle)
    battery: Option<BatteryInfo>,
    battery_source: Option<BatterySource>,
    last_battery_check: Instant,
    is_wireless: bool,
    // Help popup
    show_help: bool,
    // Keyboard interface (async, wrapped in Arc for spawning tasks)
    keyboard: Option<Arc<KeyboardInterface>>,
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
    sleep_time: u16, // seconds, 0 = never sleep
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
    feature_list: LoadState,    // precision
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
    FeatureList(Result<monsgeek_keyboard::FeatureList, String>),
    SleepTime(Result<u16, String>),
    FirmwareCheck(FirmwareCheckResult),
    // Other tab results
    Triggers(Result<TriggerSettings, String>),
    Options(Result<KbOptions, String>),
    Macros(Result<Vec<MacroSlot>, String>),
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
        keys: "n / N",
        description: "Actuation -/+ 0.1mm",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "t / T",
        description: "RT press sens -/+ 0.1mm",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "d / D",
        description: "RT release sens -/+ 0.1mm",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "s / S",
        description: "DKS sensitivity -/+ 0.1mm",
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
            input_device: None,
            info: DeviceInfo::default(),
            tab: 0,
            selected: 0,
            key_depths: Vec::new(),
            depth_monitoring: false,
            status_msg: String::new(),
            connected: false,
            device_name: String::new(),
            key_count: 0,
            triggers: None,
            trigger_scroll: 0,
            trigger_view_mode: TriggerViewMode::default(),
            trigger_selected_key: 0,
            precision_factor: 100.0, // Default 0.01mm precision
            options: None,
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
            depth_sample_idx: 0,
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
        };
        (app, result_rx)
    }

    async fn connect(&mut self) -> Result<(), String> {
        // Use async device discovery
        let discovery = HidDiscovery::new();
        let devices = discovery
            .list_devices()
            .await
            .map_err(|e| format!("Failed to list devices: {e}"))?;

        if devices.is_empty() {
            return Err("No supported device found".to_string());
        }

        let transport = discovery
            .open_device(&devices[0])
            .await
            .map_err(|e| format!("Failed to open device: {e}"))?;

        let transport_info = transport.device_info().clone();
        let vid = transport_info.vid;
        let pid = transport_info.pid;

        // Look up device info - default to 98 keys with magnetism for M1 V5 HE
        let (key_count, has_magnetism) = match (vid, pid) {
            (0x3151, 0x5030) => (98, true), // M1 V5 HE wired
            (0x3151, 0x5038) => (98, true), // M1 V5 HE dongle
            _ => (98, true),                // Default
        };

        let keyboard = Arc::new(KeyboardInterface::new(transport, key_count, has_magnetism));
        let is_wireless = keyboard.is_wireless();

        let device_name = if let Some(def) = devices::find_device(vid, pid) {
            def.display_name.to_string()
        } else {
            transport_info
                .product_name
                .unwrap_or_else(|| format!("Device {vid:04x}:{pid:04x}"))
        };

        self.device_name = device_name;
        self.key_count = key_count;
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

        // Also open the INPUT interface for depth reports
        self.input_device = open_input_interface(vid, pid).ok();

        // Detect battery source (kernel power_supply if eBPF loaded, else vendor)
        if self.is_wireless {
            self.battery_source = if let Some(path) = find_hid_battery_power_supply(vid, pid) {
                Some(BatterySource::Kernel(path))
            } else {
                Some(BatterySource::Vendor)
            };
        }

        self.connected = true;
        self.status_msg = format!("Connected to {}", self.device_name);

        // Load battery status immediately for wireless devices
        if self.is_wireless {
            self.refresh_battery();
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
        self.loading.feature_list = LoadState::Loading;
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
                let result = kb.get_feature_list().await.map_err(|e| e.to_string());
                let _ = tx.send(AsyncResult::FeatureList(result));
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
        self.battery_source = if let Some(path) = find_hid_battery_power_supply(0x3151, 0x5038) {
            Some(BatterySource::Kernel(path))
        } else {
            Some(BatterySource::Vendor)
        };

        match &self.battery_source {
            Some(BatterySource::Kernel(path)) => {
                // Read from kernel power_supply sysfs
                self.battery = read_kernel_battery(path);
            }
            Some(BatterySource::Vendor) => {
                // Query battery from 2.4GHz dongle vendor interface
                if let Ok(hidapi) = hidapi::HidApi::new() {
                    for device_info in hidapi.device_list() {
                        let vid = device_info.vendor_id();
                        let pid = device_info.product_id();

                        // Only match dongle (PID 0x5038) vendor interface
                        if vid != 0x3151 || pid != 0x5038 || device_info.usage_page() != 0xFFFF {
                            continue;
                        }

                        if let Ok(device) = device_info.open_device(&hidapi) {
                            let mut buf = [0u8; 65];
                            buf[0] = 0x05; // Report ID

                            if let Ok(_len) = device.get_feature_report(&mut buf) {
                                self.battery = BatteryInfo::from_feature_report(&buf);
                            }
                            break;
                        }
                    }
                }
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

    async fn set_sleep_time(&mut self, seconds: u16) {
        if let Some(ref mut opts) = self.options {
            opts.sleep_time = seconds;
        }
        self.info.sleep_seconds = seconds;
        if let Some(ref keyboard) = self.keyboard {
            if keyboard.set_sleep_time(seconds).await.is_ok() {
                if seconds == 0 {
                    self.status_msg = "Sleep: Never".to_string();
                } else {
                    self.status_msg = format!("Sleep: {seconds}s");
                }
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
                self.precision_factor = ver.precision_factor() as f32;
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
            AsyncResult::FeatureList(Ok(features)) => {
                self.info.precision = features.precision;
                self.loading.feature_list = LoadState::Loaded;
            }
            AsyncResult::FeatureList(Err(_)) => {
                self.loading.feature_list = LoadState::Error;
            }
            AsyncResult::SleepTime(Ok(s)) => {
                self.info.sleep_seconds = s;
                self.loading.sleep_time = LoadState::Loaded;
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
                self.options = Some(KeyboardOptions {
                    os_mode: opts.os_mode,
                    fn_layer: opts.fn_layer,
                    anti_mistouch: opts.anti_mistouch,
                    rt_stability: opts.rt_stability,
                    wasd_swap: opts.wasd_swap,
                    sleep_time: self.info.sleep_seconds,
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

        let precision =
            monsgeek_keyboard::FirmwareVersion::new(self.info.version).precision_factor() as f32;
        let now = Instant::now();
        // Long timeout as fallback - primary release detection is via depth < 0.05 threshold
        // This only catches truly stale keys (e.g., missed release reports)
        let stale_timeout = Duration::from_secs(2);

        // Read from INPUT interface (where depth reports come from)
        if let Some(ref input_dev) = self.input_device {
            let mut buf = [0u8; 64];
            while let Ok(len) = input_dev.read_timeout(&mut buf, 5) {
                if len == 0 {
                    break;
                }
                if let Some(report) = crate::protocol::depth_report::parse(&buf[..len]) {
                    let depth_mm = report.depth_mm(precision);
                    let key_index = report.key_index as usize;
                    if key_index < self.key_depths.len() {
                        self.key_depths[key_index] = depth_mm;
                        // Update timestamp for this key
                        if key_index < self.depth_last_update.len() {
                            self.depth_last_update[key_index] = now;
                        }
                        // Track max observed depth for bar chart scaling
                        if depth_mm > self.max_observed_depth {
                            self.max_observed_depth = depth_mm;
                        }
                        // Mark key as active when pressed, remove when fully released
                        if depth_mm > 0.1 {
                            self.active_keys.insert(key_index);
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
            }
        }

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

        // Push current depths to history for all selected keys (time-series lockstep)
        // All selected keys advance together so X-axis (time) is consistent
        self.depth_sample_idx += 1;
        for &key_idx in &self.selected_keys.clone() {
            if key_idx < self.depth_history.len() {
                let history = &mut self.depth_history[key_idx];
                if history.len() >= DEPTH_HISTORY_LEN {
                    history.pop_front();
                }
                // Push current depth (0.0 if not active)
                history.push_back(self.key_depths[key_idx]);
            }
        }
        // Also update history for active but not selected keys (for when they get selected)
        for &key_idx in &self.active_keys.clone() {
            if !self.selected_keys.contains(&key_idx) && key_idx < self.depth_history.len() {
                let history = &mut self.depth_history[key_idx];
                if history.len() >= DEPTH_HISTORY_LEN {
                    history.pop_front();
                }
                history.push_back(self.key_depths[key_idx]);
            }
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
        // Load device info (TUI starts on tab 0) - spawns background tasks
        app.load_device_info();
    }

    // Set up async event stream
    let mut event_stream = EventStream::new();
    let mut tick_interval = tokio::time::interval(Duration::from_millis(100));

    loop {
        terminal.draw(|f| ui(f, &app))?;

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
                                } else if app.trigger_scroll > 0 {
                                    app.trigger_scroll -= 1;
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
                                    let max_scroll = app.triggers.as_ref()
                                        .map(|t| t.key_modes.len().saturating_sub(15))
                                        .unwrap_or(0);
                                    if app.trigger_scroll < max_scroll {
                                        app.trigger_scroll += 1;
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
                                let step: u8 = if key.modifiers.contains(KeyModifiers::SHIFT) { 10 } else { 1 };
                                match app.selected {
                                    0 if app.info.led_mode > 0 => app.set_led_mode(app.info.led_mode - 1).await,
                                    1 if app.info.led_brightness > 0 => app.set_brightness(app.info.led_brightness - 1).await,
                                    2 => {
                                        let current = 4 - app.info.led_speed.min(4);
                                        if current > 0 { app.set_speed(current - 1).await; }
                                    }
                                    3 => { let r = app.info.led_r.saturating_sub(step); app.set_color(r, app.info.led_g, app.info.led_b).await; }
                                    4 => { let g = app.info.led_g.saturating_sub(step); app.set_color(app.info.led_r, g, app.info.led_b).await; }
                                    5 => { let b = app.info.led_b.saturating_sub(step); app.set_color(app.info.led_r, app.info.led_g, b).await; }
                                    7 => app.toggle_dazzle().await,
                                    9 if app.info.side_mode > 0 => app.set_side_mode(app.info.side_mode - 1).await,
                                    10 if app.info.side_brightness > 0 => app.set_side_brightness(app.info.side_brightness - 1).await,
                                    11 => {
                                        let current = 4 - app.info.side_speed.min(4);
                                        if current > 0 { app.set_side_speed(current - 1).await; }
                                    }
                                    12 => { let r = app.info.side_r.saturating_sub(step); app.set_side_color(r, app.info.side_g, app.info.side_b).await; }
                                    13 => { let g = app.info.side_g.saturating_sub(step); app.set_side_color(app.info.side_r, g, app.info.side_b).await; }
                                    14 => { let b = app.info.side_b.saturating_sub(step); app.set_side_color(app.info.side_r, app.info.side_g, b).await; }
                                    16 => app.toggle_side_dazzle().await,
                                    _ => {}
                                }
                            } else if app.tab == 4 {
                                if let Some(ref opts) = app.options.clone() {
                                    match app.selected {
                                        0 if opts.fn_layer > 0 => app.set_fn_layer(opts.fn_layer - 1).await,
                                        1 => app.toggle_wasd_swap().await,
                                        2 => app.toggle_anti_mistouch().await,
                                        3 if opts.rt_stability >= 25 => app.set_rt_stability(opts.rt_stability - 25).await,
                                        4 if opts.sleep_time >= 60 => app.set_sleep_time(opts.sleep_time - 60).await,
                                        4 if opts.sleep_time > 0 && opts.sleep_time < 60 => app.set_sleep_time(0).await,
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
                                let step: u8 = if key.modifiers.contains(KeyModifiers::SHIFT) { 10 } else { 1 };
                                match app.selected {
                                    0 if app.info.led_mode < cmd::LED_MODE_MAX => app.set_led_mode(app.info.led_mode + 1).await,
                                    1 if app.info.led_brightness < 4 => app.set_brightness(app.info.led_brightness + 1).await,
                                    2 => {
                                        let current = 4 - app.info.led_speed.min(4);
                                        if current < 4 { app.set_speed(current + 1).await; }
                                    }
                                    3 => { let r = app.info.led_r.saturating_add(step); app.set_color(r, app.info.led_g, app.info.led_b).await; }
                                    4 => { let g = app.info.led_g.saturating_add(step); app.set_color(app.info.led_r, g, app.info.led_b).await; }
                                    5 => { let b = app.info.led_b.saturating_add(step); app.set_color(app.info.led_r, app.info.led_g, b).await; }
                                    7 => app.toggle_dazzle().await,
                                    9 if app.info.side_mode < cmd::LED_MODE_MAX => app.set_side_mode(app.info.side_mode + 1).await,
                                    10 if app.info.side_brightness < 4 => app.set_side_brightness(app.info.side_brightness + 1).await,
                                    11 => {
                                        let current = 4 - app.info.side_speed.min(4);
                                        if current < 4 { app.set_side_speed(current + 1).await; }
                                    }
                                    12 => { let r = app.info.side_r.saturating_add(step); app.set_side_color(r, app.info.side_g, app.info.side_b).await; }
                                    13 => { let g = app.info.side_g.saturating_add(step); app.set_side_color(app.info.side_r, g, app.info.side_b).await; }
                                    14 => { let b = app.info.side_b.saturating_add(step); app.set_side_color(app.info.side_r, app.info.side_g, b).await; }
                                    16 => app.toggle_side_dazzle().await,
                                    _ => {}
                                }
                            } else if app.tab == 4 {
                                if let Some(ref opts) = app.options.clone() {
                                    match app.selected {
                                        0 if opts.fn_layer < 3 => app.set_fn_layer(opts.fn_layer + 1).await,
                                        1 => app.toggle_wasd_swap().await,
                                        2 => app.toggle_anti_mistouch().await,
                                        3 if opts.rt_stability < 125 => app.set_rt_stability(opts.rt_stability + 25).await,
                                        4 if opts.sleep_time < 3600 => app.set_sleep_time(opts.sleep_time + 60).await,
                                        _ => {}
                                    }
                                }
                            }
                        }
                        KeyCode::Char('r') => {
                            app.status_msg = "Refreshing...".to_string();
                            app.load_device_info();
                            if app.tab == 3 { app.load_triggers(); }
                            else if app.tab == 4 { app.load_options(); }
                            else if app.tab == 5 { app.load_macros(); }
                        }
                        KeyCode::Char('u') if app.tab == 0 => {
                            app.check_firmware();
                        }
                        KeyCode::Enter if app.tab == 1 => {
                            if app.selected == 6 {
                                app.start_hex_input(HexColorTarget::MainLed);
                            } else if app.selected == 15 {
                                app.start_hex_input(HexColorTarget::SideLed);
                            }
                        }
                        KeyCode::Char('#') if app.tab == 1 => {
                            if app.selected >= 3 && app.selected <= 6 {
                                app.start_hex_input(HexColorTarget::MainLed);
                            } else if app.selected >= 12 && app.selected <= 15 {
                                app.start_hex_input(HexColorTarget::SideLed);
                            }
                        }
                        KeyCode::Char(c) if app.tab == 1 && (app.selected == 6 || app.selected == 15) && c.is_ascii_hexdigit() => {
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
                                app.load_device_info();
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
                        // Scroll up navigates up in lists
                        match app.tab {
                            1 => {
                                if app.selected > 0 {
                                    app.selected -= 1;
                                }
                            }
                            3 => {
                                if app.trigger_scroll > 0 {
                                    app.trigger_scroll -= 1;
                                }
                            }
                            4 => {
                                if app.selected > 0 {
                                    app.selected -= 1;
                                }
                            }
                            5 => {
                                if app.macro_selected > 0 {
                                    app.macro_selected -= 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    MouseEventKind::ScrollDown if content.contains(pos) => {
                        // Scroll down navigates down in lists
                        match app.tab {
                            1 => {
                                if app.selected < 16 {
                                    app.selected += 1;
                                }
                            }
                            3 => {
                                if let Some(ref triggers) = app.triggers {
                                    let max_scroll = triggers.press_travel.len().saturating_sub(15);
                                    if app.trigger_scroll < max_scroll {
                                        app.trigger_scroll += 1;
                                    }
                                }
                            }
                            4 => {
                                if app.selected < 4 {
                                    app.selected += 1;
                                }
                            }
                            5 => {
                                if app.macro_selected + 1 < app.macros.len() {
                                    app.macro_selected += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                    }
                } else if let Some(Ok(Event::Resize(_, _))) = maybe_event {
                    // Resize is handled automatically by ratatui on next draw
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
                    app.refresh_battery();
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

fn ui(f: &mut Frame, app: &App) {
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
            format!(" {}{}%({src})", icon, batt.level)
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

fn render_device_info(f: &mut Frame, app: &App, area: Rect) {
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
        // Feature list (precision)
        Line::from(vec![
            Span::raw("Precision:      "),
            value_span(
                loading.feature_list,
                precision_str(info.precision).to_string(),
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

    let para = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Device Information [r to refresh]"),
    );
    f.render_widget(para, area);
}

fn render_led_settings(f: &mut Frame, app: &App, area: Rect) {
    let info = &app.info;
    let speed = 4 - info.led_speed.min(4);

    // Helper to create RGB bar visualization
    let rgb_bar = |val: u8| -> String {
        let bars = (val as usize * 16 / 255).min(16);
        format!("{:3} {}", val, "█".repeat(bars))
    };

    let items: Vec<ListItem> = vec![
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
        // Side LED section
        ListItem::new(Line::from(vec![Span::styled(
            "─── Side LEDs (Side Lights) ───",
            Style::default().fg(Color::DarkGray),
        )])),
        ListItem::new(Line::from(vec![
            Span::raw("Mode:       "),
            Span::styled(
                format!(
                    "< {} ({}) >",
                    info.side_mode,
                    cmd::led_mode_name(info.side_mode)
                ),
                Style::default().fg(Color::Cyan),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Brightness: "),
            Span::styled(
                format!(
                    "< {}/4 >  {}",
                    info.side_brightness,
                    "█".repeat(info.side_brightness as usize)
                ),
                Style::default().fg(Color::Cyan),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Speed:      "),
            Span::styled(
                format!(
                    "< {}/4 >  {}",
                    4 - info.side_speed.min(4),
                    "█".repeat((4 - info.side_speed.min(4)) as usize)
                ),
                Style::default().fg(Color::Cyan),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Red:        "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_r)),
                Style::default().fg(Color::Red),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Green:      "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_g)),
                Style::default().fg(Color::Green),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Blue:       "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_b)),
                Style::default().fg(Color::Blue),
            ),
        ])),
        ListItem::new(Line::from(vec![
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
        ])),
        ListItem::new(Line::from(vec![
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
        ])),
    ];

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

fn render_depth_monitor(f: &mut Frame, app: &App, area: Rect) {
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

/// Get precision string from feature list precision value
fn precision_str(precision: u8) -> &'static str {
    match precision {
        2 => "0.005mm",
        1 => "0.01mm",
        _ => "0.1mm",
    }
}

/// Open the INPUT interface for depth reports
fn open_input_interface(vid: u16, pid: u16) -> Result<hidapi::HidDevice, String> {
    let hidapi = hidapi::HidApi::new().map_err(|e| format!("HID init failed: {e}"))?;

    for dev_info in hidapi.device_list() {
        if dev_info.vendor_id() == vid
            && dev_info.product_id() == pid
            && dev_info.usage_page() == hal::USAGE_PAGE
            && dev_info.usage() == hal::USAGE_INPUT
        {
            return dev_info
                .open_device(&hidapi)
                .map_err(|e| format!("Failed to open input interface: {e}"));
        }
    }
    Err(format!("Input interface for {vid:04x}:{pid:04x} not found"))
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

fn render_depth_bar_chart(f: &mut Frame, app: &App, area: Rect) {
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

fn render_depth_time_series(f: &mut Frame, app: &App, area: Rect) {
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

    // Build datasets for Chart widget
    let mut datasets: Vec<Dataset> = Vec::new();
    let mut all_data: Vec<Vec<(f64, f64)>> = Vec::new();

    // Find the maximum history length to align x-axis
    let max_len = active_keys
        .iter()
        .filter_map(|&k| app.depth_history.get(k))
        .map(|h| h.len())
        .max()
        .unwrap_or(0);

    for (color_idx, &key_idx) in active_keys.iter().enumerate() {
        if key_idx < app.depth_history.len() {
            let history = &app.depth_history[key_idx];
            // Use actual sample indices for scrolling effect
            let start_idx = max_len.saturating_sub(history.len());
            let data: Vec<(f64, f64)> = history
                .iter()
                .enumerate()
                .map(|(i, &depth)| ((start_idx + i) as f64, depth as f64))
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

    // X-axis scrolls: show last DEPTH_HISTORY_LEN samples
    let x_max = max_len.max(DEPTH_HISTORY_LEN) as f64;
    let x_min = x_max - DEPTH_HISTORY_LEN as f64;

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Time Series: {legend}")),
        )
        .x_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds([x_min.max(0.0), x_max]),
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

fn render_trigger_settings(f: &mut Frame, app: &App, area: Rect) {
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
fn render_trigger_list(f: &mut Frame, app: &App, area: Rect) {
    // Split into summary and detail areas
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Summary
            Constraint::Min(10),   // Key list
        ])
        .split(area);

    // Summary section
    let factor = app.precision_factor;
    let precision_str = if factor >= 200.0 {
        "0.005mm"
    } else if factor >= 100.0 {
        "0.01mm"
    } else {
        "0.1mm"
    };

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
                Span::styled(" (v to toggle)", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Global Settings (all keys same): ",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
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

    // Key list section
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

        // Build rows for table
        let rows: Vec<Row> = (0..num_keys)
            .skip(app.trigger_scroll)
            .take(15) // Show 15 keys at a time
            .map(|i| {
                let press = decode_u16(&triggers.press_travel, i);
                let lift = decode_u16(&triggers.lift_travel, i);
                let rt_p = decode_u16(&triggers.rt_press, i);
                let rt_l = decode_u16(&triggers.rt_lift, i);
                let mode = triggers.key_modes.get(i).copied().unwrap_or(0);
                let key_name = get_key_label(i);

                Row::new(vec![
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
                ])
            })
            .collect();

        let header = Row::new(vec!["#", "Key", "Act", "Rel", "RT↓", "RT↑", "Mode"]).style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        );

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
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Per-Key [{}-{}] n/t/d/s:SetAll v:Layout",
            app.trigger_scroll,
            (app.trigger_scroll + 15).min(num_keys)
        )));

        f.render_widget(table, chunks[1]);
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
fn render_trigger_layout(f: &mut Frame, app: &App, area: Rect) {
    // Split into layout area and detail area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // Keyboard layout
            Constraint::Length(9), // Selected key details
        ])
        .split(area);

    let factor = app.precision_factor;

    // Render keyboard layout
    let layout_area = chunks[0];
    let inner = Block::default()
        .borders(Borders::ALL)
        .title("Keyboard Layout [↑↓←→:Navigate v:List n/t/d/s:Mode]")
        .inner(layout_area);

    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title("Keyboard Layout [↑↓←→:Navigate v:List n/t/d/s:Mode]"),
        layout_area,
    );

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

fn render_options(f: &mut Frame, app: &App, area: Rect) {
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
            ListItem::new(Line::from(vec![
                Span::raw("Sleep Time:     "),
                Span::styled(
                    if opts.sleep_time == 0 {
                        "< Never >".to_string()
                    } else {
                        format!("< {}s >", opts.sleep_time)
                    },
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    "  (0=never, auto-sleep timeout)",
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
        state.select(Some(app.selected.min(4)));
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

fn render_macros(f: &mut Frame, app: &App, area: Rect) {
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
