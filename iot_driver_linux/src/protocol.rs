// MonsGeek M1 V5 HE Protocol Definitions
// Extracted from Akko Cloud Driver JS

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
    pub const SET_FN: u8 = 0x10;
    pub const SET_SLEEPTIME: u8 = 0x11;
    pub const SET_AUTOOS_EN: u8 = 0x17;
    pub const SET_MAGNETISM_REPORT: u8 = 0x1B;
    pub const SET_MAGNETISM_CAL: u8 = 0x1C;
    pub const SET_MAGNETISM_MAX_CAL: u8 = 0x1E;
    pub const SET_KEY_MAGNETISM_MODE: u8 = 0x1D;
    pub const SET_MULTI_MAGNETISM: u8 = 0x65;

    // GET commands (0x80 - 0xE6)
    pub const GET_REV: u8 = 0x80;           // Get firmware revision
    pub const GET_REPORT: u8 = 0x83;        // Get report rate
    pub const GET_PROFILE: u8 = 0x84;       // Get active profile
    pub const GET_DEBOUNCE: u8 = 0x86;      // Get debounce settings
    pub const GET_LEDPARAM: u8 = 0x87;      // Get LED parameters
    pub const GET_SLEDPARAM: u8 = 0x88;     // Get secondary LED params
    pub const GET_KBOPTION: u8 = 0x89;      // Get keyboard options
    pub const GET_KEYMATRIX: u8 = 0x8A;     // Get key mappings
    pub const GET_MACRO: u8 = 0x8B;         // Get macros
    pub const GET_USB_VERSION: u8 = 0x8F;   // Get USB version
    pub const GET_FN: u8 = 0x90;            // Get Fn layer
    pub const GET_SLEEPTIME: u8 = 0x91;     // Get sleep timeout
    pub const GET_AUTOOS_EN: u8 = 0x97;     // Get auto-OS setting
    pub const GET_KEY_MAGNETISM_MODE: u8 = 0x9D;
    pub const GET_MULTI_MAGNETISM: u8 = 0xE5;  // Get RT/DKS per-key settings
    pub const GET_FEATURE_LIST: u8 = 0xE6;     // Get supported features

    // Response status
    pub const STATUS_SUCCESS: u8 = 0xAA;

    /// LED effect mode names (from Akko Cloud LightList)
    pub const LED_MODES: &[&str] = &[
        "Off",              // 0
        "Constant",         // 1
        "Breathing",        // 2
        "Neon",             // 3
        "Wave",             // 4
        "Ripple",           // 5
        "Raindrop",         // 6
        "Snake",            // 7
        "Reactive",         // 8
        "Converge",         // 9
        "Sine Wave",        // 10
        "Kaleidoscope",     // 11
        "Line Wave",        // 12
        "User Picture",     // 13
        "Laser",            // 14
        "Circle Wave",      // 15
        "Rainbow",          // 16
        "Rain Down",        // 17
        "Meteor",           // 18
        "Reactive Off",     // 19
        "Music Reactive 3", // 20
        "Screen Color",     // 21
        "Music Reactive 2", // 22
        "Train",            // 23
        "Fireworks",        // 24
        "Per-Key Color",    // 25
    ];

    pub fn led_mode_name(mode: u8) -> &'static str {
        LED_MODES.get(mode as usize).unwrap_or(&"Unknown")
    }

    /// Maximum LED mode index
    pub const LED_MODE_MAX: u8 = (LED_MODES.len() - 1) as u8;

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
            GET_DEBOUNCE => "GET_DEBOUNCE",
            GET_LEDPARAM => "GET_LEDPARAM",
            GET_SLEDPARAM => "GET_SLEDPARAM",
            GET_KBOPTION => "GET_KBOPTION",
            GET_KEYMATRIX => "GET_KEYMATRIX",
            GET_MACRO => "GET_MACRO",
            GET_USB_VERSION => "GET_USB_VERSION",
            GET_FN => "GET_FN",
            GET_SLEEPTIME => "GET_SLEEPTIME",
            GET_AUTOOS_EN => "GET_AUTOOS_EN",
            GET_KEY_MAGNETISM_MODE => "GET_KEY_MAGNETISM_MODE",
            GET_MULTI_MAGNETISM => "GET_MULTI_MAGNETISM",
            GET_FEATURE_LIST => "GET_FEATURE_LIST",
            _ => "UNKNOWN",
        }
    }
}

