//! Keyboard settings types

/// Precision level for trigger/travel settings
///
/// Determines the resolution of travel distance measurements.
/// Higher precision allows finer control over actuation points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Precision {
    /// 0.1mm resolution (legacy/low precision)
    #[default]
    Coarse,
    /// 0.01mm resolution (standard precision)
    Medium,
    /// 0.005mm resolution (high precision)
    Fine,
}

impl Precision {
    /// Create from feature list precision byte
    ///
    /// The feature list response uses: 0 = 0.1mm, 1 = 0.05mm, 2 = 0.01mm
    /// Note: 0.05mm maps to Medium since we don't have a separate variant
    pub fn from_feature_byte(byte: u8) -> Self {
        match byte {
            2 => Self::Fine,   // 0.005mm - highest precision
            1 => Self::Medium, // 0.01mm (0.05mm in feature list)
            _ => Self::Coarse, // 0.1mm - default/legacy
        }
    }

    /// Create from firmware version
    ///
    /// Older firmware doesn't support feature list, so precision is
    /// inferred from version number thresholds.
    pub fn from_firmware_version(version: u16) -> Self {
        use monsgeek_transport::protocol::precision;
        if version >= precision::FINE_VERSION {
            Self::Fine // 0.005mm
        } else if version >= precision::MEDIUM_VERSION {
            Self::Medium // 0.01mm
        } else {
            Self::Coarse // 0.1mm
        }
    }

    /// Get the precision factor (multiplier for raw values)
    ///
    /// Raw travel values are multiplied by 1/factor to get mm.
    /// E.g., raw value 100 with factor 100 = 1.0mm
    pub fn factor(&self) -> f64 {
        match self {
            Self::Fine => 200.0,   // 0.005mm steps
            Self::Medium => 100.0, // 0.01mm steps
            Self::Coarse => 10.0,  // 0.1mm steps
        }
    }

    /// Get precision as display string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fine => "0.005mm",
            Self::Medium => "0.01mm",
            Self::Coarse => "0.1mm",
        }
    }

    /// Convert raw travel value to millimeters
    pub fn raw_to_mm(&self, raw: u16) -> f64 {
        raw as f64 / self.factor()
    }

    /// Convert millimeters to raw travel value
    pub fn mm_to_raw(&self, mm: f64) -> u16 {
        (mm * self.factor()).round() as u16
    }
}

/// Firmware version information
#[derive(Debug, Clone, Default)]
pub struct FirmwareVersion {
    /// Version as raw u16 (e.g., 1029 for v10.29)
    pub raw: u16,
}

impl FirmwareVersion {
    /// Create from raw version number
    pub fn new(raw: u16) -> Self {
        Self { raw }
    }

    /// Parse version from GET_REV response bytes (starting after cmd echo)
    /// Format: bytes 0-3 = device_id, bytes 7-8 = version (little-endian u16)
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() < 9 {
            return Self::default();
        }
        let raw = u16::from_le_bytes([bytes[7], bytes[8]]);
        Self { raw }
    }

    /// Get precision level based on firmware version
    pub fn precision(&self) -> Precision {
        Precision::from_firmware_version(self.raw)
    }

    /// Get precision factor based on firmware version
    /// Newer firmware has higher precision for travel settings
    pub fn precision_factor(&self) -> f64 {
        self.precision().factor()
    }

    /// Get precision string (e.g., "0.01mm")
    pub fn precision_str(&self) -> &'static str {
        self.precision().as_str()
    }

    /// Format as human-readable string (e.g., "v10.29" for raw=1029)
    pub fn format(&self) -> String {
        format!("v{}.{:02}", self.raw / 100, self.raw % 100)
    }

    /// Format as major.minor.patch (e.g., "4.0.5" for raw=0x405)
    pub fn format_dotted(&self) -> String {
        let major = (self.raw >> 8) & 0xF;
        let minor = (self.raw >> 4) & 0xF;
        let patch = self.raw & 0xF;
        format!("{major}.{minor}.{patch}")
    }

    /// Get precision factor from raw version number (static)
    pub fn precision_factor_from_raw(version: u16) -> f32 {
        use monsgeek_transport::protocol::precision;
        if version >= precision::FINE_VERSION {
            200.0 // 0.005mm precision
        } else if version >= precision::MEDIUM_VERSION {
            100.0 // 0.01mm precision
        } else {
            10.0 // 0.1mm precision
        }
    }

    /// Decode precision byte from feature list response
    /// Returns human-readable precision string
    pub fn precision_byte_str(precision: u8) -> &'static str {
        match precision {
            0 => "0.1mm",
            1 => "0.05mm",
            2 => "0.01mm",
            _ => "unknown",
        }
    }
}

