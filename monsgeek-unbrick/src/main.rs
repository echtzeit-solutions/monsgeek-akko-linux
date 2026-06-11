mod dfuse;
mod driver;
mod firmware;
mod flash_map;
mod upload;
mod winusb;

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Stock v407 firmware code (device 2949), embedded at compile time.
const FIRMWARE_V407: &[u8] =
    include_bytes!("../../firmwares/2949-v407/firmware_reconstructed.bin");

/// Stock v408 firmware code (device 2949), embedded at compile time.
const FIRMWARE_V408: &[u8] =
    include_bytes!("../../firmwares/2949-v408/firmware_2949_v408.bin");

/// Full 256KB flash dump from a working M1 V5 TMR running v402 (device 2679).
/// Contains firmware, config, keymaps, and calibration data.
const FLASH_V402: &[u8] = include_bytes!("../../firmwares/2679-v402/flash_256k.bin");

fn main() {
    // Catch panics so the elevated console window stays open
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("\nPanic: {info}");
        eprintln!("{msg}");
        let _ = append_log(&msg);
        eprint!("\nPress Enter to exit...");
        let _ = std::io::stdin().read_line(&mut String::new());
    }));

    if let Err(e) = run() {
        let msg = format!("Error: {e:#}");
        eprintln!("\n{msg}");
        let _ = append_log(&msg);
        wait_for_enter();
        std::process::exit(1);
    }
    wait_for_enter();
}

/// Append a message to %TEMP%\monsgeek-unbrick.log so it survives window close.
fn append_log(msg: &str) -> std::io::Result<()> {
    use std::io::Write;
    let path = std::env::temp_dir().join("monsgeek-unbrick.log");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{msg}")?;
    Ok(())
}

fn run() -> Result<()> {
    println!("MonsGeek Keyboard Recovery Tool v0.6.0");
    println!("======================================\n");

    // Hidden dev-only self-test: exercises WinHTTP + BCrypt upload of the
    // embedded 256KB image without needing a keyboard attached.
    #[cfg(debug_assertions)]
    if std::env::args().any(|a| a == "--selftest-upload") {
        return selftest_upload();
    }

    let dev = try_open_device()?;

    // Read and display chip ID
    let id_data = dev.read_data(flash_map::FIRMWARE_START, 32)?;
    let id_str: String = id_data
        .iter()
        .take_while(|&&b| (0x20..0x7F).contains(&b))
        .map(|&b| b as char)
        .collect();

    if id_data.starts_with(flash_map::CHIP_ID_KEYBOARD) {
        println!("Found: MonsGeek Keyboard ({})\n", id_str);
    } else if id_data.starts_with(flash_map::CHIP_ID_DONGLE) {
        println!("Found: MonsGeek Dongle ({})\n", id_str);
    } else {
        println!("Found: Unknown device (chip ID: \"{}\")\n", id_str);
    }

    println!("What would you like to do?");
    println!("  1) Factory reset (erase settings, keymaps, macros — keeps firmware + calibration)");
    println!("  2) Flash stock firmware v407 (device 2949) + reference calibration");
    println!("  3) Flash stock firmware v408 (device 2949) + reference calibration");
    println!("  4) Flash stock firmware v402 (device 2679) — full image with calibration");
    println!("  5) Deep reset (factory reset + erase calibration data — requires recalibration)");
    println!("  6) Flash a custom firmware file");
    println!("  7) FULL RECOVERY — restore bootloader + firmware v402 (for corrupted bootloader)");
    println!("  8) Flash a custom file INCLUDING bootloader");
    println!("  9) Read device info");
    println!("  0) Dump flash to file (for diagnosis)");
    println!("  U) Upload current firmware to developers for analysis (helps support your board)");
    println!();

    let choice = prompt("Choice [0-9 or U]")?;

    match choice.trim().to_ascii_lowercase().as_str() {
        "1" => cmd_factory_reset(&dev)?,
        "2" => cmd_flash_stock(&dev, "v407", FIRMWARE_V407, &id_data)?,
        "3" => cmd_flash_stock(&dev, "v408", FIRMWARE_V408, &id_data)?,
        "4" => cmd_flash_stock_v402(&dev, &id_data)?,
        "5" => cmd_deep_reset(&dev)?,
        "6" => cmd_flash_custom(&dev, false, &id_data)?,
        "7" => cmd_full_recovery(&dev, &id_data)?,
        "8" => cmd_flash_custom(&dev, true, &id_data)?,
        "9" => cmd_info(&dev, &id_data)?,
        "0" => cmd_dump(&dev)?,
        "u" => cmd_upload(&dev, &id_data)?,
        _ => println!("Invalid choice."),
    }

    Ok(())
}

