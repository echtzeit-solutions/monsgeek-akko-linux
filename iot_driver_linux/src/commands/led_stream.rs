//! LED streaming commands using the 0xFC patch protocol.
//!
//! These commands write RGB data directly to the WS2812 frame buffer via the
//! patched firmware's 0xFC command, without any flash writes.
//!
//! **Coordinate convention (row-major, physical layout)**
//!
//! The 16×6 grid matches the physical keyboard layout.  Each row is 16 columns
//! wide — the widest row including gaps for wide keys (LShift, Space, etc.).
//! Positions with no LED (gaps) are part of the coordinate space; the firmware
//! simply skips them (strip index 0xFF).
//!
//! - Position index: `pos = row * 16 + col`  (row-major)
//! - Image mapping:  `pixel(x, y) → leds[y * 16 + x]`  (trivial)
//! - Sweep test:     iterates all 96 positions; gaps produce no visible LED
//!   (the sweep "disappears" at gap positions, which is expected)

use super::{open_keyboard, setup_interrupt_handler, CommandResult};
use monsgeek_keyboard::SyncKeyboard;
use monsgeek_transport::PrinterConfig;
use std::sync::atomic::Ordering;

/// Matrix dimensions (row-major: index = row * COLS + col)
const COLS: usize = 16;
const ROWS: usize = 6;
/// Total matrix positions in the LED grid (including gaps)
const MATRIX_LEN: usize = COLS * ROWS; // 96
const LEDS_PER_PAGE: usize = 18;
/// Number of pages needed to cover the 16×6 grid
const PAGE_COUNT: usize = MATRIX_LEN.div_ceil(LEDS_PER_PAGE); // 6

/// Colors to cycle through in stream test
const TEST_COLORS: [(u8, u8, u8); 7] = [
    (255, 0, 0),     // Red
    (0, 255, 0),     // Green
    (0, 0, 255),     // Blue
    (255, 255, 0),   // Yellow
    (0, 255, 255),   // Cyan
    (255, 0, 255),   // Magenta
    (255, 255, 255), // White
];

/// Open keyboard and verify patch LED streaming is supported.
fn open_with_patch_check(
    printer_config: Option<PrinterConfig>,
) -> Result<SyncKeyboard, Box<dyn std::error::Error>> {
    let kb = open_keyboard(printer_config).map_err(|e| format!("No device found: {e}"))?;

    let patch = kb
        .get_patch_info()
        .map_err(|e| format!("Failed to query patch info: {e}"))?;

    match patch {
        Some(ref p) if p.has_led_stream() => {
            println!(
                "Patch: {} v{} (caps=0x{:04X})",
                p.name, p.version, p.capabilities
            );
        }
        Some(ref p) => {
            return Err(format!(
                "Patch '{}' found but LED streaming not supported (caps=0x{:04X})",
                p.name, p.capabilities
            )
            .into());
        }
        None => {
            return Err("Stock firmware — LED streaming requires patched firmware".into());
        }
    }

    Ok(kb)
}

