// Color conversion utilities
//
// This module provides color conversion helpers.
// For RGB types and HSV conversion, prefer using `monsgeek_keyboard::led::RgbColor::from_hsv()`.

use monsgeek_keyboard::led::RgbColor;

/// Convert HSV to RGB tuple
///
/// This is a convenience wrapper around `RgbColor::from_hsv()`.
/// Consider using `RgbColor::from_hsv()` directly when working with `RgbColor`.
///
/// # Arguments
/// * `h` - hue (0-360)
/// * `s` - saturation (0-1)
/// * `v` - value/brightness (0-1)
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let color = RgbColor::from_hsv(h, s, v);
    (color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hsv_to_rgb() {
        // Red
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0));
        // Green
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), (0, 255, 0));
        // Blue
        assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), (0, 0, 255));
        // White (no saturation)
        assert_eq!(hsv_to_rgb(0.0, 0.0, 1.0), (255, 255, 255));
        // Black (no value)
        assert_eq!(hsv_to_rgb(0.0, 1.0, 0.0), (0, 0, 0));
    }
}
