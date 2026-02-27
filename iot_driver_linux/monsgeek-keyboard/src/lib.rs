//! High-level keyboard interface for MonsGeek/Akko keyboards
//!
//! This crate provides a convenient API for interacting with keyboard features
//! on top of any transport layer (HID wired, dongle, Bluetooth, etc.)

pub mod error;
pub mod hid_codes;
pub mod led;
pub mod magnetism;
pub mod settings;
pub mod sync;

pub use error::KeyboardError;
pub use led::{LedMode, LedParams, RgbColor};
pub use magnetism::{
    KeyDepthEvent, KeyMode, KeyTriggerSettings, KeyTriggerSettingsDetail, TravelDepth,
    TriggerSettings,
};
pub use settings::{
    BatteryInfo, FeatureList, FirmwareVersion, KeyboardOptions, PollingRate, Precision,
    SleepTimeSettings,
};
pub use sync::{list_keyboards, SyncKeyboard};

/// Information about firmware patches applied to the keyboard
#[derive(Debug, Clone)]
pub struct PatchInfo {
    pub version: u8,
    pub capabilities: u16,
    pub name: String,
}

impl PatchInfo {
    pub fn has_battery(&self) -> bool {
        self.capabilities & 0x01 != 0
    }

    pub fn has_led_stream(&self) -> bool {
        self.capabilities & 0x02 != 0
    }
}

// Macro parsing
// (MacroEvent struct and parse_macro_events fn are defined after KeyboardInterface impl)

/// Number of physical keys on M1 V5 HE
pub const KEY_COUNT_M1_V5: u8 = 98;

/// Total matrix positions for M1 V5 HE (98 active keys + empty positions)
pub const MATRIX_SIZE_M1_V5: usize = 126;

// Re-export VendorEvent and TimestampedEvent for use by consumers (TUI notification handling)
pub use monsgeek_transport::{TimestampedEvent, VendorEvent};

use std::sync::Arc;

use monsgeek_transport::protocol::{cmd, magnetism as mag_cmd};
use monsgeek_transport::{ChecksumType, FlowControlTransport, Transport};
// Typed commands
use monsgeek_transport::command::{
    DebounceResponse, GetFnData, GetKeyMatrixData, GetMacroData, GetMultiMagnetismData,
    LedParamsResponse as TransportLedParamsResponse, PollingRate as TransportPollingRate,
    PollingRateResponse, ProfileResponse, QueryDebounce, QueryLedParams, QueryPollingRate,
    QueryProfile, QuerySleepTime, SetDebounce, SetFnData, SetKeyMagnetismModeData,
    SetKeyMatrixData, SetMacroCommand, SetMagnetismReport, SetMultiMagnetismCommand,
    SetMultiMagnetismHeader, SetPollingRate, SetProfile, SetSleepTime, SleepTimeResponse,
};
use zerocopy::IntoBytes;

/// High-level keyboard interface using any transport
///
/// Provides convenient methods for keyboard features like LED control,
/// key mapping, trigger settings, etc.
pub struct KeyboardInterface {
    transport: Arc<FlowControlTransport>,
    key_count: u8,
    has_magnetism: bool,
}

impl KeyboardInterface {
    /// Create a new keyboard interface
    ///
    /// # Arguments
    /// * `transport` - Flow-controlled transport layer
    /// * `key_count` - Number of keys on the keyboard
    /// * `has_magnetism` - Whether the keyboard has Hall Effect switches
    pub fn new(transport: Arc<FlowControlTransport>, key_count: u8, has_magnetism: bool) -> Self {
        Self {
            transport,
            key_count,
            has_magnetism,
        }
    }

    /// Get the underlying transport
    pub fn transport(&self) -> &Arc<FlowControlTransport> {
        &self.transport
    }

    /// Get number of keys
    pub fn key_count(&self) -> u8 {
        self.key_count
    }

    /// Check if keyboard has magnetism (Hall Effect) support
    pub fn has_magnetism(&self) -> bool {
        self.has_magnetism
    }

    /// Check if using wireless transport
    pub fn is_wireless(&self) -> bool {
        self.transport.device_info().is_wireless()
    }

    /// Check if connected via dongle
    pub fn is_dongle(&self) -> bool {
        self.transport.device_info().is_dongle()
    }

    // === Device Info ===

