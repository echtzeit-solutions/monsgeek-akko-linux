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
    pub const SET_USERPIC: u8 = 0x0C; // Per-key RGB colors (static)
    pub const SET_AUDIO_VIZ: u8 = 0x0D; // Audio visualizer frequency bands (16 bands, values 0-6)
    pub const SET_SCREEN_COLOR: u8 = 0x0E; // Screen color RGB (streamed, for mode 21)
    pub const SET_USERGIF: u8 = 0x12; // Per-key RGB animation (dynamic)
    pub const SET_FN: u8 = 0x10;
    pub const SET_SLEEPTIME: u8 = 0x11;
    pub const SET_AUTOOS_EN: u8 = 0x17;
    pub const SET_LEDONOFF: u8 = 0x05; // LED master on/off
    pub const SET_MAGNETISM_REPORT: u8 = 0x1B;
    pub const SET_MAGNETISM_CAL: u8 = 0x1C;
    pub const SET_MAGNETISM_MAX_CAL: u8 = 0x1E;
    pub const SET_KEY_MAGNETISM_MODE: u8 = 0x1D;
    pub const SET_USERGIFSTART: u8 = 0x18; // Start animation upload
    pub const SET_MULTI_MAGNETISM: u8 = 0x65;

    // Undocumented/variant-specific SET commands (from firmware analysis)
    // These commands exist in firmware but may not be used on all devices
    /// OLED screen options (for keyboards with OLED displays)
    pub const SET_OLEDOPTION: u8 = 0x22;
    /// TFT LCD image data (for keyboards with color screens)
    pub const SET_TFTLCDDATA: u8 = 0x25;
    /// OLED language setting
    pub const SET_OLEDLANGUAGE: u8 = 0x27;
    /// OLED clock display
    pub const SET_OLEDCLOCK: u8 = 0x28;
    /// 24-bit color screen data
    pub const SET_SCREEN_24BITDATA: u8 = 0x29;
    /// OLED bootloader mode entry
    pub const SET_OLEDBOOTLOADER: u8 = 0x30;
    /// OLED boot start
    pub const SET_OLEDBOOTSTART: u8 = 0x31;
    /// TFT flash data
    pub const SET_TFTFLASHDATA: u8 = 0x32;
    /// Factory SKU setting (manufacturing only)
    pub const SET_SKU: u8 = 0x50;
    /// Factory reset - DANGEROUS: erases flash
    /// Format: [0x7F, 0x55, 0xAA, 0x55, 0xAA] with magic bytes
    pub const FACTORY_RESET: u8 = 0x7F;
    /// Flash chip erase - DANGEROUS
    pub const SET_FLASHCHIPERASSE: u8 = 0xAC;

    // Dongle-specific commands (from dongle firmware RE)
    /// Get dongle info: returns {0xF0, 1, 8, 0,0,0,0, fw_ver}
    pub const GET_DONGLE_INFO: u8 = 0xF0;
    /// Set control byte: stores data[0] → dongle_state.ctrl_byte
    pub const SET_CTRL_BYTE: u8 = 0xF6;
    /// Get dongle status (9 bytes): has_response, kb_battery_info, 0,
    /// kb_charging, 1, rf_ready, 1, pairing_mode, pairing_status.
    /// Handled locally by dongle — NOT forwarded to keyboard.
    pub const GET_DONGLE_STATUS: u8 = 0xF7;
    /// Enter pairing mode: requires 55AA55AA magic
    pub const ENTER_PAIRING: u8 = 0xF8;
    /// Pairing control: sends 3-byte SPI packet {cmd=1, data[0], data[1]}
    pub const PAIRING_CMD: u8 = 0x7A;
    /// Get cached keyboard response: copies 64B cached_kb_response into
    /// USB feature report buffer, clears has_response. Used as flush.
    pub const GET_CACHED_RESPONSE: u8 = 0xFC;
    /// Get dongle ID: returns {0xAA, 0x55, 0x01, 0x00}
    pub const GET_DONGLE_ID: u8 = 0xFD;
    /// FE: GET_CALIBRATION on keyboard, SET_RESPONSE_SIZE on dongle (same byte)
    pub const GET_CALIBRATION: u8 = 0xFE;

    // GET commands (0x80 - 0xE6)
    pub const GET_REV: u8 = 0x80; // Get firmware revision
    pub const GET_REPORT: u8 = 0x83; // Get report rate
    pub const GET_PROFILE: u8 = 0x84; // Get active profile
    pub const GET_DEBOUNCE: u8 = 0x86; // Get debounce settings
    pub const GET_LEDPARAM: u8 = 0x87; // Get LED parameters
    pub const GET_SLEDPARAM: u8 = 0x88; // Get secondary LED params
    pub const GET_KBOPTION: u8 = 0x89; // Get keyboard options
    pub const GET_USERPIC: u8 = 0x8C; // Get per-key RGB colors
    pub const GET_KEYMATRIX: u8 = 0x8A; // Get key mappings
    pub const GET_MACRO: u8 = 0x8B; // Get macros
    pub const GET_USB_VERSION: u8 = 0x8F; // Get USB version
    pub const GET_FN: u8 = 0x90; // Get Fn layer
    pub const GET_SLEEPTIME: u8 = 0x91; // Get sleep timeout
    pub const GET_AUTOOS_EN: u8 = 0x97; // Get auto-OS setting
    pub const GET_MAGNETISM_CAL: u8 = 0x9C; // Calibration data (min)
    pub const GET_KEY_MAGNETISM_MODE: u8 = 0x9D;
    pub const GET_MAGNETISM_CALMAX: u8 = 0x9E; // Calibration data (max)
    pub const GET_MULTI_MAGNETISM: u8 = 0xE5; // Get RT/DKS per-key settings
    pub const GET_FEATURE_LIST: u8 = 0xE6; // Get supported features

    // Undocumented/variant-specific GET commands (from firmware analysis)
    /// LED on/off state
    pub const GET_LEDONOFF: u8 = 0x85;
    /// TFT LCD data readback
    pub const GET_TFTLCDDATA: u8 = 0xA5;
    /// 24-bit screen data readback
    pub const GET_SCREEN_24BITDATA: u8 = 0xA9;
    /// OLED firmware version
    pub const GET_OLED_VERSION: u8 = 0xAD;
    /// Matrix LED controller version
    pub const GET_MLED_VERSION: u8 = 0xAE;
    /// OLED bootloader state
    pub const GET_OLEDBOOTLOADER: u8 = 0xB0;
    /// OLED firmware checksum
    pub const GET_OLEDBOOTCHECKSUM: u8 = 0xB1;
    /// TFT flash data readback
    pub const GET_TFTFLASHDATA: u8 = 0xB2;
    /// Factory SKU readback
    pub const GET_SKU: u8 = 0xD0;

    // Response status
    pub const STATUS_SUCCESS: u8 = 0xAA;

    /// LED effect mode names (from Akko Cloud LightList)
    /// Music modes use command 0x0D for audio data, Screen Sync uses 0x0E for RGB
    pub const LED_MODES: &[&str] = &[
        "Off",            // 0  - LEDs off
        "Constant",       // 1  - Static color
        "Breathing",      // 2  - Breathing/pulsing effect
        "Neon",           // 3  - Neon glow
        "Wave",           // 4  - Color wave
        "Ripple",         // 5  - Ripple from keypress
        "Raindrop",       // 6  - Raindrops falling
        "Snake",          // 7  - Snake pattern
        "Reactive",       // 8  - React to keypress (keep lit)
        "Converge",       // 9  - Converging pattern
        "Sine Wave",      // 10 - Sine wave pattern
        "Kaleidoscope",   // 11 - Kaleidoscope effect
        "Line Wave",      // 12 - Line wave pattern
        "User Picture",   // 13 - Custom per-key colors (4 layers)
        "Laser",          // 14 - Laser effect
        "Circle Wave",    // 15 - Circular wave
        "Rainbow",        // 16 - Rainbow/dazzle effect
        "Rain Down",      // 17 - Rain downward
        "Meteor",         // 18 - Meteor shower
        "Reactive Off",   // 19 - React to keypress (brief flash)
        "Music Patterns", // 20 - LightMusicFollow3: audio reactive with preset patterns (1-5)
        "Screen Sync",    // 21 - LightScreenColor: ambient RGB from 0x0E command
        "Music Bars", // 22 - LightMusicFollow2: audio reactive bars (upright/separate/intersect)
        "Train",      // 23 - Train pattern
        "Fireworks",  // 24 - Fireworks effect
        "Per-Key Color", // 25 - Dynamic per-key animation (GIF)
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
        UserPicture = 13, // Static per-key colors (4 layers)
        Laser = 14,
        CircleWave = 15,
        Rainbow = 16,
        RainDown = 17,
        Meteor = 18,
        ReactiveOff = 19,
        /// Music reactive mode with preset patterns (1-5)
        /// Use SET_AUDIO_VIZ (0x0D) to send frequency band data
        MusicPatterns = 20,
        /// Screen color sync mode - receives RGB via SET_SCREEN_COLOR (0x0E)
        ScreenSync = 21,
        /// Music reactive mode with bar visualization (upright/separate/intersect)
        /// Use SET_AUDIO_VIZ (0x0D) to send frequency band data
        MusicBars = 22,
        Train = 23,
        Fireworks = 24,
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
                20 => Some(Self::MusicPatterns),
                21 => Some(Self::ScreenSync),
                22 => Some(Self::MusicBars),
                23 => Some(Self::Train),
                24 => Some(Self::Fireworks),
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
                "musicpatterns" | "music3" | "patterns" => Some(Self::MusicPatterns),
                "screensync" | "screencolor" | "screen" | "ambient" => Some(Self::ScreenSync),
                "musicbars" | "music2" | "bars" | "music" => Some(Self::MusicBars),
                "train" => Some(Self::Train),
                "fireworks" => Some(Self::Fireworks),
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
            LED_MODES
                .iter()
                .enumerate()
                .map(|(i, name)| (i as u8, *name))
        }
    }

    impl std::fmt::Display for LedMode {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.name())
        }
    }

    // Keep constants for backward compatibility
    pub const LED_MODE_USER_PICTURE: u8 = LedMode::UserPicture as u8;

    pub fn name(cmd: u8) -> &'static str {
        match cmd {
            SET_RESET => "SET_RESET",
            SET_REPORT => "SET_REPORT",
            SET_PROFILE => "SET_PROFILE",
            SET_LEDONOFF => "SET_LEDONOFF",
            SET_DEBOUNCE => "SET_DEBOUNCE",
            SET_LEDPARAM => "SET_LEDPARAM",
            SET_SLEDPARAM => "SET_SLEDPARAM",
            SET_KBOPTION => "SET_KBOPTION",
            SET_KEYMATRIX => "SET_KEYMATRIX",
            SET_MACRO => "SET_MACRO",
            SET_USERPIC => "SET_USERPIC",
            SET_AUDIO_VIZ => "SET_AUDIO_VIZ",
            SET_SCREEN_COLOR => "SET_SCREEN_COLOR",
            SET_FN => "SET_FN",
            SET_SLEEPTIME => "SET_SLEEPTIME",
            SET_USERGIF => "SET_USERGIF",
            SET_AUTOOS_EN => "SET_AUTOOS_EN",
            SET_USERGIFSTART => "SET_USERGIFSTART",
            SET_MAGNETISM_REPORT => "SET_MAGNETISM_REPORT",
            SET_MAGNETISM_CAL => "SET_MAGNETISM_CAL",
            SET_MAGNETISM_MAX_CAL => "SET_MAGNETISM_MAX_CAL",
            SET_KEY_MAGNETISM_MODE => "SET_KEY_MAGNETISM_MODE",
            SET_MULTI_MAGNETISM => "SET_MULTI_MAGNETISM",
            // Undocumented SET commands
            SET_OLEDOPTION => "SET_OLEDOPTION",
            SET_TFTLCDDATA => "SET_TFTLCDDATA",
            SET_OLEDLANGUAGE => "SET_OLEDLANGUAGE",
            SET_OLEDCLOCK => "SET_OLEDCLOCK",
            SET_SCREEN_24BITDATA => "SET_SCREEN_24BITDATA",
            SET_OLEDBOOTLOADER => "SET_OLEDBOOTLOADER",
            SET_OLEDBOOTSTART => "SET_OLEDBOOTSTART",
            SET_TFTFLASHDATA => "SET_TFTFLASHDATA",
            SET_SKU => "SET_SKU",
            FACTORY_RESET => "FACTORY_RESET",
            SET_FLASHCHIPERASSE => "SET_FLASHCHIPERASSE",
            // GET commands
            GET_REV => "GET_REV",
            GET_REPORT => "GET_REPORT",
            GET_PROFILE => "GET_PROFILE",
            GET_LEDONOFF => "GET_LEDONOFF",
            GET_DEBOUNCE => "GET_DEBOUNCE",
            GET_LEDPARAM => "GET_LEDPARAM",
            GET_SLEDPARAM => "GET_SLEDPARAM",
            GET_KBOPTION => "GET_KBOPTION",
            GET_KEYMATRIX => "GET_KEYMATRIX",
            GET_MACRO => "GET_MACRO",
            GET_USERPIC => "GET_USERPIC",
            GET_USB_VERSION => "GET_USB_VERSION",
            GET_FN => "GET_FN",
            GET_SLEEPTIME => "GET_SLEEPTIME",
            GET_AUTOOS_EN => "GET_AUTOOS_EN",
            GET_MAGNETISM_CAL => "GET_MAGNETISM_CAL",
            GET_KEY_MAGNETISM_MODE => "GET_KEY_MAGNETISM_MODE",
            GET_MAGNETISM_CALMAX => "GET_MAGNETISM_CALMAX",
            GET_TFTLCDDATA => "GET_TFTLCDDATA",
            GET_SCREEN_24BITDATA => "GET_SCREEN_24BITDATA",
            GET_OLED_VERSION => "GET_OLED_VERSION",
            GET_MLED_VERSION => "GET_MLED_VERSION",
            GET_OLEDBOOTLOADER => "GET_OLEDBOOTLOADER",
            GET_OLEDBOOTCHECKSUM => "GET_OLEDBOOTCHECKSUM",
            GET_TFTFLASHDATA => "GET_TFTFLASHDATA",
            GET_SKU => "GET_SKU",
            GET_MULTI_MAGNETISM => "GET_MULTI_MAGNETISM",
            GET_FEATURE_LIST => "GET_FEATURE_LIST",
            // Dongle commands
            GET_DONGLE_INFO => "GET_DONGLE_INFO",
            SET_CTRL_BYTE => "SET_CTRL_BYTE",
            GET_DONGLE_STATUS => "GET_DONGLE_STATUS",
            ENTER_PAIRING => "ENTER_PAIRING",
            PAIRING_CMD => "PAIRING_CMD",
            GET_CACHED_RESPONSE => "GET_CACHED_RESPONSE",
            GET_DONGLE_ID => "GET_DONGLE_ID",
            GET_CALIBRATION => "GET_CALIBRATION",
            _ => "UNKNOWN",
        }
    }
}

