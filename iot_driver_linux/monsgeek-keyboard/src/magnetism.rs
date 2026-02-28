//! Magnetism (Hall Effect) related types for trigger settings

use crate::settings::Precision;

/// Travel distance in raw firmware units
///
/// Provides type-safe conversion to/from millimeters based on firmware precision.
/// The raw value is stored as u16 and represents travel distance in precision-dependent units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TravelDepth(u16);

impl TravelDepth {
    /// Create from raw firmware value
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    /// Create from millimeters using the given precision
    pub fn from_mm(mm: f32, precision: Precision) -> Self {
        Self(precision.mm_to_raw(mm as f64))
    }

    /// Get the raw firmware value
    pub const fn raw(&self) -> u16 {
        self.0
    }

    /// Convert to millimeters using the given precision
    pub fn to_mm(&self, precision: Precision) -> f32 {
        precision.raw_to_mm(self.0) as f32
    }

    /// Format as string with mm suffix (e.g., "1.50mm")
    pub fn format(&self, precision: Precision) -> String {
        format!("{:.2}mm", self.to_mm(precision))
    }
}

impl From<u16> for TravelDepth {
    fn from(raw: u16) -> Self {
        Self(raw)
    }
}

impl From<TravelDepth> for u16 {
    fn from(depth: TravelDepth) -> Self {
        depth.0
    }
}

/// Key depth event from magnetism report
#[derive(Debug, Clone)]
pub struct KeyDepthEvent {
    /// Key matrix index
    pub key_index: u8,
    /// Raw depth value from sensor
    pub depth_raw: u16,
    /// Depth in mm (requires precision factor)
    pub depth_mm: f32,
}

/// Per-key trigger settings
#[derive(Debug, Clone, Default)]
pub struct TriggerSettings {
    /// Number of keys
    pub key_count: usize,
    /// Press travel (actuation point) per key, in raw u16 firmware units
    pub press_travel: Vec<u16>,
    /// Lift travel (release point) per key
    pub lift_travel: Vec<u16>,
    /// Rapid Trigger press sensitivity per key
    pub rt_press: Vec<u16>,
    /// Rapid Trigger lift sensitivity per key
    pub rt_lift: Vec<u16>,
    /// Key mode per key (0=Normal, 2=DKS, 3=MT, etc.)
    pub key_modes: Vec<u8>,
    /// Bottom deadzone per key
    pub bottom_deadzone: Vec<u16>,
    /// Top deadzone per key
    pub top_deadzone: Vec<u16>,
}

impl TriggerSettings {
    /// Create empty settings for a given key count
    pub fn new(key_count: usize) -> Self {
        Self {
            key_count,
            press_travel: vec![0; key_count],
            lift_travel: vec![0; key_count],
            rt_press: vec![0; key_count],
            rt_lift: vec![0; key_count],
            key_modes: vec![0; key_count],
            bottom_deadzone: vec![0; key_count],
            top_deadzone: vec![0; key_count],
        }
    }

    /// Decode a raw byte buffer of LE u16 pairs into a Vec<u16>
    pub fn decode_u16_values(bytes: &[u8], key_count: usize) -> Vec<u16> {
        bytes
            .chunks_exact(2)
            .take(key_count)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    }

    /// Get settings for a specific key
    pub fn get_key(&self, index: usize) -> Option<KeyTriggerSettingsDetail> {
        if index >= self.key_count {
            return None;
        }
        Some(KeyTriggerSettingsDetail {
            press_travel: self.press_travel.get(index).copied().unwrap_or(0),
            lift_travel: self.lift_travel.get(index).copied().unwrap_or(0),
            rt_press: self.rt_press.get(index).copied().unwrap_or(0),
            rt_lift: self.rt_lift.get(index).copied().unwrap_or(0),
            key_mode: KeyMode::from_u8(self.key_modes.get(index).copied().unwrap_or(0)),
            bottom_deadzone: self.bottom_deadzone.get(index).copied().unwrap_or(0),
            top_deadzone: self.top_deadzone.get(index).copied().unwrap_or(0),
        })
    }
}

/// Simple trigger settings for single key get/set operations
#[derive(Debug, Clone, Default)]
pub struct KeyTriggerSettings {
    /// Key matrix index
    pub key_index: u8,
    /// Actuation point (raw units)
    pub actuation: u8,
    /// Deactuation point (raw units)
    pub deactuation: u8,
    /// Key mode
    pub mode: KeyMode,
}

/// Detailed settings for a single key (from bulk query)
#[derive(Debug, Clone, Default)]
pub struct KeyTriggerSettingsDetail {
    /// Press travel (actuation point) in raw u16 firmware units
    pub press_travel: u16,
    /// Lift travel (release point)
    pub lift_travel: u16,
    /// Rapid Trigger press sensitivity
    pub rt_press: u16,
    /// Rapid Trigger lift sensitivity
    pub rt_lift: u16,
    /// Key mode
    pub key_mode: KeyMode,
    /// Bottom deadzone
    pub bottom_deadzone: u16,
    /// Top deadzone
    pub top_deadzone: u16,
}

/// Key trigger mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyMode {
    /// Normal mode - simple actuation/release points
    #[default]
    Normal,
    /// Dynamic Keystroke (DKS)
    DynamicKeystroke,
    /// Mod-Tap
    ModTap,
    /// Toggle Hold
    ToggleHold,
    /// Toggle Dots
    ToggleDots,
    /// Snap Tap
    SnapTap,
    /// Rapid Trigger enabled
    RapidTrigger,
    /// Unknown mode
    Unknown(u8),
}

impl KeyMode {
    /// Parse from protocol value
    pub fn from_u8(value: u8) -> Self {
        let base = value & 0x7F;
        let rt = value & 0x80 != 0;

        if rt {
            Self::RapidTrigger
        } else {
            match base {
                0 => Self::Normal,
                2 => Self::DynamicKeystroke,
                3 => Self::ModTap,
                4 => Self::ToggleHold,
                5 => Self::ToggleDots,
                7 => Self::SnapTap,
                _ => Self::Unknown(value),
            }
        }
    }

    /// Convert to protocol value
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::DynamicKeystroke => 2,
            Self::ModTap => 3,
            Self::ToggleHold => 4,
            Self::ToggleDots => 5,
            Self::SnapTap => 7,
            Self::RapidTrigger => 0x80,
            Self::Unknown(v) => v,
        }
    }
}
