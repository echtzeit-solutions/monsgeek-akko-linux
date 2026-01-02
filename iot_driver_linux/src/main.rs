use clap::Parser;
use hidapi::HidApi;
use tonic::transport::Server;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

// Use shared protocol definitions from library
use iot_driver::protocol::{self, cmd};
use iot_driver::color::hsv_to_rgb;

// CLI definitions
mod cli;
use cli::{Cli, Commands, FirmwareCommands};

// gRPC server module
mod grpc;
use grpc::{DriverService, DriverGrpcServer, get_known_devices, dj_dev};


/// CLI test function - send a command and print response
/// Uses retry pattern due to Linux HID feature report buffering
fn cli_test(hidapi: &HidApi, cmd: u8) -> Result<(), Box<dyn std::error::Error>> {
    let known_devices = get_known_devices();

    for device_info in hidapi.device_list() {
        let vid = device_info.vendor_id();
        let pid = device_info.product_id();
        let usage_page = device_info.usage_page();
        let usage = device_info.usage();
        let interface = device_info.interface_number();

        for known in &known_devices {
            if vid == known.vid && pid == known.pid
                && usage_page == known.usage_page
                && usage == known.usage
                && interface == known.interface_number {

                println!("Found device: VID={:04x} PID={:04x} path={}",
                    vid, pid, device_info.path().to_string_lossy());

                let device = device_info.open_device(hidapi)?;

                // Prepare command with checksum (Bit7 mode)
                let mut buf = vec![0u8; 65];
                buf[0] = 0; // Report ID
                buf[1] = cmd; // Command byte
                // Checksum at byte 7 of payload (buf[8])
                let sum: u32 = buf[1..8].iter().map(|&b| b as u32).sum();
                buf[8] = (255 - (sum & 0xFF)) as u8;

                println!("Sending command 0x{:02x} ({})...", cmd, cmd::name(cmd));
                println!("  TX: {:02x?}", &buf[1..12]);

                // Linux HID feature reports have buffering - need retry pattern
                // Send command, wait, then retry reading until we get our response
                const MAX_RETRIES: usize = 5;
                let mut resp = vec![0u8; 65];
                let mut success = false;

                for attempt in 0..MAX_RETRIES {
                    // Send the command
                    device.send_feature_report(&buf)?;
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    // Read response
                    resp[0] = 0;
                    let _len = device.get_feature_report(&mut resp)?;

                    let cmd_echo = resp[1];
                    println!("  Attempt {}: echo=0x{:02x} data={:02x?}",
                        attempt + 1, cmd_echo, &resp[1..12]);

                    if cmd_echo == cmd {
                        success = true;
                        break;
                    }
                }

                if !success {
                    println!("\nFailed to get response after {MAX_RETRIES} attempts");
                    return Err("No valid response".into());
                }

                println!("\nResponse (0x{:02x} = {}):", resp[1], cmd::name(resp[1]));

                // Parse based on command type
                match cmd {
                    cmd::GET_USB_VERSION => {
                        // Device ID at bytes 2-5 (little-endian uint32)
                        let device_id = (resp[2] as u32)
                            | ((resp[3] as u32) << 8)
                            | ((resp[4] as u32) << 16)
                            | ((resp[5] as u32) << 24);
                        // Version at bytes 8-9 (little-endian uint16)
                        let version = (resp[8] as u16) | ((resp[9] as u16) << 8);
                        println!("  Device ID:  {device_id} (0x{device_id:04X})");
                        println!("  Version:    {} (v{}.{:02})", version, version / 100, version % 100);
                    }
                    cmd::GET_PROFILE => {
                        println!("  Profile:    {}", resp[2]);
                    }
                    cmd::GET_DEBOUNCE => {
                        println!("  Debounce:   {} ms", resp[2]);
                    }
                    cmd::GET_LEDPARAM => {
                        let mode = resp[2];
                        let brightness = resp[3];
                        let speed = protocol::LED_SPEED_MAX - resp[4].min(protocol::LED_SPEED_MAX);
                        let options = resp[5];
                        let r = resp[6];
                        let g = resp[7];
                        let b = resp[8];
                        let dazzle = (options & protocol::LED_OPTIONS_MASK) == protocol::LED_DAZZLE_ON;
                        println!("  LED Mode:   {} ({})", mode, cmd::led_mode_name(mode));
                        println!("  Brightness: {brightness}/4");
                        println!("  Speed:      {speed}/4");
                        println!("  Color RGB:  ({r}, {g}, {b}) #{r:02X}{g:02X}{b:02X}");
                        if dazzle {
                            println!("  Dazzle:     ON (rainbow cycle)");
                        }
                    }
                    cmd::GET_KBOPTION => {
                        println!("  Fn Layer:   {}", resp[3]);
                        println!("  Anti-ghost: {}", resp[4]);
                        println!("  RTStab:     {} ms", resp[5] as u32 * 25);
                        println!("  WASD Swap:  {}", resp[6]);
                    }
                    cmd::GET_FEATURE_LIST => {
                        println!("  Features:   {:02x?}", &resp[2..12]);
                        let precision = match resp[3] {
                            0 => "0.1mm",
                            1 => "0.05mm",
                            2 => "0.01mm",
                            _ => "unknown",
                        };
                        println!("  Precision:  {precision}");
                    }
                    cmd::GET_SLEEPTIME => {
                        let sleep_s = (resp[2] as u16) | ((resp[3] as u16) << 8);
                        println!("  Sleep:      {} seconds ({} min)", sleep_s, sleep_s / 60);
                    }
                    _ => {
                        println!("  Raw data:   {:02x?}", &resp[1..17]);
                    }
                }

                return Ok(());
            }
        }
    }

    Err("No compatible device found".into())
}