/// Checksum types used by the protocol
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChecksumType {
    Bit7, // Checksum at byte 7 (most common)
    Bit8, // Checksum at byte 8 (for LED commands)
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

/// Build a complete HID command buffer (65 bytes with report ID)
///
/// # Arguments
/// * `cmd` - Command byte (e.g., `cmd::GET_USB_VERSION`)
/// * `data` - Additional data bytes after command (can be empty)
/// * `checksum_type` - Checksum type to apply
///
/// # Returns
/// 65-byte buffer: [report_id=0, cmd, data..., checksum, zeros...]
pub fn build_command(cmd: u8, data: &[u8], checksum_type: ChecksumType) -> Vec<u8> {
    let mut buf = vec![0u8; REPORT_SIZE];
    buf[0] = 0; // Report ID
    buf[1] = cmd;
    let len = std::cmp::min(data.len(), REPORT_SIZE - 2);
    if len > 0 {
        buf[2..2 + len].copy_from_slice(&data[..len]);
    }
    apply_checksum(&mut buf[1..], checksum_type);
    buf
}

// Device constants (VID, PID, USAGE, etc.) are now in hal::constants
// Use hal::VENDOR_ID, hal::PRODUCT_ID_*, etc.

/// HID report sizes
pub const REPORT_SIZE: usize = 65; // Feature report size (with report ID)
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
    use crate::hal::constants::MATRIX_SIZE_M1_V5;

    /// Total RGB data size (MATRIX_SIZE * 3 bytes)
    pub const TOTAL_RGB_SIZE: usize = MATRIX_SIZE_M1_V5 * 3;
    /// Number of pages per frame
    pub const NUM_PAGES: usize = 7;
    /// RGB data per full page
    pub const PAGE_SIZE: usize = 56;
    /// RGB data in last page
    pub const LAST_PAGE_SIZE: usize = 42;
    /// LED matrix positions (keys)
    pub const MATRIX_SIZE: usize = MATRIX_SIZE_M1_V5;
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
pub const LED_DAZZLE_OFF: u8 = 8;
pub const LED_DAZZLE_ON: u8 = 7;
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
        if RATES.contains(&hz) {
            Some(hz)
        } else {
            None
        }
    }
}

