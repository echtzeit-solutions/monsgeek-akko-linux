//! Trigger-related command handlers.

use super::CommandResult;
use iot_driver::protocol::magnetism;
use monsgeek_keyboard::{KeyMode, KeyTriggerSettings, SyncKeyboard};

/// Run calibration (min + max)
pub fn calibrate() -> CommandResult {
    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            println!("Starting calibration...");
            println!("Step 1: Calibrating minimum (released) position...");
            println!("        Keep all keys released!");
            let _ = keyboard.calibrate_min(true);
            std::thread::sleep(std::time::Duration::from_secs(2));
            let _ = keyboard.calibrate_min(false);
            println!("        Done.");
            println!();
            println!("Step 2: Calibrating maximum (pressed) position...");
            println!("        Press and hold ALL keys firmly for 3 seconds!");
            let _ = keyboard.calibrate_max(true);
            std::thread::sleep(std::time::Duration::from_secs(3));
            let _ = keyboard.calibrate_max(false);
            println!("        Done.");
            println!();
            println!("Calibration complete!");
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}

/// Show current trigger settings
pub fn triggers() -> CommandResult {
    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            let version = keyboard.get_version().unwrap_or_default();
            let precision = keyboard.get_precision().unwrap_or_default();
            let factor = precision.factor() as f32;
            println!(
                "Trigger Settings (firmware {}, precision: {})",
                version.format(),
                precision.as_str()
            );
            println!();

            match keyboard.get_all_triggers() {
                Ok(triggers) => {
                    let decode_u16 = |data: &[u8], idx: usize| -> u16 {
                        if idx * 2 + 1 < data.len() {
                            u16::from_le_bytes([data[idx * 2], data[idx * 2 + 1]])
                        } else {
                            0
                        }
                    };

                    let first_press = decode_u16(&triggers.press_travel, 0);
                    let first_lift = decode_u16(&triggers.lift_travel, 0);
                    let first_rt_press = decode_u16(&triggers.rt_press, 0);
                    let first_rt_lift = decode_u16(&triggers.rt_lift, 0);
                    let first_mode = triggers.key_modes.first().copied().unwrap_or(0);

                    let num_keys = triggers
                        .key_modes
                        .len()
                        .min(triggers.press_travel.len() / 2);

                    println!("First key settings (as sample):");
                    println!(
                        "  Actuation:     {:.1}mm (raw: {})",
                        first_press as f32 / factor,
                        first_press
                    );
                    println!(
                        "  Release:       {:.1}mm (raw: {})",
                        first_lift as f32 / factor,
                        first_lift
                    );
                    println!(
                        "  RT Press:      {:.2}mm (raw: {})",
                        first_rt_press as f32 / factor,
                        first_rt_press
                    );
                    println!(
                        "  RT Release:    {:.2}mm (raw: {})",
                        first_rt_lift as f32 / factor,
                        first_rt_lift
                    );
                    println!(
                        "  Mode:          {} ({})",
                        first_mode,
                        magnetism::mode_name(first_mode)
                    );
                    println!();

                    let all_same_press =
                        (0..num_keys).all(|i| decode_u16(&triggers.press_travel, i) == first_press);
                    let all_same_mode = triggers
                        .key_modes
                        .iter()
                        .take(num_keys)
                        .all(|&v| v == first_mode);

                    if all_same_press && all_same_mode {
                        println!("All {num_keys} keys have identical settings");
                    } else {
                        println!("Keys have varying settings ({num_keys} keys total)");
                        println!("\nFirst 10 key values:");
                        for i in 0..10.min(num_keys) {
                            let press = decode_u16(&triggers.press_travel, i);
                            let mode = triggers.key_modes.get(i).copied().unwrap_or(0);
                            println!(
                                "  Key {:2}: {:.1}mm mode={}",
                                i,
                                press as f32 / factor,
                                mode
                            );
                        }
                    }
                }
                Err(e) => eprintln!("Failed to read trigger settings: {e}"),
            }
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}

/// Set actuation point for all keys
pub fn set_actuation(mm: f32) -> CommandResult {
    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            let precision = keyboard.get_precision().unwrap_or_default();
            let factor = precision.factor() as f32;
            let raw = (mm * factor) as u16;
            match keyboard.set_actuation_all_u16(raw) {
                Ok(_) => println!("Actuation point set to {mm:.2}mm (raw: {raw}) for all keys"),
                Err(e) => eprintln!("Failed to set actuation point: {e}"),
            }
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}

