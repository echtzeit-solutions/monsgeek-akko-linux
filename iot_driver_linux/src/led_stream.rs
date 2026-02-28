//! Shared LED frame-sending utilities for the 0xE8 patch protocol.
//!
//! Page packing, power budget scaling, and constants used by `commands::led_stream`,
//! `notify::daemon`, `effect::preview`, and the gRPC server.

use crate::notify::keymap::MATRIX_LEN;

const LEDS_PER_PAGE: usize = 18;

/// Number of pages needed to cover the 16x6 LED grid.
const PAGE_COUNT: usize = MATRIX_LEN.div_ceil(LEDS_PER_PAGE);

/// Estimated current draw per WS2812 channel at full brightness (value=255).
/// WS2812B datasheet: ~20mA typical per channel.
pub const MA_PER_CHANNEL: f32 = 20.0;

/// Default power budget for LED effects (milliamps).
pub const DEFAULT_POWER_BUDGET_MA: f32 = 400.0;

/// Scale LED values to stay within a power budget.
///
/// WS2812B model: each channel draws `MA_PER_CHANNEL` mA at value 255.
/// Total per LED = `(R + G + B) / 255 * MA_PER_CHANNEL`.
/// If total exceeds `budget_ma`, all values are uniformly scaled down.
/// Returns `(estimated_ma_before_scaling, was_scaled)`.
pub fn apply_power_budget(leds: &mut [(u8, u8, u8); MATRIX_LEN], budget_ma: u32) -> (f32, bool) {
    let ma_per_unit = MA_PER_CHANNEL / 255.0;
    let total_ma: f32 = leds
        .iter()
        .map(|&(r, g, b)| (r as f32 + g as f32 + b as f32) * ma_per_unit)
        .sum();

    if budget_ma > 0 && total_ma > budget_ma as f32 {
        let scale = budget_ma as f32 / total_ma;
        for led in leds.iter_mut() {
            led.0 = (led.0 as f32 * scale) as u8;
            led.1 = (led.1 as f32 * scale) as u8;
            led.2 = (led.2 as f32 * scale) as u8;
        }
        (total_ma, true)
    } else {
        (total_ma, false)
    }
}

/// Send a full frame of RGB data to the keyboard.
///
/// `leds` has `MATRIX_LEN` entries (row-major: index = row*16 + col).
/// Packs into pages of 18 entries each and sends via stream_led_page + commit.
pub fn send_full_frame(
    kb: &monsgeek_keyboard::KeyboardInterface,
    leds: &[(u8, u8, u8); MATRIX_LEN],
) -> Result<(), Box<dyn std::error::Error>> {
    for page in 0..PAGE_COUNT {
        let start = page * LEDS_PER_PAGE;
        let end = (start + LEDS_PER_PAGE).min(MATRIX_LEN);
        let count = end - start;

        let mut rgb_data = [0u8; LEDS_PER_PAGE * 3];
        for i in 0..count {
            let (r, g, b) = leds[start + i];
            rgb_data[i * 3] = r;
            rgb_data[i * 3 + 1] = g;
            rgb_data[i * 3 + 2] = b;
        }

        kb.stream_led_page(page as u8, &rgb_data[..LEDS_PER_PAGE * 3])?;
    }

    kb.stream_led_commit()?;
    Ok(())
}