/// Magnetism (key depth) report parsing
/// Report format: [report_id(0x05), cmd(0x1B), depth_lo, depth_hi, key_index, 0, 0, 0, ...]
pub mod depth_report {
    use super::cmd;
    use monsgeek_transport::event_parser::report_id::USB_VENDOR_EVENT as REPORT_ID;

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
            let (depth_lo, depth_hi, key_idx) =
                if buf[0] == REPORT_ID && buf[1] == cmd::SET_MAGNETISM_REPORT {
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
            0x04 => "A",
            0x05 => "B",
            0x06 => "C",
            0x07 => "D",
            0x08 => "E",
            0x09 => "F",
            0x0A => "G",
            0x0B => "H",
            0x0C => "I",
            0x0D => "J",
            0x0E => "K",
            0x0F => "L",
            0x10 => "M",
            0x11 => "N",
            0x12 => "O",
            0x13 => "P",
            0x14 => "Q",
            0x15 => "R",
            0x16 => "S",
            0x17 => "T",
            0x18 => "U",
            0x19 => "V",
            0x1A => "W",
            0x1B => "X",
            0x1C => "Y",
            0x1D => "Z",
            0x1E => "1",
            0x1F => "2",
            0x20 => "3",
            0x21 => "4",
            0x22 => "5",
            0x23 => "6",
            0x24 => "7",
            0x25 => "8",
            0x26 => "9",
            0x27 => "0",
            0x28 => "Enter",
            0x29 => "Escape",
            0x2A => "Backspace",
            0x2B => "Tab",
            0x2C => "Space",
            0x2D => "-",
            0x2E => "=",
            0x2F => "[",
            0x30 => "]",
            0x31 => "\\",
            0x32 => "#",
            0x33 => ";",
            0x34 => "'",
            0x35 => "`",
            0x36 => ",",
            0x37 => ".",
            0x38 => "/",
            0x39 => "CapsLock",
            0x3A => "F1",
            0x3B => "F2",
            0x3C => "F3",
            0x3D => "F4",
            0x3E => "F5",
            0x3F => "F6",
            0x40 => "F7",
            0x41 => "F8",
            0x42 => "F9",
            0x43 => "F10",
            0x44 => "F11",
            0x45 => "F12",
            0x46 => "PrintScr",
            0x47 => "ScrollLock",
            0x48 => "Pause",
            0x49 => "Insert",
            0x4A => "Home",
            0x4B => "PageUp",
            0x4C => "Delete",
            0x4D => "End",
            0x4E => "PageDown",
            0x4F => "Right",
            0x50 => "Left",
            0x51 => "Down",
            0x52 => "Up",
            0x53 => "NumLock",
            0x54 => "KP/",
            0x55 => "KP*",
            0x56 => "KP-",
            0x57 => "KP+",
            0x58 => "KPEnter",
            0x59 => "KP1",
            0x5A => "KP2",
            0x5B => "KP3",
            0x5C => "KP4",
            0x5D => "KP5",
            0x5E => "KP6",
            0x5F => "KP7",
            0x60 => "KP8",
            0x61 => "KP9",
            0x62 => "KP0",
            0x63 => "KP.",
            0x64 => "NonUS\\",
            0x65 => "App",
            0x66 => "Power",
            0x67 => "KP=",
            0x68..=0x73 => "F13-F24",
            0xE0 => "LCtrl",
            0xE1 => "LShift",
            0xE2 => "LAlt",
            0xE3 => "LGUI",
            0xE4 => "RCtrl",
            0xE5 => "RShift",
            0xE6 => "RAlt",
            0xE7 => "RGUI",
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
            ' ' => Some((0x2C, false)), // Space
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
            '!' => Some((0x1E, true)), // Shift+1
            '@' => Some((0x1F, true)), // Shift+2
            '#' => Some((0x20, true)), // Shift+3
            '$' => Some((0x21, true)), // Shift+4
            '%' => Some((0x22, true)), // Shift+5
            '^' => Some((0x23, true)), // Shift+6
            '&' => Some((0x24, true)), // Shift+7
            '*' => Some((0x25, true)), // Shift+8
            '(' => Some((0x26, true)), // Shift+9
            ')' => Some((0x27, true)), // Shift+0
            '_' => Some((0x2D, true)), // Shift+-
            '+' => Some((0x2E, true)), // Shift+=
            '{' => Some((0x2F, true)), // Shift+[
            '}' => Some((0x30, true)), // Shift+]
            '|' => Some((0x31, true)), // Shift+\
            ':' => Some((0x33, true)), // Shift+;
            '"' => Some((0x34, true)), // Shift+'
            '~' => Some((0x35, true)), // Shift+`
            '<' => Some((0x36, true)), // Shift+,
            '>' => Some((0x37, true)), // Shift+.
            '?' => Some((0x38, true)), // Shift+/
            _ => None,
        }
    }

    /// Look up HID keycode from a key name (case-insensitive).
    ///
    /// Accepts both canonical names from `key_name()` (e.g. "Escape", "LCtrl")
    /// and common aliases (e.g. "Esc", "LShf", "Del", "Win").
    pub fn key_code_from_name(name: &str) -> Option<u8> {
        let name_lower = name.to_ascii_lowercase();

        // F13-F24: key_name() returns "F13-F24" for the whole range,
        // so we need individual matching here.
        if let Some(rest) = name_lower.strip_prefix('f') {
            if let Ok(n) = rest.parse::<u8>() {
                if (13..=24).contains(&n) {
                    return Some(0x68 + (n - 13));
                }
            }
        }

        // Try exact match against key_name() for all valid HID codes.
        for code in (0x00..=0x00).chain(0x04..=0x67).chain(0xE0..=0xE7u8) {
            let kn = key_name(code);
            if kn != "?" && kn.to_ascii_lowercase() == name_lower {
                return Some(code);
            }
        }

        // Common aliases (matrix key names, abbreviations, etc.)
        match name_lower.as_str() {
            "esc" => Some(0x29),
            "return" | "ret" => Some(0x28),
            "del" => Some(0x4C),
            "ins" => Some(0x49),
            "bksp" | "bs" => Some(0x2A),
            "caps" => Some(0x39),
            "lshf" => Some(0xE1),
            "rshf" => Some(0xE5),
            "lctl" => Some(0xE0),
            "rctl" => Some(0xE4),
            "win" | "lwin" | "super" | "lsuper" | "cmd" | "lcmd" => Some(0xE3),
            "rwin" | "rsuper" | "rcmd" => Some(0xE7),
            "printscreen" | "prtsc" => Some(0x46),
            "scrlk" => Some(0x47),
            "numlk" => Some(0x53),
            "menu" | "application" => Some(0x65),
            "spc" => Some(0x2C),
            "pgup" => Some(0x4B),
            "pgdn" | "pgdown" => Some(0x4E),
            "nonusbs" | "intlbs" => Some(0x64),
            "ent" => Some(0x28),
            "intlro" => Some(0x87),
            _ => None,
        }
    }
}

