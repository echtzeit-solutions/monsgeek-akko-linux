//! Configuration structures for the joystick mapper
//!
//! Supports TOML serialization for persistent config storage.
//! Key references use the shared `KeyRef` type from monsgeek-transport,
//! with custom serde that serializes as bare key name strings (e.g. `"W"`)
//! and accepts both the new format and the old `{ key_index, label }` format.

use monsgeek_transport::protocol::{matrix, KeyRef, Layer};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::PathBuf;

/// Joystick axis identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AxisId {
    X,
    Y,
    RX,
    RY,
    Z,
    RZ,
}

impl AxisId {
    /// Get display name for the axis
    pub fn display_name(&self) -> &'static str {
        match self {
            AxisId::X => "X",
            AxisId::Y => "Y",
            AxisId::RX => "RX",
            AxisId::RY => "RY",
            AxisId::Z => "Z",
            AxisId::RZ => "RZ",
        }
    }

    /// All available axis IDs
    pub const ALL: &'static [AxisId] = &[
        AxisId::X,
        AxisId::Y,
        AxisId::RX,
        AxisId::RY,
        AxisId::Z,
        AxisId::RZ,
    ];
}

// ---------------------------------------------------------------------------
// Custom serde for KeyRef — serialize as bare name, deserialize both formats
// ---------------------------------------------------------------------------

/// Serialize a `KeyRef` as its key name string (e.g. `"W"`, `"Caps"`).
fn serialize_keyref<S: Serializer>(key: &KeyRef, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(key.position)
}

/// Deserialize a `KeyRef` from either:
/// - New format: bare string `"W"` → resolved via `matrix::key_index_from_name`
/// - Old format: `{ key_index = 14, label = "W" }` → uses `key_index` directly
fn deserialize_keyref<'de, D: Deserializer<'de>>(d: D) -> Result<KeyRef, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum KeyRefRepr {
        /// New format: bare key name string
        Name(String),
        /// Old format: { key_index, label } — label accepted but ignored
        Legacy {
            key_index: u8,
            #[serde(default)]
            #[allow(dead_code)]
            label: String,
        },
    }

    match KeyRefRepr::deserialize(d)? {
        KeyRefRepr::Name(name) => {
            if let Some(idx) = matrix::key_index_from_name(&name) {
                Ok(KeyRef::new(idx, Layer::Base))
            } else if let Ok(idx) = name.parse::<u8>() {
                Ok(KeyRef::new(idx, Layer::Base))
            } else {
                Err(serde::de::Error::custom(format!(
                    "unknown key name: \"{name}\""
                )))
            }
        }
        KeyRefRepr::Legacy { key_index, .. } => Ok(KeyRef::new(key_index, Layer::Base)),
    }
}

/// How an axis is mapped from key(s) to joystick value
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AxisMappingMode {
    /// Two keys control positive and negative directions
    /// e.g., W/S for Y-axis: W pushes positive, S pushes negative
    TwoKey {
        #[serde(
            serialize_with = "serialize_keyref",
            deserialize_with = "deserialize_keyref"
        )]
        positive_key: KeyRef,
        #[serde(
            serialize_with = "serialize_keyref",
            deserialize_with = "deserialize_keyref"
        )]
        negative_key: KeyRef,
    },
    /// Single key controls 0 to max (throttle-style)
    SingleKey {
        #[serde(
            serialize_with = "serialize_keyref",
            deserialize_with = "deserialize_keyref"
        )]
        key: KeyRef,
        /// If true, inverts the axis (pressed = 0, released = max)
        invert: bool,
    },
}

impl AxisMappingMode {
    /// Get all key indices used by this mapping
    pub fn key_indices(&self) -> Vec<u8> {
        match self {
            AxisMappingMode::TwoKey {
                positive_key,
                negative_key,
            } => vec![positive_key.index, negative_key.index],
            AxisMappingMode::SingleKey { key, .. } => vec![key.index],
        }
    }
}

/// Calibration settings for an axis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxisCalibration {
    /// Minimum key travel in mm (typically 0.0)
    #[serde(default)]
    pub min_travel_mm: f32,
    /// Maximum key travel in mm (typically 3.5-4.0)
    #[serde(default = "default_max_travel")]
    pub max_travel_mm: f32,
    /// Deadzone as percentage of travel (0-100)
    #[serde(default = "default_deadzone")]
    pub deadzone_percent: f32,
    /// Response curve exponent (1.0 = linear, >1 = less sensitive in center)
    #[serde(default = "default_curve")]
    pub curve_exponent: f32,
}

fn default_max_travel() -> f32 {
    4.0
}
fn default_deadzone() -> f32 {
    5.0
}
fn default_curve() -> f32 {
    1.0
}

impl Default for AxisCalibration {
    fn default() -> Self {
        Self {
            min_travel_mm: 0.0,
            max_travel_mm: default_max_travel(),
            deadzone_percent: default_deadzone(),
            curve_exponent: default_curve(),
        }
    }
}

/// Complete configuration for a single axis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxisConfig {
    /// Which joystick axis this configures
    pub id: AxisId,
    /// Whether this axis is active
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// How keys map to this axis
    pub mapping: AxisMappingMode,
    /// Calibration and response curve settings
    #[serde(default)]
    pub calibration: AxisCalibration,
}

fn default_true() -> bool {
    true
}

/// Complete joystick mapper configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoystickConfig {
    /// Name for the virtual joystick device
    #[serde(default = "default_device_name")]
    pub device_name: String,
    /// Axis configurations
    #[serde(default)]
    pub axes: Vec<AxisConfig>,
}

