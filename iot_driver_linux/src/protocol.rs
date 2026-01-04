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
    pub const SET_USERPIC: u8 = 0x0C;  // Per-key RGB colors (static)
    pub const SET_AUDIO_VIZ: u8 = 0x0D;  // Audio visualizer frequency bands (16 bands, values 0-6)
    pub const SET_SCREEN_COLOR: u8 = 0x0E;  // Screen color RGB (streamed, for mode 21)
    pub const SET_USERGIF: u8 = 0x12;  // Per-key RGB animation (dynamic)
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
    pub const GET_USERPIC: u8 = 0x8C;       // Get per-key RGB colors
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

    /// LED mode enum for type-safe mode selection
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u8)]
    pub enum LedMode {
        Off = 0,
        Constant = 1,
        Breathing = 2,
        Neon = 3,
        Wave = 4,
        Ripple = 5,
        Raindrop = 6,
        Snake = 7,
        Reactive = 8,
        Converge = 9,
        SineWave = 10,
        Kaleidoscope = 11,
        LineWave = 12,
        UserPicture = 13,   // Static per-key colors (4 layers)
        Laser = 14,
        CircleWave = 15,
        Rainbow = 16,
        RainDown = 17,
        Meteor = 18,
        ReactiveOff = 19,
        Music3 = 20,
        ScreenColor = 21,
        Music2 = 22,
        Train = 23,
        Fireworks = 24,
        UserColor = 25,     // Dynamic per-key animation (GIF)
    }

    impl LedMode {
        /// Convert from u8, returns None if invalid
        pub fn from_u8(value: u8) -> Option<Self> {
            match value {
                0 => Some(Self::Off),
                1 => Some(Self::Constant),
                2 => Some(Self::Breathing),
                3 => Some(Self::Neon),
                4 => Some(Self::Wave),
                5 => Some(Self::Ripple),
                6 => Some(Self::Raindrop),
                7 => Some(Self::Snake),
                8 => Some(Self::Reactive),
                9 => Some(Self::Converge),
                10 => Some(Self::SineWave),
                11 => Some(Self::Kaleidoscope),
                12 => Some(Self::LineWave),
                13 => Some(Self::UserPicture),
                14 => Some(Self::Laser),
                15 => Some(Self::CircleWave),
                16 => Some(Self::Rainbow),
                17 => Some(Self::RainDown),
                18 => Some(Self::Meteor),
                19 => Some(Self::ReactiveOff),
                20 => Some(Self::Music3),
                21 => Some(Self::ScreenColor),
                22 => Some(Self::Music2),
                23 => Some(Self::Train),
                24 => Some(Self::Fireworks),
                25 => Some(Self::UserColor),
                _ => None,
            }
        }

        /// Parse from string (case-insensitive, supports names and numbers)
        pub fn parse(s: &str) -> Option<Self> {
            // Try parsing as number first
            if let Ok(n) = s.parse::<u8>() {
                return Self::from_u8(n);
            }

            // Try matching name (case-insensitive)
            match s.to_lowercase().as_str() {
                "off" => Some(Self::Off),
                "constant" | "solid" => Some(Self::Constant),
                "breathing" | "breath" => Some(Self::Breathing),
                "neon" => Some(Self::Neon),
                "wave" => Some(Self::Wave),
                "ripple" => Some(Self::Ripple),
                "raindrop" | "rain" => Some(Self::Raindrop),
                "snake" => Some(Self::Snake),
                "reactive" => Some(Self::Reactive),
                "converge" => Some(Self::Converge),
                "sinewave" | "sine" => Some(Self::SineWave),
                "kaleidoscope" | "kaleid" => Some(Self::Kaleidoscope),
                "linewave" | "line" => Some(Self::LineWave),
                "userpicture" | "picture" | "static" => Some(Self::UserPicture),
                "laser" => Some(Self::Laser),
                "circlewave" | "circle" => Some(Self::CircleWave),
                "rainbow" => Some(Self::Rainbow),
                "raindown" => Some(Self::RainDown),
                "meteor" => Some(Self::Meteor),
                "reactiveoff" => Some(Self::ReactiveOff),
                "music3" => Some(Self::Music3),
                "screencolor" | "screen" => Some(Self::ScreenColor),
                "music2" => Some(Self::Music2),
                "train" => Some(Self::Train),
                "fireworks" => Some(Self::Fireworks),
                "usercolor" | "color" | "gif" | "animation" => Some(Self::UserColor),
                _ => None,
            }
        }

        /// Get the display name
        pub fn name(&self) -> &'static str {
            LED_MODES[*self as usize]
        }

        /// Get the numeric value
        pub fn as_u8(&self) -> u8 {
            *self as u8
        }

        /// List all modes with their names
        pub fn list_all() -> impl Iterator<Item = (u8, &'static str)> {
            LED_MODES.iter().enumerate().map(|(i, name)| (i as u8, *name))
        }
    }

    impl std::fmt::Display for LedMode {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.name())
        }
    }

    // Keep constants for backward compatibility
    pub const LED_MODE_USER_PICTURE: u8 = LedMode::UserPicture as u8;
    pub const LED_MODE_USER_COLOR: u8 = LedMode::UserColor as u8;

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
            SET_SCREEN_COLOR => "SET_SCREEN_COLOR",
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

