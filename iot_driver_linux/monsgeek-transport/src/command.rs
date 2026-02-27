//! Type-safe HID command builders and response parsers
//!
//! This module provides a cleaner API for building HID commands and parsing responses,
//! handling protocol quirks (checksums, byte ordering, value transformations) in one place.

use std::fmt;

use crate::protocol::{self, cmd};
use crate::types::ChecksumType;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

// =============================================================================
// Bounds limits for keymatrix writes
// =============================================================================

/// Maximum valid profile index (0-3).
pub const MAX_PROFILE: u8 = 3;
/// Maximum valid key index (0-125, matrix has 126 positions).
pub const MAX_KEY_INDEX: u8 = 125;
/// Maximum valid layer index (0 = base, 1 = layer1, 2 = Fn).
pub const MAX_LAYER: u8 = 2;
/// Maximum valid macro slot index (0-49).
///
/// The firmware's macro save function (Ghidra 0x08008384) writes 256 bytes per
/// macro at `(macro_id & 7) * 0x100` within 2KB flash pages at
/// `0x0802B800 + (macro_id >> 3) * 0x800`.  The 0x800-byte stack buffer cannot
/// overflow (max offset 7*256+256 = 2048 = buffer size).  The web app supports
/// 50 macros (maxMacro=50), fitting in 7 pages (14KB) below the userpic area.
pub const MAX_MACRO_INDEX: u8 = 49;
/// Maximum valid chunk page index (staging buffer holds ~9 chunks of 56 bytes).
pub const MAX_CHUNK_PAGE: u8 = 9;
/// Maximum payload bytes per chunk.
pub const CHUNK_PAYLOAD_SIZE: usize = 56;

/// Firmware commands write to RAM and flash using indices from the HID report
/// with NO bounds checking. Out-of-range values corrupt RAM or overflow the
/// stack; if corrupted data reaches flash the MCU boot-loops.
#[derive(Debug, Clone)]
pub struct KeyMatrixBoundsError(String);

impl fmt::Display for KeyMatrixBoundsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for KeyMatrixBoundsError {}

// =============================================================================
// Core Traits
// =============================================================================

/// A command that can be serialized to HID bytes
pub trait HidCommand: Sized {
    /// Command byte (e.g., 0x07 for SET_LEDPARAM)
    const CMD: u8;

    /// Checksum type for this command
    const CHECKSUM: ChecksumType;

    /// Serialize to bytes (excluding report ID and command byte)
    fn to_data(&self) -> Vec<u8>;

    /// Build complete HID buffer (65 bytes with report ID, command, data, checksum)
    fn build(&self) -> Vec<u8> {
        protocol::build_command(Self::CMD, &self.to_data(), Self::CHECKSUM)
    }
}

/// A response that can be parsed from HID bytes
pub trait HidResponse: Sized {
    /// Expected command echo byte (for validation)
    const CMD_ECHO: u8;

    /// Minimum response length required
    const MIN_LEN: usize;

    /// Parse from response bytes (excluding report ID, starting with command echo)
    fn from_data(data: &[u8]) -> Result<Self, ParseError>;

    /// Parse with validation
    fn parse(data: &[u8]) -> Result<Self, ParseError> {
        if data.len() < Self::MIN_LEN {
            return Err(ParseError::TooShort {
                expected: Self::MIN_LEN,
                got: data.len(),
            });
        }
        if data[0] != Self::CMD_ECHO {
            return Err(ParseError::CommandMismatch {
                expected: Self::CMD_ECHO,
                got: data[0],
            });
        }
        Self::from_data(data)
    }
}

/// Parse error for responses
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    TooShort { expected: usize, got: usize },
    CommandMismatch { expected: u8, got: u8 },
    InvalidValue { field: &'static str, value: u8 },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { expected, got } => {
                write!(
                    f,
                    "Response too short: expected {} bytes, got {}",
                    expected, got
                )
            }
            Self::CommandMismatch { expected, got } => {
                write!(
                    f,
                    "Command mismatch: expected 0x{:02X}, got 0x{:02X}",
                    expected, got
                )
            }
            Self::InvalidValue { field, value } => {
                write!(f, "Invalid value for {}: 0x{:02X}", field, value)
            }
        }
    }
}

impl std::error::Error for ParseError {}

// =============================================================================
// LED Parameters
// =============================================================================

/// LED mode enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum LedMode {
    #[default]
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
    UserPicture = 13,
    Laser = 14,
    CircleWave = 15,
    Rainbow = 16,
    RainDown = 17,
    Meteor = 18,
    ReactiveOff = 19,
    MusicPatterns = 20,
    ScreenSync = 21,
    MusicBars = 22,
    Train = 23,
    Fireworks = 24,
}

impl LedMode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
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
}

/// RGB color
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const BLACK: Self = Self::new(0, 0, 0);
    pub const WHITE: Self = Self::new(255, 255, 255);
    pub const RED: Self = Self::new(255, 0, 0);
    pub const GREEN: Self = Self::new(0, 255, 0);
    pub const BLUE: Self = Self::new(0, 0, 255);
}

/// LED dazzle (rainbow cycle) on - rainbow cycling (value 7 in protocol)
pub const DAZZLE_ON: u8 = 7;
/// LED dazzle (rainbow cycle) off - unicolor (value 8 in protocol)
pub const DAZZLE_OFF: u8 = 8;
/// Maximum speed value (5 levels: 0-4)
pub const SPEED_MAX: u8 = 4;
/// Maximum brightness value (4 levels: 0-4)
pub const BRIGHTNESS_MAX: u8 = 4;

/// Convert user-facing speed (0=slow, 4=fast) to wire format (4=slow, 0=fast)
#[inline]
pub fn speed_to_wire(speed: u8) -> u8 {
    SPEED_MAX - speed.min(SPEED_MAX)
}

/// Convert wire format speed (4=slow, 0=fast) to user-facing (0=slow, 4=fast)
#[inline]
pub fn speed_from_wire(wire: u8) -> u8 {
    SPEED_MAX - wire.min(SPEED_MAX)
}

/// SET_LEDPARAM command (0x07)
#[derive(Debug, Clone)]
pub struct SetLedParams {
    pub mode: LedMode,
    pub speed: u8,      // 0-4, user-facing (0=slow, 4=fast)
    pub brightness: u8, // 0-4
    pub color: Rgb,
    pub dazzle: bool,
    pub layer: u8, // For UserPicture mode
}

impl Default for SetLedParams {
    fn default() -> Self {
        Self {
            mode: LedMode::Off,
            speed: 2,
            brightness: 4,
            color: Rgb::WHITE,
            dazzle: false,
            layer: 0,
        }
    }
}

impl SetLedParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mode(mut self, mode: LedMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn speed(mut self, speed: u8) -> Self {
        self.speed = speed.min(SPEED_MAX);
        self
    }

    pub fn brightness(mut self, brightness: u8) -> Self {
        self.brightness = brightness.min(BRIGHTNESS_MAX);
        self
    }

    pub fn color(mut self, r: u8, g: u8, b: u8) -> Self {
        self.color = Rgb::new(r, g, b);
        self
    }

    pub fn rgb(mut self, color: Rgb) -> Self {
        self.color = color;
        self
    }

    pub fn dazzle(mut self, enabled: bool) -> Self {
        self.dazzle = enabled;
        self
    }

    pub fn layer(mut self, layer: u8) -> Self {
        self.layer = layer;
        self
    }
}

impl HidCommand for SetLedParams {
    const CMD: u8 = cmd::SET_LEDPARAM;
    const CHECKSUM: ChecksumType = ChecksumType::Bit8;

    fn to_data(&self) -> Vec<u8> {
        // Protocol quirks handled here:
        // - Speed is INVERTED in protocol (0 = fast, 4 = slow)
        // - UserPicture mode has special option/color handling

        let (option, r, g, b) = if self.mode == LedMode::UserPicture {
            // UserPicture: option = layer << 4, fixed color (0, 200, 200)
            (self.layer << 4, 0, 200, 200)
        } else {
            let opt = if self.dazzle { DAZZLE_ON } else { DAZZLE_OFF };
            (opt, self.color.r, self.color.g, self.color.b)
        };

        vec![
            self.mode as u8,
            SPEED_MAX - self.speed, // Invert speed for protocol
            self.brightness.min(BRIGHTNESS_MAX),
            option,
            r,
            g,
            b,
        ]
    }
}

/// GET_LEDPARAM response
#[derive(Debug, Clone)]
pub struct LedParamsResponse {
    pub mode: LedMode,
    pub speed: u8, // User-facing (0=slow, 4=fast)
    pub brightness: u8,
    pub color: Rgb,
    pub dazzle: bool,
    pub option_raw: u8, // Raw option byte for special modes
}

impl HidResponse for LedParamsResponse {
    const CMD_ECHO: u8 = cmd::GET_LEDPARAM;
    const MIN_LEN: usize = 8;

