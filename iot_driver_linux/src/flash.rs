//! Firmware flash engine for the RY bootloader protocol.
//!
//! Handles entering bootloader mode, discovering the bootloader device,
//! transferring firmware chunks, and reporting progress via a callback trait.

use std::ffi::CString;
use std::fmt;

use hidapi::HidApi;

use crate::firmware::FirmwareFile;
use crate::protocol::firmware_update;

/// Phases of the flash process (reported to progress callback).
#[derive(Debug, Clone)]
pub enum FlashPhase {
    Scanning,
    BootloaderDetected,
    EnteringBootloader,
    WaitingForBootloader { elapsed_ms: u64, timeout_ms: u64 },
    BootloaderFound,
    StartingTransfer { chunks: usize, size: usize },
    TransferringData,
    CompletingTransfer,
    WaitingForReboot,
}

impl fmt::Display for FlashPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scanning => write!(f, "Scanning for devices"),
            Self::BootloaderDetected => write!(f, "Bootloader device already present"),
            Self::EnteringBootloader => write!(f, "Entering bootloader mode"),
            Self::WaitingForBootloader {
                elapsed_ms,
                timeout_ms,
            } => write!(
                f,
                "Waiting for bootloader ({:.1}s / {:.1}s)",
                *elapsed_ms as f64 / 1000.0,
                *timeout_ms as f64 / 1000.0
            ),
            Self::BootloaderFound => write!(f, "Bootloader device found"),
            Self::StartingTransfer { chunks, size } => {
                write!(f, "Starting transfer: {chunks} chunks, {size} bytes")
            }
            Self::TransferringData => write!(f, "Transferring firmware data"),
            Self::CompletingTransfer => write!(f, "Completing transfer"),
            Self::WaitingForReboot => write!(f, "Waiting for device reboot"),
        }
    }
}

/// Flash errors.
#[derive(Debug)]
pub enum FlashError {
    FirmwareValidation(String),
    DeviceNotFound(String),
    MultipleDevices(Vec<String>),
    BootloaderTimeout,
    TransferFailed(String),
    AckFailed(String),
    HidError(String),
}

impl fmt::Display for FlashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FirmwareValidation(msg) => write!(f, "Firmware validation failed: {msg}"),
            Self::DeviceNotFound(msg) => write!(f, "Device not found: {msg}"),
            Self::MultipleDevices(paths) => {
                writeln!(f, "Multiple devices found, specify --device:")?;
                for p in paths {
                    writeln!(f, "  {p}")?;
                }
                Ok(())
            }
            Self::BootloaderTimeout => write!(f, "Timeout waiting for bootloader device"),
            Self::TransferFailed(msg) => write!(f, "Transfer failed: {msg}"),
            Self::AckFailed(msg) => write!(f, "Ack failed: {msg}"),
            Self::HidError(msg) => write!(f, "HID error: {msg}"),
        }
    }
}

impl std::error::Error for FlashError {}

impl From<hidapi::HidError> for FlashError {
    fn from(e: hidapi::HidError) -> Self {
        Self::HidError(e.to_string())
    }
}

/// Progress callback trait — implement for CLI, TUI, or tests.
pub trait FlashProgress: Send {
    fn on_phase(&mut self, phase: &FlashPhase);
    fn on_chunk(&mut self, sent: usize, total: usize);
    fn on_error(&mut self, error: &FlashError);
    fn on_complete(&mut self);
}

/// Options for the flash operation.
pub struct FlashOptions {
    /// Specific device path to use (None = autodetect).
    pub device_path: Option<String>,
    /// Timeout for bootloader discovery (ms).
    pub bootloader_timeout_ms: u64,
    /// Delay after sending ENTER_BOOTLOADER before scanning (ms).
    pub boot_entry_delay_ms: u64,
}

impl Default for FlashOptions {
    fn default() -> Self {
        Self {
            device_path: None,
            bootloader_timeout_ms: 10_000,
            boot_entry_delay_ms: firmware_update::BOOT_ENTRY_DELAY_MS,
        }
    }
}

/// What we found during device discovery.
enum FlashTarget {
    /// Device in normal mode — must enter bootloader first.
    Normal(CString),
    /// Device already in bootloader mode — skip straight to transfer.
    AlreadyInBootloader(CString),
}

