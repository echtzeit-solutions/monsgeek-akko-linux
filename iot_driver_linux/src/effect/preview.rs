//! Terminal preview for LED effects using crossterm.
//!
//! Renders a 16x6 keyboard grid in an alternate screen with true-color.
//! Targeted keys show animated color+brightness; others are dark gray.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::{
    cursor, event,
    style::{self, Color, Stylize},
    terminal, ExecutableCommand, QueueableCommand,
};

use super::{resolve, EffectDef, ResolvedEffect};
use crate::notify::keymap::{COLS, MATRIX_LEN, ROWS};
use crate::profile::M1_V5_HE_KEY_NAMES;

/// Width of each cell in characters.
const CELL_W: usize = 5;
/// Dark gray for inactive keys.
const DIM: Color = Color::Rgb {
    r: 40,
    g: 40,
    b: 40,
};
/// Background color.
const BG: Color = Color::Rgb {
    r: 20,
    g: 20,
    b: 20,
};

/// Run the terminal preview. Blocks until q/Esc is pressed.
pub fn run(
    def: &EffectDef,
    key_indices: &[usize],
    vars: &BTreeMap<String, String>,
    fps: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve(def, vars).map_err(|e| format!("resolve effect: {e}"))?;
    let fps = fps.clamp(1, 60);
    let frame_dur = Duration::from_secs_f64(1.0 / fps as f64);

    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    stdout
        .execute(terminal::EnterAlternateScreen)?
        .execute(cursor::Hide)?;

    let result = run_loop(&mut stdout, &resolved, key_indices, frame_dur, def);

    // Cleanup
    stdout
        .execute(cursor::Show)?
        .execute(terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    result
}

fn run_loop(
    stdout: &mut io::Stdout,
    resolved: &ResolvedEffect,
    key_indices: &[usize],
    frame_dur: Duration,
    def: &EffectDef,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();

    // Build key label lookup: matrix_index -> 3-char label
    let labels = build_labels();

    loop {
        // Check for quit
        if event::poll(Duration::ZERO)? {
            if let event::Event::Key(key) = event::read()? {
                match key.code {
                    event::KeyCode::Char('q') | event::KeyCode::Esc => break,
                    event::KeyCode::Char('c')
                        if key.modifiers.contains(event::KeyModifiers::CONTROL) =>
                    {
                        break
                    }
                    _ => {}
                }
            }
        }

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        let rgb = resolved.evaluate(elapsed_ms);

        // Header
        stdout.queue(cursor::MoveTo(0, 0))?;
        stdout.queue(style::PrintStyledContent(
            format!(
                " Effect: {}  |  {:5.0}ms  |  q/Esc to quit ",
                def.name, elapsed_ms
            )
            .with(Color::White)
            .on(Color::DarkGrey),
        ))?;

        // Draw grid
        for row in 0..ROWS {
            stdout.queue(cursor::MoveTo(0, (row + 2) as u16))?;
            for col in 0..COLS {
                let idx = row * COLS + col;
                let label = &labels[idx];

                let (fg, bg) = if key_indices.contains(&idx) {
                    // Active key — show effect color
                    let lum = (rgb.r as u16 + rgb.g as u16 + rgb.b as u16) / 3;
                    let fg = if lum > 128 {
                        Color::Black
                    } else {
                        Color::White
                    };
                    let bg = Color::Rgb {
                        r: rgb.r,
                        g: rgb.g,
                        b: rgb.b,
                    };
                    (fg, bg)
                } else if label.trim().is_empty() {
                    // Gap position
                    (DIM, BG)
                } else {
                    // Inactive key
                    (Color::DarkGrey, DIM)
                };

                stdout.queue(style::PrintStyledContent(
                    format!("{:^width$}", label, width = CELL_W).with(fg).on(bg),
                ))?;
            }
        }

        // Color info line
        stdout.queue(cursor::MoveTo(0, (ROWS + 3) as u16))?;
        stdout.queue(style::PrintStyledContent(
            format!(
                " RGB({:3},{:3},{:3})  #{:02X}{:02X}{:02X} ",
                rgb.r, rgb.g, rgb.b, rgb.r, rgb.g, rgb.b
            )
            .with(Color::White)
            .on(Color::Rgb {
                r: rgb.r,
                g: rgb.g,
                b: rgb.b,
            }),
        ))?;

        stdout.flush()?;
        std::thread::sleep(frame_dur);
    }

    Ok(())
}

/// Build 3-char labels for each matrix position from the key name table.
fn build_labels() -> Vec<String> {
    let mut labels = vec![String::new(); MATRIX_LEN];

    // M1_V5_HE_KEY_NAMES is column-major: index = col * 6 + row
    for (col_major_idx, &name) in M1_V5_HE_KEY_NAMES.iter().enumerate() {
        if col_major_idx >= MATRIX_LEN {
            break;
        }
        let col = col_major_idx / ROWS;
        let row = col_major_idx % ROWS;
        let matrix_idx = row * COLS + col;
        if matrix_idx < MATRIX_LEN {
            // Truncate to 3 chars for display
            let label: String = name.chars().take(3).collect();
            labels[matrix_idx] = label;
        }
    }

    labels
}

/// Play an effect directly on the keyboard hardware.
///
/// Sends RGB frames at ~30 FPS using the 0xFC patch protocol.
/// The caller is responsible for Ctrl-C handling and LED release.
pub fn play_on_hardware(
    kb: &monsgeek_keyboard::SyncKeyboard,
    resolved: &ResolvedEffect,
    key_indices: &[usize],
    running: &std::sync::atomic::AtomicBool,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::atomic::Ordering;

    const LEDS_PER_PAGE: usize = 18;
    const PAGE_COUNT: usize = MATRIX_LEN.div_ceil(LEDS_PER_PAGE);
    const MA_PER_CHANNEL: f32 = 20.0;

    let start = Instant::now();
    let frame_dur = Duration::from_millis(33); // ~30 FPS

    while running.load(Ordering::SeqCst) {
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        let rgb = resolved.evaluate(elapsed_ms);

        let mut leds = [(0u8, 0u8, 0u8); MATRIX_LEN];
        for &idx in key_indices {
            if idx < MATRIX_LEN {
                leds[idx] = (rgb.r, rgb.g, rgb.b);
            }
        }

        // Apply power budget (400mA)
        let ma_per_unit = MA_PER_CHANNEL / 255.0;
        let total_ma: f32 = leds
            .iter()
            .map(|&(r, g, b)| (r as f32 + g as f32 + b as f32) * ma_per_unit)
            .sum();
        if total_ma > 400.0 {
            let scale = 400.0 / total_ma;
            for led in leds.iter_mut() {
                led.0 = (led.0 as f32 * scale) as u8;
                led.1 = (led.1 as f32 * scale) as u8;
                led.2 = (led.2 as f32 * scale) as u8;
            }
        }

        // Send frame via 0xFC pages
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

        print!(
            "\rRGB({:3},{:3},{:3}) {:6.0}ms",
            rgb.r, rgb.g, rgb.b, elapsed_ms
        );
        io::stdout().flush().ok();

        std::thread::sleep(frame_dur);
    }

    Ok(())
}
