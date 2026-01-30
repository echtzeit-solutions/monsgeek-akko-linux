//! Trigger-related command handlers.

use super::{with_keyboard, CommandResult};
use iot_driver::profile::M1_V5_HE_KEY_NAMES;
use iot_driver::protocol::magnetism;
use monsgeek_keyboard::{KeyMode, KeyTriggerSettings, SyncKeyboard};
use std::collections::HashSet;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Run calibration (min + max) with per-key progress display
pub fn calibrate() -> CommandResult {
    let keyboard = match SyncKeyboard::open_any() {
        Ok(kb) => kb,
        Err(e) => {
            eprintln!("No device found: {e}");
            return Ok(());
        }
    };

    let key_count = keyboard.key_count() as usize;

    // Set up Ctrl+C handler
    let interrupted = Arc::new(AtomicBool::new(false));
    let int_clone = interrupted.clone();
    if let Err(e) = ctrlc::set_handler(move || {
        int_clone.store(true, Ordering::SeqCst);
    }) {
        eprintln!("Warning: Could not set Ctrl+C handler: {e}");
    }

    println!("Starting calibration for {} keys...", key_count);
    println!("  Ctrl+C = abort (no save)");
    println!("  Press Enter during max calibration = save partial and exit");
    println!();

    // Phase 1: Min calibration (released position)
    println!("Step 1: Calibrating minimum (released) position");
    println!("        Keep all keys RELEASED for 2 seconds...");

    if let Err(e) = keyboard.calibrate_min(true) {
        eprintln!("Failed to start min calibration: {e}");
        return Ok(());
    }

    // Show countdown (check for interrupt)
    for i in (1..=2).rev() {
        if interrupted.load(Ordering::SeqCst) {
            println!("\n\nAborted during min calibration.");
            let _ = keyboard.calibrate_min(false);
            return Ok(());
        }
        print!("\r        {} seconds remaining...", i);
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_secs(1));
    }
    println!("\r        Done.                    ");
    let _ = keyboard.calibrate_min(false);

    // Phase 2: Max calibration with progress display
    println!();
    println!("Step 2: Calibrating maximum (pressed) position");
    println!("        Press ALL keys firmly and hold...");

    if let Err(e) = keyboard.calibrate_max(true) {
        eprintln!("Failed to start max calibration: {e}");
        return Ok(());
    }

    // Poll and display progress
    let mut finished = HashSet::new();
    let pages = key_count.div_ceil(32);

    // Set stdin to non-blocking for checking Enter key
    let stdin_check = setup_stdin_nonblocking();

    loop {
        // Check for Ctrl+C (abort)
        if interrupted.load(Ordering::SeqCst) {
            let _ = keyboard.calibrate_max(false);
            println!("\n\nCalibration aborted (not saved).");
            restore_stdin(&stdin_check);
            return Ok(());
        }

        // Check for Enter key (graceful end with partial save)
        if check_stdin_ready(&stdin_check) {
            let _ = keyboard.calibrate_max(false);
            println!(
                "\n\nPartial calibration saved ({}/{} keys).",
                finished.len(),
                key_count
            );
            restore_stdin(&stdin_check);
            return Ok(());
        }

        // Poll each page for calibration progress
        for page in 0..pages as u8 {
            match keyboard.get_calibration_progress(page) {
                Ok(values) => {
                    for (i, &val) in values.iter().enumerate() {
                        let key_idx = page as usize * 32 + i;
                        if key_idx < key_count && val >= 300 && !finished.contains(&key_idx) {
                            finished.insert(key_idx);
                        }
                    }
                }
                Err(_) => continue, // Ignore errors, retry next iteration
            }
        }

        // Build list of missing key names
        let missing: Vec<&str> = (0..key_count)
            .filter(|i| !finished.contains(i))
            .map(|i| {
                M1_V5_HE_KEY_NAMES
                    .get(i)
                    .copied()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("?")
            })
            .collect();

        // Clear line and print progress + missing keys (elided if many)
        print!(
            "\x1b[2K\r        Progress: {}/{} keys calibrated",
            finished.len(),
            key_count
        );
        if !missing.is_empty() {
            let max_show = 10;
            if missing.len() <= max_show {
                print!("  Missing: {}", missing.join(", "));
            } else {
                let shown: Vec<&str> = missing.iter().copied().take(max_show).collect();
                print!(
                    "  Missing: {}, ... (+{})",
                    shown.join(", "),
                    missing.len() - max_show
                );
            }
        }
        let _ = std::io::stdout().flush();

        if finished.len() >= key_count {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let _ = keyboard.calibrate_max(false);
    restore_stdin(&stdin_check);
    println!("\n\nCalibration complete!");
    Ok(())
}

/// Platform-specific stdin setup for non-blocking input
#[cfg(unix)]
struct StdinState {
    old_termios: Option<libc::termios>,
}

#[cfg(unix)]
fn setup_stdin_nonblocking() -> StdinState {
    use std::os::unix::io::AsRawFd;

    let fd = std::io::stdin().as_raw_fd();
    let mut old_termios: libc::termios = unsafe { std::mem::zeroed() };

    // Get current terminal settings
    if unsafe { libc::tcgetattr(fd, &mut old_termios) } != 0 {
        return StdinState { old_termios: None };
    }

    // Set non-canonical mode with no echo
    let mut new_termios = old_termios;
    new_termios.c_lflag &= !(libc::ICANON | libc::ECHO);
    new_termios.c_cc[libc::VMIN] = 0;
    new_termios.c_cc[libc::VTIME] = 0;

    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &new_termios) } != 0 {
        return StdinState { old_termios: None };
    }

    StdinState {
        old_termios: Some(old_termios),
    }
}

