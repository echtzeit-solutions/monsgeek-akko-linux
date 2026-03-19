//! Notification daemon — D-Bus server + render loop + LED writer.
//!
//! Uses the persistent additive overlay: only sends LED updates when the
//! rendered frame actually changes, avoiding constant USB traffic that
//! interferes with keyboard scan timing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::dbus::{NotifyInterface, SharedStore};
use super::keymap::MATRIX_LEN;
use super::state::{self, NotificationStore};
use crate::effect::EffectLibrary;
use crate::led_stream::{apply_power_budget, send_overlay_diff};

/// Run the notification daemon (blocking).
///
/// The caller is responsible for opening the keyboard with patch check.
/// This function:
/// - Loads the effect library from `~/.config/monsgeek/effects.toml`
/// - Starts a D-Bus server on `org.monsgeek.Notify1`
/// - Runs a render loop at the specified FPS
/// - Only sends LED updates when the frame changes (persistent overlay)
/// - Clears overlay on Ctrl-C
pub async fn run(
    kb: monsgeek_keyboard::KeyboardInterface,
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
    let effective_fps = fps.min(10);
    println!(
        "Render: {} FPS (sparse delta), power budget {}",
        effective_fps,
        if power_budget > 0 {
            format!("{power_budget}mA")
        } else {
            "unlimited".to_string()
        }
    );
    println!("Ready. Ctrl+C to stop.");

    // Render loop — sparse delta sends at capped FPS.
    // Cap at 10 FPS max to minimize USB interrupt load (prevents stuck keys).
    let frame_duration = std::time::Duration::from_secs_f64(1.0 / effective_fps as f64);
    let mut interval = tokio::time::interval(frame_duration);
    let mut prev_frame = [(0u8, 0u8, 0u8); MATRIX_LEN];

    while running.load(Ordering::SeqCst) {
        interval.tick().await;

        let mut frame = {
            let mut store_guard = store.lock().await;
            store_guard.expire();
            state::render_frame(&store_guard)
        };

        apply_power_budget(&mut frame, power_budget);

        if frame != prev_frame {
            if let Err(e) = send_overlay_diff(&kb, &prev_frame, &frame) {
                eprintln!("LED write error: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            prev_frame = frame;
        }
    }

    // Cleanup: clear overlay so animation shows through cleanly
    println!("\nClearing overlay...");
    kb.stream_led_release().ok();
    drop(conn);
    println!("Done.");
    Ok(())
}
