//! Keyboard settings types

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

    /// Get precision factor based on firmware version
    /// Newer firmware has higher precision for travel settings
    pub fn precision_factor(&self) -> f64 {
        if self.raw >= 1280 {
            200.0 // 0.005mm precision
        } else if self.raw >= 768 {
            100.0 // 0.01mm precision
        } else {
            10.0 // 0.1mm precision
        }
    }

    /// Get precision string (e.g., "0.01mm")
    pub fn precision_str(&self) -> &'static str {
        if self.raw >= 1280 {
            "0.005mm"
        } else if self.raw >= 768 {
            "0.01mm"
        } else {
            "0.1mm"
        }
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
        if version >= 1280 {
            200.0 // 0.005mm precision
        } else if version >= 768 {
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

/// Device feature list
#[derive(Debug, Clone, Default)]
pub struct FeatureList {
    /// Precision factor for trigger settings
    pub precision: u8,
    /// Raw feature flags
    pub raw_features: Vec<u8>,
}

impl FeatureList {
    /// Parse from GET_FEATURE_LIST response bytes
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            precision: bytes.first().copied().unwrap_or(0),
            raw_features: bytes.to_vec(),
        }
    }

    /// Get the precision factor (10, 100, or 200)
    pub fn precision_factor(&self) -> f64 {
        match self.precision {
            2 => 200.0, // 0.005mm
            1 => 100.0, // 0.01mm
            _ => 10.0,  // 0.1mm
        }
    }
}