    /// Get device ID (unique identifier)
    pub fn get_device_id(&self) -> Result<u32, KeyboardError> {
        let resp = self
            .transport
            .query_command(cmd::GET_USB_VERSION, &[], ChecksumType::Bit7)?;

        if resp.len() < 5 || resp[0] != cmd::GET_USB_VERSION {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid device ID response".into(),
            ));
        }

        let device_id = u32::from_le_bytes([resp[1], resp[2], resp[3], resp[4]]);
        Ok(device_id)
    }

    /// Get firmware version
    pub fn get_version(&self) -> Result<FirmwareVersion, KeyboardError> {
        // Use GET_USB_VERSION which returns device_id and version
        let resp = self
            .transport
            .query_command(cmd::GET_USB_VERSION, &[], ChecksumType::Bit7)?;

        if resp.len() < 9 || resp[0] != cmd::GET_USB_VERSION {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid version response".into(),
            ));
        }

        // GET_USB_VERSION response (after report ID stripped):
        // [0] = cmd echo, [1..5] = device_id, [7..9] = version
        let raw = u16::from_le_bytes([resp[7], resp[8]]);
        Ok(FirmwareVersion::new(raw))
    }

    /// Get battery info (dongle/wireless only)
    ///
    /// For dongle connections, this sends F7 to refresh and reads the cached
    /// value from feature report 0x05. For wired connections, returns full battery.
    pub fn get_battery(&self) -> Result<BatteryInfo, KeyboardError> {
        let (level, online, idle) = self.transport.get_battery_status()?;
        Ok(BatteryInfo {
            level,
            online,
            charging: false, // Not available via dongle protocol
            idle,
        })
    }

    // === LED Control ===

    /// Get current LED parameters
    pub fn get_led_params(&self) -> Result<LedParams, KeyboardError> {
        let resp: TransportLedParamsResponse = self.transport.query(&QueryLedParams::default())?;
        Ok(LedParams::from_transport_response(&resp))
    }

    /// Set LED mode
    pub fn set_led_mode(&self, mode: LedMode) -> Result<(), KeyboardError> {
        let mut params = self.get_led_params()?;
        params.mode = mode;
        self.set_led_params(&params)
    }

    /// Set LED parameters
    pub fn set_led_params(&self, params: &LedParams) -> Result<(), KeyboardError> {
        self.transport.send(&params.to_transport_cmd())?;
        Ok(())
    }

    // === Settings ===

    /// Get current profile (0-3)
    pub fn get_profile(&self) -> Result<u8, KeyboardError> {
        let resp: ProfileResponse = self.transport.query(&QueryProfile::default())?;
        Ok(resp.profile)
    }

    /// Set current profile (0-3)
    pub fn set_profile(&self, profile: u8) -> Result<(), KeyboardError> {
        if profile > 3 {
            return Err(KeyboardError::InvalidParameter(
                "Profile must be 0-3".into(),
            ));
        }
        self.transport.send(&SetProfile::new(profile))?;
        Ok(())
    }

    /// Get polling rate
    pub fn get_polling_rate(&self) -> Result<PollingRate, KeyboardError> {
        let resp: PollingRateResponse = self.transport.query(&QueryPollingRate::default())?;
        // Convert transport PollingRate to keyboard PollingRate
        PollingRate::from_hz(resp.rate.to_hz())
            .ok_or_else(|| KeyboardError::UnexpectedResponse("Unknown polling rate".into()))
    }

    /// Set polling rate
    pub fn set_polling_rate(&self, rate: PollingRate) -> Result<(), KeyboardError> {
        // Convert keyboard PollingRate to transport PollingRate
        let transport_rate = TransportPollingRate::from_hz(rate as u16)
            .ok_or_else(|| KeyboardError::InvalidParameter("Invalid polling rate".into()))?;
        self.transport.send(&SetPollingRate::new(transport_rate))?;
        Ok(())
    }

    // === Debounce ===

    /// Get debounce time in milliseconds
    pub fn get_debounce(&self) -> Result<u8, KeyboardError> {
        let resp: DebounceResponse = self.transport.query(&QueryDebounce::default())?;
        Ok(resp.ms)
    }

    /// Set debounce time in milliseconds (0-50)
    pub fn set_debounce(&self, ms: u8) -> Result<(), KeyboardError> {
        if ms > 50 {
            return Err(KeyboardError::InvalidParameter(
                "Debounce must be 0-50ms".into(),
            ));
        }
        self.transport.send(&SetDebounce::new(ms))?;
        Ok(())
    }

    // === Sleep ===

    /// Get sleep time settings for all wireless modes
    ///
    /// Returns idle and deep sleep timeouts for both Bluetooth and 2.4GHz.
    /// All values are in seconds.
    pub fn get_sleep_time(&self) -> Result<SleepTimeSettings, KeyboardError> {
        let resp: SleepTimeResponse = self.transport.query(&QuerySleepTime::default())?;
        Ok(SleepTimeSettings {
            idle_bt: resp.idle_bt,
            idle_24g: resp.idle_24g,
            deep_bt: resp.deep_bt,
            deep_24g: resp.deep_24g,
        })
    }

    /// Set sleep time settings for all wireless modes
    ///
    /// Sets idle and deep sleep timeouts for both Bluetooth and 2.4GHz.
    /// All values are in seconds. Set to 0 to disable a particular timeout.
    pub fn set_sleep_time(&self, settings: &SleepTimeSettings) -> Result<(), KeyboardError> {
        self.transport.send(&SetSleepTime::new(
            settings.idle_bt,
            settings.idle_24g,
            settings.deep_bt,
            settings.deep_24g,
        ))?;
        Ok(())
    }

    // === Keyboard Options ===

    /// Get keyboard options (OS mode, Fn layer, etc.)
    pub fn get_kb_options(&self) -> Result<KeyboardOptions, KeyboardError> {
        let resp = self
            .transport
            .query_command(cmd::GET_KBOPTION, &[], ChecksumType::Bit7)?;

        if resp.len() < 9 || resp[0] != cmd::GET_KBOPTION {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid KB options response".into(),
            ));
        }

        Ok(KeyboardOptions::from_bytes(&resp[1..]))
    }

    /// Set keyboard options
    pub fn set_kb_options(&self, options: &KeyboardOptions) -> Result<(), KeyboardError> {
        self.transport
            .send_command(cmd::SET_KBOPTION, &options.to_bytes(), ChecksumType::Bit7)?;

        Ok(())
    }

    // === Feature List ===

    /// Get device feature list (precision, capabilities)
    pub fn get_feature_list(&self) -> Result<FeatureList, KeyboardError> {
        let resp = self
            .transport
            .query_command(cmd::GET_FEATURE_LIST, &[], ChecksumType::Bit7)?;

        if resp.is_empty() || resp[0] != cmd::GET_FEATURE_LIST {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid feature list response".into(),
            ));
        }

        Ok(FeatureList::from_bytes(&resp[1..]))
    }

    /// Get precision level for travel/trigger settings
    ///
    /// This method tries to get precision from the feature list first.
    /// If the keyboard doesn't support the feature list command (returns invalid response),
    /// it falls back to inferring precision from the firmware version.
    ///
    /// This is the recommended way to get precision - consumers should use this
    /// instead of calling get_feature_list() or get_version() directly for precision.
    pub fn get_precision(&self) -> Result<settings::Precision, KeyboardError> {
        // Try feature list first
        if let Ok(features) = self.get_feature_list() {
            if let Some(precision) = features.precision() {
                return Ok(precision);
            }
        }

        // Fall back to firmware version
        let version = self.get_version()?;
        Ok(version.precision())
    }

    // === Side LED (Sidelight) ===

    /// Get side LED parameters
    pub fn get_side_led_params(&self) -> Result<LedParams, KeyboardError> {
        let resp = self
            .transport
            .query_command(cmd::GET_SLEDPARAM, &[], ChecksumType::Bit7)?;

        if resp.len() < 8 || resp[0] != cmd::GET_SLEDPARAM {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid side LED params response".into(),
            ));
        }

        // Protocol format: [cmd, mode, speed, brightness, option, r, g, b]
        // Note: Side LED speed is NOT inverted (unlike main LED)
        Ok(LedParams {
            mode: LedMode::from_u8(resp[1]).unwrap_or(LedMode::Off),
            speed: resp[2],
            brightness: resp[3],
            color: RgbColor::new(resp[5], resp[6], resp[7]),
            direction: resp.get(4).copied().unwrap_or(0), // Option byte (dazzle info)
        })
    }

    /// Set side LED parameters
    pub fn set_side_led_params(&self, params: &LedParams) -> Result<(), KeyboardError> {
        // Protocol format: [mode, speed, brightness, option, r, g, b]
        // Note: Side LED speed is NOT inverted (unlike main LED)
        let data = [
            params.mode as u8,
            params.speed.min(led::SPEED_MAX),
            params.brightness.min(led::BRIGHTNESS_MAX),
            params.direction, // Option byte (dazzle info)
            params.color.r,
            params.color.g,
            params.color.b,
        ];

        self.transport
            .send_command(cmd::SET_SLEDPARAM, &data, ChecksumType::Bit8)?;

        Ok(())
    }

    // === Per-Key RGB ===

    /// Set all keys to a single color (for per-key RGB mode)
    pub fn set_all_keys_color(&self, color: RgbColor, layer: u8) -> Result<(), KeyboardError> {
        let colors = vec![(color.r, color.g, color.b); MATRIX_SIZE_M1_V5];
        self.set_per_key_colors_to_layer(&colors, layer)
    }

    // === Userpic (Flash-Based Per-Key Colors, Mode 13) ===

    /// Upload a userpic to a flash slot (0-4).
    ///
    /// `data` must be exactly 288 bytes in column-major format:
    /// pixel (col, row) at offset `col * 18 + row * 3`.
    /// Padded to 384 bytes with zeros for the flash slot.
    ///
    /// Uses the SET_USERPIC (0x0C) bulk protocol: 7 pages of 56/42 bytes.
    pub fn upload_userpic(&self, slot: u8, data: &[u8]) -> Result<(), KeyboardError> {
        if slot > 4 {
            return Err(KeyboardError::InvalidParameter(
                "Userpic slot must be 0-4".into(),
            ));
        }

        // Pad data to full slot size (384 bytes)
        let mut slot_data = vec![0u8; 384];
        let len = data.len().min(384);
        slot_data[..len].copy_from_slice(&data[..len]);

        // Send 7 pages: pages 0-5 have 56 bytes, page 6 has 42 bytes
        // Total: 6*56 + 42 = 378 bytes (covers 384 with some overlap handled by firmware)
        const PAGE_SIZE: usize = 56;
        const LAST_PAGE_SIZE: usize = 42;
        const NUM_PAGES: usize = 7;

        for page in 0..NUM_PAGES {
            let data_size = if page == NUM_PAGES - 1 {
                LAST_PAGE_SIZE
            } else {
                PAGE_SIZE
            };
            let is_last = page == NUM_PAGES - 1;

            let start = page * PAGE_SIZE;
            let end = (start + data_size).min(slot_data.len());

            // Build payload: [slot, 0xFF, page, data_size, last_flag, 0, 0, ...rgb_data...]
            let mut payload = vec![0u8; 7 + data_size];
            payload[0] = slot;
            payload[1] = 0xFF;
            payload[2] = page as u8;
            payload[3] = data_size as u8;
            payload[4] = if is_last { 1 } else { 0 };
            // payload[5] = 0; payload[6] = 0; // already zero
            if end > start {
                let chunk_len = end - start;
                payload[7..7 + chunk_len].copy_from_slice(&slot_data[start..end]);
            }

            self.transport
                .send_command(cmd::SET_USERPIC, &payload, ChecksumType::Bit7)?;

            // Small delay between pages
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        Ok(())
    }

    /// Download a userpic from a flash slot (0-4).
    ///
    /// Returns 384 bytes in column-major format (6 blocks × 64 bytes).
    /// Uses GET_USERPIC (0x8C) block read protocol.
    pub fn download_userpic(&self, slot: u8) -> Result<Vec<u8>, KeyboardError> {
        if slot > 4 {
            return Err(KeyboardError::InvalidParameter(
                "Userpic slot must be 0-4".into(),
            ));
        }

        let mut data = Vec::with_capacity(384);

        // Read 6 blocks of 64 bytes each
        for block in 0..6u8 {
            let query = [slot, 0xFF, block];
            let resp = self
                .transport
                .query_raw(cmd::GET_USERPIC, &query, ChecksumType::Bit7)?;
            data.extend_from_slice(&resp);
        }

        // Truncate to slot size
        data.truncate(384);
        Ok(data)
    }

    // === Magnetism / Hall Effect ===

    /// Start magnetism (key depth) reporting
    pub fn start_magnetism_report(&self) -> Result<(), KeyboardError> {
        if !self.has_magnetism {
            return Err(KeyboardError::NotSupported(
                "Device does not have Hall Effect switches".into(),
            ));
        }
        self.transport.send(&SetMagnetismReport::enable())?;
        Ok(())
    }

    /// Stop magnetism (key depth) reporting
    pub fn stop_magnetism_report(&self) -> Result<(), KeyboardError> {
        if !self.has_magnetism {
            return Ok(());
        }
        self.transport.send(&SetMagnetismReport::disable())?;
        Ok(())
    }

    /// Read a key depth event
    ///
    /// Returns None on timeout
    pub fn read_key_depth(
        &self,
        timeout_ms: u32,
        precision_factor: f64,
    ) -> Result<Option<KeyDepthEvent>, KeyboardError> {
        match self.transport.read_event(timeout_ms)? {
            Some(VendorEvent::KeyDepth {
                key_index,
                depth_raw,
            }) => Ok(Some(KeyDepthEvent {
                key_index,
                depth_raw,
                depth_mm: depth_raw as f32 / precision_factor as f32,
            })),
            _ => Ok(None),
        }
    }

    /// Poll for vendor notifications (non-blocking with timeout)
    ///
    /// Returns any EP2 vendor event from the keyboard, including:
    /// - Profile changes (Fn+F9..F12)
    /// - LED settings (brightness, effect, speed, color)
    /// - Keyboard functions (Win lock, WASD swap)
    /// - Key depth events (during magnetism monitoring)
    /// - Settings acknowledgments
    /// - Wake events
    ///
    /// This is useful for real-time TUI updates when the user changes
    /// settings via the keyboard's Fn key combinations.
    ///
    /// Returns None on timeout (no event within timeout_ms)
    pub fn poll_notification(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, KeyboardError> {
        self.transport
            .read_event(timeout_ms)
            .map_err(KeyboardError::Transport)
    }

    /// Get trigger settings for a specific key
    pub fn get_key_trigger(&self, key_index: u8) -> Result<KeyTriggerSettings, KeyboardError> {
        if !self.has_magnetism {
            return Err(KeyboardError::NotSupported(
                "Device does not have Hall Effect switches".into(),
            ));
        }

        let resp = self.transport.query_command(
            cmd::GET_KEY_MAGNETISM_MODE,
            &[key_index],
            ChecksumType::Bit7,
        )?;

        if resp.len() < 5 || resp[0] != cmd::GET_KEY_MAGNETISM_MODE {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid trigger response".into(),
            ));
        }

        Ok(KeyTriggerSettings {
            key_index,
            actuation: resp[1],
            deactuation: resp[2],
            mode: KeyMode::from_u8(resp[3]),
        })
    }

    /// Set trigger settings for a specific key
    pub fn set_key_trigger(&self, settings: &KeyTriggerSettings) -> Result<(), KeyboardError> {
        if !self.has_magnetism {
            return Err(KeyboardError::NotSupported(
                "Device does not have Hall Effect switches".into(),
            ));
        }

        self.transport.send(&SetKeyMagnetismModeData {
            key_index: settings.key_index,
            actuation: settings.actuation,
            deactuation: settings.deactuation,
            mode: settings.mode.to_u8(),
        })?;

        Ok(())
    }

    /// Query magnetism data for a specific sub-command
    ///
    /// Magnetism queries use a multi-page protocol:
    /// - Send: [sub_cmd, flag=1, page]
    /// - Response doesn't echo command, data starts at byte 0
    fn get_magnetism(&self, sub_cmd: u8, num_pages: usize) -> Result<Vec<u8>, KeyboardError> {
        let mut all_data = Vec::new();

        for page in 0..num_pages {
            let query = GetMultiMagnetismData {
                sub_cmd,
                flag: 1,
                page: page as u8,
            };
            match self.transport.query_raw(
                cmd::GET_MULTI_MAGNETISM,
                query.as_bytes(),
                ChecksumType::Bit7,
            ) {
                Ok(resp) => {
                    all_data.extend_from_slice(&resp);
                }
                Err(_) => {
                    all_data.extend(std::iter::repeat_n(0u8, 64));
                }
            }
        }

        Ok(all_data)
    }

    /// Get all trigger settings
    pub fn get_all_triggers(&self) -> Result<TriggerSettings, KeyboardError> {
        if !self.has_magnetism {
            return Err(KeyboardError::NotSupported(
                "Device does not have Hall Effect switches".into(),
            ));
        }

        // Calculate pages needed based on key count (64 bytes per page)
        let pages_u8 = (self.key_count as usize).div_ceil(64); // 1 byte per key
        let pages_u16 = (self.key_count as usize * 2).div_ceil(64); // 2 bytes per key

        // Key modes use 1 byte per key
        let modes = self.get_magnetism(mag_cmd::KEY_MODE, pages_u8)?;

        // Travel values use 2 bytes per key (16-bit little-endian)
        let press = self.get_magnetism(mag_cmd::PRESS_TRAVEL, pages_u16)?;
        let lift = self.get_magnetism(mag_cmd::LIFT_TRAVEL, pages_u16)?;
        let rt_press = self.get_magnetism(mag_cmd::RT_PRESS, pages_u16)?;
        let rt_lift = self.get_magnetism(mag_cmd::RT_LIFT, pages_u16)?;

        // Deadzones - may fail on older firmware
        let bottom_dz = self
            .get_magnetism(mag_cmd::BOTTOM_DEADZONE, pages_u16)
            .unwrap_or_default();
        let top_dz = self
            .get_magnetism(mag_cmd::TOP_DEADZONE, pages_u16)
            .unwrap_or_default();

        Ok(TriggerSettings {
            key_count: self.key_count as usize,
            press_travel: press,
            lift_travel: lift,
            rt_press,
            rt_lift,
            key_modes: modes,
            bottom_deadzone: bottom_dz,
            top_deadzone: top_dz,
        })
    }

    // === Bulk Trigger Setters ===

    /// Set magnetism values for all keys (u16 version, used by newer firmware)
    ///
    /// Sends values in pages of 56 bytes each.
    /// Format: [sub_cmd, flag=1, page, commit, 0, 0, 0, data...]
    fn set_magnetism_u16(&self, sub_cmd: u8, values: &[u16]) -> Result<(), KeyboardError> {
        // Convert u16 values to bytes (little-endian)
        let bytes: Vec<u8> = values
            .iter()
            .take(self.key_count as usize)
            .flat_map(|&v| v.to_le_bytes())
            .collect();

        // Send in pages (56 bytes per page)
        const PAGE_SIZE: usize = 56;
        let num_pages = bytes.len().div_ceil(PAGE_SIZE);

        for (page, chunk) in bytes.chunks(PAGE_SIZE).enumerate() {
            let is_last = page == num_pages - 1;
            let cmd = SetMultiMagnetismCommand {
                header: SetMultiMagnetismHeader {
                    sub_cmd,
                    flag: 1,
                    page: page as u8,
                    commit: if is_last { 1 } else { 0 },
                    _pad0: 0,
                    _pad1: 0,
                    _checksum: 0,
                },
                payload: chunk.to_vec(),
            };

            self.transport.send_with_delay(&cmd, 30)?;
        }

        Ok(())
    }

    /// Set magnetism values for all keys (u8 version, legacy)
    fn set_magnetism_u8(&self, sub_cmd: u8, values: &[u8]) -> Result<(), KeyboardError> {
        let mut data = vec![sub_cmd];
        data.extend_from_slice(&values[..self.key_count as usize]);
        self.transport
            .send_command(cmd::SET_MULTI_MAGNETISM, &data, ChecksumType::Bit7)?;
        Ok(())
    }

    /// Set actuation point for all keys (u16 raw value)
    ///
    /// Value is in precision units (e.g., 200 = 2.0mm at 0.01mm precision)
    pub fn set_actuation_all_u16(&self, travel: u16) -> Result<(), KeyboardError> {
        let values = vec![travel; self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::PRESS_TRAVEL, &values)
    }

    /// Set release point for all keys (u16 raw value)
    pub fn set_release_all_u16(&self, travel: u16) -> Result<(), KeyboardError> {
        let values = vec![travel; self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::LIFT_TRAVEL, &values)
    }

    /// Set Rapid Trigger press sensitivity for all keys (u16 raw value)
    pub fn set_rt_press_all_u16(&self, sensitivity: u16) -> Result<(), KeyboardError> {
        let values = vec![sensitivity; self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::RT_PRESS, &values)
    }

    /// Set Rapid Trigger release sensitivity for all keys (u16 raw value)
    pub fn set_rt_lift_all_u16(&self, sensitivity: u16) -> Result<(), KeyboardError> {
        let values = vec![sensitivity; self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::RT_LIFT, &values)
    }

    /// Enable/disable Rapid Trigger for all keys
    pub fn set_rapid_trigger_all(&self, enable: bool) -> Result<(), KeyboardError> {
        // Mode values: 0=Normal, 1=RapidTrigger
        let mode = if enable { 1u8 } else { 0u8 };
        let values = vec![mode; self.key_count as usize];
        self.set_magnetism_u8(mag_cmd::KEY_MODE, &values)
    }

    /// Set bottom deadzone for all keys (u16 raw value)
    ///
    /// Bottom deadzone is the distance from bottom of travel that is ignored.
    pub fn set_bottom_deadzone_all_u16(&self, travel: u16) -> Result<(), KeyboardError> {
        let values = vec![travel; self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::BOTTOM_DEADZONE, &values)
    }

    /// Set top deadzone for all keys (u16 raw value)
    ///
    /// Top deadzone is the distance from top of travel that is ignored.
    pub fn set_top_deadzone_all_u16(&self, travel: u16) -> Result<(), KeyboardError> {
        let values = vec![travel; self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::TOP_DEADZONE, &values)
    }

    // === Extended LED Control ===

    /// Set LED mode with full parameters
    ///
    /// # Arguments
    /// * `mode` - LED mode (0-22)
    /// * `brightness` - Brightness level (0-4)
    /// * `speed` - Animation speed (0-4)
    /// * `r`, `g`, `b` - RGB color values
    /// * `dazzle` - Enable rainbow color cycling
    #[allow(clippy::too_many_arguments)]
    pub fn set_led(
        &self,
        mode: u8,
        brightness: u8,
        speed: u8,
        r: u8,
        g: u8,
        b: u8,
        dazzle: bool,
    ) -> Result<(), KeyboardError> {
        self.set_led_with_option(mode, brightness, speed, r, g, b, dazzle, 0)
    }

    /// Set LED mode with layer option (for UserPicture mode)
    ///
    /// For mode 13 (UserPicture):
    /// - `layer`: which custom color layer to display (0-3)
    /// - RGB values are ignored, using (0, 200, 200) per protocol
    #[allow(clippy::too_many_arguments)]
    pub fn set_led_with_option(
        &self,
        mode: u8,
        brightness: u8,
        speed: u8,
        r: u8,
        g: u8,
        b: u8,
        dazzle: bool,
        layer: u8,
    ) -> Result<(), KeyboardError> {
        let (option, r_val, g_val, b_val) = if mode == 13 {
            // For UserPicture mode: option = layer << 4, RGB = (0, 200, 200)
            (layer << 4, 0u8, 200u8, 200u8)
        } else {
            let opt = if dazzle {
                led::DAZZLE_ON
            } else {
                led::DAZZLE_OFF
            };
            (opt, r, g, b)
        };

        let data = [
            mode,
            led::SPEED_MAX - speed.min(led::SPEED_MAX), // Speed is inverted in protocol
            brightness.min(led::BRIGHTNESS_MAX),
            option,
            r_val,
            g_val,
            b_val,
        ];

        self.transport
            .send_command(cmd::SET_LEDPARAM, &data, ChecksumType::Bit8)?;

        Ok(())
    }

    /// Stream per-key colors for real-time effects
    ///
    /// # Arguments
    /// * `colors` - Tuple of (r, g, b) for each key (126 keys)
    /// * `repeat` - Number of times to send (for reliability)
    /// * `layer` - Which layer to update (0-3)
    pub fn set_per_key_colors_fast(
        &self,
        colors: &[(u8, u8, u8)],
        repeat: u8,
        layer: u8,
    ) -> Result<(), KeyboardError> {
        const CHUNK_SIZE: usize = 18; // 18 keys per chunk (54 bytes RGB)

        // Pad colors to full matrix size
        let mut full_colors = vec![(0u8, 0u8, 0u8); MATRIX_SIZE_M1_V5];
        let len = colors.len().min(MATRIX_SIZE_M1_V5);
        full_colors[..len].copy_from_slice(&colors[..len]);

        for _ in 0..repeat.max(1) {
            for (chunk_idx, chunk) in full_colors.chunks(CHUNK_SIZE).enumerate() {
                let mut data = vec![0u8; 56]; // layer + page + 54 RGB bytes
                data[0] = layer;
                data[1] = chunk_idx as u8;
                for (i, &(r, g, b)) in chunk.iter().enumerate() {
                    data[2 + i * 3] = r;
                    data[2 + i * 3 + 1] = g;
                    data[2 + i * 3 + 2] = b;
                }

                self.transport.send_command_with_delay(
                    cmd::SET_USERPIC,
                    &data,
                    ChecksumType::Bit8,
                    5,
                )?;
            }
        }

        Ok(())
    }

    /// Store per-key colors to a specific layer
    pub fn set_per_key_colors_to_layer(
        &self,
        colors: &[(u8, u8, u8)],
        layer: u8,
    ) -> Result<(), KeyboardError> {
        self.set_per_key_colors_fast(colors, 1, layer)
    }

    // === Calibration ===

    /// Start/stop minimum position calibration (keys released)
    pub fn calibrate_min(&self, start: bool) -> Result<(), KeyboardError> {
        self.transport.send_command(
            cmd::SET_MAGNETISM_CAL,
            &[if start { 1 } else { 0 }],
            ChecksumType::Bit7,
        )?;
        Ok(())
    }

    /// Start/stop maximum position calibration (keys pressed)
    pub fn calibrate_max(&self, start: bool) -> Result<(), KeyboardError> {
        self.transport.send_command(
            cmd::SET_MAGNETISM_MAX_CAL,
            &[if start { 1 } else { 0 }],
            ChecksumType::Bit7,
        )?;
        Ok(())
    }

    /// Get calibration progress for a page of keys (32 keys per page)
    ///
    /// During max calibration, polls the keyboard for per-key calibration values.
    /// Values >= 300 indicate the key has been calibrated (pressed to bottom).
    ///
    /// # Arguments
    /// * `page` - Page number (0-3, each page has 32 keys)
    ///
    /// # Returns
    /// Vector of 16-bit calibration values for up to 32 keys
    pub fn get_calibration_progress(&self, page: u8) -> Result<Vec<u16>, KeyboardError> {
        let query = GetMultiMagnetismData {
            sub_cmd: mag_cmd::CALIBRATION,
            flag: 1,
            page,
        };
        let response = self.transport.query_raw(
            cmd::GET_MULTI_MAGNETISM,
            query.as_bytes(),
            ChecksumType::Bit7,
        )?;

        // Decode 16-bit LE values from response (64 bytes = 32 values)
        let mut values = Vec::with_capacity(32);
        for chunk in response.chunks(2) {
            if chunk.len() == 2 {
                values.push(u16::from_le_bytes([chunk[0], chunk[1]]));
            }
        }
        Ok(values)
    }

    // === Factory Reset ===

    /// Factory reset the keyboard
    pub fn reset(&self) -> Result<(), KeyboardError> {
        self.transport
            .send_command(cmd::SET_RESET, &[], ChecksumType::Bit7)?;
        Ok(())
    }

    // === Raw Commands (for CLI compatibility) ===

    /// Send a raw command and get response
    pub fn query_raw_cmd(&self, cmd_byte: u8) -> Result<Vec<u8>, KeyboardError> {
        let resp = self
            .transport
            .query_command(cmd_byte, &[], ChecksumType::Bit7)?;
        Ok(resp)
    }

    /// Send raw command with data
    pub fn query_raw_cmd_data(&self, cmd_byte: u8, data: &[u8]) -> Result<Vec<u8>, KeyboardError> {
        let resp = self
            .transport
            .query_command(cmd_byte, data, ChecksumType::Bit7)?;
        Ok(resp)
    }

    /// Send raw command without expecting response
    pub fn send_raw_cmd(&self, cmd_byte: u8, data: &[u8]) -> Result<(), KeyboardError> {
        self.transport
            .send_command(cmd_byte, data, ChecksumType::Bit7)?;
        Ok(())
    }

    // === Key Matrix (Key Remapping) ===

    /// Get key matrix (key remappings) for a profile
    ///
    /// # Arguments
    /// * `profile` - Profile index (0-3)
    /// * `num_pages` - Number of pages to read (8 for full 126-key matrix)
    ///
    /// # Returns
    /// Raw key matrix data (4 bytes per key: type, enabled, layer, keycode)
    pub fn get_keymatrix(&self, profile: u8, num_pages: usize) -> Result<Vec<u8>, KeyboardError> {
        let mut all_data = Vec::new();

        for page in 0..num_pages {
            let query = GetKeyMatrixData {
                profile,
                magic: 0xFF,
                page: page as u8,
                magnetism_profile: 0,
            };

            match self
                .transport
                .query_raw(cmd::GET_KEYMATRIX, query.as_bytes(), ChecksumType::Bit7)
            {
                Ok(resp) => {
                    all_data.extend_from_slice(&resp);
                }
                Err(_) => continue,
            }
        }

        if all_data.is_empty() {
            Err(KeyboardError::UnexpectedResponse(
                "No keymatrix data".into(),
            ))
        } else {
            Ok(all_data)
        }
    }

    /// Read the Fn layer key matrix using GET_FN (0x90).
    ///
    /// Unlike `get_keymatrix` which reads base/Fn remaps via GET_KEYMATRIX (0x8A),
    /// this reads the actual Fn layer bindings (media keys, LED controls, etc.)
    /// via the dedicated GET_FN command.
    ///
    /// # Arguments
    /// * `profile` - Profile index (0-3)
    /// * `sys` - OS mode: 0=Windows, 1=Mac
    /// * `num_pages` - Number of pages to read (8 for full matrix)
    pub fn get_fn_keymatrix(
        &self,
        profile: u8,
        sys: u8,
        num_pages: usize,
    ) -> Result<Vec<u8>, KeyboardError> {
        let mut all_data = Vec::new();

        for page in 0..num_pages {
            let query = GetFnData {
                sys,
                profile,
                magic: 0xFF,
                page: page as u8,
            };
            match self
                .transport
                .query_raw(cmd::GET_FN, query.as_bytes(), ChecksumType::Bit7)
            {
                Ok(resp) => {
                    all_data.extend_from_slice(&resp);
                }
                Err(_) => continue,
            }
        }

        if all_data.is_empty() {
            Err(KeyboardError::UnexpectedResponse(
                "GET_FN returned no data".into(),
            ))
        } else {
            Ok(all_data)
        }
    }

    /// Set a key's 4-byte config on any layer.
    ///
    /// Routes to SET_KEYMATRIX (0x0A) for layers 0-1 or SET_FN (0x10) for layer 2+.
    /// The `config` bytes are `[config_type, b1, b2, b3]` as used by the protocol.
    pub fn set_key_config(
        &self,
        profile: u8,
        key_index: u8,
        layer: u8,
        config: [u8; 4],
    ) -> Result<(), KeyboardError> {
        let enabled = config != [0, 0, 0, 0];
        if layer <= 1 {
            self.transport.send(&SetKeyMatrixData::new(
                profile, key_index, layer, enabled, config,
            )?)?;
        } else {
            self.transport
                .send(&SetFnData::new(0, profile, key_index, config)?)?;
        }
        Ok(())
    }

    /// Set a single key's mapping (base layer only).
    ///
    /// For layer-aware remapping, use [`set_key_config`](Self::set_key_config).
    pub fn set_keymatrix(
        &self,
        profile: u8,
        key_index: u8,
        hid_code: u8,
        enabled: bool,
        layer: u8,
    ) -> Result<(), KeyboardError> {
        self.transport.send(&SetKeyMatrixData::new(
            profile,
            key_index,
            layer,
            enabled,
            [0, 0, hid_code, 0],
        )?)?;
        Ok(())
    }

    /// Reset a key to its default mapping on any layer.
    ///
    /// Sets the key to "disabled" which causes the firmware to use the default.
    pub fn reset_key(&self, layer: u8, key_index: u8) -> Result<(), KeyboardError> {
        self.set_key_config(0, key_index, layer, [0, 0, 0, 0])
    }

    /// Swap two keys
    pub fn swap_keys(
        &self,
        profile: u8,
        key_a: u8,
        code_a: u8,
        key_b: u8,
        code_b: u8,
    ) -> Result<(), KeyboardError> {
        // Set key_a to code_b
        self.set_keymatrix(profile, key_a, code_b, true, 0)?;
        // Set key_b to code_a
        self.set_keymatrix(profile, key_b, code_a, true, 0)
    }

    // === Macros ===

    /// Get macro data for a macro slot
    ///
    /// # Arguments
    /// * `macro_index` - Macro slot number (0-based)
    ///
    /// # Returns
    /// Raw macro data: [2-byte repeat count (LE), then 2-byte events (keycode, flags)]
    pub fn get_macro(&self, macro_index: u8) -> Result<Vec<u8>, KeyboardError> {
        let mut all_data = Vec::new();

        for page in 0..4u8 {
            let query = GetMacroData { macro_index, page };

            match self
                .transport
                .query_raw(cmd::GET_MACRO, query.as_bytes(), ChecksumType::Bit7)
            {
                Ok(resp) => {
                    // Skip command echo if present (some transports may add it)
                    let start = if !resp.is_empty() && resp[0] == cmd::GET_MACRO {
                        1
                    } else {
                        0
                    };
                    if resp.len() > start {
                        all_data.extend_from_slice(&resp[start..]);
                    }

                    // Check for 4 consecutive zeros (end marker)
                    if resp[start..].windows(4).any(|w| w == [0, 0, 0, 0]) {
                        break;
                    }
                }
                Err(_) => continue,
            }
        }

        if all_data.is_empty() {
            Err(KeyboardError::UnexpectedResponse("No macro data".into()))
        } else if all_data.iter().all(|&b| b == 0xFF) {
            // Uninitialized slot — treat as empty
            Ok(vec![0, 0]) // repeat_count=0, no events
        } else {
            Ok(all_data)
        }
    }

    /// Set macro data for a macro slot
    ///
    /// # Arguments
    /// * `macro_index` - Macro slot number (0-based)
    /// * `events` - List of (keycode, is_down, delay_ms) tuples with u16 delay
    /// * `repeat_count` - How many times to repeat the macro
    ///
    /// Events use variable-length encoding:
    /// - Short delay (0-127ms): 2 bytes `[keycode, direction_bit | delay]`
    /// - Long delay (128+ms): 4 bytes `[keycode, direction_bit, delay_lo, delay_hi]`
    pub fn set_macro(
        &self,
        macro_index: u8,
        events: &[(u8, bool, u16)],
        repeat_count: u16,
    ) -> Result<(), KeyboardError> {
        // Build macro data
        let mut macro_data = Vec::with_capacity(256);

        // 2-byte repeat count (little-endian)
        macro_data.push((repeat_count & 0xFF) as u8);
        macro_data.push((repeat_count >> 8) as u8);

        // Add events with variable-length encoding
        // Short format (1-127ms): 2 bytes [keycode, direction_bit | delay]
        // Long format (0ms or 128+ms): 4 bytes [keycode, direction_bit, delay_lo, delay_hi]
        // Note: 0ms uses long format to avoid ambiguity with the parser
        // (the parser treats low-7-bits==0 as long format indicator)
        for &(keycode, is_down, delay) in events {
            macro_data.push(keycode);
            if (1..=127).contains(&delay) {
                // Short format
                let flags = if is_down {
                    0x80 | (delay as u8)
                } else {
                    delay as u8
                };
                macro_data.push(flags);
            } else {
                // Long format (0ms or 128+ms)
                let flags = if is_down { 0x80 } else { 0x00 };
                macro_data.push(flags);
                macro_data.push((delay & 0xFF) as u8);
                macro_data.push((delay >> 8) as u8);
            }
        }

        // Pad to at least fill first page
        while macro_data.len() < 56 {
            macro_data.push(0);
        }

        // Send in pages of 56 bytes
        const PAGE_SIZE: usize = 56;
        let num_pages = macro_data.len().div_ceil(PAGE_SIZE);

        for page in 0..num_pages {
            let start = page * PAGE_SIZE;
            let end = (start + PAGE_SIZE).min(macro_data.len());
            let chunk = &macro_data[start..end];
            let is_last = page == num_pages - 1;

            let cmd = SetMacroCommand::new(macro_index, page as u8, is_last, chunk.to_vec())?;

            self.transport.send_with_delay(&cmd, 30)?;
        }

        Ok(())
    }

    /// Set a text macro (convenience method)
    ///
    /// # Arguments
    /// * `macro_index` - Macro slot number (0-based)
    /// * `text` - Text to type
    /// * `delay_ms` - Delay between keystrokes in ms
    /// * `repeat` - How many times to repeat
    pub fn set_text_macro(
        &self,
        macro_index: u8,
        text: &str,
        delay_ms: u16,
        repeat: u16,
    ) -> Result<(), KeyboardError> {
        use crate::hid_codes::char_to_hid;

        const LSHIFT: u8 = 0xE1; // Left Shift HID code
        let mut events = Vec::new();

        for ch in text.chars() {
            if let Some((keycode, needs_shift)) = char_to_hid(ch) {
                if needs_shift {
                    events.push((LSHIFT, true, 0u16)); // Shift down
                    events.push((keycode, true, delay_ms)); // Key down
                    events.push((keycode, false, 0u16)); // Key up
                    events.push((LSHIFT, false, delay_ms)); // Shift up
                } else {
                    events.push((keycode, true, delay_ms)); // Key down
                    events.push((keycode, false, delay_ms)); // Key up
                }
            }
        }

        self.set_macro(macro_index, &events, repeat)
    }

    /// Assign a macro to a key on any layer.
    ///
    /// * `layer` - 0 for base, 1 for Fn
    /// * `macro_type` - 0=repeat by count, 1=toggle, 2=hold to repeat
    pub fn assign_macro_to_key(
        &self,
        layer: u8,
        key_index: u8,
        macro_index: u8,
        macro_type: u8,
    ) -> Result<(), KeyboardError> {
        self.set_key_config(0, key_index, layer, [9, macro_type, macro_index, 0])
    }

    /// Remove macro assignment from a key, restoring default behavior.
    pub fn unassign_macro_from_key(&self, layer: u8, key_index: u8) -> Result<(), KeyboardError> {
        self.reset_key(layer, key_index)
    }

    // === Device Info ===

    /// Get device VID
    pub fn vid(&self) -> u16 {
        self.transport.device_info().vid
    }

    /// Get device PID
    pub fn pid(&self) -> u16 {
        self.transport.device_info().pid
    }

    /// Get device name
    pub fn device_name(&self) -> String {
        self.transport
            .device_info()
            .product_name
            .clone()
            .unwrap_or_else(|| format!("{:04X}:{:04X}", self.vid(), self.pid()))
    }

    // === Connection ===

    /// Check if the keyboard is still connected
    pub fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    /// Close the connection
    pub fn close(&self) -> Result<(), KeyboardError> {
        self.transport.close()?;
        Ok(())
    }

    // === Patch Features ===

    /// Stream a page of per-key RGB data to the LED frame buffer (patched firmware)
    ///
    /// Writes 18 keys of RGB data directly to the WS2812 frame buffer without
    /// touching flash. Call `stream_led_commit()` after sending all pages to
    /// update the LEDs.
    ///
    /// Uses zero delay — the firmware handles 0xE8 instantly (memcpy to frame
    /// buffer), so the default 100ms flow-control delay is unnecessary and would
    /// limit throughput to ~1.4 FPS.
    ///
    /// # Arguments
    /// * `page` - Page index (0-6, each page = 18 keys)
    /// * `rgb_data` - RGB data (up to 54 bytes = 18 keys × 3 bytes)
    pub fn stream_led_page(&self, page: u8, rgb_data: &[u8]) -> Result<(), KeyboardError> {
        // Firmware receives: [0xE8, page, 54B RGB] at cmd_buf+2, page at cmd_buf[3]
        let mut data = vec![0u8; 55]; // page + 54 RGB bytes
        data[0] = page;
        let len = rgb_data.len().min(54);
        data[1..1 + len].copy_from_slice(&rgb_data[..len]);
        self.transport
            .send_command_with_delay(0xE8, &data, ChecksumType::None, 0)?;
        Ok(())
    }

    /// Commit streamed LED data — copies frame buffer to DMA buffer for display
    pub fn stream_led_commit(&self) -> Result<(), KeyboardError> {
        self.transport
            .send_command_with_delay(0xE8, &[0xFF], ChecksumType::None, 0)?;
        Ok(())
    }

    /// Release LED streaming — signals end of streaming session
    pub fn stream_led_release(&self) -> Result<(), KeyboardError> {
        self.transport
            .send_command_with_delay(0xE8, &[0xFE], ChecksumType::None, 0)?;
        Ok(())
    }

    /// Query patch info from modded firmware
    ///
    /// Returns `Some(PatchInfo)` if the keyboard is running patched firmware,
    /// `None` if it's running stock firmware (command 0xE7 not recognized or
    /// response doesn't contain the expected magic bytes).
    pub fn get_patch_info(&self) -> Result<Option<PatchInfo>, KeyboardError> {
        let resp = self.transport.query_raw(0xE7, &[], ChecksumType::Bit7);

        match resp {
            Ok(resp) => {
                // Response layout: resp[0]=cmd echo (0xE7), resp[1..2]=magic,
                // resp[3]=ver, resp[4..5]=caps, resp[6..]=name.
                // (GET_REPORT returns from lp_class_report_buf = cmd_buf+2,
                //  handler writes magic at cmd_buf[3..4], so resp[1..2])
                if resp.len() < 8 || resp[1] != 0xCA || resp[2] != 0xFE {
                    return Ok(None);
                }
                let version = resp[3];
                let capabilities = u16::from_le_bytes([resp[4], resp[5]]);
                let name_end = resp.len().min(14);
                let name_bytes = &resp[6..name_end];
                let name_len = name_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(name_bytes.len());
                let name = String::from_utf8_lossy(&name_bytes[..name_len]).to_string();
                Ok(Some(PatchInfo {
                    version,
                    capabilities,
                    name,
                }))
            }
            Err(_) => Ok(None),
        }
    }

    /// Subscribe to timestamped vendor events via broadcast channel
    ///
    /// Returns a receiver for asynchronous vendor event notifications.
    /// Events are pushed from a dedicated reader thread with near-zero latency
    /// when data arrives. Each event includes a timestamp (seconds since transport
    /// was opened) for accurate timing in visualizations.
    ///
    /// Returns None if event subscriptions are not supported (no input endpoint).
    pub fn subscribe_events(&self) -> Option<tokio::sync::broadcast::Receiver<TimestampedEvent>> {
        self.transport.subscribe_events()
    }
}

