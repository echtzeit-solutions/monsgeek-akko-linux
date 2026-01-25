//! Configuration structures for the joystick mapper
//!
//! Supports TOML serialization for persistent config storage.

use serde::{Deserialize, Serialize};
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

/// A key binding referencing a physical key on the keyboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBinding {
    /// Matrix index of the key (0-125 for typical keyboards)
    pub key_index: u8,
    /// Human-readable label for the key (e.g., "W", "A", "LShift")
    pub label: String,
}

impl KeyBinding {
    /// Create a new key binding
    pub fn new(key_index: u8, label: impl Into<String>) -> Self {
        Self {
            key_index,
            label: label.into(),
        }
    }
}

/// How an axis is mapped from key(s) to joystick value
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AxisMappingMode {
    /// Two keys control positive and negative directions
    /// e.g., W/S for Y-axis: W pushes positive, S pushes negative
    TwoKey {
        positive_key: KeyBinding,
        negative_key: KeyBinding,
    },
    /// Single key controls 0 to max (throttle-style)
    SingleKey {
        key: KeyBinding,
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
            } => vec![positive_key.key_index, negative_key.key_index],
            AxisMappingMode::SingleKey { key, .. } => vec![key.key_index],
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
                        positive_key: KeyBinding::new(21, "D"),
                        negative_key: KeyBinding::new(9, "A"),
                    },
                    calibration: AxisCalibration::default(),
                },
                AxisConfig {
                    id: AxisId::Y,
                    enabled: true,
                    mapping: AxisMappingMode::TwoKey {
                        positive_key: KeyBinding::new(14, "W"),
                        negative_key: KeyBinding::new(15, "S"),
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
    }

    #[test]
    fn test_roundtrip() {
        let config = JoystickConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: JoystickConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.device_name, config.device_name);
        assert_eq!(parsed.axes.len(), config.axes.len());
    }
}