/// Checksum types used by the protocol
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChecksumType {
    Bit7,  // Checksum at byte 7 (most common)
    Bit8,  // Checksum at byte 8 (for LED commands)
    None,
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

/// Device identification
pub const VENDOR_ID: u16 = 0x3151;
pub const PRODUCT_ID: u16 = 0x5030;
pub const USAGE_PAGE: u16 = 0xFFFF;
pub const USAGE: u16 = 0x02;
pub const INTERFACE: i32 = 2;

/// Additional known product IDs (from iot_driver.exe)
pub const PRODUCT_ID_M1_V5_WIRED: u16 = 0x5030;
pub const PRODUCT_ID_M1_V5_WIRELESS: u16 = 0x503A;
pub const PRODUCT_ID_DONGLE_1: u16 = 0x503D;
pub const PRODUCT_ID_DONGLE_2: u16 = 0x5040;

/// HID report sizes
pub const REPORT_SIZE: usize = 65;       // Feature report size (with report ID)
pub const INPUT_REPORT_SIZE: usize = 64; // Input report size

/// LED dazzle (rainbow color cycle) option values
pub const LED_DAZZLE_OFF: u8 = 7;
pub const LED_DAZZLE_ON: u8 = 8;
pub const LED_OPTIONS_MASK: u8 = 0x0F;

/// LED brightness/speed range (0-4)
pub const LED_BRIGHTNESS_MAX: u8 = 4;
pub const LED_SPEED_MAX: u8 = 4;

/// Magnetism sub-commands for GET/SET_MULTI_MAGNETISM
pub mod magnetism {
    /// Press travel (actuation point) - values in precision units
    pub const PRESS_TRAVEL: u8 = 0;
    /// Lift travel (release point)
    pub const LIFT_TRAVEL: u8 = 1;
    /// Rapid Trigger press sensitivity
    pub const RT_PRESS: u8 = 2;
    /// Rapid Trigger lift sensitivity
    pub const RT_LIFT: u8 = 3;
    /// DKS (Dynamic Keystroke) travel
    pub const DKS_TRAVEL: u8 = 4;
    /// Mod-Tap activation time
    pub const MODTAP_TIME: u8 = 5;
    /// Bottom dead zone
    pub const BOTTOM_DEADZONE: u8 = 6;
    /// Key mode flags (Normal/RT/DKS/ModTap/Toggle/SnapTap)
    pub const KEY_MODE: u8 = 7;
    /// Snap Tap anti-SOCD enable
    pub const SNAPTAP_ENABLE: u8 = 9;
    /// DKS trigger modes/actions
    pub const DKS_MODES: u8 = 10;
    /// Top dead zone (firmware >= 1024)
    pub const TOP_DEADZONE: u8 = 251;
    /// Switch type (if replaceable)
    pub const SWITCH_TYPE: u8 = 252;
    /// Calibration values (raw sensor)
    pub const CALIBRATION: u8 = 254;

    /// Key mode values
    pub const MODE_NORMAL: u8 = 0;
    pub const MODE_RAPID_TRIGGER: u8 = 1;
    pub const MODE_DKS: u8 = 2;
    pub const MODE_MODTAP: u8 = 3;
    pub const MODE_TOGGLE: u8 = 4;
    pub const MODE_SNAPTAP: u8 = 5;

    pub fn mode_name(mode: u8) -> &'static str {
        match mode {
            MODE_NORMAL => "Normal",
            MODE_RAPID_TRIGGER => "Rapid Trigger",
            MODE_DKS => "DKS",
            MODE_MODTAP => "Mod-Tap",
            MODE_TOGGLE => "Toggle",
            MODE_SNAPTAP => "Snap Tap",
            _ => "Unknown",
        }
    }
}
