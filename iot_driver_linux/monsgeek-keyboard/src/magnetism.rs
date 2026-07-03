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
        let mode_byte = ModeByte::from_u8(self.key_modes.get(index).copied().unwrap_or(0));
        Some(KeyTriggerSettingsDetail {
            press_travel: self.press_travel.get(index).copied().unwrap_or(0),
            lift_travel: self.lift_travel.get(index).copied().unwrap_or(0),
            rt_press: self.rt_press.get(index).copied().unwrap_or(0),
            rt_lift: self.rt_lift.get(index).copied().unwrap_or(0),
            key_mode: mode_byte.base,
            rapid_trigger: mode_byte.rapid_trigger,
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
    /// Base key mode
    pub mode: KeyMode,
    /// Rapid-Trigger flag (orthogonal to `mode`)
    pub rapid_trigger: bool,
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
    /// Base key mode
    pub key_mode: KeyMode,
    /// Rapid-Trigger flag (orthogonal to `key_mode`)
    pub rapid_trigger: bool,
    /// Bottom deadzone
    pub bottom_deadzone: u16,
    /// Top deadzone
    pub top_deadzone: u16,
}

/// Per-key base trigger mode — the low 7 bits of the firmware mode byte.
///
/// The Rapid-Trigger ("fire") flag is the orthogonal `0x80` bit and combines
/// with *any* base mode; it is modelled separately by [`ModeByte`]. Values match
/// the official webapp decoder (`CommonKBRY5088.js::_decodeMagnetismKeyModes`).
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
    /// Snap Tap (SOCD)
    SnapTap,
    /// Unrecognized base mode (low 7 bits)
    Unknown(u8),
}

impl KeyMode {
    /// Every configurable base mode, in display/cycle order.
    pub const ALL: [KeyMode; 6] = [
        Self::Normal,
        Self::DynamicKeystroke,
        Self::ModTap,
        Self::ToggleHold,
        Self::ToggleDots,
        Self::SnapTap,
    ];

    /// Parse the base mode from the low 7 bits (ignores the `0x80` RT flag).
    pub fn from_u8(value: u8) -> Self {
        match value & 0x7F {
            0 => Self::Normal,
            2 => Self::DynamicKeystroke,
            3 => Self::ModTap,
            4 => Self::ToggleHold,
            5 => Self::ToggleDots,
            7 => Self::SnapTap,
            other => Self::Unknown(other),
        }
    }

    /// Base mode value (low 7 bits; no RT flag).
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::DynamicKeystroke => 2,
            Self::ModTap => 3,
            Self::ToggleHold => 4,
            Self::ToggleDots => 5,
            Self::SnapTap => 7,
            Self::Unknown(v) => v & 0x7F,
        }
    }

    /// Human-readable label — the single source of truth for mode naming.
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::DynamicKeystroke => "DKS",
            Self::ModTap => "Mod-Tap",
            Self::ToggleHold => "Toggle-Hold",
            Self::ToggleDots => "Toggle-Dots",
            Self::SnapTap => "SnapTap",
            Self::Unknown(_) => "Unknown",
        }
    }
}

impl std::fmt::Display for KeyMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A full per-key mode byte: a [`KeyMode`] base plus the orthogonal
/// Rapid-Trigger flag (`0x80`). RT can be enabled on top of any base mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModeByte {
    /// Base mode (low 7 bits)
    pub base: KeyMode,
    /// Rapid-Trigger ("fire") flag (`0x80`)
    pub rapid_trigger: bool,
}

impl ModeByte {
    /// Rapid-Trigger flag bit.
    pub const RT_FLAG: u8 = 0x80;

    pub fn new(base: KeyMode, rapid_trigger: bool) -> Self {
        Self {
            base,
            rapid_trigger,
        }
    }

    /// Split a raw mode byte into base mode + RT flag.
    pub fn from_u8(value: u8) -> Self {
        Self {
            base: KeyMode::from_u8(value),
            rapid_trigger: value & Self::RT_FLAG != 0,
        }
    }

    /// Combine base mode + RT flag back into the raw mode byte.
    pub fn to_u8(self) -> u8 {
        self.base.to_u8() | if self.rapid_trigger { Self::RT_FLAG } else { 0 }
    }
}

impl std::fmt::Display for ModeByte {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.base.label())?;
        if self.rapid_trigger {
            f.write_str("+RT")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_byte_round_trips_all_bases_and_rt() {
        for base in KeyMode::ALL {
            for rapid_trigger in [false, true] {
                let mb = ModeByte::new(base, rapid_trigger);
                let byte = mb.to_u8();
                assert_eq!(byte & ModeByte::RT_FLAG != 0, rapid_trigger);
                assert_eq!(ModeByte::from_u8(byte), mb);
                // Base value never sets the RT bit itself.
                assert_eq!(base.to_u8() & ModeByte::RT_FLAG, 0);
            }
        }
    }

    #[test]
    fn known_wire_values() {
        // Values per CommonKBRY5088.js::_decodeMagnetismKeyModes.
        assert_eq!(KeyMode::Normal.to_u8(), 0);
        assert_eq!(KeyMode::DynamicKeystroke.to_u8(), 2);
        assert_eq!(KeyMode::ModTap.to_u8(), 3);
        assert_eq!(KeyMode::ToggleHold.to_u8(), 4);
        assert_eq!(KeyMode::ToggleDots.to_u8(), 5);
        assert_eq!(KeyMode::SnapTap.to_u8(), 7);
        // DKS + Rapid Trigger, the byte the old model collapsed to bare RT.
        let dks_rt = ModeByte::from_u8(0x82);
        assert_eq!(dks_rt.base, KeyMode::DynamicKeystroke);
        assert!(dks_rt.rapid_trigger);
        assert_eq!(dks_rt.to_u8(), 0x82);
    }

    #[test]
    fn unknown_base_preserved_without_rt_bit() {
        let mb = ModeByte::from_u8(0x86); // base 6 (unknown) + RT
        assert_eq!(mb.base, KeyMode::Unknown(6));
        assert!(mb.rapid_trigger);
        assert_eq!(mb.to_u8(), 0x86);
    }
}