/// Enable/disable Rapid Trigger or set sensitivity
pub fn set_rt(value: &str) -> CommandResult {
    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            let precision = keyboard.get_precision().unwrap_or_default();
            let factor = precision.factor() as f32;

            match value.to_lowercase().as_str() {
                "off" | "0" | "disable" => match keyboard.set_rapid_trigger_all(false) {
                    Ok(_) => println!("Rapid Trigger disabled for all keys"),
                    Err(e) => eprintln!("Failed to disable Rapid Trigger: {e}"),
                },
                "on" | "enable" => {
                    let sensitivity = (0.3 * factor) as u16;
                    let _ = keyboard.set_rapid_trigger_all(true);
                    let _ = keyboard.set_rt_press_all_u16(sensitivity);
                    let _ = keyboard.set_rt_lift_all_u16(sensitivity);
                    println!("Rapid Trigger enabled with 0.3mm sensitivity for all keys");
                }
                _ => {
                    let mm: f32 = value.parse().unwrap_or(0.3);
                    let sensitivity = (mm * factor) as u16;
                    let _ = keyboard.set_rapid_trigger_all(true);
                    let _ = keyboard.set_rt_press_all_u16(sensitivity);
                    let _ = keyboard.set_rt_lift_all_u16(sensitivity);
                    println!("Rapid Trigger enabled with {mm:.2}mm sensitivity for all keys");
                }
            }
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}

/// Set release point for all keys
pub fn set_release(mm: f32) -> CommandResult {
    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            let precision = keyboard.get_precision().unwrap_or_default();
            let factor = precision.factor() as f32;
            let raw = (mm * factor) as u16;
            match keyboard.set_release_all_u16(raw) {
                Ok(_) => println!("Release point set to {mm:.2}mm (raw: {raw}) for all keys"),
                Err(e) => eprintln!("Failed to set release point: {e}"),
            }
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}

/// Set bottom deadzone for all keys
pub fn set_bottom_deadzone(mm: f32) -> CommandResult {
    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            let precision = keyboard.get_precision().unwrap_or_default();
            let factor = precision.factor() as f32;
            let raw = (mm * factor) as u16;
            match keyboard.set_bottom_deadzone_all_u16(raw) {
                Ok(_) => println!("Bottom deadzone set to {mm:.2}mm (raw: {raw}) for all keys"),
                Err(e) => eprintln!("Failed to set bottom deadzone: {e}"),
            }
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}

/// Set top deadzone for all keys
pub fn set_top_deadzone(mm: f32) -> CommandResult {
    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            let precision = keyboard.get_precision().unwrap_or_default();
            let factor = precision.factor() as f32;
            let raw = (mm * factor) as u16;
            match keyboard.set_top_deadzone_all_u16(raw) {
                Ok(_) => println!("Top deadzone set to {mm:.2}mm (raw: {raw}) for all keys"),
                Err(e) => eprintln!("Failed to set top deadzone: {e}"),
            }
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}

/// Set trigger settings for a specific key
pub fn set_key_trigger(
    key: u8,
    actuation: Option<f32>,
    release: Option<f32>,
    mode: Option<String>,
) -> CommandResult {
    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            // Get current settings first
            let current = match keyboard.get_key_trigger(key) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to get current settings for key {key}: {e}");
                    return Ok(());
                }
            };

            let precision = keyboard.get_precision().unwrap_or_default();
            // Note: Single-key protocol uses u8, with factor of 10 (0.1mm steps)
            let factor = 10.0f32;

            // Build settings with modifications
            let settings = KeyTriggerSettings {
                key_index: key,
                actuation: actuation
                    .map(|mm| (mm * factor) as u8)
                    .unwrap_or(current.actuation),
                deactuation: release
                    .map(|mm| (mm * factor) as u8)
                    .unwrap_or(current.deactuation),
                mode: mode
                    .as_ref()
                    .map(|m| match m.to_lowercase().as_str() {
                        "normal" | "n" => KeyMode::Normal,
                        "rt" | "rapid" | "rapidtrigger" => KeyMode::RapidTrigger,
                        "dks" | "dynamic" => KeyMode::DynamicKeystroke,
                        "snaptap" | "snap" | "st" => KeyMode::SnapTap,
                        "modtap" | "mt" => KeyMode::ModTap,
                        "toggle" | "tgl" => KeyMode::ToggleHold,
                        _ => current.mode,
                    })
                    .unwrap_or(current.mode),
            };

            match keyboard.set_key_trigger(&settings) {
                Ok(_) => {
                    println!("Key {key} trigger settings updated:");
                    println!(
                        "  Actuation: {:.1}mm, Release: {:.1}mm, Mode: {:?}",
                        settings.actuation as f32 / factor,
                        settings.deactuation as f32 / factor,
                        settings.mode
                    );
                    println!(
                        "  (precision: {}, bulk commands use higher precision)",
                        precision.as_str()
                    );
                }
                Err(e) => eprintln!("Failed to set key trigger: {e}"),
            }
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}
