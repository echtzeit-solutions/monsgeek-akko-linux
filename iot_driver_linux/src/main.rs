use clap::Parser;
use hidapi::HidApi;
use tonic::transport::Server;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

// Use shared protocol definitions from library
use iot_driver::color::hsv_to_rgb;
use iot_driver::protocol::{self, cmd};

// CLI definitions
mod cli;
use cli::{Cli, Commands, FirmwareCommands};

// gRPC server module
mod grpc;
use grpc::{dj_dev, DriverGrpcServer, DriverService};
use iot_driver::hal;

// New transport abstraction layer
use monsgeek_keyboard::{SleepTimeSettings, SyncKeyboard};
use monsgeek_transport::protocol::cmd as transport_cmd;
use monsgeek_transport::{list_devices_sync, ChecksumType, SyncTransport};

/// Open a device via the new transport layer, preferring Bluetooth when present.
fn open_preferred_transport() -> Result<SyncTransport, Box<dyn std::error::Error>> {
    let devices = list_devices_sync()?;
    if devices.is_empty() {
        return Err("No supported device found".into());
    }

    // Prefer Bluetooth for vendor protocol testing (then dongle, then wired)
    let preferred = devices
        .iter()
        .find(|d| d.info.transport_type == monsgeek_transport::TransportType::Bluetooth)
        .or_else(|| {
            devices
                .iter()
                .find(|d| d.info.transport_type == monsgeek_transport::TransportType::HidDongle)
        })
        .unwrap_or(&devices[0]);

    Ok(monsgeek_transport::open_device_sync(preferred)?)
}

/// Format and print a command response from the transport layer
/// `resp` is the response data (64 bytes, first byte is command echo)
fn format_command_response(cmd: u8, resp: &[u8]) {
    println!("\nResponse (0x{:02x} = {}):", resp[0], cmd::name(resp[0]));

    // Response offsets: resp[0] = cmd echo, resp[1..] = data
    // (Transport layer strips report ID, so indices are shifted -1 from raw HID)
    match cmd {
        cmd::GET_USB_VERSION => {
            let device_id = u32::from_le_bytes([resp[1], resp[2], resp[3], resp[4]]);
            let version = u16::from_le_bytes([resp[7], resp[8]]);
            println!("  Device ID:  {device_id} (0x{device_id:04X})");
            println!(
                "  Version:    {} (v{}.{:02})",
                version,
                version / 100,
                version % 100
            );
        }
        cmd::GET_PROFILE => {
            println!("  Profile:    {}", resp[1]);
        }
        cmd::GET_DEBOUNCE => {
            println!("  Debounce:   {} ms", resp[1]);
        }
        cmd::GET_LEDPARAM => {
            let mode = resp[1];
            let brightness = resp[2];
            let speed = protocol::LED_SPEED_MAX - resp[3].min(protocol::LED_SPEED_MAX);
            let options = resp[4];
            let r = resp[5];
            let g = resp[6];
            let b = resp[7];
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
            println!("  Fn Layer:   {}", resp[2]);
            println!("  Anti-ghost: {}", resp[3]);
            println!("  RTStab:     {} ms", resp[4] as u32 * 25);
            println!("  WASD Swap:  {}", resp[5]);
        }
        cmd::GET_FEATURE_LIST => {
            println!("  Features:   {:02x?}", &resp[1..11]);
            let precision =
                monsgeek_keyboard::settings::FirmwareVersion::precision_byte_str(resp[2]);
            println!("  Precision:  {precision}");
        }
        cmd::GET_SLEEPTIME => {
            let sleep_s = u16::from_le_bytes([resp[1], resp[2]]);
            println!("  Sleep:      {} seconds ({} min)", sleep_s, sleep_s / 60);
        }
        _ => {
            println!("  Raw data:   {:02x?}", &resp[..16.min(resp.len())]);
        }
    }
}

/// Send a raw command via transport layer and print response
fn cli_raw_command(cmd: u8) -> Result<(), Box<dyn std::error::Error>> {
    let transport = open_preferred_transport()?;
    let info = transport.device_info();
    println!(
        "Device: VID={:04X} PID={:04X} type={:?}",
        info.vid, info.pid, info.transport_type
    );
    println!("Sending command 0x{:02x} ({})...", cmd, cmd::name(cmd));

    let resp = transport.query_command(cmd, &[], ChecksumType::Bit7)?;
    format_command_response(cmd, &resp);
    Ok(())
}

/// List all HID devices
fn cli_list(hidapi: &HidApi) {
    println!("All HID devices:");
    for device_info in hidapi.device_list() {
        println!(
            "  VID={:04x} PID={:04x} usage={:04x} page={:04x} if={} path={}",
            device_info.vendor_id(),
            device_info.product_id(),
            device_info.usage(),
            device_info.usage_page(),
            device_info.interface_number(),
            device_info.path().to_string_lossy()
        );
    }
}

/// Run multiple commands and show all info (via transport layer)
fn cli_all() -> Result<(), Box<dyn std::error::Error>> {
    println!("MonsGeek M1 V5 HE - Device Information");
    println!("======================================\n");

    let transport = open_preferred_transport()?;
    let info = transport.device_info();
    println!(
        "Device: VID={:04X} PID={:04X} type={:?}\n",
        info.vid, info.pid, info.transport_type
    );

    // Query all relevant settings
    let commands = [
        (transport_cmd::GET_USB_VERSION, "Device Info"),
        (transport_cmd::GET_PROFILE, "Profile"),
        (transport_cmd::GET_DEBOUNCE, "Debounce"),
        (transport_cmd::GET_LEDPARAM, "LED"),
        (transport_cmd::GET_KBOPTION, "Options"),
        (transport_cmd::GET_FEATURE_LIST, "Features"),
    ];

    for (cmd_byte, name) in commands {
        print!("{name}: ");
        match transport.query_command(cmd_byte, &[], ChecksumType::Bit7) {
            Ok(resp) => format_command_response(cmd_byte, &resp),
            Err(e) => println!("Error: {e}"),
        }
        println!();
    }

    Ok(())
}

/// Test the new transport abstraction layer
fn cli_test_transport() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing new transport abstraction layer");
    println!("=======================================\n");

    // List devices using new discovery
    println!("Discovering devices...");
    let devices = list_devices_sync()?;

    if devices.is_empty() {
        println!("No devices found!");
        return Ok(());
    }

    for (i, dev) in devices.iter().enumerate() {
        println!(
            "  [{}] VID={:04X} PID={:04X} type={:?}",
            i, dev.info.vid, dev.info.pid, dev.info.transport_type
        );
        if let Some(name) = &dev.info.product_name {
            println!("      Name: {name}");
        }
    }

    // Open first device
    println!("\nOpening first device...");
    let transport = SyncTransport::open_any()?;
    let info = transport.device_info();
    println!(
        "  Opened: VID={:04X} PID={:04X} type={:?}",
        info.vid, info.pid, info.transport_type
    );

    // Query device ID
    println!("\nQuerying device ID (GET_USB_VERSION)...");
    let resp = transport.query_command(transport_cmd::GET_USB_VERSION, &[], ChecksumType::Bit7)?;
    let device_id = u32::from_le_bytes([resp[1], resp[2], resp[3], resp[4]]);
    let version = u16::from_le_bytes([resp[7], resp[8]]);
    println!("  Device ID: {device_id} (0x{device_id:08X})");
    println!(
        "  Version:   {} (v{}.{:02})",
        version,
        version / 100,
        version % 100
    );

    // Query profile
    println!("\nQuerying profile (GET_PROFILE)...");
    let resp = transport.query_command(transport_cmd::GET_PROFILE, &[], ChecksumType::Bit7)?;
    println!("  Profile:   {}", resp[1]);

    // Query LED params
    println!("\nQuerying LED params (GET_LEDPARAM)...");
    let resp = transport.query_command(transport_cmd::GET_LEDPARAM, &[], ChecksumType::Bit7)?;
    let mode = resp[1];
    let brightness = resp[2];
    let r = resp[5];
    let g = resp[6];
    let b = resp[7];
    println!("  Mode:       {} ({})", mode, cmd::led_mode_name(mode));
    println!("  Brightness: {brightness}/4");
    println!("  Color:      #{r:02X}{g:02X}{b:02X}");

    // Check if connected
    println!(
        "\nConnection status: {}",
        if transport.is_connected() {
            "connected"
        } else {
            "disconnected"
        }
    );

    // Test keyboard interface (includes trigger settings)
    println!("\n--- Testing Keyboard Interface ---");
    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            println!(
                "  Opened keyboard: {} keys, magnetism={}",
                keyboard.key_count(),
                keyboard.has_magnetism()
            );

            // Test trigger settings
            println!("\nQuerying trigger settings...");
            match keyboard.get_all_triggers() {
                Ok(triggers) => {
                    println!("  Got {} key modes", triggers.key_modes.len());
                    println!(
                        "  Got {} bytes of press_travel",
                        triggers.press_travel.len()
                    );

                    // Show first few bytes of each array
                    println!(
                        "\n  First 10 key_modes:  {:?}",
                        &triggers.key_modes[..10.min(triggers.key_modes.len())]
                    );
                    println!(
                        "  First 10 press_travel: {:?}",
                        &triggers.press_travel[..10.min(triggers.press_travel.len())]
                    );

                    // Decode first key's 16-bit travel
                    if triggers.press_travel.len() >= 2 {
                        let first_travel = u16::from_le_bytes([
                            triggers.press_travel[0],
                            triggers.press_travel[1],
                        ]);
                        println!(
                            "  First key travel (u16): {} ({:.2}mm at 0.01mm precision)",
                            first_travel,
                            first_travel as f32 / 100.0
                        );
                    }
                }
                Err(e) => println!("  Error: {e}"),
            }
        }
        Err(e) => println!("  Error opening keyboard: {e}"),
    }

    println!("\nTransport layer test PASSED!");
    Ok(())
}