fn try_open_device() -> Result<dfuse::DfuSeDevice> {
    print!("[Checking for DFU device...] ");
    match dfuse::DfuSeDevice::open() {
        Ok(dev) => {
            println!("OK");
            Ok(dev)
        }
        Err(first_err) => {
            println!("not found.\n");
            println!("The DFU device was not found. This usually means the WinUSB driver");
            println!("is not installed. Attempting automatic driver installation...\n");

            if let Err(e) = driver::install_winusb_driver() {
                eprintln!("Driver install failed: {e:#}");
                eprintln!("You may need to install the driver manually (e.g. with Zadig).");
            } else {
                println!("\nDriver installed. Waiting for Windows to bind it...");

                // Give Windows time to load the driver and register the interface
                for i in 0..10 {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    print!("\r[Waiting... {}/10s] ", i + 1);
                    if let Ok(dev) = dfuse::DfuSeDevice::open() {
                        println!("found!");
                        return Ok(dev);
                    }
                }
                println!("not yet.");
            }

            println!("\nUnplug and replug the device, then press Enter...");
            let _ = read_line();

            print!("[Retrying...] ");
            match dfuse::DfuSeDevice::open() {
                Ok(dev) => {
                    println!("OK");
                    Ok(dev)
                }
                Err(_) => {
                    println!("still not found.");
                    Err(first_err).context(
                        "Could not open DFU device. Make sure:\n\
                         - The keyboard is in DFU mode (BOOT0 bridged to 3.3V)\n\
                         - The USB cable is connected\n\
                         - The WinUSB driver is installed (try Zadig if auto-install failed)",
                    )
                }
            }
        }
    }
}

fn cmd_factory_reset(dev: &dfuse::DfuSeDevice) -> Result<()> {
    println!("\nThis will erase ALL user data (config, keymaps, FN layers, macros, userpics).");
    println!("Firmware and calibration data will NOT be touched.");
    println!(
        "Erase region: 0x{:08X}–0x{:08X} ({}KB)",
        flash_map::CONFIG_START,
        flash_map::USER_DATA_END,
        flash_map::USER_DATA_SIZE / 1024
    );
    if !confirm("Proceed?")? {
        println!("Aborted.");
        return Ok(());
    }

    println!("Erasing user data...");
    dev.write_data(flash_map::CONFIG_START, flash_map::USER_DATA_ERASE)?;

    println!("\nDone! Unplug device, disconnect BOOT0, replug.");
    println!("The keyboard will regenerate default keymaps on first boot.");
    Ok(())
}

fn cmd_flash_stock(
    dev: &dfuse::DfuSeDevice,
    version: &str,
    firmware: &[u8],
    id_data: &[u8],
) -> Result<()> {
    offer_pre_overwrite_upload(dev, id_data)?;
    println!(
        "\nThis will flash stock firmware {version} (device 2949), erase user data,"
    );
    println!("and write reference calibration from a known-good M1 V5 TMR board.");
    println!("  Firmware:    {} bytes", firmware.len());
    println!("  Calibration: from reference board (may need recalibration for best results)");
    if !confirm("Proceed?")? {
        println!("Aborted.");
        return Ok(());
    }

    println!(
        "Flashing firmware to 0x{:08X} ({} bytes)...",
        flash_map::FIRMWARE_START,
        firmware.len()
    );
    dev.write_data(flash_map::FIRMWARE_START, firmware)?;

    println!("Erasing user data (config, keymaps, macros)...");
    dev.write_data(flash_map::CONFIG_START, flash_map::USER_DATA_ERASE)?;

    // Write reference calibration from v402 dump
    let cal_offset = (flash_map::CALIBRATION_START - flash_map::BOOTLOADER_START) as usize;
    let cal_end = (flash_map::CALIBRATION_END - flash_map::BOOTLOADER_START) as usize;
    let cal_data = &FLASH_V402[cal_offset..cal_end];
    println!(
        "Writing reference calibration to 0x{:08X} ({}KB)...",
        flash_map::CALIBRATION_START,
        flash_map::CALIBRATION_SIZE / 1024,
    );
    dev.write_data(flash_map::CALIBRATION_START, cal_data)?;

    println!("\nDone! Unplug device, disconnect BOOT0, replug.");
    println!("The keyboard will regenerate default keymaps on first boot.");
    println!("For best results, recalibrate in the MonsGeek app afterward.");
    Ok(())
}

