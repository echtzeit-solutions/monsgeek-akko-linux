//! Type-safe HID command builders and response parsers
//!
//! This module provides a cleaner API for building HID commands and parsing responses,
//! handling protocol quirks (checksums, byte ordering, value transformations) in one place.

use crate::protocol::{self, cmd};
use crate::types::ChecksumType;

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
    UserColor = 25,
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
            25 => Some(Self::UserColor),
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

/// LED dazzle (rainbow cycle) option
const DAZZLE_ON: u8 = 7;
const DAZZLE_OFF: u8 = 8;
const SPEED_MAX: u8 = 4;
const BRIGHTNESS_MAX: u8 = 4;

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
#[derive(Debug, Clone)]
pub struct SetSleepTime {
    pub seconds: u16,
}

impl SetSleepTime {
    pub fn new(seconds: u16) -> Self {
        Self { seconds }
    }

    pub fn from_minutes(minutes: u16) -> Self {
        Self {
            seconds: minutes * 60,
        }
    }

    /// Disable sleep (set to 0)
    pub fn disabled() -> Self {
        Self { seconds: 0 }
    }
}

impl HidCommand for SetSleepTime {
    const CMD: u8 = cmd::SET_SLEEPTIME;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;

    fn to_data(&self) -> Vec<u8> {
        self.seconds.to_le_bytes().to_vec()
    }
}

/// GET_SLEEPTIME response
#[derive(Debug, Clone)]
pub struct SleepTimeResponse {
    pub seconds: u16,
}

impl SleepTimeResponse {
    pub fn minutes(&self) -> u16 {
        self.seconds / 60
    }

    pub fn is_disabled(&self) -> bool {
        self.seconds == 0
    }
}

impl HidResponse for SleepTimeResponse {
    const CMD_ECHO: u8 = cmd::GET_SLEEPTIME;
    const MIN_LEN: usize = 3;

    fn from_data(data: &[u8]) -> Result<Self, ParseError> {
        let seconds = u16::from_le_bytes([data[1], data[2]]);
        Ok(Self { seconds })
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
// Battery (Dongle)
// =============================================================================

/// BATTERY_REFRESH command (0xF7) - for wireless dongles
#[derive(Debug, Clone, Default)]
pub struct BatteryRefresh;

impl HidCommand for BatteryRefresh {
    const CMD: u8 = cmd::BATTERY_REFRESH;
    const CHECKSUM: ChecksumType = ChecksumType::Bit7;

    fn to_data(&self) -> Vec<u8> {
        vec![]
    }
}

/// Battery response from dongle
#[derive(Debug, Clone)]
pub struct BatteryResponse {
    pub level: u8,    // 0-100%
    pub online: bool, // Keyboard connected to dongle
    pub idle: bool,   // Keyboard is idle (no recent activity)
}

impl HidResponse for BatteryResponse {
    const CMD_ECHO: u8 = 0x01; // Battery response has 0x01 as first byte, not command echo
    const MIN_LEN: usize = 5;

    fn from_data(data: &[u8]) -> Result<Self, ParseError> {
        // Battery response format (65 bytes including Report ID):
        // [0] = Report ID (0x00)
        // [1] = battery level (0-100%)
        // [2] = unknown (always 0x00)
        // [3] = idle flag (1 = idle/sleeping, 0 = active/recently pressed)
        // [4] = online flag (1 = connected)
        // [5-6] = unknown (both 0x01)
        // [7+] = padding (0x00)
        let level = data[1];
        if level > 100 {
            return Err(ParseError::InvalidValue {
                field: "battery_level",
                value: level,
            });
        }
        Ok(Self {
            level,
            online: data[4] != 0,
            idle: data.len() > 3 && data[3] != 0,
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

use crate::{Transport, TransportError};
use async_trait::async_trait;

/// Extension trait for sending typed commands via Transport
#[async_trait]
pub trait TransportExt: Transport {
    /// Send a typed command (fire-and-forget)
    async fn send<C: HidCommand + Send + Sync>(&self, cmd: &C) -> Result<(), TransportError> {
        self.send_command(C::CMD, &cmd.to_data(), C::CHECKSUM).await
    }

    /// Query and parse a typed response (validates command echo)
    async fn query<C, R>(&self, cmd: &C) -> Result<R, TransportError>
    where
        C: HidCommand + Send + Sync,
        R: HidResponse,
    {
        let resp = self
            .query_command(C::CMD, &cmd.to_data(), C::CHECKSUM)
            .await?;
        R::parse(&resp).map_err(|e| match e {
            ParseError::CommandMismatch { expected, got } => TransportError::InvalidResponse {
                expected,
                actual: got,
            },
            _ => TransportError::Internal(e.to_string()),
        })
    }

    /// Query without command echo validation (for special responses like battery)
    async fn query_no_echo<C, R>(&self, cmd: &C) -> Result<R, TransportError>
    where
        C: HidCommand + Send + Sync,
        R: HidResponse,
    {
        let resp = self.query_raw(C::CMD, &cmd.to_data(), C::CHECKSUM).await?;
        R::parse(&resp).map_err(|e| TransportError::Internal(e.to_string()))
    }
}

// Blanket implementation for all Transport implementations
impl<T: Transport + ?Sized> TransportExt for T {}

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
        let cmd = SetSleepTime::from_minutes(10);
        let data = cmd.to_data();
        assert_eq!(data, [0x58, 0x02]); // 600 seconds = 0x0258 LE

        let resp_data = [0x91, 0x58, 0x02];
        let resp = SleepTimeResponse::parse(&resp_data).unwrap();
        assert_eq!(resp.seconds, 600);
        assert_eq!(resp.minutes(), 10);
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
}