// Device constants (VID, PID, USAGE, etc.) are now in hal::constants
// Use hal::VENDOR_ID, hal::PRODUCT_ID_*, etc.

/// HID report sizes
pub const REPORT_SIZE: usize = 65;       // Feature report size (with report ID)
pub const INPUT_REPORT_SIZE: usize = 64; // Input report size

/// HID communication timing constants
pub mod timing {
    /// Number of retries for query operations
    pub const QUERY_RETRIES: usize = 5;
    /// Number of retries for send operations
    pub const SEND_RETRIES: usize = 3;
    /// Default delay after HID command (ms)
    pub const DEFAULT_DELAY_MS: u64 = 100;
    /// Short delay for fast operations (ms)
    pub const SHORT_DELAY_MS: u64 = 50;
    /// Minimum delay for streaming (ms)
    pub const MIN_DELAY_MS: u64 = 5;
    /// Delay after animation start (ms)
    pub const ANIMATION_START_DELAY_MS: u64 = 500;
}

/// Per-key RGB animation constants
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
    /// Magic value for per-key color commands
    pub const MAGIC_VALUE: u8 = 255;
}

/// Firmware version thresholds for precision
pub mod firmware {
    /// Version threshold for 0.005mm precision
    pub const PRECISION_HIGH_VERSION: u16 = 1280;
    /// Version threshold for 0.01mm precision
    pub const PRECISION_MID_VERSION: u16 = 768;
    /// Precision factor for 0.005mm
    pub const PRECISION_HIGH_FACTOR: f32 = 200.0;
    /// Precision factor for 0.01mm
    pub const PRECISION_MID_FACTOR: f32 = 100.0;
    /// Precision factor for 0.1mm (legacy)
    pub const PRECISION_LOW_FACTOR: f32 = 10.0;
}

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

