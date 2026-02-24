//! Notification daemon — D-Bus server + render loop + LED writer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::dbus::{NotifyInterface, SharedStore};
use super::keymap::MATRIX_LEN;
use super::state::{self, NotificationStore};
use crate::effect::EffectLibrary;

const LEDS_PER_PAGE: usize = 18;
const PAGE_COUNT: usize = MATRIX_LEN.div_ceil(LEDS_PER_PAGE);

/// Estimated current draw per WS2812 channel at full brightness.
const MA_PER_CHANNEL: f32 = 20.0;

/// Send a full frame of RGB data to the keyboard via 0xFC streaming.
fn send_full_frame(
    kb: &monsgeek_keyboard::SyncKeyboard,
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

/// Scale LED values to stay within a power budget (milliamps).
fn apply_power_budget(leds: &mut [(u8, u8, u8); MATRIX_LEN], budget_ma: u32) {
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
    }
}

/// Run the notification daemon (blocking).
///
/// The caller is responsible for opening the keyboard with patch check.
/// This function:
/// - Loads the effect library from `~/.config/monsgeek/effects.toml`
/// - Starts a D-Bus server on `org.monsgeek.Notify1`
/// - Runs a render loop at the specified FPS
/// - Releases LEDs on Ctrl-C
pub async fn run(
    kb: monsgeek_keyboard::SyncKeyboard,
    fps: u32,
    power_budget: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load effect library
    let effects = EffectLibrary::load_default().map_err(|e| format!("load effects: {e}"))?;
    println!(
        "Effects: {} loaded from {}",
        effects.effects.len(),
        crate::effect::default_effects_path().display()
    );
    for name in effects.names() {
        println!("  - {name}");
    }

    // Shared state
    let store: SharedStore = Arc::new(Mutex::new(NotificationStore::new()));
    let effects = Arc::new(effects);

    // Set up Ctrl-C handler
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);
    ctrlc::set_handler(move || {
        running_clone.store(false, Ordering::SeqCst);
    })
    .ok();

    // Start D-Bus server
    let dbus_store = Arc::clone(&store);
    let dbus_effects = Arc::clone(&effects);
    let conn = zbus::connection::Builder::session()?
        .name("org.monsgeek.Notify1")?
        .serve_at(
            "/org/monsgeek/Notify1",
            NotifyInterface::new(dbus_store, dbus_effects),
        )?
        .build()
        .await?;

    println!("D-Bus: org.monsgeek.Notify1 on session bus");
    println!(
        "Render: {} FPS, power budget {}",
        fps,
        if power_budget > 0 {
            format!("{power_budget}mA")
        } else {
            "unlimited".to_string()
        }
    );
    println!("Ready. Ctrl+C to stop.");

    // Render loop
    let frame_duration = std::time::Duration::from_secs_f64(1.0 / fps as f64);
    let mut interval = tokio::time::interval(frame_duration);

    while running.load(Ordering::SeqCst) {
        interval.tick().await;

        let mut frame = {
            let mut store_guard = store.lock().await;
            store_guard.expire();
            state::render_frame(&store_guard)
        };

        apply_power_budget(&mut frame, power_budget);

        if let Err(e) = send_full_frame(&kb, &frame) {
            eprintln!("LED write error: {e}");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    // Cleanup
    println!("\nReleasing LED stream...");
    kb.stream_led_release().ok();
    drop(conn);
    println!("Done.");
    Ok(())
}
