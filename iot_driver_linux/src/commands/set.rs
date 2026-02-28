//! Set (write) command handlers.

use super::CommandResult;
use iot_driver::protocol::{cmd, polling_rate};
use monsgeek_keyboard::{KeyboardInterface, PollingRate, SleepTimeSettings};
use std::io::{self, Write};

/// Set active profile
pub fn set_profile(keyboard: &KeyboardInterface, profile: u8) -> CommandResult {
    match keyboard.set_profile(profile) {
        Ok(_) => println!("Profile set to {profile}"),
        Err(e) => eprintln!("Failed to set profile: {e}"),
    }
    Ok(())
}

/// Set debounce time
pub fn set_debounce(keyboard: &KeyboardInterface, ms: u8) -> CommandResult {
    match keyboard.set_debounce(ms) {
        Ok(_) => println!("Debounce set to {ms} ms"),
        Err(e) => eprintln!("Failed to set debounce: {e}"),
    }
    Ok(())
}

/// Set polling rate
pub fn set_rate(keyboard: &KeyboardInterface, rate: &str) -> CommandResult {
    if let Some(hz) = polling_rate::parse(rate) {
        if let Some(rate_enum) = PollingRate::from_hz(hz) {
            match keyboard.set_polling_rate(rate_enum) {
                Ok(_) => println!("Polling rate set to {hz} ({})", polling_rate::name(hz)),
                Err(e) => eprintln!("Failed to set polling rate: {e}"),
            }
        } else {
            eprintln!(
                "Invalid polling rate '{hz}'. Valid rates: 125, 250, 500, 1000, 2000, 4000, 8000"
            );
        }
    } else {
        eprintln!(
            "Invalid polling rate '{rate}'. Valid rates: 125, 250, 500, 1000, 2000, 4000, 8000"
        );
    }
    Ok(())
}

/// Set LED mode and parameters
pub fn set_led(
    keyboard: &KeyboardInterface,
    mode: &str,
    brightness: u8,
    speed: u8,
    r: u8,
    g: u8,
    b: u8,
) -> CommandResult {
    let mode_num = cmd::LedMode::parse(mode)
        .map(|m| m.as_u8())
        .unwrap_or_else(|| mode.parse().unwrap_or(1));

    match keyboard.set_led(mode_num, brightness, speed, r, g, b, false) {
        Ok(_) => println!(
            "LED set: mode={} ({}) brightness={} speed={} color=#{:02X}{:02X}{:02X}",
            mode_num,
            cmd::led_mode_name(mode_num),
            brightness,
            speed,
            r,
            g,
            b
        ),
        Err(e) => eprintln!("Failed to set LED: {e}"),
    }
    Ok(())
}