    fn from_data(data: &[u8]) -> Result<Self, ParseError> {
        // data[0] = cmd echo (already validated)
        // data[1] = mode, data[2] = speed (inverted), data[3] = brightness
        // data[4] = option, data[5..8] = RGB

        let mode = LedMode::from_u8(data[1]).unwrap_or(LedMode::Off);
        let speed_raw = data[2];
        let option = data[4];

        Ok(Self {
            mode,
            speed: SPEED_MAX - speed_raw.min(SPEED_MAX), // Invert back
            brightness: data[3],
            color: Rgb::new(data[5], data[6], data[7]),
            dazzle: (option & 0x0F) == DAZZLE_ON,
            option_raw: option,
        })
    }
}

// =============================================================================
// Profile Commands
// =============================================================================

/// SET_PROFILE command (0x04)
#[derive(Debug, Clone)]
pub struct SetProfile {
    pub profile: u8, // 0-3
}

impl SetProfile {
    pub fn new(profile: u8) -> Self {
        Self {
            profile: profile.min(3),
        }
    }
}

impl HidCommand for SetProfile {
    const CMD: u8 = cmd::SET_PROFILE;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;

    fn to_data(&self) -> Vec<u8> {
        vec![self.profile]
    }
}

/// GET_PROFILE response
#[derive(Debug, Clone)]
pub struct ProfileResponse {
    pub profile: u8,
}

impl HidResponse for ProfileResponse {
    const CMD_ECHO: u8 = cmd::GET_PROFILE;
    const MIN_LEN: usize = 2;

    fn from_data(data: &[u8]) -> Result<Self, ParseError> {
        Ok(Self { profile: data[1] })
    }
}

// =============================================================================
// Polling Rate
// =============================================================================

/// Polling rate enumeration with Hz values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollingRate {
    Hz8000 = 0,
    Hz4000 = 1,
    Hz2000 = 2,
    Hz1000 = 3,
    Hz500 = 4,
    Hz250 = 5,
    Hz125 = 6,
}

impl PollingRate {
    pub fn from_protocol(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Hz8000),
            1 => Some(Self::Hz4000),
            2 => Some(Self::Hz2000),
            3 => Some(Self::Hz1000),
            4 => Some(Self::Hz500),
            5 => Some(Self::Hz250),
            6 => Some(Self::Hz125),
            _ => None,
        }
    }

    pub fn to_hz(self) -> u16 {
        match self {
            Self::Hz8000 => 8000,
            Self::Hz4000 => 4000,
            Self::Hz2000 => 2000,
            Self::Hz1000 => 1000,
            Self::Hz500 => 500,
            Self::Hz250 => 250,
            Self::Hz125 => 125,
        }
    }

    pub fn from_hz(hz: u16) -> Option<Self> {
        match hz {
            8000 => Some(Self::Hz8000),
            4000 => Some(Self::Hz4000),
            2000 => Some(Self::Hz2000),
            1000 => Some(Self::Hz1000),
            500 => Some(Self::Hz500),
            250 => Some(Self::Hz250),
            125 => Some(Self::Hz125),
            _ => None,
        }
    }
}

/// SET_REPORT (polling rate) command (0x03)
#[derive(Debug, Clone)]
pub struct SetPollingRate {
    pub rate: PollingRate,
}

impl SetPollingRate {
    pub fn new(rate: PollingRate) -> Self {
        Self { rate }
    }

    pub fn from_hz(hz: u16) -> Option<Self> {
        PollingRate::from_hz(hz).map(|rate| Self { rate })
    }
}

impl HidCommand for SetPollingRate {
    const CMD: u8 = cmd::SET_REPORT;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;

    fn to_data(&self) -> Vec<u8> {
        vec![self.rate as u8]
    }
}

/// GET_REPORT (polling rate) response
#[derive(Debug, Clone)]
pub struct PollingRateResponse {
    pub rate: PollingRate,
}

impl HidResponse for PollingRateResponse {
    const CMD_ECHO: u8 = cmd::GET_REPORT;
    const MIN_LEN: usize = 2;

    fn from_data(data: &[u8]) -> Result<Self, ParseError> {
        let rate = PollingRate::from_protocol(data[1]).ok_or(ParseError::InvalidValue {
            field: "polling_rate",
            value: data[1],
        })?;
        Ok(Self { rate })
    }
}

// =============================================================================
// Sleep Time
// =============================================================================

/// SET_SLEEPTIME command (0x11)
///
/// Sets all 4 sleep time values:
/// - idle_bt: Bluetooth idle timeout (light sleep)
/// - idle_24g: 2.4GHz idle timeout (light sleep)
/// - deep_bt: Bluetooth deep sleep timeout
/// - deep_24g: 2.4GHz deep sleep timeout
///
/// All values are in seconds. Set to 0 to disable.
#[derive(Debug, Clone)]
pub struct SetSleepTime {
    /// Bluetooth idle timeout in seconds
    pub idle_bt: u16,
    /// 2.4GHz idle timeout in seconds
    pub idle_24g: u16,
    /// Bluetooth deep sleep timeout in seconds
    pub deep_bt: u16,
    /// 2.4GHz deep sleep timeout in seconds
    pub deep_24g: u16,
}

impl SetSleepTime {
    /// Create with all 4 sleep time values
    pub fn new(idle_bt: u16, idle_24g: u16, deep_bt: u16, deep_24g: u16) -> Self {
        Self {
            idle_bt,
            idle_24g,
            deep_bt,
            deep_24g,
        }
    }

    /// Create with uniform idle and deep timeouts for both wireless modes
    pub fn uniform(idle_seconds: u16, deep_seconds: u16) -> Self {
        Self {
            idle_bt: idle_seconds,
            idle_24g: idle_seconds,
            deep_bt: deep_seconds,
            deep_24g: deep_seconds,
        }
    }

    /// Create from minutes (convenience method)
    pub fn from_minutes(idle_mins: u16, deep_mins: u16) -> Self {
        Self::uniform(idle_mins * 60, deep_mins * 60)
    }

    /// Disable all sleep timeouts
    pub fn disabled() -> Self {
        Self::uniform(0, 0)
    }
}

impl HidCommand for SetSleepTime {
    const CMD: u8 = cmd::SET_SLEEPTIME;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;

    fn to_data(&self) -> Vec<u8> {
        // Webapp packet layout: command at [0], data at [8..16]
        // Our to_data() goes to buf[2..], so we need padding to reach buf[9]
        // buf[9] = to_data()[7], buf[10] = to_data()[8], etc.
        let mut data = vec![0u8; 15];
        // idle_bt at bytes 7-8 (becomes packet bytes 9-10, webapp bytes 8-9)
        data[7..9].copy_from_slice(&self.idle_bt.to_le_bytes());
        // idle_24g at bytes 9-10
        data[9..11].copy_from_slice(&self.idle_24g.to_le_bytes());
        // deep_bt at bytes 11-12
        data[11..13].copy_from_slice(&self.deep_bt.to_le_bytes());
        // deep_24g at bytes 13-14
        data[13..15].copy_from_slice(&self.deep_24g.to_le_bytes());
        data
    }
}

/// GET_SLEEPTIME response (0x91)
///
/// Contains all 4 sleep time values in seconds.
#[derive(Debug, Clone)]
pub struct SleepTimeResponse {
    /// Bluetooth idle timeout in seconds
    pub idle_bt: u16,
    /// 2.4GHz idle timeout in seconds
    pub idle_24g: u16,
    /// Bluetooth deep sleep timeout in seconds
    pub deep_bt: u16,
    /// 2.4GHz deep sleep timeout in seconds
    pub deep_24g: u16,
}

impl SleepTimeResponse {
    /// Get idle timeout in minutes for specified mode
    pub fn idle_minutes(&self, is_bt: bool) -> u16 {
        let secs = if is_bt { self.idle_bt } else { self.idle_24g };
        secs / 60
    }

    /// Get deep sleep timeout in minutes for specified mode
    pub fn deep_minutes(&self, is_bt: bool) -> u16 {
        let secs = if is_bt { self.deep_bt } else { self.deep_24g };
        secs / 60
    }

    /// Check if idle sleep is disabled for specified mode
    pub fn is_idle_disabled(&self, is_bt: bool) -> bool {
        if is_bt {
            self.idle_bt == 0
        } else {
            self.idle_24g == 0
        }
    }

    /// Check if deep sleep is disabled for specified mode
    pub fn is_deep_disabled(&self, is_bt: bool) -> bool {
        if is_bt {
            self.deep_bt == 0
        } else {
            self.deep_24g == 0
        }
    }
}

impl HidResponse for SleepTimeResponse {
    const CMD_ECHO: u8 = cmd::GET_SLEEPTIME;
    const MIN_LEN: usize = 16; // Need bytes 8-15 for all 4 values

    fn from_data(data: &[u8]) -> Result<Self, ParseError> {
        // Webapp reads from response bytes 8-15
        // data[0] = command echo, so data[8..16] = sleep time values
        if data.len() < 16 {
            return Err(ParseError::TooShort {
                expected: 16,
                got: data.len(),
            });
        }
        let idle_bt = u16::from_le_bytes([data[8], data[9]]);
        let idle_24g = u16::from_le_bytes([data[10], data[11]]);
        let deep_bt = u16::from_le_bytes([data[12], data[13]]);
        let deep_24g = u16::from_le_bytes([data[14], data[15]]);
        Ok(Self {
            idle_bt,
            idle_24g,
            deep_bt,
            deep_24g,
        })
    }
}