/// A single parsed macro event
#[derive(Debug, Clone)]
pub struct MacroEvent {
    pub keycode: u8,
    pub is_down: bool,
    pub delay_ms: u16,
}

/// Parse raw macro data into repeat count and structured events.
///
/// Input `data` should be the full macro data (starting with 2-byte LE repeat count).
/// Events use variable-length encoding:
/// - Short delay (0-127ms): 2 bytes `[keycode, direction_bit | delay]`
/// - Long delay (128+ms): 4 bytes `[keycode, direction_bit, delay_lo, delay_hi]`
///
/// Returns `(repeat_count, events)`. Stops on `[0, 0]` end marker or end of data.
pub fn parse_macro_events(data: &[u8]) -> (u16, Vec<MacroEvent>) {
    if data.len() < 2 {
        return (0, Vec::new());
    }

    let repeat_count = u16::from_le_bytes([data[0], data[1]]);
    let mut events = Vec::new();
    let mut pos = 2;

    while pos + 1 < data.len() {
        let keycode = data[pos];
        let flags = data[pos + 1];

        // End marker: [0, 0]
        if keycode == 0 && flags == 0 {
            break;
        }

        let is_down = (flags & 0x80) != 0;
        let delay_low_bits = flags & 0x7F;

        if delay_low_bits == 0 && pos + 3 < data.len() {
            // Long format: direction-only byte followed by 16-bit LE delay
            let delay_ms = u16::from_le_bytes([data[pos + 2], data[pos + 3]]);
            events.push(MacroEvent {
                keycode,
                is_down,
                delay_ms,
            });
            pos += 4;
        } else {
            // Short format: delay encoded in low 7 bits
            events.push(MacroEvent {
                keycode,
                is_down,
                delay_ms: delay_low_bits as u16,
            });
            pos += 2;
        }
    }

    (repeat_count, events)
}
