//! Macro command handlers.

use super::CommandResult;
use iot_driver::protocol::hid;
use monsgeek_keyboard::SyncKeyboard;

/// Get macro for a key
pub fn get_macro(key: &str) -> CommandResult {
    let macro_index: u8 = key.parse().unwrap_or(0);
    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            println!("Reading macro {macro_index}...");
            match keyboard.get_macro(macro_index) {
                Ok(data) => {
                    if data.len() >= 2 {
                        let length = u16::from_le_bytes([data[0], data[1]]) as usize;
                        println!("Macro length: {length} bytes");

                        if length > 0 && data.len() > 2 {
                            println!("\nMacro events (2 bytes each: [keycode, flags]):");
                            let events = &data[2..];
                            for (i, chunk) in events.chunks(2).enumerate() {
                                if chunk.len() < 2 || chunk.iter().all(|&b| b == 0) {
                                    break;
                                }
                                let keycode = chunk[0];
                                let flags = chunk[1];

                                let event_type = if flags & 0x80 != 0 { "Down" } else { "Up" };
                                let key_name = hid::key_name(keycode);
                                println!(
                                    "  Event {i:2}: {event_type} {key_name} (0x{keycode:02x}, flags={flags:02x})"
                                );
                            }
                        } else {
                            println!("Macro is empty");
                        }
                    } else {
                        println!("Invalid macro data");
                    }

                    println!("\nRaw data ({} bytes):", data.len().min(64));
                    for chunk in data.chunks(16).take(4) {
                        for b in chunk {
                            print!("{b:02x} ");
                        }
                        println!();
                    }
                }
                Err(e) => eprintln!("Failed to read macro: {e}"),
            }
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}

/// Set a text macro for a key
pub fn set_macro(key: &str, text: &str) -> CommandResult {
    let macro_index: u8 = key.parse().unwrap_or(0);

    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            println!("Setting macro {macro_index} to type: \"{text}\"");

            match keyboard.set_text_macro(macro_index, text, 10, 1) {
                Ok(()) => {
                    println!("Macro {macro_index} set successfully!");
                    println!("Assign this macro to a key in the Akko driver to test.");
                }
                Err(e) => eprintln!("Failed to set macro: {e}"),
            }
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}

/// Clear macro from a key
pub fn clear_macro(key: &str) -> CommandResult {
    let macro_index: u8 = key.parse().unwrap_or(0);

    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            println!("Clearing macro {macro_index}...");

            match keyboard.set_macro(macro_index, &[], 1) {
                Ok(()) => println!("Macro {macro_index} cleared!"),
                Err(e) => eprintln!("Failed to clear macro: {e}"),
            }
        }
        Err(e) => eprintln!("No device found: {e}"),
    }
    Ok(())
}
