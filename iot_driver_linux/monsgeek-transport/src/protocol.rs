//! Protocol constants and utilities for MonsGeek/Akko keyboard communication

use crate::types::ChecksumType;
use std::fmt;
use std::str::FromStr;

/// HID Protocol Commands (FEA_CMD_*)
pub mod cmd {
    // SET commands (0x01 - 0x65)
    pub const SET_RESET: u8 = 0x01;
    pub const SET_REPORT: u8 = 0x03;
    pub const SET_PROFILE: u8 = 0x04;
    pub const SET_DEBOUNCE: u8 = 0x06;
    pub const SET_LEDPARAM: u8 = 0x07;
    pub const SET_SLEDPARAM: u8 = 0x08;
    pub const SET_KBOPTION: u8 = 0x09;
    pub const SET_KEYMATRIX: u8 = 0x0A;
    pub const SET_MACRO: u8 = 0x0B;
    pub const SET_USERPIC: u8 = 0x0C;
    pub const SET_AUDIO_VIZ: u8 = 0x0D;
    pub const SET_SCREEN_COLOR: u8 = 0x0E;
    pub const SET_USERGIF: u8 = 0x12;
    pub const SET_FN: u8 = 0x10;
    pub const SET_SLEEPTIME: u8 = 0x11;
    pub const SET_AUTOOS_EN: u8 = 0x17;
    pub const SET_MAGNETISM_REPORT: u8 = 0x1B;
    pub const SET_MAGNETISM_CAL: u8 = 0x1C;
    pub const SET_MAGNETISM_MAX_CAL: u8 = 0x1E;
    pub const SET_KEY_MAGNETISM_MODE: u8 = 0x1D;
    pub const SET_MULTI_MAGNETISM: u8 = 0x65;

    // GET commands (0x80 - 0xE6)
    pub const GET_REV: u8 = 0x80;
    pub const GET_REPORT: u8 = 0x83;
    pub const GET_PROFILE: u8 = 0x84;
    pub const GET_LEDONOFF: u8 = 0x85;
    pub const GET_DEBOUNCE: u8 = 0x86;
    pub const GET_LEDPARAM: u8 = 0x87;
    pub const GET_SLEDPARAM: u8 = 0x88;
    pub const GET_KBOPTION: u8 = 0x89;
    pub const GET_USERPIC: u8 = 0x8C;
    pub const GET_KEYMATRIX: u8 = 0x8A;
    pub const GET_MACRO: u8 = 0x8B;
    pub const GET_USB_VERSION: u8 = 0x8F;
    pub const GET_FN: u8 = 0x90;
    pub const GET_SLEEPTIME: u8 = 0x91;
    pub const GET_AUTOOS_EN: u8 = 0x97;
    pub const GET_KEY_MAGNETISM_MODE: u8 = 0x9D;
    pub const GET_OLED_VERSION: u8 = 0xAD;
    pub const GET_MLED_VERSION: u8 = 0xAE;
    pub const GET_MULTI_MAGNETISM: u8 = 0xE5;
    pub const GET_FEATURE_LIST: u8 = 0xE6;
    pub const GET_CALIBRATION: u8 = 0xFE;

    // Dongle-specific commands (from dongle firmware RE)
    /// Get dongle info: returns {0xF0, 1, 8, 0,0,0,0, fw_ver}
    pub const GET_DONGLE_INFO: u8 = 0xF0;
    /// Set control byte: stores data[0] → dongle_state.ctrl_byte
    pub const SET_CTRL_BYTE: u8 = 0xF6;
    /// Get dongle status (9-byte response): has_response, kb_battery_info, 0,
    /// kb_charging, 1, rf_ready, 1, pairing_mode, pairing_status.
    /// Handled locally by dongle — NOT forwarded to keyboard.
    pub const GET_DONGLE_STATUS: u8 = 0xF7;
    /// Enter pairing mode: requires 55AA55AA magic
    pub const ENTER_PAIRING: u8 = 0xF8;
    /// Pairing control: sends 3-byte SPI packet {cmd=1, data[0], data[1]}
    pub const PAIRING_CMD: u8 = 0x7A;
    /// Patch info - custom firmware capabilities (battery HID, LED stream, etc.)
    pub const GET_PATCH_INFO: u8 = 0xFB;
    /// Get cached keyboard response: copies 64B cached_kb_response into the
    /// USB feature report buffer and clears has_response. Used as flush.
    pub const GET_CACHED_RESPONSE: u8 = 0xFC;
    /// Get dongle ID: returns {0xAA, 0x55, 0x01, 0x00}.
    /// Note: same byte as GET_CALIBRATION (0xFE is GET_CALIBRATION on keyboard,
    /// 0xFD is GET_DONGLE_ID on dongle, 0xFE is SET_RESPONSE_SIZE on dongle).
    pub const GET_DONGLE_ID: u8 = 0xFD;

