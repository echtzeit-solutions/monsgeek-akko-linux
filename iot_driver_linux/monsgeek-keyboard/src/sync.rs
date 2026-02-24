//! Synchronous adapter for KeyboardInterface
//!
//! Now that `KeyboardInterface` is natively synchronous, `SyncKeyboard` is
//! a thin convenience wrapper that adds `open_any()`/`open_device()` helpers.

use std::sync::Arc;

use crate::error::KeyboardError;
use crate::led::{LedParams, RgbColor};
use crate::magnetism::{KeyDepthEvent, KeyTriggerSettings, TriggerSettings};
use crate::settings::{
    BatteryInfo, FeatureList, FirmwareVersion, KeyboardOptions, PollingRate, Precision,
    SleepTimeSettings,
};
use crate::{KeyboardInterface, PatchInfo};

use monsgeek_transport::{
    list_devices_sync, open_device_sync, DiscoveredDevice, FlowControlTransport, VendorEvent,
};

/// Macro to generate delegation methods (no more block_on needed)
macro_rules! delegate {
    // No arguments, with return type
    ($name:ident() -> $ret:ty) => {
        pub fn $name(&self) -> $ret {
            self.inner.$name()
        }
    };
    // Single argument by value
    ($name:ident($arg:ident: $ty:ty) -> $ret:ty) => {
        pub fn $name(&self, $arg: $ty) -> $ret {
            self.inner.$name($arg)
        }
    };
    // Single argument by reference
    ($name:ident(&$arg:ident: &$ty:ty) -> $ret:ty) => {
        pub fn $name(&self, $arg: &$ty) -> $ret {
            self.inner.$name($arg)
        }
    };
    // Two arguments
    ($name:ident($arg1:ident: $ty1:ty, $arg2:ident: $ty2:ty) -> $ret:ty) => {
        pub fn $name(&self, $arg1: $ty1, $arg2: $ty2) -> $ret {
            self.inner.$name($arg1, $arg2)
        }
    };
    // Three arguments
    ($name:ident($arg1:ident: $ty1:ty, $arg2:ident: $ty2:ty, $arg3:ident: $ty3:ty) -> $ret:ty) => {
        pub fn $name(&self, $arg1: $ty1, $arg2: $ty2, $arg3: $ty3) -> $ret {
            self.inner.$name($arg1, $arg2, $arg3)
        }
    };
}

/// Convenience wrapper around KeyboardInterface with device open helpers.
///
/// Since `KeyboardInterface` is now fully synchronous, this is a thin
/// delegation layer kept for API compatibility.
pub struct SyncKeyboard {
    inner: KeyboardInterface,
}

impl SyncKeyboard {
    /// Create from a KeyboardInterface
    pub fn new(keyboard: KeyboardInterface) -> Self {
        Self { inner: keyboard }
    }

    /// Create from a flow-controlled transport
    pub fn from_transport(
        transport: Arc<FlowControlTransport>,
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

        // Look up device info - default to M1 V5 HE key count with magnetism
        let (key_count, has_magnetism) = match (info.vid, info.pid) {
            (0x3151, 0x5030) => (crate::KEY_COUNT_M1_V5, true), // M1 V5 HE wired
            (0x3151, 0x5038) => (crate::KEY_COUNT_M1_V5, true), // M1 V5 HE dongle
            _ => (crate::KEY_COUNT_M1_V5, true),                // Default
        };

        Ok(Self {
            inner: KeyboardInterface::new(transport.inner().clone(), key_count, has_magnetism),
        })
    }

    /// Get the underlying interface
    pub fn inner(&self) -> &KeyboardInterface {
        &self.inner
    }

    // === Device Info ===

    pub fn key_count(&self) -> u8 {
        self.inner.key_count()
    }

    pub fn has_magnetism(&self) -> bool {
        self.inner.has_magnetism()
    }

    pub fn is_wireless(&self) -> bool {
        self.inner.is_wireless()
    }

    delegate!(get_device_id() -> Result<u32, KeyboardError>);
    delegate!(get_version() -> Result<FirmwareVersion, KeyboardError>);
    delegate!(get_battery() -> Result<BatteryInfo, KeyboardError>);

    // === Patch Info ===
    delegate!(get_patch_info() -> Result<Option<PatchInfo>, KeyboardError>);

