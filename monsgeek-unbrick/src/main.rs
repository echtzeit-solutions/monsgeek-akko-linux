mod dfuse;
mod driver;
mod firmware;
mod flash_map;
mod winusb;

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Stock v407 firmware, embedded at compile time.
const FIRMWARE_V407: &[u8] =
    include_bytes!("../../firmwares/2949-v407/firmware_reconstructed.bin");

fn main() {
    if let Err(e) = run() {
        eprintln!("\nError: {e:#}");
        wait_for_enter();
        std::process::exit(1);
    }
    wait_for_enter();
}

fn run() -> Result<()> {
    println!("MonsGeek Keyboard Recovery Tool v0.1.0");
    println!("======================================\n");

    let dev = try_open_device()?;

    // Read and display chip ID
    let id_data = dev.read_data(flash_map::FIRMWARE_START, 32)?;
    let id_str: String = id_data
        .iter()
        .take_while(|&&b| b >= 0x20 && b < 0x7F)
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
    println!("  1) Factory reset (erase config only — keeps current firmware)");
    println!("  2) Flash stock firmware v407 + factory reset");
    println!("  3) Flash a custom firmware file");
    println!("  4) Read device info");
    println!();

    let choice = prompt("Choice [1-4]")?;

    match choice.trim() {
        "1" => cmd_factory_reset(&dev)?,
        "2" => cmd_flash_stock(&dev)?,
        "3" => cmd_flash_custom(&dev)?,
        "4" => cmd_info(&dev, &id_data)?,
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
                println!("\nDriver installed successfully.");
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
    println!("\nThis will erase the config area (keymaps, macros, LED settings).");
    println!("Firmware itself will NOT be touched.");
    if !confirm("Proceed?")? {
        println!("Aborted.");
        return Ok(());
    }

    println!("Erasing config area at 0x{:08X}...", flash_map::CONFIG_START);
    dev.write_data(flash_map::CONFIG_START, flash_map::CONFIG_ERASE_IMAGE)?;

    println!("\nDone! Unplug device, disconnect BOOT0, replug.");
    Ok(())
}

fn cmd_flash_stock(dev: &dfuse::DfuSeDevice) -> Result<()> {
    println!(
        "\nThis will flash stock firmware v407 ({} bytes) and erase config.",
        FIRMWARE_V407.len()
    );
    if !confirm("Proceed?")? {
        println!("Aborted.");
        return Ok(());
    }

    println!(
        "Flashing firmware to 0x{:08X} ({} bytes)...",
        flash_map::FIRMWARE_START,
        FIRMWARE_V407.len()
    );
    dev.write_data(flash_map::FIRMWARE_START, FIRMWARE_V407)?;

    println!("Erasing config area...");
    dev.write_data(flash_map::CONFIG_START, flash_map::CONFIG_ERASE_IMAGE)?;

    println!("\nDone! Unplug device, disconnect BOOT0, replug.");
    Ok(())
}

fn cmd_flash_custom(dev: &dfuse::DfuSeDevice) -> Result<()> {
    let path_str = prompt("Path to firmware file")?;
    let path = PathBuf::from(path_str.trim());

    let images = firmware::load_firmware(&path, None)
        .with_context(|| format!("Failed to load {}", path.display()))?;

    println!("\nFirmware: {}", path.display());
    for (i, img) in images.iter().enumerate() {
        println!(
            "  segment {}: 0x{:08X}..0x{:08X} ({} bytes)",
            i,
            img.address,
            img.address + img.data.len() as u32,
            img.data.len()
        );
    }

    if !confirm("\nFlash these segments?")? {
        println!("Aborted.");
        return Ok(());
    }

    for (i, img) in images.iter().enumerate() {
        println!(
            "Flashing segment {} (0x{:08X}, {} bytes)...",
            i,
            img.address,
            img.data.len()
        );
        dev.write_data(img.address, &img.data)?;
    }

    println!("\nDone! Unplug device, disconnect BOOT0, replug.");
    Ok(())
}

fn cmd_info(_dev: &dfuse::DfuSeDevice, id_data: &[u8]) -> Result<()> {
    let id_str: String = id_data
        .iter()
        .take_while(|&&b| b >= 0x20 && b < 0x7F)
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

    // Read firmware version area (just after chip ID header, first 512 bytes)
    println!("\nFirst 32 bytes at 0x{:08X}:", flash_map::FIRMWARE_START);
    for (i, chunk) in id_data.chunks(16).enumerate() {
        print!("  {:08X}: ", flash_map::FIRMWARE_START + (i as u32) * 16);
        for b in chunk {
            print!("{b:02X} ");
        }
        println!();
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
