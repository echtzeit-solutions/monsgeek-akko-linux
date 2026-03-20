// Device Info tab (Tab 0) — types, App methods, and render function

use ratatui::{prelude::*, widgets::*};

use crate::cmd;
use crate::firmware_api::FirmwareCheckResult;
use crate::hid::BatteryInfo;
use crate::power_supply::{find_dongle_battery_power_supply, read_kernel_battery};
use monsgeek_keyboard::KeyboardOptions as KbOptions;
use monsgeek_keyboard::{
    led::{speed_from_wire, speed_to_wire},
    LedMode, LedParams, RgbColor, SleepTimeSettings,
};
use monsgeek_transport::Transport;

use crate::tui::shared::{AsyncResult, BatterySource, LoadState, PatchInfoData, SleepField};
use crate::tui::App;

/// Which color is being edited with hex input
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(in crate::tui) enum HexColorTarget {
    #[default]
    MainLed,
    SideLed,
}

/// Tag identifying what each row in the Device Info list controls
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(in crate::tui) enum InfoTag {
    #[default]
    ReadOnly,
    Separator,
    Device,
    FirmwareCheck,
    Profile,
    Debounce,
    PollingRate,
    LedMode,
    LedBrightness,
    LedSpeed,
    LedRed,
    LedGreen,
    LedBlue,
    LedColorHex,
    LedDazzle,
    SideMode,
    SideBrightness,
    SideSpeed,
    SideRed,
    SideGreen,
    SideBlue,
    SideColorHex,
    SideDazzle,
    FnLayer,
    WasdSwap,
    AntiMistouch,
    RtStability,
    SleepIdleBt,
    SleepIdle24g,
    SleepDeepBt,
    SleepDeep24g,
}

