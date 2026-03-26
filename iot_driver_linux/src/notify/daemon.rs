//! Notification daemon — D-Bus server + render loop + LED writer.
//!
//! Two modes of operation:
//! - **Animation engine** (preferred): compiles effects into firmware keyframes,
//!   sends once, firmware ticks autonomously at 200Hz. No USB traffic during playback.
//! - **Streaming fallback**: evaluates effects on host at 60fps, sends sparse
//!   RGB deltas over USB. Used when firmware lacks animation engine support.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::dbus::{NotifyInterface, SharedStore};
use super::keymap::MATRIX_LEN;
use super::state::{self, NotificationStore};
use crate::anim::{self, AnimEngine, SharedSlotInfo, SlotEntry};
use crate::effect::EffectLibrary;
use crate::led_stream::{apply_power_budget, send_overlay_diff};

/// Run the notification daemon (blocking, standalone CLI entry point).
///
/// Opens its own Ctrl-C handler. For TUI integration, use `run_with_cancel` instead.
pub async fn run(
    kb: monsgeek_keyboard::KeyboardInterface,
    power_budget: u32,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);
    ctrlc::set_handler(move || r.store(false, Ordering::SeqCst)).ok();
    let slot_info = Arc::new(std::sync::Mutex::new(crate::anim::SlotInfo::default()));
    let log = super::log::DaemonLog::new(verbose);
    run_with_cancel(Arc::new(kb), power_budget, running, slot_info, log).await
}

/// Tracks firmware animation slot allocation.
struct AnimSlotManager {
    /// For each firmware def slot (0-7), the notification ID using it (or None).
    slots: [Option<u64>; 8],
}

impl AnimSlotManager {
    fn new() -> Self {
        Self { slots: [None; 8] }
    }

    /// Allocate a slot, returning its index. Evicts lowest-priority if full.
    fn allocate(&mut self, notif_id: u64) -> Option<u8> {
        // First: find empty slot
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(notif_id);
                return Some(i as u8);
            }
        }
        None // all full — caller should fall back to streaming
    }

    /// Free a slot by notification ID.
    fn free_by_notif(&mut self, notif_id: u64) -> Option<u8> {
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if *slot == Some(notif_id) {
                *slot = None;
                return Some(i as u8);
            }
        }
        None
    }
}

/// Diagonal magenta flash from Esc to bottom-right on daemon startup.
fn startup_animation(engine: &AnimEngine) {
    use crate::effect::{fw_easing, fw_flags, rgb_to_565};

    // One-shot magenta pulse: instant on → exponential decay over 400ms (40 ticks)
    let magenta_565 = rgb_to_565(255, 0, 255);
    let keyframes = vec![
        (0u16, magenta_565, fw_easing::OUT_EXPO), // t=0: full magenta, decay to next
        (40u16, 0u16, fw_easing::LINEAR),         // t=40: black (end)
    ];

    // Total duration: animation (40 ticks) + max stagger (~20 ticks for diagonal)
    if engine
        .kb()
        .anim_define(7, fw_flags::ONE_SHOT, -128, 40, &keyframes)
        .is_err()
    {
        return;
    }

    // Assign all keys with diagonal stagger: phase = (row + col) * 1
    // At 100Hz, phase_offset=1 → 8 ticks = 80ms per diagonal step
    let mut keys = Vec::new();
    for row in 0..6u8 {
        for col in 0..16u8 {
            let matrix_idx = row * 16 + col;
            let phase = row + col; // diagonal distance from Esc
            keys.push((matrix_idx, phase));
        }
    }

    let _ = engine.kb().anim_assign(7, &keys);

    // Cancel after animation completes: max_phase(20)*8 + duration(40) = 200 ticks ≈ 2s
    let kb = engine.kb_arc();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let _ = kb.anim_cancel(7);
    });
}

/// Program a notification into the firmware animation engine.
///
/// Returns true if successful, false if fallback needed.
fn program_notification(engine: &AnimEngine, notif: &state::Notification, def_id: u8) -> bool {
    let one_shot = notif.ttl.is_some() && notif.resolved.duration_ms > 0.0;
    let priority = notif.priority.clamp(-128, 127) as i8;

    let keys: Vec<(u8, u8)> = notif
        .matrix_indices
        .iter()
        .map(|&idx| {
            let stagger_ms = notif.stagger_offsets.get(&idx).copied().unwrap_or(0.0);
            (idx as u8, anim::ms_to_phase_offset(stagger_ms))
        })
        .collect();

    engine
        .program(def_id, &notif.resolved, priority, one_shot, &keys)
        .is_ok()
}

