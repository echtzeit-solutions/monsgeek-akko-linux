// MonsGeek/Akko HID Communication
// Shared types for device communication

use crate::devices::DeviceDefinition;

/// Connected device info (VID, PID, path)
#[derive(Debug, Clone)]
pub struct ConnectedDeviceInfo {
    pub vid: u16,
    pub pid: u16,
    pub path: String,
    pub definition: &'static DeviceDefinition,
}

/// Vendor event types from HID input reports
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VendorEventType {
    /// Key depth/magnetism data (byte[0] = 0x1B)
    KeyDepth,
    /// Magnetism reporting started (0x0F, 0x01, 0x00)
    MagnetismStart,
    /// Magnetism reporting stopped (0x0F, 0x00, 0x00)
    MagnetismStop,
    /// Profile changed (byte[0] = 0x01)
    ProfileChange,
    /// LED effect changed (byte[0] = 0x04-0x07)
    LedChange,
    /// Magnetism mode changed (byte[0] = 0x1D)
    MagnetismModeChange,
    /// Battery/wireless status from dongle (byte[0] = 0x88)
    BatteryStatus,
    /// Unknown event type
    Unknown,
}

/// Battery/power status from wireless dongle
#[derive(Debug, Clone, Copy, Default)]
pub struct BatteryInfo {
    /// Battery level 0-100 (255 = unknown/wired)
    pub level: u8,
    /// Device is online/connected
    pub online: bool,
    /// Device is charging
    pub charging: bool,
    /// Device is idle (no recent key activity)
    pub idle: bool,
}

impl BatteryInfo {
    /// Parse battery info from dongle feature report response
    ///
    /// Confirmed byte layout from Windows iot_driver.exe decompilation:
    /// The driver uses protobuf Status24 { battery: u32, is_online: bool }
    ///
    /// HID response format from 2.4GHz dongle (VID:3151 PID:5038):
    /// - byte[0] = 0x00 (Report ID)
    /// - byte[1] = battery level (0-100) - CONFIRMED via USB capture
    /// - byte[2] = 0x00 (unknown)
    /// - byte[3] = idle flag (1 = idle/sleeping, 0 = active/recently pressed)
    /// - byte[4] = online flag (1 = connected)
    /// - byte[5] = 0x01 (unknown flag, always 0x01 in captures)
    /// - byte[6] = 0x01 (unknown flag, always 0x01 in captures)
    /// - byte[7] = 0x00 (unknown)
    ///
    /// Note: Charging status is NOT available via this protocol.
    /// USB packet analysis confirmed bytes 0-7 are identical whether charger
    /// is plugged in or not - only battery percentage changes. The firmware
    /// has charging detection internally (led_state_flags bit 4) but this
    /// is not exposed via USB HID.
    pub fn from_feature_report(data: &[u8]) -> Option<Self> {
        if data.len() < 5 {
            return None;
        }

        // Byte offsets confirmed via Windows driver decompilation
        let level = data[1];
        let online = data[4] != 0;
        // byte[3] = idle flag (1 = idle, 0 = active)
        let idle = data.len() > 3 && data[3] != 0;
        // Note: byte[5] is NOT charging status (user confirmed KB wasn't charging when byte[5]=1)
        // Charging status is not available from dongle protocol
        let charging = false;

        // Sanity check - battery level should be 0-100
        if level > 100 {
            return None;
        }

        Some(Self {
            level,
            online,
            charging,
            idle,
        })
    }

    /// Parse battery info from vendor input event (legacy format)
    /// Format (speculative, from firmware analysis):
    /// - byte[0] = 0x88 (status marker)
    /// - byte[3] = battery level (1-100)
    /// - byte[4] = flags (bit 0 = online, bit 1 = charging)
    pub fn from_vendor_event(data: &[u8]) -> Option<Self> {
        // Skip report ID if present
        let cmd_data = if data.first() == Some(&0x05) && data.len() > 1 {
            &data[1..]
        } else {
            data
        };

        if cmd_data.len() < 5 || cmd_data[0] != 0x88 {
            return None;
        }

        Some(Self {
            level: cmd_data[3],
            online: cmd_data[4] & 0x01 != 0,
            charging: cmd_data[4] & 0x02 != 0,
            idle: false, // Not available in vendor event format
        })
    }

    /// Check if this is valid battery data (not wired/unknown)
    pub fn is_valid(&self) -> bool {
        self.level <= 100
    }
}

/// Per-key trigger settings
#[derive(Debug, Clone, Default)]
pub struct TriggerSettings {
    /// Press travel (actuation point) per key, in precision units
    pub press_travel: Vec<u8>,
    /// Lift travel (release point) per key
    pub lift_travel: Vec<u8>,
    /// Rapid Trigger press sensitivity per key
    pub rt_press: Vec<u8>,
    /// Rapid Trigger lift sensitivity per key
    pub rt_lift: Vec<u8>,
    /// Key mode per key (0=Normal, 2=DKS, 3=MT, 4=TglHold, 5=TglDots, 7=Snap, +128=RT)
    pub key_modes: Vec<u8>,
    /// Bottom deadzone per key (travel below which key won't deactivate)
    pub bottom_deadzone: Vec<u8>,
    /// Top deadzone per key (travel above which key won't activate)
    pub top_deadzone: Vec<u8>,
}

/// Key mode values for per-key settings
pub mod key_mode {
    /// Normal mode - simple actuation/release points
    pub const NORMAL: u8 = 0;
    /// Dynamic Keystroke - 4-stage trigger
    pub const DKS: u8 = 2;
    /// Mod-Tap - different action for tap vs hold
    pub const MOD_TAP: u8 = 3;
    /// Toggle on hold
    pub const TOGGLE_HOLD: u8 = 4;
    /// Toggle on double-tap
    pub const TOGGLE_DOTS: u8 = 5;
    /// Snap-tap - bind to another key
    pub const SNAP_TAP: u8 = 7;
    /// Rapid Trigger flag (OR with mode)
    pub const RT_FLAG: u8 = 0x80;

    /// Check if RT is enabled for this mode
    pub fn has_rt(mode: u8) -> bool {
        mode & RT_FLAG != 0
    }

    /// Get base mode without RT flag
    pub fn base_mode(mode: u8) -> u8 {
        mode & 0x7F
    }

    /// Get mode name
    pub fn name(mode: u8) -> &'static str {
        let base = base_mode(mode);
        match base {
            NORMAL => "Normal",
            DKS => "DKS",
            MOD_TAP => "Mod-Tap",
            TOGGLE_HOLD => "TglHold",
            TOGGLE_DOTS => "TglDots",
            SNAP_TAP => "SnapTap",
            _ => "Unknown",
        }
    }
}

/// Device information read from keyboard
#[derive(Debug, Clone, Default)]
pub struct DeviceInfo {
    pub device_id: u32,
    pub version: u16,
    pub profile: u8,
    pub debounce: u8,
    pub polling_rate: u16,
    pub led_mode: u8,
    pub led_brightness: u8,
    pub led_speed: u8,
    pub led_r: u8,
    pub led_g: u8,
    pub led_b: u8,
    pub led_dazzle: bool,
    // Secondary LED (side lights)
    pub side_mode: u8,
    pub side_brightness: u8,
    pub side_speed: u8,
    pub side_r: u8,
    pub side_g: u8,
    pub side_b: u8,
    pub side_dazzle: bool,
    // Other settings
    pub fn_layer: u8,
    pub wasd_swap: bool,
    pub precision: u8,
    pub sleep_seconds: u16,
}
