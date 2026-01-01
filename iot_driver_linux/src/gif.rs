// GIF file parsing and keyboard LED mapping
// Converts GIF animations to per-key RGB frames

use image::{AnimationDecoder, DynamicImage, GenericImageView, ImageDecoder, Rgba};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::devices::M1_V5_HE_LED_MATRIX;
use crate::protocol::rgb::MATRIX_SIZE;

/// Keyboard layout dimensions for M1 V5 HE
/// The matrix is organized in 16 columns x 6 rows
const MATRIX_COLS: usize = 16;
const MATRIX_ROWS: usize = 6;

/// A single frame of keyboard LED colors
#[derive(Debug, Clone)]
pub struct LedFrame {
    /// RGB colors for each of the 126 matrix positions
    pub colors: Vec<(u8, u8, u8)>,
    /// Frame delay in milliseconds
    pub delay_ms: u16,
}

/// Mapping mode for converting image pixels to keyboard LEDs
#[derive(Debug, Clone, Copy, Default)]
pub enum MappingMode {
    /// Scale image to fit keyboard grid, sample center of each cell
    #[default]
    ScaleToFit,
    /// Tile image across keyboard, wrapping if smaller
    Tile,
    /// Use image as-is, centering on keyboard
    Center,
    /// Sample specific pixel positions (for pre-made keyboard images)
    Direct,
}

/// Result of loading a GIF file
#[derive(Debug)]
pub struct GifAnimation {
    pub frames: Vec<LedFrame>,
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
}

/// Load a GIF file and convert to keyboard LED frames
pub fn load_gif<P: AsRef<Path>>(
    path: P,
    mode: MappingMode,
) -> Result<GifAnimation, Box<dyn std::error::Error>> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);

    let decoder = image::codecs::gif::GifDecoder::new(reader)?;
    let (width, height) = decoder.dimensions();

    let frames_iter = decoder.into_frames();
    let mut frames = Vec::new();

    for frame_result in frames_iter {
        let frame = frame_result?;
        let delay = frame.delay().numer_denom_ms();
        let delay_ms = (delay.0 / delay.1.max(1)) as u16;
        let delay_ms = if delay_ms == 0 { 100 } else { delay_ms }; // Default 100ms if unspecified

        let img = DynamicImage::ImageRgba8(frame.into_buffer());
        let colors = map_image_to_leds(&img, mode);

        frames.push(LedFrame { colors, delay_ms });
    }

    let frame_count = frames.len();

    Ok(GifAnimation {
        frames,
        width,
        height,
        frame_count,
    })
}

/// Load a static image (PNG, etc.) as a single frame
pub fn load_image<P: AsRef<Path>>(
    path: P,
    mode: MappingMode,
) -> Result<GifAnimation, Box<dyn std::error::Error>> {
    let img = image::open(path.as_ref())?;
    let (width, height) = img.dimensions();

    let colors = map_image_to_leds(&img, mode);
    let frames = vec![LedFrame {
        colors,
        delay_ms: 100,
    }];

    Ok(GifAnimation {
        frames,
        width,
        height,
        frame_count: 1,
    })
}

/// Map an image to the 126-position LED matrix
fn map_image_to_leds(img: &DynamicImage, mode: MappingMode) -> Vec<(u8, u8, u8)> {
    let mut colors = vec![(0u8, 0u8, 0u8); MATRIX_SIZE];

    match mode {
        MappingMode::ScaleToFit => map_scale_to_fit(img, &mut colors),
        MappingMode::Tile => map_tile(img, &mut colors),
        MappingMode::Center => map_center(img, &mut colors),
        MappingMode::Direct => map_direct(img, &mut colors),
    }

    colors
}

/// Scale image to keyboard grid dimensions and sample
fn map_scale_to_fit(img: &DynamicImage, colors: &mut [(u8, u8, u8)]) {
    let (img_w, img_h) = img.dimensions();

    // Calculate scaling factors
    let scale_x = img_w as f32 / MATRIX_COLS as f32;
    let scale_y = img_h as f32 / MATRIX_ROWS as f32;

    // For each matrix position, sample the corresponding image pixel
    for (pos, &hid_code) in M1_V5_HE_LED_MATRIX.iter().enumerate() {
        if hid_code == 0 {
            continue; // Skip empty positions
        }

        // Convert linear position to row/col
        let col = pos / MATRIX_ROWS;
        let row = pos % MATRIX_ROWS;

        // Calculate image coordinates (center of cell)
        let img_x = ((col as f32 + 0.5) * scale_x) as u32;
        let img_y = ((row as f32 + 0.5) * scale_y) as u32;

        // Clamp to image bounds
        let img_x = img_x.min(img_w - 1);
        let img_y = img_y.min(img_h - 1);

        let pixel = img.get_pixel(img_x, img_y);
        colors[pos] = rgba_to_rgb(pixel);
    }
}