/// Firmware patch discovery protocol (command 0xFB)
///
/// Used to detect whether the keyboard is running patched firmware and
/// what capabilities are available.
pub mod patch_info {
    /// Patch info query command
    pub const CMD: u8 = 0xFB;
    /// Magic high byte in response
    pub const MAGIC_HI: u8 = 0xCA;
    /// Magic low byte in response
    pub const MAGIC_LO: u8 = 0xFE;
    /// Capability: HID battery reporting
    pub const CAP_BATTERY: u16 = 1 << 0;
    /// Capability: LED streaming (0xFC)
    pub const CAP_LED_STREAM: u16 = 1 << 1;

    pub fn capability_names(caps: u16) -> Vec<&'static str> {
        let mut names = Vec::new();
        if caps & CAP_BATTERY != 0 {
            names.push("battery");
        }
        if caps & CAP_LED_STREAM != 0 {
            names.push("led_stream");
        }
        names
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

    /// Our keyboard's bootloader PID (VID stays 0x3151)
    pub const BOOT_PID_M1_V5: u16 = 0x502A;

    /// Dongle bootloader PID (VID stays 0x3151)
    pub const BOOT_PID_DONGLE: u16 = 0x5039;

    /// Bootloader usage page
    pub const BOOT_USAGE_PAGE: u16 = 0xFF01;

