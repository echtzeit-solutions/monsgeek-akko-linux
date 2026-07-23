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
pub mod probe;
pub mod query;
pub mod reactive;
pub mod set;
pub mod triggers;
pub mod userpic;
pub mod utility;

use iot_driver::protocol::{self, cmd};
use monsgeek_keyboard::settings::FirmwareVersion;
use monsgeek_transport::{
    format_device_list, DeviceDiscovery, FlowControlTransport, HidDiscovery, PacketFilter,
    PrinterConfig, Transport,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Result type for command handlers
pub type CommandResult = Result<(), Box<dyn std::error::Error>>;

/// Command context threaded through all command handlers.
/// Carries printer config (--monitor) and device selector (--device).
#[derive(Clone, Default)]
pub struct CmdCtx {
    pub printer_config: Option<PrinterConfig>,
    pub device: Option<String>,
}

impl CmdCtx {
    pub fn new(printer_config: Option<PrinterConfig>, device: Option<String>) -> Self {
        Self {
            printer_config,
            device,
        }
    }

    pub fn device_selector(&self) -> Option<&str> {
        self.device.as_deref()
    }
}

/// Model name resolver for device labeling.
/// Uses the device database to look up display names.
fn resolve_model_name(device_id: Option<u32>, vid: u16, pid: u16) -> Option<String> {
    iot_driver::devices::get_device_info_with_id(device_id.map(|id| id as i32), vid, pid)
        .map(|info| info.display_name)
}

/// Resolve which device to use based on the --device selector.
///
/// When selector is None:
/// - 0 devices: error
/// - 1 device: use it
/// - Multiple: print numbered list to stderr, return error
///
/// When selector is Some:
/// - Try parse as index (usize)
/// - Try match transport name ("usb", "dongle", "bt")
/// - Otherwise treat as HID path prefix
pub(crate) fn resolve_device(
    discovery: &HidDiscovery,
    selector: Option<&str>,
) -> Result<monsgeek_transport::DiscoveredDevice, Box<dyn std::error::Error>> {
    let labeled = discovery.list_labeled_devices(resolve_model_name)?;

    if labeled.is_empty() {
        return Err(monsgeek_transport::TransportError::DeviceNotFound(
            "No supported device found".into(),
        )
        .into());
    }

    if let Some(sel) = selector {
        // Try parse as index
        if let Ok(idx) = sel.parse::<usize>() {
            let len = labeled.len();
            return labeled
                .into_iter()
                .find(|(_, l)| l.index == idx)
                .map(|(p, _)| p.device)
                .ok_or_else(|| format!("Device index {idx} out of range (0-{})", len - 1).into());
        }

        // Try match transport name
        let transport_matches: Vec<_> = labeled
            .iter()
            .filter(|(_, l)| l.transport_name == sel)
            .collect();
        if transport_matches.len() == 1 {
            return Ok(transport_matches[0].0.device.clone());
        }
        if transport_matches.len() > 1 {
            let labels: Vec<_> = labeled.iter().map(|(_, l)| l.clone()).collect();
            eprintln!("Multiple devices match transport '{sel}':");
            eprint!("{}", format_device_list(&labels));
            return Err(format!(
                "Ambiguous --device '{sel}': {} matches. Use index or HID path.",
                transport_matches.len()
            )
            .into());
        }

        // Try HID path prefix match
        let path_matches: Vec<_> = labeled
            .iter()
            .filter(|(_, l)| l.hid_path.contains(sel))
            .collect();
        if path_matches.len() == 1 {
            return Ok(path_matches[0].0.device.clone());
        }
        if path_matches.len() > 1 {
            let labels: Vec<_> = labeled.iter().map(|(_, l)| l.clone()).collect();
            eprintln!("Multiple devices match path '{sel}':");
            eprint!("{}", format_device_list(&labels));
            return Err(format!(
                "Ambiguous --device '{sel}': {} matches.",
                path_matches.len()
            )
            .into());
        }

        return Err(format!("No device matches '{sel}'").into());
    }

    // No selector: auto-select
    if labeled.len() == 1 {
        return Ok(labeled.into_iter().next().unwrap().0.device);
    }

    // Multiple devices: print list and error
    let labels: Vec<_> = labeled.iter().map(|(_, l)| l.clone()).collect();
    eprintln!("Multiple devices found. Use --device (-D) to select:");
    eprint!("{}", format_device_list(&labels));
    Err("Multiple devices found, use --device to select".into())
}

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

/// Open a keyboard with device selection support.
pub fn open_keyboard(
    ctx: &CmdCtx,
) -> Result<monsgeek_keyboard::KeyboardInterface, Box<dyn std::error::Error>> {
    let flow = open_preferred_transport(ctx)?;

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
    let matrix_db = device_id.and_then(|id| registry.get_device_matrix(vid, pid, id));
    if let Some(matrix) = matrix_db {
        let matrix_size = matrix.matrix_size() as u8;
        if key_count == 0 || (key_count < matrix_size && matrix_size > 0) {
            key_count = matrix_size;
        }
    }

    let mut kb =
        monsgeek_keyboard::KeyboardInterface::new(flow, key_count, has_magnetism, protocol);

    // Cap settable polling rates at what this model supports.
    if let Some(def) = device_id.and_then(|id| registry.get_device_info_by_id_and_usb(id, vid, pid))
    {
        kb.set_polling_rates(def.polling_rates().to_vec());
    }

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
pub fn with_keyboard<F>(ctx: &CmdCtx, f: F) -> CommandResult
where
    F: FnOnce(&monsgeek_keyboard::KeyboardInterface) -> CommandResult,
{
    match open_keyboard(ctx) {
        Ok(keyboard) => f(&keyboard),
        Err(e) => {
            eprintln!("No device found: {e}");
            Ok(())
        }
    }
}

/// Open a device via the transport layer with device selection support.
/// Prefers wired USB > Bluetooth > dongle when no --device is specified and only one device exists.
pub fn open_preferred_transport(
    ctx: &CmdCtx,
) -> Result<Arc<FlowControlTransport>, Box<dyn std::error::Error>> {
    let discovery = match &ctx.printer_config {
        Some(config) => HidDiscovery::with_printer_config(config.clone()),
        None => HidDiscovery::new(),
    };

    let device = resolve_device(&discovery, ctx.device_selector())?;
    let transport = discovery.open_device(&device)?;
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

/// Create printer config from CLI flags.
///
/// The printer is enabled when `--monitor` is set OR a `--record` file is given
/// (recording implies monitoring). When recording, output is written to the
/// file as JSONL and the format is forced to JSON.
pub fn create_printer_config(
    monitor: bool,
    hex: bool,
    all_hid: bool,
    filter: Option<&str>,
    record: Option<&std::path::Path>,
) -> Result<Option<PrinterConfig>, Box<dyn std::error::Error>> {
    if !monitor && record.is_none() {
        return Ok(None);
    }

    let filter = match filter {
        Some(f) => std::str::FromStr::from_str(f)?,
        None => PacketFilter::All,
    };

    let mut config = PrinterConfig::default()
        .with_hex(hex)
        .with_all_hid(all_hid)
        .with_filter(filter);

    if let Some(path) = record {
        config = config.with_output_file(path)?;
    }

    Ok(Some(config))
}