/// Battery information (wireless only)
#[derive(Debug, Clone, Default)]
pub struct BatteryInfo {
    /// Battery level 0-100
    pub level: u8,
    /// Device is online/connected
    pub online: bool,
    /// Device is charging (may not be available)
    pub charging: bool,
    /// Device is idle (no recent key activity)
    pub idle: bool,
}

/// Polling rate options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum PollingRate {
    Hz125 = 125,
    Hz250 = 250,
    Hz500 = 500,
    Hz1000 = 1000,
    Hz2000 = 2000,
    Hz4000 = 4000,
    Hz8000 = 8000,
}

impl PollingRate {
    /// Get polling rate from Hz value
    pub fn from_hz(hz: u16) -> Option<Self> {
        match hz {
            125 => Some(Self::Hz125),
            250 => Some(Self::Hz250),
            500 => Some(Self::Hz500),
            1000 => Some(Self::Hz1000),
            2000 => Some(Self::Hz2000),
            4000 => Some(Self::Hz4000),
            8000 => Some(Self::Hz8000),
            _ => None,
        }
    }

    /// Get the protocol value for this rate
    pub fn to_protocol_value(self) -> u8 {
        match self {
            Self::Hz125 => 6,
            Self::Hz250 => 5,
            Self::Hz500 => 4,
            Self::Hz1000 => 3,
            Self::Hz2000 => 2,
            Self::Hz4000 => 1,
            Self::Hz8000 => 0,
        }
    }

    /// Parse from protocol value
    pub fn from_protocol_value(value: u8) -> Option<Self> {
        match value {
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
}

/// Keyboard options
#[derive(Debug, Clone, Default)]
pub struct KeyboardOptions {
    /// OS mode (0=Windows, 1=Mac)
    pub os_mode: u8,
    /// Fn layer setting
    pub fn_layer: u8,
    /// Anti-mistouch enabled
    pub anti_mistouch: bool,
    /// Rapid Trigger stability mode (0=off, 1-3=levels)
    pub rt_stability: u8,
    /// WASD/arrow swap
    pub wasd_swap: bool,
}

impl KeyboardOptions {
    /// Parse from GET_KBOPTION response bytes
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() < 8 {
            return Self::default();
        }
        Self {
            os_mode: bytes[0],
            fn_layer: bytes[1],
            anti_mistouch: bytes[2] != 0,
            rt_stability: bytes[3],
            wasd_swap: bytes[7] != 0,
        }
    }

    /// Convert to protocol bytes for SET_KBOPTION
    pub fn to_bytes(&self) -> [u8; 8] {
        [
            self.os_mode,
            self.fn_layer,
            if self.anti_mistouch { 1 } else { 0 },
            self.rt_stability,
            0, // Reserved
            0, // Reserved
            0, // Reserved
            if self.wasd_swap { 1 } else { 0 },
        ]
    }
}

/// Sleep time settings for wireless modes
///
/// Controls idle and deep sleep timeouts for Bluetooth and 2.4GHz connections.
/// Times are in seconds. Set to 0 to disable that particular timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepTimeSettings {
    /// Bluetooth idle timeout (seconds) - keyboard enters light sleep
    pub idle_bt: u16,
    /// 2.4GHz idle timeout (seconds) - keyboard enters light sleep
    pub idle_24g: u16,
    /// Bluetooth deep sleep timeout (seconds) - keyboard powers down further
    pub deep_bt: u16,
    /// 2.4GHz deep sleep timeout (seconds) - keyboard powers down further
    pub deep_24g: u16,
}

impl Default for SleepTimeSettings {
    fn default() -> Self {
        Self {
            idle_bt: 120,   // 2 minutes
            idle_24g: 120,  // 2 minutes
            deep_bt: 1680,  // 28 minutes
            deep_24g: 1680, // 28 minutes
        }
    }
}

