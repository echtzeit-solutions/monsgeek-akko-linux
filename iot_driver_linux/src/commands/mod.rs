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
pub mod memory;
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
use monsgeek_transport::protocol::Profile;
use monsgeek_transport::{
    DeviceDiscovery, FlowControlTransport, HidDiscovery, HidResponse, PacketFilter, PrinterConfig,
    Transport, format_device_list,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Result type for command handlers
pub type CommandResult = Result<(), Box<dyn std::error::Error>>;

/// Command context threaded through all command handlers.
/// Carries printer config (--monitor), device selector (--device) and the
/// profile override (--profile).
#[derive(Clone, Default)]
pub struct CmdCtx {
    pub printer_config: Option<PrinterConfig>,
    pub device: Option<String>,
    /// `--profile`: operate on this profile instead of the board's active one.
    pub profile: Option<Profile>,
}

impl CmdCtx {
    pub fn new(
        printer_config: Option<PrinterConfig>,
        device: Option<String>,
        profile: Option<Profile>,
    ) -> Self {
        Self {
            printer_config,
            device,
            profile,
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
) -> Result<monsgeek_keyboard::RyKeyboard, Box<dyn std::error::Error>> {
    let flow = open_preferred_transport(ctx)?;

    let info = flow.device_info();
    let (vid, pid) = (info.vid, info.pid);
    let device_id = query_device_id(&flow);
    let db_key_count = iot_driver::devices::key_count_with_id(device_id, vid, pid);
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
    let key_count = iot_driver::device_loader::scan_extent(db_key_count, matrix_db);

    let mut kb = monsgeek_keyboard::RyKeyboard::new(flow, key_count, has_magnetism, protocol);

    // Cap settable polling rates at what this model supports.
    if let Some(def) = device_id.and_then(|id| registry.get_device_info_by_id_and_usb(id, vid, pid))
    {
        kb.set_polling_rates(def.polling_rates().to_vec());
    }

    if let Some(names) = registry.resolve_matrix_key_names(device_id, vid, pid) {
        kb.set_matrix_key_names(names);
    }
    if let Some(m) = matrix_db {
        kb.set_matrix_defaults(m.matrix.clone());
    }

    // Keymatrix and Fn operations target the board's active profile unless the
    // caller overrides it, so a read and the write that follows always agree.
    //
    // If the query fails there is no safe default: assuming profile 0 would send
    // every subsequent write to profile 0's storage while the board is running
    // some other profile — the user's edit lands somewhere they are not looking,
    // and the profile they *were* editing is left untouched. Say so instead.
    let profile = match ctx.profile {
        Some(p) => p,
        None => kb.get_profile().map_err(|e| {
            format!("could not determine the active profile ({e}); pass --profile to choose one")
        })?,
    };
    kb.set_active_profile(profile);

    // Set non-analog positions from matrix database (encoder/GPIO keys).
    if let Some(matrix) = matrix_db
        && let Some(positions) = &matrix.non_analog_positions
    {
        kb.set_non_analog_positions(positions.clone());
    }

    Ok(kb)
}

/// Open a keyboard and run a closure with it.
/// Restores the board's profile when dropped, on every exit path.
struct ProfileGuard<'a> {
    keyboard: &'a monsgeek_keyboard::RyKeyboard,
    restore_to: Profile,
}

impl<'a> ProfileGuard<'a> {
    fn new(keyboard: &'a monsgeek_keyboard::RyKeyboard, restore_to: Profile) -> Self {
        *PENDING_PROFILE_RESTORE.lock().unwrap() = Some(restore_to);
        Self {
            keyboard,
            restore_to,
        }
    }
}

impl Drop for ProfileGuard<'_> {
    fn drop(&mut self) {
        // The magnetism table only reaches flash after two `config_save_apply`
        // passes, and switching profiles reloads RAM from flash — restoring too
        // early would discard the write we just made.
        //
        // The pending marker stays set across this sleep on purpose: it is the
        // longest stretch where the board is still on the wrong profile, so a
        // forced exit here is exactly when the user needs to be told.
        std::thread::sleep(std::time::Duration::from_millis(MAGNETISM_FLUSH_MS));
        let result = self.keyboard.set_profile(self.restore_to);
        // Cleared only once the board is actually back, or is beyond helping.
        *PENDING_PROFILE_RESTORE.lock().unwrap() = None;
        match result {
            Ok(()) => {
                self.keyboard.set_active_profile(self.restore_to);
                println!("Restored profile {}.", self.restore_to.number());
            }
            Err(e) => eprintln!(
                "WARNING: could not restore profile {}: {e}\n\
                 The keyboard is still on another profile — run `iot_driver set-profile {}`.",
                self.restore_to.number(),
                self.restore_to.get()
            ),
        }
    }
}

/// Long enough for the firmware's two-pass magnetism flush to reach flash.
/// The per-write settle is 250ms; this is the same order, applied once at the end.
const MAGNETISM_FLUSH_MS: u64 = 400;

/// Like [`with_keyboard`], but for commands whose settings carry no profile on the
/// wire — triggers, deadzones, DKS, Snap-Tap, Mod-Tap, calibration.
///
/// The firmware keys those tables off the *active* profile
/// (`mag_calibration_load_or_init` reads `FLASH_MAGNETISM + profile * 0x1000`), so
/// editing another profile means making it active first. That is visible on the
/// keyboard, so it is announced rather than done quietly, and the original profile
/// is restored through a guard that also runs on error and panic.
pub fn with_keyboard_on_profile<F>(ctx: &CmdCtx, f: F) -> CommandResult
where
    F: FnOnce(&monsgeek_keyboard::RyKeyboard) -> CommandResult,
{
    let keyboard = match open_keyboard(ctx) {
        Ok(kb) => kb,
        Err(e) => {
            eprintln!("No device found: {e}");
            return Ok(());
        }
    };

    let wanted = keyboard.active_profile();
    let current = keyboard.get_profile().unwrap_or(wanted);
    if wanted == current {
        return f(&keyboard);
    }

    println!(
        "Trigger settings are stored per profile but carry no profile in the protocol,\n\
         so profile {} has to be active while it is edited.",
        wanted.number()
    );
    println!("Switching {} -> {} …", current.number(), wanted.number());
    if let Err(e) = keyboard.set_profile(wanted) {
        eprintln!("Failed to switch to profile {}: {e}", wanted.number());
        return Ok(());
    }
    // Installed before the guard exists: with a handler in place Ctrl-C no longer
    // kills the process outright, so the closure unwinds and `Drop` gets to run.
    // Without it the profile switch announced above would simply be left in place.
    setup_interrupt_handler();
    let _guard = ProfileGuard::new(&keyboard, current);
    f(&keyboard)
}

pub fn with_keyboard<F>(ctx: &CmdCtx, f: F) -> CommandResult
where
    F: FnOnce(&monsgeek_keyboard::RyKeyboard) -> CommandResult,
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
            // Byte 2 is speed and byte 3 is brightness, not the other way round —
            // reading them swapped reported a fully lit board as brightness 0.
            let Ok(params) = monsgeek_transport::command::LedParamsResponse::from_data(resp) else {
                println!("  LED:        unparseable response");
                return;
            };
            let (r, g, b) = (params.color.r, params.color.g, params.color.b);
            println!(
                "  LED Mode:   {} ({})",
                params.mode as u8,
                params.mode.name()
            );
            println!("  Brightness: {}/4", params.brightness);
            println!("  Speed:      {}/4", params.speed);
            println!("  Color RGB:  ({r}, {g}, {b}) #{r:02X}{g:02X}{b:02X}");
            if params.dazzle {
                println!("  Dazzle:     ON (rainbow cycle)");
            }
        }
        cmd::GET_KBOPTION => {
            let opts = monsgeek_keyboard::KeyboardOptions::from_bytes(&resp[1..]);
            println!("  OS Mode:    {}", opts.os_mode_name());
            println!("  Fn Layer:   {}", opts.fn_layer);
            println!("  Anti-touch: {}", opts.anti_mistouch);
            println!("  RTStab:     {} ms", opts.rt_stability_ms());
            println!("  WASD Swap:  {}", opts.wasd_swap);
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

/// Profile the keyboard must be put back on, if a command switched it.
///
/// Set while a [`ProfileGuard`] is alive so the Ctrl-C path can name it: a forced
/// exit skips `Drop`, and a keyboard left on the wrong profile looks like the
/// user's settings vanished.
static PENDING_PROFILE_RESTORE: Mutex<Option<Profile>> = Mutex::new(None);

/// The process-wide "keep running" flag, `false` once Ctrl-C has been pressed.
///
/// `ctrlc::set_handler` may only be installed once per process, so every caller
/// shares this one flag. Previously each command installed its own; only the
/// first succeeded, and any later one polled a flag nothing would ever clear.
static INTERRUPT_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// Shared Ctrl-C flag, installing the handler on first use.
///
/// The handler only clears the flag — it does no I/O, since it runs concurrently
/// with whatever the main thread is doing on the USB device. That means the
/// current operation finishes and unwinds normally, which is what lets
/// [`ProfileGuard`] restore the profile. A second Ctrl-C gives up on that and
/// exits, printing the command needed to undo the switch by hand.
pub fn setup_interrupt_handler() -> Arc<AtomicBool> {
    Arc::clone(INTERRUPT_FLAG.get_or_init(|| {
        let running = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&running);
        ctrlc::set_handler(move || {
            if flag.swap(false, Ordering::SeqCst) {
                eprintln!("\nInterrupted — finishing the current operation…");
                return;
            }
            // Second Ctrl-C: the user wants out now.
            if let Some(profile) = PENDING_PROFILE_RESTORE.lock().unwrap().take() {
                eprintln!(
                    "Exiting without restoring the profile.\n\
                     The keyboard is on another profile — run `iot_driver set-profile {}`.",
                    profile.get()
                );
            }
            std::process::exit(130); // 128 + SIGINT
        })
        .ok();
        running
    }))
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
