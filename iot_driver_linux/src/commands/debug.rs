//! Debug command handlers.

use super::{open_preferred_transport, setup_interrupt_handler, with_keyboard, CommandResult};
use iot_driver::protocol::cmd;
use monsgeek_keyboard::SyncKeyboard;
use monsgeek_transport::protocol::cmd as transport_cmd;
use monsgeek_transport::{list_devices_sync, ChecksumType, PrinterConfig};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Instant;

/// Test the new transport abstraction layer
pub fn test_transport(printer_config: Option<PrinterConfig>) -> CommandResult {
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

    // Open first device (with optional monitoring)
    println!("\nOpening first device...");
    let transport = open_preferred_transport(printer_config.clone())?;
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
    with_keyboard(printer_config, |keyboard| {
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
                    let first_travel =
                        u16::from_le_bytes([triggers.press_travel[0], triggers.press_travel[1]]);
                    println!(
                        "  First key travel (u16): {} ({:.2}mm at 0.01mm precision)",
                        first_travel,
                        first_travel as f32 / 100.0
                    );
                }
            }
            Err(e) => println!("  Error: {e}"),
        }
        Ok(())
    })?;

    println!("\nTransport layer test PASSED!");
    Ok(())
}

/// Monitor real-time key depth (magnetism) from keyboard
pub fn depth(
    keyboard: &SyncKeyboard,
    show_raw: bool,
    show_zero: bool,
    verbose: bool,
) -> CommandResult {
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

    let running = setup_interrupt_handler();

    let mut report_count = 0u64;
    let start = Instant::now();
    let mut last_print = Instant::now();

    // Track latest depth per key for batched display
    let mut key_depths: HashMap<u8, (u16, f32)> = HashMap::new();

    while running.load(Ordering::SeqCst) {
        let mut batch_count = 0u32;

        // Batch read via transport abstraction
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
        let now = Instant::now();
        if now.duration_since(last_print).as_millis() >= 16 && !key_depths.is_empty() {
            // Clear line and print all active keys
            print!("\r\x1b[K");

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
    Ok(())
}
