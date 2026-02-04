//! Depth-to-axis value mapping logic
//!
//! Converts raw key depth readings (in mm) to joystick axis values.

use crate::config::{AxisCalibration, AxisConfig, AxisId, AxisMappingMode, JoystickConfig};
use crate::joystick::{AXIS_MAX, AXIS_MIN};
use std::collections::HashMap;

/// Stores current key depth values and computes axis outputs
pub struct AxisMapper {
    /// Current depth (in mm) for each key index
    key_depths: HashMap<u8, f32>,
    /// Computed axis values (cached for display)
    axis_values: HashMap<AxisId, i32>,
}

impl AxisMapper {
    /// Create a new mapper
    pub fn new() -> Self {
        Self {
            key_depths: HashMap::new(),
            axis_values: HashMap::new(),
        }
    }

    /// Update a key's depth value
    pub fn update_key_depth(&mut self, key_index: u8, depth_mm: f32) {
        self.key_depths.insert(key_index, depth_mm);
    }

    /// Get current depth for a key
    pub fn get_key_depth(&self, key_index: u8) -> f32 {
        self.key_depths.get(&key_index).copied().unwrap_or(0.0)
    }

    /// Clear depth for a key (key released / no data)
    pub fn clear_key_depth(&mut self, key_index: u8) {
        self.key_depths.remove(&key_index);
    }

    /// Compute axis values based on current key depths
    ///
    /// Returns a list of (AxisId, value) pairs for all enabled axes.
    pub fn compute_axes(&mut self, config: &JoystickConfig) -> Vec<(AxisId, i32)> {
        let mut results = Vec::new();

        for axis_config in &config.axes {
            if !axis_config.enabled {
                continue;
            }

            let value = self.compute_axis(axis_config);
            self.axis_values.insert(axis_config.id, value);
            results.push((axis_config.id, value));
        }

        results
    }

    /// Compute a single axis value
    fn compute_axis(&self, config: &AxisConfig) -> i32 {
        match &config.mapping {
            AxisMappingMode::TwoKey {
                positive_key,
                negative_key,
            } => {
                let pos_depth = self.get_key_depth(positive_key.index);
                let neg_depth = self.get_key_depth(negative_key.index);
                map_two_key(pos_depth, neg_depth, &config.calibration)
            }
            AxisMappingMode::SingleKey { key, invert } => {
                let depth = self.get_key_depth(key.index);
                map_single_key(depth, *invert, &config.calibration)
            }
        }
    }

    /// Get cached axis value
    pub fn get_axis_value(&self, axis: AxisId) -> i32 {
        self.axis_values.get(&axis).copied().unwrap_or(0)
    }

    /// Get all current key depths (for display)
    pub fn all_key_depths(&self) -> &HashMap<u8, f32> {
        &self.key_depths
    }
}

impl Default for AxisMapper {
    fn default() -> Self {
        Self::new()
    }
}

/// Map two keys to a bipolar axis value (-32767 to +32767)
///
/// Positive key increases value, negative key decreases it.
/// If both are pressed, they can cancel out.
fn map_two_key(pos_depth_mm: f32, neg_depth_mm: f32, cal: &AxisCalibration) -> i32 {
    // Normalize each key's depth to 0.0-1.0
    let pos_norm = normalize_depth(pos_depth_mm, cal);
    let neg_norm = normalize_depth(neg_depth_mm, cal);

    // Combine: positive pushes up, negative pushes down
    let combined = pos_norm - neg_norm; // Range: -1.0 to +1.0

    // Apply deadzone (as a fraction of the combined range)
    let deadzone = cal.deadzone_percent / 100.0;
    let with_deadzone = if combined.abs() < deadzone {
        0.0
    } else {
        // Scale remaining range to full -1.0 to +1.0
        let sign = combined.signum();
        let magnitude = (combined.abs() - deadzone) / (1.0 - deadzone);
        sign * magnitude
    };

    // Apply response curve
    let curved = apply_curve(with_deadzone, cal.curve_exponent);

    // Scale to axis range
    (curved * AXIS_MAX as f32) as i32
}

/// Map a single key to a unipolar axis value (-32767 to +32767)
///
/// 0mm = -32767 (or +32767 if inverted)
/// max_travel = +32767 (or -32767 if inverted)
fn map_single_key(depth_mm: f32, invert: bool, cal: &AxisCalibration) -> i32 {
    // Normalize to 0.0-1.0
    let norm = normalize_depth(depth_mm, cal);

    // Apply deadzone
    let deadzone = cal.deadzone_percent / 100.0;
    let with_deadzone = if norm < deadzone {
        0.0
    } else {
        (norm - deadzone) / (1.0 - deadzone)
    };

    // Apply response curve
    let curved = apply_curve(with_deadzone, cal.curve_exponent);

    // Convert to full axis range (-32767 to +32767)
    // 0.0 -> -32767, 1.0 -> +32767
    let scaled = AXIS_MIN as f32 + curved * (AXIS_MAX - AXIS_MIN) as f32;

    if invert {
        -scaled as i32
    } else {
        scaled as i32
    }
}

/// Normalize depth to 0.0-1.0 based on calibration
fn normalize_depth(depth_mm: f32, cal: &AxisCalibration) -> f32 {
    let range = cal.max_travel_mm - cal.min_travel_mm;
    if range <= 0.0 {
        return 0.0;
    }
    ((depth_mm - cal.min_travel_mm) / range).clamp(0.0, 1.0)
}

/// Apply response curve (exponent)
///
/// Exponent > 1.0 makes the center less sensitive
/// Exponent < 1.0 makes the center more sensitive
fn apply_curve(value: f32, exponent: f32) -> f32 {
    value.signum() * value.abs().powf(exponent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_cal() -> AxisCalibration {
        AxisCalibration {
            min_travel_mm: 0.0,
            max_travel_mm: 4.0,
            deadzone_percent: 0.0,
            curve_exponent: 1.0,
        }
    }

    #[test]
    fn test_two_key_neutral() {
        let cal = default_cal();
        // Both keys released
        assert_eq!(map_two_key(0.0, 0.0, &cal), 0);
    }

    #[test]
    fn test_two_key_positive_full() {
        let cal = default_cal();
        // Positive key fully pressed
        let val = map_two_key(4.0, 0.0, &cal);
        assert_eq!(val, AXIS_MAX);
    }

    #[test]
    fn test_two_key_negative_full() {
        let cal = default_cal();
        // Negative key fully pressed
        let val = map_two_key(0.0, 4.0, &cal);
        assert_eq!(val, AXIS_MIN);
    }

    #[test]
    fn test_two_key_both_pressed() {
        let cal = default_cal();
        // Both keys pressed equally -> cancel out
        let val = map_two_key(2.0, 2.0, &cal);
        assert_eq!(val, 0);
    }

    #[test]
    fn test_two_key_deadzone() {
        let mut cal = default_cal();
        cal.deadzone_percent = 10.0;

        // Small press should be absorbed by deadzone
        let val = map_two_key(0.2, 0.0, &cal); // 5% travel
        assert_eq!(val, 0);
    }

    #[test]
    fn test_single_key_neutral() {
        let cal = default_cal();
        let val = map_single_key(0.0, false, &cal);
        assert_eq!(val, AXIS_MIN);
    }

    #[test]
    fn test_single_key_full() {
        let cal = default_cal();
        let val = map_single_key(4.0, false, &cal);
        assert_eq!(val, AXIS_MAX);
    }

    #[test]
    fn test_single_key_inverted() {
        let cal = default_cal();
        let val = map_single_key(4.0, true, &cal);
        assert_eq!(val, AXIS_MIN);
    }
}