// =============================================================================
// Debounce
// =============================================================================

/// SET_DEBOUNCE command (0x06)
#[derive(Debug, Clone)]
pub struct SetDebounce {
    pub ms: u8, // 0-50
}

impl SetDebounce {
    pub fn new(ms: u8) -> Self {
        Self { ms: ms.min(50) }
    }
}

impl HidCommand for SetDebounce {
    const CMD: u8 = cmd::SET_DEBOUNCE;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;

    fn to_data(&self) -> Vec<u8> {
        vec![self.ms]
    }
}

/// GET_DEBOUNCE response
#[derive(Debug, Clone)]
pub struct DebounceResponse {
    pub ms: u8,
}

impl HidResponse for DebounceResponse {
    const CMD_ECHO: u8 = cmd::GET_DEBOUNCE;
    const MIN_LEN: usize = 2;

    fn from_data(data: &[u8]) -> Result<Self, ParseError> {
        Ok(Self { ms: data[1] })
    }
}

// =============================================================================
// Dongle Status (0xF7)
// =============================================================================

/// GET_DONGLE_STATUS command (0xF7) - for wireless dongles
///
/// Handled locally by the dongle — NOT forwarded to keyboard.
/// Returns a 9-byte status struct (see `DongleStatusResponse`).
#[derive(Debug, Clone, Default)]
pub struct DongleStatusQuery;

impl HidCommand for DongleStatusQuery {
    const CMD: u8 = cmd::GET_DONGLE_STATUS;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;

    fn to_data(&self) -> Vec<u8> {
        vec![]
    }
}

/// Dongle status response (9 bytes from GET_DONGLE_STATUS / 0xF7)
///
/// From dongle firmware RE (vendor_command_dispatch):
/// ```text
/// Byte 0: has_response      1 if cached_kb_response has unread data
/// Byte 1: kb_battery_info   keyboard battery level (0-100%)
/// Byte 2: 0                 reserved
/// Byte 3: kb_charging       1 if keyboard is charging
/// Byte 4: 1                 hardcoded (always valid marker)
/// Byte 5: rf_ready          0=waiting for kb response, 1=idle/ready
/// Byte 6: 1                 hardcoded (dongle alive marker)
/// Byte 7: pairing_mode      1=in pairing mode
/// Byte 8: pairing_status    1=paired
/// ```
#[derive(Debug, Clone)]
pub struct DongleStatusResponse {
    /// Whether cached_kb_response has unread data
    pub has_response: bool,
    /// Keyboard battery level (0-100%)
    pub kb_battery_level: u8,
    /// Keyboard is charging
    pub kb_charging: bool,
    /// RF link ready (0=waiting for keyboard response, 1=idle/ready).
    /// Forced to 1 when kb_charging is set.
    pub rf_ready: bool,
    /// Dongle is in pairing mode
    pub pairing_mode: bool,
    /// Dongle is paired with a keyboard
    pub pairing_status: bool,
}

impl DongleStatusResponse {
    /// Parse directly from feature report buffer (65 bytes with report ID at [0]).
    ///
    /// This is the preferred path for `get_battery_status()` since the F7 response
    /// uses `has_response` as byte 0 (not a command echo), so the standard
    /// `HidResponse::parse()` flow doesn't apply cleanly.
    pub fn from_feature_report(buf: &[u8]) -> Result<Self, ParseError> {
        // buf[0] = Report ID, buf[1..] = response data
        if buf.len() < 10 {
            return Err(ParseError::TooShort {
                expected: 10,
                got: buf.len(),
            });
        }
        let level = buf[2]; // usb_response[1] = kb_battery_info
        if level > 100 {
            return Err(ParseError::InvalidValue {
                field: "kb_battery_level",
                value: level,
            });
        }
        Ok(Self {
            has_response: buf[1] != 0,
            kb_battery_level: level,
            kb_charging: buf[4] != 0,
            rf_ready: buf[6] != 0,
            pairing_mode: buf[8] != 0,
            pairing_status: buf[9] != 0,
        })
    }
}

impl HidResponse for DongleStatusResponse {
    // Byte 0 of the response is `has_response` (0 or 1), not a command echo.
    // We use 0x01 to match the common case (has_response=1 after a forwarded
    // command), but callers should prefer `from_feature_report()` instead.
    const CMD_ECHO: u8 = 0x01;
    const MIN_LEN: usize = 9;

    fn from_data(data: &[u8]) -> Result<Self, ParseError> {
        // data[0] = has_response (already validated as CMD_ECHO)
        // data[1] = kb_battery_info
        // data[2] = 0 (reserved)
        // data[3] = kb_charging
        // data[4] = 1 (hardcoded)
        // data[5] = rf_ready
        // data[6] = 1 (hardcoded)
        // data[7] = pairing_mode
        // data[8] = pairing_status
        let level = data[1];
        if level > 100 {
            return Err(ParseError::InvalidValue {
                field: "kb_battery_level",
                value: level,
            });
        }
        Ok(Self {
            has_response: data[0] != 0,
            kb_battery_level: level,
            kb_charging: data[3] != 0,
            rf_ready: data[5] != 0,
            pairing_mode: data[7] != 0,
            pairing_status: data[8] != 0,
        })
    }
}

// =============================================================================
// Magnetism (Hall Effect) Commands
// =============================================================================

/// SET_MAGNETISM_REPORT command (0x1B) - enable/disable key depth reporting
#[derive(Debug, Clone)]
pub struct SetMagnetismReport {
    pub enabled: bool,
}

impl SetMagnetismReport {
    pub fn enable() -> Self {
        Self { enabled: true }
    }

    pub fn disable() -> Self {
        Self { enabled: false }
    }
}

impl HidCommand for SetMagnetismReport {
    const CMD: u8 = cmd::SET_MAGNETISM_REPORT;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;

    fn to_data(&self) -> Vec<u8> {
        vec![if self.enabled { 1 } else { 0 }]
    }
}

// =============================================================================
// Typed Packet Structs (zerocopy)
// =============================================================================

/// SET_KEYMATRIX (0x0A) — 11-byte data payload, Bit7 checksum.
///
/// Sets a key's config on the base layer (layer 0-1).
/// Byte 6 (`_checksum`) is a placeholder overwritten by the transport's Bit7 checksum.
///
/// Fields are private to enforce bounds at construction time — the firmware
/// performs NO bounds checking on profile/key_index/layer.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct SetKeyMatrixData {
    profile: u8,
    key_index: u8,
    _pad0: u8,
    _pad1: u8,
    enabled: u8,
    layer: u8,
    _checksum: u8,
    config_type: u8,
    b1: u8,
    b2: u8,
    b3: u8,
}

impl SetKeyMatrixData {
    /// Create a new keymatrix write command with bounds-checked parameters.
    ///
    /// Returns `Err` if any of profile/key_index/layer exceed firmware limits.
    pub fn new(
        profile: u8,
        key_index: u8,
        layer: u8,
        enabled: bool,
        config: [u8; 4],
    ) -> Result<Self, KeyMatrixBoundsError> {
        if profile > MAX_PROFILE {
            return Err(KeyMatrixBoundsError(format!(
                "profile {profile} out of range (max {MAX_PROFILE})"
            )));
        }
        if key_index > MAX_KEY_INDEX {
            return Err(KeyMatrixBoundsError(format!(
                "key_index {key_index} out of range (max {MAX_KEY_INDEX})"
            )));
        }
        if layer > MAX_LAYER {
            return Err(KeyMatrixBoundsError(format!(
                "layer {layer} out of range (max {MAX_LAYER})"
            )));
        }
        Ok(Self {
            profile,
            key_index,
            _pad0: 0,
            _pad1: 0,
            enabled: u8::from(enabled),
            layer,
            _checksum: 0,
            config_type: config[0],
            b1: config[1],
            b2: config[2],
            b3: config[3],
        })
    }
}