#[cfg(unix)]
fn check_stdin_ready(state: &StdinState) -> bool {
    if state.old_termios.is_none() {
        return false;
    }

    use std::os::unix::io::AsRawFd;
    let fd = std::io::stdin().as_raw_fd();

    let mut fds: libc::fd_set = unsafe { std::mem::zeroed() };
    unsafe {
        libc::FD_ZERO(&mut fds);
        libc::FD_SET(fd, &mut fds);
    }

    let mut timeout = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };

    let result = unsafe {
        libc::select(
            fd + 1,
            &mut fds,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut timeout,
        )
    };

    if result > 0 {
        // Read the character to consume it
        let mut buf = [0u8; 1];
        let _ = std::io::Read::read(&mut std::io::stdin(), &mut buf);
        buf[0] == b'\n' || buf[0] == b'\r'
    } else {
        false
    }
}

#[cfg(unix)]
fn restore_stdin(state: &StdinState) {
    if let Some(ref old_termios) = state.old_termios {
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdin().as_raw_fd();
        unsafe {
            libc::tcsetattr(fd, libc::TCSANOW, old_termios);
        }
    }
}

#[cfg(not(unix))]
struct StdinState;

#[cfg(not(unix))]
fn setup_stdin_nonblocking() -> StdinState {
    StdinState
}

#[cfg(not(unix))]
fn check_stdin_ready(_state: &StdinState) -> bool {
    false
}

#[cfg(not(unix))]
fn restore_stdin(_state: &StdinState) {}

/// Show current trigger settings
pub fn triggers() -> CommandResult {
    with_keyboard(|keyboard| {
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
        Ok(())
    })
}

/// Set actuation point for all keys
pub fn set_actuation(mm: f32) -> CommandResult {
    with_keyboard(|keyboard| {
        let precision = keyboard.get_precision().unwrap_or_default();
        let factor = precision.factor() as f32;
        let raw = (mm * factor) as u16;
        match keyboard.set_actuation_all_u16(raw) {
            Ok(_) => println!("Actuation point set to {mm:.2}mm (raw: {raw}) for all keys"),
            Err(e) => eprintln!("Failed to set actuation point: {e}"),
        }
        Ok(())
    })
}

/// Enable/disable Rapid Trigger or set sensitivity
pub fn set_rt(value: &str) -> CommandResult {
    with_keyboard(|keyboard| {
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
        Ok(())
    })
}

/// Set release point for all keys
pub fn set_release(mm: f32) -> CommandResult {
    with_keyboard(|keyboard| {
        let precision = keyboard.get_precision().unwrap_or_default();
        let factor = precision.factor() as f32;
        let raw = (mm * factor) as u16;
        match keyboard.set_release_all_u16(raw) {
            Ok(_) => println!("Release point set to {mm:.2}mm (raw: {raw}) for all keys"),
            Err(e) => eprintln!("Failed to set release point: {e}"),
        }
        Ok(())
    })
}

/// Set bottom deadzone for all keys
pub fn set_bottom_deadzone(mm: f32) -> CommandResult {
    with_keyboard(|keyboard| {
        let precision = keyboard.get_precision().unwrap_or_default();
        let factor = precision.factor() as f32;
        let raw = (mm * factor) as u16;
        match keyboard.set_bottom_deadzone_all_u16(raw) {
            Ok(_) => println!("Bottom deadzone set to {mm:.2}mm (raw: {raw}) for all keys"),
            Err(e) => eprintln!("Failed to set bottom deadzone: {e}"),
        }
        Ok(())
    })
}

/// Set top deadzone for all keys
pub fn set_top_deadzone(mm: f32) -> CommandResult {
    with_keyboard(|keyboard| {
        let precision = keyboard.get_precision().unwrap_or_default();
        let factor = precision.factor() as f32;
        let raw = (mm * factor) as u16;
        match keyboard.set_top_deadzone_all_u16(raw) {
            Ok(_) => println!("Top deadzone set to {mm:.2}mm (raw: {raw}) for all keys"),
            Err(e) => eprintln!("Failed to set top deadzone: {e}"),
        }
        Ok(())
    })
}

/// Set trigger settings for a specific key
pub fn set_key_trigger(
    key: u8,
    actuation: Option<f32>,
    release: Option<f32>,
    mode: Option<String>,
) -> CommandResult {
    with_keyboard(|keyboard| {
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
        Ok(())
    })
}