    /// Normal-mode vendor usage page
    pub const NORMAL_USAGE_PAGE: u16 = 0xFFFF;

    /// Keyboard normal-mode PID
    pub const NORMAL_PID: u16 = 0x5030;

    /// Dongle normal-mode PID
    pub const DONGLE_NORMAL_PID: u16 = 0x5038;

    /// Normal-mode vendor config usage (IF2)
    pub const NORMAL_USAGE: u16 = 0x02;

    /// Dongle vendor usage (IF2, usage_page=0xFFFF, usage=0x01)
    pub const DONGLE_NORMAL_USAGE: u16 = 0x01;

    /// Common VID for MonsGeek M1 V5
    pub const VID: u16 = 0x3151;

    /// Keyboard boot mode VID/PIDs
    pub const KB_BOOT_VID_PIDS: [(u16, u16); 3] = [
        (0x3151, 0x502A), // MonsGeek M1 V5 TMR bootloader
        (0x3141, 0x504A), // USB boot mode 1 (generic RY)
        (0x3141, 0x404A), // USB boot mode 2 (generic RY)
    ];

    /// Dongle boot mode VID/PIDs
    pub const DONGLE_BOOT_VID_PIDS: [(u16, u16); 1] = [
        (0x3151, 0x5039), // MonsGeek dongle bootloader
    ];

    /// Boot mode VID/PIDs - all devices (keyboard + dongle + RF)
    pub const BOOT_VID_PIDS: [(u16, u16); 6] = [
        (0x3151, 0x502A), // MonsGeek M1 V5 TMR bootloader
        (0x3151, 0x5039), // MonsGeek dongle bootloader
        (0x3141, 0x504A), // USB boot mode 1 (generic RY)
        (0x3141, 0x404A), // USB boot mode 2 (generic RY)
        (0x046A, 0x012E), // RF boot mode 1
        (0x046A, 0x0130), // RF boot mode 2
    ];

    /// Chip ID strings for safety validation (first 16 bytes at 0x08005000)
    pub const CHIP_ID_KEYBOARD: &[u8] = b"AT32F405 8KMKB  ";
    pub const CHIP_ID_DONGLE: &[u8] = b"AT32F405 8K-DGKB";