    // === LED ===
    delegate!(get_led_params() -> Result<LedParams, KeyboardError>);
    delegate!(set_led_params(&params: &LedParams) -> Result<(), KeyboardError>);
    delegate!(get_side_led_params() -> Result<LedParams, KeyboardError>);
    delegate!(set_side_led_params(&params: &LedParams) -> Result<(), KeyboardError>);
    delegate!(set_all_keys_color(color: RgbColor, layer: u8) -> Result<(), KeyboardError>);

    // === Userpic ===

    pub fn upload_userpic(&self, slot: u8, data: &[u8]) -> Result<(), KeyboardError> {
        self.inner.upload_userpic(slot, data)
    }

    pub fn download_userpic(&self, slot: u8) -> Result<Vec<u8>, KeyboardError> {
        self.inner.download_userpic(slot)
    }

    // === Settings ===
    delegate!(get_profile() -> Result<u8, KeyboardError>);
    delegate!(set_profile(profile: u8) -> Result<(), KeyboardError>);
    delegate!(get_polling_rate() -> Result<PollingRate, KeyboardError>);
    delegate!(set_polling_rate(rate: PollingRate) -> Result<(), KeyboardError>);
    delegate!(get_debounce() -> Result<u8, KeyboardError>);
    delegate!(set_debounce(ms: u8) -> Result<(), KeyboardError>);
    delegate!(get_sleep_time() -> Result<SleepTimeSettings, KeyboardError>);
    delegate!(set_sleep_time(&settings: &SleepTimeSettings) -> Result<(), KeyboardError>);
    delegate!(get_kb_options() -> Result<KeyboardOptions, KeyboardError>);
    delegate!(set_kb_options(&options: &KeyboardOptions) -> Result<(), KeyboardError>);
    delegate!(get_feature_list() -> Result<FeatureList, KeyboardError>);
    delegate!(get_precision() -> Result<Precision, KeyboardError>);

    // === Magnetism ===
    delegate!(start_magnetism_report() -> Result<(), KeyboardError>);
    delegate!(stop_magnetism_report() -> Result<(), KeyboardError>);

    pub fn read_key_depth(
        &self,
        timeout_ms: u32,
        precision_factor: f64,
    ) -> Result<Option<KeyDepthEvent>, KeyboardError> {
        self.inner.read_key_depth(timeout_ms, precision_factor)
    }