/// Scan HID bus for a single flash target.
fn find_flash_target(api: &HidApi, device_path: Option<&str>) -> Result<FlashTarget, FlashError> {
    let normal: Vec<_> = api
        .device_list()
        .filter(|d| {
            d.vendor_id() == firmware_update::VID
                && d.product_id() == firmware_update::NORMAL_PID
                && d.usage_page() == firmware_update::NORMAL_USAGE_PAGE
                && d.usage() == firmware_update::NORMAL_USAGE
        })
        .collect();

    let boot: Vec<_> = api
        .device_list()
        .filter(|d| {
            d.vendor_id() == firmware_update::VID
                && firmware_update::is_boot_mode(d.vendor_id(), d.product_id())
                && d.usage_page() == firmware_update::BOOT_USAGE_PAGE
        })
        .collect();

    // If user specified a device path, find exactly that one
    if let Some(path) = device_path {
        let path_cstr =
            CString::new(path).map_err(|e| FlashError::DeviceNotFound(e.to_string()))?;

        // Check bootloader devices first
        for d in &boot {
            if d.path() == path_cstr.as_c_str() {
                return Ok(FlashTarget::AlreadyInBootloader(path_cstr));
            }
        }
        // Then normal devices
        for d in &normal {
            if d.path() == path_cstr.as_c_str() {
                return Ok(FlashTarget::Normal(path_cstr));
            }
        }
        return Err(FlashError::DeviceNotFound(format!(
            "No matching device at path: {path}"
        )));
    }

    let total = normal.len() + boot.len();
    if total == 0 {
        return Err(FlashError::DeviceNotFound(
            "No MonsGeek keyboard or bootloader device found".to_string(),
        ));
    }
    if total > 1 {
        let mut paths = Vec::new();
        for d in &boot {
            paths.push(format!(
                "{} (bootloader, {:04x}:{:04x})",
                d.path().to_string_lossy(),
                d.vendor_id(),
                d.product_id(),
            ));
        }
        for d in &normal {
            paths.push(format!(
                "{} (normal, {:04x}:{:04x})",
                d.path().to_string_lossy(),
                d.vendor_id(),
                d.product_id(),
            ));
        }
        return Err(FlashError::MultipleDevices(paths));
    }

    // Exactly one device
    if !boot.is_empty() {
        Ok(FlashTarget::AlreadyInBootloader(boot[0].path().to_owned()))
    } else {
        Ok(FlashTarget::Normal(normal[0].path().to_owned()))
    }
}

/// Poll for the bootloader device to appear after entering bootloader mode.
fn poll_for_bootloader(
    timeout_ms: u64,
    progress: &mut dyn FlashProgress,
) -> Result<CString, FlashError> {
    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(300);

    loop {
        let elapsed_ms = start.elapsed().as_millis() as u64;
        if elapsed_ms > timeout_ms {
            return Err(FlashError::BootloaderTimeout);
        }

        progress.on_phase(&FlashPhase::WaitingForBootloader {
            elapsed_ms,
            timeout_ms,
        });

        // Re-create HidApi to get fresh device list
        if let Ok(api) = HidApi::new() {
            for d in api.device_list() {
                if d.vendor_id() == firmware_update::VID
                    && firmware_update::is_boot_mode(d.vendor_id(), d.product_id())
                    && d.usage_page() == firmware_update::BOOT_USAGE_PAGE
                {
                    // Small extra delay for device to stabilize
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    return Ok(d.path().to_owned());
                }
            }
        }

        std::thread::sleep(poll_interval);
    }
}

/// Send the ENTER_BOOTLOADER command to a normal-mode device.
///
/// The device reboots immediately after receiving this command, so the
/// ioctl may return EIO due to device disconnection. That's expected.
fn send_enter_bootloader(path: &CString) -> Result<(), FlashError> {
    let api = HidApi::new()?;
    let dev = api.open_path(path.as_c_str())?;

    // 65 bytes: report_id(0) + 0x7F + 55AA55AA + padding
    let mut buf = [0u8; 65];
    buf[0] = 0x00; // report ID
    buf[1] = firmware_update::BOOT_ENTRY_USB[0]; // 0x7F
    buf[2] = firmware_update::BOOT_ENTRY_USB[1]; // 0x55
    buf[3] = firmware_update::BOOT_ENTRY_USB[2]; // 0xAA
    buf[4] = firmware_update::BOOT_ENTRY_USB[3]; // 0x55
    buf[5] = firmware_update::BOOT_ENTRY_USB[4]; // 0xAA

    match dev.send_feature_report(&buf) {
        Ok(_) => {}
        Err(_) => {
            // EIO is expected — device reboots immediately after receiving
            // the command, which can race with the ioctl return.
        }
    }

    Ok(())
}

/// Build a 65-byte feature report buffer: [report_id=0x00, ...64 bytes data].
///
/// The bootloader descriptor has no report IDs. Per hidapi convention, we set
/// buf[0] = 0x00 and put the actual 64-byte payload at buf[1..65].
/// On real USB hardware, the kernel strips the 0x00 prefix (HIDIOCSFEATURE
/// skips report_id=0). On UHID, the full buffer is delivered to the dummy.
fn boot_feature_buf(payload: &[u8]) -> [u8; 65] {
    let mut buf = [0u8; 65];
    buf[0] = 0x00; // report ID (none)
    let len = payload.len().min(64);
    buf[1..1 + len].copy_from_slice(&payload[..len]);
    buf
}