impl SleepTimeSettings {
    /// Create new sleep time settings
    pub fn new(idle_bt: u16, idle_24g: u16, deep_bt: u16, deep_24g: u16) -> Self {
        Self {
            idle_bt,
            idle_24g,
            deep_bt,
            deep_24g,
        }
    }

    /// Create with same idle and deep timeout for both wireless modes
    pub fn uniform(idle_seconds: u16, deep_seconds: u16) -> Self {
        Self {
            idle_bt: idle_seconds,
            idle_24g: idle_seconds,
            deep_bt: deep_seconds,
            deep_24g: deep_seconds,
        }
    }

    /// Format idle timeout as human-readable duration
    pub fn format_idle(&self, is_bt: bool) -> String {
        let secs = if is_bt { self.idle_bt } else { self.idle_24g };
        Self::format_duration(secs)
    }

    /// Format deep sleep timeout as human-readable duration
    pub fn format_deep(&self, is_bt: bool) -> String {
        let secs = if is_bt { self.deep_bt } else { self.deep_24g };
        Self::format_duration(secs)
    }

    /// Format seconds as human-readable duration string
    pub fn format_duration(secs: u16) -> String {
        if secs == 0 {
            "disabled".to_string()
        } else if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            let mins = secs / 60;
            let rem = secs % 60;
            if rem == 0 {
                format!("{}m", mins)
            } else {
                format!("{}m {}s", mins, rem)
            }
        } else {
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;
            if mins == 0 {
                format!("{}h", hours)
            } else {
                format!("{}h {}m", hours, mins)
            }
        }
    }

    /// Parse duration string (e.g., "2m", "30s", "1h 30m") to seconds
    pub fn parse_duration(s: &str) -> Option<u16> {
        let s = s.trim().to_lowercase();

        // Handle "disabled" or "off"
        if s == "disabled" || s == "off" || s == "0" {
            return Some(0);
        }

        let mut total_secs: u32 = 0;
        let mut current_num = String::new();

        for c in s.chars() {
            if c.is_ascii_digit() {
                current_num.push(c);
            } else if !current_num.is_empty() {
                let num: u32 = current_num.parse().ok()?;
                current_num.clear();
                match c {
                    'h' => total_secs += num * 3600,
                    'm' => total_secs += num * 60,
                    's' => total_secs += num,
                    _ => return None,
                }
            }
        }

        // If there's a trailing number with no unit, treat as seconds
        if !current_num.is_empty() {
            let num: u32 = current_num.parse().ok()?;
            total_secs += num;
        }

        // Clamp to u16 max
        Some(total_secs.min(u16::MAX as u32) as u16)
    }
}

/// Device feature list
#[derive(Debug, Clone, Default)]
pub struct FeatureList {
    /// Precision factor for trigger settings
    pub precision: u8,
    /// Raw feature flags
    pub raw_features: Vec<u8>,
}

impl FeatureList {
    /// Parse from GET_FEATURE_LIST response bytes (after echo byte stripped)
    /// Response format: [0xAA validity marker, precision_enum, ...]
    /// If validity marker is not 0xAA, the response is invalid and precision defaults to 0xFF (unknown)
    pub fn from_bytes(bytes: &[u8]) -> Self {
        // Check validity marker (first byte should be 0xAA)
        let valid = bytes.first().copied() == Some(0xAA);
        Self {
            // Byte 0 = 0xAA validity marker, Byte 1 = precision enum
            // Use 0xFF to indicate unknown/invalid response
            precision: if valid {
                bytes.get(1).copied().unwrap_or(0xFF)
            } else {
                0xFF // Invalid response - will trigger fallback to firmware version
            },
            raw_features: bytes.to_vec(),
        }
    }

    /// Check if the feature list response was valid
    pub fn is_valid(&self) -> bool {
        self.precision != 0xFF
    }

    /// Get precision level from feature list
    ///
    /// Returns None if the feature list response was invalid (command not supported).
    /// Caller should fall back to firmware version in that case.
    pub fn precision(&self) -> Option<Precision> {
        if self.is_valid() {
            Some(Precision::from_feature_byte(self.precision))
        } else {
            None
        }
    }

    /// Get the precision factor (10, 100, or 200)
    pub fn precision_factor(&self) -> f64 {
        self.precision().map(|p| p.factor()).unwrap_or(10.0) // Default to coarse if invalid
    }
}