    // Response status
    pub const STATUS_SUCCESS: u8 = 0xAA;

    /// Get human-readable name for command byte
    pub fn name(cmd: u8) -> &'static str {
        match cmd {
            SET_RESET => "SET_RESET",
            SET_REPORT => "SET_REPORT",
            SET_PROFILE => "SET_PROFILE",
            SET_DEBOUNCE => "SET_DEBOUNCE",
            SET_LEDPARAM => "SET_LEDPARAM",
            SET_SLEDPARAM => "SET_SLEDPARAM",
            SET_KBOPTION => "SET_KBOPTION",
            SET_KEYMATRIX => "SET_KEYMATRIX",
            SET_MACRO => "SET_MACRO",
            SET_USERPIC => "SET_USERPIC",
            SET_AUDIO_VIZ => "SET_AUDIO_VIZ",
            SET_SCREEN_COLOR => "SET_SCREEN_COLOR",
            SET_USERGIF => "SET_USERGIF",
            SET_FN => "SET_FN",
            SET_SLEEPTIME => "SET_SLEEPTIME",
            SET_AUTOOS_EN => "SET_AUTOOS_EN",
            SET_MAGNETISM_REPORT => "SET_MAGNETISM_REPORT",
            SET_MAGNETISM_CAL => "SET_MAGNETISM_CAL",
            SET_MAGNETISM_MAX_CAL => "SET_MAGNETISM_MAX_CAL",
            SET_KEY_MAGNETISM_MODE => "SET_KEY_MAGNETISM_MODE",
            SET_MULTI_MAGNETISM => "SET_MULTI_MAGNETISM",
            GET_REV => "GET_REV",
            GET_REPORT => "GET_REPORT",
            GET_PROFILE => "GET_PROFILE",
            GET_LEDONOFF => "GET_LEDONOFF",
            GET_DEBOUNCE => "GET_DEBOUNCE",
            GET_LEDPARAM => "GET_LEDPARAM",
            GET_SLEDPARAM => "GET_SLEDPARAM",
            GET_KBOPTION => "GET_KBOPTION",
            GET_USERPIC => "GET_USERPIC",
            GET_KEYMATRIX => "GET_KEYMATRIX",
            GET_MACRO => "GET_MACRO",
            GET_USB_VERSION => "GET_USB_VERSION",
            GET_FN => "GET_FN",
            GET_SLEEPTIME => "GET_SLEEPTIME",
            GET_AUTOOS_EN => "GET_AUTOOS_EN",
            GET_KEY_MAGNETISM_MODE => "GET_KEY_MAGNETISM_MODE",
            GET_OLED_VERSION => "GET_OLED_VERSION",
            GET_MLED_VERSION => "GET_MLED_VERSION",
            GET_MULTI_MAGNETISM => "GET_MULTI_MAGNETISM",
            GET_FEATURE_LIST => "GET_FEATURE_LIST",
            GET_CALIBRATION => "GET_CALIBRATION",
            GET_DONGLE_INFO => "GET_DONGLE_INFO",
            SET_CTRL_BYTE => "SET_CTRL_BYTE",
            GET_DONGLE_STATUS => "GET_DONGLE_STATUS",
            ENTER_PAIRING => "ENTER_PAIRING",
            PAIRING_CMD => "PAIRING_CMD",
            GET_PATCH_INFO => "GET_PATCH_INFO",
            GET_CACHED_RESPONSE => "GET_CACHED_RESPONSE",
            GET_DONGLE_ID => "GET_DONGLE_ID",
            STATUS_SUCCESS => "STATUS_SUCCESS",
            _ => "UNKNOWN",
        }
    }
}

