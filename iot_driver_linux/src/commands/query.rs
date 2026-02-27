//! Query (read-only) command handlers.

use super::{format_command_response, open_preferred_transport, CommandResult};
use hidapi::HidApi;
use iot_driver::hal;
use iot_driver::protocol::{self, cmd};
use monsgeek_keyboard::SleepTimeSettings;
use monsgeek_transport::protocol::cmd as transport_cmd;
use monsgeek_transport::{ChecksumType, PrinterConfig};
use std::time::Duration;

/// Get device info (firmware version, device ID, patch, boot mode, API ID)
pub fn info(printer_config: Option<PrinterConfig>) -> CommandResult {
    let transport = open_preferred_transport(printer_config)?;
    let dev = transport.device_info();

    // Device identity
    if let Some(ref name) = dev.product_name {
        println!("Device:    {name}");
    }
    println!(
        "  VID/PID:  {:04X}:{:04X}  type={:?}",
        dev.vid, dev.pid, dev.transport_type
    );

    let resp = transport.query_command(transport_cmd::GET_USB_VERSION, &[], ChecksumType::Bit7)?;
    let device_id = u32::from_le_bytes([resp[1], resp[2], resp[3], resp[4]]);
    let version = u16::from_le_bytes([resp[7], resp[8]]);

    // Version is stored as major.minor in high/low byte (e.g. 0x0407 = v4.07)
    let (major, minor) = (version >> 8, version & 0xFF);
    println!("Firmware:  v{major}.{minor:02} (raw 0x{version:04X}, dec {version})");
    println!("Device ID: {device_id} (0x{device_id:08X})");

    // Boot mode (bootloader / firmware update mode)
    let is_boot = iot_driver::protocol::firmware_update::is_boot_mode(dev.vid, dev.pid);
    println!("Boot mode: {}", if is_boot { "Yes" } else { "No" });

    // API ID (for firmware server; same as device ID or VID/PID fallback)
    let api_id = if device_id != 0 {
        Some(device_id)
    } else {
        iot_driver::firmware_api::device_ids::from_vid_pid(dev.vid, dev.pid)
    };
    if let Some(id) = api_id {
        println!("API ID:    {id}");
    }

    // Patched firmware (battery HID, LED stream, etc.)
    // Probe 0xE7: wired HID returns [cmd_echo, magic_hi, magic_lo, ...]; some paths may return [magic_hi, magic_lo, ...]
    match transport.query_raw(protocol::patch_info::CMD, &[], ChecksumType::Bit7) {
        Ok(resp) => {
            let offsets = if resp.len() >= 6
                && resp[0] == protocol::patch_info::MAGIC_HI
                && resp[1] == protocol::patch_info::MAGIC_LO
            {
                Some((2, 3, 5)) // payload only
            } else if resp.len() >= 7
                && resp[1] == protocol::patch_info::MAGIC_HI
                && resp[2] == protocol::patch_info::MAGIC_LO
            {
                Some((3, 4, 6)) // echo + payload (wired strips report ID only)
            } else {
                None
            };
            match offsets {
                Some((ver_off, caps_off, name_off)) => {
                    let patch_ver = resp[ver_off];
                    let caps = u16::from_le_bytes([resp[caps_off], resp[caps_off + 1]]);
                    let name_end = resp.len().min(name_off + 9);
                    let name_bytes = &resp[name_off..name_end];
                    let name_len = name_bytes
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(name_bytes.len());
                    let name = String::from_utf8_lossy(&name_bytes[..name_len]);
                    let cap_names = protocol::patch_info::capability_names(caps);
                    if cap_names.is_empty() {
                        println!("Patch:     {} v{} (no features enabled).", name, patch_ver);
                    } else {
                        println!(
                            "Patch:     {} v{} [{}]",
                            name,
                            patch_ver,
                            cap_names.join(", ")
                        );
                    }
                }
                None => {
                    println!("Patch:     Stock firmware (no patch support).");
                    // Show raw 0xE7 response to investigate: patched FW returns 0xCA 0xFE magic;
                    // stock may echo the command and return other data.
                    let hex_len = resp.len().min(16);
                    println!(
                        "           0xE7 response ({} bytes): {}",
                        resp.len(),
                        resp[..hex_len]
                            .iter()
                            .map(|b| format!("{:02x}", b))
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                }
            }
        }
        Err(_) => {
            println!("Patch:     Stock firmware (no patch support).");
            println!("           0xE7 response: none (timeout or error).");
        }
    }

    Ok(())
}

/// Get current profile
pub fn profile(printer_config: Option<PrinterConfig>) -> CommandResult {
    let transport = open_preferred_transport(printer_config)?;
    let resp = transport.query_command(transport_cmd::GET_PROFILE, &[], ChecksumType::Bit7)?;
    println!("Profile: {}", resp[1]);
    Ok(())
}

/// Get LED settings
pub fn led(printer_config: Option<PrinterConfig>) -> CommandResult {
    let transport = open_preferred_transport(printer_config)?;
    let resp = transport.query_command(transport_cmd::GET_LEDPARAM, &[], ChecksumType::Bit7)?;
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
    Ok(())
}

/// Get debounce time
pub fn debounce(printer_config: Option<PrinterConfig>) -> CommandResult {
    let transport = open_preferred_transport(printer_config)?;
    let resp = transport.query_command(transport_cmd::GET_DEBOUNCE, &[], ChecksumType::Bit7)?;
    println!("Debounce: {} ms", resp[1]);
    Ok(())
}

/// Get polling rate
pub fn rate(keyboard: &monsgeek_keyboard::SyncKeyboard) -> CommandResult {
    use iot_driver::protocol::polling_rate;

    match keyboard.get_polling_rate() {
        Ok(rate) => {
            let hz = rate as u16;
            println!("Polling rate: {hz} ({})", polling_rate::name(hz));
        }
        Err(e) => eprintln!("Failed to get polling rate: {e}"),
    }
    Ok(())
}

/// Get keyboard options
pub fn options(printer_config: Option<PrinterConfig>) -> CommandResult {
    let transport = open_preferred_transport(printer_config)?;
    let resp = transport.query_command(transport_cmd::GET_KBOPTION, &[], ChecksumType::Bit7)?;
    println!("Options (raw): {:02X?}", &resp[..16.min(resp.len())]);
    Ok(())
}

/// Get supported features
pub fn features(printer_config: Option<PrinterConfig>) -> CommandResult {
    let transport = open_preferred_transport(printer_config)?;
    let resp = transport.query_command(transport_cmd::GET_FEATURE_LIST, &[], ChecksumType::Bit7)?;
    println!("Features (raw): {:02X?}", &resp[..24.min(resp.len())]);
    Ok(())
}

/// Get sleep time settings
pub fn sleep(keyboard: &monsgeek_keyboard::SyncKeyboard) -> CommandResult {
    match keyboard.get_sleep_time() {
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
    }
    Ok(())
}

/// Show all device information
pub fn all(printer_config: Option<PrinterConfig>) -> CommandResult {
    println!("MonsGeek M1 V5 HE - Device Information");
    println!("======================================\n");

    let transport = open_preferred_transport(printer_config)?;
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

/// Get battery status from 2.4GHz dongle
///
/// Checks kernel power_supply first (when eBPF filter loaded), falls back to vendor protocol.
pub fn battery(
    hidapi: &HidApi,
    quiet: bool,
    show_hex: bool,
    watch: Option<Option<u64>>,
    force_vendor: bool,
) -> CommandResult {
    use iot_driver::power_supply::{find_dongle_battery_power_supply, read_kernel_battery};

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
            Some((battery_level, online, idle, raw_bytes)) => {
                if quiet {
                    println!("{battery_level}");
                } else if show_hex {
                    print_hex_dump(&raw_bytes);
                } else {
                    println!("Battery Status (vendor)");
                    println!("-----------------------");
                    println!("  Level:     {battery_level}%");
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
            protocol::build_command(cmd::GET_DONGLE_STATUS, &[], protocol::ChecksumType::Bit7);
        if let Err(e) = device.send_feature_report(&f7_cmd) {
            if show_debug {
                eprintln!("F7 send failed: {e:?}");
            }
        } else if show_debug {
            eprintln!("F7 sent OK, not waiting");
        }

        // Get Feature report with Report ID 5
        let mut buf = [0u8; 65];
        buf[0] = 0x05;

        match device.get_feature_report(&mut buf) {
            Ok(_len) => {
                let battery_level = buf[1];
                let idle = buf[3] != 0;
                let online = buf[4] != 0;

                return Some((battery_level, online, idle, buf));
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
    let secs = now.as_secs() % 86400;
    let hours = (secs / 3600) % 24;
    let mins = (secs % 3600) / 60;
    let sec = secs % 60;
    let millis = now.subsec_millis();
    println!(
        "[{hours:02}:{mins:02}:{sec:02}.{millis:03}] Full vendor response ({} bytes):",
        data.len()
    );

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
