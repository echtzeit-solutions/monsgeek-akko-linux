// MonsGeek M1 V5 HE TUI Application
// Real-time monitoring and settings configuration

use std::collections::{HashSet, VecDeque};
use std::io::{self, stdout};
use std::time::{Duration, Instant};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::*,
};

// Use shared library
use crate::{cmd, MonsGeekDevice, DeviceInfo, TriggerSettings, magnetism, key_mode};
use crate::hid::BatteryInfo;

/// Application state
struct App {
    device: Option<MonsGeekDevice>,
    input_device: Option<hidapi::HidDevice>,  // Separate INPUT interface for depth reports
    info: DeviceInfo,
    tab: usize,
    selected: usize,
    key_depths: Vec<f32>,
    depth_monitoring: bool,
    last_refresh: Instant,
    status_msg: String,
    connected: bool,
    device_name: String,
    key_count: u8,
    // Trigger settings
    triggers: Option<TriggerSettings>,
    trigger_scroll: usize,
    trigger_view_mode: TriggerViewMode,
    trigger_selected_key: usize,  // Selected key in layout view
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
    depth_history: Vec<VecDeque<f32>>,  // Per-key history for time series
    active_keys: HashSet<usize>,         // Keys with recent activity
    selected_keys: HashSet<usize>,       // Keys selected for time series view
    depth_cursor: usize,                 // Cursor for key selection
    depth_sample_idx: usize,             // Global sample counter for time axis
    // Battery status (for 2.4GHz dongle)
    battery: Option<BatteryInfo>,
    last_battery_check: Instant,
    is_wireless: bool,
    // Help popup
    show_help: bool,
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
}

/// Key depth visualization mode
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum DepthViewMode {
    #[default]
    BarChart,    // Bar chart of all active keys
    TimeSeries,  // Time series graph of selected keys
}

/// Trigger settings view mode
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum TriggerViewMode {
    #[default]
    List,        // Scrollable list of keys
    Layout,      // Visual keyboard layout
}

/// History length for time series (samples)
const DEPTH_HISTORY_LEN: usize = 100;

// ============================================================================
// Help System - Self-documenting keybindings
// ============================================================================

/// Context in which a keybind is active
#[derive(Debug, Clone, Copy, PartialEq)]
enum KeyContext {
    Global,      // Available everywhere
    Led,         // LED Settings tab (1)
    Depth,       // Key Depth tab (2)
    Triggers,    // Trigger Settings tab (3)
    Macros,      // Macros tab (5)
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
    Keybind { keys: "q / Esc", description: "Quit application", context: KeyContext::Global },
    Keybind { keys: "? / F1", description: "Toggle this help", context: KeyContext::Global },
    Keybind { keys: "Tab", description: "Next tab", context: KeyContext::Global },
    Keybind { keys: "Shift+Tab", description: "Previous tab", context: KeyContext::Global },
    Keybind { keys: "↑ / k", description: "Navigate up", context: KeyContext::Global },
    Keybind { keys: "↓ / j", description: "Navigate down", context: KeyContext::Global },
    Keybind { keys: "← / h", description: "Navigate left / decrease", context: KeyContext::Global },
    Keybind { keys: "→ / l", description: "Navigate right / increase", context: KeyContext::Global },
    Keybind { keys: "r", description: "Refresh device info", context: KeyContext::Global },
    Keybind { keys: "c", description: "Connect to device", context: KeyContext::Global },
    Keybind { keys: "m", description: "Toggle depth monitoring", context: KeyContext::Global },
    Keybind { keys: "Ctrl+1-4", description: "Switch profile 1-4", context: KeyContext::Global },
    Keybind { keys: "PgUp/PgDn", description: "Fast scroll (15 items)", context: KeyContext::Global },
    // LED tab
    Keybind { keys: "p", description: "Apply LED settings", context: KeyContext::Led },
    Keybind { keys: "Shift+←/→", description: "Adjust by ±10", context: KeyContext::Led },
    // Depth tab
    Keybind { keys: "v", description: "Toggle visualization mode", context: KeyContext::Depth },
    Keybind { keys: "x", description: "Clear depth data", context: KeyContext::Depth },
    Keybind { keys: "Space", description: "Pause/resume monitoring", context: KeyContext::Depth },
    // Triggers tab
    Keybind { keys: "v", description: "Toggle list/layout view", context: KeyContext::Triggers },
    Keybind { keys: "n / N", description: "Actuation -/+ 0.1mm", context: KeyContext::Triggers },
    Keybind { keys: "t / T", description: "RT press sens -/+ 0.1mm", context: KeyContext::Triggers },
    Keybind { keys: "d / D", description: "RT release sens -/+ 0.1mm", context: KeyContext::Triggers },
    Keybind { keys: "s / S", description: "DKS sensitivity -/+ 0.1mm", context: KeyContext::Triggers },
    // Macros tab
    Keybind { keys: "e", description: "Edit selected macro", context: KeyContext::Macros },
    Keybind { keys: "c", description: "Clear selected macro", context: KeyContext::Macros },
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
    fn new() -> Self {
        Self {
            device: None,
            input_device: None,
            info: DeviceInfo::default(),
            tab: 0,
            selected: 0,
            key_depths: Vec::new(),
            depth_monitoring: false,
            last_refresh: Instant::now(),
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
            // Battery status
            battery: None,
            last_battery_check: Instant::now(),
            is_wireless: false,
            // Help popup
            show_help: false,
        }
    }

    fn connect(&mut self) -> Result<(), String> {
        match MonsGeekDevice::open() {
            Ok(dev) => {
                // Get device info from definition
                self.device_name = dev.display_name().to_string();
                self.key_count = dev.key_count();
                // Initialize key depths array based on actual key count
                self.key_depths = vec![0.0; self.key_count as usize];
                // Initialize depth history for time series
                self.depth_history = vec![VecDeque::with_capacity(DEPTH_HISTORY_LEN); self.key_count as usize];
                self.active_keys.clear();
                self.selected_keys.clear();

                // Also open the INPUT interface for depth reports
                let vid = dev.vid;
                let pid = dev.pid;
                self.input_device = MonsGeekDevice::open_input_interface(vid, pid).ok();

                // Check if this is a wireless dongle
                self.is_wireless = pid == 0x5038;
                if self.is_wireless {
                    self.refresh_battery();
                }

                self.device = Some(dev);
                self.connected = true;
                self.status_msg = format!("Connected to {}", self.device_name);
                Ok(())
            }
            Err(e) => {
                self.connected = false;
                Err(e)
            }
        }
    }

    fn refresh_info(&mut self) {
        if let Some(ref device) = self.device {
            self.info = device.read_info();
            self.precision_factor = MonsGeekDevice::precision_factor_from_version(self.info.version);
            self.last_refresh = Instant::now();
        }
    }

    fn refresh_triggers(&mut self) {
        if let Some(ref device) = self.device {
            self.triggers = device.get_all_triggers();
            if self.triggers.is_some() {
                self.status_msg = "Trigger settings loaded".to_string();
            } else {
                self.status_msg = "Failed to load trigger settings".to_string();
            }
        }
    }

    fn refresh_battery(&mut self) {
        if !self.is_wireless {
            return;
        }

        // Query battery from 2.4GHz dongle
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
        self.last_battery_check = Instant::now();
    }