/// Key matrix position to name mapping
///
/// **DEPRECATED**: Use `crate::profile::DeviceProfile::matrix_key_name()` instead.
/// This module contains incorrect mappings for some keys.
/// The profile system provides correct per-device matrix key mappings.
#[deprecated(
    since = "0.2.0",
    note = "Use crate::profile::DeviceProfile::matrix_key_name() instead"
)]
pub mod matrix {
    /// Key names indexed by matrix position (column-major order)
    /// **DEPRECATED**: This mapping is incomplete/incorrect for M1 V5 HE.
    /// Use the device profile system for accurate mappings.
    const KEY_NAMES: &[&str] = &[
        // Col 0 (0-5): Esc column
        "Esc", "`", "Tab", "Caps", "LShf", "LCtl",
        // Col 1 (6-11): 1/Q/A/Z column
        "F1", "1", "Q", "A", "Z", "Win",
        // Col 2 (12-17): 2/W/S/X column
        "F2", "2", "W", "S", "X", "LAlt",
        // Col 3 (18-23): 3/E/D/C column
        "F3", "3", "E", "D", "C", "Spc",
        // Col 4 (24-29): 4/R/F/V column
        "F4", "4", "R", "F", "V", "Spc",
        // Col 5 (30-35): 5/T/G/B column
        "F5", "5", "T", "G", "B", "Spc",
        // Col 6 (36-41): 6/Y/H/N column
        "F6", "6", "Y", "H", "N", "Spc",
        // Col 7 (42-47): 7/U/J/M column
        "F7", "7", "U", "J", "M", "Spc",
        // Col 8 (48-53): 8/I/K/, column
        "F8", "8", "I", "K", ",", "RAlt",
        // Col 9 (54-59): 9/O/L/. column
        "F9", "9", "O", "L", ".", "Fn",
        // Col 10 (60-65): 0/P/;// column
        "F10", "0", "P", ";", "/", "RCtl",
        // Col 11 (66-71): -/[/'/RShf column
        "F11", "-", "[", "'", "RShf", "Left",
        // Col 12 (72-77): =/]/Enter/Up column
        "F12", "=", "]", "Ent", "Up", "Down",
        // Col 13 (78-83): Bksp/\/PgDn/End/Right column
        "Del", "Bksp", "\\", "PgUp", "PgDn", "Right",
        // Col 14 (84-89): Extra keys (if any)
        "Knob", "Ins", "Home", "End", "?", "?",
    ];

    /// Get key name from matrix position
    ///
    /// **DEPRECATED**: Use `crate::profile::DeviceProfile::matrix_key_name()` instead.
    #[deprecated(
        since = "0.2.0",
        note = "Use crate::profile::DeviceProfile::matrix_key_name() instead"
    )]
    #[allow(deprecated)]
    pub fn key_name(index: u8) -> &'static str {
        KEY_NAMES.get(index as usize).copied().unwrap_or("?")
    }
}

/// Polling rate (report rate) encoding/decoding
/// Protocol: SET_REPORT (0x03) / GET_REPORT (0x83)
/// Format: [cmd, 0, rate_code, 0, 0, 0, 0, checksum]
pub mod polling_rate {
    /// Available polling rates in Hz
    pub const RATES: &[u16] = &[8000, 4000, 2000, 1000, 500, 250, 125];

    /// Encode polling rate (Hz) to protocol value (0-6)
    /// Returns None if rate is not supported
    pub fn encode(hz: u16) -> Option<u8> {
        match hz {
            8000 => Some(0),
            4000 => Some(1),
            2000 => Some(2),
            1000 => Some(3),
            500 => Some(4),
            250 => Some(5),
            125 => Some(6),
            _ => None,
        }
    }

    /// Decode protocol value (0-6) to polling rate in Hz
    /// Returns None if value is invalid
    pub fn decode(code: u8) -> Option<u16> {
        match code {
            0 => Some(8000),
            1 => Some(4000),
            2 => Some(2000),
            3 => Some(1000),
            4 => Some(500),
            5 => Some(250),
            6 => Some(125),
            _ => None,
        }
    }

    /// Get polling rate name for display
    pub fn name(hz: u16) -> String {
        if hz >= 1000 {
            format!("{}kHz", hz / 1000)
        } else {
            format!("{hz}Hz")
        }
    }

    /// Parse rate from string (e.g., "1000", "1000hz", "1khz", "1k")
    pub fn parse(s: &str) -> Option<u16> {
        let s = s.to_lowercase().trim().to_string();

        // Handle "khz" suffix
        if let Some(num) = s.strip_suffix("khz") {
            let n: u16 = num.trim().parse().ok()?;
            let hz = n * 1000;
            return if RATES.contains(&hz) { Some(hz) } else { None };
        }

        // Handle "k" suffix
        if let Some(num) = s.strip_suffix('k') {
            let n: u16 = num.trim().parse().ok()?;
            let hz = n * 1000;
            return if RATES.contains(&hz) { Some(hz) } else { None };
        }

        // Handle "hz" suffix
        if let Some(num) = s.strip_suffix("hz") {
            let hz: u16 = num.trim().parse().ok()?;
            return if RATES.contains(&hz) { Some(hz) } else { None };
        }

        // Plain number
        let hz: u16 = s.parse().ok()?;
        if RATES.contains(&hz) { Some(hz) } else { None }
    }
}