    pub fn poll_notification(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, KeyboardError> {
        self.inner.poll_notification(timeout_ms)
    }

    delegate!(get_key_trigger(key_index: u8) -> Result<KeyTriggerSettings, KeyboardError>);
    delegate!(set_key_trigger(&settings: &KeyTriggerSettings) -> Result<(), KeyboardError>);
    delegate!(get_all_triggers() -> Result<TriggerSettings, KeyboardError>);

    // === Bulk Trigger Setters ===
    delegate!(set_actuation_all_u16(travel: u16) -> Result<(), KeyboardError>);
    delegate!(set_release_all_u16(travel: u16) -> Result<(), KeyboardError>);
    delegate!(set_rt_press_all_u16(sensitivity: u16) -> Result<(), KeyboardError>);
    delegate!(set_rt_lift_all_u16(sensitivity: u16) -> Result<(), KeyboardError>);
    delegate!(set_rapid_trigger_all(enable: bool) -> Result<(), KeyboardError>);
    delegate!(set_bottom_deadzone_all_u16(travel: u16) -> Result<(), KeyboardError>);
    delegate!(set_top_deadzone_all_u16(travel: u16) -> Result<(), KeyboardError>);

    // === Extended LED Control ===

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
        self.inner.set_led(mode, brightness, speed, r, g, b, dazzle)
    }

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
        self.inner
            .set_led_with_option(mode, brightness, speed, r, g, b, dazzle, layer)
    }

    pub fn set_per_key_colors_fast(
        &self,
        colors: &[(u8, u8, u8)],
        repeat: u8,
        layer: u8,
    ) -> Result<(), KeyboardError> {
        self.inner.set_per_key_colors_fast(colors, repeat, layer)
    }

    pub fn set_per_key_colors_to_layer(
        &self,
        colors: &[(u8, u8, u8)],
        layer: u8,
    ) -> Result<(), KeyboardError> {
        self.inner.set_per_key_colors_to_layer(colors, layer)
    }

    // === LED Streaming (patched firmware) ===

    pub fn stream_led_page(&self, page: u8, rgb_data: &[u8]) -> Result<(), KeyboardError> {
        self.inner.stream_led_page(page, rgb_data)
    }

    delegate!(stream_led_commit() -> Result<(), KeyboardError>);
    delegate!(stream_led_release() -> Result<(), KeyboardError>);

    // === Calibration ===
    delegate!(calibrate_min(start: bool) -> Result<(), KeyboardError>);
    delegate!(calibrate_max(start: bool) -> Result<(), KeyboardError>);
    delegate!(get_calibration_progress(page: u8) -> Result<Vec<u16>, KeyboardError>);

    // === Factory Reset ===
    delegate!(reset() -> Result<(), KeyboardError>);

    // === Raw Commands ===
    delegate!(query_raw_cmd(cmd: u8) -> Result<Vec<u8>, KeyboardError>);

    pub fn query_raw_cmd_data(&self, cmd: u8, data: &[u8]) -> Result<Vec<u8>, KeyboardError> {
        self.inner.query_raw_cmd_data(cmd, data)
    }

    pub fn send_raw_cmd(&self, cmd: u8, data: &[u8]) -> Result<(), KeyboardError> {
        self.inner.send_raw_cmd(cmd, data)
    }

    // === Key Matrix ===

    pub fn get_keymatrix(&self, profile: u8, num_pages: usize) -> Result<Vec<u8>, KeyboardError> {
        self.inner.get_keymatrix(profile, num_pages)
    }

    pub fn get_fn_keymatrix(
        &self,
        profile: u8,
        sys: u8,
        num_pages: usize,
    ) -> Result<Vec<u8>, KeyboardError> {
        self.inner.get_fn_keymatrix(profile, sys, num_pages)
    }

    pub fn set_key_config(
        &self,
        profile: u8,
        key_index: u8,
        layer: u8,
        config: [u8; 4],
    ) -> Result<(), KeyboardError> {
        self.inner.set_key_config(profile, key_index, layer, config)
    }

    pub fn set_keymatrix(
        &self,
        profile: u8,
        key_index: u8,
        hid_code: u8,
        enabled: bool,
        layer: u8,
    ) -> Result<(), KeyboardError> {
        self.inner
            .set_keymatrix(profile, key_index, hid_code, enabled, layer)
    }

    pub fn reset_key(&self, layer: u8, key_index: u8) -> Result<(), KeyboardError> {
        self.inner.reset_key(layer, key_index)
    }

    pub fn swap_keys(
        &self,
        profile: u8,
        key_a: u8,
        code_a: u8,
        key_b: u8,
        code_b: u8,
    ) -> Result<(), KeyboardError> {
        self.inner.swap_keys(profile, key_a, code_a, key_b, code_b)
    }

    // === Macros ===
    delegate!(get_macro(macro_index: u8) -> Result<Vec<u8>, KeyboardError>);

    pub fn set_macro(
        &self,
        macro_index: u8,
        events: &[(u8, bool, u16)],
        repeat_count: u16,
    ) -> Result<(), KeyboardError> {
        self.inner.set_macro(macro_index, events, repeat_count)
    }

    pub fn set_text_macro(
        &self,
        macro_index: u8,
        text: &str,
        delay_ms: u16,
        repeat: u16,
    ) -> Result<(), KeyboardError> {
        self.inner
            .set_text_macro(macro_index, text, delay_ms, repeat)
    }

    pub fn assign_macro_to_key(
        &self,
        layer: u8,
        key_index: u8,
        macro_index: u8,
        macro_type: u8,
    ) -> Result<(), KeyboardError> {
        self.inner
            .assign_macro_to_key(layer, key_index, macro_index, macro_type)
    }

    pub fn unassign_macro_from_key(&self, layer: u8, key_index: u8) -> Result<(), KeyboardError> {
        self.inner.unassign_macro_from_key(layer, key_index)
    }

    // === Device Info ===

    pub fn vid(&self) -> u16 {
        self.inner.vid()
    }

    pub fn pid(&self) -> u16 {
        self.inner.pid()
    }

    pub fn device_name(&self) -> String {
        self.inner.device_name()
    }

    // === Connection ===

    pub fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    pub fn close(&self) -> Result<(), KeyboardError> {
        self.inner.close()
    }
}

/// List all connected devices
pub fn list_keyboards() -> Result<Vec<DiscoveredDevice>, KeyboardError> {
    Ok(list_devices_sync()?)
}