/// Magnetism (Hall Effect trigger) sub-commands for GET/SET_MULTI_MAGNETISM
pub mod magnetism {
    /// Press travel (actuation point)
    pub const PRESS_TRAVEL: u8 = 0x00;
    /// Lift travel (release point)
    pub const LIFT_TRAVEL: u8 = 0x01;
    /// Rapid Trigger press sensitivity
    pub const RT_PRESS: u8 = 0x02;
    /// Rapid Trigger lift sensitivity
    pub const RT_LIFT: u8 = 0x03;
    /// DKS (Dynamic Keystroke) travel
    pub const DKS_TRAVEL: u8 = 0x04;
    /// Mod-Tap activation time
    pub const MODTAP_TIME: u8 = 0x05;
    /// Bottom deadzone
    pub const BOTTOM_DEADZONE: u8 = 0x06;
    /// Key mode (Normal, RT, DKS, etc.)
    pub const KEY_MODE: u8 = 0x07;
    /// Snap Tap anti-SOCD enable
    pub const SNAPTAP_ENABLE: u8 = 0x09;
    /// DKS trigger modes/actions
    pub const DKS_MODES: u8 = 0x0A;
    /// Top deadzone (firmware >= 1024)
    pub const TOP_DEADZONE: u8 = 0xFB;
    /// Switch type (if replaceable)
    pub const SWITCH_TYPE: u8 = 0xFC;
    /// Raw sensor calibration values
    pub const CALIBRATION: u8 = 0xFE;

    /// Get human-readable name for magnetism sub-command
    pub fn name(subcmd: u8) -> &'static str {
        match subcmd {
            PRESS_TRAVEL => "PRESS_TRAVEL",
            LIFT_TRAVEL => "LIFT_TRAVEL",
            RT_PRESS => "RT_PRESS",
            RT_LIFT => "RT_LIFT",
            DKS_TRAVEL => "DKS_TRAVEL",
            MODTAP_TIME => "MODTAP_TIME",
            BOTTOM_DEADZONE => "BOTTOM_DEADZONE",
            KEY_MODE => "KEY_MODE",
            SNAPTAP_ENABLE => "SNAPTAP_ENABLE",
            DKS_MODES => "DKS_MODES",
            TOP_DEADZONE => "TOP_DEADZONE",
            SWITCH_TYPE => "SWITCH_TYPE",
            CALIBRATION => "CALIBRATION",
            _ => "UNKNOWN",
        }
    }
}

/// Key matrix position to name mapping (M1 V5 / SG9000 layout)
///
/// Column-major order, 6 rows per column.  Verified against firmware
/// GET_KEYMATRIX data (factory-default keycodes at each position).
pub mod matrix {
    /// Key names indexed by matrix position (column-major order).
    ///
    /// Row 0 = F-key row, rows 1-4 = main alpha/symbol rows,
    /// row 5 = bottom modifier row.  "?" marks unused matrix slots
    /// (e.g. spacebar columns that don't produce a keypress).
    const KEY_NAMES: &[&str] = &[
        // Col 0 (0-5)
        "Esc", "`", "Tab", "Caps", "LShf", "LCtl", // Col 1 (6-11)
        "F1", "1", "Q", "A", "IntlBs", "Win", // Col 2 (12-17)
        "F2", "2", "W", "S", "Z", "LAlt", // Col 3 (18-23)
        "F3", "3", "E", "D", "X", "?", // Col 4 (24-29)
        "F4", "4", "R", "F", "C", "?", // Col 5 (30-35)
        "F5", "5", "T", "G", "V", "?", // Col 6 (36-41)
        "F6", "6", "Y", "H", "B", "Spc", // Col 7 (42-47)
        "F7", "7", "U", "J", "N", "?", // Col 8 (48-53)
        "F8", "8", "I", "K", "M", "?", // Col 9 (54-59)
        "F9", "9", "O", "L", ",", "RAlt", // Col 10 (60-65)
        "F10", "0", "P", ";", ".", "Fn", // Col 11 (66-71)
        "F11", "-", "[", "'", "/", "RCtl", // Col 12 (72-77)
        "F12", "=", "]", "IntlRo", "RShf", "Left", // Col 13 (78-83)
        "Del", "Bksp", "\\", "Ent", "Up", "Down", // Col 14 (84-89)
        "?", "Home", "PgUp", "PgDn", "End", "Right",
    ];

