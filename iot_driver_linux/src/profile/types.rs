// Device profile types
// Supporting types for DeviceProfile trait

use serde::{Deserialize, Serialize};

/// Travel/actuation settings for magnetic (Hall effect) switches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TravelSettings {
    pub travel: RangeConfig,
    pub fire_press: RangeConfig,
    pub fire_lift: RangeConfig,
    pub deadzone: RangeConfig,
}

/// Configuration for a numeric range with min/max/step/default
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeConfig {
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub default: f32,
}

impl RangeConfig {
    pub fn new(min: f32, max: f32, step: f32, default: f32) -> Self {
        Self { min, max, step, default }
    }

    /// Check if a value is within the valid range
    pub fn contains(&self, value: f32) -> bool {
        value >= self.min && value <= self.max
    }

    /// Clamp a value to the valid range
    pub fn clamp(&self, value: f32) -> f32 {
        value.clamp(self.min, self.max)
    }

    /// Round a value to the nearest step
    pub fn snap_to_step(&self, value: f32) -> f32 {
        let steps = ((value - self.min) / self.step).round();
        self.min + steps * self.step
    }
}

impl Default for TravelSettings {
    fn default() -> Self {
        Self {
            travel: RangeConfig::new(0.1, 3.4, 0.01, 2.5),
            fire_press: RangeConfig::new(0.01, 2.5, 0.01, 1.5),
            fire_lift: RangeConfig::new(0.01, 2.5, 0.01, 1.5),
            deadzone: RangeConfig::new(0.0, 1.0, 0.01, 0.3),
        }
    }
}

/// Device feature flags
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceFeatures {
    #[serde(default)]
    pub magnetism: bool,
    #[serde(default)]
    pub sidelight: bool,
    #[serde(default)]
    pub screen: bool,
    #[serde(default)]
    pub knob: bool,
    #[serde(default, rename = "switchReplaceable")]
    pub switch_replaceable: bool,
}

/// Fn layer configuration per OS
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FnSysLayer {
    pub win: u8,
    pub mac: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_config() {
        let range = RangeConfig::new(0.1, 3.4, 0.1, 2.0);

        assert!(range.contains(1.0));
        assert!(!range.contains(0.0));
        assert!(!range.contains(4.0));

        assert_eq!(range.clamp(0.0), 0.1);
        assert_eq!(range.clamp(5.0), 3.4);
        assert_eq!(range.clamp(2.0), 2.0);
    }

    #[test]
    fn test_snap_to_step() {
        let range = RangeConfig::new(0.0, 1.0, 0.1, 0.5);

        assert!((range.snap_to_step(0.14) - 0.1).abs() < 0.001);
        assert!((range.snap_to_step(0.16) - 0.2).abs() < 0.001);
        assert!((range.snap_to_step(0.55) - 0.6).abs() < 0.001);
    }
}
