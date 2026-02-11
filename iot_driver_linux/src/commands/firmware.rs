//! Firmware command handlers.

use super::CommandResult;
use iot_driver::firmware::FirmwareFile;
use monsgeek_keyboard::SyncKeyboard;
use std::path::PathBuf;

/// Show firmware info for connected device
pub fn info() -> CommandResult {
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
            let is_boot =
                iot_driver::protocol::firmware_update::is_boot_mode(keyboard.vid(), keyboard.pid());
            println!("Boot Mode:  {}", if is_boot { "Yes" } else { "No" });

            // API ID is same as device ID, with VID/PID fallback
            let api_id = if device_id != 0 {
                Some(device_id)
            } else {
                iot_driver::firmware_api::device_ids::from_vid_pid(keyboard.vid(), keyboard.pid())
            };
            if let Some(id) = api_id {
                println!("API ID:     {id}");
            }
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}

/// Validate a firmware file
pub fn validate(file: &PathBuf) -> CommandResult {
    println!("Validating firmware file: {}", file.display());

    match FirmwareFile::load(file) {
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
                if let Ok(contents) = FirmwareFile::list_zip_contents(file) {
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
    Ok(())
}

/// Dry-run firmware update (no actual flashing)
pub fn dry_run(file: &PathBuf, verbose: bool) -> CommandResult {
    use iot_driver::firmware::dry_run_usb;

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

    match FirmwareFile::load(file) {
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
    Ok(())
}

/// Check for firmware updates from server
#[cfg(feature = "firmware-api")]
pub fn check(device_id: Option<u32>) -> CommandResult {
    use iot_driver::firmware_api::{
        check_firmware_blocking as check_firmware, device_ids, ApiError,
    };

    // Try to get device ID from connected device or argument
    let (api_device_id, keyboard) = if let Some(id) = device_id {
        (Some(id), None)
    } else {
        match SyncKeyboard::open_any() {
            Ok(kb) => {
                let id = kb.get_device_id().ok().filter(|&id| id != 0);
                let id = id.or_else(|| device_ids::from_vid_pid(kb.vid(), kb.pid()));
                (id, Some(kb))
            }
            Err(_) => (None, None),
        }
    };

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

    match check_firmware(api_device_id) {
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
            let kb = keyboard.or_else(|| SyncKeyboard::open_any().ok());
            if let Some(kb) = kb {
                if let Ok(version) = kb.get_version() {
                    let current_usb = version.raw;
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
        }
        Err(ApiError::ServerError(500, _)) => {
            println!("\nDevice ID {api_device_id} not found in server database.");
            println!("This is normal for some devices. Assuming firmware is up to date.");
        }
        Err(e) => {
            eprintln!("Failed to check firmware: {e}");
        }
    }
    Ok(())
}

#[cfg(not(feature = "firmware-api"))]
pub fn check(_device_id: Option<u32>) -> CommandResult {
    eprintln!("Firmware API not enabled. Rebuild with: cargo build --features firmware-api");
    Ok(())
}

/// Download firmware from server
#[cfg(feature = "firmware-api")]
pub fn download(device_id: Option<u32>, output: &PathBuf) -> CommandResult {
    use iot_driver::firmware_api::{
        check_firmware_blocking as check_firmware, device_ids,
        download_firmware_blocking as download_firmware,
    };

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
            eprintln!("Could not determine device ID. Use --device-id to specify.");
            eprintln!("Known device IDs:");
            eprintln!("  M1 V5 HE: {}", device_ids::M1_V5_HE);
            return Ok(());
        }
    };

    println!("Getting firmware info for device ID {api_device_id}...");

    match check_firmware(api_device_id) {
        Ok(response) => {
            if let Some(path) = response.versions.download_path {
                println!("Downloading from: {path}");
                match download_firmware(&path, output) {
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
    Ok(())
}

#[cfg(not(feature = "firmware-api"))]
pub fn download(_device_id: Option<u32>, _output: &PathBuf) -> CommandResult {
    eprintln!("Firmware API not enabled. Rebuild with: cargo build --features firmware-api");
    Ok(())
}

/// CLI progress reporter for flash operations.
struct CliFlashProgress {
    last_pct: usize,
}

impl CliFlashProgress {
    fn new() -> Self {
        Self {
            last_pct: usize::MAX,
        }
    }
}

impl iot_driver::flash::FlashProgress for CliFlashProgress {
    fn on_phase(&mut self, phase: &iot_driver::flash::FlashPhase) {
        println!("[flash] {phase}");
    }

    fn on_chunk(&mut self, sent: usize, total: usize) {
        let pct = sent * 100 / total;
        // Print every 5% or at completion
        if pct != self.last_pct && (pct.is_multiple_of(5) || sent == total) {
            self.last_pct = pct;
            let bar_len = 40;
            let filled = bar_len * sent / total;
            let bar: String = "=".repeat(filled) + &" ".repeat(bar_len - filled);
            print!("\r  [{bar}] {pct:3}% ({sent}/{total})");
            if sent == total {
                println!();
            }
            use std::io::Write;
            std::io::stdout().flush().ok();
        }
    }

    fn on_error(&mut self, error: &iot_driver::flash::FlashError) {
        eprintln!("[flash] ERROR: {error}");
    }

    fn on_complete(&mut self) {
        println!("[flash] Flash complete! Device should reboot to normal mode.");
    }
}

/// Flash firmware to a connected keyboard.
pub fn flash(file: &PathBuf, device: Option<&str>, yes: bool) -> CommandResult {
    use iot_driver::flash::{flash_firmware, FlashOptions};

    // 1. Load + validate firmware
    let fw = match FirmwareFile::load(file) {
        Ok(fw) => fw,
        Err(e) => {
            eprintln!("Failed to load firmware file: {e}");
            return Ok(());
        }
    };

    if let Err(e) = fw.validate() {
        eprintln!("Firmware validation failed: {e}");
        return Ok(());
    }

    // 2. Print summary + safety warning
    println!("Firmware Flash");
    println!("===============");
    println!("File:       {}", fw.filename);
    println!("Type:       {}", fw.firmware_type);
    println!("Size:       {} bytes ({} KB)", fw.size, fw.size / 1024);
    println!("Checksum:   0x{:08X}", fw.checksum);
    println!("Chunks:     {} (64 bytes each)", fw.chunk_count);
    println!();
    println!("WARNING: This will overwrite the keyboard firmware!");
    println!("The keyboard will be unusable if the process is interrupted.");
    println!("Make sure you have a DFU recovery method available.");

    // 3. Confirmation
    if !yes {
        println!();
        print!("Type 'yes' to continue: ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim() != "yes" {
            println!("Aborted.");
            return Ok(());
        }
    }

    println!();

    // 4. Flash
    let mut progress = CliFlashProgress::new();
    let options = FlashOptions {
        device_path: device.map(String::from),
        ..Default::default()
    };

    match flash_firmware(&fw, &mut progress, &options) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("\nFlash failed: {e}");
        }
    }

    Ok(())
}