/// Set sleep time settings
#[allow(clippy::too_many_arguments)]
pub fn set_sleep(
    keyboard: &KeyboardInterface,
    idle: Option<String>,
    deep: Option<String>,
    idle_bt: Option<String>,
    idle_24g: Option<String>,
    deep_bt: Option<String>,
    deep_24g: Option<String>,
    uniform: Option<String>,
) -> CommandResult {
    // Get current settings first
    let current = match keyboard.get_sleep_time() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read current settings: {e}");
            return Ok(());
        }
    };

    // Parse duration helper
    let parse_time = |s: &str| -> Result<u16, String> {
        SleepTimeSettings::parse_duration(s).ok_or_else(|| format!("Invalid duration: {s}"))
    };

    let mut settings = current;

    // Handle --uniform first (idle,deep format)
    if let Some(ref u) = uniform {
        let parts: Vec<&str> = u.split(',').collect();
        if parts.len() != 2 {
            eprintln!("--uniform requires format: idle,deep (e.g., '2m,28m')");
            return Ok(());
        }
        let idle_val = match parse_time(parts[0]) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{e}");
                return Ok(());
            }
        };
        let deep_val = match parse_time(parts[1]) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{e}");
                return Ok(());
            }
        };
        settings = SleepTimeSettings::uniform(idle_val, deep_val);
    }

    // Handle --idle (affects both BT and 2.4GHz)
    if let Some(ref i) = idle {
        match parse_time(i) {
            Ok(v) => {
                settings.idle_bt = v;
                settings.idle_24g = v;
            }
            Err(e) => {
                eprintln!("{e}");
                return Ok(());
            }
        }
    }

    // Handle --deep (affects both BT and 2.4GHz)
    if let Some(ref d) = deep {
        match parse_time(d) {
            Ok(v) => {
                settings.deep_bt = v;
                settings.deep_24g = v;
            }
            Err(e) => {
                eprintln!("{e}");
                return Ok(());
            }
        }
    }

    // Handle individual overrides
    if let Some(ref v) = idle_bt {
        match parse_time(v) {
            Ok(val) => settings.idle_bt = val,
            Err(e) => {
                eprintln!("{e}");
                return Ok(());
            }
        }
    }
    if let Some(ref v) = idle_24g {
        match parse_time(v) {
            Ok(val) => settings.idle_24g = val,
            Err(e) => {
                eprintln!("{e}");
                return Ok(());
            }
        }
    }
    if let Some(ref v) = deep_bt {
        match parse_time(v) {
            Ok(val) => settings.deep_bt = val,
            Err(e) => {
                eprintln!("{e}");
                return Ok(());
            }
        }
    }
    if let Some(ref v) = deep_24g {
        match parse_time(v) {
            Ok(val) => settings.deep_24g = val,
            Err(e) => {
                eprintln!("{e}");
                return Ok(());
            }
        }
    }

    // Check if any changes were made
    if settings == current
        && idle.is_none()
        && deep.is_none()
        && uniform.is_none()
        && idle_bt.is_none()
        && idle_24g.is_none()
        && deep_bt.is_none()
        && deep_24g.is_none()
    {
        eprintln!("No sleep time options specified. Use --help for usage.");
        return Ok(());
    }

    // Apply settings
    match keyboard.set_sleep_time(&settings) {
        Ok(_) => {
            println!("Sleep time settings updated:");
            println!("  Bluetooth:");
            println!(
                "    Idle:       {}",
                SleepTimeSettings::format_duration(settings.idle_bt)
            );
            println!(
                "    Deep Sleep: {}",
                SleepTimeSettings::format_duration(settings.deep_bt)
            );
            println!("  2.4GHz:");
            println!(
                "    Idle:       {}",
                SleepTimeSettings::format_duration(settings.idle_24g)
            );
            println!(
                "    Deep Sleep: {}",
                SleepTimeSettings::format_duration(settings.deep_24g)
            );
        }
        Err(e) => eprintln!("Failed to set sleep settings: {e}"),
    }
    Ok(())
}

/// Factory reset keyboard
pub fn reset(keyboard: &KeyboardInterface) -> CommandResult {
    print!("This will factory reset the keyboard. Are you sure? (y/N) ");
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    if input.trim().to_lowercase() == "y" {
        match keyboard.reset() {
            Ok(_) => println!("Keyboard reset to factory defaults"),
            Err(e) => eprintln!("Failed to reset keyboard: {e}"),
        }
    } else {
        println!("Reset cancelled");
    }
    Ok(())
}

/// Set all keys to a single color
pub fn set_color_all(
    keyboard: &KeyboardInterface,
    r: u8,
    g: u8,
    b: u8,
    layer: u8,
) -> CommandResult {
    println!("Setting all keys to color #{r:02X}{g:02X}{b:02X}...");
    let color = monsgeek_keyboard::led::RgbColor { r, g, b };
    match keyboard.set_all_keys_color(color, layer) {
        Ok(()) => println!("All keys set to #{r:02X}{g:02X}{b:02X}"),
        Err(e) => eprintln!("Failed to set per-key colors: {e}"),
    }
    Ok(())
}
