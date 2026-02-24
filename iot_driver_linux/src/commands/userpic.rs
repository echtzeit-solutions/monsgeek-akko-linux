//! Userpic upload/download command handler.
//!
//! Mode 13 (UserPicture) stores static per-key colors in flash.
//! 5 slots (0-4) at 384 bytes each, column-major layout:
//! pixel (col, row) at offset `col * 18 + row * 3`.

use super::{open_keyboard, CommandResult};
use image::imageops::FilterType;
use image::{GenericImageView, RgbImage};
use monsgeek_transport::PrinterConfig;

const COLS: usize = 16;
const ROWS: usize = 6;
const DATA_SIZE: usize = COLS * ROWS * 3; // 288 bytes of RGB data

/// Convert an image to userpic column-major format (288 bytes).
fn image_to_userpic(img: &image::DynamicImage, nearest: bool) -> Vec<u8> {
    let filter = if nearest {
        FilterType::Nearest
    } else {
        FilterType::Lanczos3
    };
    let resized = img.resize_exact(COLS as u32, ROWS as u32, filter).to_rgb8();
    let mut data = vec![0u8; DATA_SIZE];
    for col in 0..COLS {
        for row in 0..ROWS {
            let px = resized.get_pixel(col as u32, row as u32);
            let off = col * 18 + row * 3;
            data[off] = px[0];
            data[off + 1] = px[1];
            data[off + 2] = px[2];
        }
    }
    data
}

/// Convert userpic column-major data to a 16x6 RGB image.
fn userpic_to_image(data: &[u8]) -> RgbImage {
    let mut img = RgbImage::new(COLS as u32, ROWS as u32);
    for col in 0..COLS {
        for row in 0..ROWS {
            let off = col * 18 + row * 3;
            if off + 2 < data.len() {
                img.put_pixel(
                    col as u32,
                    row as u32,
                    image::Rgb([data[off], data[off + 1], data[off + 2]]),
                );
            }
        }
    }
    img
}

/// Upload or download a userpic.
pub fn userpic(
    printer_config: Option<PrinterConfig>,
    file: Option<String>,
    slot: u8,
    output: Option<String>,
    nearest: bool,
) -> CommandResult {
    let kb = open_keyboard(printer_config)?;

    if let Some(path) = file {
        // Upload
        let img = image::open(&path).map_err(|e| format!("Failed to open image: {e}"))?;
        let (w, h) = img.dimensions();
        let data = image_to_userpic(&img, nearest);
        kb.upload_userpic(slot, &data)?;
        kb.set_led_with_option(13, 4, 0, 0, 200, 200, false, slot)?;
        let filter_name = if nearest { "nearest" } else { "lanczos3" };
        println!(
            "Uploaded {path} ({w}x{h}) to slot {slot} ({filter_name}), mode set to UserPicture."
        );
    } else {
        // Download
        let data = kb.download_userpic(slot)?;

        // Check if slot is empty (all 0xFF or all 0x00)
        let all_empty = data.iter().all(|&b| b == 0xFF) || data.iter().all(|&b| b == 0);
        if all_empty {
            println!("Slot {slot} is empty.");
            return Ok(());
        }

        let img = userpic_to_image(&data);
        let out = output.unwrap_or_else(|| format!("userpic_{slot}.png"));
        img.save(&out)
            .map_err(|e| format!("Failed to save image: {e}"))?;
        println!("Saved slot {slot} to {out} (16x6 PNG)");
    }
    Ok(())
}
