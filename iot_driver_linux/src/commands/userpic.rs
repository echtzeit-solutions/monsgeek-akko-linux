//! Userpic upload/download command handler.
//!
//! Mode 13 (UserPicture) stores static per-key colors in flash, one RGB triple
//! per key-matrix position. The matrix is column-major with 6 rows on every
//! board, so a userpic is a picture 6 pixels tall; its width is however many
//! columns this board's keys occupy (15 on the M1 V5, up to 21 on full-size).

use super::{CmdCtx, CommandResult, open_ry_keyboard};
use image::imageops::FilterType;
use image::{GenericImageView, Rgb, RgbImage};
use monsgeek_keyboard::{RyKeyboard, USERPIC_BYTES};
use monsgeek_transport::protocol::{MatrixPos, matrix};

const ROWS: usize = matrix::ROWS as usize;

/// Widest grid a userpic slot can hold.
const MAX_COLS: usize = USERPIC_BYTES / 3 / ROWS;

/// Whether a matrix position has a key with an LED under it.
///
/// Usages below 0x04 are the database's placeholders, and non-analog positions
/// are encoders; neither lights up.
fn is_lit(factory_usage: Option<u8>, non_analog: bool) -> bool {
    factory_usage.is_some_and(|usage| usage >= 0x04) && !non_analog
}

/// Columns needed to reach the last lit key.
///
/// Counting an unlit trailing column (the M1 V5's volume knob) would scale the
/// image onto one column more than anyone sees. With no lit positions at all (a
/// board missing from the device database) fall back to the widest grid.
fn grid_cols(lit_positions: impl Iterator<Item = usize>) -> usize {
    lit_positions
        .filter(|&pos| pos < MAX_COLS * ROWS)
        .max()
        .map_or(MAX_COLS, |last| last / ROWS + 1)
}

fn board_grid_cols(kb: &RyKeyboard) -> usize {
    grid_cols(
        (0..MAX_COLS * ROWS).filter(|&pos| is_lit(kb.matrix_default(pos), kb.is_non_analog(pos))),
    )
}

/// The pixel a matrix position's color comes from, as (col, row).
fn pixel_of(pos: usize) -> (u32, u32) {
    let pos = MatrixPos::new(pos as u8);
    (u32::from(pos.col()), u32::from(pos.row()))
}

/// Convert an image to userpic data for a `cols`×6 grid.
fn image_to_userpic(img: &image::DynamicImage, cols: usize, nearest: bool) -> Vec<u8> {
    let filter = if nearest {
        FilterType::Nearest
    } else {
        FilterType::Lanczos3
    };
    let resized = img.resize_exact(cols as u32, ROWS as u32, filter).to_rgb8();
    let mut data = vec![0u8; cols * ROWS * 3];
    for (pos, rgb) in data.chunks_exact_mut(3).enumerate() {
        let (x, y) = pixel_of(pos);
        rgb.copy_from_slice(&resized.get_pixel(x, y).0);
    }
    data
}

/// Convert userpic data to a `cols`×6 RGB image.
fn userpic_to_image(data: &[u8], cols: usize) -> RgbImage {
    let mut img = RgbImage::new(cols as u32, ROWS as u32);
    for (pos, rgb) in data.chunks_exact(3).take(cols * ROWS).enumerate() {
        let (x, y) = pixel_of(pos);
        img.put_pixel(x, y, Rgb([rgb[0], rgb[1], rgb[2]]));
    }
    img
}

/// Upload or download a userpic.
pub fn userpic(
    ctx: &CmdCtx,
    file: Option<String>,
    slot: u8,
    output: Option<String>,
    nearest: bool,
) -> CommandResult {
    let kb = open_ry_keyboard(ctx)?;
    let cols = board_grid_cols(&kb);

    if let Some(path) = file {
        let img = image::open(&path).map_err(|e| format!("Failed to open image: {e}"))?;
        let (w, h) = img.dimensions();
        let data = image_to_userpic(&img, cols, nearest);
        kb.upload_userpic(slot, &data)?;
        kb.set_led_with_option(13, 4, 0, 0, 200, 200, false, slot)?;
        let filter_name = if nearest { "nearest" } else { "lanczos3" };
        println!(
            "Uploaded {path} ({w}x{h} -> {cols}x{ROWS}) to slot {slot} ({filter_name}), mode set to UserPicture."
        );
    } else {
        let data = kb.download_userpic(slot)?;
        let data = &data[..data.len().min(cols * ROWS * 3)];

        if data.iter().all(|&b| b == 0xFF) || data.iter().all(|&b| b == 0) {
            println!("Slot {slot} is empty.");
            return Ok(());
        }

        let img = userpic_to_image(data, cols);
        let out = output.unwrap_or_else(|| format!("userpic_{slot}.png"));
        img.save(&out)
            .map_err(|e| format!("Failed to save image: {e}"))?;
        println!("Saved slot {slot} to {out} ({cols}x{ROWS} PNG)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iot_driver::device_loader::{JsonDeviceMatricesFile, JsonDeviceMatrix};

    #[test]
    fn grid_reaches_the_last_lit_column() {
        assert_eq!(grid_cols([0, 5, 6].into_iter()), 2);
        assert_eq!(grid_cols([89].into_iter()), 15);
        assert_eq!(grid_cols([125].into_iter()), MAX_COLS);
    }

    #[test]
    fn unknown_board_gets_the_full_slot() {
        assert_eq!(grid_cols(std::iter::empty()), MAX_COLS);
        assert_eq!(MAX_COLS, 21);
    }

    #[test]
    fn pixels_land_column_major() {
        let mut img = RgbImage::new(3, ROWS as u32);
        img.put_pixel(2, 1, Rgb([10, 20, 30]));
        let data = image_to_userpic(&image::DynamicImage::ImageRgb8(img.clone()), 3, true);
        assert_eq!(data.len(), 3 * ROWS * 3);
        let off = (2 * ROWS + 1) * 3;
        assert_eq!(&data[off..off + 3], &[10, 20, 30]);
        assert_eq!(userpic_to_image(&data, 3), img);
    }

    fn lit_positions(m: &JsonDeviceMatrix) -> impl Iterator<Item = usize> + '_ {
        (0..m.matrix.len()).filter(|&pos| is_lit(m.hid_code(pos), m.is_non_analog(pos as u8)))
    }

    /// Every board in the database fits a slot, and the M1 V5's encoder column
    /// (volume knob plus two placeholders) does not widen its picture.
    #[test]
    fn database_boards_fit_a_slot() {
        let json = std::fs::read_to_string("../data/device_matrices.json")
            .or_else(|_| std::fs::read_to_string("data/device_matrices.json"));
        let Ok(json) = json else {
            return; // matrix DB not present in this checkout
        };
        let file: JsonDeviceMatricesFile = serde_json::from_str(&json).unwrap();

        let m1v5 = &file.devices["12625:20528:2949"];
        assert_eq!(grid_cols(lit_positions(m1v5)), 15);

        for m in file.devices.values() {
            assert!(
                lit_positions(m).all(|pos| pos < MAX_COLS * ROWS),
                "{} has a key past the userpic slot",
                m.name
            );
        }
    }
}