fn cmd_flash_stock_v402(dev: &dfuse::DfuSeDevice, id_data: &[u8]) -> Result<()> {
    offer_pre_overwrite_upload(dev, id_data)?;
    let boot_size = (flash_map::FIRMWARE_START - flash_map::BOOTLOADER_START) as usize;
    let writable = &FLASH_V402[boot_size..];

    // Find last non-0xFF byte to trim trailing erased pages
    let last_used = writable
        .iter()
        .rposition(|&b| b != 0xFF)
        .map(|i| i + 1)
        .unwrap_or(0);
    let len = ((last_used as u32).div_ceil(flash_map::FLASH_PAGE_SIZE)
        * flash_map::FLASH_PAGE_SIZE) as usize;
    let data = &writable[..len];

    println!("\nThis will flash a complete v402 image (device 2679) from a known-good");
    println!("M1 V5 TMR board, including firmware, config, keymaps, and calibration.");
    println!(
        "  Write: 0x{:08X}..0x{:08X} ({} bytes = {}KB)",
        flash_map::FIRMWARE_START,
        flash_map::FIRMWARE_START + data.len() as u32,
        data.len(),
        data.len() / 1024,
    );
    if !confirm("Proceed?")? {
        println!("Aborted.");
        return Ok(());
    }

    println!(
        "Flashing {} bytes to 0x{:08X}...",
        data.len(),
        flash_map::FIRMWARE_START,
    );
    dev.write_data(flash_map::FIRMWARE_START, data)?;

    println!("\nDone! Unplug device, disconnect BOOT0, replug.");
    println!("For best results, recalibrate in the MonsGeek app afterward.");
    Ok(())
}

fn cmd_deep_reset(dev: &dfuse::DfuSeDevice) -> Result<()> {
    println!("\nThis will erase ALL user data AND calibration data.");
    println!("You will need to recalibrate the keyboard through the Monsgeek app afterward.");
    println!(
        "Erase regions:\n  User data:   0x{:08X}–0x{:08X} ({}KB)\n  Calibration: 0x{:08X}–0x{:08X} ({}KB)",
        flash_map::CONFIG_START,
        flash_map::USER_DATA_END,
        flash_map::USER_DATA_SIZE / 1024,
        flash_map::CALIBRATION_START,
        flash_map::CALIBRATION_END,
        flash_map::CALIBRATION_SIZE / 1024,
    );
    if !confirm("Proceed?")? {
        println!("Aborted.");
        return Ok(());
    }

    println!("Erasing user data...");
    dev.write_data(flash_map::CONFIG_START, flash_map::USER_DATA_ERASE)?;

    println!("Erasing calibration data...");
    dev.write_data(flash_map::CALIBRATION_START, flash_map::CALIBRATION_ERASE)?;

    println!("\nDone! Unplug device, disconnect BOOT0, replug.");
    println!("IMPORTANT: You must run calibration in the Monsgeek app before keys will work.");
    Ok(())
}

