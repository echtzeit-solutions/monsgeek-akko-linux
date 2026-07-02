//! Screen-sync color calibration and region selection.
//!
//! Pure host-side transforms applied to the averaged screen color before it is
//! streamed to the keyboard. Tinted switches/keycaps bias what the LEDs display
//! (e.g. white reading as blue, red as pink); calibration pre-distorts the color
//! to compensate. Kept dependency-free (no ashpd/pipewire) so it builds and
//! unit-tests without the `screen-capture` feature.

use serde::{Deserialize, Serialize};

/// Per-channel gain + gamma with a global saturation.
///
/// `out_c = clamp01((in_c/255)^gamma_c) * gain_c)`, then each channel is mixed
/// toward luminance by `saturation`. Identity (`gain=1, gamma=1, saturation=1`)
/// is a no-op, so an uncalibrated setup streams the raw average unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorCalibration {
    /// Per-channel linear gain (R, G, B). 1.0 = unchanged (white balance).
    pub gain: [f32; 3],
    /// Per-channel gamma (R, G, B). 1.0 = linear (midtone response).
    pub gamma: [f32; 3],
    /// Saturation around luminance. 1.0 = unchanged, 0.0 = grayscale, >1 = punchier.
    pub saturation: f32,
}

impl Default for ColorCalibration {
    fn default() -> Self {
        Self {
            gain: [1.0; 3],
            gamma: [1.0; 3],
            saturation: 1.0,
        }
    }
}

impl ColorCalibration {
    /// Normalized (0..1) gain+gamma output for one channel `i` of `v`.
    fn ch_norm(&self, v: u8, i: usize) -> f32 {
        ((v as f32 / 255.0).powf(self.gamma[i]) * self.gain[i]).clamp(0.0, 1.0)
    }

    /// Per-channel input→output mapping (gain + gamma only). Saturation is a
    /// cross-channel effect and is not represented here — this is what the
    /// per-channel calibration curves plot.
    pub fn channel_map(&self, input: u8, ch: usize) -> u8 {
        to_u8(self.ch_norm(input, ch))
    }

    /// Map a captured RGB color to the color streamed to the keyboard.
    pub fn apply(&self, (r, g, b): (u8, u8, u8)) -> (u8, u8, u8) {
        let (mut rf, mut gf, mut bf) = (self.ch_norm(r, 0), self.ch_norm(g, 1), self.ch_norm(b, 2));

        if (self.saturation - 1.0).abs() > f32::EPSILON {
            let lum = 0.299 * rf + 0.587 * gf + 0.114 * bf;
            let s = self.saturation;
            rf = (lum + (rf - lum) * s).clamp(0.0, 1.0);
            gf = (lum + (gf - lum) * s).clamp(0.0, 1.0);
            bf = (lum + (bf - lum) * s).clamp(0.0, 1.0);
        }
        (to_u8(rf), to_u8(gf), to_u8(bf))
    }
}

fn to_u8(v: f32) -> u8 {
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Normalized screen sub-rectangle (fractions in 0..1) that drives the averaged
/// color, so title bars / menus can be excluded. Default is the whole screen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Default for Region {
    fn default() -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: 1.0,
            bottom: 1.0,
        }
    }
}

impl Region {
    /// Clamp to 0..1 and guarantee a non-empty rect, falling back to the full
    /// screen if the bounds are inverted or degenerate.
    pub fn sanitized(&self) -> Region {
        let l = self.left.clamp(0.0, 1.0);
        let t = self.top.clamp(0.0, 1.0);
        let r = self.right.clamp(0.0, 1.0);
        let b = self.bottom.clamp(0.0, 1.0);
        if r > l && b > t {
            Region {
                left: l,
                top: t,
                right: r,
                bottom: b,
            }
        } else {
            Region::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_noop() {
        let c = ColorCalibration::default();
        assert_eq!(c.apply((10, 200, 255)), (10, 200, 255));
    }

    #[test]
    fn gain_scales_channel() {
        // Halve blue (e.g. to neutralize a board that renders white as blue-ish).
        let c = ColorCalibration {
            gain: [1.0, 1.0, 0.5],
            ..Default::default()
        };
        assert_eq!(c.apply((255, 255, 255)), (255, 255, 128));
    }

    #[test]
    fn zero_saturation_is_gray() {
        let c = ColorCalibration {
            saturation: 0.0,
            ..Default::default()
        };
        let (r, g, b) = c.apply((255, 0, 0));
        assert_eq!(r, g);
        assert_eq!(g, b);
    }

    #[test]
    fn gamma_darkens_midtones() {
        // gamma > 1 pulls a mid value down; 128/255 ≈ 0.502, ^2 ≈ 0.252 → ~64.
        let c = ColorCalibration {
            gamma: [2.0, 2.0, 2.0],
            ..Default::default()
        };
        let (r, _, _) = c.apply((128, 128, 128));
        assert!((60..=68).contains(&r), "got {r}");
    }

    #[test]
    fn channel_map_matches_apply_without_saturation() {
        let c = ColorCalibration {
            gain: [1.2, 0.9, 0.6],
            gamma: [1.5, 1.0, 0.8],
            saturation: 1.0,
        };
        // With saturation = 1, apply is exactly the three per-channel maps.
        let (r, g, b) = c.apply((200, 128, 64));
        assert_eq!(r, c.channel_map(200, 0));
        assert_eq!(g, c.channel_map(128, 1));
        assert_eq!(b, c.channel_map(64, 2));
    }

    #[test]
    fn region_default_is_full() {
        assert_eq!(Region::default().sanitized(), Region::default());
    }

    #[test]
    fn region_inverted_falls_back_to_full() {
        let r = Region {
            left: 0.8,
            top: 0.0,
            right: 0.2,
            bottom: 1.0,
        };
        assert_eq!(r.sanitized(), Region::default());
    }
}
