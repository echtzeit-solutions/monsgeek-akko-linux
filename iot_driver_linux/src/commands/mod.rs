//! Command handlers for the CLI application.
//!
//! This module organizes command handlers by category:
//! - `query`: Read-only commands (info, profile, led, debounce, etc.)
//! - `set`: Setting commands (set-profile, set-debounce, etc.)
//! - `triggers`: Trigger-related commands (calibrate, triggers, set-actuation, etc.)
//! - `keymap`: Key remapping commands (remap, reset-key, swap, keymatrix)
//! - `macros`: Macro commands (macro, set-macro, clear-macro)
//! - `animations`: Animation commands (gif, gif-stream, mode, modes)
//! - `reactive`: Reactive mode commands (audio, audio-test, audio-levels, screen)
//! - `debug`: Debug commands (depth, test-transport)
//! - `firmware`: Firmware subcommands
//! - `utility`: Utility commands (list, raw, serve, tui, joystick)

pub mod animations;
pub mod debug;
pub mod firmware;
pub mod keymap;
pub mod macros;
pub mod query;
pub mod reactive;
pub mod set;
pub mod triggers;
pub mod utility;

use iot_driver::protocol::{self, cmd};
use monsgeek_keyboard::settings::FirmwareVersion;
use monsgeek_transport::{FlowControlTransport, PacketFilter, PrinterConfig, SyncTransport};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Result type for command handlers
pub type CommandResult = Result<(), Box<dyn std::error::Error>>;

/// Open a keyboard and run a closure with it.
/// Prints an error and returns Ok(()) if no device is found.
pub fn with_keyboard<F>(f: F) -> CommandResult
where
    F: FnOnce(&monsgeek_keyboard::SyncKeyboard) -> CommandResult,
{
    match monsgeek_keyboard::SyncKeyboard::open_any() {
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
) -> Result<SyncTransport, Box<dyn std::error::Error>> {
    use monsgeek_transport::{DeviceDiscovery, HidDiscovery};

    // Use futures block_on for sync context
    let transport: Arc<dyn monsgeek_transport::Transport> = futures::executor::block_on(async {
        // Create discovery with printer config - wrapping happens automatically in open_device()
        let discovery = match printer_config {
            Some(config) => HidDiscovery::with_printer_config(config),
            None => HidDiscovery::new(),
        };
        let devices = discovery.list_devices().await?;

        if devices.is_empty() {
            return Err(monsgeek_transport::TransportError::DeviceNotFound(
                "No supported device found".into(),
            ));
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

        discovery.open_device(preferred).await
    })?;

    let flow = Arc::new(FlowControlTransport::new(transport));
    Ok(SyncTransport::new(flow))
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