impl HidCommand for SetKeyMatrixData {
    const CMD: u8 = cmd::SET_KEYMATRIX;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;
    fn to_data(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

/// SET_FN (0x10) — 11-byte data payload, Bit7 checksum.
///
/// Sets a key's config on the Fn layer (layer 2+).
///
/// Wire layout differs from SetKeyMatrixData: byte 0 is `fn_sys` (sub-target
/// selecting the OS-specific Fn layer: 0 = Win, 1 = Mac), byte 1 is `profile`,
/// and `key_index` is at byte 2.
///
/// Fields are private to enforce bounds at construction time.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct SetFnData {
    fn_sys: u8,
    profile: u8,
    key_index: u8,
    _pad1: u8,
    _pad2: u8,
    _pad3: u8,
    _checksum: u8,
    config_type: u8,
    b1: u8,
    b2: u8,
    b3: u8,
}

impl SetFnData {
    /// Create a new Fn-layer write command with bounds-checked parameters.
    ///
    /// `fn_sys` selects the OS-specific Fn layer (0 = Win, 1 = Mac).
    pub fn new(
        fn_sys: u8,
        profile: u8,
        key_index: u8,
        config: [u8; 4],
    ) -> Result<Self, KeyMatrixBoundsError> {
        if profile > MAX_PROFILE {
            return Err(KeyMatrixBoundsError(format!(
                "profile {profile} out of range (max {MAX_PROFILE})"
            )));
        }
        if key_index > MAX_KEY_INDEX {
            return Err(KeyMatrixBoundsError(format!(
                "key_index {key_index} out of range (max {MAX_KEY_INDEX})"
            )));
        }
        Ok(Self {
            fn_sys,
            profile,
            key_index,
            _pad1: 0,
            _pad2: 0,
            _pad3: 0,
            _checksum: 0,
            config_type: config[0],
            b1: config[1],
            b2: config[2],
            b3: config[3],
        })
    }
}

impl HidCommand for SetFnData {
    const CMD: u8 = cmd::SET_FN;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;
    fn to_data(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

/// GET_KEYMATRIX (0x8A) query data — 4 bytes.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct GetKeyMatrixData {
    pub profile: u8,
    pub magic: u8,
    pub page: u8,
    pub magnetism_profile: u8,
}

/// GET_FN (0x90) query data — 4 bytes.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct GetFnData {
    pub sys: u8,
    pub profile: u8,
    pub magic: u8,
    pub page: u8,
}

/// SET_MACRO (0x0B) — 7-byte header, Bit7 checksum.
///
/// The header is followed by a variable-length payload chunk.
/// Use `SetMacroCommand::new()` to construct with bounds-checked parameters.
///
/// Fields are private: `macro_index` is bounds-checked to 0-49 (matching the web
/// app's maxMacro=50).  The firmware's macro save (Ghidra 0x08008384) uses 256
/// bytes per slot, 8 per 2KB page — no stack overflow for any index.
/// `page` indexes into a 514-byte staging buffer at 56 bytes/chunk — page >= 10
/// overflows into adjacent RAM.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct SetMacroHeader {
    macro_index: u8,
    page: u8,
    chunk_len: u8,
    is_last: u8,
    _pad0: u8,
    _pad1: u8,
    _checksum: u8,
}

/// Full SET_MACRO command: header + variable payload.
#[derive(Debug, Clone)]
pub struct SetMacroCommand {
    header: SetMacroHeader,
    payload: Vec<u8>,
}

impl SetMacroCommand {
    /// Create a bounds-checked SET_MACRO command.
    ///
    /// Returns `Err` if macro_index, page, or payload size exceed firmware limits.
    pub fn new(
        macro_index: u8,
        page: u8,
        is_last: bool,
        payload: Vec<u8>,
    ) -> Result<Self, KeyMatrixBoundsError> {
        if macro_index > MAX_MACRO_INDEX {
            return Err(KeyMatrixBoundsError(format!(
                "macro_index {macro_index} out of range (max {MAX_MACRO_INDEX})"
            )));
        }
        if page > MAX_CHUNK_PAGE {
            return Err(KeyMatrixBoundsError(format!(
                "page {page} out of range (max {MAX_CHUNK_PAGE})"
            )));
        }
        if payload.len() > CHUNK_PAYLOAD_SIZE {
            return Err(KeyMatrixBoundsError(format!(
                "payload {} bytes exceeds chunk limit ({CHUNK_PAYLOAD_SIZE})",
                payload.len()
            )));
        }
        Ok(Self {
            header: SetMacroHeader {
                macro_index,
                page,
                chunk_len: payload.len() as u8,
                is_last: u8::from(is_last),
                _pad0: 0,
                _pad1: 0,
                _checksum: 0,
            },
            payload,
        })
    }
}

impl HidCommand for SetMacroCommand {
    const CMD: u8 = cmd::SET_MACRO;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;
    fn to_data(&self) -> Vec<u8> {
        let mut data = self.header.as_bytes().to_vec();
        data.extend_from_slice(&self.payload);
        data
    }
}

/// GET_MACRO (0x8B) query data — 2 bytes.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct GetMacroData {
    pub macro_index: u8,
    pub page: u8,
}

/// SET_MULTI_MAGNETISM (0x65) — 7-byte header + variable payload, Bit7 checksum.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct SetMultiMagnetismHeader {
    pub sub_cmd: u8,
    pub flag: u8,
    pub page: u8,
    pub commit: u8,
    pub _pad0: u8,
    pub _pad1: u8,
    pub _checksum: u8,
}

/// Full SET_MULTI_MAGNETISM command: header + payload.
#[derive(Debug, Clone)]
pub struct SetMultiMagnetismCommand {
    pub header: SetMultiMagnetismHeader,
    pub payload: Vec<u8>,
}

impl HidCommand for SetMultiMagnetismCommand {
    const CMD: u8 = cmd::SET_MULTI_MAGNETISM;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;
    fn to_data(&self) -> Vec<u8> {
        let mut data = self.header.as_bytes().to_vec();
        data.extend_from_slice(&self.payload);
        data
    }
}

/// GET_MULTI_MAGNETISM (0xE5) query data — 3 bytes.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct GetMultiMagnetismData {
    pub sub_cmd: u8,
    pub flag: u8,
    pub page: u8,
}

/// SET_KEY_MAGNETISM_MODE (0x1D) — 4-byte data payload, Bit7 checksum.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct SetKeyMagnetismModeData {
    pub key_index: u8,
    pub actuation: u8,
    pub deactuation: u8,
    pub mode: u8,
}