/// List all HID devices
fn cli_list(hidapi: &HidApi) {
    println!("All HID devices:");
    for device_info in hidapi.device_list() {
        println!("  VID={:04x} PID={:04x} usage={:04x} page={:04x} if={} path={}",
            device_info.vendor_id(),
            device_info.product_id(),
            device_info.usage(),
            device_info.usage_page(),
            device_info.interface_number(),
            device_info.path().to_string_lossy()
        );
    }
}


/// Run multiple commands and show all info
fn cli_all(hidapi: &HidApi) -> Result<(), Box<dyn std::error::Error>> {
    println!("MonsGeek M1 V5 HE - Device Information");
    println!("======================================\n");

    // Get all info
    let commands = [
        (cmd::GET_USB_VERSION, "Device Info"),
        (cmd::GET_PROFILE, "Profile"),
        (cmd::GET_DEBOUNCE, "Debounce"),
        (cmd::GET_LEDPARAM, "LED"),
        (cmd::GET_KBOPTION, "Options"),
        (cmd::GET_FEATURE_LIST, "Features"),
    ];

    for (cmd_byte, _name) in commands {
        let _ = cli_test(hidapi, cmd_byte);
        println!();
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Handle commands
    match cli.command {
        // No command = show help (clap handles this automatically)
        None => {
            // Show help
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
            return Ok(());
        }

        // === Query Commands ===
        Some(Commands::Info) => {
            let hidapi = HidApi::new()?;
            cli_test(&hidapi, cmd::GET_USB_VERSION)?;
        }
        Some(Commands::Profile) => {
            let hidapi = HidApi::new()?;
            cli_test(&hidapi, cmd::GET_PROFILE)?;
        }
        Some(Commands::Led) => {
            let hidapi = HidApi::new()?;
            cli_test(&hidapi, cmd::GET_LEDPARAM)?;
        }
        Some(Commands::Debounce) => {
            let hidapi = HidApi::new()?;
            cli_test(&hidapi, cmd::GET_DEBOUNCE)?;
        }
        Some(Commands::Options) => {
            let hidapi = HidApi::new()?;
            cli_test(&hidapi, cmd::GET_KBOPTION)?;
        }
        Some(Commands::Features) => {
            let hidapi = HidApi::new()?;
            cli_test(&hidapi, cmd::GET_FEATURE_LIST)?;
        }
        Some(Commands::Sleep) => {
            let hidapi = HidApi::new()?;
            cli_test(&hidapi, cmd::GET_SLEEPTIME)?;
        }
        Some(Commands::All) => {
            let hidapi = HidApi::new()?;
            cli_all(&hidapi)?;
        }

        // === Set Commands ===
        Some(Commands::SetProfile { profile }) => {
            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                if device.set_profile(profile) {
                    println!("Profile set to {profile}");
                } else {
                    eprintln!("Failed to set profile");
                }
            } else {
                eprintln!("No device found");
            }
        }
        Some(Commands::SetDebounce { ms }) => {
            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                if device.set_debounce(ms) {
                    println!("Debounce set to {ms} ms");
                } else {
                    eprintln!("Failed to set debounce");
                }
            } else {
                eprintln!("No device found");
            }
        }
        Some(Commands::SetLed { mode, brightness, speed, r, g, b }) => {
            let mode_num = cmd::LedMode::parse(&mode)
                .map(|m| m.as_u8())
                .unwrap_or_else(|| mode.parse().unwrap_or(1));
            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                if device.set_led(mode_num, brightness, speed, r, g, b, false) {
                    println!("LED set: mode={} ({}) brightness={} speed={} color=#{:02X}{:02X}{:02X}",
                        mode_num, cmd::led_mode_name(mode_num), brightness, speed, r, g, b);
                } else {
                    eprintln!("Failed to set LED");
                }
            } else {
                eprintln!("No device found");
            }
        }
        Some(Commands::SetSleep { seconds }) => {
            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                if device.set_sleep(seconds, seconds) {
                    println!("Sleep timeout set to {} seconds ({} min)", seconds, seconds / 60);
                } else {
                    eprintln!("Failed to set sleep timeout");
                }
            } else {
                eprintln!("No device found");
            }
        }
        Some(Commands::Reset) => {
            print!("This will factory reset the keyboard. Are you sure? (y/N) ");
            use std::io::{self, Write};
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            if input.trim().to_lowercase() == "y" {
                if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                    if device.reset() {
                        println!("Keyboard reset to factory defaults");
                    } else {
                        eprintln!("Failed to reset keyboard");
                    }
                } else {
                    eprintln!("No device found");
                }
            } else {
                println!("Reset cancelled");
            }
        }
        Some(Commands::Calibrate) => {
            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                println!("Starting calibration...");
                println!("Step 1: Calibrating minimum (released) position...");
                println!("        Keep all keys released!");
                device.calibrate_min(true);
                std::thread::sleep(std::time::Duration::from_secs(2));
                device.calibrate_min(false);
                println!("        Done.");
                println!();
                println!("Step 2: Calibrating maximum (pressed) position...");
                println!("        Press and hold ALL keys firmly for 3 seconds!");
                device.calibrate_max(true);
                std::thread::sleep(std::time::Duration::from_secs(3));
                device.calibrate_max(false);
                println!("        Done.");
                println!();
                println!("Calibration complete!");
            } else {
                eprintln!("No device found");
            }
        }

        // === Trigger Commands ===
        Some(Commands::Triggers) => {
            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                let info = device.read_info();
                let factor = iot_driver::hid::MonsGeekDevice::precision_factor_from_version(info.version);
                println!("Trigger Settings (firmware v{}, precision: {})",
                    info.version,
                    iot_driver::hid::MonsGeekDevice::precision_str_from_version(info.version));
                println!();

                if let Some(triggers) = device.get_all_triggers() {
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

                    let num_keys = triggers.key_modes.len().min(triggers.press_travel.len() / 2);

                    println!("First key settings (as sample):");
                    println!("  Actuation:     {:.1}mm (raw: {})", first_press as f32 / factor, first_press);
                    println!("  Release:       {:.1}mm (raw: {})", first_lift as f32 / factor, first_lift);
                    println!("  RT Press:      {:.2}mm (raw: {})", first_rt_press as f32 / factor, first_rt_press);
                    println!("  RT Release:    {:.2}mm (raw: {})", first_rt_lift as f32 / factor, first_rt_lift);
                    println!("  Mode:          {} ({})", first_mode,
                        protocol::magnetism::mode_name(first_mode));
                    println!();

                    let all_same_press = (0..num_keys).all(|i| decode_u16(&triggers.press_travel, i) == first_press);
                    let all_same_mode = triggers.key_modes.iter().take(num_keys).all(|&v| v == first_mode);

                    if all_same_press && all_same_mode {
                        println!("All {num_keys} keys have identical settings");
                    } else {
                        println!("Keys have varying settings ({num_keys} keys total)");
                        println!("\nFirst 10 key values:");
                        for i in 0..10.min(num_keys) {
                            let press = decode_u16(&triggers.press_travel, i);
                            let mode = triggers.key_modes.get(i).copied().unwrap_or(0);
                            println!("  Key {:2}: {:.1}mm mode={}", i, press as f32 / factor, mode);
                        }
                    }
                } else {
                    eprintln!("Failed to read trigger settings");
                }
            } else {
                eprintln!("No device found");
            }
        }
        Some(Commands::SetActuation { mm }) => {
            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                let info = device.read_info();
                let factor = iot_driver::hid::MonsGeekDevice::precision_factor_from_version(info.version);
                let raw = (mm * factor) as u16;
                if device.set_actuation_all_u16(raw) {
                    println!("Actuation point set to {mm:.2}mm (raw: {raw}) for all keys");
                } else {
                    eprintln!("Failed to set actuation point");
                }
            } else {
                eprintln!("No device found");
            }
        }
        Some(Commands::SetRt { value }) => {
            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                let info = device.read_info();
                let factor = iot_driver::hid::MonsGeekDevice::precision_factor_from_version(info.version);

                match value.to_lowercase().as_str() {
                    "off" | "0" | "disable" => {
                        if device.set_rapid_trigger_all(false) {
                            println!("Rapid Trigger disabled for all keys");
                        } else {
                            eprintln!("Failed to disable Rapid Trigger");
                        }
                    }
                    "on" | "enable" => {
                        let sensitivity = (0.3 * factor) as u16;
                        device.set_rapid_trigger_all(true);
                        device.set_rt_press_all_u16(sensitivity);
                        device.set_rt_lift_all_u16(sensitivity);
                        println!("Rapid Trigger enabled with 0.3mm sensitivity for all keys");
                    }
                    _ => {
                        let mm: f32 = value.parse().unwrap_or(0.3);
                        let sensitivity = (mm * factor) as u16;
                        device.set_rapid_trigger_all(true);
                        device.set_rt_press_all_u16(sensitivity);
                        device.set_rt_lift_all_u16(sensitivity);
                        println!("Rapid Trigger enabled with {mm:.2}mm sensitivity for all keys");
                    }
                }
            } else {
                eprintln!("No device found");
            }
        }

        // === Per-key Color Commands ===
        Some(Commands::SetColorAll { r, g, b, layer: _ }) => {
            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                println!("Setting all keys to color #{r:02X}{g:02X}{b:02X}...");
                if device.set_all_keys_color(r, g, b) {
                    println!("All keys set to #{r:02X}{g:02X}{b:02X}");
                } else {
                    eprintln!("Failed to set per-key colors");
                }
            } else {
                eprintln!("No device found");
            }
        }

        // === Key Remapping ===
        Some(Commands::Remap { from, to, layer }) => {
            let key_index: u8 = from.parse().unwrap_or(0);
            let hid_code = u8::from_str_radix(&to, 16).unwrap_or(0);

            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                let key_name = iot_driver::protocol::hid::key_name(hid_code);
                println!("Remapping key {key_index} to {key_name} (0x{hid_code:02x}) on layer {layer}...");
                if device.set_keymatrix(layer, key_index, hid_code, true, 0) {
                    println!("Key {key_index} remapped to {key_name}");
                } else {
                    eprintln!("Failed to remap key");
                }
            } else {
                eprintln!("No device found");
            }
        }
        Some(Commands::ResetKey { key, layer }) => {
            let key_index: u8 = key.parse().unwrap_or(0);

            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                println!("Resetting key {key_index} on layer {layer}...");
                if device.reset_key(layer, key_index) {
                    println!("Key {key_index} reset to default");
                } else {
                    eprintln!("Failed to reset key");
                }
            } else {
                eprintln!("No device found");
            }
        }
        Some(Commands::Swap { key1, key2, layer }) => {
            let key_a: u8 = key1.parse().unwrap_or(0);
            let key_b: u8 = key2.parse().unwrap_or(0);

            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                if let Some(data) = device.get_keymatrix(layer, 2) {
                    let code_a = if (key_a as usize) * 4 + 2 < data.len() {
                        data[(key_a as usize) * 4 + 2]
                    } else { 0 };
                    let code_b = if (key_b as usize) * 4 + 2 < data.len() {
                        data[(key_b as usize) * 4 + 2]
                    } else { 0 };

                    let name_a = iot_driver::protocol::hid::key_name(code_a);
                    let name_b = iot_driver::protocol::hid::key_name(code_b);
                    println!("Swapping key {key_a} ({name_a}) <-> key {key_b} ({name_b})...");

                    if device.swap_keys(layer, key_a, code_a, key_b, code_b) {
                        println!("Keys swapped successfully");
                    } else {
                        eprintln!("Failed to swap keys");
                    }
                } else {
                    eprintln!("Failed to read current key mappings");
                }
            } else {
                eprintln!("No device found");
            }
        }
        Some(Commands::Keymatrix { layer }) => {
            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                println!("Reading key matrix for layer {layer}...");

                if let Some(data) = device.get_keymatrix(layer, 3) {
                    println!("\nKey matrix data ({} bytes):", data.len());
                    for (i, chunk) in data.chunks(16).enumerate() {
                        print!("{:04x}: ", i * 16);
                        for b in chunk {
                            print!("{b:02x} ");
                        }
                        print!("  |");
                        for b in chunk {
                            if *b >= 0x20 && *b < 0x7f {
                                print!("{}", *b as char);
                            } else {
                                print!(".");
                            }
                        }
                        println!("|");
                    }

                    println!("\nKey mappings (format: [type, flags, code, layer]):");
                    let key_count = device.key_count() as usize;
                    for i in 0..key_count.min(20) {
                        if i * 4 + 3 < data.len() {
                            let k = &data[i * 4..(i + 1) * 4];
                            let hid_code = k[2];
                            let key_name = iot_driver::protocol::hid::key_name(hid_code);
                            println!("  Key {:2}: {:02x} {:02x} {:02x} {:02x}  -> {} (0x{:02x})",
                                i, k[0], k[1], k[2], k[3], key_name, hid_code);
                        }
                    }
                } else {
                    eprintln!("Failed to read key matrix");
                }
            } else {
                eprintln!("No device found");
            }
        }

        // === Macro Commands ===
        Some(Commands::Macro { key }) => {
            let macro_index: u8 = key.parse().unwrap_or(0);
            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                println!("Reading macro {macro_index}...");
                if let Some(data) = device.get_macro(macro_index) {
                    if data.len() >= 2 {
                        let length = u16::from_le_bytes([data[0], data[1]]) as usize;
                        println!("Macro length: {length} bytes");

                        if length > 0 && data.len() > 2 {
                            println!("\nMacro events (2 bytes each: [keycode, flags]):");
                            let events = &data[2..];
                            for (i, chunk) in events.chunks(2).enumerate() {
                                if chunk.len() < 2 || chunk.iter().all(|&b| b == 0) {
                                    break;
                                }
                                let keycode = chunk[0];
                                let flags = chunk[1];

                                let event_type = if flags & 0x80 != 0 { "Down" } else { "Up" };
                                let key_name = iot_driver::protocol::hid::key_name(keycode);
                                println!("  Event {i:2}: {event_type} {key_name} (0x{keycode:02x}, flags={flags:02x})");
                            }
                        } else {
                            println!("Macro is empty");
                        }
                    } else {
                        println!("Invalid macro data");
                    }

                    println!("\nRaw data ({} bytes):", data.len().min(64));
                    for chunk in data.chunks(16).take(4) {
                        for b in chunk {
                            print!("{b:02x} ");
                        }
                        println!();
                    }
                } else {
                    eprintln!("Failed to read macro (may be empty)");
                }
            } else {
                eprintln!("No device found");
            }
        }
        Some(Commands::SetMacro { key, text }) => {
            let macro_index: u8 = key.parse().unwrap_or(0);

            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                println!("Setting macro {macro_index} to type: \"{text}\"");

                if device.set_text_macro(macro_index, &text, 10, 1) {
                    println!("Macro {macro_index} set successfully!");
                    println!("Assign this macro to a key in the Akko driver to test.");
                } else {
                    eprintln!("Failed to set macro");
                }
            } else {
                eprintln!("No device found");
            }
        }
        Some(Commands::ClearMacro { key }) => {
            let macro_index: u8 = key.parse().unwrap_or(0);

            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                println!("Clearing macro {macro_index}...");

                if device.set_macro(macro_index, &[], 1) {
                    println!("Macro {macro_index} cleared!");
                } else {
                    eprintln!("Failed to clear macro");
                }
            } else {
                eprintln!("No device found");
            }
        }

        // === Animation Commands ===
        Some(Commands::Gif { file, mode, test, frames, delay }) => {
            use iot_driver::gif::{load_gif, generate_test_animation, print_animation_info};

            let device = iot_driver::hid::MonsGeekDevice::open()
                .map_err(|e| format!("Failed to open device: {e}"))?;

            let animation = if test {
                println!("Generating {frames} frame test animation...");
                generate_test_animation(frames, delay)
            } else if let Some(path) = file {
                println!("Loading GIF: {path}");
                match load_gif(&path, mode.into()) {
                    Ok(anim) => anim,
                    Err(e) => {
                        eprintln!("Failed to load GIF: {e}");
                        return Ok(());
                    }
                }
            } else {
                eprintln!("Either provide a file path or use --test");
                return Ok(());
            };

            print_animation_info(&animation);

            let anim_frames: Vec<Vec<(u8, u8, u8)>> = animation.frames
                .iter()
                .take(255)
                .map(|f| f.colors.clone())
                .collect();

            let delay_ms = animation.frames.first()
                .map(|f| f.delay_ms)
                .unwrap_or(100);

            println!("\nUploading {} frames ({}ms delay)...", anim_frames.len(), delay_ms);
            if device.upload_animation(&anim_frames, delay_ms) {
                println!("Animation uploaded! Keyboard will play it autonomously.");
            } else {
                eprintln!("Failed to upload animation");
            }
        }
        Some(Commands::GifStream { file, mode, r#loop }) => {
            use iot_driver::gif::{load_gif, print_animation_info};
            use std::sync::atomic::{AtomicBool, Ordering};
            use std::sync::Arc;

            println!("Loading GIF: {file}");
            let animation = load_gif(&file, mode.into())
                .map_err(|e| format!("Failed to load GIF: {e}"))?;

            print_animation_info(&animation);

            let device = iot_driver::hid::MonsGeekDevice::open()
                .map_err(|e| format!("Failed to open device: {e}"))?;

            device.set_led_with_option(13, 4, 0, 0, 0, 0, false, 0);

            let running = Arc::new(AtomicBool::new(true));
            let r = running.clone();

            ctrlc::set_handler(move || {
                r.store(false, Ordering::SeqCst);
            }).ok();

            println!("\nStreaming animation (Ctrl+C to stop)...");

            loop {
                for (idx, frame) in animation.frames.iter().enumerate() {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }

                    device.set_per_key_colors_fast(&frame.colors, 10, 3);
                    print!("\rFrame {:3}/{}", idx + 1, animation.frame_count);
                    std::io::Write::flush(&mut std::io::stdout()).ok();

                    std::thread::sleep(std::time::Duration::from_millis(frame.delay_ms as u64));
                }

                if !r#loop || !running.load(Ordering::SeqCst) {
                    break;
                }
            }

            println!("\nAnimation stopped.");
        }
        Some(Commands::Mode { mode, layer }) => {
            use iot_driver::protocol::cmd::LedMode;

            let led_mode = match LedMode::parse(&mode) {
                Some(m) => m,
                None => {
                    eprintln!("Unknown mode: {mode}");
                    eprintln!("\nAvailable modes:");
                    for (id, name) in LedMode::list_all() {
                        eprintln!("  {id:2} - {name}");
                    }
                    return Ok(());
                }
            };

            let device = iot_driver::hid::MonsGeekDevice::open()
                .map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Setting LED mode to {} ({}) with layer {}...", led_mode.name(), led_mode.as_u8(), layer);
            device.set_led_with_option(led_mode.as_u8(), 4, 0, 128, 128, 128, false, layer);
            println!("Done.");
        }
        Some(Commands::Modes) => {
            use iot_driver::protocol::cmd::LedMode;
            println!("Available LED modes:");
            for (id, name) in LedMode::list_all() {
                println!("  {id:2} - {name}");
            }
        }

        // === Demo Commands ===
        Some(Commands::Rainbow) => {
            use std::sync::atomic::AtomicBool;
            use std::sync::Arc;

            let device = iot_driver::hid::MonsGeekDevice::open()
                .map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Starting rainbow test on {}...", device.display_name());
            println!("Press Ctrl+C to stop");

            let running = Arc::new(AtomicBool::new(true));
            let running_clone = Arc::clone(&running);

            ctrlc::set_handler(move || {
                running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            }).ok();

            if let Err(e) = iot_driver::audio_reactive::run_rainbow_test(&device, running) {
                eprintln!("Rainbow test error: {e}");
            }
        }
        Some(Commands::Checkerboard) => {
            let device = iot_driver::hid::MonsGeekDevice::open()
                .map_err(|e| format!("Failed to open device: {e}"))?;

            println!("=== Per-Key Color Test ===\n");

            println!("1. Current LED settings:");
            if let Some(resp) = device.query(0x87) {
                println!("   Mode: {}, Speed: {}, Brightness: {}, Option: {}, RGB: ({},{},{})",
                         resp[2], resp[3], resp[4], resp[5], resp[6], resp[7], resp[8]);
            }

            println!("\n2. Setting LED mode to 13 (LightUserPicture)...");
            if !device.set_led(13, 4, 0, 0, 0, 0, false) {
                println!("   ERROR: Failed to set LED mode!");
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(300));

            if let Some(resp) = device.query(0x87) {
                println!("   Mode: {}, Speed: {}, Brightness: {}, Option: {}, RGB: ({},{},{})",
                         resp[2], resp[3], resp[4], resp[5], resp[6], resp[7], resp[8]);
            }

            const MATRIX_SIZE: usize = 126;
            println!("\n3. Writing RED to ALL layers (0-3)...");
            let red_colors: Vec<(u8, u8, u8)> = vec![(255, 0, 0); MATRIX_SIZE];

            for layer in 0..4 {
                println!("   Writing to layer {layer}...");
                device.set_per_key_colors_to_layer(&red_colors, layer);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));

            if let Some(colors) = device.get_per_key_colors_debug() {
                print!("   Stored colors (first 10): ");
                for (r, g, b) in colors.iter().take(10) {
                    print!("({r},{g},{b}) ");
                }
                println!();
            }

            for layer in 0..4 {
                println!("   Setting layer {} (option byte = {:#04X})...", layer, layer << 4);
                device.set_led_with_option(13, 4, 0, 0, 0, 0, false, layer);
                std::thread::sleep(std::time::Duration::from_millis(500));
            }

            let mut input = String::new();
            println!("   Did any layer show RED? [Enter to continue]");
            std::io::stdin().read_line(&mut input).ok();

            println!("\n4. Setting ALL keys to BLUE...");
            let blue_colors: Vec<(u8, u8, u8)> = vec![(0, 0, 255); MATRIX_SIZE];
            device.set_per_key_colors_fast(&blue_colors, 100, 20);
            std::thread::sleep(std::time::Duration::from_millis(300));
            println!("   Did the keyboard turn BLUE? [Enter to continue]");
            std::io::stdin().read_line(&mut input).ok();

            println!("\n5. Setting ALL keys to GREEN...");
            let green_colors: Vec<(u8, u8, u8)> = vec![(0, 255, 0); MATRIX_SIZE];
            device.set_per_key_colors_fast(&green_colors, 100, 20);
            std::thread::sleep(std::time::Duration::from_millis(300));
            println!("   Did the keyboard turn GREEN? [Enter to continue]");
            std::io::stdin().read_line(&mut input).ok();

            println!("\n6. Setting checkerboard pattern (alternating RED/BLUE)...");
            let mut checker_colors = Vec::with_capacity(MATRIX_SIZE);
            for i in 0..MATRIX_SIZE {
                if i % 2 == 0 {
                    checker_colors.push((255, 0, 0));
                } else {
                    checker_colors.push((0, 0, 255));
                }
            }
            device.set_per_key_colors_fast(&checker_colors, 100, 20);
            println!("   Did the keyboard show alternating RED/BLUE? [Enter to finish]");
            std::io::stdin().read_line(&mut input).ok();

            println!("\nTest complete!");
        }
        Some(Commands::Sweep) => {
            use std::sync::atomic::AtomicBool;
            use std::sync::Arc;

            let device = iot_driver::hid::MonsGeekDevice::open()
                .map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Starting sweep animation on {}...", device.display_name());
            println!("Press Ctrl+C to stop");

            device.set_led(25, 4, 0, 0, 0, 0, false);
            std::thread::sleep(std::time::Duration::from_millis(100));

            let running = Arc::new(AtomicBool::new(true));
            let running_clone = Arc::clone(&running);

            ctrlc::set_handler(move || {
                running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            }).ok();

            let key_count = device.key_count() as usize;
            let mut position = 0usize;

            while running.load(std::sync::atomic::Ordering::SeqCst) {
                let mut colors = Vec::with_capacity(key_count);
                for i in 0..key_count {
                    let distance = ((i as i32) - (position as i32)).unsigned_abs() as usize;
                    if distance < 3 {
                        let brightness = 255 - (distance * 80) as u8;
                        colors.push((brightness, brightness, brightness));
                    } else {
                        colors.push((0, 0, 30));
                    }
                }

                device.set_per_key_colors_fast(&colors, 10, 2);
                position = (position + 1) % key_count;

                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            println!("Sweep animation stopped");
        }
        Some(Commands::Red) => {
            let device = iot_driver::hid::MonsGeekDevice::open()
                .map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Simple RED test:");
            println!("1. Setting mode 13 (LightUserPicture) with layer 0...");
            device.set_led_with_option(13, 4, 0, 0, 0, 0, false, 0);
            std::thread::sleep(std::time::Duration::from_millis(500));

            println!("2. Writing RED to layer 0 (126 keys)...");
            let red: Vec<(u8, u8, u8)> = vec![(255, 0, 0); 126];
            device.set_per_key_colors_to_layer(&red, 0);
            std::thread::sleep(std::time::Duration::from_millis(500));

            println!("3. Re-setting mode 13 with layer 0 to refresh...");
            device.set_led_with_option(13, 4, 0, 0, 0, 0, false, 0);
            std::thread::sleep(std::time::Duration::from_millis(300));

            println!("\nDid the keyboard turn RED?");
            println!("If not, try running: ./target/release/iot_driver mode 13");
        }
        Some(Commands::Wave) => {
            use std::sync::atomic::AtomicBool;
            use std::sync::Arc;
            use iot_driver::devices::M1_V5_HE_LED_MATRIX;

            let device = iot_driver::hid::MonsGeekDevice::open()
                .map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Starting column-based wave animation...");
            println!("Press Ctrl+C to stop");

            device.set_led_with_option(13, 4, 0, 0, 0, 0, false, 0);
            std::thread::sleep(std::time::Duration::from_millis(200));

            let running = Arc::new(AtomicBool::new(true));
            let running_clone = Arc::clone(&running);

            ctrlc::set_handler(move || {
                running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            }).ok();

            const LEDS_PER_COL: usize = 6;
            const NUM_COLS: usize = 16;
            let mut wave_pos: f32 = 0.0;

            while running.load(std::sync::atomic::Ordering::SeqCst) {
                let mut colors = [(0u8, 0u8, 0u8); 126];

                for col in 0..NUM_COLS {
                    let col_pos = col as f32;
                    let dist = (col_pos - wave_pos).abs().min((col_pos - wave_pos + NUM_COLS as f32).abs());
                    let intensity = if dist < 3.0 {
                        (255.0 * (1.0 - dist / 3.0)) as u8
                    } else {
                        0
                    };

                    let hue = ((col as f32 / NUM_COLS as f32) * 360.0 + wave_pos * 20.0) % 360.0;
                    let (r, g, b) = hsv_to_rgb(hue, 1.0, intensity as f32 / 255.0);

                    for row in 0..LEDS_PER_COL {
                        let led_idx = col * LEDS_PER_COL + row;
                        if led_idx < 126 && M1_V5_HE_LED_MATRIX[led_idx] != 0 {
                            colors[led_idx] = (r, g, b);
                        }
                    }
                }

                device.set_per_key_colors_fast(colors.as_ref(), 10, 3);
                wave_pos = (wave_pos + 0.3) % (NUM_COLS as f32);
                std::thread::sleep(std::time::Duration::from_millis(16));
            }

            println!("\nWave animation stopped");
        }

        // === Audio Commands ===
        Some(Commands::Audio { mode, hue, sensitivity }) => {
            use std::sync::atomic::AtomicBool;
            use std::sync::Arc;

            let device = iot_driver::hid::MonsGeekDevice::open()
                .map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Starting audio reactive mode on {}...", device.display_name());
            println!("Press Ctrl+C to stop");

            let running = Arc::new(AtomicBool::new(true));
            let running_clone = Arc::clone(&running);

            ctrlc::set_handler(move || {
                running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            }).ok();

            let config = iot_driver::audio_reactive::AudioConfig {
                color_mode: mode.as_str().to_string(),
                base_hue: hue,
                sensitivity,
                smoothing: 0.3,
            };

            if let Err(e) = iot_driver::audio_reactive::run_audio_reactive(&device, config, running) {
                eprintln!("Audio reactive error: {e}");
            }
        }
        Some(Commands::AudioTest) => {
            println!("Testing audio capture...\n");

            println!("Available audio devices:");
            for name in iot_driver::audio_reactive::list_audio_devices() {
                println!("  - {name}");
            }
            println!();

            if let Err(e) = iot_driver::audio_reactive::test_audio_capture() {
                eprintln!("Audio test failed: {e}");
            }
        }
        Some(Commands::AudioLevels) => {
            if let Err(e) = iot_driver::audio_reactive::test_audio_levels() {
                eprintln!("Audio levels test failed: {e}");
            }
        }

        // === Screen Color Commands ===
        Some(Commands::Screen { fps }) => {
            use std::sync::atomic::AtomicBool;
            use std::sync::Arc;

            let fps = fps.clamp(1, 60);

            let device = iot_driver::hid::MonsGeekDevice::open()
                .map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Starting screen color mode on {}...", device.display_name());
            println!("Press Ctrl+C to stop");

            let running = Arc::new(AtomicBool::new(true));
            let running_clone = Arc::clone(&running);

            ctrlc::set_handler(move || {
                running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            }).ok();

            // Use await since main is already async
            if let Err(e) = iot_driver::screen_capture::run_screen_color_mode(&device, running, fps).await {
                eprintln!("Screen color mode error: {e}");
            }
        }

        // === Firmware Commands (DRY-RUN ONLY) ===
        Some(Commands::Firmware(fw_cmd)) => {
            match fw_cmd {
                FirmwareCommands::Info => {
                    if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                        let info = device.read_info();
                        let version_str = iot_driver::hid::MonsGeekDevice::format_version(info.version);

                        println!("Firmware Information");
                        println!("====================");
                        println!("Device:     {}", device.display_name());
                        println!("Device ID:  {} (0x{:08X})", info.device_id, info.device_id);
                        println!("Version:    {} (raw: 0x{:04X})", version_str, info.version);
                        println!("Boot Mode:  {}", if device.is_boot_mode() { "Yes" } else { "No" });

                        if let Some(api_id) = device.get_api_device_id() {
                            println!("API ID:     {api_id}");
                        }
                    } else {
                        eprintln!("No device found");
                    }
                }
                FirmwareCommands::Validate { file } => {
                    use iot_driver::firmware::FirmwareFile;

                    println!("Validating firmware file: {}", file.display());

                    match FirmwareFile::load(&file) {
                        Ok(fw) => {
                            println!("\nFirmware File Information");
                            println!("=========================");
                            println!("Filename:   {}", fw.filename);
                            println!("Type:       {}", fw.firmware_type);
                            println!("Size:       {} bytes ({} KB)", fw.size, fw.size / 1024);
                            println!("Checksum:   0x{:08X}", fw.checksum);
                            println!("Chunks:     {} (64 bytes each)", fw.chunk_count);

                            match fw.validate() {
                                Ok(()) => println!("\nStatus:     VALID"),
                                Err(e) => println!("\nStatus:     INVALID - {e}"),
                            }

                            // If ZIP, list contents
                            if file.extension().map(|e| e == "zip").unwrap_or(false) {
                                if let Ok(contents) = FirmwareFile::list_zip_contents(&file) {
                                    println!("\nZIP contents:");
                                    for name in contents {
                                        println!("  - {name}");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to load firmware file: {e}");
                        }
                    }
                }
                FirmwareCommands::DryRun { file, verbose } => {
                    use iot_driver::firmware::{FirmwareFile, dry_run_usb};

                    println!("=== DRY RUN - NO CHANGES WILL BE MADE ===\n");

                    // Try to get current device info
                    let (current_version, device_id) = if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                        let info = device.read_info();
                        let ver_str = iot_driver::hid::MonsGeekDevice::format_version(info.version);
                        (Some(ver_str), Some(info.device_id))
                    } else {
                        println!("Note: No device connected, simulating without device info\n");
                        (None, None)
                    };

                    match FirmwareFile::load(&file) {
                        Ok(fw) => {
                            if let Err(e) = fw.validate() {
                                eprintln!("Warning: Firmware validation failed: {e}");
                            }

                            let result = dry_run_usb(&fw, current_version, device_id);
                            result.print(verbose);
                        }
                        Err(e) => {
                            eprintln!("Failed to load firmware file: {e}");
                        }
                    }
                }
                FirmwareCommands::Check { device_id } => {
                    #[cfg(feature = "firmware-api")]
                    {
                        use iot_driver::firmware_api::{check_firmware_blocking, device_ids, ApiError};

                        // Try to get device ID from connected device or argument
                        let api_device_id = device_id.or_else(|| {
                            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                                device.get_api_device_id()
                            } else {
                                None
                            }
                        });

                        let api_device_id = match api_device_id {
                            Some(id) => id,
                            None => {
                                eprintln!("Could not determine device ID. Use --device-id to specify.");
                                eprintln!("Known device IDs:");
                                eprintln!("  M1 V5 HE: {}", device_ids::M1_V5_HE);
                                return Ok(());
                            }
                        };

                        println!("Checking for firmware updates for device ID {api_device_id}...");

                        match check_firmware_blocking(api_device_id) {
                            Ok(response) => {
                                println!("\nServer Firmware Versions");
                                println!("========================");
                                println!("{}", response.versions.display());

                                if let Some(path) = &response.versions.download_path {
                                    println!("\nDownload path: {path}");
                                }

                                if let Some(min_app) = &response.lowest_app_version {
                                    println!("Min app version: {min_app}");
                                }

                                // Compare with current device if connected
                                if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                                    let info = device.read_info();
                                    let current_usb = info.version;
                                    println!("\nCurrent device USB version: 0x{current_usb:04X}");

                                    if let Some(server_usb) = response.versions.usb {
                                        if server_usb > current_usb {
                                            println!("UPDATE AVAILABLE: 0x{current_usb:04X} -> 0x{server_usb:04X}");
                                        } else {
                                            println!("Firmware is up to date.");
                                        }
                                    }
                                }
                            }
                            Err(ApiError::ServerError(500, _)) => {
                                // 500 "Record not found" means device ID not in server database
                                // This is normal - the official app also shows "up to date" in this case
                                println!("\nDevice ID {api_device_id} not found in server database.");
                                println!("This is normal for some devices. Assuming firmware is up to date.");
                            }
                            Err(e) => {
                                eprintln!("Failed to check firmware: {e}");
                            }
                        }
                    }

                    #[cfg(not(feature = "firmware-api"))]
                    {
                        let _ = device_id;
                        eprintln!("Firmware API not enabled. Rebuild with: cargo build --features firmware-api");
                    }
                }
                FirmwareCommands::Download { device_id, output } => {
                    #[cfg(feature = "firmware-api")]
                    {
                        use iot_driver::firmware_api::{check_firmware_blocking, download_firmware_blocking, device_ids};

                        // Try to get device ID from connected device or argument
                        let api_device_id = device_id.or_else(|| {
                            if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                                device.get_api_device_id()
                            } else {
                                None
                            }
                        });

                        let api_device_id = match api_device_id {
                            Some(id) => id,
                            None => {
                                eprintln!("Could not determine device ID. Use --device-id to specify.");
                                eprintln!("Known device IDs:");
                                eprintln!("  M1 V5 HE: {}", device_ids::M1_V5_HE);
                                return Ok(());
                            }
                        };

                        println!("Getting firmware info for device ID {api_device_id}...");

                        match check_firmware_blocking(api_device_id) {
                            Ok(response) => {
                                if let Some(path) = response.versions.download_path {
                                    println!("Downloading from: {path}");
                                    match download_firmware_blocking(&path, &output) {
                                        Ok(size) => {
                                            println!("Downloaded {} bytes to {}", size, output.display());
                                        }
                                        Err(e) => {
                                            eprintln!("Download failed: {e}");
                                        }
                                    }
                                } else {
                                    eprintln!("No download path available for this device");
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to get firmware info: {e}");
                            }
                        }
                    }

                    #[cfg(not(feature = "firmware-api"))]
                    {
                        let _ = (device_id, output);
                        eprintln!("Firmware API not enabled. Rebuild with: cargo build --features firmware-api");
                    }
                }
            }
        }

        // === Utility Commands ===
        Some(Commands::List) => {
            let hidapi = HidApi::new()?;
            cli_list(&hidapi);
        }
        Some(Commands::Raw { cmd: cmd_str }) => {
            let hidapi = HidApi::new()?;
            let cmd_byte = u8::from_str_radix(&cmd_str, 16)?;
            cli_test(&hidapi, cmd_byte)?;
        }
        Some(Commands::Serve) => {
            // Fall through to server mode below
            run_server().await?;
        }
        Some(Commands::Tui) => {
            iot_driver::tui::run()?;
        }
    }

    Ok(())
}

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    // Server mode
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("iot_driver=debug".parse().unwrap())
        )
        .init();

    let addr = "127.0.0.1:3814".parse()?;

    info!("Starting IOT Driver Linux on {}", addr);
    println!("addr :: {addr}");
    println!("SSSSSSSSSSSTTTTTTTTTTTTTTTTTAAAAAAAAAAAARRRRRRRRRRRTTTTTTTTTTT!!!!!!!");

    let service = DriverService::new()
        .map_err(|e| format!("Failed to initialize HID API: {e}"))?;

    // Start hot-plug monitoring for device connect/disconnect
    service.start_hotplug_monitor();

    // Scan for devices on startup
    let devices = service.scan_devices();
    info!("Found {} devices on startup", devices.len());
    for dev in &devices {
        if let Some(dj_dev::OneofDev::Dev(d)) = &dev.oneof_dev {
            info!("  - VID={:04x} PID={:04x} ID={} path={}", d.vid, d.pid, d.id, d.path);
        }
    }

    // CORS layer for browser access - must allow all gRPC-Web headers
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any)
        .expose_headers(Any);

    // Wrap service with gRPC-Web support for browser clients
    let grpc_service = tonic_web::enable(DriverGrpcServer::new(service));

    info!("Server ready with gRPC-Web support");

    Server::builder()
        .accept_http1(true)  // Required for gRPC-Web
        .layer(cors)
        .add_service(grpc_service)
        .serve(addr)
        .await?;

    Ok(())
}