/// Magnetism (key depth) report parsing
/// Report format: [report_id(0x05), cmd(0x1B), depth_lo, depth_hi, key_index, 0, 0, 0, ...]
pub mod depth_report {
    use super::cmd;

    /// Report ID for magnetism reports on Linux
    pub const REPORT_ID: u8 = 0x05;

    /// Parsed depth report
    #[derive(Debug, Clone, Copy)]
    pub struct DepthReport {
        /// Key matrix index (0-125)
        pub key_index: u8,
        /// Raw depth value from sensor
        pub depth_raw: u16,
    }

    impl DepthReport {
        /// Convert raw depth to millimeters using precision factor
        pub fn depth_mm(&self, precision: f32) -> f32 {
            self.depth_raw as f32 / precision
        }
    }

    /// Parse a magnetism depth report from raw HID buffer
    /// Returns None if buffer is not a valid depth report
    pub fn parse(buf: &[u8]) -> Option<DepthReport> {
        if buf.len() >= 5 {
            // Check for report ID prefix (Linux HID includes report ID as byte 0)
            let (depth_lo, depth_hi, key_idx) = if buf[0] == REPORT_ID && buf[1] == cmd::SET_MAGNETISM_REPORT {
                // Format: [report_id(0x05), cmd(0x1B), depth_lo, depth_hi, key_index, ...]
                (buf[2], buf[3], buf[4])
            } else if buf[0] == cmd::SET_MAGNETISM_REPORT {
                // Format without report ID: [cmd(0x1B), depth_lo, depth_hi, key_index, ...]
                (buf[1], buf[2], buf[3])
            } else {
                return None;
            };

            Some(DepthReport {
                key_index: key_idx,
                depth_raw: (depth_lo as u16) | ((depth_hi as u16) << 8),
            })
        } else {
            None
        }
    }
}

