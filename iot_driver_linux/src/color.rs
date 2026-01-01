// Color conversion utilities

/// Convert HSV to RGB
/// h: hue (0-360)
/// s: saturation (0-1)
/// v: value/brightness (0-1)
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
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