    /// Get key name from matrix position
    pub fn key_name(index: u8) -> &'static str {
        KEY_NAMES.get(index as usize).copied().unwrap_or("?")
    }

    /// Look up matrix index from key name (case-insensitive)
    ///
    /// Returns None if no matching key name is found.
    pub fn key_index_from_name(name: &str) -> Option<u8> {
        let name_lower = name.to_ascii_lowercase();
        KEY_NAMES
            .iter()
            .position(|&n| n.to_ascii_lowercase() == name_lower && n != "?")
            .map(|i| i as u8)
    }
}

// ---------------------------------------------------------------------------
// Layer
// ---------------------------------------------------------------------------

/// Logical key layer on the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// Base layer (GET_KEYMATRIX page 0, SET_KEYMATRIX layer=0).
    Base,
    /// Second base layer (GET_KEYMATRIX page 1, SET_KEYMATRIX layer=1).
    Layer1,
    /// Fn layer (GET_FN / SET_FN).
    Fn,
}

impl Layer {
    /// All layers in display order.
    pub const ALL: [Layer; 3] = [Layer::Base, Layer::Layer1, Layer::Fn];

    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            Layer::Base => "Layer 0",
            Layer::Layer1 => "Layer 1",
            Layer::Fn => "Fn layer",
        }
    }

    /// Short label for compact display.
    pub fn short(self) -> &'static str {
        match self {
            Layer::Base => "L0",
            Layer::Layer1 => "L1",
            Layer::Fn => "Fn",
        }
    }

    /// Wire value used by `set_key_config(profile, index, layer, config)`.
    pub fn wire_layer(self) -> u8 {
        match self {
            Layer::Base => 0,
            Layer::Layer1 => 1,
            Layer::Fn => 2,
        }
    }

    /// Convert from wire layer number.
    pub fn from_wire(layer: u8) -> Self {
        match layer {
            0 => Layer::Base,
            1 => Layer::Layer1,
            _ => Layer::Fn,
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.short())
    }
}

impl FromStr for Layer {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "0" | "l0" | "base" => Ok(Layer::Base),
            "1" | "l1" => Ok(Layer::Layer1),
            "2" | "fn" => Ok(Layer::Fn),
            _ => Err(format!("unknown layer: \"{s}\". Use 0/L0/base, 1/L1, 2/fn")),
        }
    }
}

// ---------------------------------------------------------------------------
// KeyRef — parsed key + layer reference
// ---------------------------------------------------------------------------

/// A key position + layer reference, e.g. "Fn+Caps" → (Caps, Fn layer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRef {
    pub index: u8,
    pub position: &'static str,
    pub layer: Layer,
}

impl KeyRef {
    pub fn new(index: u8, layer: Layer) -> Self {
        Self {
            index,
            position: matrix::key_name(index),
            layer,
        }
    }
}

impl fmt::Display for KeyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.layer {
            Layer::Base => write!(f, "{}", self.position),
            Layer::Layer1 => write!(f, "L1+{}", self.position),
            Layer::Fn => write!(f, "Fn+{}", self.position),
        }
    }
}

impl FromStr for KeyRef {
    type Err = String;

    /// Parse a key reference with optional layer prefix:
    ///
    /// - `"Caps"` → KeyRef { index=3, layer=Base }
    /// - `"Fn+Caps"` → KeyRef { index=3, layer=Fn }
    /// - `"L1+A"` → KeyRef { index=9, layer=Layer1 }
    /// - `"42"` → KeyRef { index=42, layer=Base }
    /// - `"Fn+42"` → KeyRef { index=42, layer=Fn }
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Check for layer prefix: "Fn+", "L1+", "L0+"
        let (layer, key_part) = if let Some(rest) = strip_prefix_ci(s, "Fn+") {
            (Layer::Fn, rest)
        } else if let Some(rest) = strip_prefix_ci(s, "L1+") {
            (Layer::Layer1, rest)
        } else if let Some(rest) = strip_prefix_ci(s, "L0+") {
            (Layer::Base, rest)
        } else {
            (Layer::Base, s)
        };

        let index = resolve_key(key_part)?;
        Ok(KeyRef {
            index,
            position: matrix::key_name(index),
            layer,
        })
    }
}