impl HidCommand for SetKeyMagnetismModeData {
    const CMD: u8 = cmd::SET_KEY_MAGNETISM_MODE;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;
    fn to_data(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

/// Response entry for parsing GET_KEYMATRIX / GET_FN pages.
///
/// Each key config is 4 bytes: [config_type, b1, b2, b3].
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct KeyConfigEntry {
    pub config_type: u8,
    pub b1: u8,
    pub b2: u8,
    pub b3: u8,
}

// =============================================================================
// Query Commands (no data, just request)
// =============================================================================

/// Generic query command with no data
#[derive(Debug, Clone)]
pub struct QueryCommand<const CMD_BYTE: u8>;

impl<const CMD_BYTE: u8> Default for QueryCommand<CMD_BYTE> {
    fn default() -> Self {
        Self
    }
}

impl<const CMD_BYTE: u8> HidCommand for QueryCommand<CMD_BYTE> {
    const CMD: u8 = CMD_BYTE;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;

    fn to_data(&self) -> Vec<u8> {
        vec![]
    }
}

// Type aliases for common queries
pub type QueryLedParams = QueryCommand<{ cmd::GET_LEDPARAM }>;
pub type QueryProfile = QueryCommand<{ cmd::GET_PROFILE }>;
pub type QueryPollingRate = QueryCommand<{ cmd::GET_REPORT }>;
pub type QueryDebounce = QueryCommand<{ cmd::GET_DEBOUNCE }>;
pub type QuerySleepTime = QueryCommand<{ cmd::GET_SLEEPTIME }>;
pub type QueryVersion = QueryCommand<{ cmd::GET_USB_VERSION }>;

// =============================================================================
// Transport Extension for Typed Commands
// =============================================================================

// Note: TransportExt (send/query/query_no_echo) is now implemented as
// inherent methods on FlowControlTransport in flow_control.rs.
// These methods require flow control, so they don't belong on the raw Transport trait.

// =============================================================================
// Packet Dispatcher for PCAP Analysis
// =============================================================================

use crate::protocol::magnetism as mag_const;

/// Decoded magnetism data based on subcmd type
#[derive(Debug, Clone)]
pub enum MagnetismData {
    /// 2-byte values: PRESS_TRAVEL, LIFT_TRAVEL, RT_PRESS, RT_LIFT,
    /// BOTTOM_DEADZONE, TOP_DEADZONE, CALIBRATION, SWITCH_TYPE
    TwoByteValues(Vec<u16>),

    /// 1-byte values: KEY_MODE, SNAPTAP_ENABLE, MODTAP_TIME
    OneByteValues(Vec<u8>),

    /// 4-byte DKS travel data (2 × u16 per key)
    DksTravel(Vec<[u16; 2]>),

    /// DKS modes (complex structure, keep as bytes)
    DksModes(Vec<u8>),
}

impl MagnetismData {
    /// Format calibration progress as "X/32 keys calibrated"
    pub fn calibration_progress(&self) -> Option<(usize, usize)> {
        if let MagnetismData::TwoByteValues(values) = self {
            let finished = values.iter().filter(|&&v| v >= 300).count();
            Some((finished, values.len()))
        } else {
            None
        }
    }
}

/// Decode magnetism response data based on subcmd type
pub fn decode_magnetism_data(subcmd: u8, data: &[u8]) -> MagnetismData {
    match subcmd {
        // 2-byte per key: travel values, deadzone, calibration
        mag_const::PRESS_TRAVEL
        | mag_const::LIFT_TRAVEL
        | mag_const::RT_PRESS
        | mag_const::RT_LIFT
        | mag_const::BOTTOM_DEADZONE
        | mag_const::TOP_DEADZONE
        | mag_const::SWITCH_TYPE
        | mag_const::CALIBRATION => {
            let values: Vec<u16> = data
                .chunks(2)
                .filter_map(|c| c.try_into().ok())
                .map(u16::from_le_bytes)
                .collect();
            MagnetismData::TwoByteValues(values)
        }
        // 1-byte per key: modes, flags
        mag_const::KEY_MODE | mag_const::SNAPTAP_ENABLE | mag_const::MODTAP_TIME => {
            MagnetismData::OneByteValues(data.to_vec())
        }
        // 4-byte per key: DKS travel (2 × u16)
        mag_const::DKS_TRAVEL => {
            let values: Vec<[u16; 2]> = data
                .chunks(4)
                .filter_map(|c| {
                    if c.len() >= 4 {
                        Some([
                            u16::from_le_bytes([c[0], c[1]]),
                            u16::from_le_bytes([c[2], c[3]]),
                        ])
                    } else {
                        None
                    }
                })
                .collect();
            MagnetismData::DksTravel(values)
        }
        // DKS modes (complex, keep as bytes)
        mag_const::DKS_MODES => MagnetismData::DksModes(data.to_vec()),
        // Unknown subcmd, default to 2-byte
        _ => {
            let values: Vec<u16> = data
                .chunks(2)
                .filter_map(|c| c.try_into().ok())
                .map(u16::from_le_bytes)
                .collect();
            MagnetismData::TwoByteValues(values)
        }
    }
}

/// Parsed response - uses existing response types as single source of truth
///
/// This enum provides typed parsing of responses for tools like pcap analyzer.
/// Unknown responses are flagged for protocol discovery.
#[derive(Debug)]
pub enum ParsedResponse {
    Rev {
        data: Vec<u8>,
    },
    LedParams(LedParamsResponse),
    SledParams {
        data: Vec<u8>,
    },
    Profile(ProfileResponse),
    PollingRate(PollingRateResponse),
    Debounce(DebounceResponse),
    SleepTime(SleepTimeResponse),
    DongleStatus(DongleStatusResponse),
    UsbVersion {
        device_id: u32,
        version: u16,
    },
    KbOptions {
        data: Vec<u8>,
    },
    FeatureList {
        data: Vec<u8>,
    },
    KeyMatrix {
        data: Vec<u8>,
    },
    Macro {
        data: Vec<u8>,
    },
    UserPic {
        data: Vec<u8>,
    },
    FnLayer {
        data: Vec<u8>,
    },
    MagnetismMode {
        data: Vec<u8>,
    },
    Calibration {
        data: Vec<u8>,
    },
    MultiMagnetism {
        subcmd: u8,
        subcmd_name: &'static str,
        page: u8,
        data: Vec<u8>,
    },
    /// Decoded magnetism response with parsed data
    MultiMagnetismDecoded {
        subcmd: u8,
        subcmd_name: &'static str,
        page: u8,
        data: MagnetismData,
    },
    /// Auto-OS detection enabled status
    AutoOsEnabled {
        enabled: bool,
    },
    /// LED on/off (power save) status
    LedOnOff {
        enabled: bool,
    },
    /// OLED firmware version
    OledVersion {
        oled_version: u16,
        flash_version: u16,
    },
    /// Matrix LED firmware version
    MledVersion {
        version: u16,
    },
    /// GET_PATCH_INFO (0xE7) response - patch name, version, capabilities
    PatchInfo {
        data: Vec<u8>,
    },
    /// Empty/stale buffer - all zeros or starts with 0x00 with no meaningful data
    Empty,
    /// Response we don't have a parser for yet - key for protocol discovery
    Unknown {
        cmd: u8,
        data: Vec<u8>,
    },
}

/// Parsed command - reuses response types where format matches
#[derive(Debug)]
pub enum ParsedCommand {
    // GET commands (queries)
    GetRev,
    GetLedParams,
    GetSledParams,
    GetProfile,
    GetPollingRate,
    GetDebounce,
    GetSleepTime,
    GetUsbVersion,
    GetKbOptions,
    GetFeatureList,
    GetKeyMatrix,
    GetMacro,
    GetUserPic,
    GetFn,
    GetMagnetismMode,
    GetAutoOsEnabled,
    GetLedOnOff,
    GetOledVersion,
    GetMledVersion,
    GetCalibration {
        data: Vec<u8>,
    },
    /// GET_PATCH_INFO (0xE7) - custom firmware capabilities
    GetPatchInfo,
    /// GET_MULTI_MAGNETISM query
    /// Format: [0xE5, subcmd, 0x01, page, 0, 0, 0, checksum]
    GetMultiMagnetism {
        subcmd: u8,
        subcmd_name: &'static str,
        page: u8,
    },
    // SET commands
    SetReset,
    SetLedParams(LedParamsResponse), // Same format as response
    SetSledParams {
        data: Vec<u8>,
    }, // Side LED params
    SetProfile(ProfileResponse),
    SetPollingRate(PollingRateResponse),
    SetDebounce(DebounceResponse),
    SetSleepTime(SleepTimeResponse),
    SetMagnetismReport {
        enabled: bool,
    },
    SetKbOption {
        data: Vec<u8>,
    },
    SetKeyMatrix {
        data: Vec<u8>,
    },
    SetMacro {
        data: Vec<u8>,
    },
    SetUserPic {
        data: Vec<u8>,
    },
    SetAudioViz {
        data: Vec<u8>,
    },
    SetScreenColor {
        r: u8,
        g: u8,
        b: u8,
    },
    SetUserGif {
        data: Vec<u8>,
    },
    SetFn {
        data: Vec<u8>,
    },
    /// SET_MAGNETISM_CAL (0x1C) - enable/disable minimum position calibration mode
    /// Format: [0x1C, enabled, 0, 0, 0, 0, 0, checksum]
    SetMagnetismCal {
        enabled: bool,
    },
    /// SET_MAGNETISM_MAX_CAL (0x1E) - enable/disable maximum travel calibration mode
    /// Format: [0x1E, enabled, 0, 0, 0, 0, 0, checksum]
    SetMagnetismMaxCal {
        enabled: bool,
    },
    SetKeyMagnetismMode {
        data: Vec<u8>,
    },
    SetMultiMagnetism {
        subcmd: u8,
        subcmd_name: &'static str,
        page: u8,
        data: Vec<u8>,
    },
    // Dongle commands
    GetDongleStatus,
    GetCachedResponse,
    /// Command we don't have a parser for yet
    Unknown {
        cmd: u8,
        data: Vec<u8>,
    },
}

/// Try to parse response based on command byte - dispatches to existing parsers
///
/// This is the single source of truth for response parsing. Unknown responses
/// are flagged with the Unknown variant for investigation.
pub fn try_parse_response(data: &[u8]) -> ParsedResponse {
    if data.is_empty() {
        return ParsedResponse::Empty;
    }

    let cmd = data[0];

    // Detect empty/stale buffer: starts with 0x00 and all remaining bytes are zero
    // These are typically stale responses or padding from the device
    if cmd == 0x00 && data.iter().all(|&b| b == 0) {
        return ParsedResponse::Empty;
    }

    match cmd {
        cmd::GET_REV => ParsedResponse::Rev {
            data: data[1..].to_vec(),
        },
        cmd::GET_SLEDPARAM => ParsedResponse::SledParams {
            data: data[1..].to_vec(),
        },
        cmd::GET_MACRO => ParsedResponse::Macro {
            data: data[1..].to_vec(),
        },
        cmd::GET_USERPIC => ParsedResponse::UserPic {
            data: data[1..].to_vec(),
        },
        cmd::GET_LEDPARAM => LedParamsResponse::parse(data)
            .map(ParsedResponse::LedParams)
            .unwrap_or_else(|_| ParsedResponse::Unknown {
                cmd,
                data: data.to_vec(),
            }),
        cmd::GET_PROFILE => ProfileResponse::parse(data)
            .map(ParsedResponse::Profile)
            .unwrap_or_else(|_| ParsedResponse::Unknown {
                cmd,
                data: data.to_vec(),
            }),
        cmd::GET_REPORT => PollingRateResponse::parse(data)
            .map(ParsedResponse::PollingRate)
            .unwrap_or_else(|_| ParsedResponse::Unknown {
                cmd,
                data: data.to_vec(),
            }),
        cmd::GET_DEBOUNCE => DebounceResponse::parse(data)
            .map(ParsedResponse::Debounce)
            .unwrap_or_else(|_| ParsedResponse::Unknown {
                cmd,
                data: data.to_vec(),
            }),
        cmd::GET_SLEEPTIME => SleepTimeResponse::parse(data)
            .map(ParsedResponse::SleepTime)
            .unwrap_or_else(|_| ParsedResponse::Unknown {
                cmd,
                data: data.to_vec(),
            }),
        cmd::GET_USB_VERSION => {
            if data.len() >= 9 {
                ParsedResponse::UsbVersion {
                    device_id: u32::from_le_bytes([data[1], data[2], data[3], data[4]]),
                    version: u16::from_le_bytes([data[7], data[8]]),
                }
            } else {
                ParsedResponse::Unknown {
                    cmd,
                    data: data.to_vec(),
                }
            }
        }
        cmd::GET_KBOPTION => ParsedResponse::KbOptions {
            data: data[1..].to_vec(),
        },
        cmd::GET_FEATURE_LIST => ParsedResponse::FeatureList {
            data: data[1..].to_vec(),
        },
        cmd::GET_KEYMATRIX => ParsedResponse::KeyMatrix {
            data: data[1..].to_vec(),
        },
        cmd::GET_FN => ParsedResponse::FnLayer {
            data: data[1..].to_vec(),
        },
        cmd::GET_KEY_MAGNETISM_MODE => ParsedResponse::MagnetismMode {
            data: data[1..].to_vec(),
        },
        cmd::GET_AUTOOS_EN => ParsedResponse::AutoOsEnabled {
            enabled: data.get(1).copied().unwrap_or(0) == 1,
        },
        cmd::GET_LEDONOFF => ParsedResponse::LedOnOff {
            enabled: data.get(1).copied().unwrap_or(0) == 1,
        },
        cmd::GET_OLED_VERSION => {
            let oled = u16::from_le_bytes([
                data.get(1).copied().unwrap_or(0),
                data.get(2).copied().unwrap_or(0),
            ]);
            let flash = u16::from_le_bytes([
                data.get(3).copied().unwrap_or(0),
                data.get(4).copied().unwrap_or(0),
            ]);
            ParsedResponse::OledVersion {
                oled_version: oled,
                flash_version: flash,
            }
        }
        cmd::GET_MLED_VERSION => {
            let ver = u16::from_le_bytes([
                data.get(1).copied().unwrap_or(0),
                data.get(2).copied().unwrap_or(0),
            ]);
            ParsedResponse::MledVersion { version: ver }
        }
        cmd::GET_CALIBRATION => ParsedResponse::Calibration {
            data: data[1..].to_vec(),
        },
        cmd::GET_PATCH_INFO => ParsedResponse::PatchInfo {
            data: data[1..].to_vec(),
        },
        cmd::GET_MULTI_MAGNETISM => {
            let subcmd = data.get(1).copied().unwrap_or(0);
            let page = data.get(3).copied().unwrap_or(0);
            let raw_data = data.get(4..).unwrap_or(&[]);
            ParsedResponse::MultiMagnetismDecoded {
                subcmd,
                subcmd_name: protocol::magnetism::name(subcmd),
                page,
                data: decode_magnetism_data(subcmd, raw_data),
            }
        }
        // DongleStatus response uses has_response (0 or 1) as first byte, not
        // a standard command echo. Dispatch it when byte 0 == 0x01 (has_response=1).
        0x01 => {
            if data.len() >= 9 {
                DongleStatusResponse::from_data(data)
                    .map(ParsedResponse::DongleStatus)
                    .unwrap_or_else(|_| ParsedResponse::Unknown {
                        cmd,
                        data: data.to_vec(),
                    })
            } else {
                ParsedResponse::Unknown {
                    cmd,
                    data: data.to_vec(),
                }
            }
        }
        _ => ParsedResponse::Unknown {
            cmd,
            data: data.to_vec(),
        },
    }
}

/// Try to parse command based on command byte
///
/// Commands often have the same format as responses, so we reuse response parsers.
pub fn try_parse_command(data: &[u8]) -> ParsedCommand {
    if data.is_empty() {
        return ParsedCommand::Unknown {
            cmd: 0,
            data: vec![],
        };
    }
    let cmd = data[0];
    match cmd {
        // LED params: SET uses same format as GET response
        cmd::SET_LEDPARAM => parse_led_params_command(data)
            .map(ParsedCommand::SetLedParams)
            .unwrap_or_else(|| ParsedCommand::Unknown {
                cmd,
                data: data.to_vec(),
            }),
        cmd::SET_PROFILE => {
            if data.len() >= 2 {
                ParsedCommand::SetProfile(ProfileResponse { profile: data[1] })
            } else {
                ParsedCommand::Unknown {
                    cmd,
                    data: data.to_vec(),
                }
            }
        }
        cmd::SET_REPORT => {
            if data.len() >= 2 {
                PollingRate::from_protocol(data[1])
                    .map(|rate| ParsedCommand::SetPollingRate(PollingRateResponse { rate }))
                    .unwrap_or_else(|| ParsedCommand::Unknown {
                        cmd,
                        data: data.to_vec(),
                    })
            } else {
                ParsedCommand::Unknown {
                    cmd,
                    data: data.to_vec(),
                }
            }
        }
        cmd::SET_DEBOUNCE => {
            if data.len() >= 2 {
                ParsedCommand::SetDebounce(DebounceResponse { ms: data[1] })
            } else {
                ParsedCommand::Unknown {
                    cmd,
                    data: data.to_vec(),
                }
            }
        }
        cmd::SET_MAGNETISM_REPORT => {
            if data.len() >= 2 {
                ParsedCommand::SetMagnetismReport {
                    enabled: data[1] != 0,
                }
            } else {
                ParsedCommand::Unknown {
                    cmd,
                    data: data.to_vec(),
                }
            }
        }
        cmd::SET_RESET => ParsedCommand::SetReset,
        cmd::SET_KBOPTION => ParsedCommand::SetKbOption {
            data: data[1..].to_vec(),
        },
        cmd::SET_KEYMATRIX => ParsedCommand::SetKeyMatrix {
            data: data[1..].to_vec(),
        },
        cmd::SET_MACRO => ParsedCommand::SetMacro {
            data: data[1..].to_vec(),
        },
        cmd::SET_USERPIC => ParsedCommand::SetUserPic {
            data: data[1..].to_vec(),
        },
        cmd::SET_AUDIO_VIZ => ParsedCommand::SetAudioViz {
            data: data[1..].to_vec(),
        },
        cmd::SET_SCREEN_COLOR => ParsedCommand::SetScreenColor {
            r: data.get(1).copied().unwrap_or(0),
            g: data.get(2).copied().unwrap_or(0),
            b: data.get(3).copied().unwrap_or(0),
        },
        cmd::SET_USERGIF => ParsedCommand::SetUserGif {
            data: data[1..].to_vec(),
        },
        cmd::SET_FN => ParsedCommand::SetFn {
            data: data[1..].to_vec(),
        },
        cmd::SET_SLEDPARAM => ParsedCommand::SetSledParams {
            data: data[1..].to_vec(),
        },
        cmd::SET_MAGNETISM_CAL => ParsedCommand::SetMagnetismCal {
            enabled: data.get(1).copied().unwrap_or(0) != 0,
        },
        cmd::SET_MAGNETISM_MAX_CAL => ParsedCommand::SetMagnetismMaxCal {
            enabled: data.get(1).copied().unwrap_or(0) != 0,
        },
        cmd::SET_KEY_MAGNETISM_MODE => ParsedCommand::SetKeyMagnetismMode {
            data: data[1..].to_vec(),
        },
        cmd::SET_MULTI_MAGNETISM => {
            let subcmd = data.get(1).copied().unwrap_or(0);
            ParsedCommand::SetMultiMagnetism {
                subcmd,
                subcmd_name: protocol::magnetism::name(subcmd),
                page: data.get(3).copied().unwrap_or(0),
                data: data.get(4..).unwrap_or(&[]).to_vec(),
            }
        }

        // GET commands (queries - typically just command byte)
        cmd::GET_REV => ParsedCommand::GetRev,
        cmd::GET_LEDPARAM => ParsedCommand::GetLedParams,
        cmd::GET_SLEDPARAM => ParsedCommand::GetSledParams,
        cmd::GET_PROFILE => ParsedCommand::GetProfile,
        cmd::GET_REPORT => ParsedCommand::GetPollingRate,
        cmd::GET_DEBOUNCE => ParsedCommand::GetDebounce,
        cmd::GET_SLEEPTIME => ParsedCommand::GetSleepTime,
        cmd::GET_USB_VERSION => ParsedCommand::GetUsbVersion,
        cmd::GET_KBOPTION => ParsedCommand::GetKbOptions,
        cmd::GET_FEATURE_LIST => ParsedCommand::GetFeatureList,
        cmd::GET_KEYMATRIX => ParsedCommand::GetKeyMatrix,
        cmd::GET_MACRO => ParsedCommand::GetMacro,
        cmd::GET_USERPIC => ParsedCommand::GetUserPic,
        cmd::GET_FN => ParsedCommand::GetFn,
        cmd::GET_KEY_MAGNETISM_MODE => ParsedCommand::GetMagnetismMode,
        cmd::GET_AUTOOS_EN => ParsedCommand::GetAutoOsEnabled,
        cmd::GET_LEDONOFF => ParsedCommand::GetLedOnOff,
        cmd::GET_OLED_VERSION => ParsedCommand::GetOledVersion,
        cmd::GET_MLED_VERSION => ParsedCommand::GetMledVersion,
        cmd::GET_CALIBRATION => ParsedCommand::GetCalibration {
            data: data[1..].to_vec(),
        },
        cmd::GET_PATCH_INFO => ParsedCommand::GetPatchInfo,
        cmd::GET_MULTI_MAGNETISM => {
            let subcmd = data.get(1).copied().unwrap_or(0);
            ParsedCommand::GetMultiMagnetism {
                subcmd,
                subcmd_name: protocol::magnetism::name(subcmd),
                page: data.get(3).copied().unwrap_or(0),
            }
        }

        // Dongle commands
        cmd::GET_DONGLE_STATUS => ParsedCommand::GetDongleStatus,
        cmd::GET_CACHED_RESPONSE => ParsedCommand::GetCachedResponse,

        _ => ParsedCommand::Unknown {
            cmd,
            data: data.to_vec(),
        },
    }
}

/// Parse LED params from command data
/// Format: [cmd, mode, speed_inv, brightness, option, r, g, b]
fn parse_led_params_command(data: &[u8]) -> Option<LedParamsResponse> {
    if data.len() < 8 {
        return None;
    }
    let mode = LedMode::from_u8(data[1]).unwrap_or(LedMode::Off);
    let speed_raw = data[2];
    let option = data[4];

    Some(LedParamsResponse {
        mode,
        speed: 4u8.saturating_sub(speed_raw.min(4)), // Invert back
        brightness: data[3],
        color: Rgb::new(data[5], data[6], data[7]),
        dazzle: (option & 0x0F) == 7, // DAZZLE_ON = 7
        option_raw: option,
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::REPORT_SIZE;

    #[test]
    fn test_set_led_params_builder() {
        let cmd = SetLedParams::new()
            .mode(LedMode::Wave)
            .brightness(4)
            .speed(3)
            .color(255, 0, 128)
            .dazzle(true);

        let data = cmd.to_data();
        assert_eq!(data[0], LedMode::Wave as u8); // mode
        assert_eq!(data[1], 1); // speed inverted: 4 - 3 = 1
        assert_eq!(data[2], 4); // brightness
        assert_eq!(data[3], DAZZLE_ON); // dazzle
        assert_eq!(data[4], 255); // R
        assert_eq!(data[5], 0); // G
        assert_eq!(data[6], 128); // B
    }

    #[test]
    fn test_set_led_params_user_picture() {
        let cmd = SetLedParams::new()
            .mode(LedMode::UserPicture)
            .layer(2)
            .color(255, 0, 0); // Should be ignored

        let data = cmd.to_data();
        assert_eq!(data[0], LedMode::UserPicture as u8);
        assert_eq!(data[3], 2 << 4); // layer in option
        assert_eq!(data[4], 0); // Fixed R
        assert_eq!(data[5], 200); // Fixed G
        assert_eq!(data[6], 200); // Fixed B
    }

    #[test]
    fn test_led_params_response_parse() {
        // Simulated response: [cmd_echo, mode, speed_inv, brightness, option, r, g, b]
        let data = [0x87, 4, 1, 3, DAZZLE_ON, 128, 64, 32];

        let resp = LedParamsResponse::parse(&data).unwrap();
        assert_eq!(resp.mode, LedMode::Wave);
        assert_eq!(resp.speed, 3); // 4 - 1 = 3 (inverted back)
        assert_eq!(resp.brightness, 3);
        assert!(resp.dazzle);
        assert_eq!(resp.color.r, 128);
    }

    #[test]
    fn test_polling_rate() {
        let cmd = SetPollingRate::from_hz(1000).unwrap();
        let data = cmd.to_data();
        assert_eq!(data[0], 3); // 1000Hz = protocol value 3

        let resp_data = [0x83, 3];
        let resp = PollingRateResponse::parse(&resp_data).unwrap();
        assert_eq!(resp.rate.to_hz(), 1000);
    }

    #[test]
    fn test_sleep_time() {
        // Test command with 4 values: idle=120s, deep=1680s for both modes
        let cmd = SetSleepTime::uniform(120, 1680);
        let data = cmd.to_data();
        // Data should be 15 bytes with values at indices 7-14
        assert_eq!(data.len(), 15);
        // idle_bt at [7..9]: 120 = 0x0078 LE
        assert_eq!(data[7], 0x78);
        assert_eq!(data[8], 0x00);
        // idle_24g at [9..11]: 120 = 0x0078 LE
        assert_eq!(data[9], 0x78);
        assert_eq!(data[10], 0x00);
        // deep_bt at [11..13]: 1680 = 0x0690 LE
        assert_eq!(data[11], 0x90);
        assert_eq!(data[12], 0x06);
        // deep_24g at [13..15]: 1680 = 0x0690 LE
        assert_eq!(data[13], 0x90);
        assert_eq!(data[14], 0x06);

        // Test response parsing (16 bytes minimum with values at indices 8-15)
        let mut resp_data = [0u8; 16];
        resp_data[0] = 0x91; // command echo
                             // idle_bt = 120 at [8..10]
        resp_data[8] = 0x78;
        resp_data[9] = 0x00;
        // idle_24g = 120 at [10..12]
        resp_data[10] = 0x78;
        resp_data[11] = 0x00;
        // deep_bt = 1680 at [12..14]
        resp_data[12] = 0x90;
        resp_data[13] = 0x06;
        // deep_24g = 1680 at [14..16]
        resp_data[14] = 0x90;
        resp_data[15] = 0x06;

        let resp = SleepTimeResponse::parse(&resp_data).unwrap();
        assert_eq!(resp.idle_bt, 120);
        assert_eq!(resp.idle_24g, 120);
        assert_eq!(resp.deep_bt, 1680);
        assert_eq!(resp.deep_24g, 1680);
        assert_eq!(resp.idle_minutes(true), 2);
        assert_eq!(resp.deep_minutes(true), 28);
    }

    #[test]
    fn test_full_buffer_build() {
        let cmd = SetProfile::new(2);
        let buf = cmd.build();

        assert_eq!(buf.len(), REPORT_SIZE);
        assert_eq!(buf[0], 0); // Report ID
        assert_eq!(buf[1], cmd::SET_PROFILE); // Command
        assert_eq!(buf[2], 2); // Profile
                               // Checksum at buf[8] for Bit7
    }

    #[test]
    fn test_try_parse_command_led_params() {
        // SET_LEDPARAM: [cmd=0x07, mode=1(Static), speed_inv=1, brightness=4, option=8, r=255, g=128, b=64]
        let data = [0x07, 0x01, 0x01, 0x04, 0x08, 0xff, 0x80, 0x40];
        let parsed = try_parse_command(&data);
        match parsed {
            ParsedCommand::SetLedParams(led) => {
                assert_eq!(led.mode, LedMode::Constant);
                assert_eq!(led.speed, 3); // 4 - 1 = 3 (inverted back)
                assert_eq!(led.brightness, 4);
                assert_eq!(led.color.r, 255);
                assert_eq!(led.color.g, 128);
                assert_eq!(led.color.b, 64);
            }
            _ => panic!("Expected SetLedParams, got {:?}", parsed),
        }
    }

    #[test]
    fn test_try_parse_command_profile() {
        let data = [0x04, 0x02]; // SET_PROFILE, profile=2
        let parsed = try_parse_command(&data);
        match parsed {
            ParsedCommand::SetProfile(p) => {
                assert_eq!(p.profile, 2);
            }
            _ => panic!("Expected SetProfile, got {:?}", parsed),
        }
    }

    #[test]
    fn test_try_parse_command_polling_rate() {
        let data = [0x03, 0x03]; // SET_REPORT, rate=3 (1000Hz)
        let parsed = try_parse_command(&data);
        match parsed {
            ParsedCommand::SetPollingRate(r) => {
                assert_eq!(r.rate.to_hz(), 1000);
            }
            _ => panic!("Expected SetPollingRate, got {:?}", parsed),
        }
    }

    #[test]
    fn test_try_parse_command_screen_color() {
        let data = [0x0e, 0x67, 0x67, 0x67]; // SET_SCREEN_COLOR, RGB
        let parsed = try_parse_command(&data);
        match parsed {
            ParsedCommand::SetScreenColor { r, g, b } => {
                assert_eq!(r, 0x67);
                assert_eq!(g, 0x67);
                assert_eq!(b, 0x67);
            }
            _ => panic!("Expected SetScreenColor, got {:?}", parsed),
        }
    }

    #[test]
    fn test_try_parse_command_get_commands() {
        // GET commands should parse to simple variants
        assert!(matches!(
            try_parse_command(&[0x87]),
            ParsedCommand::GetLedParams
        ));
        assert!(matches!(
            try_parse_command(&[0x8f]),
            ParsedCommand::GetUsbVersion
        ));
        assert!(matches!(
            try_parse_command(&[0xf7]),
            ParsedCommand::GetDongleStatus
        ));
        assert!(matches!(
            try_parse_command(&[0xfc]),
            ParsedCommand::GetCachedResponse
        ));
    }

    // =========================================================================
    // Typed Packet Struct Tests
    // =========================================================================

    #[test]
    fn test_set_key_matrix_data_size_and_layout() {
        assert_eq!(std::mem::size_of::<SetKeyMatrixData>(), 11);
        let pkt = SetKeyMatrixData::new(1, 42, 0, true, [9, 0, 5, 0]).unwrap();
        let bytes = pkt.as_bytes();
        assert_eq!(bytes[0], 1); // profile
        assert_eq!(bytes[1], 42); // key_index
        assert_eq!(bytes[2], 0); // pad0
        assert_eq!(bytes[3], 0); // pad1
        assert_eq!(bytes[4], 1); // enabled
        assert_eq!(bytes[5], 0); // layer
        assert_eq!(bytes[6], 0); // checksum placeholder
        assert_eq!(bytes[7], 9); // config_type
        assert_eq!(bytes[8], 0); // b1
        assert_eq!(bytes[9], 5); // b2
        assert_eq!(bytes[10], 0); // b3
    }

    #[test]
    fn test_set_key_matrix_rejects_bad_profile() {
        assert!(SetKeyMatrixData::new(4, 0, 0, true, [0; 4]).is_err());
    }

    #[test]
    fn test_set_key_matrix_rejects_bad_key_index() {
        assert!(SetKeyMatrixData::new(0, 126, 0, true, [0; 4]).is_err());
    }

    #[test]
    fn test_set_key_matrix_rejects_bad_layer() {
        assert!(SetKeyMatrixData::new(0, 0, 3, true, [0; 4]).is_err());
    }

    #[test]
    fn test_set_key_matrix_accepts_boundary_values() {
        assert!(SetKeyMatrixData::new(3, 125, 2, true, [0; 4]).is_ok());
    }

    #[test]
    fn test_set_fn_data_size_and_layout() {
        assert_eq!(std::mem::size_of::<SetFnData>(), 11);
        let pkt = SetFnData::new(0, 2, 10, [9, 1, 3, 0]).unwrap();
        let bytes = pkt.as_bytes();
        assert_eq!(bytes[0], 0); // fn_sys (win)
        assert_eq!(bytes[1], 2); // profile
        assert_eq!(bytes[2], 10); // key_index
        assert_eq!(bytes[3], 0); // pad1
        assert_eq!(bytes[4], 0); // pad2
        assert_eq!(bytes[5], 0); // pad3
        assert_eq!(bytes[6], 0); // checksum placeholder
        assert_eq!(bytes[7], 9); // config_type
        assert_eq!(bytes[8], 1); // b1
        assert_eq!(bytes[9], 3); // b2
        assert_eq!(bytes[10], 0); // b3
    }

    #[test]
    fn test_set_fn_rejects_bad_key_index() {
        assert!(SetFnData::new(0, 0, 126, [0; 4]).is_err());
    }

    #[test]
    fn test_get_key_matrix_data_size_and_layout() {
        assert_eq!(std::mem::size_of::<GetKeyMatrixData>(), 4);
        let pkt = GetKeyMatrixData {
            profile: 0,
            magic: 0xFF,
            page: 3,
            magnetism_profile: 0,
        };
        let bytes = pkt.as_bytes();
        assert_eq!(bytes, &[0, 0xFF, 3, 0]);
    }

    #[test]
    fn test_get_fn_data_size_and_layout() {
        assert_eq!(std::mem::size_of::<GetFnData>(), 4);
        let pkt = GetFnData {
            sys: 0,
            profile: 1,
            magic: 0xFF,
            page: 2,
        };
        let bytes = pkt.as_bytes();
        assert_eq!(bytes, &[0, 1, 0xFF, 2]);
    }

    #[test]
    fn test_set_macro_header_size_and_layout() {
        assert_eq!(std::mem::size_of::<SetMacroHeader>(), 7);
        let cmd = SetMacroCommand::new(3, 1, false, vec![0u8; 56]).unwrap();
        let data = cmd.to_data();
        assert_eq!(data[0], 3); // macro_index
        assert_eq!(data[1], 1); // page
        assert_eq!(data[2], 56); // chunk_len
        assert_eq!(data[3], 0); // is_last
        assert_eq!(data[6], 0); // checksum placeholder
    }

    #[test]
    fn test_set_macro_command_to_data() {
        let cmd = SetMacroCommand::new(0, 0, true, vec![0xAA, 0xBB, 0xCC]).unwrap();
        let data = cmd.to_data();
        assert_eq!(data.len(), 10); // 7 header + 3 payload
        assert_eq!(data[0], 0); // macro_index
        assert_eq!(data[3], 1); // is_last
        assert_eq!(data[7], 0xAA); // first payload byte
        assert_eq!(data[9], 0xCC); // last payload byte
    }

    #[test]
    fn test_set_macro_rejects_bad_index() {
        assert!(SetMacroCommand::new(50, 0, true, vec![0; 56]).is_err());
    }

    #[test]
    fn test_set_macro_rejects_bad_page() {
        assert!(SetMacroCommand::new(0, 10, true, vec![0; 56]).is_err());
    }

    #[test]
    fn test_set_macro_rejects_oversized_payload() {
        assert!(SetMacroCommand::new(0, 0, true, vec![0; 57]).is_err());
    }

    #[test]
    fn test_set_macro_accepts_boundary_values() {
        assert!(SetMacroCommand::new(49, 9, true, vec![0; 56]).is_ok());
    }

    #[test]
    fn test_get_macro_data_size_and_layout() {
        assert_eq!(std::mem::size_of::<GetMacroData>(), 2);
        let pkt = GetMacroData {
            macro_index: 3,
            page: 2,
        };
        assert_eq!(pkt.as_bytes(), &[3, 2]);
    }

    #[test]
    fn test_set_multi_magnetism_header_size_and_layout() {
        assert_eq!(std::mem::size_of::<SetMultiMagnetismHeader>(), 7);
        let hdr = SetMultiMagnetismHeader {
            sub_cmd: 0x01,
            flag: 1,
            page: 2,
            commit: 1,
            _pad0: 0,
            _pad1: 0,
            _checksum: 0,
        };
        let bytes = hdr.as_bytes();
        assert_eq!(bytes[0], 0x01); // sub_cmd
        assert_eq!(bytes[1], 1); // flag
        assert_eq!(bytes[2], 2); // page
        assert_eq!(bytes[3], 1); // commit
    }

    #[test]
    fn test_get_multi_magnetism_data_size_and_layout() {
        assert_eq!(std::mem::size_of::<GetMultiMagnetismData>(), 3);
        let pkt = GetMultiMagnetismData {
            sub_cmd: 0x0A,
            flag: 1,
            page: 0,
        };
        assert_eq!(pkt.as_bytes(), &[0x0A, 1, 0]);
    }

    #[test]
    fn test_set_key_magnetism_mode_data_size_and_layout() {
        assert_eq!(std::mem::size_of::<SetKeyMagnetismModeData>(), 4);
        let pkt = SetKeyMagnetismModeData {
            key_index: 15,
            actuation: 200,
            deactuation: 150,
            mode: 1,
        };
        assert_eq!(pkt.as_bytes(), &[15, 200, 150, 1]);
    }

    #[test]
    fn test_key_config_entry_size_and_roundtrip() {
        assert_eq!(std::mem::size_of::<KeyConfigEntry>(), 4);
        // Parse from bytes
        let bytes = [9u8, 0, 5, 0]; // config_type=Macro, b2=macro_index=5
        let entry = KeyConfigEntry::read_from_bytes(&bytes).unwrap();
        assert_eq!(entry.config_type, 9);
        assert_eq!(entry.b2, 5);
        // Roundtrip
        assert_eq!(entry.as_bytes(), &bytes);
    }

    #[test]
    fn test_set_key_matrix_hid_command_trait() {
        assert_eq!(SetKeyMatrixData::CMD, cmd::SET_KEYMATRIX);
        assert_eq!(SetKeyMatrixData::CHECKSUM, ChecksumType::Bit7);
        let pkt = SetKeyMatrixData::new(0, 1, 0, true, [0, 0, 0x04, 0]).unwrap();
        let buf = pkt.build();
        assert_eq!(buf.len(), REPORT_SIZE);
        assert_eq!(buf[0], 0); // report ID
        assert_eq!(buf[1], cmd::SET_KEYMATRIX); // command byte
        assert_eq!(buf[2], 0); // profile
        assert_eq!(buf[3], 1); // key_index
    }

    #[test]
    fn test_set_fn_hid_command_trait() {
        assert_eq!(SetFnData::CMD, cmd::SET_FN);
        let pkt = SetFnData::new(0, 0, 5, [9, 0, 2, 0]).unwrap();
        let buf = pkt.build();
        assert_eq!(buf.len(), REPORT_SIZE);
        assert_eq!(buf[1], cmd::SET_FN);
        assert_eq!(buf[2], 0); // fn_sys at byte 0 of data = buf[2]
        assert_eq!(buf[3], 0); // profile at byte 1 of data = buf[3]
        assert_eq!(buf[4], 5); // key_index at byte 2 of data = buf[4]
    }

    #[test]
    fn test_set_key_magnetism_mode_hid_command_trait() {
        assert_eq!(SetKeyMagnetismModeData::CMD, cmd::SET_KEY_MAGNETISM_MODE);
        let pkt = SetKeyMagnetismModeData {
            key_index: 10,
            actuation: 200,
            deactuation: 150,
            mode: 1,
        };
        let buf = pkt.build();
        assert_eq!(buf.len(), REPORT_SIZE);
        assert_eq!(buf[1], cmd::SET_KEY_MAGNETISM_MODE);
        assert_eq!(buf[2], 10); // key_index
        assert_eq!(buf[3], 200); // actuation
        assert_eq!(buf[4], 150); // deactuation
        assert_eq!(buf[5], 1); // mode
    }
}