    fn toggle_depth_monitoring(&mut self) {
        if let Some(ref device) = self.device {
            self.depth_monitoring = !self.depth_monitoring;
            device.set_magnetism_report(self.depth_monitoring);
            self.status_msg = if self.depth_monitoring {
                "Key depth monitoring ENABLED".to_string()
            } else {
                "Key depth monitoring DISABLED".to_string()
            };
        }
    }

    fn set_led_mode(&mut self, mode: u8) {
        if let Some(ref device) = self.device {
            if device.set_led(
                mode,
                self.info.led_brightness,
                4 - self.info.led_speed.min(4),
                self.info.led_r,
                self.info.led_g,
                self.info.led_b,
                self.info.led_dazzle,
            ) {
                self.info.led_mode = mode;
                self.status_msg = format!("LED mode: {}", cmd::led_mode_name(mode));
            }
        }
    }

    fn set_brightness(&mut self, brightness: u8) {
        if let Some(ref device) = self.device {
            let brightness = brightness.min(4);
            if device.set_led(
                self.info.led_mode,
                brightness,
                4 - self.info.led_speed.min(4),
                self.info.led_r,
                self.info.led_g,
                self.info.led_b,
                self.info.led_dazzle,
            ) {
                self.info.led_brightness = brightness;
                self.status_msg = format!("Brightness: {brightness}/4");
            }
        }
    }

    fn set_speed(&mut self, speed: u8) {
        if let Some(ref device) = self.device {
            let speed = speed.min(4);
            if device.set_led(
                self.info.led_mode,
                self.info.led_brightness,
                speed,
                self.info.led_r,
                self.info.led_g,
                self.info.led_b,
                self.info.led_dazzle,
            ) {
                self.info.led_speed = 4 - speed;
                self.status_msg = format!("Speed: {speed}/4");
            }
        }
    }

    fn set_profile(&mut self, profile: u8) {
        if let Some(ref device) = self.device {
            if device.set_profile(profile) {
                self.info.profile = profile;
                self.status_msg = format!("Switched to profile {}", profile + 1);
                // Refresh info to get profile-specific settings
                self.refresh_info();
            } else {
                self.status_msg = format!("Failed to set profile {}", profile + 1);
            }
        }
    }

    fn set_color(&mut self, r: u8, g: u8, b: u8) {
        if let Some(ref device) = self.device {
            if device.set_led(
                self.info.led_mode,
                self.info.led_brightness,
                4 - self.info.led_speed.min(4),
                r, g, b,
                self.info.led_dazzle,
            ) {
                self.info.led_r = r;
                self.info.led_g = g;
                self.info.led_b = b;
                self.status_msg = format!("Color: #{r:02X}{g:02X}{b:02X}");
            }
        }
    }

    fn toggle_dazzle(&mut self) {
        if let Some(ref device) = self.device {
            let new_dazzle = !self.info.led_dazzle;
            if device.set_led(
                self.info.led_mode,
                self.info.led_brightness,
                4 - self.info.led_speed.min(4),
                self.info.led_r,
                self.info.led_g,
                self.info.led_b,
                new_dazzle,
            ) {
                self.info.led_dazzle = new_dazzle;
                self.status_msg = format!("Dazzle: {}", if new_dazzle { "ON" } else { "OFF" });
            }
        }
    }

    // Side LED methods
    fn set_side_mode(&mut self, mode: u8) {
        if let Some(ref device) = self.device {
            if device.set_side_led(
                mode,
                self.info.side_brightness,
                4 - self.info.side_speed.min(4),
                self.info.side_r,
                self.info.side_g,
                self.info.side_b,
                self.info.side_dazzle,
            ) {
                self.info.side_mode = mode;
                self.status_msg = format!("Side LED mode: {}", cmd::led_mode_name(mode));
            }
        }
    }

    fn set_side_brightness(&mut self, brightness: u8) {
        if let Some(ref device) = self.device {
            let brightness = brightness.min(4);
            if device.set_side_led(
                self.info.side_mode,
                brightness,
                4 - self.info.side_speed.min(4),
                self.info.side_r,
                self.info.side_g,
                self.info.side_b,
                self.info.side_dazzle,
            ) {
                self.info.side_brightness = brightness;
                self.status_msg = format!("Side brightness: {brightness}/4");
            }
        }
    }

    fn set_side_speed(&mut self, speed: u8) {
        if let Some(ref device) = self.device {
            let speed = speed.min(4);
            if device.set_side_led(
                self.info.side_mode,
                self.info.side_brightness,
                speed,
                self.info.side_r,
                self.info.side_g,
                self.info.side_b,
                self.info.side_dazzle,
            ) {
                self.info.side_speed = 4 - speed;
                self.status_msg = format!("Side speed: {speed}/4");
            }
        }
    }

    fn set_side_color(&mut self, r: u8, g: u8, b: u8) {
        if let Some(ref device) = self.device {
            if device.set_side_led(
                self.info.side_mode,
                self.info.side_brightness,
                4 - self.info.side_speed.min(4),
                r, g, b,
                self.info.side_dazzle,
            ) {
                self.info.side_r = r;
                self.info.side_g = g;
                self.info.side_b = b;
                self.status_msg = format!("Side color: #{r:02X}{g:02X}{b:02X}");
            }
        }
    }

    fn toggle_side_dazzle(&mut self) {
        if let Some(ref device) = self.device {
            let new_dazzle = !self.info.side_dazzle;
            if device.set_side_led(
                self.info.side_mode,
                self.info.side_brightness,
                4 - self.info.side_speed.min(4),
                self.info.side_r,
                self.info.side_g,
                self.info.side_b,
                new_dazzle,
            ) {
                self.info.side_dazzle = new_dazzle;
                self.status_msg = format!("Side dazzle: {}", if new_dazzle { "ON" } else { "OFF" });
            }
        }
    }

    fn set_all_key_modes(&mut self, mode: u8) {
        if let (Some(ref device), Some(ref mut triggers)) = (&self.device, &mut self.triggers) {
            let key_count = triggers.key_modes.len();
            let modes: Vec<u8> = vec![mode; key_count];
            if device.set_key_modes(&modes) {
                triggers.key_modes = modes;
                self.status_msg = format!("All keys set to {}", magnetism::mode_name(mode));
            } else {
                self.status_msg = "Failed to set key modes".to_string();
            }
        }
    }

    /// Set mode for a single key (used in layout view)
    fn set_single_key_mode(&mut self, key_index: usize, mode: u8) {
        if let (Some(ref device), Some(ref mut triggers)) = (&self.device, &mut self.triggers) {
            if key_index >= triggers.key_modes.len() {
                self.status_msg = format!("Invalid key index: {key_index}");
                return;
            }
            if device.set_single_key_mode(&mut triggers.key_modes, key_index, mode) {
                let key_name = get_key_label(key_index);
                self.status_msg = format!("Key {} ({}) set to {}", key_index, key_name, magnetism::mode_name(mode));
            } else {
                self.status_msg = format!("Failed to set key {key_index} mode");
            }
        }
    }

    /// Set key mode - dispatches to single or all based on view mode
    fn set_key_mode(&mut self, mode: u8) {
        if self.trigger_view_mode == TriggerViewMode::Layout {
            self.set_single_key_mode(self.trigger_selected_key, mode);
        } else {
            self.set_all_key_modes(mode);
        }
    }