fn cmd_full_recovery(dev: &dfuse::DfuSeDevice, id_data: &[u8]) -> Result<()> {
    offer_pre_overwrite_upload(dev, id_data)?;
    println!("\nFULL RECOVERY — restores bootloader + firmware + calibration from a");
    println!("known-good M1 V5 HE board (v402, device 2679).");
    println!();
    println!("  Bootloader:  0x{:08X}–0x{:08X} (20KB)", flash_map::BOOTLOADER_START, flash_map::BOOTLOADER_END);
    println!("  Firmware:    0x{:08X}+ (from embedded v402 image)", flash_map::FIRMWARE_START);
    println!();
    println!("  WARNING: This overwrites the bootloader! Only use if your bootloader");
    println!("  is corrupted and the board only works in ROM DFU mode (BOOT0).");
    println!("  The bootloader image is from a different board and may differ from your");
    println!("  original, but uses the same chip and flash layout.");

    if !confirm("\nProceed with full recovery?")? {
        println!("Aborted.");
        return Ok(());
    }

    // Write bootloader (first 20KB)
    let boot_size = (flash_map::FIRMWARE_START - flash_map::BOOTLOADER_START) as usize;
    let boot_data = &FLASH_V402[..boot_size];
    println!(
        "\nRestoring bootloader to 0x{:08X} ({} bytes)...",
        flash_map::BOOTLOADER_START,
        boot_data.len(),
    );
    dev.write_data_force(flash_map::BOOTLOADER_START, boot_data)?;

    // Write firmware + data (everything after bootloader, trimmed)
    let writable = &FLASH_V402[boot_size..];
    let last_used = writable.iter().rposition(|&b| b != 0xFF).map(|i| i + 1).unwrap_or(0);
    let len = ((last_used as u32).div_ceil(flash_map::FLASH_PAGE_SIZE)
        * flash_map::FLASH_PAGE_SIZE) as usize;
    let fw_data = &writable[..len];

    println!(
        "Flashing firmware + data to 0x{:08X} ({} bytes = {}KB)...",
        flash_map::FIRMWARE_START,
        fw_data.len(),
        fw_data.len() / 1024,
    );
    dev.write_data(flash_map::FIRMWARE_START, fw_data)?;

    println!("\nDone! Unplug device, disconnect BOOT0, replug.");
    println!("The keyboard should boot normally with v402 firmware.");
    println!("You can then update to the latest firmware via the MonsGeek app.");
    println!("For best results, recalibrate afterward.");
    Ok(())
}

fn cmd_flash_custom(
    dev: &dfuse::DfuSeDevice,
    include_bootloader: bool,
    id_data: &[u8],
) -> Result<()> {
    offer_pre_overwrite_upload(dev, id_data)?;
    let path_str = prompt("Path to firmware file")?;
    let path = PathBuf::from(path_str.trim());

    let images = firmware::load_firmware(&path, None, include_bootloader)
        .with_context(|| format!("Failed to load {}", path.display()))?;

    let has_bootloader_seg = images.iter().any(|img| img.address < flash_map::FIRMWARE_START);

    println!("\nFirmware: {}", path.display());
    for (i, img) in images.iter().enumerate() {
        let is_bl = img.address < flash_map::FIRMWARE_START;
        println!(
            "  segment {}: 0x{:08X}..0x{:08X} ({} bytes){}",
            i,
            img.address,
            img.address + img.data.len() as u32,
            img.data.len(),
            if is_bl { " [BOOTLOADER]" } else { "" },
        );
    }

    if has_bootloader_seg {
        println!("\n  WARNING: This will overwrite the bootloader region!");
        println!("  Only proceed if you have a known-good 256KB flash dump");
        println!("  from the same board model. A bad bootloader = permanent brick.");
    }

    if !confirm("\nFlash these segments?")? {
        println!("Aborted.");
        return Ok(());
    }

    for (i, img) in images.iter().enumerate() {
        let is_bootloader = img.address < flash_map::FIRMWARE_START;
        println!(
            "Flashing segment {} (0x{:08X}, {} bytes){}...",
            i,
            img.address,
            img.data.len(),
            if is_bootloader { " [BOOTLOADER]" } else { "" },
        );
        if is_bootloader {
            dev.write_data_force(img.address, &img.data)?;
        } else {
            dev.write_data(img.address, &img.data)?;
        }
    }

    println!("\nDone! Unplug device, disconnect BOOT0, replug.");
    Ok(())
}

