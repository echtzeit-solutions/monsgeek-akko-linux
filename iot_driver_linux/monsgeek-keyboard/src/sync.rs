//! Synchronous adapter for KeyboardInterface
//!
//! Provides blocking wrappers around the async KeyboardInterface,
//! enabling use in synchronous code (like TUI worker threads).

use std::sync::Arc;

use crate::error::KeyboardError;
use crate::led::{LedParams, RgbColor};
use crate::magnetism::{KeyDepthEvent, KeyTriggerSettings, TriggerSettings};
use crate::settings::{BatteryInfo, FeatureList, FirmwareVersion, KeyboardOptions, PollingRate};
use crate::KeyboardInterface;

use monsgeek_transport::{list_devices_sync, open_device_sync, DiscoveredDevice, Transport};

/// Block on a future using futures crate (works in any context)
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    futures::executor::block_on(f)
}

/// Synchronous wrapper around KeyboardInterface
pub struct SyncKeyboard {
    inner: KeyboardInterface,
}

impl SyncKeyboard {
    /// Create from an async KeyboardInterface
    pub fn new(keyboard: KeyboardInterface) -> Self {
        Self { inner: keyboard }
    }

    /// Create from a transport
    pub fn from_transport(
        transport: Arc<dyn Transport>,
        key_count: u8,
        has_magnetism: bool,
    ) -> Self {
        Self {
            inner: KeyboardInterface::new(transport, key_count, has_magnetism),
        }
    }

    /// Open any supported device (auto-detecting)
    pub fn open_any() -> Result<Self, KeyboardError> {
        let devices = list_devices_sync()?;

        if devices.is_empty() {
            return Err(KeyboardError::NotFound("No supported device found".into()));
        }

        Self::open_device(&devices[0])
    }

    /// Open a specific discovered device
    pub fn open_device(device: &DiscoveredDevice) -> Result<Self, KeyboardError> {
        let transport = open_device_sync(device)?;
        let info = transport.device_info();

        // Look up device info - default to 98 keys with magnetism for M1 V5 HE
        let (key_count, has_magnetism) = match (info.vid, info.pid) {
            (0x3151, 0x5030) => (98, true), // M1 V5 HE wired
            (0x3151, 0x5038) => (98, true), // M1 V5 HE dongle
            _ => (98, true),                // Default
        };

        Ok(Self {
            inner: KeyboardInterface::new(transport.inner().clone(), key_count, has_magnetism),
        })
    }

    /// Get the underlying async interface
    pub fn inner(&self) -> &KeyboardInterface {
        &self.inner
    }

    // === Device Info ===

    /// Get key count
    pub fn key_count(&self) -> u8 {
        self.inner.key_count()
    }

    /// Check if device has magnetism
    pub fn has_magnetism(&self) -> bool {
        self.inner.has_magnetism()
    }

    /// Check if wireless
    pub fn is_wireless(&self) -> bool {
        self.inner.is_wireless()
    }

    /// Get device ID
    pub fn get_device_id(&self) -> Result<u32, KeyboardError> {
        block_on(self.inner.get_device_id())
    }

    /// Get firmware version
    pub fn get_version(&self) -> Result<FirmwareVersion, KeyboardError> {
        block_on(self.inner.get_version())
    }

    /// Get battery info
    pub fn get_battery(&self) -> Result<BatteryInfo, KeyboardError> {
        block_on(self.inner.get_battery())
    }

    // === LED ===

    /// Get LED parameters
    pub fn get_led_params(&self) -> Result<LedParams, KeyboardError> {
        block_on(self.inner.get_led_params())
    }

    /// Set LED parameters
    pub fn set_led_params(&self, params: &LedParams) -> Result<(), KeyboardError> {
        block_on(self.inner.set_led_params(params))
    }

    /// Get side LED parameters
    pub fn get_side_led_params(&self) -> Result<LedParams, KeyboardError> {
        block_on(self.inner.get_side_led_params())
    }

    /// Set side LED parameters
    pub fn set_side_led_params(&self, params: &LedParams) -> Result<(), KeyboardError> {
        block_on(self.inner.set_side_led_params(params))
    }

    /// Set all keys to a single color
    pub fn set_all_keys_color(&self, color: RgbColor, layer: u8) -> Result<(), KeyboardError> {
        block_on(self.inner.set_all_keys_color(color, layer))
    }

    // === Settings ===

    /// Get current profile
    pub fn get_profile(&self) -> Result<u8, KeyboardError> {
        block_on(self.inner.get_profile())
    }

    /// Set current profile
    pub fn set_profile(&self, profile: u8) -> Result<(), KeyboardError> {
        block_on(self.inner.set_profile(profile))
    }