    fn apply_per_key_color(&mut self) {
        if let Some(ref device) = self.device {
            let (r, g, b) = (self.info.led_r, self.info.led_g, self.info.led_b);
            self.status_msg = format!("Applying #{r:02X}{g:02X}{b:02X} to all keys...");
            if device.set_all_keys_color(r, g, b) {
                self.info.led_mode = 25; // Update to Per-Key Color mode
                self.status_msg = format!("Per-key color set: #{r:02X}{g:02X}{b:02X}");
            } else {
                self.status_msg = "Failed to set per-key colors".to_string();
            }
        }
    }

    fn refresh_options(&mut self) {
        if let Some(ref device) = self.device {
            if let Some((os_mode, fn_layer, anti_mistouch, rt_stability, wasd_swap)) = device.get_options() {
                self.options = Some(KeyboardOptions {
                    os_mode,
                    fn_layer,
                    anti_mistouch,
                    rt_stability,
                    wasd_swap,
                });
                self.status_msg = "Keyboard options loaded".to_string();
            } else {
                self.status_msg = "Failed to load keyboard options".to_string();
            }
        }
    }

    fn save_options(&mut self) {
        if let (Some(ref device), Some(ref opts)) = (&self.device, &self.options) {
            if device.set_options(opts.fn_layer, opts.anti_mistouch, opts.rt_stability, opts.wasd_swap) {
                self.status_msg = "Options saved".to_string();
            } else {
                self.status_msg = "Failed to save options".to_string();
            }
        }
    }

    fn set_fn_layer(&mut self, layer: u8) {
        let layer = layer.min(3);
        if let Some(ref mut opts) = self.options {
            opts.fn_layer = layer;
        }
        self.save_options();
        self.status_msg = format!("Fn layer: {layer}");
    }

    fn toggle_wasd_swap(&mut self) {
        let new_val = self.options.as_ref().map(|o| !o.wasd_swap).unwrap_or(false);
        if let Some(ref mut opts) = self.options {
            opts.wasd_swap = new_val;
        }
        self.save_options();
        self.status_msg = format!("WASD swap: {}", if new_val { "ON" } else { "OFF" });
    }

    fn toggle_anti_mistouch(&mut self) {
        let new_val = self.options.as_ref().map(|o| !o.anti_mistouch).unwrap_or(false);
        if let Some(ref mut opts) = self.options {
            opts.anti_mistouch = new_val;
        }
        self.save_options();
        self.status_msg = format!("Anti-mistouch: {}", if new_val { "ON" } else { "OFF" });
    }

    fn set_rt_stability(&mut self, value: u8) {
        let value = value.min(125);
        if let Some(ref mut opts) = self.options {
            opts.rt_stability = value;
        }
        self.save_options();
        self.status_msg = format!("RT stability: {value}ms");
    }

    fn refresh_macros(&mut self) {
        use crate::protocol::hid::key_name;

        if let Some(ref device) = self.device {
            self.macros.clear();

            // Load first 8 macro slots
            for i in 0..8 {
                if let Some(data) = device.get_macro(i) {
                    let mut slot = MacroSlot::default();

                    if data.len() >= 2 {
                        slot.repeat_count = u16::from_le_bytes([data[0], data[1]]);

                        // Parse events
                        let events = &data[2..];
                        let mut text = String::new();

                        for chunk in events.chunks(2) {
                            if chunk.len() < 2 || (chunk[0] == 0 && chunk[1] == 0) {
                                break;
                            }
                            let keycode = chunk[0];
                            let flags = chunk[1];
                            let is_down = flags & 0x80 != 0;
                            let delay_ms = flags & 0x7F;

                            slot.events.push(MacroEvent { keycode, is_down, delay_ms });

                            // Build text preview (only on key down, skip modifiers)
                            if is_down && keycode < 0xE0 {
                                let name = key_name(keycode);
                                if name.len() == 1 {
                                    text.push_str(name);
                                } else if name == "Space" {
                                    text.push(' ');
                                } else if name == "Enter" {
                                    text.push('↵');
                                }
                            }
                        }
                        slot.text_preview = if text.is_empty() {
                            format!("{} events", slot.events.len())
                        } else {
                            text.chars().take(20).collect()
                        };
                    }
                    self.macros.push(slot);
                } else {
                    self.macros.push(MacroSlot::default());
                }
            }
            self.status_msg = format!("Loaded {} macro slots", self.macros.len());
        }
    }

    fn set_macro_text(&mut self, index: usize, text: &str, delay_ms: u8, repeat: u16) {
        if let Some(ref device) = self.device {
            if device.set_text_macro(index as u8, text, delay_ms, repeat) {
                self.status_msg = format!("Macro {index} set to: {text}");
                // Refresh to show updated macro
                self.refresh_macros();
            } else {
                self.status_msg = format!("Failed to set macro {index}");
            }
        }
    }

    fn clear_macro(&mut self, index: usize) {
        if let Some(ref device) = self.device {
            if device.set_macro(index as u8, &[], 1) {
                self.status_msg = format!("Macro {index} cleared");
                self.refresh_macros();
            } else {
                self.status_msg = format!("Failed to clear macro {index}");
            }
        }
    }

