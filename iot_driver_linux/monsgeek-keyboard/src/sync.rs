//! Synchronous adapter for KeyboardInterface
//!
//! Provides blocking wrappers around the async KeyboardInterface,
//! enabling use in synchronous code (like TUI worker threads).

use std::sync::Arc;

use crate::error::KeyboardError;
use crate::led::{LedParams, RgbColor};
use crate::magnetism::{KeyDepthEvent, KeyTriggerSettings, TriggerSettings};
use crate::settings::{
    BatteryInfo, FeatureList, FirmwareVersion, KeyboardOptions, PollingRate, Precision,
    SleepTimeSettings,
};
use crate::KeyboardInterface;

use monsgeek_transport::{
    list_devices_sync, open_device_sync, DiscoveredDevice, FlowControlTransport, VendorEvent,
};

/// Block on a future using futures crate (works in any context)
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    futures::executor::block_on(f)
}

/// Macro to generate synchronous wrapper methods
///
/// Reduces boilerplate by auto-generating `block_on(self.inner.method())` calls.
///
/// # Usage patterns:
/// - `sync_method!(name() -> Type)` - no args, returns Result<Type, Error>
/// - `sync_method!(name() -> ())` - no args, returns Result<(), Error>
/// - `sync_method!(name(arg: Type) -> RetType)` - with args
/// - `sync_method!(name(&arg: &Type) -> RetType)` - with ref args
macro_rules! sync_method {
    // No arguments, with return type
    ($name:ident() -> $ret:ty) => {
        pub fn $name(&self) -> $ret {
            block_on(self.inner.$name())
        }
    };
    // Single argument by value
    ($name:ident($arg:ident: $ty:ty) -> $ret:ty) => {
        pub fn $name(&self, $arg: $ty) -> $ret {
            block_on(self.inner.$name($arg))
        }
    };
    // Single argument by reference
    ($name:ident(&$arg:ident: &$ty:ty) -> $ret:ty) => {
        pub fn $name(&self, $arg: &$ty) -> $ret {
            block_on(self.inner.$name($arg))
        }
    };
    // Two arguments
    ($name:ident($arg1:ident: $ty1:ty, $arg2:ident: $ty2:ty) -> $ret:ty) => {
        pub fn $name(&self, $arg1: $ty1, $arg2: $ty2) -> $ret {
            block_on(self.inner.$name($arg1, $arg2))
        }
    };
    // Three arguments
    ($name:ident($arg1:ident: $ty1:ty, $arg2:ident: $ty2:ty, $arg3:ident: $ty3:ty) -> $ret:ty) => {
        pub fn $name(&self, $arg1: $ty1, $arg2: $ty2, $arg3: $ty3) -> $ret {
            block_on(self.inner.$name($arg1, $arg2, $arg3))
        }
    };
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

    // Device queries using macro
    sync_method!(get_device_id() -> Result<u32, KeyboardError>);
    sync_method!(get_version() -> Result<FirmwareVersion, KeyboardError>);
    sync_method!(get_battery() -> Result<BatteryInfo, KeyboardError>);

    // === LED ===
    sync_method!(get_led_params() -> Result<LedParams, KeyboardError>);
    sync_method!(set_led_params(&params: &LedParams) -> Result<(), KeyboardError>);
    sync_method!(get_side_led_params() -> Result<LedParams, KeyboardError>);
    sync_method!(set_side_led_params(&params: &LedParams) -> Result<(), KeyboardError>);
    sync_method!(set_all_keys_color(color: RgbColor, layer: u8) -> Result<(), KeyboardError>);

    // === Settings ===
    sync_method!(get_profile() -> Result<u8, KeyboardError>);
    sync_method!(set_profile(profile: u8) -> Result<(), KeyboardError>);
    sync_method!(get_polling_rate() -> Result<PollingRate, KeyboardError>);
    sync_method!(set_polling_rate(rate: PollingRate) -> Result<(), KeyboardError>);
    sync_method!(get_debounce() -> Result<u8, KeyboardError>);
    sync_method!(set_debounce(ms: u8) -> Result<(), KeyboardError>);
    sync_method!(get_sleep_time() -> Result<SleepTimeSettings, KeyboardError>);
    sync_method!(set_sleep_time(&settings: &SleepTimeSettings) -> Result<(), KeyboardError>);
    sync_method!(get_kb_options() -> Result<KeyboardOptions, KeyboardError>);
    sync_method!(set_kb_options(&options: &KeyboardOptions) -> Result<(), KeyboardError>);
    sync_method!(get_feature_list() -> Result<FeatureList, KeyboardError>);
    sync_method!(get_precision() -> Result<Precision, KeyboardError>);

    // === Magnetism ===
    sync_method!(start_magnetism_report() -> Result<(), KeyboardError>);
    sync_method!(stop_magnetism_report() -> Result<(), KeyboardError>);

    /// Read key depth event
    pub fn read_key_depth(
        &self,
        timeout_ms: u32,
        precision_factor: f64,
    ) -> Result<Option<KeyDepthEvent>, KeyboardError> {
        block_on(self.inner.read_key_depth(timeout_ms, precision_factor))
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
    pub fn poll_notification(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, KeyboardError> {
        block_on(self.inner.poll_notification(timeout_ms))
    }

    sync_method!(get_key_trigger(key_index: u8) -> Result<KeyTriggerSettings, KeyboardError>);
    sync_method!(set_key_trigger(&settings: &KeyTriggerSettings) -> Result<(), KeyboardError>);
    sync_method!(get_all_triggers() -> Result<TriggerSettings, KeyboardError>);

    // === Bulk Trigger Setters ===
    sync_method!(set_actuation_all_u16(travel: u16) -> Result<(), KeyboardError>);
    sync_method!(set_release_all_u16(travel: u16) -> Result<(), KeyboardError>);
    sync_method!(set_rt_press_all_u16(sensitivity: u16) -> Result<(), KeyboardError>);
    sync_method!(set_rt_lift_all_u16(sensitivity: u16) -> Result<(), KeyboardError>);
    sync_method!(set_rapid_trigger_all(enable: bool) -> Result<(), KeyboardError>);
    sync_method!(set_bottom_deadzone_all_u16(travel: u16) -> Result<(), KeyboardError>);
    sync_method!(set_top_deadzone_all_u16(travel: u16) -> Result<(), KeyboardError>);

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
    sync_method!(start_user_gif() -> Result<(), KeyboardError>);

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
    sync_method!(calibrate_min(start: bool) -> Result<(), KeyboardError>);
    sync_method!(calibrate_max(start: bool) -> Result<(), KeyboardError>);
    sync_method!(get_calibration_progress(page: u8) -> Result<Vec<u16>, KeyboardError>);

    // === Factory Reset ===
    sync_method!(reset() -> Result<(), KeyboardError>);

    // === Raw Commands ===
    sync_method!(query_raw_cmd(cmd: u8) -> Result<Vec<u8>, KeyboardError>);

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
    sync_method!(get_macro(macro_index: u8) -> Result<Vec<u8>, KeyboardError>);

    /// Set macro data for a macro slot
    pub fn set_macro(
        &self,
        macro_index: u8,
        events: &[(u8, bool, u16)],
        repeat_count: u16,
    ) -> Result<(), KeyboardError> {
        block_on(self.inner.set_macro(macro_index, events, repeat_count))
    }

    /// Set a text macro (convenience method)
    pub fn set_text_macro(
        &self,
        macro_index: u8,
        text: &str,
        delay_ms: u16,
        repeat: u16,
    ) -> Result<(), KeyboardError> {
        block_on(
            self.inner
                .set_text_macro(macro_index, text, delay_ms, repeat),
        )
    }

    /// Assign a macro to a key on the Fn layer
    pub fn assign_macro_to_key(
        &self,
        profile: u8,
        key_index: u8,
        macro_index: u8,
        macro_type: u8,
    ) -> Result<(), KeyboardError> {
        block_on(
            self.inner
                .assign_macro_to_key(profile, key_index, macro_index, macro_type),
        )
    }

    /// Remove macro assignment from a key
    pub fn unassign_macro_from_key(&self, profile: u8, key_index: u8) -> Result<(), KeyboardError> {
        block_on(self.inner.unassign_macro_from_key(profile, key_index))
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

    sync_method!(close() -> Result<(), KeyboardError>);
}

/// List all connected devices
pub fn list_keyboards() -> Result<Vec<DiscoveredDevice>, KeyboardError> {
    Ok(list_devices_sync()?)
}