    /// Get polling rate
    pub fn get_polling_rate(&self) -> Result<PollingRate, KeyboardError> {
        block_on(self.inner.get_polling_rate())
    }

    /// Set polling rate
    pub fn set_polling_rate(&self, rate: PollingRate) -> Result<(), KeyboardError> {
        block_on(self.inner.set_polling_rate(rate))
    }

    /// Get debounce time
    pub fn get_debounce(&self) -> Result<u8, KeyboardError> {
        block_on(self.inner.get_debounce())
    }

    /// Set debounce time
    pub fn set_debounce(&self, ms: u8) -> Result<(), KeyboardError> {
        block_on(self.inner.set_debounce(ms))
    }

    /// Get sleep time
    pub fn get_sleep_time(&self) -> Result<u16, KeyboardError> {
        block_on(self.inner.get_sleep_time())
    }

    /// Set sleep time
    pub fn set_sleep_time(&self, seconds: u16) -> Result<(), KeyboardError> {
        block_on(self.inner.set_sleep_time(seconds))
    }

    /// Get keyboard options
    pub fn get_kb_options(&self) -> Result<KeyboardOptions, KeyboardError> {
        block_on(self.inner.get_kb_options())
    }

    /// Set keyboard options
    pub fn set_kb_options(&self, options: &KeyboardOptions) -> Result<(), KeyboardError> {
        block_on(self.inner.set_kb_options(options))
    }

    /// Get feature list
    pub fn get_feature_list(&self) -> Result<FeatureList, KeyboardError> {
        block_on(self.inner.get_feature_list())
    }

    // === Magnetism ===

    /// Start magnetism reporting
    pub fn start_magnetism_report(&self) -> Result<(), KeyboardError> {
        block_on(self.inner.start_magnetism_report())
    }

    /// Stop magnetism reporting
    pub fn stop_magnetism_report(&self) -> Result<(), KeyboardError> {
        block_on(self.inner.stop_magnetism_report())
    }

    /// Read key depth event
    pub fn read_key_depth(
        &self,
        timeout_ms: u32,
        precision_factor: f64,
    ) -> Result<Option<KeyDepthEvent>, KeyboardError> {
        block_on(self.inner.read_key_depth(timeout_ms, precision_factor))
    }

    /// Get trigger settings for a key
    pub fn get_key_trigger(&self, key_index: u8) -> Result<KeyTriggerSettings, KeyboardError> {
        block_on(self.inner.get_key_trigger(key_index))
    }

    /// Set trigger settings for a key
    pub fn set_key_trigger(&self, settings: &KeyTriggerSettings) -> Result<(), KeyboardError> {
        block_on(self.inner.set_key_trigger(settings))
    }

    /// Get all trigger settings
    pub fn get_all_triggers(&self) -> Result<TriggerSettings, KeyboardError> {
        block_on(self.inner.get_all_triggers())
    }

    // === Bulk Trigger Setters ===

    /// Set actuation point for all keys (u16 raw value)
    pub fn set_actuation_all_u16(&self, travel: u16) -> Result<(), KeyboardError> {
        block_on(self.inner.set_actuation_all_u16(travel))
    }

    /// Set release point for all keys (u16 raw value)
    pub fn set_release_all_u16(&self, travel: u16) -> Result<(), KeyboardError> {
        block_on(self.inner.set_release_all_u16(travel))
    }

    /// Set Rapid Trigger press sensitivity for all keys (u16 raw value)
    pub fn set_rt_press_all_u16(&self, sensitivity: u16) -> Result<(), KeyboardError> {
        block_on(self.inner.set_rt_press_all_u16(sensitivity))
    }

    /// Set Rapid Trigger release sensitivity for all keys (u16 raw value)
    pub fn set_rt_lift_all_u16(&self, sensitivity: u16) -> Result<(), KeyboardError> {
        block_on(self.inner.set_rt_lift_all_u16(sensitivity))
    }

    /// Enable/disable Rapid Trigger for all keys
    pub fn set_rapid_trigger_all(&self, enable: bool) -> Result<(), KeyboardError> {
        block_on(self.inner.set_rapid_trigger_all(enable))
    }

    // === Extended LED Control ===