fn cmd_info(dev: &dfuse::DfuSeDevice, id_data: &[u8]) -> Result<()> {
    let id_str: String = id_data
        .iter()
        .take_while(|&&b| (0x20..0x7F).contains(&b))
        .map(|&b| b as char)
        .collect();

    println!();
    if id_data.starts_with(flash_map::CHIP_ID_KEYBOARD) {
        println!("Device: MonsGeek Keyboard (AT32F405 8KMKB)");
    } else if id_data.starts_with(flash_map::CHIP_ID_DONGLE) {
        println!("Device: MonsGeek Dongle (AT32F405 8K-DGKB)");
    } else {
        println!("Device: Unknown");
    }
    println!("Chip ID: \"{}\"", id_str);

    print!("Raw: ");
    for b in &id_data[..id_data.len().min(32)] {
        print!("{b:02X} ");
    }
    println!();

    // Chip ID header
    println!("\nFirst 32 bytes at 0x{:08X}:", flash_map::FIRMWARE_START);
    for (i, chunk) in id_data.chunks(16).enumerate() {
        print!("  {:08X}: ", flash_map::FIRMWARE_START + (i as u32) * 16);
        for b in chunk {
            print!("{b:02X} ");
        }
        println!();
    }

    // Config header status
    println!("\nConfig region (0x{:08X}):", flash_map::CONFIG_START);
    match dev.read_data(flash_map::CONFIG_START, 32) {
        Ok(cfg) => {
            let all_ff = cfg.iter().all(|&b| b == 0xFF);
            let all_zero = cfg.iter().all(|&b| b == 0x00);
            print!("  ");
            for b in &cfg[..cfg.len().min(32)] {
                print!("{b:02X} ");
            }
            println!();
            if all_ff {
                println!("  Status: ERASED (factory defaults will be used)");
            } else if all_zero {
                println!("  Status: ALL ZEROS (possibly corrupt)");
            } else {
                println!("  Status: has data");
            }
        }
        Err(e) => println!("  Read failed: {e}"),
    }

    // Calibration data status
    println!("\nCalibration data (0x{:08X}):", flash_map::CALIBRATION_START);
    match dev.read_data(flash_map::CALIBRATION_START, 64) {
        Ok(cal) => {
            let all_ff = cal.iter().all(|&b| b == 0xFF);
            let all_zero = cal.iter().all(|&b| b == 0x00);
            print!("  ");
            for b in &cal[..cal.len().min(32)] {
                print!("{b:02X} ");
            }
            println!();
            if all_ff {
                println!("  Status: ERASED (no calibration — keys will NOT work until calibrated)");
            } else if all_zero {
                println!("  Status: ALL ZEROS (possibly corrupt)");
            } else {
                println!("  Status: has data (calibrated)");
            }
        }
        Err(e) => println!("  Read failed: {e}"),
    }

    // Keymap data status
    println!("\nKeymap data (0x{:08X}):", flash_map::CONFIG_START + 0x800);
    match dev.read_data(flash_map::CONFIG_START + 0x800, 32) {
        Ok(km) => {
            let all_ff = km.iter().all(|&b| b == 0xFF);
            print!("  ");
            for b in &km[..km.len().min(32)] {
                print!("{b:02X} ");
            }
            println!();
            if all_ff {
                println!("  Status: ERASED (firmware will use default keymaps)");
            } else {
                println!("  Status: has data");
            }
        }
        Err(e) => println!("  Read failed: {e}"),
    }

    Ok(())
}

fn cmd_dump(dev: &dfuse::DfuSeDevice) -> Result<()> {
    println!("\nThis will read the full flash (256KB) and save it to a file.");
    println!("The dump can be shared for diagnosis — it does NOT contain personal data,");
    println!("only firmware code, keymaps, and calibration values.\n");

    // Default filename next to the exe, or current dir
    let default_name = "flash_dump.bin";
    let path_str = prompt(&format!("Output file [{default_name}]"))?;
    let path_str = path_str.trim();
    let path = if path_str.is_empty() {
        PathBuf::from(default_name)
    } else {
        PathBuf::from(path_str)
    };

    let data = read_full_flash(dev)?;

    std::fs::write(&path, &data)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    println!("\nSaved to: {}", path.display());
    println!("You can share this file for diagnosis.");

    Ok(())
}