    fn read_input_reports(&mut self) {
        if !self.depth_monitoring {
            return;
        }

        let precision = MonsGeekDevice::precision_factor_from_version(self.info.version);

        // Read from feature interface
        if let Some(ref device) = self.device {
            while let Some(buf) = device.read_input(5) {
                if let Some(report) = crate::protocol::depth_report::parse(&buf) {
                    let depth_mm = report.depth_mm(precision);
                    let key_index = report.key_index as usize;
                    if key_index < self.key_depths.len() {
                        self.key_depths[key_index] = depth_mm;
                        if depth_mm > 0.1 {
                            self.active_keys.insert(key_index);
                        }
                    }
                }
            }
        }

        // Read from INPUT interface (where depth reports actually come from)
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
                        if depth_mm > 0.1 {
                            self.active_keys.insert(key_index);
                        }
                    }
                }
            }
        }

        // Push current depths to history for all active keys (time-based sampling)
        self.depth_sample_idx += 1;
        for &key_idx in &self.active_keys.clone() {
            if key_idx < self.depth_history.len() {
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
        let max_key = self.triggers.as_ref()
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
        if col < 20 {  // 21 columns total
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

/// Run the TUI - called via 'iot_driver tui' command
pub fn run() -> io::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let mut app = App::new();

    // Try to connect
    if let Err(e) = app.connect() {
        app.status_msg = e;
    } else {
        app.refresh_info();
    }

    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, &app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
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
                            // Auto-refresh when entering tabs
                            if app.tab == 3 && app.triggers.is_none() {
                                app.refresh_triggers();
                            } else if app.tab == 4 && app.options.is_none() {
                                app.refresh_options();
                            } else if app.tab == 5 && app.macros.is_empty() {
                                app.refresh_macros();
                            }
                        }
                        KeyCode::BackTab => {
                            app.tab = if app.tab == 0 { 5 } else { app.tab - 1 };
                            app.selected = 0;
                            app.trigger_scroll = 0;
                            // Auto-refresh when entering tabs
                            if app.tab == 3 && app.triggers.is_none() {
                                app.refresh_triggers();
                            } else if app.tab == 4 && app.options.is_none() {
                                app.refresh_options();
                            } else if app.tab == 5 && app.macros.is_empty() {
                                app.refresh_macros();
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.tab == 2 && app.depth_view_mode == DepthViewMode::BarChart {
                                // Move cursor up one row in depth bar chart
                                // Row sizes: 15, 15, 13, 13, 10
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
                                // Triggers tab navigation
                                if app.trigger_view_mode == TriggerViewMode::Layout {
                                    app.layout_key_up();
                                } else if app.trigger_scroll > 0 {
                                    app.trigger_scroll -= 1;
                                }
                            } else if app.tab == 5 {
                                // Select macro
                                if app.macro_selected > 0 {
                                    app.macro_selected -= 1;
                                }
                            } else if app.selected > 0 {
                                app.selected -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.tab == 2 && app.depth_view_mode == DepthViewMode::BarChart {
                                // Move cursor down one row in depth bar chart
                                let row_starts = [0, 15, 30, 43, 56, 66]; // last is end sentinel
                                if let Some(row) = row_starts.iter().rposition(|&s| s <= app.depth_cursor) {
                                    if row < 4 { // 5 rows total
                                        let col = app.depth_cursor - row_starts[row];
                                        let next_row_start = row_starts[row + 1];
                                        let next_row_size = row_starts[row + 2] - next_row_start;
                                        app.depth_cursor = next_row_start + col.min(next_row_size - 1);
                                    }
                                }
                            } else if app.tab == 3 {
                                // Triggers tab navigation
                                if app.trigger_view_mode == TriggerViewMode::Layout {
                                    app.layout_key_down();
                                } else {
                                    // Scroll trigger list
                                    let max_scroll = app.triggers.as_ref()
                                        .map(|t| t.key_modes.len().saturating_sub(15))
                                        .unwrap_or(0);
                                    if app.trigger_scroll < max_scroll {
                                        app.trigger_scroll += 1;
                                    }
                                }
                            } else if app.tab == 5 {
                                // Select macro
                                if app.macro_selected < app.macros.len().saturating_sub(1) {
                                    app.macro_selected += 1;
                                }
                            } else {
                                app.selected += 1;
                            }
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            if app.tab == 2 && app.depth_view_mode == DepthViewMode::BarChart {
                                // Move cursor left in depth bar chart
                                if app.depth_cursor > 0 {
                                    app.depth_cursor -= 1;
                                }
                            } else if app.tab == 3 && app.trigger_view_mode == TriggerViewMode::Layout {
                                app.layout_key_left();
                            } else if app.tab == 1 {
                                let step: u8 = if key.modifiers.contains(event::KeyModifiers::SHIFT) { 10 } else { 1 };
                                match app.selected {
                                    // Main LED
                                    0 if app.info.led_mode > 0 => app.set_led_mode(app.info.led_mode - 1),
                                    1 if app.info.led_brightness > 0 => app.set_brightness(app.info.led_brightness - 1),
                                    2 => {
                                        let current = 4 - app.info.led_speed.min(4);
                                        if current > 0 {
                                            app.set_speed(current - 1);
                                        }
                                    }
                                    3 => { // Red
                                        let r = app.info.led_r.saturating_sub(step);
                                        app.set_color(r, app.info.led_g, app.info.led_b);
                                    }
                                    4 => { // Green
                                        let g = app.info.led_g.saturating_sub(step);
                                        app.set_color(app.info.led_r, g, app.info.led_b);
                                    }
                                    5 => { // Blue
                                        let b = app.info.led_b.saturating_sub(step);
                                        app.set_color(app.info.led_r, app.info.led_g, b);
                                    }
                                    7 => app.toggle_dazzle(), // Dazzle
                                    // Side LED (8 is separator)
                                    9 if app.info.side_mode > 0 => app.set_side_mode(app.info.side_mode - 1),
                                    10 if app.info.side_brightness > 0 => app.set_side_brightness(app.info.side_brightness - 1),
                                    11 => {
                                        let current = 4 - app.info.side_speed.min(4);
                                        if current > 0 {
                                            app.set_side_speed(current - 1);
                                        }
                                    }
                                    12 => { // Side Red
                                        let r = app.info.side_r.saturating_sub(step);
                                        app.set_side_color(r, app.info.side_g, app.info.side_b);
                                    }
                                    13 => { // Side Green
                                        let g = app.info.side_g.saturating_sub(step);
                                        app.set_side_color(app.info.side_r, g, app.info.side_b);
                                    }
                                    14 => { // Side Blue
                                        let b = app.info.side_b.saturating_sub(step);
                                        app.set_side_color(app.info.side_r, app.info.side_g, b);
                                    }
                                    15 => app.toggle_side_dazzle(), // Side Dazzle
                                    _ => {}
                                }
                            } else if app.tab == 4 {
                                // Options tab
                                if let Some(ref opts) = app.options.clone() {
                                    match app.selected {
                                        0 if opts.fn_layer > 0 => app.set_fn_layer(opts.fn_layer - 1),
                                        1 => app.toggle_wasd_swap(),
                                        2 => app.toggle_anti_mistouch(),
                                        3 if opts.rt_stability >= 25 => app.set_rt_stability(opts.rt_stability - 25),
                                        _ => {}
                                    }
                                }
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            if app.tab == 2 && app.depth_view_mode == DepthViewMode::BarChart {
                                // Move cursor right in depth bar chart
                                let max_key = app.key_depths.len().min(66).saturating_sub(1);
                                if app.depth_cursor < max_key {
                                    app.depth_cursor += 1;
                                }
                            } else if app.tab == 3 && app.trigger_view_mode == TriggerViewMode::Layout {
                                app.layout_key_right();
                            } else if app.tab == 1 {
                                let step: u8 = if key.modifiers.contains(event::KeyModifiers::SHIFT) { 10 } else { 1 };
                                match app.selected {
                                    // Main LED
                                    0 if app.info.led_mode < cmd::LED_MODE_MAX => app.set_led_mode(app.info.led_mode + 1),
                                    1 if app.info.led_brightness < 4 => app.set_brightness(app.info.led_brightness + 1),
                                    2 => {
                                        let current = 4 - app.info.led_speed.min(4);
                                        if current < 4 {
                                            app.set_speed(current + 1);
                                        }
                                    }
                                    3 => { // Red
                                        let r = app.info.led_r.saturating_add(step);
                                        app.set_color(r, app.info.led_g, app.info.led_b);
                                    }
                                    4 => { // Green
                                        let g = app.info.led_g.saturating_add(step);
                                        app.set_color(app.info.led_r, g, app.info.led_b);
                                    }
                                    5 => { // Blue
                                        let b = app.info.led_b.saturating_add(step);
                                        app.set_color(app.info.led_r, app.info.led_g, b);
                                    }
                                    7 => app.toggle_dazzle(), // Dazzle
                                    // Side LED (8 is separator)
                                    9 if app.info.side_mode < cmd::LED_MODE_MAX => app.set_side_mode(app.info.side_mode + 1),
                                    10 if app.info.side_brightness < 4 => app.set_side_brightness(app.info.side_brightness + 1),
                                    11 => {
                                        let current = 4 - app.info.side_speed.min(4);
                                        if current < 4 {
                                            app.set_side_speed(current + 1);
                                        }
                                    }
                                    12 => { // Side Red
                                        let r = app.info.side_r.saturating_add(step);
                                        app.set_side_color(r, app.info.side_g, app.info.side_b);
                                    }
                                    13 => { // Side Green
                                        let g = app.info.side_g.saturating_add(step);
                                        app.set_side_color(app.info.side_r, g, app.info.side_b);
                                    }
                                    14 => { // Side Blue
                                        let b = app.info.side_b.saturating_add(step);
                                        app.set_side_color(app.info.side_r, app.info.side_g, b);
                                    }
                                    15 => app.toggle_side_dazzle(), // Side Dazzle
                                    _ => {}
                                }
                            } else if app.tab == 4 {
                                // Options tab
                                if let Some(ref opts) = app.options.clone() {
                                    match app.selected {
                                        0 if opts.fn_layer < 3 => app.set_fn_layer(opts.fn_layer + 1),
                                        1 => app.toggle_wasd_swap(),
                                        2 => app.toggle_anti_mistouch(),
                                        3 if opts.rt_stability < 125 => app.set_rt_stability(opts.rt_stability + 25),
                                        _ => {}
                                    }
                                }
                            }
                        }
                        KeyCode::Char('r') => {
                            app.refresh_info();
                            if app.tab == 3 {
                                app.refresh_triggers();
                            } else if app.tab == 4 {
                                app.refresh_options();
                            } else if app.tab == 5 {
                                app.refresh_macros();
                            } else {
                                app.status_msg = "Refreshed device info".to_string();
                            }
                        }
                        KeyCode::Char('e') if app.tab == 5 => {
                            // Edit selected macro
                            if !app.macros.is_empty() {
                                app.macro_editing = true;
                                app.macro_edit_text.clear();
                                // Pre-fill with existing text preview if available
                                let m = &app.macros[app.macro_selected];
                                if !m.text_preview.is_empty() && !m.text_preview.contains("events") {
                                    app.macro_edit_text = m.text_preview.clone();
                                }
                                app.status_msg = format!("Editing macro {} - type text and press Enter", app.macro_selected);
                            }
                        }
                        KeyCode::Char('c') if app.tab == 5 => {
                            // Clear selected macro
                            if !app.macros.is_empty() {
                                app.clear_macro(app.macro_selected);
                            }
                        }
                        KeyCode::Char('m') => {
                            app.toggle_depth_monitoring();
                        }
                        KeyCode::Char('c') => {
                            if let Err(e) = app.connect() {
                                app.status_msg = e;
                            } else {
                                app.refresh_info();
                            }
                        }
                        // Profile switching with Ctrl+1-4
                        KeyCode::Char('1') if key.modifiers.contains(event::KeyModifiers::CONTROL) => app.set_profile(0),
                        KeyCode::Char('2') if key.modifiers.contains(event::KeyModifiers::CONTROL) => app.set_profile(1),
                        KeyCode::Char('3') if key.modifiers.contains(event::KeyModifiers::CONTROL) => app.set_profile(2),
                        KeyCode::Char('4') if key.modifiers.contains(event::KeyModifiers::CONTROL) => app.set_profile(3),
                        // Page up/down for fast trigger scrolling
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
                        // Key mode switching on Triggers tab
                        // In Layout view: sets selected key only
                        // In List view: sets all keys
                        // Shift+key always sets all keys
                        KeyCode::Char('n') if app.tab == 3 => {
                            app.set_key_mode(magnetism::MODE_NORMAL);
                        }
                        KeyCode::Char('N') if app.tab == 3 => {
                            app.set_all_key_modes(magnetism::MODE_NORMAL);
                        }
                        KeyCode::Char('t') if app.tab == 3 => {
                            app.set_key_mode(magnetism::MODE_RAPID_TRIGGER);
                        }
                        KeyCode::Char('T') if app.tab == 3 => {
                            app.set_all_key_modes(magnetism::MODE_RAPID_TRIGGER);
                        }
                        KeyCode::Char('d') if app.tab == 3 => {
                            app.set_key_mode(magnetism::MODE_DKS);
                        }
                        KeyCode::Char('D') if app.tab == 3 => {
                            app.set_all_key_modes(magnetism::MODE_DKS);
                        }
                        KeyCode::Char('s') if app.tab == 3 => {
                            app.set_key_mode(magnetism::MODE_SNAPTAP);
                        }
                        KeyCode::Char('S') if app.tab == 3 => {
                            app.set_all_key_modes(magnetism::MODE_SNAPTAP);
                        }
                        // Per-key color mode in LED Settings tab
                        KeyCode::Char('p') if app.tab == 1 => {
                            app.apply_per_key_color();
                        }
                        // Depth tab controls
                        KeyCode::Char('v') if app.tab == 2 => {
                            app.toggle_depth_view();
                        }
                        // Triggers tab view toggle
                        KeyCode::Char('v') if app.tab == 3 => {
                            app.toggle_trigger_view();
                        }
                        KeyCode::Char('x') if app.tab == 2 => {
                            app.clear_depth_data();
                        }
                        KeyCode::Char(' ') if app.tab == 2 => {
                            // Select/deselect key at cursor in bar chart mode
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
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.read_input_reports();
            last_tick = Instant::now();

            // Refresh battery every 30 seconds for wireless devices
            if app.is_wireless && app.last_battery_check.elapsed() >= Duration::from_secs(30) {
                app.refresh_battery();
            }
        }
    }

    // Cleanup
    if app.depth_monitoring {
        if let Some(ref device) = app.device {
            device.set_magnetism_report(false);
        }
    }
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(3),  // Tabs
            Constraint::Min(10),    // Content
            Constraint::Length(3),  // Status bar
        ])
        .split(f.area());

    // Title - show device name if connected, otherwise generic title
    let title_text = if app.connected && !app.device_name.is_empty() {
        format!("{} - Configuration Tool", app.device_name)
    } else {
        "MonsGeek/Akko Keyboard - Configuration Tool".to_string()
    };
    let title = Paragraph::new(title_text)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Tabs
    let tabs = Tabs::new(vec!["Device Info", "LED Settings", "Key Depth", "Triggers", "Options", "Macros"])
        .select(app.tab)
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL).title("Tabs [Tab/Shift+Tab]"));
    f.render_widget(tabs, chunks[1]);

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
    let status_color = if app.connected { Color::Green } else { Color::Red };
    let conn_status = if app.connected { "Connected" } else { "Disconnected" };
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
            format!(" {}{}%", icon, batt.level)
        } else {
            " ?%".to_string()
        }
    } else {
        String::new()
    };

    let monitoring_str = if app.depth_monitoring { " MONITORING" } else { "" };

    let status = Paragraph::new(format!(
        " [{}{}{}] {} | ?:Help q:Quit{}",
        conn_status, profile_str, battery_str, app.status_msg, monitoring_str
    ))
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
    let mut tui_lines: Vec<Line> = vec![
        Line::from(Span::styled("── Global ──", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
    ];

    let mut current_context = KeyContext::Global;
    for kb in TUI_KEYBINDS {
        if kb.context != current_context {
            current_context = kb.context;
            let section_name = match current_context {
                KeyContext::Global => "Global",
                KeyContext::Led => "LED Tab",
                KeyContext::Depth => "Depth Tab",
                KeyContext::Triggers => "Triggers Tab",
                KeyContext::Macros => "Macros Tab",
            };
            tui_lines.push(Line::from(""));
            tui_lines.push(Line::from(Span::styled(
                format!("── {section_name} ──"),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
        }
        tui_lines.push(Line::from(vec![
            Span::styled(format!("{:14}", kb.keys), Style::default().fg(Color::Cyan)),
            Span::raw(" "),
            Span::raw(kb.description),
        ]));
    }

    let tui_help = Paragraph::new(tui_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(" TUI Shortcuts [? to close] ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)))
        .wrap(Wrap { trim: false });
    f.render_widget(tui_help, columns[0]);

    // Right column: Physical Keyboard Shortcuts
    let mut kb_lines: Vec<Line> = vec![
        Line::from(Span::styled("── Profiles ──", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
    ];

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
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
        }
        for (key, desc) in &KEYBOARD_SHORTCUTS[start..end] {
            kb_lines.push(Line::from(vec![
                Span::styled(format!("{:14}", key), Style::default().fg(Color::Magenta)),
                Span::raw(" "),
                Span::raw(*desc),
            ]));
        }
    }

    let kb_help = Paragraph::new(kb_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(" Physical Keyboard (Fn+key) ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)))
        .wrap(Wrap { trim: false });
    f.render_widget(kb_help, columns[1]);
}

fn render_device_info(f: &mut Frame, app: &App, area: Rect) {
    let info = &app.info;

    let text = vec![
        Line::from(vec![
            Span::raw("Device:         "),
            Span::styled(&app.device_name, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw("Key Count:      "),
            Span::styled(format!("{}", app.key_count), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::raw("Device ID:      "),
            Span::styled(format!("{} (0x{:04X})", info.device_id, info.device_id), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::raw("Firmware:       "),
            Span::styled(format!("v{}.{:02}", info.version / 100, info.version % 100), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::raw("Profile:        "),
            Span::styled(format!("{} (1-4)", info.profile + 1), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("Debounce:       "),
            Span::styled(format!("{} ms", info.debounce), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("Polling Rate:   "),
            Span::styled(
                if info.polling_rate > 0 {
                    crate::protocol::polling_rate::name(info.polling_rate)
                } else {
                    "N/A".to_string()
                },
                Style::default().fg(Color::Cyan)
            ),
        ]),
        Line::from(vec![
            Span::raw("Fn Layer:       "),
            Span::styled(format!("{}", info.fn_layer), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("WASD Swap:      "),
            Span::styled(if info.wasd_swap { "Yes" } else { "No" }, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("Precision:      "),
            Span::styled(MonsGeekDevice::precision_str(info.precision), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::raw("Sleep:          "),
            Span::styled(format!("{} sec ({} min)", info.sleep_seconds, info.sleep_seconds / 60), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("LED Mode:       "),
            Span::styled(
                format!("{} ({})", info.led_mode, cmd::led_mode_name(info.led_mode)),
                Style::default().fg(Color::Magenta)
            ),
        ]),
        Line::from(vec![
            Span::raw("LED Color:      "),
            Span::styled(
                format!("RGB({}, {}, {}) #{:02X}{:02X}{:02X}",
                    info.led_r, info.led_g, info.led_b,
                    info.led_r, info.led_g, info.led_b),
                Style::default().fg(Color::Rgb(info.led_r, info.led_g, info.led_b))
            ),
        ]),
        Line::from(vec![
            Span::raw("Brightness:     "),
            Span::styled(format!("{}/4", info.led_brightness), Style::default().fg(Color::Magenta)),
        ]),
        Line::from(vec![
            Span::raw("Speed:          "),
            Span::styled(format!("{}/4", 4 - info.led_speed.min(4)), Style::default().fg(Color::Magenta)),
        ]),
    ];

    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Device Information [r to refresh]"));
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
                format!("< {} ({}) >", info.led_mode, cmd::led_mode_name(info.led_mode)),
                Style::default().fg(Color::Yellow)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Brightness: "),
            Span::styled(
                format!("< {}/4 >  {}", info.led_brightness, "█".repeat(info.led_brightness as usize)),
                Style::default().fg(Color::Yellow)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Speed:      "),
            Span::styled(
                format!("< {}/4 >  {}", speed, "█".repeat(speed as usize)),
                Style::default().fg(Color::Yellow)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Red:        "),
            Span::styled(
                format!("< {} >", rgb_bar(info.led_r)),
                Style::default().fg(Color::Red)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Green:      "),
            Span::styled(
                format!("< {} >", rgb_bar(info.led_g)),
                Style::default().fg(Color::Green)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Blue:       "),
            Span::styled(
                format!("< {} >", rgb_bar(info.led_b)),
                Style::default().fg(Color::Blue)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Preview:    "),
            Span::styled(
                format!("████████ #{:02X}{:02X}{:02X}", info.led_r, info.led_g, info.led_b),
                Style::default().fg(Color::Rgb(info.led_r, info.led_g, info.led_b))
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Dazzle:     "),
            Span::styled(
                if info.led_dazzle { "< ON (rainbow) >" } else { "< OFF >" },
                Style::default().fg(if info.led_dazzle { Color::Magenta } else { Color::Gray })
            ),
        ])),
        // Side LED section
        ListItem::new(Line::from(vec![
            Span::styled("─── Side LEDs (Side Lights) ───", Style::default().fg(Color::DarkGray)),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Mode:       "),
            Span::styled(
                format!("< {} ({}) >", info.side_mode, cmd::led_mode_name(info.side_mode)),
                Style::default().fg(Color::Cyan)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Brightness: "),
            Span::styled(
                format!("< {}/4 >  {}", info.side_brightness, "█".repeat(info.side_brightness as usize)),
                Style::default().fg(Color::Cyan)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Speed:      "),
            Span::styled(
                format!("< {}/4 >  {}", 4 - info.side_speed.min(4), "█".repeat((4 - info.side_speed.min(4)) as usize)),
                Style::default().fg(Color::Cyan)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Red:        "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_r)),
                Style::default().fg(Color::Red)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Green:      "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_g)),
                Style::default().fg(Color::Green)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Blue:       "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_b)),
                Style::default().fg(Color::Blue)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Dazzle:     "),
            Span::styled(
                if info.side_dazzle { "< ON (rainbow) >" } else { "< OFF >" },
                Style::default().fg(if info.side_dazzle { Color::Magenta } else { Color::Gray })
            ),
        ])),
    ];

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("LED Settings [←/→ adjust, ↑/↓ select, p=per-key mode]"))
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");

    let mut state = ListState::default();
    state.select(Some(app.selected.min(15)));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_depth_monitor(f: &mut Frame, app: &App, area: Rect) {
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(5), Constraint::Length(3)])
        .split(area);

    // Status and mode indicator
    let mode_str = match app.depth_view_mode {
        DepthViewMode::BarChart => "Bar Chart",
        DepthViewMode::TimeSeries => "Time Series",
    };
    let status_text = if app.depth_monitoring {
        vec![
            Line::from(vec![
                Span::styled("MONITORING ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("| View: "),
                Span::styled(mode_str, Style::default().fg(Color::Cyan)),
                Span::raw(" | Active keys: "),
                Span::styled(format!("{}", app.active_keys.len()), Style::default().fg(Color::Yellow)),
                Span::raw(" | Selected: "),
                Span::styled(format!("{}", app.selected_keys.len()), Style::default().fg(Color::Magenta)),
            ]),
            Line::from("Press 'v' to switch view, Space to select key, 'x' to clear data"),
        ]
    } else {
        vec![
            Line::from(Span::styled("Key depth monitoring is OFF - press 'm' to start", Style::default().fg(Color::Yellow))),
            Line::from(""),
        ]
    };
    let status = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL).title("Monitor Status"));
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

fn render_depth_bar_chart(f: &mut Frame, app: &App, area: Rect) {
    // Find max depth for normalization (minimum 0.1mm to avoid division by zero)
    let max_depth = app.key_depths.iter().cloned().fold(0.1_f32, f32::max);

    // Show all keys with non-zero depth as a single row of bars
    let mut bar_data: Vec<(String, u64)> = Vec::new();

    for (i, &depth) in app.key_depths.iter().enumerate() {
        if depth > 0.01 || app.active_keys.contains(&i) {
            // Normalize to 100 based on max depth
            let depth_pct = ((depth / max_depth) * 100.0).min(100.0) as u64;
            let label = get_key_label(i);
            bar_data.push((label, depth_pct));
        }
    }

    // If no active keys, show placeholder
    if bar_data.is_empty() {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled("No keys pressed", Style::default().fg(Color::DarkGray))),
            Line::from("Press keys to see their depth"),
        ];
        let para = Paragraph::new(text)
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(format!("Key Depths (max: {max_depth:.1}mm)")));
        f.render_widget(para, area);
        return;
    }

    // Convert to references for BarChart
    let bar_refs: Vec<(&str, u64)> = bar_data.iter().map(|(s, v)| (s.as_str(), *v)).collect();

    let chart = BarChart::default()
        .block(Block::default().borders(Borders::ALL).title(format!("Key Depths (max: {max_depth:.2}mm)")))
        .bar_width(3)
        .bar_gap(1)
        .bar_style(Style::default().fg(Color::Cyan))
        .value_style(Style::default().fg(Color::White).add_modifier(Modifier::DIM))
        .data(&bar_refs);

    f.render_widget(chart, area);
}

fn render_depth_time_series(f: &mut Frame, app: &App, area: Rect) {
    // Find all keys with history data (any non-empty history)
    let mut active_keys: Vec<usize> = app.depth_history
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
            Line::from(Span::styled("No key activity recorded yet", Style::default().fg(Color::Yellow))),
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
        Color::Cyan, Color::Yellow, Color::Green, Color::Magenta,
        Color::Red, Color::Blue, Color::LightCyan, Color::LightYellow,
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
                    .style(Style::default().fg(color))
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
                0 => "C", 1 => "Y", 2 => "G", 3 => "M",
                4 => "R", 5 => "B", 6 => "c", 7 => "y",
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
        .block(Block::default()
            .borders(Borders::ALL)
            .title(format!("Time Series: {legend}")))
        .x_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds([x_min.max(0.0), x_max])
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
                ])
        );

    f.render_widget(chart, area);
}

fn render_trigger_settings(f: &mut Frame, app: &App, area: Rect) {
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
            Constraint::Length(8),   // Summary
            Constraint::Min(10),     // Key list
        ])
        .split(area);

    // Summary section
    let factor = app.precision_factor;
    let precision_str = if factor >= 200.0 { "0.005mm" }
        else if factor >= 100.0 { "0.01mm" }
        else { "0.1mm" };

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
        let num_keys = triggers.key_modes.len().min(triggers.press_travel.len() / 2);

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
            Line::from(vec![
                Span::styled("Global Settings (all keys same): ", Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::raw("  Actuation: "),
                Span::styled(format!("{:.2}mm", first_press as f32 / factor), Style::default().fg(Color::Cyan)),
                Span::raw("  |  Release: "),
                Span::styled(format!("{:.2}mm", first_lift as f32 / factor), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::raw("  RT Press: "),
                Span::styled(format!("{:.2}mm", first_rt_press as f32 / factor), Style::default().fg(Color::Yellow)),
                Span::raw("   |  RT Release: "),
                Span::styled(format!("{:.2}mm", first_rt_lift as f32 / factor), Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::raw("  Mode: "),
                Span::styled(magnetism::mode_name(first_mode), Style::default().fg(Color::Magenta)),
            ]),
        ]
    } else {
        vec![
            Line::from(Span::styled("No trigger data loaded", Style::default().fg(Color::Red))),
            Line::from(""),
            Line::from("Press 'r' to load trigger settings from device"),
        ]
    };

    let summary_block = Paragraph::new(summary)
        .block(Block::default().borders(Borders::ALL).title("Trigger Settings Summary"));
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

        let num_keys = triggers.key_modes.len().min(triggers.press_travel.len() / 2);

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
                    Cell::from(if key_name.is_empty() { "-".to_string() } else { key_name }),
                    Cell::from(format!("{:.2}", press as f32 / factor)),
                    Cell::from(format!("{:.2}", lift as f32 / factor)),
                    Cell::from(format!("{:.2}", rt_p as f32 / factor)),
                    Cell::from(format!("{:.2}", rt_l as f32 / factor)),
                    Cell::from(magnetism::mode_name(mode)),
                ])
            })
            .collect();

        let header = Row::new(vec!["#", "Key", "Act", "Rel", "RT↓", "RT↑", "Mode"])
            .style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan));

        let table = Table::new(rows, [
            Constraint::Length(4),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(14),
        ])
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
            Line::from(Span::styled("Controls:", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  r - Reload trigger settings from device"),
            Line::from("  v - Toggle layout/list view"),
            Line::from("  ↑/↓ - Scroll through keys"),
            Line::from(""),
            Line::from(Span::styled("Mode Switching (all keys):", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  n - Normal mode"),
            Line::from("  t - Rapid Trigger mode"),
            Line::from("  d - DKS mode"),
            Line::from("  s - SnapTap mode"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Per-Key Settings"));
        f.render_widget(help, chunks[1]);
    }
}

/// Render trigger settings as a keyboard layout view
fn render_trigger_layout(f: &mut Frame, app: &App, area: Rect) {
    // Split into layout area and detail area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),      // Keyboard layout
            Constraint::Length(9),    // Selected key details
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
        Block::default().borders(Borders::ALL).title("Keyboard Layout [↑↓←→:Navigate v:List n/t/d/s:Mode]"),
        layout_area
    );

    // Calculate key cell dimensions
    // Layout is 16 main columns + nav cluster (5 more columns) = ~21 columns
    // 6 rows
    let key_width = 5u16;  // Width of each key cell
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
        let mode = app.triggers.as_ref()
            .and_then(|t| t.key_modes.get(pos).copied())
            .unwrap_or(0);

        let mode_color = match key_mode::base_mode(mode) {
            0 => Color::White,      // Normal
            0x80 => Color::Yellow,  // RT
            2 => Color::Magenta,    // DKS
            7 => Color::Cyan,       // SnapTap
            _ => Color::Gray,
        };

        let style = if is_selected {
            Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
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
                Span::styled(&key_name, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("  |  Mode: "),
                Span::styled(magnetism::mode_name(mode), Style::default().fg(Color::Magenta)),
            ]),
            Line::from(vec![
                Span::raw("Actuation: "),
                Span::styled(format!("{:.2}mm", press as f32 / factor), Style::default().fg(Color::Cyan)),
                Span::raw("  |  Release: "),
                Span::styled(format!("{:.2}mm", lift as f32 / factor), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::raw("RT Press: "),
                Span::styled(format!("{:.2}mm", rt_press as f32 / factor), Style::default().fg(Color::Yellow)),
                Span::raw("   |  RT Release: "),
                Span::styled(format!("{:.2}mm", rt_lift as f32 / factor), Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::raw("Deadzone: Bottom "),
                Span::styled(format!("{:.2}mm", bottom_dz as f32 / factor), Style::default().fg(Color::Green)),
                Span::raw("  |  Top "),
                Span::styled(format!("{:.2}mm", top_dz as f32 / factor), Style::default().fg(Color::Green)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "n/t/d/s: Set this key  |  N/T/D/S: Set ALL keys",
                Style::default().fg(Color::DarkGray)
            )),
        ];

        let detail_block = Paragraph::new(details)
            .block(Block::default().borders(Borders::ALL).title(format!("Selected Key Details [{key_name}]")));
        f.render_widget(detail_block, chunks[1]);
    } else {
        let help = Paragraph::new("Press 'r' to load trigger settings")
            .block(Block::default().borders(Borders::ALL).title("Selected Key Details"));
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
                    Style::default().fg(Color::Yellow)
                ),
                Span::styled("  (0-3)", Style::default().fg(Color::DarkGray)),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("WASD Swap:      "),
                Span::styled(
                    if opts.wasd_swap { "< ON >" } else { "< OFF >" },
                    Style::default().fg(if opts.wasd_swap { Color::Green } else { Color::Gray })
                ),
                Span::styled("  (swap WASD/Arrow keys)", Style::default().fg(Color::DarkGray)),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("Anti-Mistouch:  "),
                Span::styled(
                    if opts.anti_mistouch { "< ON >" } else { "< OFF >" },
                    Style::default().fg(if opts.anti_mistouch { Color::Green } else { Color::Gray })
                ),
                Span::styled("  (prevent accidental key presses)", Style::default().fg(Color::DarkGray)),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("RT Stability:   "),
                Span::styled(
                    format!("< {}ms >", opts.rt_stability),
                    Style::default().fg(Color::Cyan)
                ),
                Span::styled("  (0-125ms, delay for stability)", Style::default().fg(Color::DarkGray)),
            ])),
            ListItem::new(Line::from("")),
            ListItem::new(Line::from(vec![
                Span::styled("Read-Only Info:", Style::default().add_modifier(Modifier::BOLD)),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("OS Mode:        "),
                Span::styled(os_mode_str, Style::default().fg(Color::Magenta)),
            ])),
        ];

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Keyboard Options [←/→ adjust, ↑/↓ select, Enter to toggle]"))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol("> ");

        let mut state = ListState::default();
        state.select(Some(app.selected.min(3)));
        f.render_stateful_widget(list, area, &mut state);
    } else {
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("No options loaded", Style::default().fg(Color::Red))),
            Line::from(""),
            Line::from("Press 'r' to load keyboard options from device"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Keyboard Options"));
        f.render_widget(help, area);
    }
}

fn render_macros(f: &mut Frame, app: &App, area: Rect) {
    use crate::protocol::hid::key_name;

    // Split into macro list (left) and detail/edit area (right)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Left panel: Macro list
    if app.macros.is_empty() {
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("No macros loaded", Style::default().fg(Color::Yellow))),
            Line::from(""),
            Line::from("Press 'r' to load macros from device"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Macros"));
        f.render_widget(help, chunks[0]);
    } else {
        let items: Vec<ListItem> = app.macros.iter().enumerate().map(|(i, m)| {
            let status = if m.events.is_empty() {
                Span::styled("(empty)", Style::default().fg(Color::DarkGray))
            } else {
                Span::styled(
                    format!("{} (x{})", &m.text_preview, m.repeat_count),
                    Style::default().fg(Color::Green)
                )
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("M{i}: "), Style::default().fg(Color::Cyan)),
                status,
            ]))
        }).collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Macros [↑/↓ select]"))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol("> ");

        let mut state = ListState::default();
        state.select(Some(app.macro_selected.min(app.macros.len().saturating_sub(1))));
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
                Span::styled(&app.macro_edit_text, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("█", Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(Span::styled("Type your macro text, then press Enter to save", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled("Press Escape to cancel", Style::default().fg(Color::DarkGray))),
        ])
        .block(Block::default().borders(Borders::ALL).title("Edit Macro (typing mode)"));
        f.render_widget(edit_text, right_chunks[0]);
    } else if !app.macros.is_empty() && app.macro_selected < app.macros.len() {
        // Detail view
        let m = &app.macros[app.macro_selected];
        let mut lines = vec![
            Line::from(vec![
                Span::styled(format!("Macro {}", app.macro_selected), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("Repeat: "),
                Span::styled(format!("{}", m.repeat_count), Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::raw("Events: "),
                Span::styled(format!("{}", m.events.len()), Style::default().fg(Color::Yellow)),
            ]),
            Line::from(""),
        ];

        // Show events (up to 10)
        if !m.events.is_empty() {
            lines.push(Line::from(Span::styled("Events:", Style::default().add_modifier(Modifier::BOLD))));
            for (i, evt) in m.events.iter().take(12).enumerate() {
                let arrow = if evt.is_down { "↓" } else { "↑" };
                let delay_str = if evt.delay_ms > 0 {
                    format!(" +{}ms", evt.delay_ms)
                } else {
                    String::new()
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{i:2}: "), Style::default().fg(Color::DarkGray)),
                    Span::styled(arrow, Style::default().fg(if evt.is_down { Color::Green } else { Color::Red })),
                    Span::raw(" "),
                    Span::styled(key_name(evt.keycode), Style::default().fg(Color::Yellow)),
                    Span::styled(delay_str, Style::default().fg(Color::DarkGray)),
                ]));
            }
            if m.events.len() > 12 {
                lines.push(Line::from(Span::styled(
                    format!("... and {} more", m.events.len() - 12),
                    Style::default().fg(Color::DarkGray)
                )));
            }
        } else {
            lines.push(Line::from(Span::styled("(empty)", Style::default().fg(Color::DarkGray))));
        }

        let detail = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Macro Details"));
        f.render_widget(detail, right_chunks[0]);
    } else {
        let empty = Paragraph::new("Select a macro")
            .block(Block::default().borders(Borders::ALL).title("Macro Details"));
        f.render_widget(empty, right_chunks[0]);
    }

    // Help text at bottom
    let help_lines = if app.macro_editing {
        vec![
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(" Save  "),
                Span::styled("Escape", Style::default().fg(Color::Yellow)),
                Span::raw(" Cancel"),
            ]),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled("e", Style::default().fg(Color::Yellow)),
                Span::raw(" Edit macro  "),
                Span::styled("c", Style::default().fg(Color::Yellow)),
                Span::raw(" Clear macro  "),
                Span::styled("r", Style::default().fg(Color::Yellow)),
                Span::raw(" Refresh"),
            ]),
        ]
    };
    let help = Paragraph::new(help_lines)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title("Keys"));
    f.render_widget(help, right_chunks[1]);
}