    /// Set LED mode with full parameters
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
        block_on(self.inner.set_led(mode, brightness, speed, r, g, b, dazzle))
    }

    /// Set LED mode with layer option (for UserPicture mode)
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
        block_on(
            self.inner
                .set_led_with_option(mode, brightness, speed, r, g, b, dazzle, layer),
        )
    }

    /// Stream per-key colors for real-time effects
    pub fn set_per_key_colors_fast(
        &self,
        colors: &[(u8, u8, u8)],
        repeat: u8,
        layer: u8,
    ) -> Result<(), KeyboardError> {
        block_on(self.inner.set_per_key_colors_fast(colors, repeat, layer))
    }

    /// Store per-key colors to a layer
    pub fn set_per_key_colors_to_layer(
        &self,
        colors: &[(u8, u8, u8)],
        layer: u8,
    ) -> Result<(), KeyboardError> {
        block_on(self.inner.set_per_key_colors_to_layer(colors, layer))
    }

    // === Animation Upload ===

    /// Initialize per-key RGB/animation mode
    pub fn start_user_gif(&self) -> Result<(), KeyboardError> {
        block_on(self.inner.start_user_gif())
    }

    /// Upload a complete animation to the keyboard
    ///
    /// The keyboard stores up to 255 frames and plays them autonomously.
    pub fn upload_animation(
        &self,
        frames: &[Vec<(u8, u8, u8)>],
        frame_delay_ms: u16,
    ) -> Result<(), KeyboardError> {
        block_on(self.inner.upload_animation(frames, frame_delay_ms))
    }

    // === Calibration ===

    /// Start/stop minimum position calibration
    pub fn calibrate_min(&self, start: bool) -> Result<(), KeyboardError> {
        block_on(self.inner.calibrate_min(start))
    }

    /// Start/stop maximum position calibration
    pub fn calibrate_max(&self, start: bool) -> Result<(), KeyboardError> {
        block_on(self.inner.calibrate_max(start))
    }

    // === Factory Reset ===

    /// Factory reset the keyboard
    pub fn reset(&self) -> Result<(), KeyboardError> {
        block_on(self.inner.reset())
    }

    // === Raw Commands ===

    /// Send a raw command and get response
    pub fn query_raw_cmd(&self, cmd: u8) -> Result<Vec<u8>, KeyboardError> {
        block_on(self.inner.query_raw_cmd(cmd))
    }

    /// Send raw command with data
    pub fn query_raw_cmd_data(&self, cmd: u8, data: &[u8]) -> Result<Vec<u8>, KeyboardError> {
        block_on(self.inner.query_raw_cmd_data(cmd, data))
    }

    /// Send raw command without expecting response
    pub fn send_raw_cmd(&self, cmd: u8, data: &[u8]) -> Result<(), KeyboardError> {
        block_on(self.inner.send_raw_cmd(cmd, data))
    }

    // === Key Matrix (Key Remapping) ===

    /// Get key matrix (key remappings) for a profile
    pub fn get_keymatrix(&self, profile: u8, num_pages: usize) -> Result<Vec<u8>, KeyboardError> {
        block_on(self.inner.get_keymatrix(profile, num_pages))
    }

    /// Set a single key's mapping
    pub fn set_keymatrix(
        &self,
        profile: u8,
        key_index: u8,
        hid_code: u8,
        enabled: bool,
        layer: u8,
    ) -> Result<(), KeyboardError> {
        block_on(
            self.inner
                .set_keymatrix(profile, key_index, hid_code, enabled, layer),
        )
    }

    /// Reset a key to its default mapping
    pub fn reset_key(&self, profile: u8, key_index: u8) -> Result<(), KeyboardError> {
        block_on(self.inner.reset_key(profile, key_index))
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
        block_on(self.inner.swap_keys(profile, key_a, code_a, key_b, code_b))
    }

    // === Macros ===

    /// Get macro data for a macro slot
    pub fn get_macro(&self, macro_index: u8) -> Result<Vec<u8>, KeyboardError> {
        block_on(self.inner.get_macro(macro_index))
    }

    /// Set macro data for a macro slot
    pub fn set_macro(
        &self,
        macro_index: u8,
        events: &[(u8, bool, u8)],
        repeat_count: u16,
    ) -> Result<(), KeyboardError> {
        block_on(self.inner.set_macro(macro_index, events, repeat_count))
    }

    /// Set a text macro (convenience method)
    pub fn set_text_macro(
        &self,
        macro_index: u8,
        text: &str,
        delay_ms: u8,
        repeat: u16,
    ) -> Result<(), KeyboardError> {
        block_on(
            self.inner
                .set_text_macro(macro_index, text, delay_ms, repeat),
        )
    }

    // === Device Info ===

    /// Get device VID
    pub fn vid(&self) -> u16 {
        self.inner.vid()
    }

    /// Get device PID
    pub fn pid(&self) -> u16 {
        self.inner.pid()
    }

    /// Get device name
    pub fn device_name(&self) -> String {
        self.inner.device_name()
    }

    // === Connection ===

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        block_on(self.inner.is_connected())
    }

    /// Close connection
    pub fn close(&self) -> Result<(), KeyboardError> {
        block_on(self.inner.close())
    }
}

/// List all connected devices
pub fn list_keyboards() -> Result<Vec<DiscoveredDevice>, KeyboardError> {
    Ok(list_devices_sync()?)
}