/// Read the full flash (256KB) in 2KB chunks with progress. Shared by the
/// dump-to-file and upload commands.
fn read_full_flash(dev: &dfuse::DfuSeDevice) -> Result<Vec<u8>> {
    let total = (flash_map::FLASH_END - flash_map::BOOTLOADER_START) as usize;
    println!(
        "Reading 0x{:08X}–0x{:08X} ({total} bytes = {}KB)...",
        flash_map::BOOTLOADER_START,
        flash_map::FLASH_END,
        total / 1024
    );

    let chunk_size = 2048usize;
    let total_chunks = total.div_ceil(chunk_size);
    let mut data = Vec::with_capacity(total);

    for i in 0..total_chunks {
        let addr = flash_map::BOOTLOADER_START + (i as u32) * chunk_size as u32;
        let remaining = total - data.len();
        let this_size = remaining.min(chunk_size);
        print!(
            "\r  reading {}/{}KB...",
            (i * chunk_size) / 1024,
            total / 1024
        );
        std::io::Write::flush(&mut std::io::stdout()).ok();
        let chunk = dev.read_data(addr, this_size)?;
        data.extend_from_slice(&chunk);
    }
    println!(
        "\r  read {} bytes ({} KB).                    ",
        data.len(),
        data.len() / 1024
    );
    Ok(data)
}

/// Offer to upload the original firmware before a destructive flash overwrites
/// it. One y/N; declining proceeds straight to the flash. Its own failures are
/// swallowed inside `cmd_upload`, so this never blocks recovery.
fn offer_pre_overwrite_upload(dev: &dfuse::DfuSeDevice, id_data: &[u8]) -> Result<()> {
    println!("\nThis option OVERWRITES your current firmware.");
    if confirm("Upload your ORIGINAL firmware to the developers first? (optional)")? {
        cmd_upload(dev, id_data)?;
    }
    Ok(())
}

/// Read the full flash and upload it for variant analysis. Shows an explicit
/// consent screen first (the dump includes custom macros/keymaps).
fn cmd_upload(dev: &dfuse::DfuSeDevice, id_data: &[u8]) -> Result<()> {
    println!("\nUpload your keyboard's full 256KB flash to the developers for analysis.");
    println!("This helps catalog board variants (e.g. 2679 vs 2949) and improve recovery.\n");
    println!("  WHAT IS SENT: the COMPLETE flash image, which INCLUDES your custom");
    println!("  keymaps and any MACROS you recorded (these may contain text you typed),");
    println!("  plus the chip ID string, dump size, and a SHA-256 checksum.");
    println!("  Nothing is sent unless you type 'y' below.\n");
    if !confirm("Upload full flash dump now?")? {
        println!("Skipped — nothing was uploaded.");
        return Ok(());
    }

    let data = read_full_flash(dev)?;
    let chip_id: String = id_data
        .iter()
        .take_while(|&&b| (0x20..0x7F).contains(&b))
        .map(|&b| b as char)
        .collect();

    println!("Uploading {} bytes...", data.len());
    match upload::upload_dump(chip_id.trim(), &data) {
        Ok(200) | Ok(201) | Ok(409) => {
            println!("\nUpload complete. Thank you — this helps us support your board!");
        }
        Ok(code) => {
            println!(
                "\nServer responded with HTTP {code}; the upload may not have been stored. \
                 Continuing."
            );
        }
        Err(e) => {
            println!("\nUpload failed ({e}). This does NOT affect recovery — continuing.");
            let _ = append_log(&format!("upload failed: {e:#}"));
        }
    }
    Ok(())
}

/// Dev-only self-test: upload the embedded 256KB image (a known 2679 dump)
/// without a keyboard. Exercises WinHTTP + BCrypt against `$MONSGEEK_UPLOAD_URL`.
#[cfg(debug_assertions)]
fn selftest_upload() -> Result<()> {
    println!(
        "[selftest] uploading embedded image ({} bytes)...",
        FLASH_V402.len()
    );
    match upload::upload_dump("AT32F405 8KMKB", FLASH_V402) {
        Ok(code) => println!("[selftest] HTTP {code}"),
        Err(e) => println!("[selftest] upload error: {e:#}"),
    }
    Ok(())
}

fn confirm(prompt_text: &str) -> Result<bool> {
    eprint!("{prompt_text} [y/N] ");
    let input = read_line()?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

fn prompt(prompt_text: &str) -> Result<String> {
    eprint!("{prompt_text}: ");
    read_line()
}

fn read_line() -> Result<String> {
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input)
}

fn wait_for_enter() {
    eprint!("\nPress Enter to exit...");
    let _ = read_line();
}
