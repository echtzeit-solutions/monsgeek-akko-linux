//! Protocol constants and utilities for MonsGeek/Akko keyboard communication

use crate::types::ChecksumType;

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

    // Dongle-specific commands
    /// Battery refresh - triggers dongle to query keyboard over 2.4GHz RF
    pub const BATTERY_REFRESH: u8 = 0xF7;
    /// Flush/NOP - used to flush dongle response buffer
    pub const DONGLE_FLUSH_NOP: u8 = 0xFC;

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
            BATTERY_REFRESH => "BATTERY_REFRESH",
            DONGLE_FLUSH_NOP => "DONGLE_FLUSH_NOP",
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
/// Derived from ledMatrix in device database - maps firmware matrix position to HID codes.
/// The matrix is column-major with 6 rows per column.
pub mod matrix {
    /// Key names indexed by matrix position (column-major order)
    /// Derived from Common82_SG9000 ledMatrix which maps position -> HID usage code
    const KEY_NAMES: &[&str] = &[
        // Col 0 (0-5)
        "Esc", "`", "Tab", "Caps", "LShf", "LCtl",
        // Col 1 (6-11) - has empty slots for layout variation
        "?", "1", "Q", "A", "IntlBs", "?", // Col 2 (12-17)
        "F1", "2", "W", "S", "Z", "Win", // Col 3 (18-23)
        "F2", "3", "E", "D", "X", "LAlt", // Col 4 (24-29)
        "F3", "4", "R", "F", "C", "?", // Col 5 (30-35)
        "F4", "5", "T", "G", "V", "Spc", // Col 6 (36-41)
        "F5", "6", "Y", "H", "B", "?", // Col 7 (42-47)
        "F6", "7", "U", "J", "N", "?", // Col 8 (48-53)
        "F7", "8", "I", "K", "M", "RAlt", // Col 9 (54-59)
        "F8", "9", "O", "L", ",", "?", // Col 10 (60-65)
        "F9", "0", "P", ";", ".", "RCtl", // Col 11 (66-71)
        "F10", "-", "[", "'", "/", "?", // Col 12 (72-77)
        "F11", "=", "]", "IntlRo", "RShf", "Left", // Col 13 (78-83)
        "F12", "Bksp", "\\", "Ent", "Up", "Down", // Col 14 (84-89)
        "Del", "Home", "PgUp", "PgDn", "End", "Right",
    ];

    /// Get key name from matrix position
    pub fn key_name(index: u8) -> &'static str {
        KEY_NAMES.get(index as usize).copied().unwrap_or("?")
    }
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
    /// Delay after sending command to dongle (ms) - legacy, use dongle_timing for new code
    pub const DONGLE_POST_SEND_DELAY_MS: u64 = 150;
    /// Delay after dongle flush before reading (ms) - legacy, use dongle_timing for new code
    pub const DONGLE_POST_FLUSH_DELAY_MS: u64 = 100;
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