/// Run the notification daemon with an externally-controlled cancellation flag.
///
/// The keyboard interface is shared via `Arc` so the TUI can use the same
/// HID handle for vendor commands while the daemon streams LEDs.
///
/// Returns when `running` is set to `false`.
pub async fn run_with_cancel(
    kb: Arc<monsgeek_keyboard::KeyboardInterface>,
    power_budget: u32,
    running: Arc<AtomicBool>,
    slot_info: SharedSlotInfo,
    log: super::log::DaemonLog,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Load effect library
    let effects = EffectLibrary::load_default().map_err(|e| format!("load effects: {e}"))?;
    info!(
        count = effects.effects.len(),
        path = %crate::effect::default_effects_path().display(),
        "Effects loaded"
    );
    for name in effects.names() {
        debug!(effect = name, "Registered effect");
    }

    // Detect animation engine support
    let engine = AnimEngine::new(Arc::clone(&kb));
    let has_anim = engine.is_available();
    if has_anim {
        engine.clear().ok();
        startup_animation(&engine);
        log.push("animation engine ready");
    } else {
        log.push("no animation engine — streaming fallback");
    }

    // Shared state
    let store: SharedStore = Arc::new(Mutex::new(NotificationStore::new()));
    let effects = Arc::new(effects);

    // Pending wave queue for repeated-key text animations
    let pending_waves: super::dbus::PendingWaveQueue = Arc::new(Mutex::new(Vec::new()));

    // Wake signal: D-Bus handler signals daemon loop on store changes
    let wake: super::dbus::WakeSignal = Arc::new(tokio::sync::Notify::new());

    // Start D-Bus server
    let dbus_store = Arc::clone(&store);
    let dbus_effects = Arc::clone(&effects);
    let dbus_waves = Arc::clone(&pending_waves);
    let dbus_wake = Arc::clone(&wake);
    let conn = zbus::connection::Builder::session()?
        .name("org.monsgeek.Notify1")?
        .serve_at(
            "/org/monsgeek/Notify1",
            NotifyInterface::new(dbus_store, dbus_effects, dbus_waves, dbus_wake, log.clone()),
        )?
        .build()
        .await?;

    info!("D-Bus: org.monsgeek.Notify1 on session bus");
    info!(
        power_budget = if power_budget > 0 {
            format!("{power_budget}mA")
        } else {
            "unlimited".into()
        },
        "Render loop started"
    );

    // Expiry/wave timer — only needed for TTL expiry and pending wave processing.
    let expiry_interval = std::time::Duration::from_secs(1);
    let wave_interval = std::time::Duration::from_millis(10);
    let mut timer = tokio::time::interval(expiry_interval);
    let mut prev_frame = [(0u8, 0u8, 0u8); MATRIX_LEN];

    // Animation engine state
    let mut slots = AnimSlotManager::new();
    let mut programmed: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut prev_state_count: usize = 0;
    let mut last_sync = std::time::Instant::now();
    let sync_interval = std::time::Duration::from_secs(3);

    while running.load(Ordering::SeqCst) {
        // Wait for either: D-Bus wake signal or timer tick (expiry/waves)
        tokio::select! {
            _ = wake.notified() => {}
            _ = timer.tick() => {}
        }

        // Periodic sync: verify firmware state matches daemon expectations.
        // Catches keyboard sleep/wake (firmware resets, daemon still thinks slots are active).
        if has_anim && last_sync.elapsed() >= sync_interval && !programmed.is_empty() {
            last_sync = std::time::Instant::now();
            if let Ok(Some(status)) = engine.kb().anim_query() {
                let fw_active = status.active_count;
                let expected = programmed.len() as u8;
                if fw_active == 0 && expected > 0 {
                    // Firmware lost all animations (sleep/wake reset)
                    log.push("sync: firmware reset detected, reprogramming");
                    slots = AnimSlotManager::new();
                    programmed.clear();
                    slot_info.lock().unwrap().clear_all();
                }
            }
        }

        let mut store_guard = store.lock().await;

        // Expire notifications
        let expired = store_guard.expire();
        if has_anim {
            for id in &expired {
                if let Some(def_id) = slots.free_by_notif(*id) {
                    engine.cancel(def_id).ok();
                    slot_info.lock().unwrap().clear(def_id);
                    log.push(format!("expired id={id} → cancel slot {def_id}"));
                }
                programmed.remove(id);
            }

            // Cancel animations for ack'd/removed notifications
            let active_ids: std::collections::HashSet<u64> = store_guard
                .list()
                .iter()
                .map(|&(id, _, _, _, _)| id)
                .collect();
            let removed: Vec<u64> = programmed
                .iter()
                .copied()
                .filter(|id| !active_ids.contains(id))
                .collect();
            for id in removed {
                if let Some(def_id) = slots.free_by_notif(id) {
                    engine.cancel(def_id).ok();
                    slot_info.lock().unwrap().clear(def_id);
                    log.push(format!("ack id={id} → cancel slot {def_id}"));
                }
                programmed.remove(&id);
            }
        }

        if has_anim {
            // Animation engine mode: program new notifications, skip rendering
            let mut needs_streaming = false;

            for &(id, _, _, _, _) in &store_guard.list() {
                if programmed.contains(&id) {
                    continue;
                }

                let Some(notif) = store_guard.get(id) else {
                    continue;
                };

                // Compile to check for slot reuse
                let one_shot = notif.ttl.is_some() && notif.resolved.duration_ms > 0.0;
                let priority = notif.priority.clamp(-128, 127) as i8;
                let compiled = match notif.resolved.compile_for_firmware(priority, one_shot) {
                    Some(c) => c,
                    None => continue,
                };

                // Try to join an existing def with identical compiled output.
                // For one-shot: phase offset = original slot × stagger, so the
                // key starts at the right point in the def's elapsed timeline.
                let existing_slot = {
                    let si = slot_info.lock().unwrap();
                    (0..8u8).find(|&s| si.get(s).is_some_and(|e| e.compiled == compiled))
                };

                if let Some(def_id) = existing_slot {
                    let keys: Vec<(u8, u8)> = notif
                        .matrix_indices
                        .iter()
                        .map(|&idx| {
                            let stagger_ms =
                                notif.stagger_offsets.get(&idx).copied().unwrap_or(0.0);
                            (idx as u8, anim::ms_to_phase_offset(stagger_ms))
                        })
                        .collect();
                    let _ = engine.kb().anim_assign(def_id, &keys);
                    slots.slots[def_id as usize] = Some(id);
                    programmed.insert(id);
                    log.push(format!(
                        "join {} → slot {} ({} keys)",
                        notif.effect_name,
                        def_id,
                        keys.len()
                    ));
                } else if let Some(def_id) = slots.allocate(id) {
                    if program_notification(&engine, notif, def_id) {
                        programmed.insert(id);
                        slot_info.lock().unwrap().set(
                            def_id,
                            SlotEntry {
                                effect_name: notif.effect_name.clone(),
                                resolved: notif.resolved.clone(),
                                compiled: compiled.clone(),
                            },
                        );
                        log.push(format!(
                            "upload {} → slot {} ({} keys)",
                            notif.effect_name,
                            def_id,
                            notif.matrix_indices.len()
                        ));
                    } else {
                        slots.free_by_notif(id);
                        needs_streaming = true;
                        log.push(format!("upload {} failed, streaming", notif.effect_name));
                    }
                } else {
                    needs_streaming = true;
                }
            }

            // Process pending waves (repeated-key reassignments)
            {
                let now = std::time::Instant::now();
                let mut waves = pending_waves.lock().await;
                waves.retain(|wave| {
                    if now < wave.send_at {
                        return true; // not yet
                    }
                    // Find the def slot running this animation
                    let def_id = {
                        let si = slot_info.lock().unwrap();
                        (0..8u8).find(|&s| si.get(s).is_some_and(|e| e.compiled == wave.compiled))
                    };
                    if let Some(def_id) = def_id {
                        let keys: Vec<(u8, u8)> = wave
                            .indices
                            .iter()
                            .zip(&wave.slots)
                            .map(|(&idx, &slot)| {
                                (
                                    idx as u8,
                                    anim::ms_to_phase_offset(slot as f64 * wave.stagger_ms),
                                )
                            })
                            .collect();
                        let _ = engine.kb().anim_assign(def_id, &keys);
                        log.push(format!(
                            "wave reassign slot {} ({} keys)",
                            def_id,
                            keys.len()
                        ));
                    }
                    false // remove from queue
                });
                // Speed up loop while waves are pending
                // Speed up timer while waves are pending
                let new_dur = if waves.is_empty() {
                    expiry_interval
                } else {
                    wave_interval
                };
                if timer.period() != new_dur {
                    timer = tokio::time::interval(new_dur);
                    timer.reset();
                }
            }

            // Print state summary when something changed (verbose only)
            let current_count = store_guard.list().len() + programmed.len();
            if current_count != prev_state_count {
                log.print_state(&store_guard.list());
                prev_state_count = current_count;
            }

            if !needs_streaming {
                drop(store_guard);
                continue;
            }
            // Fall through to streaming for unprogrammable notifications
        }

        let mut frame = state::render_frame(&store_guard);
        drop(store_guard);

        apply_power_budget(&mut frame, power_budget);

        if frame != prev_frame {
            let send_ok = send_overlay_diff(&kb, &prev_frame, &frame).is_ok();
            if !send_ok {
                warn!("LED write error");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            prev_frame = frame;
        }
    }

    // Cleanup: clear overlay so animation shows through cleanly
    debug!("Clearing overlay");
    if has_anim {
        engine.clear().ok();
        slot_info.lock().unwrap().clear_all();
    } else {
        kb.stream_led_release().ok();
    }
    drop(conn);
    debug!("Daemon stopped");
    Ok(())
}