/// HID Usage Table for Keyboard/Keypad (USB HID Usage Tables, Section 10)
pub mod hid {
    /// Get the name of a HID keyboard usage code
    pub fn key_name(code: u8) -> &'static str {
        match code {
            0x00 => "None",
            0x04 => "A", 0x05 => "B", 0x06 => "C", 0x07 => "D",
            0x08 => "E", 0x09 => "F", 0x0A => "G", 0x0B => "H",
            0x0C => "I", 0x0D => "J", 0x0E => "K", 0x0F => "L",
            0x10 => "M", 0x11 => "N", 0x12 => "O", 0x13 => "P",
            0x14 => "Q", 0x15 => "R", 0x16 => "S", 0x17 => "T",
            0x18 => "U", 0x19 => "V", 0x1A => "W", 0x1B => "X",
            0x1C => "Y", 0x1D => "Z",
            0x1E => "1", 0x1F => "2", 0x20 => "3", 0x21 => "4",
            0x22 => "5", 0x23 => "6", 0x24 => "7", 0x25 => "8",
            0x26 => "9", 0x27 => "0",
            0x28 => "Enter", 0x29 => "Escape", 0x2A => "Backspace",
            0x2B => "Tab", 0x2C => "Space", 0x2D => "-", 0x2E => "=",
            0x2F => "[", 0x30 => "]", 0x31 => "\\", 0x32 => "#",
            0x33 => ";", 0x34 => "'", 0x35 => "`", 0x36 => ",",
            0x37 => ".", 0x38 => "/", 0x39 => "CapsLock",
            0x3A => "F1", 0x3B => "F2", 0x3C => "F3", 0x3D => "F4",
            0x3E => "F5", 0x3F => "F6", 0x40 => "F7", 0x41 => "F8",
            0x42 => "F9", 0x43 => "F10", 0x44 => "F11", 0x45 => "F12",
            0x46 => "PrintScr", 0x47 => "ScrollLock", 0x48 => "Pause",
            0x49 => "Insert", 0x4A => "Home", 0x4B => "PageUp",
            0x4C => "Delete", 0x4D => "End", 0x4E => "PageDown",
            0x4F => "Right", 0x50 => "Left", 0x51 => "Down", 0x52 => "Up",
            0x53 => "NumLock", 0x54 => "KP/", 0x55 => "KP*", 0x56 => "KP-",
            0x57 => "KP+", 0x58 => "KPEnter",
            0x59 => "KP1", 0x5A => "KP2", 0x5B => "KP3", 0x5C => "KP4",
            0x5D => "KP5", 0x5E => "KP6", 0x5F => "KP7", 0x60 => "KP8",
            0x61 => "KP9", 0x62 => "KP0", 0x63 => "KP.",
            0x64 => "NonUS\\", 0x65 => "App", 0x66 => "Power",
            0x67 => "KP=",
            0x68..=0x73 => "F13-F24",
            0xE0 => "LCtrl", 0xE1 => "LShift", 0xE2 => "LAlt", 0xE3 => "LGUI",
            0xE4 => "RCtrl", 0xE5 => "RShift", 0xE6 => "RAlt", 0xE7 => "RGUI",
            _ => "?",
        }
    }

    /// Convert a character to HID keycode
    /// Returns (keycode, needs_shift) or None if unsupported
    pub fn char_to_hid(ch: char) -> Option<(u8, bool)> {
        match ch {
            // Letters (a-z lowercase, A-Z needs shift)
            'a'..='z' => Some((0x04 + (ch as u8 - b'a'), false)),
            'A'..='Z' => Some((0x04 + (ch as u8 - b'A'), true)),
            // Numbers
            '1'..='9' => Some((0x1E + (ch as u8 - b'1'), false)),
            '0' => Some((0x27, false)),
            // Special characters (unshifted)
            ' ' => Some((0x2C, false)),  // Space
            '-' => Some((0x2D, false)),
            '=' => Some((0x2E, false)),
            '[' => Some((0x2F, false)),
            ']' => Some((0x30, false)),
            '\\' => Some((0x31, false)),
            ';' => Some((0x33, false)),
            '\'' => Some((0x34, false)),
            '`' => Some((0x35, false)),
            ',' => Some((0x36, false)),
            '.' => Some((0x37, false)),
            '/' => Some((0x38, false)),
            '\n' => Some((0x28, false)), // Enter
            '\t' => Some((0x2B, false)), // Tab
            // Shifted characters
            '!' => Some((0x1E, true)),  // Shift+1
            '@' => Some((0x1F, true)),  // Shift+2
            '#' => Some((0x20, true)),  // Shift+3
            '$' => Some((0x21, true)),  // Shift+4
            '%' => Some((0x22, true)),  // Shift+5
            '^' => Some((0x23, true)),  // Shift+6
            '&' => Some((0x24, true)),  // Shift+7
            '*' => Some((0x25, true)),  // Shift+8
            '(' => Some((0x26, true)),  // Shift+9
            ')' => Some((0x27, true)),  // Shift+0
            '_' => Some((0x2D, true)),  // Shift+-
            '+' => Some((0x2E, true)),  // Shift+=
            '{' => Some((0x2F, true)),  // Shift+[
            '}' => Some((0x30, true)),  // Shift+]
            '|' => Some((0x31, true)),  // Shift+\
            ':' => Some((0x33, true)),  // Shift+;
            '"' => Some((0x34, true)),  // Shift+'
            '~' => Some((0x35, true)),  // Shift+`
            '<' => Some((0x36, true)),  // Shift+,
            '>' => Some((0x37, true)),  // Shift+.
            '?' => Some((0x38, true)),  // Shift+/
            _ => None,
        }
    }
}