/// Get battery status from 2.4GHz dongle
///
/// Checks kernel power_supply first (when eBPF filter loaded), falls back to vendor protocol.
fn cli_battery(
    hidapi: &HidApi,
    quiet: bool,
    show_hex: bool,
    watch: Option<Option<u64>>,
    force_vendor: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use iot_driver::power_supply::{find_dongle_battery_power_supply, read_kernel_battery};
    use std::time::Duration;

    // Determine watch interval (None = no watch, Some(None) = default 1s, Some(Some(n)) = n seconds)
    let watch_interval = watch.map(|opt| opt.unwrap_or(1));

    loop {
        // Check for kernel power_supply (eBPF filter loaded) unless --vendor flag
        if !force_vendor {
            if let Some(path) = find_dongle_battery_power_supply() {
                if quiet {
                    if let Some(info) = read_kernel_battery(&path) {
                        println!("{}", info.level);
                    } else {
                        eprintln!("Failed to read battery");
                        std::process::exit(1);
                    }
                } else {
                    println!("Battery Status (kernel)");
                    println!("-----------------------");
                    println!("  Source: {}", path.display());
                    if let Some(info) = read_kernel_battery(&path) {
                        println!("  Level:     {}%", info.level);
                        println!("  Connected: {}", if info.online { "Yes" } else { "No" });
                        println!("  Charging:  {}", if info.charging { "Yes" } else { "No" });
                    } else {
                        println!("  Failed to read battery status");
                    }
                }
                if watch_interval.is_none() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_secs(watch_interval.unwrap()));
                continue;
            }
        }

        // Use vendor protocol (direct HID)
        let result = read_vendor_battery(hidapi, show_hex);

        match result {
            Some((battery, online, idle, raw_bytes)) => {
                if quiet {
                    println!("{battery}");
                } else if show_hex {
                    // Print full hex dump for analysis
                    print_hex_dump(&raw_bytes);
                } else {
                    println!("Battery Status (vendor)");
                    println!("-----------------------");
                    println!("  Level:     {battery}%");
                    println!("  Connected: {}", if online { "Yes" } else { "No" });
                    println!(
                        "  Idle:      {}",
                        if idle {
                            "Yes (sleeping)"
                        } else {
                            "No (active)"
                        }
                    );
                    let hex: Vec<String> =
                        raw_bytes[1..8].iter().map(|b| format!("{b:02x}")).collect();
                    println!("  Raw[1..8]: {}", hex.join(" "));
                }
            }
            None => {
                if quiet {
                    eprintln!("No battery data");
                    std::process::exit(1);
                } else {
                    println!("No 2.4GHz dongle found or battery data unavailable");
                }
            }
        }

        if let Some(interval) = watch_interval {
            std::thread::sleep(Duration::from_secs(interval));
        } else {
            break;
        }
    }

    Ok(())
}

/// Read battery from vendor protocol, returns (battery%, online, idle, full_response)
fn read_vendor_battery(hidapi: &HidApi, show_debug: bool) -> Option<(u8, bool, bool, [u8; 65])> {
    for device_info in hidapi.device_list() {
        let vid = device_info.vendor_id();
        let pid = device_info.product_id();

        // Only match dongle devices
        if vid != hal::VENDOR_ID || !hal::is_dongle_pid(pid) {
            continue;
        }

        // Match vendor interface (Usage 0x02 on page 0xFFFF)
        if device_info.usage_page() != 0xFFFF || device_info.usage() != 0x02 {
            continue;
        }

        let device = match device_info.open_device(hidapi) {
            Ok(d) => d,
            Err(e) => {
                if show_debug {
                    eprintln!("Failed to open vendor interface: {e:?}");
                }
                continue;
            }
        };

        // Send F7 command to trigger battery refresh
        let f7_cmd =
            protocol::build_command(cmd::BATTERY_REFRESH, &[], protocol::ChecksumType::Bit7);
        if let Err(e) = device.send_feature_report(&f7_cmd) {
            if show_debug {
                eprintln!("F7 send failed: {e:?}");
            }
        } else if show_debug {
            eprintln!("F7 sent OK, not waiting");
        }

        // Small delay for dongle to query keyboard
        //        std::thread::sleep(std::time::Duration::from_millis(0));

        // Get Feature report with Report ID 5
        let mut buf = [0u8; 65];
        buf[0] = 0x05;

        match device.get_feature_report(&mut buf) {
            Ok(_len) => {
                let battery = buf[1];
                let idle = buf[3] != 0;
                let online = buf[4] != 0;

                // Return data even if battery is 0 (for debugging)
                // Caller can decide if 0 is valid
                return Some((battery, online, idle, buf));
            }
            Err(e) => {
                if show_debug {
                    eprintln!("get_feature_report failed: {e:?}");
                }
            }
        }
    }
    None
}