impl App {
    /// Load all device info (all queries for tabs 0/1)
    /// Spawns background tasks to avoid blocking the UI
    pub(in crate::tui) fn load_device_info(&mut self) {
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
        self.loading.patch_info = LoadState::Loading;

        // Spawn background tasks for each query
        let tx = self.gen_sender();

        // Device ID + Version (combined query)
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = match (kb.get_device_id(), kb.get_version()) {
                    (Ok(id), Ok(ver)) => Ok((id, ver)),
                    (Err(e), _) | (_, Err(e)) => Err(e.to_string()),
                };
                tx.send(AsyncResult::DeviceIdAndVersion(result));
            });
        }

        // Profile
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_profile().map_err(|e| e.to_string());
                tx.send(AsyncResult::Profile(result));
            });
        }

        // Debounce
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_debounce().map_err(|e| e.to_string());
                tx.send(AsyncResult::Debounce(result));
            });
        }

        // Polling rate
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb
                    .get_polling_rate()
                    .map(|r| r as u16)
                    .map_err(|e| e.to_string());
                tx.send(AsyncResult::PollingRate(result));
            });
        }

        // LED params
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_led_params().map_err(|e| e.to_string());
                tx.send(AsyncResult::LedParams(result));
            });
        }

        // Side LED params
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_side_led_params().map_err(|e| e.to_string());
                tx.send(AsyncResult::SideLedParams(result));
            });
        }

        // KB options
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_kb_options().map_err(|e| e.to_string());
                tx.send(AsyncResult::KbOptions(result));
            });
        }

        // Feature list
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_precision().map_err(|e| e.to_string());
                tx.send(AsyncResult::Precision(result));
            });
        }

        // Sleep time
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = kb.get_sleep_time().map_err(|e| e.to_string());
                tx.send(AsyncResult::SleepTime(result));
            });
        }

        // Patch info (0xE7)
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                use crate::protocol::patch_info;
                use monsgeek_transport::ChecksumType;

                let result =
                    match kb
                        .transport()
                        .query_raw(patch_info::CMD, &[], ChecksumType::Bit7)
                    {
                        Ok(resp) => {
                            let offsets = if resp.len() >= 6
                                && resp[0] == patch_info::MAGIC_HI
                                && resp[1] == patch_info::MAGIC_LO
                            {
                                Some((2, 3, 5))
                            } else if resp.len() >= 7
                                && resp[1] == patch_info::MAGIC_HI
                                && resp[2] == patch_info::MAGIC_LO
                            {
                                Some((3, 4, 6))
                            } else {
                                None
                            };
                            match offsets {
                                Some((ver_off, caps_off, name_off)) => {
                                    let version = resp[ver_off];
                                    let caps =
                                        u16::from_le_bytes([resp[caps_off], resp[caps_off + 1]]);
                                    let name_end = resp.len().min(name_off + 9);
                                    let name_bytes = &resp[name_off..name_end];
                                    let name_len = name_bytes
                                        .iter()
                                        .position(|&b| b == 0)
                                        .unwrap_or(name_bytes.len());
                                    let name = String::from_utf8_lossy(&name_bytes[..name_len])
                                        .to_string();
                                    Ok(PatchInfoData {
                                        name,
                                        version,
                                        capabilities: patch_info::capability_names(caps),
                                    })
                                }
                                None => Err("stock firmware".to_string()),
                            }
                        }
                        Err(e) => Err(e.to_string()),
                    };
                tx.send(AsyncResult::PatchInfo(result));
            });
        }

        // Dongle patch info (Feature Report ID 8)
        {
            let kb = keyboard.clone();
            let tx = tx.clone();
            self.loading.dongle_patch_info = LoadState::Loading;
            tokio::spawn(async move {
                use crate::protocol::patch_info;

                let result = match kb.get_dongle_patch_info() {
                    Ok(Some(pi)) => Ok(PatchInfoData {
                        name: pi.name,
                        version: pi.version,
                        capabilities: patch_info::capability_names(pi.capabilities),
                    }),
                    Ok(None) => Err("not available".to_string()),
                    Err(e) => Err(e.to_string()),
                };
                tx.send(AsyncResult::DonglePatchInfo(result));
            });
        }
    }

    pub(in crate::tui) fn refresh_dongle_status(&mut self) {
        if !self.is_wireless {
            return;
        }
        if let Some(ref keyboard) = self.keyboard {
            self.dongle_status = keyboard.transport().query_dongle_status().ok().flatten();
        }
    }

    pub(in crate::tui) fn refresh_battery(&mut self) {
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
                let tx = self.gen_sender();
                tokio::spawn(async move {
                    let result = keyboard
                        .get_battery()
                        .map(|kb_info| BatteryInfo {
                            level: kb_info.level,
                            online: kb_info.online,
                            charging: kb_info.charging,
                            idle: kb_info.idle,
                        })
                        .map_err(|e| e.to_string());
                    tx.send(AsyncResult::Battery(result));
                });
            }
            None => {}
        }
        self.last_battery_check = std::time::Instant::now();
    }

    /// Check for firmware updates from server
    pub(in crate::tui) fn check_firmware(&mut self) {
        if !self.connected || self.loading.firmware_check == LoadState::Loading {
            return;
        }

        let device_id = self.info.device_id;
        let local_version = self.info.version;
        let tx = self.gen_sender();

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

            tx.send(AsyncResult::FirmwareCheck(result));
        });
    }

    /// Send current main LED params to keyboard
    pub(in crate::tui) fn send_main_led(&self) {
        if let Some(ref keyboard) = self.keyboard {
            let _ = keyboard.set_led(
                self.info.led_mode,
                self.info.led_brightness,
                speed_to_wire(self.info.led_speed),
                self.info.led_r,
                self.info.led_g,
                self.info.led_b,
                self.info.led_dazzle,
            );
        }
    }

    pub(in crate::tui) fn set_debounce(&mut self, ms: u8) {
        let ms = ms.min(25);
        if let Some(ref keyboard) = self.keyboard {
            if keyboard.set_debounce(ms).is_ok() {
                self.info.debounce = ms;
                self.status_msg = format!("Debounce: {ms} ms");
            } else {
                self.status_msg = "Failed to set debounce".to_string();
            }
        }
    }

    pub(in crate::tui) fn cycle_polling_rate(&mut self, delta: i32) {
        use crate::protocol::polling_rate;
        let rates = polling_rate::RATES;
        let current_idx = rates
            .iter()
            .position(|&r| r == self.info.polling_rate)
            .unwrap_or(3); // default to 1000Hz
        let new_idx = (current_idx as i32 + delta).clamp(0, rates.len() as i32 - 1) as usize;
        let new_hz = rates[new_idx];
        if let Some(rate_enum) = monsgeek_keyboard::PollingRate::from_hz(new_hz) {
            if let Some(ref keyboard) = self.keyboard {
                if keyboard.set_polling_rate(rate_enum).is_ok() {
                    self.info.polling_rate = new_hz;
                    self.status_msg = format!("Polling rate: {}", polling_rate::name(new_hz));
                } else {
                    self.status_msg = "Failed to set polling rate".to_string();
                }
            }
        }
    }

    pub(in crate::tui) fn set_led_mode(&mut self, mode: u8) {
        self.info.led_mode = mode;
        self.send_main_led();
        self.status_msg = format!("LED mode: {}", cmd::led_mode_name(mode));
    }

    pub(in crate::tui) fn set_brightness(&mut self, brightness: u8) {
        self.info.led_brightness = brightness.min(4);
        self.send_main_led();
        self.status_msg = format!("Brightness: {}/4", self.info.led_brightness);
    }

    pub(in crate::tui) fn set_speed(&mut self, speed: u8) {
        let speed = speed.min(4);
        self.info.led_speed = speed_from_wire(speed);
        self.send_main_led();
        self.status_msg = format!("Speed: {speed}/4");
    }

    pub(in crate::tui) fn set_profile(&mut self, profile: u8) {
        if let Some(ref keyboard) = self.keyboard {
            if keyboard.set_profile(profile).is_ok() {
                self.info.profile = profile;
                self.status_msg = format!("Profile {} active", profile + 1);
                // Reload device info after profile switch
                self.load_device_info();
            } else {
                self.status_msg = "Failed to set profile".to_string();
            }
        }
    }

    pub(in crate::tui) fn set_color(&mut self, r: u8, g: u8, b: u8) {
        self.info.led_r = r;
        self.info.led_g = g;
        self.info.led_b = b;
        self.send_main_led();
        self.status_msg = format!("Color: #{r:02X}{g:02X}{b:02X}");
    }

    pub(in crate::tui) fn toggle_dazzle(&mut self) {
        self.info.led_dazzle = !self.info.led_dazzle;
        self.send_main_led();
        self.status_msg = format!(
            "Dazzle: {}",
            if self.info.led_dazzle { "ON" } else { "OFF" }
        );
    }

    /// Start hex color input mode
    pub(in crate::tui) fn start_hex_input(&mut self, target: HexColorTarget) {
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
    pub(in crate::tui) fn cancel_hex_input(&mut self) {
        self.hex_editing = false;
        self.hex_input.clear();
        self.status_msg.clear();
    }

    /// Apply hex color input
    pub(in crate::tui) fn apply_hex_input(&mut self) {
        if let Some((r, g, b)) = parse_hex_color(&self.hex_input) {
            match self.hex_target {
                HexColorTarget::MainLed => self.set_color(r, g, b),
                HexColorTarget::SideLed => self.set_side_color(r, g, b),
            }
        } else {
            self.status_msg = format!("Invalid hex color: {}", self.hex_input);
        }
        self.hex_editing = false;
        self.hex_input.clear();
    }

    // Side LED methods

    /// Build current side LED params from state
    fn current_side_led_params(&self) -> LedParams {
        LedParams {
            mode: LedMode::from_u8(self.info.side_mode).unwrap_or(LedMode::Off),
            brightness: self.info.side_brightness,
            speed: speed_to_wire(self.info.side_speed),
            color: RgbColor::new(self.info.side_r, self.info.side_g, self.info.side_b),
            direction: if self.info.side_dazzle { 7 } else { 8 },
        }
    }

    /// Send current side LED params to keyboard
    fn send_side_led(&self) {
        if let Some(ref keyboard) = self.keyboard {
            let _ = keyboard.set_side_led_params(&self.current_side_led_params());
        }
    }

    pub(in crate::tui) fn set_side_mode(&mut self, mode: u8) {
        self.info.side_mode = mode;
        self.send_side_led();
        self.status_msg = format!("Side LED mode: {}", cmd::led_mode_name(mode));
    }

    pub(in crate::tui) fn set_side_brightness(&mut self, brightness: u8) {
        self.info.side_brightness = brightness.min(4);
        self.send_side_led();
        self.status_msg = format!("Side brightness: {}/4", self.info.side_brightness);
    }

    pub(in crate::tui) fn set_side_speed(&mut self, speed: u8) {
        let speed = speed.min(4);
        self.info.side_speed = speed_from_wire(speed);
        self.send_side_led();
        self.status_msg = format!("Side speed: {speed}/4");
    }

    pub(in crate::tui) fn set_side_color(&mut self, r: u8, g: u8, b: u8) {
        self.info.side_r = r;
        self.info.side_g = g;
        self.info.side_b = b;
        self.send_side_led();
        self.status_msg = format!("Side color: #{r:02X}{g:02X}{b:02X}");
    }

    pub(in crate::tui) fn toggle_side_dazzle(&mut self) {
        self.info.side_dazzle = !self.info.side_dazzle;
        self.send_side_led();
        self.status_msg = format!(
            "Side dazzle: {}",
            if self.info.side_dazzle { "ON" } else { "OFF" }
        );
    }

    pub(in crate::tui) fn apply_per_key_color(&mut self) {
        let (r, g, b) = (self.info.led_r, self.info.led_g, self.info.led_b);
        if let Some(ref keyboard) = self.keyboard {
            if keyboard
                .set_all_keys_color(RgbColor::new(r, g, b), 0)
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
    pub(in crate::tui) fn load_options(&mut self) {
        let Some(keyboard) = self.keyboard.clone() else {
            return;
        };

        self.loading.options = LoadState::Loading;
        let tx = self.gen_sender();
        tokio::spawn(async move {
            let result = keyboard.get_kb_options().map_err(|e| e.to_string());
            tx.send(AsyncResult::Options(result));
        });
    }

    fn save_options(&mut self) {
        if let Some(ref opts) = self.options {
            if let Some(ref keyboard) = self.keyboard {
                let kb_opts = KbOptions {
                    os_mode: opts.os_mode,
                    fn_layer: opts.fn_layer,
                    anti_mistouch: opts.anti_mistouch,
                    rt_stability: opts.rt_stability,
                    wasd_swap: opts.wasd_swap,
                };
                if keyboard.set_kb_options(&kb_opts).is_ok() {
                    self.status_msg = "Options saved".to_string();
                } else {
                    self.status_msg = "Failed to save options".to_string();
                }
            }
        }
    }

    pub(in crate::tui) fn set_fn_layer(&mut self, layer: u8) {
        let layer = layer.min(3);
        if let Some(ref mut opts) = self.options {
            opts.fn_layer = layer;
        }
        self.save_options();
        self.status_msg = format!("Fn layer: {layer}");
    }

    pub(in crate::tui) fn toggle_wasd_swap(&mut self) {
        let new_val = self.options.as_ref().map(|o| !o.wasd_swap).unwrap_or(false);
        if let Some(ref mut opts) = self.options {
            opts.wasd_swap = new_val;
        }
        self.save_options();
        self.status_msg = format!("WASD swap: {}", if new_val { "ON" } else { "OFF" });
    }

    pub(in crate::tui) fn toggle_anti_mistouch(&mut self) {
        let new_val = self
            .options
            .as_ref()
            .map(|o| !o.anti_mistouch)
            .unwrap_or(false);
        if let Some(ref mut opts) = self.options {
            opts.anti_mistouch = new_val;
        }
        self.save_options();
        self.status_msg = format!("Anti-mistouch: {}", if new_val { "ON" } else { "OFF" });
    }

    pub(in crate::tui) fn set_rt_stability(&mut self, value: u8) {
        let value = value.min(125);
        if let Some(ref mut opts) = self.options {
            opts.rt_stability = value;
        }
        self.save_options();
        self.status_msg = format!("RT stability: {value}ms");
    }

    /// Update a single sleep time value with validation (deep >= idle)
    pub(in crate::tui) fn update_sleep_time(&mut self, field: SleepField, delta: i32) {
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
            if keyboard.set_sleep_time(&settings).is_ok() {
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
}

/// Parse hex color string (supports #RRGGBB, RRGGBB formats)
pub(in crate::tui) fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}

pub(in crate::tui) fn render_device_info(f: &mut Frame, app: &mut App, area: Rect) {
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

    // Helper to create editable value span with < > spinners
    let editable_span = |state: LoadState, value: String, color: Color| -> Span<'static> {
        match state {
            LoadState::Loaded => Span::styled(format!("< {value} >"), Style::default().fg(color)),
            _ => value_span(state, value, color),
        }
    };

    // Helper to create RGB bar visualization
    let rgb_bar = |val: u8| -> String {
        let bars = (val as usize * 16 / 255).min(16);
        format!("{:3} {}", val, "█".repeat(bars))
    };

    // Build items with tags
    let mut items: Vec<(InfoTag, ListItem)> = Vec::new();

    // Device entry (clickable — opens device picker)
    let device_display = if app.transport_name.is_empty() {
        app.device_name.clone()
    } else {
        format!("{} ({})", app.device_name, app.transport_name)
    };
    items.push((
        InfoTag::Device,
        ListItem::new(Line::from(vec![
            Span::raw("Device:         "),
            Span::styled(
                device_display,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  [d]", Style::default().fg(Color::DarkGray)),
        ])),
    ));
    items.push((
        InfoTag::ReadOnly,
        ListItem::new(Line::from(vec![
            Span::raw("Key Count:      "),
            Span::styled(
                format!("{}", app.key_count),
                Style::default().fg(Color::Green),
            ),
        ])),
    ));
    items.push((
        InfoTag::ReadOnly,
        ListItem::new(Line::from(vec![
            Span::raw("Device ID:      "),
            value_span(
                loading.usb_version,
                format!("{} (0x{:04X})", info.device_id, info.device_id),
                Color::Yellow,
            ),
        ])),
    ));
    items.push((
        InfoTag::ReadOnly,
        ListItem::new(Line::from(vec![
            Span::raw("Firmware:       "),
            value_span(
                loading.usb_version,
                format!("v{:X}", info.version),
                Color::Yellow,
            ),
        ])),
    ));
    items.push((
        InfoTag::ReadOnly,
        ListItem::new(Line::from(vec![
            Span::raw("Patch:          "),
            match loading.patch_info {
                LoadState::NotLoaded | LoadState::Loading => {
                    Span::styled(spinner.to_string(), Style::default().fg(Color::Yellow))
                }
                LoadState::Loaded => {
                    if let Some(ref pi) = app.patch_info {
                        let caps = if pi.capabilities.is_empty() {
                            String::new()
                        } else {
                            format!(" [{}]", pi.capabilities.join(", "))
                        };
                        Span::styled(
                            format!("{} v{}{}", pi.name, pi.version, caps),
                            Style::default().fg(Color::LightCyan),
                        )
                    } else {
                        Span::styled("None".to_string(), Style::default().fg(Color::DarkGray))
                    }
                }
                LoadState::Error => {
                    Span::styled("None".to_string(), Style::default().fg(Color::DarkGray))
                }
            },
        ])),
    ));
    // Dongle patch info (only shown when available via dongle transport)
    if matches!(
        loading.dongle_patch_info,
        LoadState::Loading | LoadState::Loaded
    ) {
        items.push((
            InfoTag::ReadOnly,
            ListItem::new(Line::from(vec![
                Span::raw("Dongle patch:   "),
                match loading.dongle_patch_info {
                    LoadState::NotLoaded | LoadState::Loading => {
                        Span::styled(spinner.to_string(), Style::default().fg(Color::Yellow))
                    }
                    LoadState::Loaded => {
                        if let Some(ref pi) = app.dongle_patch_info {
                            let caps = if pi.capabilities.is_empty() {
                                String::new()
                            } else {
                                format!(" [{}]", pi.capabilities.join(", "))
                            };
                            Span::styled(
                                format!("{} v{}{}", pi.name, pi.version, caps),
                                Style::default().fg(Color::LightCyan),
                            )
                        } else {
                            Span::styled("None".to_string(), Style::default().fg(Color::DarkGray))
                        }
                    }
                    LoadState::Error => {
                        Span::styled("None".to_string(), Style::default().fg(Color::DarkGray))
                    }
                },
            ])),
        ));
    }

    items.push((
        InfoTag::FirmwareCheck,
        ListItem::new(Line::from(vec![
            Span::raw("Update:         "),
            match loading.firmware_check {
                LoadState::NotLoaded => Span::styled(
                    "[Enter] Check".to_string(),
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
        ])),
    ));

    // Settings separator
    items.push((
        InfoTag::Separator,
        ListItem::new(Line::from(Span::styled(
            "─── Settings ───",
            Style::default().fg(Color::DarkGray),
        ))),
    ));

    // Editable settings
    items.push((
        InfoTag::Profile,
        ListItem::new(Line::from(vec![
            Span::raw("Profile:        "),
            editable_span(
                loading.profile,
                format!("{} (1-4)", info.profile + 1),
                Color::Cyan,
            ),
        ])),
    ));
    items.push((
        InfoTag::Debounce,
        ListItem::new(Line::from(vec![
            Span::raw("Debounce:       "),
            editable_span(
                loading.debounce,
                format!("{} ms", info.debounce),
                Color::Cyan,
            ),
        ])),
    ));
    items.push((
        InfoTag::PollingRate,
        ListItem::new(Line::from(vec![
            Span::raw("Polling Rate:   "),
            editable_span(
                loading.polling_rate,
                if info.polling_rate > 0 {
                    crate::protocol::polling_rate::name(info.polling_rate)
                } else {
                    "N/A".to_string()
                },
                Color::Cyan,
            ),
        ])),
    ));
    items.push((
        InfoTag::ReadOnly,
        ListItem::new(Line::from(vec![
            Span::raw("Precision:      "),
            value_span(
                loading.precision,
                app.precision.as_str().to_string(),
                Color::Green,
            ),
        ])),
    ));

    // Options section (fn layer, wasd, anti-mistouch, RT stability, sleep times, OS mode)
    if let Some(ref opts) = app.options {
        items.push((
            InfoTag::FnLayer,
            ListItem::new(Line::from(vec![
                Span::raw("Fn Layer:       "),
                Span::styled(
                    format!("< {} >", opts.fn_layer),
                    Style::default().fg(Color::Cyan),
                ),
            ])),
        ));
        items.push((
            InfoTag::WasdSwap,
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
            ])),
        ));
        items.push((
            InfoTag::AntiMistouch,
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
            ])),
        ));
        items.push((
            InfoTag::RtStability,
            ListItem::new(Line::from(vec![
                Span::raw("RT Stability:   "),
                Span::styled(
                    format!("< {}ms >", opts.rt_stability),
                    Style::default().fg(Color::Cyan),
                ),
            ])),
        ));
        items.push((
            InfoTag::SleepIdleBt,
            ListItem::new(Line::from(vec![
                Span::raw("BT Idle:        "),
                Span::styled(
                    format!("< {} >", SleepTimeSettings::format_duration(opts.idle_bt)),
                    Style::default().fg(Color::Cyan),
                ),
            ])),
        ));
        items.push((
            InfoTag::SleepIdle24g,
            ListItem::new(Line::from(vec![
                Span::raw("2.4G Idle:      "),
                Span::styled(
                    format!("< {} >", SleepTimeSettings::format_duration(opts.idle_24g)),
                    Style::default().fg(Color::Cyan),
                ),
            ])),
        ));
        items.push((
            InfoTag::SleepDeepBt,
            ListItem::new(Line::from(vec![
                Span::raw("BT Deep Sleep:  "),
                Span::styled(
                    format!("< {} >", SleepTimeSettings::format_duration(opts.deep_bt)),
                    Style::default().fg(Color::Green),
                ),
            ])),
        ));
        items.push((
            InfoTag::SleepDeep24g,
            ListItem::new(Line::from(vec![
                Span::raw("2.4G Deep Sleep:"),
                Span::styled(
                    format!("< {} >", SleepTimeSettings::format_duration(opts.deep_24g)),
                    Style::default().fg(Color::Green),
                ),
            ])),
        ));
        let os_mode_str = match opts.os_mode {
            0 => "Windows",
            1 => "macOS",
            2 => "Linux",
            _ => "Unknown",
        };
        items.push((
            InfoTag::ReadOnly,
            ListItem::new(Line::from(vec![
                Span::raw("OS Mode:        "),
                Span::styled(os_mode_str, Style::default().fg(Color::Magenta)),
            ])),
        ));
    } else if loading.options == LoadState::Loading {
        items.push((
            InfoTag::ReadOnly,
            ListItem::new(Line::from(vec![
                Span::raw("Options:        "),
                Span::styled(spinner.to_string(), Style::default().fg(Color::Yellow)),
            ])),
        ));
    } else if loading.options == LoadState::Error {
        items.push((
            InfoTag::ReadOnly,
            ListItem::new(Line::from(vec![
                Span::raw("Options:        "),
                Span::styled("!", Style::default().fg(Color::Red)),
            ])),
        ));
    }

    // LED separator
    items.push((
        InfoTag::Separator,
        ListItem::new(Line::from(Span::styled(
            "─── LED ───",
            Style::default().fg(Color::DarkGray),
        ))),
    ));

    // LED settings (editable)
    items.push((
        InfoTag::LedMode,
        ListItem::new(Line::from(vec![
            Span::raw("Mode:           "),
            editable_span(
                loading.led_params,
                format!("{} ({})", info.led_mode, cmd::led_mode_name(info.led_mode)),
                Color::Yellow,
            ),
        ])),
    ));
    items.push((
        InfoTag::LedBrightness,
        ListItem::new(Line::from(vec![
            Span::raw("Brightness:     "),
            editable_span(
                loading.led_params,
                format!(
                    "{}/4  {}",
                    info.led_brightness,
                    "█".repeat(info.led_brightness as usize)
                ),
                Color::Yellow,
            ),
        ])),
    ));
    let speed = speed_to_wire(info.led_speed);
    items.push((
        InfoTag::LedSpeed,
        ListItem::new(Line::from(vec![
            Span::raw("Speed:          "),
            editable_span(
                loading.led_params,
                format!("{}/4  {}", speed, "█".repeat(speed as usize)),
                Color::Yellow,
            ),
        ])),
    ));
    items.push((
        InfoTag::LedRed,
        ListItem::new(Line::from(vec![
            Span::raw("Red:            "),
            if loading.led_params == LoadState::Loaded {
                Span::styled(
                    format!("< {} >", rgb_bar(info.led_r)),
                    Style::default().fg(Color::Red),
                )
            } else {
                value_span(loading.led_params, String::new(), Color::Red)
            },
        ])),
    ));
    items.push((
        InfoTag::LedGreen,
        ListItem::new(Line::from(vec![
            Span::raw("Green:          "),
            if loading.led_params == LoadState::Loaded {
                Span::styled(
                    format!("< {} >", rgb_bar(info.led_g)),
                    Style::default().fg(Color::Green),
                )
            } else {
                value_span(loading.led_params, String::new(), Color::Green)
            },
        ])),
    ));
    items.push((
        InfoTag::LedBlue,
        ListItem::new(Line::from(vec![
            Span::raw("Blue:           "),
            if loading.led_params == LoadState::Loaded {
                Span::styled(
                    format!("< {} >", rgb_bar(info.led_b)),
                    Style::default().fg(Color::Blue),
                )
            } else {
                value_span(loading.led_params, String::new(), Color::Blue)
            },
        ])),
    ));
    items.push((
        InfoTag::LedColorHex,
        ListItem::new(Line::from(vec![
            Span::raw("Color:          "),
            if app.hex_editing && app.hex_target == HexColorTarget::MainLed {
                Span::styled(
                    format!("████████ [#{}_]", app.hex_input),
                    Style::default()
                        .fg(Color::Rgb(info.led_r, info.led_g, info.led_b))
                        .add_modifier(Modifier::BOLD),
                )
            } else if loading.led_params == LoadState::Loaded {
                Span::styled(
                    format!(
                        "████████ [#{:02X}{:02X}{:02X}]",
                        info.led_r, info.led_g, info.led_b
                    ),
                    Style::default().fg(Color::Rgb(info.led_r, info.led_g, info.led_b)),
                )
            } else {
                value_span(loading.led_params, String::new(), Color::Magenta)
            },
            Span::styled("  Enter to edit", Style::default().fg(Color::DarkGray)),
        ])),
    ));
    items.push((
        InfoTag::LedDazzle,
        ListItem::new(Line::from(vec![
            Span::raw("Dazzle:         "),
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
    ));

    // Side LED section
    if app.has_sidelight {
        items.push((
            InfoTag::Separator,
            ListItem::new(Line::from(Span::styled(
                "─── Side LED ───",
                Style::default().fg(Color::DarkGray),
            ))),
        ));
        items.push((
            InfoTag::SideMode,
            ListItem::new(Line::from(vec![
                Span::raw("Mode:           "),
                Span::styled(
                    format!(
                        "< {} ({}) >",
                        info.side_mode,
                        cmd::led_mode_name(info.side_mode)
                    ),
                    Style::default().fg(Color::Cyan),
                ),
            ])),
        ));
        items.push((
            InfoTag::SideBrightness,
            ListItem::new(Line::from(vec![
                Span::raw("Brightness:     "),
                Span::styled(
                    format!(
                        "< {}/4 >  {}",
                        info.side_brightness,
                        "█".repeat(info.side_brightness as usize)
                    ),
                    Style::default().fg(Color::Cyan),
                ),
            ])),
        ));
        items.push((
            InfoTag::SideSpeed,
            ListItem::new(Line::from(vec![
                Span::raw("Speed:          "),
                Span::styled(
                    format!(
                        "< {}/4 >  {}",
                        speed_to_wire(info.side_speed),
                        "█".repeat(speed_to_wire(info.side_speed) as usize)
                    ),
                    Style::default().fg(Color::Cyan),
                ),
            ])),
        ));
        items.push((
            InfoTag::SideRed,
            ListItem::new(Line::from(vec![
                Span::raw("Red:            "),
                Span::styled(
                    format!("< {} >", rgb_bar(info.side_r)),
                    Style::default().fg(Color::Red),
                ),
            ])),
        ));
        items.push((
            InfoTag::SideGreen,
            ListItem::new(Line::from(vec![
                Span::raw("Green:          "),
                Span::styled(
                    format!("< {} >", rgb_bar(info.side_g)),
                    Style::default().fg(Color::Green),
                ),
            ])),
        ));
        items.push((
            InfoTag::SideBlue,
            ListItem::new(Line::from(vec![
                Span::raw("Blue:           "),
                Span::styled(
                    format!("< {} >", rgb_bar(info.side_b)),
                    Style::default().fg(Color::Blue),
                ),
            ])),
        ));
        items.push((
            InfoTag::SideColorHex,
            ListItem::new(Line::from(vec![
                Span::raw("Color:          "),
                if app.hex_editing && app.hex_target == HexColorTarget::SideLed {
                    Span::styled(
                        format!("████████ [#{}_]", app.hex_input),
                        Style::default()
                            .fg(Color::Rgb(info.side_r, info.side_g, info.side_b))
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
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
        ));
        items.push((
            InfoTag::SideDazzle,
            ListItem::new(Line::from(vec![
                Span::raw("Dazzle:         "),
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
        ));
    }

    // Dongle info section
    if app.is_wireless {
        items.push((
            InfoTag::Separator,
            ListItem::new(Line::from(Span::styled(
                "─── Dongle ───",
                Style::default().fg(Color::DarkGray),
            ))),
        ));
        if let Some(ref di) = app.dongle_info {
            items.push((
                InfoTag::ReadOnly,
                ListItem::new(Line::from(vec![
                    Span::raw("Dongle FW:      "),
                    Span::styled(
                        format!("v{}", di.firmware_version),
                        Style::default().fg(Color::Yellow),
                    ),
                ])),
            ));
        }
        if let Some(ref rf) = app.rf_info {
            items.push((
                InfoTag::ReadOnly,
                ListItem::new(Line::from(vec![
                    Span::raw("RF Address:     "),
                    Span::styled(
                        format!(
                            "{:02X}:{:02X}:{:02X}:{:02X}",
                            rf.rf_address[0], rf.rf_address[1], rf.rf_address[2], rf.rf_address[3]
                        ),
                        Style::default().fg(Color::Yellow),
                    ),
                ])),
            ));
            items.push((
                InfoTag::ReadOnly,
                ListItem::new(Line::from(vec![
                    Span::raw("RF FW Version:  "),
                    Span::styled(
                        format!(
                            "v{}.{}",
                            rf.firmware_version_major, rf.firmware_version_minor
                        ),
                        Style::default().fg(Color::Yellow),
                    ),
                ])),
            ));
        }
        if let Some(ref ds) = app.dongle_status {
            items.push((
                InfoTag::ReadOnly,
                ListItem::new(Line::from(vec![
                    Span::raw("RF Ready:       "),
                    Span::styled(
                        if ds.rf_ready { "Yes" } else { "No" },
                        Style::default().fg(if ds.rf_ready {
                            Color::Green
                        } else {
                            Color::Red
                        }),
                    ),
                ])),
            ));
        }
    }

    // Store tags and build list items
    app.info_tags = items.iter().map(|(tag, _)| *tag).collect();
    let list_items: Vec<ListItem> = items.into_iter().map(|(_, item)| item).collect();
    let max_idx = list_items.len().saturating_sub(1);

    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Device Info [r: refresh, u: check update, ←/→: adjust]"),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    // Clamp cursor to actual list length (list can grow/shrink as async data arrives)
    app.selected = app.selected.min(max_idx);
    let mut state = ListState::default();
    state.select(Some(app.selected));
    f.render_stateful_widget(list, area, &mut state);
}