/// Case-insensitive prefix strip that returns the remainder with original casing.
///
/// All prefixes used here ("Fn+", "L1+", "L0+") are pure ASCII, so the byte-length
/// comparison is safe on UTF-8 strings.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len()
        && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Resolve a key name or numeric index to a matrix position.
pub fn resolve_key(key: &str) -> Result<u8, String> {
    // Try numeric index first
    if let Ok(idx) = key.parse::<u8>() {
        return Ok(idx);
    }
    // Try matrix key name (Esc, F3, LShf, etc.)
    if let Some(idx) = matrix::key_index_from_name(key) {
        return Ok(idx);
    }
    Err(format!(
        "unknown key: \"{key}\". Use a matrix index (0-95) or name like F3, Esc, Tab"
    ))
}

/// HID report sizes
pub const REPORT_SIZE: usize = 65;
pub const INPUT_REPORT_SIZE: usize = 64;

/// HID communication timing constants
pub mod timing {
    /// Number of retries for query operations
    pub const QUERY_RETRIES: usize = 5;
    /// Number of retries for send operations
    pub const SEND_RETRIES: usize = 3;
    /// Default delay after HID command (ms) - for wired devices
    pub const DEFAULT_DELAY_MS: u64 = 100;
    /// Short delay for fast operations (ms)
    pub const SHORT_DELAY_MS: u64 = 50;
    /// Minimum delay for streaming (ms)
    pub const MIN_DELAY_MS: u64 = 5;
    /// Delay after starting animation upload (ms)
    pub const ANIMATION_START_DELAY_MS: u64 = 500;
}

/// Dongle-specific timing for polling-based flow control
///
/// Based on throughput testing:
/// - Minimum observed latency: ~8-10ms (awake keyboard)
/// - Response requires flush command to push into buffer
/// - Concurrent commands not supported by hardware
pub mod dongle_timing {
    /// Initial wait before first poll attempt (ms)
    /// Adaptive baseline - actual wait is computed from moving average
    pub const INITIAL_WAIT_MS: u64 = 5;

    /// Default timeout for query operations (ms)
    pub const QUERY_TIMEOUT_MS: u64 = 500;

    /// Extended timeout when keyboard may be waking from sleep (ms)
    pub const WAKE_TIMEOUT_MS: u64 = 2000;

    /// Minimum time per poll cycle - flush + read (ms)
    /// Observed ~1.1ms in testing, but allow brief yield
    pub const POLL_CYCLE_MS: u64 = 1;

    /// Moving average window size for latency tracking
    pub const LATENCY_WINDOW_SIZE: usize = 8;

    /// Maximum consecutive timeouts before marking device offline
    pub const MAX_CONSECUTIVE_TIMEOUTS: usize = 3;

    /// Queue capacity for pending command requests
    pub const REQUEST_QUEUE_SIZE: usize = 16;
}

/// RGB/LED data constants
pub mod rgb {
    /// Total RGB data size (126 keys * 3 bytes)
    pub const TOTAL_RGB_SIZE: usize = 378;
    /// Number of pages per frame
    pub const NUM_PAGES: usize = 7;
    /// RGB data per full page
    pub const PAGE_SIZE: usize = 56;
    /// RGB data in last page
    pub const LAST_PAGE_SIZE: usize = 42;
    /// LED matrix positions (keys)
    pub const MATRIX_SIZE: usize = 126;
    /// Number of keys to send per chunk in streaming mode
    pub const CHUNK_SIZE: usize = 18;
    /// Magic value for per-key color commands
    pub const MAGIC_VALUE: u8 = 255;
}

/// Bluetooth Low Energy protocol constants
pub mod ble {
    /// Vendor report ID for BLE HID
    pub const VENDOR_REPORT_ID: u8 = 0x06;
    /// Marker byte for command/response channel
    pub const CMDRESP_MARKER: u8 = 0x55;
    /// Marker byte for event channel
    pub const EVENT_MARKER: u8 = 0x66;
    /// Buffer size for BLE reports (65 bytes + report ID)
    pub const REPORT_SIZE: usize = 66;
    /// Default command delay for BLE (higher than USB due to latency)
    pub const DEFAULT_DELAY_MS: u64 = 150;
}