/// Tile image across keyboard, wrapping if needed
fn map_tile(img: &DynamicImage, colors: &mut [(u8, u8, u8)]) {
    let (img_w, img_h) = img.dimensions();

    for (pos, &hid_code) in M1_V5_HE_LED_MATRIX.iter().enumerate() {
        if hid_code == 0 {
            continue;
        }

        let col = pos / MATRIX_ROWS;
        let row = pos % MATRIX_ROWS;

        // Wrap coordinates
        let img_x = (col as u32) % img_w;
        let img_y = (row as u32) % img_h;

        let pixel = img.get_pixel(img_x, img_y);
        colors[pos] = rgba_to_rgb(pixel);
    }
}

/// Center image on keyboard
fn map_center(img: &DynamicImage, colors: &mut [(u8, u8, u8)]) {
    let (img_w, img_h) = img.dimensions();

    // Calculate offset to center
    let offset_x = (MATRIX_COLS as i32 - img_w as i32) / 2;
    let offset_y = (MATRIX_ROWS as i32 - img_h as i32) / 2;

    for (pos, &hid_code) in M1_V5_HE_LED_MATRIX.iter().enumerate() {
        if hid_code == 0 {
            continue;
        }

        let col = (pos / MATRIX_ROWS) as i32;
        let row = (pos % MATRIX_ROWS) as i32;

        // Calculate image coordinates
        let img_x = col - offset_x;
        let img_y = row - offset_y;

        // Check bounds
        if img_x >= 0 && img_x < img_w as i32 && img_y >= 0 && img_y < img_h as i32 {
            let pixel = img.get_pixel(img_x as u32, img_y as u32);
            colors[pos] = rgba_to_rgb(pixel);
        }
    }
}

/// Direct 1:1 pixel mapping (for pre-made keyboard images)
fn map_direct(img: &DynamicImage, colors: &mut [(u8, u8, u8)]) {
    let (img_w, img_h) = img.dimensions();

    for (pos, &hid_code) in M1_V5_HE_LED_MATRIX.iter().enumerate() {
        if hid_code == 0 {
            continue;
        }

        let col = pos / MATRIX_ROWS;
        let row = pos % MATRIX_ROWS;

        if (col as u32) < img_w && (row as u32) < img_h {
            let pixel = img.get_pixel(col as u32, row as u32);
            colors[pos] = rgba_to_rgb(pixel);
        }
    }
}

/// Convert RGBA pixel to RGB tuple, handling alpha by blending with black
fn rgba_to_rgb(pixel: Rgba<u8>) -> (u8, u8, u8) {
    let Rgba([r, g, b, a]) = pixel;

    if a == 255 {
        (r, g, b)
    } else if a == 0 {
        (0, 0, 0)
    } else {
        // Blend with black background
        let alpha = a as f32 / 255.0;
        (
            (r as f32 * alpha) as u8,
            (g as f32 * alpha) as u8,
            (b as f32 * alpha) as u8,
        )
    }
}

/// Generate a simple test pattern animation
pub fn generate_test_animation(num_frames: usize, delay_ms: u16) -> GifAnimation {
    let mut frames = Vec::with_capacity(num_frames);

    for frame_idx in 0..num_frames {
        let mut colors = vec![(0u8, 0u8, 0u8); MATRIX_SIZE];
        let hue_offset = (frame_idx as f32 / num_frames as f32) * 360.0;

        for (pos, &hid_code) in M1_V5_HE_LED_MATRIX.iter().enumerate() {
            if hid_code == 0 {
                continue;
            }

            let col = pos / MATRIX_ROWS;
            let hue = (hue_offset + (col as f32 * 20.0)) % 360.0;
            colors[pos] = hsv_to_rgb(hue, 1.0, 1.0);
        }

        frames.push(LedFrame { colors, delay_ms });
    }

    GifAnimation {
        frames,
        width: MATRIX_COLS as u32,
        height: MATRIX_ROWS as u32,
        frame_count: num_frames,
    }
}

use crate::color::hsv_to_rgb;

/// Print animation info
pub fn print_animation_info(anim: &GifAnimation) {
    println!("GIF Animation Info:");
    println!("  Dimensions: {}x{}", anim.width, anim.height);
    println!("  Frames: {}", anim.frame_count);

    if !anim.frames.is_empty() {
        let delays: Vec<u16> = anim.frames.iter().map(|f| f.delay_ms).collect();
        let avg_delay: f32 = delays.iter().map(|&d| d as f32).sum::<f32>() / delays.len() as f32;
        let total_duration: u32 = delays.iter().map(|&d| d as u32).sum();

        println!("  Avg frame delay: {avg_delay:.1}ms");
        println!("  Total duration: {}ms ({:.1}s)", total_duration, total_duration as f32 / 1000.0);

        // Count active LEDs in first frame
        let active = anim.frames[0].colors.iter().filter(|&&(r, g, b)| r > 0 || g > 0 || b > 0).count();
        println!("  Active LEDs (frame 0): {active}/{MATRIX_SIZE}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_animation() {
        let anim = generate_test_animation(10, 50);
        assert_eq!(anim.frame_count, 10);
        assert_eq!(anim.frames.len(), 10);
        assert_eq!(anim.frames[0].colors.len(), MATRIX_SIZE);
    }
}