/// Firmware update protocol constants (DRY-RUN ONLY - no actual flashing)
/// These constants document the protocol but should NOT be used to send boot commands
pub mod firmware_update {
    /// Boot mode entry command for USB firmware (DANGEROUS - DO NOT SEND)
    /// Format: [0x7F, 0x55, 0xAA, 0x55, 0xAA] with Bit7 checksum
    pub const BOOT_ENTRY_USB: [u8; 5] = [0x7F, 0x55, 0xAA, 0x55, 0xAA];

    /// Boot mode entry command for RF firmware (DANGEROUS - DO NOT SEND)
    /// Format: [0xF8, 0x55, 0xAA, 0x55, 0xAA, 0x00, 0x00, 0x82] with Bit7 checksum
    pub const BOOT_ENTRY_RF: [u8; 8] = [0xF8, 0x55, 0xAA, 0x55, 0xAA, 0x00, 0x00, 0x82];

    /// Firmware transfer start marker
    pub const TRANSFER_START: [u8; 2] = [0xBA, 0xC0];

    /// Firmware transfer complete marker
    pub const TRANSFER_COMPLETE: [u8; 2] = [0xBA, 0xC2];

    /// Boot mode VID/PIDs - device uses these when in bootloader mode
    pub const BOOT_VID_PIDS: [(u16, u16); 4] = [
        (0x3141, 0x504A),  // USB boot mode 1
        (0x3141, 0x404A),  // USB boot mode 2
        (0x046A, 0x012E),  // RF boot mode 1
        (0x046A, 0x0130),  // RF boot mode 2
    ];

    /// Firmware data chunk size
    pub const CHUNK_SIZE: usize = 64;

    /// USB firmware offset in combined file
    pub const USB_FIRMWARE_OFFSET: usize = 20480;

    /// RF firmware offset in combined file
    pub const RF_FIRMWARE_OFFSET: usize = 65536;

    /// Delay after boot entry (ms)
    pub const BOOT_ENTRY_DELAY_MS: u64 = 1000;

    /// Delay after RF boot entry (ms)
    pub const RF_BOOT_ENTRY_DELAY_MS: u64 = 3000;

    /// Check if a VID/PID pair indicates boot mode
    pub fn is_boot_mode(vid: u16, pid: u16) -> bool {
        BOOT_VID_PIDS.contains(&(vid, pid))
    }

    /// Calculate firmware checksum (simple 32-bit sum of all bytes)
    pub fn calculate_checksum(data: &[u8]) -> u32 {
        data.iter().map(|&b| b as u32).sum()
    }

    /// Build transfer start command header
    /// Returns: [0xBA, 0xC0, chunk_count_lo, chunk_count_hi, size_lo, size_mid, size_hi]
    pub fn build_start_header(chunk_count: u16, size: u32) -> [u8; 7] {
        [
            TRANSFER_START[0],
            TRANSFER_START[1],
            (chunk_count & 0xFF) as u8,
            (chunk_count >> 8) as u8,
            (size & 0xFF) as u8,
            ((size >> 8) & 0xFF) as u8,
            ((size >> 16) & 0xFF) as u8,
        ]
    }

    /// Build transfer complete command header
    /// Returns bytes for: [0xBA, 0xC2, chunk_count_2bytes, checksum_4bytes, size_4bytes]
    pub fn build_complete_header(chunk_count: u16, checksum: u32, size: u32) -> Vec<u8> {
        vec![
            TRANSFER_COMPLETE[0],
            TRANSFER_COMPLETE[1],
            (chunk_count & 0xFF) as u8,
            (chunk_count >> 8) as u8,
            (checksum & 0xFF) as u8,
            ((checksum >> 8) & 0xFF) as u8,
            ((checksum >> 16) & 0xFF) as u8,
            ((checksum >> 24) & 0xFF) as u8,
            (size & 0xFF) as u8,
            ((size >> 8) & 0xFF) as u8,
            ((size >> 16) & 0xFF) as u8,
            ((size >> 24) & 0xFF) as u8,
        ]
    }
}

