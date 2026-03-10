//! Command handlers for the CLI application.
//!
//! This module organizes command handlers by category:
//! - `query`: Read-only commands (info, profile, led, debounce, etc.)
//! - `set`: Setting commands (set-profile, set-debounce, etc.)
//! - `triggers`: Trigger-related commands (calibrate, triggers, set-actuation, etc.)
//! - `keymap`: Key remapping commands (remap, reset-key, swap, keymatrix)
//! - `macros`: Macro commands (macro, set-macro, clear-macro)
//! - `animations`: Animation commands (mode, modes)
//! - `userpic`: Userpic upload/download (mode 13 flash slots)
//! - `reactive`: Reactive mode commands (audio, audio-test, audio-levels, screen)
//! - `debug`: Debug commands (depth, test-transport)
//! - `firmware`: Firmware subcommands
//! - `utility`: Utility commands (list, raw, serve, tui, joystick)

pub mod animations;
pub mod debug;
pub mod dongle;
pub mod effect;
pub mod firmware;
pub mod keymap;
pub mod led_stream;
pub mod macros;
#[cfg(feature = "notify")]
pub mod notify;
pub mod query;
pub mod reactive;
pub mod set;
pub mod triggers;
pub mod userpic;
pub mod utility;

use iot_driver::protocol::{self, cmd};
use monsgeek_keyboard::settings::FirmwareVersion;
use monsgeek_transport::{
    DeviceDiscovery, FlowControlTransport, HidDiscovery, PacketFilter, PrinterConfig, Transport,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Result type for command handlers
pub type CommandResult = Result<(), Box<dyn std::error::Error>>;

/// Open a keyboard, optionally with transport monitoring (--monitor).
/// When printer_config is Some, the transport is wrapped so send/receive is printed.
/// Query firmware device ID from a transport (GET_USB_VERSION bytes 1-4).
/// Returns None if the device doesn't respond or the response is malformed.
pub(crate) fn query_device_id(flow: &FlowControlTransport) -> Option<i32> {
    flow.query_command(
        protocol::cmd::GET_USB_VERSION,
        &[],
        monsgeek_transport::ChecksumType::Bit7,
    )
    .ok()
    .filter(|r| r.len() >= 5 && r[0] == protocol::cmd::GET_USB_VERSION)
    .map(|r| u32::from_le_bytes([r[1], r[2], r[3], r[4]]) as i32)
}

pub fn open_keyboard(
    printer_config: Option<PrinterConfig>,
) -> Result<monsgeek_keyboard::KeyboardInterface, Box<dyn std::error::Error>> {
    let flow = match &printer_config {
        Some(config) => {
            let discovery = HidDiscovery::with_printer_config(config.clone());
            let devices = discovery.list_devices()?;
            if devices.is_empty() {
                return Err(monsgeek_transport::TransportError::DeviceNotFound(
                    "No supported device found".into(),
                )
                .into());
            }
            let preferred = devices
                .iter()
                .find(|d| d.info.transport_type == monsgeek_transport::TransportType::Bluetooth)
                .or_else(|| {
                    devices.iter().find(|d| {
                        d.info.transport_type == monsgeek_transport::TransportType::HidDongle
                    })
                })
                .unwrap_or(&devices[0]);
            let transport = discovery.open_device(preferred)?;
            Arc::new(FlowControlTransport::new(transport))
        }
        None => open_preferred_transport(None)?,
    };

    let info = flow.device_info();
    let (vid, pid) = (info.vid, info.pid);
    let device_id = query_device_id(&flow);
    let mut key_count = iot_driver::devices::key_count_with_id(device_id, vid, pid);
    let has_magnetism = iot_driver::devices::has_magnetism_with_id(device_id, vid, pid);
    let device_info = iot_driver::devices::get_device_info_with_id(device_id, vid, pid);
    let protocol = monsgeek_transport::protocol::ProtocolFamily::detect(
        device_info.as_ref().map(|d| d.name.as_str()),
        pid,
    );

    let registry = iot_driver::profile_registry();

    // Try matrix database for key names and matrix size (covers 390+ devices).
    // This is the generic path — no hardcoded profile needed.
    let matrix_db = device_id.and_then(|id| registry.get_device_matrix(id));
    if let Some(matrix) = matrix_db {
        let matrix_size = matrix.matrix_size() as u8;
        if key_count == 0 || (key_count < matrix_size && matrix_size > 0) {
            key_count = matrix_size;
        }
    }

    let mut kb =
        monsgeek_keyboard::KeyboardInterface::new(flow, key_count, has_magnetism, protocol);

    // Resolve key names: prefer builtin profile, fall back to matrix database.
    let profile = device_id
        .and_then(|id| registry.find_by_id(id as u32))
        .or_else(|| registry.find_by_vid_pid(vid, pid));
    if let Some(p) = profile {
        let names: Vec<String> = (0..p.matrix_size())
            .map(|i| p.matrix_key_name(i as u8).to_string())
            .collect();
        kb.set_matrix_key_names(names);
    } else if let Some(matrix) = matrix_db {
        let size = matrix.matrix_size();
        let names: Vec<String> = (0..size)
            .map(|i| matrix.key_name(i).unwrap_or("").to_string())
            .collect();
        kb.set_matrix_key_names(names);
    }

    // Set non-analog positions from matrix database (encoder/GPIO keys).
    if let Some(matrix) = matrix_db {
        if let Some(positions) = &matrix.non_analog_positions {
            kb.set_non_analog_positions(positions.clone());
        }
    }

    Ok(kb)
}

/// Open a keyboard and run a closure with it.
/// When printer_config is Some, uses monitoring transport so --monitor shows send/receive.
pub fn with_keyboard<F>(printer_config: Option<PrinterConfig>, f: F) -> CommandResult
where
    F: FnOnce(&monsgeek_keyboard::KeyboardInterface) -> CommandResult,
{
    match open_keyboard(printer_config) {
        Ok(keyboard) => f(&keyboard),
        Err(e) => {
            eprintln!("No device found: {e}");
            Ok(())
        }
    }
}

/// Open a device via the new transport layer, preferring Bluetooth when present.
/// If `printer_config` is Some, the transport is automatically wrapped with Printer for monitoring.
pub fn open_preferred_transport(
    printer_config: Option<PrinterConfig>,
) -> Result<Arc<FlowControlTransport>, Box<dyn std::error::Error>> {
    use monsgeek_transport::{DeviceDiscovery, HidDiscovery};

    let discovery = match printer_config {
        Some(config) => HidDiscovery::with_printer_config(config),
        None => HidDiscovery::new(),
    };
    let devices = discovery.list_devices()?;

    if devices.is_empty() {
        return Err(monsgeek_transport::TransportError::DeviceNotFound(
            "No supported device found".into(),
        )
        .into());
    }

    // Prefer wired USB (direct, most reliable), then Bluetooth, then dongle.
    // Dongle is always present even when keyboard is in wired mode, so it should
    // be the lowest priority to avoid picking it when a direct path exists.
    let preferred = devices
        .iter()
        .find(|d| d.info.transport_type == monsgeek_transport::TransportType::HidWired)
        .or_else(|| {
            devices
                .iter()
                .find(|d| d.info.transport_type == monsgeek_transport::TransportType::Bluetooth)
        })
        .or_else(|| {
            devices
                .iter()
                .find(|d| d.info.transport_type == monsgeek_transport::TransportType::HidDongle)
        })
        .unwrap_or(&devices[0]);

    let transport = discovery.open_device(preferred)?;
    Ok(Arc::new(FlowControlTransport::new(transport)))
}

/// Format and print a command response from the transport layer
/// `resp` is the response data (64 bytes, first byte is command echo)
pub fn format_command_response(cmd_byte: u8, resp: &[u8]) {
    println!("\nResponse (0x{:02x} = {}):", resp[0], cmd::name(resp[0]));

    // Response offsets: resp[0] = cmd echo, resp[1..] = data
    // (Transport layer strips report ID, so indices are shifted -1 from raw HID)
    match cmd_byte {
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
            let precision = FirmwareVersion::precision_byte_str(resp[2]);
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

/// Set up a Ctrl-C handler that sets the given flag to false when triggered.
/// Returns the Arc<AtomicBool> for use in the main loop.
pub fn setup_interrupt_handler() -> Arc<AtomicBool> {
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    ctrlc::set_handler(move || {
        running_clone.store(false, Ordering::SeqCst);
    })
    .ok();

    running
}

/// Create printer config from CLI flags
pub fn create_printer_config(
    monitor: bool,
    hex: bool,
    all_hid: bool,
    filter: Option<&str>,
) -> Result<Option<PrinterConfig>, Box<dyn std::error::Error>> {
    if !monitor {
        return Ok(None);
    }

    let filter = match filter {
        Some(f) => std::str::FromStr::from_str(f)?,
        None => PacketFilter::All,
    };

    Ok(Some(
        PrinterConfig::default()
            .with_hex(hex)
            .with_all_hid(all_hid)
            .with_filter(filter),
    ))
}