/// Print hex dump of full response for protocol analysis
fn print_hex_dump(data: &[u8; 65]) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() % 86400; // Seconds since midnight (rough)
    let hours = (secs / 3600) % 24;
    let mins = (secs % 3600) / 60;
    let sec = secs % 60;
    let millis = now.subsec_millis();
    println!(
        "[{hours:02}:{mins:02}:{sec:02}.{millis:03}] Full vendor response ({} bytes):",
        data.len()
    );

    // Print in 16-byte rows with offset
    for (i, chunk) in data.chunks(16).enumerate() {
        let offset = i * 16;
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("  {offset:04x}: {:<48} |{ascii}|", hex.join(" "));
    }

    // Highlight known fields
    println!("  ---");
    println!("  byte[0] = 0x{:02x} (Report ID)", data[0]);
    println!("  byte[1] = {} (Battery %)", data[1]);
    println!("  byte[2] = 0x{:02x}", data[2]);
    println!(
        "  byte[3] = 0x{:02x} (Idle: {})",
        data[3],
        if data[3] != 0 { "Yes" } else { "No" }
    );
    println!(
        "  byte[4] = {} (Online: {})",
        data[4],
        if data[4] != 0 { "Yes" } else { "No" }
    );
    println!("  byte[5] = 0x{:02x}", data[5]);
    println!("  byte[6] = 0x{:02x}", data[6]);
    println!("  byte[7] = 0x{:02x}", data[7]);
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
            let transport = open_preferred_transport()?;
            let info = transport.device_info();
            println!(
                "Device: VID={:04X} PID={:04X} type={:?}",
                info.vid, info.pid, info.transport_type
            );

            let resp =
                transport.query_command(transport_cmd::GET_USB_VERSION, &[], ChecksumType::Bit7)?;
            let device_id = u32::from_le_bytes([resp[1], resp[2], resp[3], resp[4]]);
            let version = u16::from_le_bytes([resp[7], resp[8]]);
            println!("Device ID: {device_id} (0x{device_id:08X})");
            println!(
                "Version:   {} (v{}.{:02})",
                version,
                version / 100,
                version % 100
            );
        }
        Some(Commands::Profile) => {
            let transport = open_preferred_transport()?;
            let resp =
                transport.query_command(transport_cmd::GET_PROFILE, &[], ChecksumType::Bit7)?;
            println!("Profile: {}", resp[1]);
        }
        Some(Commands::Led) => {
            let transport = open_preferred_transport()?;
            let resp =
                transport.query_command(transport_cmd::GET_LEDPARAM, &[], ChecksumType::Bit7)?;
            let mode = resp[1];
            let speed = resp[2];
            let brightness = resp[3];
            let r = resp[5];
            let g = resp[6];
            let b = resp[7];
            println!("LED:");
            println!("  Mode:       {} ({})", mode, cmd::led_mode_name(mode));
            println!("  Speed:      {speed}/4");
            println!("  Brightness: {brightness}/4");
            println!("  Color:      #{r:02X}{g:02X}{b:02X}");
        }
        Some(Commands::Debounce) => {
            let transport = open_preferred_transport()?;
            let resp =
                transport.query_command(transport_cmd::GET_DEBOUNCE, &[], ChecksumType::Bit7)?;
            println!("Debounce: {} ms", resp[1]);
        }
        Some(Commands::Rate) => match SyncKeyboard::open_any() {
            Ok(keyboard) => match keyboard.get_polling_rate() {
                Ok(rate) => {
                    let hz = rate as u16;
                    println!("Polling rate: {hz} ({})", protocol::polling_rate::name(hz));
                }
                Err(e) => eprintln!("Failed to get polling rate: {e}"),
            },
            Err(e) => eprintln!("No device found: {e}"),
        },
        Some(Commands::Options) => {
            let transport = open_preferred_transport()?;
            let resp =
                transport.query_command(transport_cmd::GET_KBOPTION, &[], ChecksumType::Bit7)?;
            println!("Options (raw): {:02X?}", &resp[..16.min(resp.len())]);
        }
        Some(Commands::Features) => {
            let transport = open_preferred_transport()?;
            let resp = transport.query_command(
                transport_cmd::GET_FEATURE_LIST,
                &[],
                ChecksumType::Bit7,
            )?;
            println!("Features (raw): {:02X?}", &resp[..24.min(resp.len())]);
        }
        Some(Commands::Sleep) => match SyncKeyboard::open_any() {
            Ok(keyboard) => match keyboard.get_sleep_time() {
                Ok(settings) => {
                    println!("Sleep Time Settings:");
                    println!("  Bluetooth:");
                    println!(
                        "    Idle:       {} ({})",
                        settings.idle_bt,
                        SleepTimeSettings::format_duration(settings.idle_bt)
                    );
                    println!(
                        "    Deep Sleep: {} ({})",
                        settings.deep_bt,
                        SleepTimeSettings::format_duration(settings.deep_bt)
                    );
                    println!("  2.4GHz:");
                    println!(
                        "    Idle:       {} ({})",
                        settings.idle_24g,
                        SleepTimeSettings::format_duration(settings.idle_24g)
                    );
                    println!(
                        "    Deep Sleep: {} ({})",
                        settings.deep_24g,
                        SleepTimeSettings::format_duration(settings.deep_24g)
                    );
                }
                Err(e) => eprintln!("Failed to get sleep settings: {e}"),
            },
            Err(e) => eprintln!("No device found: {e}"),
        },
        Some(Commands::All) => {
            cli_all()?;
        }

        Some(Commands::Battery {
            quiet,
            hex,
            watch,
            vendor,
        }) => {
            let hidapi = HidApi::new()?;
            cli_battery(&hidapi, quiet, hex, watch, vendor)?;
        }

        // === Set Commands ===
        Some(Commands::SetProfile { profile }) => match SyncKeyboard::open_any() {
            Ok(keyboard) => match keyboard.set_profile(profile) {
                Ok(_) => println!("Profile set to {profile}"),
                Err(e) => eprintln!("Failed to set profile: {e}"),
            },
            Err(e) => eprintln!("No device found: {e}"),
        },
        Some(Commands::SetDebounce { ms }) => match SyncKeyboard::open_any() {
            Ok(keyboard) => match keyboard.set_debounce(ms) {
                Ok(_) => println!("Debounce set to {ms} ms"),
                Err(e) => eprintln!("Failed to set debounce: {e}"),
            },
            Err(e) => eprintln!("No device found: {e}"),
        },
        Some(Commands::SetRate { rate }) => {
            use monsgeek_keyboard::PollingRate;
            if let Some(hz) = protocol::polling_rate::parse(&rate) {
                if let Some(rate_enum) = PollingRate::from_hz(hz) {
                    match SyncKeyboard::open_any() {
                        Ok(keyboard) => match keyboard.set_polling_rate(rate_enum) {
                            Ok(_) => println!(
                                "Polling rate set to {hz} ({})",
                                protocol::polling_rate::name(hz)
                            ),
                            Err(e) => eprintln!("Failed to set polling rate: {e}"),
                        },
                        Err(e) => eprintln!("No device found: {e}"),
                    }
                } else {
                    eprintln!("Invalid polling rate '{hz}'. Valid rates: 125, 250, 500, 1000, 2000, 4000, 8000");
                }
            } else {
                eprintln!("Invalid polling rate '{rate}'. Valid rates: 125, 250, 500, 1000, 2000, 4000, 8000");
            }
        }
        Some(Commands::SetLed {
            mode,
            brightness,
            speed,
            r,
            g,
            b,
        }) => {
            let mode_num = cmd::LedMode::parse(&mode)
                .map(|m| m.as_u8())
                .unwrap_or_else(|| mode.parse().unwrap_or(1));
            match SyncKeyboard::open_any() {
                Ok(keyboard) => {
                    match keyboard.set_led(mode_num, brightness, speed, r, g, b, false) {
                        Ok(_) => println!(
                        "LED set: mode={} ({}) brightness={} speed={} color=#{:02X}{:02X}{:02X}",
                        mode_num, cmd::led_mode_name(mode_num), brightness, speed, r, g, b
                    ),
                        Err(e) => eprintln!("Failed to set LED: {e}"),
                    }
                }
                Err(e) => eprintln!("No device found: {e}"),
            }
        }
        Some(Commands::SetSleep {
            idle,
            deep,
            idle_bt,
            idle_24g,
            deep_bt,
            deep_24g,
            uniform,
        }) => match SyncKeyboard::open_any() {
            Ok(keyboard) => {
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
                    SleepTimeSettings::parse_duration(s)
                        .ok_or_else(|| format!("Invalid duration: {s}"))
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
            }
            Err(e) => eprintln!("No device found: {e}"),
        },
        Some(Commands::Reset) => {
            print!("This will factory reset the keyboard. Are you sure? (y/N) ");
            use std::io::{self, Write};
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            if input.trim().to_lowercase() == "y" {
                match SyncKeyboard::open_any() {
                    Ok(keyboard) => match keyboard.reset() {
                        Ok(_) => println!("Keyboard reset to factory defaults"),
                        Err(e) => eprintln!("Failed to reset keyboard: {e}"),
                    },
                    Err(e) => eprintln!("No device found: {e}"),
                }
            } else {
                println!("Reset cancelled");
            }
        }
        Some(Commands::Calibrate) => match SyncKeyboard::open_any() {
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
        },

        // === Trigger Commands ===
        Some(Commands::Triggers) => match SyncKeyboard::open_any() {
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
                            protocol::magnetism::mode_name(first_mode)
                        );
                        println!();

                        let all_same_press = (0..num_keys)
                            .all(|i| decode_u16(&triggers.press_travel, i) == first_press);
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
        },
        Some(Commands::SetActuation { mm }) => match SyncKeyboard::open_any() {
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
        },
        Some(Commands::SetRt { value }) => match SyncKeyboard::open_any() {
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
        },
        Some(Commands::SetRelease { mm }) => match SyncKeyboard::open_any() {
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
        },
        Some(Commands::SetBottomDeadzone { mm }) => match SyncKeyboard::open_any() {
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
        },
        Some(Commands::SetTopDeadzone { mm }) => match SyncKeyboard::open_any() {
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
        },
        Some(Commands::SetKeyTrigger {
            key,
            actuation,
            release,
            mode,
        }) => match SyncKeyboard::open_any() {
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
                let settings = monsgeek_keyboard::KeyTriggerSettings {
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
                            "normal" | "n" => monsgeek_keyboard::KeyMode::Normal,
                            "rt" | "rapid" | "rapidtrigger" => {
                                monsgeek_keyboard::KeyMode::RapidTrigger
                            }
                            "dks" | "dynamic" => monsgeek_keyboard::KeyMode::DynamicKeystroke,
                            "snaptap" | "snap" | "st" => monsgeek_keyboard::KeyMode::SnapTap,
                            "modtap" | "mt" => monsgeek_keyboard::KeyMode::ModTap,
                            "toggle" | "tgl" => monsgeek_keyboard::KeyMode::ToggleHold,
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
        },

        // === Per-key Color Commands ===
        Some(Commands::SetColorAll { r, g, b, layer }) => match SyncKeyboard::open_any() {
            Ok(keyboard) => {
                println!("Setting all keys to color #{r:02X}{g:02X}{b:02X}...");
                let color = monsgeek_keyboard::led::RgbColor { r, g, b };
                match keyboard.set_all_keys_color(color, layer) {
                    Ok(()) => println!("All keys set to #{r:02X}{g:02X}{b:02X}"),
                    Err(e) => eprintln!("Failed to set per-key colors: {e}"),
                }
            }
            Err(e) => eprintln!("No device found: {e}"),
        },

        // === Key Remapping ===
        Some(Commands::Remap { from, to, layer }) => {
            let key_index: u8 = from.parse().unwrap_or(0);
            let hid_code = u8::from_str_radix(&to, 16).unwrap_or(0);

            match SyncKeyboard::open_any() {
                Ok(keyboard) => {
                    let key_name = iot_driver::protocol::hid::key_name(hid_code);
                    println!("Remapping key {key_index} to {key_name} (0x{hid_code:02x}) on layer {layer}...");
                    match keyboard.set_keymatrix(layer, key_index, hid_code, true, 0) {
                        Ok(()) => println!("Key {key_index} remapped to {key_name}"),
                        Err(e) => eprintln!("Failed to remap key: {e}"),
                    }
                }
                Err(e) => eprintln!("No device found: {e}"),
            }
        }
        Some(Commands::ResetKey { key, layer }) => {
            let key_index: u8 = key.parse().unwrap_or(0);

            match SyncKeyboard::open_any() {
                Ok(keyboard) => {
                    println!("Resetting key {key_index} on layer {layer}...");
                    match keyboard.reset_key(layer, key_index) {
                        Ok(()) => println!("Key {key_index} reset to default"),
                        Err(e) => eprintln!("Failed to reset key: {e}"),
                    }
                }
                Err(e) => eprintln!("No device found: {e}"),
            }
        }
        Some(Commands::Swap { key1, key2, layer }) => {
            let key_a: u8 = key1.parse().unwrap_or(0);
            let key_b: u8 = key2.parse().unwrap_or(0);

            match SyncKeyboard::open_any() {
                Ok(keyboard) => match keyboard.get_keymatrix(layer, 2) {
                    Ok(data) => {
                        let code_a = if (key_a as usize) * 4 + 2 < data.len() {
                            data[(key_a as usize) * 4 + 2]
                        } else {
                            0
                        };
                        let code_b = if (key_b as usize) * 4 + 2 < data.len() {
                            data[(key_b as usize) * 4 + 2]
                        } else {
                            0
                        };

                        let name_a = iot_driver::protocol::hid::key_name(code_a);
                        let name_b = iot_driver::protocol::hid::key_name(code_b);
                        println!("Swapping key {key_a} ({name_a}) <-> key {key_b} ({name_b})...");

                        match keyboard.swap_keys(layer, key_a, code_a, key_b, code_b) {
                            Ok(()) => println!("Keys swapped successfully"),
                            Err(e) => eprintln!("Failed to swap keys: {e}"),
                        }
                    }
                    Err(e) => eprintln!("Failed to read current key mappings: {e}"),
                },
                Err(e) => eprintln!("No device found: {e}"),
            }
        }
        Some(Commands::Keymatrix { layer }) => match SyncKeyboard::open_any() {
            Ok(keyboard) => {
                println!("Reading key matrix for layer {layer}...");

                match keyboard.get_keymatrix(layer, 3) {
                    Ok(data) => {
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
                        let key_count = keyboard.key_count() as usize;
                        for i in 0..key_count.min(20) {
                            if i * 4 + 3 < data.len() {
                                let k = &data[i * 4..(i + 1) * 4];
                                let hid_code = k[2];
                                let key_name = iot_driver::protocol::hid::key_name(hid_code);
                                println!(
                                    "  Key {:2}: {:02x} {:02x} {:02x} {:02x}  -> {} (0x{:02x})",
                                    i, k[0], k[1], k[2], k[3], key_name, hid_code
                                );
                            }
                        }
                    }
                    Err(e) => eprintln!("Failed to read key matrix: {e}"),
                }
            }
            Err(e) => eprintln!("No device found: {e}"),
        },

        // === Macro Commands ===
        Some(Commands::Macro { key }) => {
            let macro_index: u8 = key.parse().unwrap_or(0);
            match SyncKeyboard::open_any() {
                Ok(keyboard) => {
                    println!("Reading macro {macro_index}...");
                    match keyboard.get_macro(macro_index) {
                        Ok(data) => {
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

                                        let event_type =
                                            if flags & 0x80 != 0 { "Down" } else { "Up" };
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
                        }
                        Err(e) => eprintln!("Failed to read macro: {e}"),
                    }
                }
                Err(e) => eprintln!("No device found: {e}"),
            }
        }
        Some(Commands::SetMacro { key, text }) => {
            let macro_index: u8 = key.parse().unwrap_or(0);

            match SyncKeyboard::open_any() {
                Ok(keyboard) => {
                    println!("Setting macro {macro_index} to type: \"{text}\"");

                    match keyboard.set_text_macro(macro_index, &text, 10, 1) {
                        Ok(()) => {
                            println!("Macro {macro_index} set successfully!");
                            println!("Assign this macro to a key in the Akko driver to test.");
                        }
                        Err(e) => eprintln!("Failed to set macro: {e}"),
                    }
                }
                Err(e) => eprintln!("No device found: {e}"),
            }
        }
        Some(Commands::ClearMacro { key }) => {
            let macro_index: u8 = key.parse().unwrap_or(0);

            match SyncKeyboard::open_any() {
                Ok(keyboard) => {
                    println!("Clearing macro {macro_index}...");

                    match keyboard.set_macro(macro_index, &[], 1) {
                        Ok(()) => println!("Macro {macro_index} cleared!"),
                        Err(e) => eprintln!("Failed to clear macro: {e}"),
                    }
                }
                Err(e) => eprintln!("No device found: {e}"),
            }
        }

        // === Animation Commands ===
        Some(Commands::Gif {
            file,
            mode,
            test,
            frames,
            delay,
        }) => {
            use iot_driver::gif::{generate_test_animation, load_gif, print_animation_info};

            let keyboard =
                SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

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

            let anim_frames: Vec<Vec<(u8, u8, u8)>> = animation
                .frames
                .iter()
                .take(255)
                .map(|f| f.colors.clone())
                .collect();

            let delay_ms = animation.frames.first().map(|f| f.delay_ms).unwrap_or(100);

            println!(
                "\nUploading {} frames ({}ms delay)...",
                anim_frames.len(),
                delay_ms
            );
            match keyboard.upload_animation(&anim_frames, delay_ms) {
                Ok(()) => println!("Animation uploaded! Keyboard will play it autonomously."),
                Err(e) => eprintln!("Failed to upload animation: {e}"),
            }
        }
        Some(Commands::GifStream { file, mode, r#loop }) => {
            use iot_driver::gif::{load_gif, print_animation_info};
            use std::sync::atomic::{AtomicBool, Ordering};
            use std::sync::Arc;

            println!("Loading GIF: {file}");
            let animation =
                load_gif(&file, mode.into()).map_err(|e| format!("Failed to load GIF: {e}"))?;

            print_animation_info(&animation);

            let keyboard =
                SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

            let _ = keyboard.set_led_with_option(13, 4, 0, 0, 0, 0, false, 0);

            let running = Arc::new(AtomicBool::new(true));
            let r = running.clone();

            ctrlc::set_handler(move || {
                r.store(false, Ordering::SeqCst);
            })
            .ok();

            println!("\nStreaming animation (Ctrl+C to stop)...");

            loop {
                for (idx, frame) in animation.frames.iter().enumerate() {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }

                    let _ = keyboard.set_per_key_colors_fast(&frame.colors, 10, 3);
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

            match SyncKeyboard::open_any() {
                Ok(keyboard) => {
                    println!(
                        "Setting LED mode to {} ({}) with layer {}...",
                        led_mode.name(),
                        led_mode.as_u8(),
                        layer
                    );
                    match keyboard.set_led_with_option(
                        led_mode.as_u8(),
                        4,
                        0,
                        128,
                        128,
                        128,
                        false,
                        layer,
                    ) {
                        Ok(_) => println!("Done."),
                        Err(e) => eprintln!("Failed to set LED mode: {e}"),
                    }
                }
                Err(e) => eprintln!("Failed to open device: {e}"),
            }
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

            let keyboard =
                SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Starting rainbow test on {}...", keyboard.device_name());
            println!("Press Ctrl+C to stop");

            let running = Arc::new(AtomicBool::new(true));
            let running_clone = Arc::clone(&running);

            ctrlc::set_handler(move || {
                running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            })
            .ok();

            if let Err(e) = iot_driver::audio_reactive::run_rainbow_test(&keyboard, running) {
                eprintln!("Rainbow test error: {e}");
            }
        }
        Some(Commands::Checkerboard) => {
            let keyboard =
                SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

            println!("=== Per-Key Color Test ===\n");

            println!("1. Current LED settings:");
            if let Ok(resp) = keyboard.query_raw_cmd(0x87) {
                if resp.len() >= 9 {
                    println!(
                        "   Mode: {}, Speed: {}, Brightness: {}, Option: {}, RGB: ({},{},{})",
                        resp[1], resp[2], resp[3], resp[4], resp[5], resp[6], resp[7]
                    );
                }
            }

            println!("\n2. Setting LED mode to 13 (LightUserPicture)...");
            if keyboard.set_led(13, 4, 0, 0, 0, 0, false).is_err() {
                println!("   ERROR: Failed to set LED mode!");
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(300));

            if let Ok(resp) = keyboard.query_raw_cmd(0x87) {
                if resp.len() >= 9 {
                    println!(
                        "   Mode: {}, Speed: {}, Brightness: {}, Option: {}, RGB: ({},{},{})",
                        resp[1], resp[2], resp[3], resp[4], resp[5], resp[6], resp[7]
                    );
                }
            }

            const MATRIX_SIZE: usize = 126;
            println!("\n3. Writing RED to ALL layers (0-3)...");
            let red_colors: Vec<(u8, u8, u8)> = vec![(255, 0, 0); MATRIX_SIZE];

            for layer in 0..4 {
                println!("   Writing to layer {layer}...");
                let _ = keyboard.set_per_key_colors_to_layer(&red_colors, layer);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));

            for layer in 0..4 {
                println!(
                    "   Setting layer {} (option byte = {:#04X})...",
                    layer,
                    layer << 4
                );
                let _ = keyboard.set_led_with_option(13, 4, 0, 0, 0, 0, false, layer);
                std::thread::sleep(std::time::Duration::from_millis(500));
            }

            let mut input = String::new();
            println!("   Did any layer show RED? [Enter to continue]");
            std::io::stdin().read_line(&mut input).ok();

            println!("\n4. Setting ALL keys to BLUE...");
            let blue_colors: Vec<(u8, u8, u8)> = vec![(0, 0, 255); MATRIX_SIZE];
            let _ = keyboard.set_per_key_colors_fast(&blue_colors, 100, 20);
            std::thread::sleep(std::time::Duration::from_millis(300));
            println!("   Did the keyboard turn BLUE? [Enter to continue]");
            std::io::stdin().read_line(&mut input).ok();

            println!("\n5. Setting ALL keys to GREEN...");
            let green_colors: Vec<(u8, u8, u8)> = vec![(0, 255, 0); MATRIX_SIZE];
            let _ = keyboard.set_per_key_colors_fast(&green_colors, 100, 20);
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
            let _ = keyboard.set_per_key_colors_fast(&checker_colors, 100, 20);
            println!("   Did the keyboard show alternating RED/BLUE? [Enter to finish]");
            std::io::stdin().read_line(&mut input).ok();

            println!("\nTest complete!");
        }
        Some(Commands::Sweep) => {
            use std::sync::atomic::AtomicBool;
            use std::sync::Arc;

            let keyboard =
                SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Starting sweep animation on {}...", keyboard.device_name());
            println!("Press Ctrl+C to stop");

            let _ = keyboard.set_led(25, 4, 0, 0, 0, 0, false);
            std::thread::sleep(std::time::Duration::from_millis(100));

            let running = Arc::new(AtomicBool::new(true));
            let running_clone = Arc::clone(&running);

            ctrlc::set_handler(move || {
                running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            })
            .ok();

            let key_count = keyboard.key_count() as usize;
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

                let _ = keyboard.set_per_key_colors_fast(&colors, 10, 2);
                position = (position + 1) % key_count;

                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            println!("Sweep animation stopped");
        }
        Some(Commands::Red) => {
            let keyboard =
                SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Simple RED test:");
            println!("1. Setting mode 13 (LightUserPicture) with layer 0...");
            let _ = keyboard.set_led_with_option(13, 4, 0, 0, 0, 0, false, 0);
            std::thread::sleep(std::time::Duration::from_millis(500));

            println!("2. Writing RED to layer 0 (126 keys)...");
            let red: Vec<(u8, u8, u8)> = vec![(255, 0, 0); 126];
            let _ = keyboard.set_per_key_colors_to_layer(&red, 0);
            std::thread::sleep(std::time::Duration::from_millis(500));

            println!("3. Re-setting mode 13 with layer 0 to refresh...");
            let _ = keyboard.set_led_with_option(13, 4, 0, 0, 0, 0, false, 0);
            std::thread::sleep(std::time::Duration::from_millis(300));

            println!("\nDid the keyboard turn RED?");
            println!("If not, try running: ./target/release/iot_driver mode 13");
        }
        Some(Commands::Wave) => {
            use iot_driver::devices::M1_V5_HE_LED_MATRIX;
            use std::sync::atomic::AtomicBool;
            use std::sync::Arc;

            let keyboard =
                SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Starting column-based wave animation...");
            println!("Press Ctrl+C to stop");

            let _ = keyboard.set_led_with_option(13, 4, 0, 0, 0, 0, false, 0);
            std::thread::sleep(std::time::Duration::from_millis(200));

            let running = Arc::new(AtomicBool::new(true));
            let running_clone = Arc::clone(&running);

            ctrlc::set_handler(move || {
                running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            })
            .ok();

            const LEDS_PER_COL: usize = 6;
            const NUM_COLS: usize = 16;
            let mut wave_pos: f32 = 0.0;

            while running.load(std::sync::atomic::Ordering::SeqCst) {
                let mut colors = [(0u8, 0u8, 0u8); 126];

                for col in 0..NUM_COLS {
                    let col_pos = col as f32;
                    let dist = (col_pos - wave_pos)
                        .abs()
                        .min((col_pos - wave_pos + NUM_COLS as f32).abs());
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

                let _ = keyboard.set_per_key_colors_fast(colors.as_ref(), 10, 3);
                wave_pos = (wave_pos + 0.3) % (NUM_COLS as f32);
                std::thread::sleep(std::time::Duration::from_millis(16));
            }

            println!("\nWave animation stopped");
        }

        // === Audio Commands ===
        Some(Commands::Audio {
            mode,
            hue,
            sensitivity,
        }) => {
            use std::sync::atomic::AtomicBool;
            use std::sync::Arc;

            let keyboard =
                SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

            println!(
                "Starting audio reactive mode on {}...",
                keyboard.device_name()
            );
            println!("Press Ctrl+C to stop");

            let running = Arc::new(AtomicBool::new(true));
            let running_clone = Arc::clone(&running);

            ctrlc::set_handler(move || {
                running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            })
            .ok();

            let config = iot_driver::audio_reactive::AudioConfig {
                color_mode: mode.as_str().to_string(),
                base_hue: hue,
                sensitivity,
                smoothing: 0.3,
            };

            if let Err(e) =
                iot_driver::audio_reactive::run_audio_reactive(&keyboard, config, running)
            {
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
        #[cfg(feature = "screen-capture")]
        Some(Commands::Screen { fps }) => {
            use std::sync::atomic::AtomicBool;
            use std::sync::Arc;

            let fps = fps.clamp(1, 60);

            let keyboard =
                SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

            println!(
                "Starting screen color mode on {}...",
                keyboard.device_name()
            );
            println!("Press Ctrl+C to stop");

            let running = Arc::new(AtomicBool::new(true));
            let running_clone = Arc::clone(&running);

            ctrlc::set_handler(move || {
                running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            })
            .ok();

            // Use await since main is already async
            if let Err(e) =
                iot_driver::screen_capture::run_screen_color_mode(&keyboard, running, fps).await
            {
                eprintln!("Screen color mode error: {e}");
            }
        }

        // === Debug: Key Depth Monitoring ===
        Some(Commands::Depth {
            raw: show_raw,
            zero: show_zero,
            verbose,
        }) => {
            use std::sync::atomic::{AtomicBool, Ordering};
            use std::sync::Arc;

            // Open device via transport abstraction (works with wired, dongle, BT, WebRTC)
            let keyboard =
                SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

            println!("Device: {}", keyboard.device_name());

            // Get precision from device
            let version = keyboard.get_version().unwrap_or_default();
            let precision = keyboard.get_precision().unwrap_or_default();
            println!(
                "Firmware version: {} (precision: {})",
                version.format_dotted(),
                precision.as_str()
            );

            // Enable magnetism reporting via transport
            println!("\nEnabling magnetism reporting...");
            match keyboard.start_magnetism_report() {
                Ok(()) => println!("Magnetism reporting enabled"),
                Err(e) => {
                    eprintln!("Failed to enable magnetism reporting: {e}");
                    return Ok(());
                }
            }

            // Wait for start confirmation
            println!("Waiting for magnetism start confirmation...");
            std::thread::sleep(std::time::Duration::from_millis(200));

            // Try to read confirmation event
            match keyboard.read_key_depth(500, precision.factor()) {
                Ok(Some(event)) => {
                    println!(
                        "Confirmation: Key {} depth={:.2}mm",
                        event.key_index, event.depth_mm
                    );
                }
                Ok(None) => println!("No confirmation event (timeout)"),
                Err(e) => println!("Read error: {e}"),
            }

            println!("\nMonitoring key depth (Ctrl+C to stop)...");
            println!("Press keys to see depth data.\n");

            let running = Arc::new(AtomicBool::new(true));
            let running_clone = Arc::clone(&running);

            ctrlc::set_handler(move || {
                running_clone.store(false, Ordering::SeqCst);
            })
            .ok();

            let mut report_count = 0u64;
            let start = std::time::Instant::now();
            let mut last_print = std::time::Instant::now();

            // Track latest depth per key for batched display
            let mut key_depths: std::collections::HashMap<u8, (u16, f32)> =
                std::collections::HashMap::new();

            while running.load(Ordering::SeqCst) {
                let mut batch_count = 0u32;

                // Batch read via transport abstraction
                // Works with any transport: HID wired, dongle, Bluetooth, WebRTC
                loop {
                    let timeout = if batch_count == 0 { 10 } else { 0 }; // 10ms initial, then non-blocking
                    match keyboard.read_key_depth(timeout, precision.factor()) {
                        Ok(Some(event)) => {
                            report_count += 1;
                            batch_count += 1;
                            key_depths.insert(event.key_index, (event.depth_raw, event.depth_mm));
                        }
                        _ => break, // No more data, timeout, or error
                    }
                }

                // Print at ~60Hz max to avoid flooding terminal
                let now = std::time::Instant::now();
                if now.duration_since(last_print).as_millis() >= 16 && !key_depths.is_empty() {
                    // Clear line and print all active keys
                    print!("\r\x1b[K"); // Clear line

                    // Sort keys and print
                    let mut keys: Vec<_> = key_depths.iter().collect();
                    keys.sort_by_key(|(k, _)| *k);

                    for (key_idx, (raw, depth_mm)) in &keys {
                        // Skip zero depths unless show_zero
                        if *raw == 0 && !show_zero {
                            continue;
                        }

                        // Compact bar (20 chars max)
                        let bar_len = ((*depth_mm * 5.0).min(20.0)) as usize;
                        let bar: String = "█".repeat(bar_len);
                        let empty: String = "░".repeat(20 - bar_len);

                        // Simple key index display (could add profile-based names later)
                        let key_name = format!("K{key_idx:02}");

                        if show_raw {
                            print!("{key_name}[{bar}{empty}]{depth_mm:.1}({raw:4}) ");
                        } else {
                            print!("{key_name}[{bar}{empty}]{depth_mm:.1} ");
                        }
                    }

                    if verbose {
                        let elapsed = start.elapsed().as_secs_f32();
                        let rate = report_count as f32 / elapsed;
                        print!(" [{rate:.0}/s]");
                    }

                    use std::io::Write;
                    std::io::stdout().flush().ok();

                    last_print = now;

                    // Remove keys that have returned to zero (after displaying once)
                    key_depths.retain(|_, (raw, _)| *raw > 0 || show_zero);
                }
            }

            println!("\n\nStopping...");
            let _ = keyboard.stop_magnetism_report();
            let elapsed = start.elapsed().as_secs_f32();
            println!(
                "Received {report_count} reports in {:.1}s ({:.0} reports/sec)",
                elapsed,
                report_count as f32 / elapsed
            );
        }

        // === Firmware Commands (DRY-RUN ONLY) ===
        Some(Commands::Firmware(fw_cmd)) => {
            match fw_cmd {
                FirmwareCommands::Info => {
                    match SyncKeyboard::open_any() {
                        Ok(keyboard) => {
                            let device_id = keyboard.get_device_id().unwrap_or(0);
                            let version = keyboard.get_version().unwrap_or_default();
                            let version_str = version.format_dotted();

                            println!("Firmware Information");
                            println!("====================");
                            println!("Device:     {}", keyboard.device_name());
                            println!("Device ID:  {device_id} (0x{device_id:08X})");
                            println!("Version:    {} (raw: 0x{:04X})", version_str, version.raw);

                            // Check boot mode via firmware_update module
                            let is_boot = iot_driver::protocol::firmware_update::is_boot_mode(
                                keyboard.vid(),
                                keyboard.pid(),
                            );
                            println!("Boot Mode:  {}", if is_boot { "Yes" } else { "No" });

                            // API ID is same as device ID, with VID/PID fallback
                            let api_id = if device_id != 0 {
                                Some(device_id)
                            } else {
                                iot_driver::firmware_api::device_ids::from_vid_pid(
                                    keyboard.vid(),
                                    keyboard.pid(),
                                )
                            };
                            if let Some(id) = api_id {
                                println!("API ID:     {id}");
                            }
                        }
                        Err(e) => eprintln!("No device found: {e}"),
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
                    use iot_driver::firmware::{dry_run_usb, FirmwareFile};

                    println!("=== DRY RUN - NO CHANGES WILL BE MADE ===\n");

                    // Try to get current device info
                    let (current_version, device_id) = match SyncKeyboard::open_any() {
                        Ok(keyboard) => {
                            let version = keyboard.get_version().unwrap_or_default();
                            let device_id = keyboard.get_device_id().unwrap_or(0);
                            (Some(version.format_dotted()), Some(device_id))
                        }
                        Err(_) => {
                            println!("Note: No device connected, simulating without device info\n");
                            (None, None)
                        }
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
                        use iot_driver::firmware_api::{check_firmware, device_ids, ApiError};

                        // Try to get device ID from connected device or argument
                        let (api_device_id, keyboard) = if let Some(id) = device_id {
                            (Some(id), None)
                        } else {
                            match SyncKeyboard::open_any() {
                                Ok(kb) => {
                                    let id = kb.get_device_id().ok().filter(|&id| id != 0);
                                    let id =
                                        id.or_else(|| device_ids::from_vid_pid(kb.vid(), kb.pid()));
                                    (id, Some(kb))
                                }
                                Err(_) => (None, None),
                            }
                        };

                        let api_device_id = match api_device_id {
                            Some(id) => id,
                            None => {
                                eprintln!(
                                    "Could not determine device ID. Use --device-id to specify."
                                );
                                eprintln!("Known device IDs:");
                                eprintln!("  M1 V5 HE: {}", device_ids::M1_V5_HE);
                                return Ok(());
                            }
                        };

                        println!("Checking for firmware updates for device ID {api_device_id}...");

                        match check_firmware(api_device_id).await {
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
                                if let Some(kb) = keyboard.as_ref() {
                                    if let Ok(version) = kb.get_version() {
                                        let current_usb = version.raw;
                                        println!(
                                            "\nCurrent device USB version: 0x{current_usb:04X}"
                                        );

                                        if let Some(server_usb) = response.versions.usb {
                                            if server_usb > current_usb {
                                                println!("UPDATE AVAILABLE: 0x{current_usb:04X} -> 0x{server_usb:04X}");
                                            } else {
                                                println!("Firmware is up to date.");
                                            }
                                        }
                                    }
                                } else if let Ok(kb) = SyncKeyboard::open_any() {
                                    if let Ok(version) = kb.get_version() {
                                        let current_usb = version.raw;
                                        println!(
                                            "\nCurrent device USB version: 0x{current_usb:04X}"
                                        );

                                        if let Some(server_usb) = response.versions.usb {
                                            if server_usb > current_usb {
                                                println!("UPDATE AVAILABLE: 0x{current_usb:04X} -> 0x{server_usb:04X}");
                                            } else {
                                                println!("Firmware is up to date.");
                                            }
                                        }
                                    }
                                }
                            }
                            Err(ApiError::ServerError(500, _)) => {
                                // 500 "Record not found" means device ID not in server database
                                // This is normal - the official app also shows "up to date" in this case
                                println!(
                                    "\nDevice ID {api_device_id} not found in server database."
                                );
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
                        use iot_driver::firmware_api::{
                            check_firmware, device_ids, download_firmware,
                        };
                        let output = output.clone();

                        // Try to get device ID from connected device or argument
                        let api_device_id = device_id.or_else(|| {
                            if let Ok(kb) = SyncKeyboard::open_any() {
                                kb.get_device_id()
                                    .ok()
                                    .filter(|&id| id != 0)
                                    .or_else(|| device_ids::from_vid_pid(kb.vid(), kb.pid()))
                            } else {
                                None
                            }
                        });

                        let api_device_id = match api_device_id {
                            Some(id) => id,
                            None => {
                                eprintln!(
                                    "Could not determine device ID. Use --device-id to specify."
                                );
                                eprintln!("Known device IDs:");
                                eprintln!("  M1 V5 HE: {}", device_ids::M1_V5_HE);
                                return Ok(());
                            }
                        };

                        println!("Getting firmware info for device ID {api_device_id}...");

                        match check_firmware(api_device_id).await {
                            Ok(response) => {
                                if let Some(path) = response.versions.download_path {
                                    println!("Downloading from: {path}");
                                    match download_firmware(&path, &output).await {
                                        Ok(size) => {
                                            println!(
                                                "Downloaded {} bytes to {}",
                                                size,
                                                output.display()
                                            );
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

        // === Debug Commands ===
        Some(Commands::TestTransport) => {
            cli_test_transport()?;
        }

        // === Utility Commands ===
        Some(Commands::List) => {
            let hidapi = HidApi::new()?;
            cli_list(&hidapi);
        }
        Some(Commands::Raw { cmd: cmd_str }) => {
            let cmd_byte = u8::from_str_radix(&cmd_str, 16)?;
            cli_raw_command(cmd_byte)?;
        }
        Some(Commands::Serve) => {
            // Fall through to server mode below
            run_server().await?;
        }
        Some(Commands::Tui) => {
            iot_driver::tui::run().await?;
        }
        Some(Commands::Monitor) => {
            watch_settings_changes().await?;
        }
        Some(Commands::Pcap {
            file,
            format,
            filter,
        }) => {
            use iot_driver::pcap_analyzer::{self, OutputFormat};
            let output_format = match format {
                cli::PcapOutputFormat::Text => OutputFormat::Text,
                cli::PcapOutputFormat::Json => OutputFormat::Json,
            };
            pcap_analyzer::run_pcap_analysis(&file, output_format, filter.as_deref())?;
        }
        Some(Commands::Joystick { config, headless }) => {
            // Launch the joystick mapper
            // We can either spawn it as a subprocess or call into the library directly
            // For now, spawn the separate binary
            let mut cmd = std::process::Command::new("monsgeek-joystick");
            if let Some(config_path) = config {
                cmd.arg("--config").arg(config_path);
            }
            if headless {
                cmd.arg("--headless");
            }
            let status = cmd.status();
            match status {
                Ok(s) if s.success() => {}
                Ok(s) => {
                    eprintln!("Joystick mapper exited with status: {}", s);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    eprintln!(
                        "monsgeek-joystick binary not found. Run: cargo build -p monsgeek-joystick"
                    );
                }
                Err(e) => {
                    eprintln!("Failed to run joystick mapper: {}", e);
                }
            }
        }
    }

    Ok(())
}

/// Snapshot of all queryable keyboard settings
#[derive(Debug, Clone)]
struct SettingsSnapshot {
    profile: Option<u8>,
    debounce: Option<u8>,
    polling_rate: Option<u16>,
    led_mode: Option<u8>,
    led_brightness: Option<u8>,
    led_speed: Option<u8>,
    led_color: Option<(u8, u8, u8)>,
    fn_layer: Option<u8>,
    wasd_swap: Option<bool>,
    anti_mistouch: Option<bool>,
    sleep_idle_bt: Option<u16>,
    sleep_idle_24g: Option<u16>,
    sleep_deep_bt: Option<u16>,
    sleep_deep_24g: Option<u16>,
}

impl SettingsSnapshot {
    fn new() -> Self {
        Self {
            profile: None,
            debounce: None,
            polling_rate: None,
            led_mode: None,
            led_brightness: None,
            led_speed: None,
            led_color: None,
            fn_layer: None,
            wasd_swap: None,
            anti_mistouch: None,
            sleep_idle_bt: None,
            sleep_idle_24g: None,
            sleep_deep_bt: None,
            sleep_deep_24g: None,
        }
    }

    fn diff(&self, other: &Self) -> Vec<String> {
        let mut changes = Vec::new();

        if self.profile != other.profile {
            changes.push(format!(
                "Profile: {:?} -> {:?}",
                self.profile, other.profile
            ));
        }
        if self.debounce != other.debounce {
            changes.push(format!(
                "Debounce: {:?}ms -> {:?}ms",
                self.debounce, other.debounce
            ));
        }
        if self.polling_rate != other.polling_rate {
            changes.push(format!(
                "Polling Rate: {:?}Hz -> {:?}Hz",
                self.polling_rate, other.polling_rate
            ));
        }
        if self.led_mode != other.led_mode {
            changes.push(format!(
                "LED Mode: {:?} -> {:?}",
                self.led_mode, other.led_mode
            ));
        }
        if self.led_brightness != other.led_brightness {
            changes.push(format!(
                "LED Brightness: {:?} -> {:?}",
                self.led_brightness, other.led_brightness
            ));
        }
        if self.led_speed != other.led_speed {
            changes.push(format!(
                "LED Speed: {:?} -> {:?}",
                self.led_speed, other.led_speed
            ));
        }
        if self.led_color != other.led_color {
            changes.push(format!(
                "LED Color: {:?} -> {:?}",
                self.led_color, other.led_color
            ));
        }
        if self.fn_layer != other.fn_layer {
            changes.push(format!(
                "Fn Layer: {:?} -> {:?}",
                self.fn_layer, other.fn_layer
            ));
        }
        if self.wasd_swap != other.wasd_swap {
            changes.push(format!(
                "WASD Swap: {:?} -> {:?}",
                self.wasd_swap, other.wasd_swap
            ));
        }
        if self.anti_mistouch != other.anti_mistouch {
            changes.push(format!(
                "Anti-Mistouch: {:?} -> {:?}",
                self.anti_mistouch, other.anti_mistouch
            ));
        }
        if self.sleep_idle_bt != other.sleep_idle_bt {
            changes.push(format!(
                "Sleep Idle BT: {:?}s -> {:?}s",
                self.sleep_idle_bt, other.sleep_idle_bt
            ));
        }
        if self.sleep_idle_24g != other.sleep_idle_24g {
            changes.push(format!(
                "Sleep Idle 2.4G: {:?}s -> {:?}s",
                self.sleep_idle_24g, other.sleep_idle_24g
            ));
        }
        if self.sleep_deep_bt != other.sleep_deep_bt {
            changes.push(format!(
                "Sleep Deep BT: {:?}s -> {:?}s",
                self.sleep_deep_bt, other.sleep_deep_bt
            ));
        }
        if self.sleep_deep_24g != other.sleep_deep_24g {
            changes.push(format!(
                "Sleep Deep 2.4G: {:?}s -> {:?}s",
                self.sleep_deep_24g, other.sleep_deep_24g
            ));
        }

        changes
    }
}

async fn query_all_settings(keyboard: &monsgeek_keyboard::KeyboardInterface) -> SettingsSnapshot {
    let mut snap = SettingsSnapshot::new();

    // Query each setting, ignoring errors
    if let Ok(p) = keyboard.get_profile().await {
        snap.profile = Some(p);
    }
    if let Ok(d) = keyboard.get_debounce().await {
        snap.debounce = Some(d);
    }
    if let Ok(r) = keyboard.get_polling_rate().await {
        snap.polling_rate = Some(r as u16);
    }
    if let Ok(led) = keyboard.get_led_params().await {
        snap.led_mode = Some(led.mode as u8);
        snap.led_brightness = Some(led.brightness);
        snap.led_speed = Some(led.speed);
        snap.led_color = Some((led.color.r, led.color.g, led.color.b));
    }
    if let Ok(opts) = keyboard.get_kb_options().await {
        snap.fn_layer = Some(opts.fn_layer);
        snap.wasd_swap = Some(opts.wasd_swap);
        snap.anti_mistouch = Some(opts.anti_mistouch);
    }
    if let Ok(sleep) = keyboard.get_sleep_time().await {
        snap.sleep_idle_bt = Some(sleep.idle_bt);
        snap.sleep_idle_24g = Some(sleep.idle_24g);
        snap.sleep_deep_bt = Some(sleep.deep_bt);
        snap.sleep_deep_24g = Some(sleep.deep_24g);
    }

    snap
}

async fn watch_settings_changes() -> Result<(), Box<dyn std::error::Error>> {
    use monsgeek_keyboard::KeyboardInterface;
    use monsgeek_transport::{DeviceDiscovery, HidDiscovery, VendorEvent};

    println!("=== Settings Change Watcher ===");
    println!("Press Fn key combinations on the keyboard to see what changes.\n");

    // Open keyboard
    let discovery = HidDiscovery::new();
    let devices = discovery.list_devices().await?;
    if devices.is_empty() {
        return Err("No supported device found".into());
    }

    let transport = discovery.open_device(&devices[0]).await?;
    let info = transport.device_info().clone();
    println!(
        "Connected to: {} (VID:{:04x} PID:{:04x})",
        info.product_name.as_deref().unwrap_or("Unknown"),
        info.vid,
        info.pid
    );

    let keyboard = KeyboardInterface::new(transport, 98, true);

    // Subscribe to events
    let mut event_rx = keyboard
        .subscribe_events()
        .ok_or("No event channel available")?;

    // Query initial settings
    println!("\nQuerying initial settings...");
    let mut current = query_all_settings(&keyboard).await;
    println!("Initial snapshot captured.\n");
    println!("Waiting for settings changes (Ctrl+C to exit)...\n");

    loop {
        // Wait for an event
        match event_rx.recv().await {
            Ok(event) => {
                match &event {
                    VendorEvent::SettingsAck { started: true } => {
                        println!("[Event] Settings change started...");
                    }
                    VendorEvent::SettingsAck { started: false } => {
                        println!("[Event] Settings change complete, querying...");

                        // Small delay to let keyboard settle
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                        // Query new settings
                        let new_snap = query_all_settings(&keyboard).await;

                        // Show diff
                        let changes = current.diff(&new_snap);
                        if changes.is_empty() {
                            println!("  No queryable settings changed (may be a toggle state)");
                        } else {
                            println!("  Changes detected:");
                            for change in &changes {
                                println!("    - {}", change);
                            }
                        }
                        println!();

                        current = new_snap;
                    }
                    VendorEvent::ProfileChange { profile } => {
                        println!(
                            "[Event] Profile changed to {} (via Fn+F9..F12)",
                            profile + 1
                        );
                        // Re-query since profile change affects many settings
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        let new_snap = query_all_settings(&keyboard).await;
                        let changes = current.diff(&new_snap);
                        if !changes.is_empty() {
                            println!("  Settings now:");
                            for change in &changes {
                                println!("    - {}", change);
                            }
                        }
                        println!();
                        current = new_snap;
                    }
                    VendorEvent::BrightnessLevel { level } => {
                        println!("[Event] Brightness: {}/4", level);
                    }
                    VendorEvent::LedEffectMode { effect_id } => {
                        println!("[Event] LED Effect: {}", effect_id);
                    }
                    VendorEvent::LedEffectSpeed { speed } => {
                        println!("[Event] LED Speed: {}/4", speed);
                    }
                    VendorEvent::LedColor { color } => {
                        println!("[Event] LED Color index: {}", color);
                    }
                    VendorEvent::WasdSwapToggle { swapped } => {
                        println!(
                            "[Event] WASD/Arrows: {}",
                            if *swapped { "SWAPPED" } else { "normal" }
                        );
                        // Query to confirm
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        let new_snap = query_all_settings(&keyboard).await;
                        let changes = current.diff(&new_snap);
                        if !changes.is_empty() {
                            for change in &changes {
                                println!("    - {}", change);
                            }
                        }
                        println!();
                        current = new_snap;
                    }
                    VendorEvent::WinLockToggle { locked } => {
                        println!(
                            "[Event] Win key: {}",
                            if *locked { "LOCKED" } else { "unlocked" }
                        );
                    }
                    VendorEvent::BacklightToggle => {
                        println!("[Event] Backlight toggled");
                    }
                    VendorEvent::DialModeToggle => {
                        println!("[Event] Dial mode toggled");
                    }
                    VendorEvent::FnLayerToggle { layer } => {
                        println!("[Event] Fn layer: {} (via Fn+Alt)", layer);
                    }
                    VendorEvent::Wake => {
                        println!("[Event] Keyboard wake");
                    }
                    VendorEvent::MagnetismStart | VendorEvent::MagnetismStop => {
                        // Ignore magnetism events
                    }
                    VendorEvent::KeyDepth { .. } => {
                        // Ignore key depth events
                    }
                    VendorEvent::BatteryStatus {
                        level,
                        charging,
                        online,
                    } => {
                        println!(
                            "[Event] Battery: {}% {} {}",
                            level,
                            if *charging { "(charging)" } else { "" },
                            if *online { "online" } else { "offline" }
                        );
                    }
                    VendorEvent::UnknownKbFunc { category, action } => {
                        println!(
                            "[Event] Unknown KB func: cat={} action={}",
                            category, action
                        );
                        // Query to see what changed
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        let new_snap = query_all_settings(&keyboard).await;
                        let changes = current.diff(&new_snap);
                        if !changes.is_empty() {
                            println!("  Changes detected:");
                            for change in &changes {
                                println!("    - {}", change);
                            }
                        } else {
                            println!("  No queryable settings changed");
                        }
                        println!();
                        current = new_snap;
                    }
                    VendorEvent::Unknown(data) => {
                        println!("[Event] Unknown: {:02X?}", &data[..data.len().min(16)]);
                        // Query to see what changed
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        let new_snap = query_all_settings(&keyboard).await;
                        let changes = current.diff(&new_snap);
                        if !changes.is_empty() {
                            println!("  Changes detected:");
                            for change in &changes {
                                println!("    - {}", change);
                            }
                        }
                        println!();
                        current = new_snap;
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                println!("[Warning] Missed {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                println!("Event channel closed");
                break;
            }
        }
    }

    Ok(())
}

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    // Server mode
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("iot_driver=debug".parse().unwrap()),
        )
        .init();

    let addr = "127.0.0.1:3814".parse()?;

    info!("Starting IOT Driver Linux on {}", addr);
    println!("addr :: {addr}");
    println!("SSSSSSSSSSSTTTTTTTTTTTTTTTTTAAAAAAAAAAAARRRRRRRRRRRTTTTTTTTTTT!!!!!!!");

    let service = DriverService::new().map_err(|e| format!("Failed to initialize HID API: {e}"))?;

    // Start hot-plug monitoring for device connect/disconnect
    service.start_hotplug_monitor();

    // Scan for devices on startup
    let devices = service.scan_devices().await;
    info!("Found {} devices on startup", devices.len());
    for dev in &devices {
        if let Some(dj_dev::OneofDev::Dev(d)) = &dev.oneof_dev {
            info!(
                "  - VID={:04x} PID={:04x} ID={} path={}",
                d.vid, d.pid, d.id, d.path
            );
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
        .accept_http1(true) // Required for gRPC-Web
        .tcp_nodelay(true) // Disable Nagle's algorithm for lower latency
        .initial_stream_window_size(4096) // Smaller buffer for faster flushing
        .initial_connection_window_size(4096) // Smaller connection buffer
        .layer(cors)
        .add_service(grpc_service)
        .serve(addr)
        .await?;

    Ok(())
}