/// Audio visualizer protocol (command 0x0D)
/// Sends 16 frequency band levels to the keyboard's built-in audio reactive mode
pub mod audio_viz {
    /// Number of frequency bands
    pub const NUM_BANDS: usize = 16;
    /// Maximum value per band (0-6)
    pub const MAX_LEVEL: u8 = 6;
    /// Update rate in Hz
    pub const UPDATE_RATE_HZ: u32 = 50;
    /// Update interval in milliseconds
    pub const UPDATE_INTERVAL_MS: u64 = 20;

    /// Band frequency ranges (approximate)
    pub const BAND_BASS_START: usize = 0;      // Bands 0-3: Bass (20-250 Hz)
    pub const BAND_BASS_END: usize = 3;
    pub const BAND_LOWMID_START: usize = 4;    // Bands 4-7: Low-mid (250-1000 Hz)
    pub const BAND_LOWMID_END: usize = 7;
    pub const BAND_HIGHMID_START: usize = 8;   // Bands 8-11: High-mid (1-4 kHz)
    pub const BAND_HIGHMID_END: usize = 11;
    pub const BAND_TREBLE_START: usize = 12;   // Bands 12-15: Treble (4-20 kHz)
    pub const BAND_TREBLE_END: usize = 15;

    /// Build an audio visualizer HID report
    /// `bands` must be 16 values, each 0-6
    pub fn build_report(bands: &[u8; NUM_BANDS]) -> [u8; 64] {
        let mut buf = [0u8; 64];
        buf[0] = super::cmd::SET_AUDIO_VIZ;  // 0x0D
        // Bytes 1-6 are padding (zeros)
        // Byte 7 is checksum
        let sum: u32 = buf[0..7].iter().map(|&b| b as u32).sum();
        buf[7] = (255 - (sum & 0xFF)) as u8;
        // Bytes 8-23 are the 16 frequency bands
        for (i, &level) in bands.iter().enumerate() {
            buf[8 + i] = level.min(MAX_LEVEL);
        }
        buf
    }

    /// Convert FFT magnitudes to band levels (0-6)
    /// `magnitudes` should be normalized 0.0-1.0
    pub fn magnitudes_to_bands(magnitudes: &[f32]) -> [u8; NUM_BANDS] {
        let mut bands = [0u8; NUM_BANDS];
        let step = magnitudes.len() / NUM_BANDS;

        for (i, band) in bands.iter_mut().enumerate() {
            // Average magnitudes for this band
            let start = i * step;
            let end = (start + step).min(magnitudes.len());
            if start < end {
                let avg: f32 = magnitudes[start..end].iter().sum::<f32>() / (end - start) as f32;
                // Map 0.0-1.0 to 0-6
                *band = (avg * MAX_LEVEL as f32).round().min(MAX_LEVEL as f32) as u8;
            }
        }
        bands
    }
}

/// Screen color protocol (command 0x0E)
/// Streams average screen RGB color to the keyboard's built-in screen reactive mode (mode 21)
pub mod screen_color {
    /// Update rate in Hz
    pub const UPDATE_RATE_HZ: u32 = 50;
    /// Update interval in milliseconds
    pub const UPDATE_INTERVAL_MS: u64 = 20;

    /// Build a screen color HID report
    /// Sends RGB values to keyboard for mode 21 (Screen Color)
    pub fn build_report(r: u8, g: u8, b: u8) -> [u8; 64] {
        let mut buf = [0u8; 64];
        buf[0] = super::cmd::SET_SCREEN_COLOR;  // 0x0E
        buf[1] = r;
        buf[2] = g;
        buf[3] = b;
        // Bytes 4-6 are reserved (zeros)
        // Byte 7 is checksum (255 - sum of bytes 0-6)
        let sum: u32 = buf[0..7].iter().map(|&b| b as u32).sum();
        buf[7] = (255 - (sum & 0xFF)) as u8;
        buf
    }
}
