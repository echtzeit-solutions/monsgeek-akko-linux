//! Trigger-related command handlers.

use super::CommandResult;
use iot_driver::key_action::KeyAction;
use iot_driver::protocol::hid;
use monsgeek_keyboard::{
    DksAction, DksBinding, DksCombo, DksConfig, DksPhase, KeyMode, KeyTriggerSettings,
    KeyboardInterface, ModeByte,
};
use std::collections::{BTreeSet, HashSet};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Stop calibration with retry (keyboard can be sluggish in cal mode).
/// Sends both min and max stop commands to ensure clean exit.
fn stop_calibration(keyboard: &KeyboardInterface) {
    // Stop max calibration (the active phase)
    for attempt in 0..5 {
        match keyboard.calibrate_max(false) {
            Ok(_) => break,
            Err(e) => {
                if attempt < 4 {
                    std::thread::sleep(Duration::from_millis(100));
                } else {
                    eprintln!("Warning: failed to stop max calibration after 5 attempts: {e}");
                }
            }
        }
    }
    // Also stop min calibration (belt-and-suspenders)
    let _ = keyboard.calibrate_min(false);
    // Give firmware time to save calibration data to flash
    std::thread::sleep(Duration::from_millis(300));
}

/// Run calibration (min + max) with per-key progress display
pub fn calibrate(keyboard: &KeyboardInterface) -> CommandResult {
    let key_count = keyboard.key_count() as usize;

    // Determine which matrix indices have real analog (magnetic) keys.
    // Excluded: empty-name positions (gaps), non-analog positions (encoder/GPIO).
    let has_key_names = !keyboard.matrix_key_name(0).is_empty();
    let real_keys: HashSet<usize> = if has_key_names {
        (0..key_count)
            .filter(|&i| {
                let name = keyboard.matrix_key_name(i);
                !name.is_empty() && name != "?" && !keyboard.is_non_analog(i)
            })
            .collect()
    } else {
        // No profile key names available — exclude non-analog but include rest
        (0..key_count)
            .filter(|&i| !keyboard.is_non_analog(i))
            .collect()
    };
    let real_count = real_keys.len();

    // Set up Ctrl+C handler
    let interrupted = Arc::new(AtomicBool::new(false));
    let int_clone = interrupted.clone();
    if let Err(e) = ctrlc::set_handler(move || {
        int_clone.store(true, Ordering::SeqCst);
    }) {
        eprintln!("Warning: Could not set Ctrl+C handler: {e}");
    }

    println!("Starting calibration for {real_count} keys ({key_count} matrix positions)...");
    if !has_key_names {
        println!("  (No device profile found — key names unavailable)");
    }
    println!();
    println!("  To stop: Ctrl+C, any key, mouse click, or encoder knob.");
    println!("  Auto-stops after 10s of no progress.");
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
    let mut finished = BTreeSet::new();
    let pages = key_count.div_ceil(32);

    // Set up input monitoring: stdin + mouse clicks + encoder knob (evdev)
    let input = setup_input_monitor(keyboard.vid(), keyboard.pid());

    // Idle timeout: auto-stop if no new key calibrates in 10 seconds
    let mut last_progress_time = std::time::Instant::now();
    let mut last_finished_count = 0usize;
    let idle_timeout = Duration::from_secs(10);

    loop {
        // Check for Ctrl+C (abort without saving)
        if interrupted.load(Ordering::SeqCst) {
            stop_calibration(keyboard);
            println!("\n\nCalibration aborted (not saved).");
            restore_input(&input);
            return Ok(());
        }

        // Check for Enter key or any stdin input (graceful save)
        if check_input(&input) {
            stop_calibration(keyboard);
            println!(
                "\n\nPartial calibration saved ({}/{} keys).",
                finished.len(),
                real_count,
            );
            restore_input(&input);
            return Ok(());
        }

        // Poll each page for calibration progress
        for page in 0..pages as u8 {
            match keyboard.get_calibration_progress(page) {
                Ok(values) => {
                    for (i, &val) in values.iter().enumerate() {
                        let key_idx = page as usize * 32 + i;
                        if real_keys.contains(&key_idx)
                            && val >= 300
                            && !finished.contains(&key_idx)
                        {
                            finished.insert(key_idx);
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        // Track progress for idle timeout
        if finished.len() > last_finished_count {
            last_finished_count = finished.len();
            last_progress_time = std::time::Instant::now();
        }

        // Build sorted list of missing key names (only real keys)
        let mut missing: Vec<&str> = real_keys
            .iter()
            .filter(|i| !finished.contains(i))
            .map(|&i| keyboard.matrix_key_name(i))
            .filter(|s| !s.is_empty())
            .collect();
        missing.sort_unstable();

        // Clear line and print progress + missing keys (elided if many)
        let idle_secs = last_progress_time.elapsed().as_secs();
        print!(
            "\x1b[2K\r        Progress: {}/{} keys",
            finished.len(),
            real_count,
        );
        if idle_secs >= 3 && !missing.is_empty() {
            print!(" (idle {idle_secs}s/10s)");
        }
        if !missing.is_empty() {
            let max_show = 10;
            if missing.len() <= max_show {
                print!("  Remaining: {}", missing.join(", "));
            } else {
                let shown: Vec<&str> = missing.iter().copied().take(max_show).collect();
                print!(
                    "  Remaining: {}, ... (+{})",
                    shown.join(", "),
                    missing.len() - max_show
                );
            }
        }
        let _ = std::io::stdout().flush();

        // Check completion
        if finished.len() >= real_count {
            break;
        }

        // Idle timeout
        if last_progress_time.elapsed() >= idle_timeout && !finished.is_empty() {
            stop_calibration(keyboard);
            restore_input(&input);
            let uncalibrated: Vec<&str> = missing.clone();
            println!(
                "\n\nAuto-stopped: no progress for 10s. Calibrated {}/{} keys.",
                finished.len(),
                real_count,
            );
            if !uncalibrated.is_empty() {
                println!("  Uncalibrated: {}", uncalibrated.join(", "));
            }
            return Ok(());
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    stop_calibration(keyboard);
    restore_input(&input);
    println!("\n\nCalibration complete! All {real_count} keys calibrated.");
    Ok(())
}

/// Input monitor: watches stdin (keyboard + mouse clicks) and evdev (encoder knob).
#[cfg(unix)]
struct InputMonitor {
    old_termios: Option<libc::termios>,
    evdev_fds: Vec<std::os::unix::io::RawFd>,
}

#[cfg(unix)]
fn setup_input_monitor(vid: u16, pid: u16) -> InputMonitor {
    use std::os::unix::io::AsRawFd;

    let fd = std::io::stdin().as_raw_fd();
    let mut old_termios: libc::termios = unsafe { std::mem::zeroed() };

    let termios_ok = unsafe { libc::tcgetattr(fd, &mut old_termios) } == 0;

    if termios_ok {
        let mut new_termios = old_termios;
        new_termios.c_lflag &= !(libc::ICANON | libc::ECHO);
        new_termios.c_cc[libc::VMIN] = 0;
        new_termios.c_cc[libc::VTIME] = 0;
        unsafe {
            libc::tcsetattr(fd, libc::TCSANOW, &new_termios);
        }
    }

    // Enable terminal mouse click tracking (X10 mode: report button presses)
    // SGR extended mode for better compatibility with modern terminals
    print!("\x1b[?1000h\x1b[?1006h");
    let _ = std::io::stdout().flush();

    // Find evdev devices matching our keyboard's VID/PID (for encoder knob)
    let evdev_fds = find_evdev_devices(vid, pid);
    if !evdev_fds.is_empty() {
        eprintln!(
            "  Monitoring {} input device(s) for encoder/knob events.",
            evdev_fds.len()
        );
    }

    InputMonitor {
        old_termios: if termios_ok { Some(old_termios) } else { None },
        evdev_fds,
    }
}

/// Scan sysfs for /dev/input/eventN devices matching VID:PID, open non-blocking.
#[cfg(unix)]
fn find_evdev_devices(vid: u16, pid: u16) -> Vec<std::os::unix::io::RawFd> {
    use std::os::unix::io::RawFd;

    let vid_hex = format!("{:04x}", vid);
    let pid_hex = format!("{:04x}", pid);
    let mut fds: Vec<RawFd> = Vec::new();

    let Ok(entries) = std::fs::read_dir("/sys/class/input") else {
        return fds;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("event") {
            continue;
        }

        let id_dir = entry.path().join("device/id");
        let vendor = std::fs::read_to_string(id_dir.join("vendor")).unwrap_or_default();
        let product = std::fs::read_to_string(id_dir.join("product")).unwrap_or_default();

        if vendor.trim() == vid_hex && product.trim() == pid_hex {
            let dev_path = format!("/dev/input/{name_str}");
            let c_path = match std::ffi::CString::new(dev_path.as_str()) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
            if fd >= 0 {
                fds.push(fd);
            }
        }
    }

    fds
}

#[cfg(unix)]
fn check_input(monitor: &InputMonitor) -> bool {
    use std::os::unix::io::AsRawFd;

    if monitor.old_termios.is_none() && monitor.evdev_fds.is_empty() {
        return false;
    }

    let stdin_fd = std::io::stdin().as_raw_fd();
    let mut read_fds: libc::fd_set = unsafe { std::mem::zeroed() };
    let mut max_fd = stdin_fd;

    unsafe {
        libc::FD_ZERO(&mut read_fds);
        if monitor.old_termios.is_some() {
            libc::FD_SET(stdin_fd, &mut read_fds);
        }
        for &fd in &monitor.evdev_fds {
            libc::FD_SET(fd, &mut read_fds);
            if fd > max_fd {
                max_fd = fd;
            }
        }
    }

    let mut timeout = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };

    let result = unsafe {
        libc::select(
            max_fd + 1,
            &mut read_fds,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut timeout,
        )
    };

    if result <= 0 {
        return false;
    }

    // Check stdin (keyboard on other KB, or mouse click in terminal)
    if monitor.old_termios.is_some() && unsafe { libc::FD_ISSET(stdin_fd, &read_fds) } {
        let mut buf = [0u8; 64];
        let _ = std::io::Read::read(&mut std::io::stdin(), &mut buf);
        return true;
    }

    // Check evdev devices (encoder knob rotation or press)
    for &fd in &monitor.evdev_fds {
        if unsafe { libc::FD_ISSET(fd, &read_fds) } {
            // Drain all pending events
            let mut buf = [0u8; 256];
            while unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) } > 0 {}
            return true;
        }
    }

    false
}

#[cfg(unix)]
fn restore_input(monitor: &InputMonitor) {
    // Disable mouse tracking
    print!("\x1b[?1006l\x1b[?1000l");
    let _ = std::io::stdout().flush();

    // Restore terminal settings
    if let Some(ref old_termios) = monitor.old_termios {
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdin().as_raw_fd();
        unsafe {
            libc::tcsetattr(fd, libc::TCSANOW, old_termios);
        }
    }

    // Close evdev fds
    for &fd in &monitor.evdev_fds {
        unsafe {
            libc::close(fd);
        }
    }
}

#[cfg(not(unix))]
struct InputMonitor;

#[cfg(not(unix))]
fn setup_input_monitor(_vid: u16, _pid: u16) -> InputMonitor {
    InputMonitor
}

#[cfg(not(unix))]
fn check_input(_monitor: &InputMonitor) -> bool {
    false
}

#[cfg(not(unix))]
fn restore_input(_monitor: &InputMonitor) {}

/// Show current trigger settings
pub fn triggers(keyboard: &KeyboardInterface) -> CommandResult {
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
            let first_press = triggers.press_travel.first().copied().unwrap_or(0);
            let first_lift = triggers.lift_travel.first().copied().unwrap_or(0);
            let first_rt_press = triggers.rt_press.first().copied().unwrap_or(0);
            let first_rt_lift = triggers.rt_lift.first().copied().unwrap_or(0);
            let first_mode = triggers.key_modes.first().copied().unwrap_or(0);

            let num_keys = triggers.key_modes.len().min(triggers.press_travel.len());

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
                ModeByte::from_u8(first_mode)
            );
            println!();

            let all_same_press = triggers
                .press_travel
                .iter()
                .take(num_keys)
                .all(|&v| v == first_press);
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
                    let press = triggers.press_travel.get(i).copied().unwrap_or(0);
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
}

/// Set actuation point for all keys
pub fn set_actuation(keyboard: &KeyboardInterface, mm: f32) -> CommandResult {
    let precision = keyboard.get_precision().unwrap_or_default();
    let factor = precision.factor() as f32;
    let raw = (mm * factor) as u16;
    match keyboard.set_actuation_all_u16(raw) {
        Ok(_) => println!("Actuation point set to {mm:.2}mm (raw: {raw}) for all keys"),
        Err(e) => eprintln!("Failed to set actuation point: {e}"),
    }
    Ok(())
}

/// Enable/disable Rapid Trigger or set sensitivity
pub fn set_rt(keyboard: &KeyboardInterface, value: &str) -> CommandResult {
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
}

/// Set release point for all keys
pub fn set_release(keyboard: &KeyboardInterface, mm: f32) -> CommandResult {
    let precision = keyboard.get_precision().unwrap_or_default();
    let factor = precision.factor() as f32;
    let raw = (mm * factor) as u16;
    match keyboard.set_release_all_u16(raw) {
        Ok(_) => println!("Release point set to {mm:.2}mm (raw: {raw}) for all keys"),
        Err(e) => eprintln!("Failed to set release point: {e}"),
    }
    Ok(())
}

/// Set bottom deadzone for all keys
pub fn set_bottom_deadzone(keyboard: &KeyboardInterface, mm: f32) -> CommandResult {
    let precision = keyboard.get_precision().unwrap_or_default();
    let factor = precision.factor() as f32;
    let raw = (mm * factor) as u16;
    match keyboard.set_bottom_deadzone_all_u16(raw) {
        Ok(_) => println!("Bottom deadzone set to {mm:.2}mm (raw: {raw}) for all keys"),
        Err(e) => eprintln!("Failed to set bottom deadzone: {e}"),
    }
    Ok(())
}

/// Set top deadzone for all keys
pub fn set_top_deadzone(keyboard: &KeyboardInterface, mm: f32) -> CommandResult {
    let precision = keyboard.get_precision().unwrap_or_default();
    let factor = precision.factor() as f32;
    let raw = (mm * factor) as u16;
    match keyboard.set_top_deadzone_all_u16(raw) {
        Ok(_) => println!("Top deadzone set to {mm:.2}mm (raw: {raw}) for all keys"),
        Err(e) => eprintln!("Failed to set top deadzone: {e}"),
    }
    Ok(())
}

/// Set trigger settings for a specific key
pub fn set_key_trigger(
    keyboard: &KeyboardInterface,
    key: u8,
    actuation: Option<f32>,
    release: Option<f32>,
    mode: Option<KeyMode>,
    rt: Option<bool>,
) -> CommandResult {
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

    // Base mode and RT flag are independent; each preserves the current value
    // when not overridden.
    let settings = KeyTriggerSettings {
        key_index: key,
        actuation: actuation
            .map(|mm| (mm * factor) as u8)
            .unwrap_or(current.actuation),
        deactuation: release
            .map(|mm| (mm * factor) as u8)
            .unwrap_or(current.deactuation),
        mode: mode.unwrap_or(current.mode),
        rapid_trigger: rt.unwrap_or(current.rapid_trigger),
    };

    match keyboard.set_key_trigger(&settings) {
        Ok(_) => {
            println!("Key {key} trigger settings updated:");
            println!(
                "  Actuation: {:.1}mm, Release: {:.1}mm, Mode: {}",
                settings.actuation as f32 / factor,
                settings.deactuation as f32 / factor,
                ModeByte::new(settings.mode, settings.rapid_trigger)
            );
            println!(
                "  (precision: {}, bulk commands use higher precision)",
                precision.as_str()
            );
        }
        Err(e) => eprintln!("Failed to set key trigger: {e}"),
    }
    Ok(())
}

/// Set the base mode (and optionally the RT flag) for all keys at once.
pub fn set_mode_all(keyboard: &KeyboardInterface, mode: KeyMode, rt: bool) -> CommandResult {
    let mode_byte = ModeByte::new(mode, rt);
    match keyboard.set_mode_all(mode_byte) {
        Ok(_) => println!("Set all keys to {mode_byte}"),
        Err(e) => eprintln!("Failed to set mode for all keys: {e}"),
    }
    Ok(())
}

/// Bind, clear, or show a key's Snap-Tap (SOCD) pairing.
pub fn set_snaptap(
    keyboard: &KeyboardInterface,
    key: u8,
    with: Option<u8>,
    clear: bool,
) -> CommandResult {
    if clear {
        match keyboard.clear_snaptap(key) {
            Ok(_) => println!("Cleared Snap-Tap binding for key {key}"),
            Err(e) => eprintln!("Failed to clear Snap-Tap binding: {e}"),
        }
    } else if let Some(partner) = with {
        match keyboard.set_snaptap_pair(key, partner) {
            Ok(_) => println!("Bound keys {key} <-> {partner} as a Snap-Tap pair"),
            Err(e) => eprintln!("Failed to set Snap-Tap pair: {e}"),
        }
    } else {
        match keyboard.get_snaptap_binds() {
            Ok(binds) => {
                let partner = binds
                    .get(key as usize)
                    .copied()
                    .unwrap_or(monsgeek_keyboard::SNAPTAP_UNBOUND);
                if partner == monsgeek_keyboard::SNAPTAP_UNBOUND {
                    println!("Key {key}: no Snap-Tap binding");
                } else {
                    println!("Key {key} is bound to key {partner} (Snap-Tap)");
                }
            }
            Err(e) => eprintln!("Failed to read Snap-Tap bindings: {e}"),
        }
    }
    Ok(())
}

/// Set a key's Mod-Tap tap-vs-hold decision time (ms, quantized to 10 ms).
pub fn set_modtap_time(keyboard: &KeyboardInterface, key: u8, ms: u16) -> CommandResult {
    match keyboard.set_modtap_time(key, ms) {
        Ok(_) => println!("Key {key} Mod-Tap decision time set to {}ms", ms / 10 * 10),
        Err(e) => eprintln!("Failed to set Mod-Tap time: {e}"),
    }
    Ok(())
}

fn key_action_hid_code(action: KeyAction) -> Option<u8> {
    match action {
        KeyAction::Key(code) => Some(code),
        KeyAction::Combo { key, .. } => Some(key),
        _ => None,
    }
}

fn parse_dks_combo(spec: &str) -> Result<DksCombo, String> {
    let mut codes = [0u8; 3];
    for (i, part) in spec
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .take(3)
        .enumerate()
    {
        let action: KeyAction = part
            .parse()
            .map_err(|e| format!("slot key '{part}': {e}"))?;
        codes[i] = key_action_hid_code(action)
            .ok_or_else(|| format!("slot key '{part}': need a keyboard key"))?;
    }
    Ok(DksCombo::new(codes[0], codes[1], codes[2]))
}

fn parse_dks_actions(spec: &str) -> Result<[DksAction; 4], String> {
    let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err(format!(
            "expected 4 comma-separated actions, got {}",
            parts.len()
        ));
    }
    let mut out = [DksAction::None; 4];
    for (i, p) in parts.iter().enumerate() {
        out[i] = match *p {
            "0" | "none" => DksAction::None,
            "1" | "single" => DksAction::SingleTrigger,
            "2" | "until_next" => DksAction::ContinuousUntilNext,
            "3" | "across" => DksAction::ContinuousAcross,
            other => return Err(format!("unknown DKS action '{other}'")),
        };
    }
    Ok(out)
}

fn format_dks_combo(combo: DksCombo) -> String {
    [combo.skey, combo.key, combo.key2]
        .iter()
        .filter(|&&c| c != 0)
        .map(|&c| hid::key_name(c).to_string())
        .collect::<Vec<_>>()
        .join("+")
}

/// Show or configure DKS (Dynamic Keystroke) for a key.
pub fn dks(
    keyboard: &KeyboardInterface,
    key: u8,
    travel_mm: Option<f32>,
    modes: Option<String>,
    slots: Option<String>,
    rt: Option<bool>,
) -> CommandResult {
    let setting = travel_mm.is_some() || modes.is_some() || slots.is_some();
    if !setting {
        return show_dks(keyboard, key);
    }

    let mut config = keyboard.get_dks_config(key).unwrap_or(DksConfig {
        trigger_point_travel_raw: 0,
        bindings: [DksBinding::default(); 4],
    });

    if let Some(mm) = travel_mm {
        let precision = keyboard.get_precision().unwrap_or_default();
        config.trigger_point_travel_raw = precision.mm_to_raw(mm as f64);
    }

    if let Some(spec) = modes {
        let bytes: Result<Vec<u8>, String> = spec
            .split(',')
            .map(str::trim)
            .map(|s| {
                u8::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16)
                    .map_err(|_| format!("invalid mode byte '{s}'"))
            })
            .collect();
        let bytes = bytes?;
        if bytes.len() != 4 {
            return Err(format!("--modes requires 4 bytes, got {}", bytes.len()).into());
        }
        for (i, &b) in bytes.iter().enumerate() {
            config.bindings[i] = DksBinding::from_packed_mode(b, config.bindings[i].combo);
        }
    }

    if let Some(spec) = slots {
        let binding_specs: Vec<&str> = spec.split(';').collect();
        if binding_specs.len() != 4 {
            return Err(format!(
                "--slots requires 4 semicolon-separated binding specs, got {}",
                binding_specs.len()
            )
            .into());
        }
        for (i, binding_spec) in binding_specs.iter().enumerate() {
            if binding_spec.is_empty() {
                config.bindings[i].combo = DksCombo::default();
                continue;
            }
            let parts: Vec<&str> = binding_spec.split(':').collect();
            let combo = parse_dks_combo(parts[0])?;
            config.bindings[i].combo = combo;
            if parts.len() > 1 {
                config.bindings[i].phase_actions = parse_dks_actions(parts[1])?;
            }
        }
    }

    match keyboard.set_dks_config(key, &config, rt) {
        Ok(_) => {
            println!("DKS configuration written for key {key}");
            show_dks(keyboard, key)?;
        }
        Err(e) => eprintln!("Failed to set DKS config: {e}"),
    }
    Ok(())
}

fn show_dks(keyboard: &KeyboardInterface, key: u8) -> CommandResult {
    let precision = keyboard.get_precision().unwrap_or_default();
    let factor = precision.factor() as f32;

    match keyboard.get_dks_config(key) {
        Ok(config) => {
            let trigger = keyboard.get_key_trigger(key).ok();
            println!("DKS config for key {key}:");
            if let Some(t) = trigger {
                println!("  Mode: {}", ModeByte::new(t.mode, t.rapid_trigger));
            }
            println!(
                "  Trigger-point travel: {:.2}mm (raw {})",
                config.trigger_point_travel_raw as f32 / factor,
                config.trigger_point_travel_raw
            );
            println!("  Binding rows (packed): {:02X?}", config.trigger_modes());
            for (i, binding) in config.bindings.iter().enumerate() {
                let combo = if binding.combo.is_empty() {
                    "(empty)".to_string()
                } else {
                    format_dks_combo(binding.combo)
                };
                let phases: Vec<String> = DksPhase::ALL
                    .iter()
                    .map(|p| format!("{}={}", p.short_label(), binding.phase_actions[p.index()]))
                    .collect();
                println!(
                    "  Binding {i}: combo={combo}  phases=[{}]",
                    phases.join(", ")
                );
            }
        }
        Err(e) => eprintln!("Failed to read DKS config: {e}"),
    }
    Ok(())
}

/// Diagnostic: prove (or disprove) that setting one key to DKS disturbs other
/// keys' trigger state. Snapshots all keys, does a read-stability check (two
/// reads, no writes), sets `key` to DKS, snapshots again, diffs, then restores
/// the key's original mode/travel and snapshots once more.
pub fn dks_roundtrip(keyboard: &KeyboardInterface, key: u8, op: &str) -> CommandResult {
    let snap = |label: &str| match keyboard.get_all_triggers() {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("[{label}] read failed: {e}");
            None
        }
    };

    let orig_trigger = keyboard.get_key_trigger(key)?;
    println!(
        "Roundtrip op '{op}' on key {key} ({}), current mode {}\n",
        keyboard.matrix_key_name(key as usize),
        ModeByte::new(orig_trigger.mode, orig_trigger.rapid_trigger),
    );

    let Some(a) = snap("A") else { return Ok(()) };
    let Some(b) = snap("B") else { return Ok(()) };
    let read_noise = diff_triggers("read-stability (A vs B, no writes)", &a, &b, keyboard);

    // Isolate which write in the DKS sequence desyncs the read pipeline.
    match op {
        "travel" => keyboard.set_dks_trigger_point_travel_raw(key, 70)?,
        "modes" => keyboard.set_dks_trigger_modes(key, [0, 0, 0, 0])?,
        "combo" => keyboard.set_dks_combo_binding(0, key, 0, DksCombo::default())?,
        "mode-all" => keyboard.set_mode_all(ModeByte::new(KeyMode::Normal, false))?,
        "keytrig" => keyboard.set_key_trigger(&orig_trigger)?,
        _ => {
            let config = DksConfig {
                trigger_point_travel_raw: 0,
                bindings: [DksBinding::default(); 4],
            };
            keyboard.set_dks_config(key, &config, Some(orig_trigger.rapid_trigger))?;
        }
    }

    let Some(c) = snap("C") else { return Ok(()) };
    let after_write = diff_triggers(
        &format!("after set key {key} -> DKS (B vs C)"),
        &b,
        &c,
        keyboard,
    );

    keyboard.set_key_trigger(&orig_trigger)?;
    let Some(d) = snap("D") else { return Ok(()) };
    diff_triggers(
        &format!("after restoring key {key} (C vs D)"),
        &c,
        &d,
        keyboard,
    );
    diff_triggers("net change vs start (A vs D)", &a, &d, keyboard);

    println!("\nVerdict:");
    if read_noise > 0 {
        println!(
            "  ⚠ {read_noise} keys differ between two back-to-back reads with no writes \
             → the readback itself is unstable (response desync), not real corruption."
        );
    }
    let cross = after_write.saturating_sub(1); // the target key is expected to change
    if cross > 0 {
        println!(
            "  ✗ setting key {key} to DKS changed {cross} OTHER key(s) — real cross-key effect."
        );
    } else if read_noise == 0 {
        println!("  ✓ only key {key} changed; no other keys were disturbed.");
    }
    Ok(())
}