/// Precision version thresholds
///
/// These constants define firmware version boundaries for different
/// precision levels in travel/trigger settings.
pub mod precision {
    /// Version threshold for fine precision (0.005mm steps)
    /// Firmware versions >= 1280 (0x500) support fine precision
    pub const FINE_VERSION: u16 = 1280;
    /// Version threshold for medium precision (0.01mm steps)
    /// Firmware versions >= 768 (0x300) support medium precision
    pub const MEDIUM_VERSION: u16 = 768;
}

/// Device identification constants
pub mod device {
    /// MonsGeek/Akko vendor ID
    pub const VENDOR_ID: u16 = 0x3151;

    /// M1 V5 HE wired keyboard
    pub const PID_M1_V5_WIRED: u16 = 0x5030;
    /// M1 V5 HE 2.4GHz wireless dongle
    pub const PID_M1_V5_DONGLE: u16 = 0x5038;
    /// M1 V5 HE Bluetooth
    pub const PID_M1_V5_BLUETOOTH: u16 = 0x5027;

    /// HID usage page for vendor-defined (USB)
    pub const USAGE_PAGE: u16 = 0xFFFF;
    /// HID usage for feature interface (USB)
    pub const USAGE_FEATURE: u16 = 0x02;
    /// HID usage for input interface (USB)
    pub const USAGE_INPUT: u16 = 0x01;

    /// Feature interface number
    pub const INTERFACE_FEATURE: i32 = 2;
    /// Input interface number
    pub const INTERFACE_INPUT: i32 = 1;

    /// Check if a PID indicates a wireless dongle
    pub fn is_dongle_pid(pid: u16) -> bool {
        pid == PID_M1_V5_DONGLE
    }

    /// Check if a PID indicates a Bluetooth device
    pub fn is_bluetooth_pid(pid: u16) -> bool {
        pid == PID_M1_V5_BLUETOOTH
    }
}

/// Calculate checksum for HID message
pub fn calculate_checksum(data: &[u8], checksum_type: ChecksumType) -> u8 {
    match checksum_type {
        ChecksumType::Bit7 => {
            let sum: u32 = data.iter().take(7).map(|&b| b as u32).sum();
            (255 - (sum & 0xFF)) as u8
        }
        ChecksumType::Bit8 => {
            let sum: u32 = data.iter().take(8).map(|&b| b as u32).sum();
            (255 - (sum & 0xFF)) as u8
        }
        ChecksumType::None => 0,
    }
}

/// Apply checksum to message buffer
pub fn apply_checksum(data: &mut [u8], checksum_type: ChecksumType) {
    match checksum_type {
        ChecksumType::Bit7 => {
            if data.len() >= 8 {
                data[7] = calculate_checksum(data, checksum_type);
            }
        }
        ChecksumType::Bit8 => {
            if data.len() >= 9 {
                data[8] = calculate_checksum(data, checksum_type);
            }
        }
        ChecksumType::None => {}
    }
}

/// Build a USB command buffer with checksum
///
/// Format: `[report_id=0] [cmd] [data...] [checksum...]`
pub fn build_command(cmd: u8, data: &[u8], checksum_type: ChecksumType) -> Vec<u8> {
    let mut buf = vec![0u8; REPORT_SIZE];
    buf[0] = 0; // Report ID
    buf[1] = cmd;
    let len = std::cmp::min(data.len(), REPORT_SIZE - 2);
    buf[2..2 + len].copy_from_slice(&data[..len]);
    apply_checksum(&mut buf[1..], checksum_type);
    buf
}

/// Build a BLE command buffer with checksum
///
/// BLE uses a different framing than USB:
/// Format: `[report_id=0x06] [0x55 marker] [cmd] [data...] [checksum...]`
///
/// The checksum is calculated starting from the cmd byte (skipping the 0x55 marker).
pub fn build_ble_command(cmd: u8, data: &[u8], checksum_type: ChecksumType) -> Vec<u8> {
    let mut buf = vec![0u8; ble::REPORT_SIZE];
    buf[0] = ble::VENDOR_REPORT_ID; // Report ID 6 for BLE
    buf[1] = ble::CMDRESP_MARKER; // 0x55 marker
    buf[2] = cmd;
    let len = std::cmp::min(data.len(), ble::REPORT_SIZE - 3);
    buf[3..3 + len].copy_from_slice(&data[..len]);
    // Apply checksum starting from cmd byte (index 2)
    apply_checksum(&mut buf[2..], checksum_type);
    buf
}