fn default_device_name() -> String {
    "MonsGeek Virtual Joystick".to_string()
}

impl Default for JoystickConfig {
    fn default() -> Self {
        Self {
            device_name: default_device_name(),
            axes: vec![
                // Default WASD mapping for X/Y axes
                AxisConfig {
                    id: AxisId::X,
                    enabled: true,
                    mapping: AxisMappingMode::TwoKey {
                        positive_key: KeyRef::new(21, Layer::Base),
                        negative_key: KeyRef::new(9, Layer::Base),
                    },
                    calibration: AxisCalibration::default(),
                },
                AxisConfig {
                    id: AxisId::Y,
                    enabled: true,
                    mapping: AxisMappingMode::TwoKey {
                        positive_key: KeyRef::new(14, Layer::Base),
                        negative_key: KeyRef::new(15, Layer::Base),
                    },
                    calibration: AxisCalibration::default(),
                },
            ],
        }
    }
}

impl JoystickConfig {
    /// Get the default config file path
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("monsgeek")
            .join("joystick.toml")
    }

    /// Load config from a file, or return default if not found
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            let config: JoystickConfig = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    /// Save config to a file
    pub fn save(&self, path: &PathBuf) -> anyhow::Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get axis config by ID
    pub fn get_axis(&self, id: AxisId) -> Option<&AxisConfig> {
        self.axes.iter().find(|a| a.id == id)
    }

    /// Get mutable axis config by ID
    pub fn get_axis_mut(&mut self, id: AxisId) -> Option<&mut AxisConfig> {
        self.axes.iter_mut().find(|a| a.id == id)
    }

    /// Get all key indices that are currently mapped
    pub fn mapped_key_indices(&self) -> Vec<u8> {
        self.axes
            .iter()
            .filter(|a| a.enabled)
            .flat_map(|a| a.mapping.key_indices())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_serializes() {
        let config = JoystickConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("MonsGeek Virtual Joystick"));
        assert!(toml_str.contains("TwoKey"));
        // New format: bare key names
        assert!(toml_str.contains("positive_key = \"D\""));
        assert!(toml_str.contains("negative_key = \"A\""));
    }

    #[test]
    fn test_roundtrip() {
        let config = JoystickConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: JoystickConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.device_name, config.device_name);
        assert_eq!(parsed.axes.len(), config.axes.len());
        // Verify key indices survived roundtrip
        if let AxisMappingMode::TwoKey {
            positive_key,
            negative_key,
        } = &parsed.axes[0].mapping
        {
            assert_eq!(positive_key.index, 21); // D
            assert_eq!(negative_key.index, 9); // A
        } else {
            panic!("Expected TwoKey mapping");
        }
    }

    #[test]
    fn test_backwards_compat_old_format() {
        // Old format with { key_index, label } objects
        let old_toml = r#"
device_name = "MonsGeek Virtual Joystick"

[[axes]]
id = "X"
enabled = true

[axes.mapping]
type = "TwoKey"

[axes.mapping.positive_key]
key_index = 21
label = "D"

[axes.mapping.negative_key]
key_index = 9
label = "A"

[axes.calibration]
min_travel_mm = 0.0
max_travel_mm = 4.0
deadzone_percent = 5.0
curve_exponent = 1.0
"#;
        let config: JoystickConfig = toml::from_str(old_toml).unwrap();
        assert_eq!(config.axes.len(), 1);
        if let AxisMappingMode::TwoKey {
            positive_key,
            negative_key,
        } = &config.axes[0].mapping
        {
            assert_eq!(positive_key.index, 21);
            assert_eq!(positive_key.position, "D");
            assert_eq!(negative_key.index, 9);
            assert_eq!(negative_key.position, "A");
        } else {
            panic!("Expected TwoKey mapping");
        }
    }

    #[test]
    fn test_new_format_parse() {
        // New format with bare key name strings
        let new_toml = r#"
device_name = "MonsGeek Virtual Joystick"

[[axes]]
id = "X"
enabled = true

[axes.mapping]
type = "TwoKey"
positive_key = "D"
negative_key = "A"

[axes.calibration]
min_travel_mm = 0.0
max_travel_mm = 4.0
deadzone_percent = 5.0
curve_exponent = 1.0
"#;
        let config: JoystickConfig = toml::from_str(new_toml).unwrap();
        if let AxisMappingMode::TwoKey {
            positive_key,
            negative_key,
        } = &config.axes[0].mapping
        {
            assert_eq!(positive_key.index, 21);
            assert_eq!(positive_key.position, "D");
            assert_eq!(negative_key.index, 9);
            assert_eq!(negative_key.position, "A");
        } else {
            panic!("Expected TwoKey mapping");
        }
    }

    #[test]
    fn test_old_format_resaves_as_new() {
        let old_toml = r#"
device_name = "MonsGeek Virtual Joystick"

[[axes]]
id = "X"
enabled = true

[axes.mapping]
type = "TwoKey"

[axes.mapping.positive_key]
key_index = 21
label = "D"

[axes.mapping.negative_key]
key_index = 9
label = "A"
"#;
        let config: JoystickConfig = toml::from_str(old_toml).unwrap();
        let resaved = toml::to_string_pretty(&config).unwrap();
        // After re-saving, should use new bare string format
        assert!(resaved.contains("positive_key = \"D\""));
        assert!(resaved.contains("negative_key = \"A\""));
        // Should not contain old format
        assert!(!resaved.contains("key_index"));
        assert!(!resaved.contains("label"));
    }
}