    /// Which device we're targeting for flash operations.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FlashTarget {
        Keyboard,
        Dongle,
    }

    impl FlashTarget {
        pub fn normal_pid(self) -> u16 {
            match self {
                Self::Keyboard => NORMAL_PID,
                Self::Dongle => DONGLE_NORMAL_PID,
            }
        }

        pub fn normal_usage(self) -> u16 {
            match self {
                Self::Keyboard => NORMAL_USAGE,
                Self::Dongle => DONGLE_NORMAL_USAGE,
            }
        }

        pub fn boot_vid_pids(self) -> &'static [(u16, u16)] {
            match self {
                Self::Keyboard => &KB_BOOT_VID_PIDS,
                Self::Dongle => &DONGLE_BOOT_VID_PIDS,
            }
        }

        pub fn chip_id(self) -> &'static [u8] {
            match self {
                Self::Keyboard => CHIP_ID_KEYBOARD,
                Self::Dongle => CHIP_ID_DONGLE,
            }
        }

        pub fn name(self) -> &'static str {
            match self {
                Self::Keyboard => "keyboard",
                Self::Dongle => "dongle",
            }
        }
    }

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

    /// Calculate firmware checksum matching the bootloader's algorithm.
    ///
    /// The bootloader checksums every byte of every 64-byte chunk, including
    /// 0xFF padding in the last chunk. We must match this exactly.
    pub fn calculate_checksum(data: &[u8]) -> u32 {
        let mut sum: u32 = data.iter().map(|&b| b as u32).sum();
        let remainder = data.len() % CHUNK_SIZE;
        if remainder != 0 {
            // Add the 0xFF padding bytes that the bootloader will also checksum
            sum += (CHUNK_SIZE - remainder) as u32 * 0xFF;
        }
        sum
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

/// Music mode visualization options
/// Used with SET_LEDPARAM to configure the visualization style
pub mod music_viz {
    /// Visualization style for MusicBars mode (22 / LightMusicFollow2)
    /// The option value is stored in the upper nibble of the option byte
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u8)]
    pub enum BarsStyle {
        /// Vertical bars rising from bottom (竖直)
        Upright = 0,
        /// Separated/split frequency bands (分离)
        Separate = 1,
        /// Horizontal crossing pattern (横断)
        Intersect = 2,
    }

    impl BarsStyle {
        pub fn from_u8(value: u8) -> Option<Self> {
            match value {
                0 => Some(Self::Upright),
                1 => Some(Self::Separate),
                2 => Some(Self::Intersect),
                _ => None,
            }
        }

        pub fn name(&self) -> &'static str {
            match self {
                Self::Upright => "Upright",
                Self::Separate => "Separate",
                Self::Intersect => "Intersect",
            }
        }
    }

    /// Pattern selection for MusicPatterns mode (20 / LightMusicFollow3)
    /// The option value is stored in the upper nibble of the option byte
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u8)]
    pub enum PatternsStyle {
        Pattern1 = 0,
        Pattern2 = 1,
        Pattern3 = 2,
        Pattern4 = 3,
        Pattern5 = 4,
    }

    impl PatternsStyle {
        pub fn from_u8(value: u8) -> Option<Self> {
            match value {
                0 => Some(Self::Pattern1),
                1 => Some(Self::Pattern2),
                2 => Some(Self::Pattern3),
                3 => Some(Self::Pattern4),
                4 => Some(Self::Pattern5),
                _ => None,
            }
        }

        pub fn name(&self) -> &'static str {
            match self {
                Self::Pattern1 => "Pattern 1",
                Self::Pattern2 => "Pattern 2",
                Self::Pattern3 => "Pattern 3",
                Self::Pattern4 => "Pattern 4",
                Self::Pattern5 => "Pattern 5",
            }
        }
    }

    /// Encode music mode option byte
    /// option = (style << 4) | dazzle_flag
    pub fn encode_option(style: u8, dazzle: bool) -> u8 {
        let dazzle_flag = if dazzle {
            super::LED_DAZZLE_ON
        } else {
            super::LED_DAZZLE_OFF
        };
        (style << 4) | dazzle_flag
    }

    /// Decode music mode option byte
    /// Returns (style, dazzle)
    pub fn decode_option(option: u8) -> (u8, bool) {
        let style = option >> 4;
        let dazzle = (option & super::LED_OPTIONS_MASK) == super::LED_DAZZLE_ON;
        (style, dazzle)
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
    pub const BAND_BASS_START: usize = 0; // Bands 0-3: Bass (20-250 Hz)
    pub const BAND_BASS_END: usize = 3;
    pub const BAND_LOWMID_START: usize = 4; // Bands 4-7: Low-mid (250-1000 Hz)
    pub const BAND_LOWMID_END: usize = 7;
    pub const BAND_HIGHMID_START: usize = 8; // Bands 8-11: High-mid (1-4 kHz)
    pub const BAND_HIGHMID_END: usize = 11;
    pub const BAND_TREBLE_START: usize = 12; // Bands 12-15: Treble (4-20 kHz)
    pub const BAND_TREBLE_END: usize = 15;

    /// Build an audio visualizer HID report
    /// `bands` must be 16 values, each 0-6
    pub fn build_report(bands: &[u8; NUM_BANDS]) -> [u8; 64] {
        let mut buf = [0u8; 64];
        buf[0] = super::cmd::SET_AUDIO_VIZ; // 0x0D
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

/// Keyboard-initiated events (INT-IN endpoint / EP2)
/// These are notifications sent by the keyboard, not responses to commands
/// All events use Report ID 0x05: [0x05, event_type, value1, value2, ...]
pub mod events {
    use monsgeek_transport::event_parser::report_id::USB_VENDOR_EVENT as REPORT_ID;

    // Event types (byte 1 after report ID)

    /// Wake from sleep - all zeros after report ID
    pub const WAKE: u8 = 0x00;

    /// Profile changed (via Fn+F9..F12)
    /// Format: [0x05, 0x01, profile, ...]
    pub const PROFILE_CHANGE: u8 = 0x01;

    /// Keyboard function events (Win lock, WASD swap, Fn layer, backlight, dial mode)
    /// Format: [0x05, 0x03, state, sub_type, ...]
    pub const KB_FUNC: u8 = 0x03;

    /// Main LED effect mode changed (via Fn+Home/PgUp/End/PgDn)
    /// Format: [0x05, 0x04, mode, ...]
    pub const LED_EFFECT_MODE: u8 = 0x04;

    /// Main LED speed changed (via Fn+←/→)
    /// Format: [0x05, 0x05, speed, ...]
    pub const LED_EFFECT_SPEED: u8 = 0x05;

    /// Main LED brightness changed (via Fn+↑/↓ or dial)
    /// Format: [0x05, 0x06, level, ...]
    pub const LED_BRIGHTNESS: u8 = 0x06;

    /// Main LED color changed (via Fn+\)
    /// Format: [0x05, 0x07, color, ...]
    pub const LED_COLOR: u8 = 0x07;

    /// Side LED effect mode changed
    /// Format: [0x05, 0x08, mode, ...]
    pub const SIDE_LED_MODE: u8 = 0x08;

    /// Side LED speed changed
    /// Format: [0x05, 0x09, speed, ...]
    pub const SIDE_LED_SPEED: u8 = 0x09;

    /// Side LED brightness changed
    /// Format: [0x05, 0x0A, level, ...]
    pub const SIDE_LED_BRIGHTNESS: u8 = 0x0A;

    /// Side LED color changed
    /// Format: [0x05, 0x0B, color, ...]
    pub const SIDE_LED_COLOR: u8 = 0x0B;

    /// Factory reset triggered (via Fn+~)
    /// Format: [0x05, 0x0D, 0x00, ...]
    /// Followed by SETTINGS_ACK (0x0F 0x01) then (0x0F 0x00) when complete
    pub const RESET_TRIGGERED: u8 = 0x0D;

    /// Settings acknowledgment event
    /// Sent when keyboard settings change (via Fn keys or commands)
    /// Format: [0x05, 0x0F, status, ...]
    /// status: 0x01 = change in progress, 0x00 = change complete
    pub const SETTINGS_ACK: u8 = 0x0F;

    /// Sleep mode change
    /// Format: [0x05, 0x13, state, ...]
    pub const SLEEP_MODE_CHANGE: u8 = 0x13;

    /// Key depth report (when magnetism monitoring enabled)
    /// Format: [0x05, 0x1B, depth_lo, depth_hi, key_index, ...]
    pub const KEY_DEPTH: u8 = 0x1B;

    /// Magnetic mode changed (per-key mode)
    /// Format: [0x05, 0x1D, mode, ...]
    pub const MAGNETIC_MODE_CHANGE: u8 = 0x1D;

    /// Screen clear complete (OLED/TFT)
    /// Format: [0x05, 0x2C, 0x00, ...]
    pub const SCREEN_CLEAR_DONE: u8 = 0x2C;

    /// Battery status (via dongle EP2 after F7)
    /// Format: [0x05, 0x88, 0x00, 0x00, level, flags, ...]
    pub const BATTERY_STATUS: u8 = 0x88;

    // KB_FUNC sub-types (byte 3)

    /// Win key lock toggle - state in byte 2 (0=unlocked, 1=locked)
    pub const KB_FUNC_WIN_LOCK: u8 = 0x01;

    /// WASD/Arrow swap toggle - state in byte 2 (0=normal, 8=swapped)
    pub const KB_FUNC_WASD_SWAP: u8 = 0x03;

    /// Fn layer toggle - layer in byte 2 (0=default, 1=alternate)
    pub const KB_FUNC_FN_LAYER: u8 = 0x08;

    /// Backlight toggle - byte 2=0x04
    pub const KB_FUNC_BACKLIGHT: u8 = 0x09;

    /// Dial mode toggle (volume ↔ brightness) - byte 2=0x00
    pub const KB_FUNC_DIAL_MODE: u8 = 0x11;

    // Settings ack status values

    /// Settings save started
    pub const SETTINGS_ACK_START: u8 = 0x01;
    /// Settings save complete
    pub const SETTINGS_ACK_DONE: u8 = 0x00;

    // Battery flags (byte 5 of BATTERY_STATUS)

    /// Keyboard is online/connected
    pub const BATTERY_FLAG_ONLINE: u8 = 0x01;
    /// Keyboard is charging
    pub const BATTERY_FLAG_CHARGING: u8 = 0x02;

    /// Parsed event with typed payload
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Event {
        Wake,
        ProfileChange {
            profile: u8,
        },
        WinLockToggle {
            locked: bool,
        },
        WasdSwapToggle {
            swapped: bool,
        },
        FnLayerToggle {
            layer: u8,
        },
        BacklightToggle,
        DialModeToggle,
        LedEffectMode {
            mode: u8,
        },
        LedEffectSpeed {
            speed: u8,
        },
        LedBrightness {
            level: u8,
        },
        LedColor {
            color: u8,
        },
        SideLedMode {
            mode: u8,
        },
        SideLedSpeed {
            speed: u8,
        },
        SideLedBrightness {
            level: u8,
        },
        SideLedColor {
            color: u8,
        },
        ResetTriggered,
        SettingsAck {
            started: bool,
        },
        SleepModeChange {
            state: u8,
        },
        KeyDepth {
            depth: u16,
            key_index: u8,
        },
        MagneticModeChange {
            mode: u8,
        },
        ScreenClearDone,
        BatteryStatus {
            level: u8,
            online: bool,
            charging: bool,
        },
        Unknown {
            event_type: u8,
            data: [u8; 4],
        },
    }

    /// Parse an event from raw HID buffer
    /// Returns parsed Event or None if buffer is too short
    pub fn parse(buf: &[u8]) -> Option<Event> {
        if buf.len() < 2 {
            return None;
        }

        // Check report ID
        if buf[0] != REPORT_ID {
            return None;
        }

        let event_type = buf[1];
        let b2 = buf.get(2).copied().unwrap_or(0);
        let b3 = buf.get(3).copied().unwrap_or(0);
        let b4 = buf.get(4).copied().unwrap_or(0);
        let b5 = buf.get(5).copied().unwrap_or(0);

        Some(match event_type {
            WAKE if b2 == 0 && b3 == 0 && b4 == 0 => Event::Wake,
            PROFILE_CHANGE => Event::ProfileChange { profile: b2 },
            KB_FUNC => match b3 {
                KB_FUNC_WIN_LOCK => Event::WinLockToggle { locked: b2 != 0 },
                KB_FUNC_WASD_SWAP => Event::WasdSwapToggle { swapped: b2 == 8 },
                KB_FUNC_FN_LAYER => Event::FnLayerToggle { layer: b2 },
                KB_FUNC_BACKLIGHT => Event::BacklightToggle,
                KB_FUNC_DIAL_MODE => Event::DialModeToggle,
                _ => Event::Unknown {
                    event_type,
                    data: [b2, b3, b4, b5],
                },
            },
            LED_EFFECT_MODE => Event::LedEffectMode { mode: b2 },
            LED_EFFECT_SPEED => Event::LedEffectSpeed { speed: b2 },
            LED_BRIGHTNESS => Event::LedBrightness { level: b2 },
            LED_COLOR => Event::LedColor { color: b2 },
            SIDE_LED_MODE => Event::SideLedMode { mode: b2 },
            SIDE_LED_SPEED => Event::SideLedSpeed { speed: b2 },
            SIDE_LED_BRIGHTNESS => Event::SideLedBrightness { level: b2 },
            SIDE_LED_COLOR => Event::SideLedColor { color: b2 },
            RESET_TRIGGERED => Event::ResetTriggered,
            SETTINGS_ACK => Event::SettingsAck {
                started: b2 == SETTINGS_ACK_START,
            },
            SLEEP_MODE_CHANGE => Event::SleepModeChange { state: b2 },
            KEY_DEPTH => Event::KeyDepth {
                depth: (b2 as u16) | ((b3 as u16) << 8),
                key_index: b4,
            },
            MAGNETIC_MODE_CHANGE => Event::MagneticModeChange { mode: b2 },
            SCREEN_CLEAR_DONE => Event::ScreenClearDone,
            BATTERY_STATUS => Event::BatteryStatus {
                level: b4,
                online: (b5 & BATTERY_FLAG_ONLINE) != 0,
                charging: (b5 & BATTERY_FLAG_CHARGING) != 0,
            },
            _ => Event::Unknown {
                event_type,
                data: [b2, b3, b4, b5],
            },
        })
    }

    /// Get event name for display
    pub fn name(event_type: u8) -> &'static str {
        match event_type {
            WAKE => "WAKE",
            PROFILE_CHANGE => "PROFILE_CHANGE",
            KB_FUNC => "KB_FUNC",
            LED_EFFECT_MODE => "LED_EFFECT_MODE",
            LED_EFFECT_SPEED => "LED_EFFECT_SPEED",
            LED_BRIGHTNESS => "LED_BRIGHTNESS",
            LED_COLOR => "LED_COLOR",
            SIDE_LED_MODE => "SIDE_LED_MODE",
            SIDE_LED_SPEED => "SIDE_LED_SPEED",
            SIDE_LED_BRIGHTNESS => "SIDE_LED_BRIGHTNESS",
            SIDE_LED_COLOR => "SIDE_LED_COLOR",
            RESET_TRIGGERED => "RESET_TRIGGERED",
            SETTINGS_ACK => "SETTINGS_ACK",
            SLEEP_MODE_CHANGE => "SLEEP_MODE_CHANGE",
            KEY_DEPTH => "KEY_DEPTH",
            MAGNETIC_MODE_CHANGE => "MAGNETIC_MODE_CHANGE",
            SCREEN_CLEAR_DONE => "SCREEN_CLEAR_DONE",
            BATTERY_STATUS => "BATTERY_STATUS",
            _ => "UNKNOWN",
        }
    }
}

/// FEATURE report command reference (observed from USB captures)
///
/// These commands are sent via SET_REPORT/GET_REPORT HID requests on the control endpoint.
/// Format: `[cmd, param1, param2, param3, param4, param5, param6, checksum, ...]`
/// Checksum at byte 7 = 255 - (sum of bytes 0-6)
///
/// # Protocol Commands (not data commands)
/// - `0xF7` - WAKE/INIT: Wakes dongle, triggers pending response retrieval
/// - `0xFC` - FLUSH: No-op that flushes response buffer without overwriting
///
/// # GET Commands (read settings)
/// | Cmd  | Name | Description |
/// |------|------|-------------|
/// | 0x80 | GET_REV | Firmware revision |
/// | 0x83 | GET_LED_MODE | LED effect mode (0-25) |
/// | 0x84 | GET_LED_BRIGHTNESS | LED brightness level (0-4) |
/// | 0x86 | GET_LED_SPEED | LED animation speed (0-4) |
/// | 0x87 | GET_LED_COLOR | LED color index (0-7) |
/// | 0x88 | GET_DEBOUNCE | Debounce time in ms |
/// | 0x89 | GET_POLLING_RATE | Polling rate code (0-6) |
/// | 0x8F | GET_ALL_SETTINGS | Bulk settings read |
/// | 0x91 | GET_KB_OPTIONS | Keyboard options (WASD swap, Win lock, etc.) |
/// | 0x97 | GET_AUTOOS_EN | Auto OS detection enabled (boolean) |
/// | 0xAD | GET_OLED_VERSION | OLED screen firmware version (u16 LE) |
/// | 0xAE | GET_MLED_VERSION | Matrix LED firmware version (u16 LE) |
/// | 0xE5 | GET_MULTI_MAGNETISM | Per-key actuation/RT settings |
///
/// # SET Commands (write settings)
/// | Cmd  | Name | Format |
/// |------|------|--------|
/// | 0x04 | SET_PROFILE | `[0x04, profile, 0, 0, 0, 0, 0, chk]` |
/// | 0x8A | SET_ACTUATION | `[0x8A, profile, value, col, row, 0, 0, chk]` |
/// | 0x90 | SET_RT | `[0x90, profile, value, col, row, 0, 0, chk]` |
/// | 0xE5 | SET_MULTI_MAGNETISM | Extended per-key settings |
///
/// # E5 Extended Command Format
/// GET: `[0xE5, type, 0x01, profile, 0, 0, 0, chk]`
/// | Type | Description |
/// |------|-------------|
/// | 0x00 | Actuation per-key data, page 0 |
/// | 0x01 | Actuation per-key data, page 1 |
/// | 0x06 | Rapid Trigger per-key data, page 0 |
/// | 0x07 | Rapid Trigger per-key data, page 1 |
/// | 0xFB | Extended trigger data 1 |
/// | 0xFC | Extended trigger data 2 |
///
/// # 8A/90 Per-Key Command Format
/// `[cmd, profile, value, col, row, 0, 0, checksum]`
/// - profile: 0-1 (only 2 profiles observed, though 0-3 may be valid)
/// - value: 0xFF = max (use default actuation/RT distance)
/// - col: 0-7 (key column in matrix)
/// - row: 0-3 (key row in matrix)
///
/// Iterates through all keys: profiles 0-1, cols 0-7, rows 0-3 (64 commands total)
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
        buf[0] = super::cmd::SET_SCREEN_COLOR; // 0x0E
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