/// Execute the firmware transfer on an open bootloader device.
fn do_transfer(
    path: &CString,
    firmware: &FirmwareFile,
    progress: &mut dyn FlashProgress,
) -> Result<(), FlashError> {
    let api = HidApi::new()?;
    let dev = api.open_path(path.as_c_str())?;

    let data = &firmware.data;
    let size = data.len() as u32;
    let chunk_count = firmware.chunk_count as u16;
    let checksum = firmware.checksum;

    // 1. Send FW_TRANSFER_START
    progress.on_phase(&FlashPhase::StartingTransfer {
        chunks: chunk_count as usize,
        size: data.len(),
    });

    let start_header = firmware_update::build_start_header(chunk_count, size);
    let mut start_payload = [0u8; 64];
    start_payload[..start_header.len()].copy_from_slice(&start_header);

    dev.send_feature_report(&boot_feature_buf(&start_payload))
        .map_err(|e| FlashError::TransferFailed(format!("FW_TRANSFER_START failed: {e}")))?;

    // 2. Read ack
    let mut ack_buf = [0u8; 65];
    ack_buf[0] = 0x00; // request report ID 0
    dev.get_feature_report(&mut ack_buf)
        .map_err(|e| FlashError::AckFailed(format!("Start ack failed: {e}")))?;

    // 3. Send firmware chunks
    progress.on_phase(&FlashPhase::TransferringData);

    let total_chunks = chunk_count as usize;
    for i in 0..total_chunks {
        let offset = i * firmware_update::CHUNK_SIZE;
        let end = (offset + firmware_update::CHUNK_SIZE).min(data.len());

        let mut chunk = [0xFFu8; 64]; // pad last chunk with 0xFF
        chunk[..end - offset].copy_from_slice(&data[offset..end]);

        dev.send_feature_report(&boot_feature_buf(&chunk))
            .map_err(|e| {
                FlashError::TransferFailed(format!("Chunk {}/{} failed: {e}", i + 1, total_chunks))
            })?;

        progress.on_chunk(i + 1, total_chunks);
    }

    // 4. Send FW_TRANSFER_COMPLETE
    progress.on_phase(&FlashPhase::CompletingTransfer);

    let complete_header = firmware_update::build_complete_header(chunk_count, checksum, size);
    let mut complete_payload = [0u8; 64];
    let copy_len = complete_header.len().min(64);
    complete_payload[..copy_len].copy_from_slice(&complete_header[..copy_len]);

    dev.send_feature_report(&boot_feature_buf(&complete_payload))
        .map_err(|e| FlashError::TransferFailed(format!("FW_TRANSFER_COMPLETE failed: {e}")))?;

    // 5. Try to read final ack (may fail if device reboots immediately)
    progress.on_phase(&FlashPhase::WaitingForReboot);

    let mut final_ack = [0u8; 65];
    final_ack[0] = 0x00;
    // May fail if device reboots immediately — that's fine
    let _ = dev.get_feature_report(&mut final_ack);

    progress.on_complete();
    Ok(())
}

/// Flash firmware to a keyboard.
///
/// This is the main entry point. It handles:
/// 1. Device discovery and autodetection
/// 2. Entering bootloader mode (if needed)
/// 3. Firmware transfer
/// 4. Progress reporting
///
/// Runs synchronously (blocking) — call from `spawn_blocking` if needed.
pub fn flash_firmware(
    firmware: &FirmwareFile,
    progress: &mut dyn FlashProgress,
    options: &FlashOptions,
) -> Result<(), FlashError> {
    // Validate firmware first
    firmware
        .validate()
        .map_err(|e| FlashError::FirmwareValidation(e.to_string()))?;

    // Scan for devices
    progress.on_phase(&FlashPhase::Scanning);
    let api = HidApi::new()?;
    let target = find_flash_target(&api, options.device_path.as_deref())?;

    // Drop the HidApi — we'll re-create as needed (required after device re-enumeration)
    drop(api);

    let boot_path = match target {
        FlashTarget::AlreadyInBootloader(path) => {
            progress.on_phase(&FlashPhase::BootloaderDetected);
            path
        }
        FlashTarget::Normal(path) => {
            // Enter bootloader
            progress.on_phase(&FlashPhase::EnteringBootloader);
            send_enter_bootloader(&path)?;

            // Wait for bootloader device to appear
            std::thread::sleep(std::time::Duration::from_millis(
                options.boot_entry_delay_ms,
            ));
            let boot_path = poll_for_bootloader(options.bootloader_timeout_ms, progress)?;

            progress.on_phase(&FlashPhase::BootloaderFound);
            boot_path
        }
    };

    // Transfer firmware
    do_transfer(&boot_path, firmware, progress)?;

    Ok(())
}