/// Compare two full trigger snapshots; print every key whose per-key state
/// changed and return the count of changed keys.
fn diff_triggers(
    label: &str,
    before: &monsgeek_keyboard::TriggerSettings,
    after: &monsgeek_keyboard::TriggerSettings,
    keyboard: &KeyboardInterface,
) -> usize {
    let n = before
        .key_modes
        .len()
        .min(after.key_modes.len())
        .max(before.press_travel.len().min(after.press_travel.len()));
    let mut changed = 0usize;
    println!("=== {label} ===");
    for i in 0..n {
        let mut fields = Vec::new();
        let bm = before.key_modes.get(i).copied();
        let am = after.key_modes.get(i).copied();
        if bm != am {
            let (bb, ab) = (bm.unwrap_or(0), am.unwrap_or(0));
            fields.push(format!(
                "mode 0x{bb:02X} ({}) -> 0x{ab:02X} ({})",
                ModeByte::from_u8(bb),
                ModeByte::from_u8(ab),
            ));
        }
        for (name, bv, av) in [
            ("press", &before.press_travel, &after.press_travel),
            ("lift", &before.lift_travel, &after.lift_travel),
            ("rt_press", &before.rt_press, &after.rt_press),
            ("rt_lift", &before.rt_lift, &after.rt_lift),
            ("bot_dz", &before.bottom_deadzone, &after.bottom_deadzone),
            ("top_dz", &before.top_deadzone, &after.top_deadzone),
        ] {
            let x = bv.get(i).copied();
            let y = av.get(i).copied();
            if x != y {
                fields.push(format!("{name} {} -> {}", x.unwrap_or(0), y.unwrap_or(0)));
            }
        }
        if !fields.is_empty() {
            changed += 1;
            println!(
                "  key {i:>3} ({:<8}): {}",
                keyboard.matrix_key_name(i),
                fields.join(", ")
            );
        }
    }
    if changed == 0 {
        println!("  (no changes)");
    } else {
        println!("  {changed} key(s) changed");
    }
    changed
}