/// Send a full frame of RGB data to the keyboard.
///
/// `leds` has `MATRIX_LEN` entries (row-major: index = row*16 + col).
/// Packs into pages of 18 entries each and sends via stream_led_page + commit.
fn send_full_frame(
    kb: &SyncKeyboard,
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

/// Test LED streaming — lights one LED at a time, cycling through colors.
///
/// Sweeps all 96 positions in row-major order (row 0 left→right, row 1, …).
/// Gap positions (no physical LED) produce a dark frame — the sweep
/// "disappears" momentarily, which is the expected spatial behaviour.
pub fn stream_test(printer_config: Option<PrinterConfig>, fps: f32) -> CommandResult {
    let kb = open_with_patch_check(printer_config)?;

    let frame_duration = std::time::Duration::from_secs_f32(1.0 / fps);
    let running = setup_interrupt_handler();

    println!(
        "Streaming test: {MATRIX_LEN} positions ({COLS}×{ROWS}), {fps:.1} FPS (Ctrl+C to stop)"
    );

    for &(cr, cg, cb) in TEST_COLORS.iter().cycle() {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        for pos in 0..MATRIX_LEN {
            if !running.load(Ordering::SeqCst) {
                break;
            }

            // Build frame: only the target position is lit (gaps → all dark)
            let page = pos / LEDS_PER_PAGE;
            let offset_in_page = pos % LEDS_PER_PAGE;

            let mut rgb_data = [0u8; LEDS_PER_PAGE * 3];
            rgb_data[offset_in_page * 3] = cr;
            rgb_data[offset_in_page * 3 + 1] = cg;
            rgb_data[offset_in_page * 3 + 2] = cb;

            // Send all pages: target page has one lit LED, others all black
            for p in 0..PAGE_COUNT {
                if p == page {
                    kb.stream_led_page(p as u8, &rgb_data)?;
                } else {
                    kb.stream_led_page(p as u8, &[0u8; LEDS_PER_PAGE * 3])?;
                }
            }
            kb.stream_led_commit()?;

            let row = pos / COLS;
            let col = pos % COLS;
            print!(
                "\rpos {:2} (row={}, col={:2}) color=({:3},{:3},{:3})  ",
                pos, row, col, cr, cg, cb
            );
            std::io::Write::flush(&mut std::io::stdout()).ok();

            std::thread::sleep(frame_duration);
        }
    }

    println!("\nReleasing LED stream...");
    kb.stream_led_release().ok();
    println!("Done.");
    Ok(())
}

/// Stream a GIF to keyboard LEDs via the 0xFC patch protocol.
pub fn stream_gif(
    printer_config: Option<PrinterConfig>,
    file: &str,
    fps: Option<f32>,
    loop_anim: bool,
) -> CommandResult {
    let kb = open_with_patch_check(printer_config)?;

    // Decode GIF
    println!("Loading GIF: {file}");
    let f = std::fs::File::open(file).map_err(|e| format!("Failed to open {file}: {e}"))?;
    let mut decoder = gif::DecodeOptions::new();
    decoder.set_color_output(gif::ColorOutput::RGBA);
    let mut reader = decoder
        .read_info(std::io::BufReader::new(f))
        .map_err(|e| format!("Failed to decode GIF: {e}"))?;

    let src_w = reader.width() as usize;
    let src_h = reader.height() as usize;
    println!("GIF: {}×{}", src_w, src_h);

    // Read all frames into memory (so we can loop)
    struct Frame {
        leds: [(u8, u8, u8); MATRIX_LEN],
        delay_ms: u64,
    }

    let scale_x = src_w as f32 / COLS as f32;
    let scale_y = src_h as f32 / ROWS as f32;

    let mut frames = Vec::new();
    while let Some(frame) = reader
        .read_next_frame()
        .map_err(|e| format!("GIF frame decode error: {e}"))?
    {
        let rgba = &frame.buffer;
        let delay_ms = if let Some(f) = fps {
            (1000.0 / f) as u64
        } else {
            // GIF delay is in centiseconds; 0 means "use default" (100ms is common)
            let d = frame.delay as u64 * 10;
            if d == 0 {
                100
            } else {
                d
            }
        };

        let mut leds = [(0u8, 0u8, 0u8); MATRIX_LEN];
        for row in 0..ROWS {
            for col in 0..COLS {
                let sx = ((col as f32 + 0.5) * scale_x) as usize;
                let sy = ((row as f32 + 0.5) * scale_y) as usize;
                let sx = sx.min(src_w - 1);
                let sy = sy.min(src_h - 1);
                let pixel = (sy * src_w + sx) * 4;
                if pixel + 2 < rgba.len() {
                    leds[row * COLS + col] = (rgba[pixel], rgba[pixel + 1], rgba[pixel + 2]);
                }
            }
        }

        frames.push(Frame { leds, delay_ms });
    }

    if frames.is_empty() {
        return Err("GIF has no frames".into());
    }

    println!(
        "Decoded {} frames, streaming at {}",
        frames.len(),
        if let Some(f) = fps {
            format!("{f:.1} FPS (override)")
        } else {
            "GIF timing".to_string()
        }
    );

    let running = setup_interrupt_handler();
    println!("Streaming (Ctrl+C to stop)...");

    loop {
        for (idx, frame) in frames.iter().enumerate() {
            if !running.load(Ordering::SeqCst) {
                break;
            }

            send_full_frame(&kb, &frame.leds)?;

            print!("\rFrame {:3}/{}", idx + 1, frames.len());
            std::io::Write::flush(&mut std::io::stdout()).ok();

            std::thread::sleep(std::time::Duration::from_millis(frame.delay_ms));
        }

        if !loop_anim || !running.load(Ordering::SeqCst) {
            break;
        }
    }

    println!("\nReleasing LED stream...");
    kb.stream_led_release().ok();
    println!("Done.");
    Ok(())
}
