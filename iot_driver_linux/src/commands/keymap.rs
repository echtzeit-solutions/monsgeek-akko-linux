//! Key remapping command handlers.

use super::{with_keyboard, CommandResult};
use iot_driver::protocol::hid;

/// Remap a key
pub fn remap(from: &str, to: &str, layer: u8) -> CommandResult {
    let key_index: u8 = from.parse().unwrap_or(0);
    let hid_code = u8::from_str_radix(to, 16).unwrap_or(0);

    with_keyboard(|keyboard| {
        let key_name = hid::key_name(hid_code);
        println!("Remapping key {key_index} to {key_name} (0x{hid_code:02x}) on layer {layer}...");
        match keyboard.set_keymatrix(layer, key_index, hid_code, true, 0) {
            Ok(()) => println!("Key {key_index} remapped to {key_name}"),
            Err(e) => eprintln!("Failed to remap key: {e}"),
        }
        Ok(())
    })
}

/// Reset a key to default
pub fn reset_key(key: &str, layer: u8) -> CommandResult {
    let key_index: u8 = key.parse().unwrap_or(0);

    with_keyboard(|keyboard| {
        println!("Resetting key {key_index} on layer {layer}...");
        match keyboard.reset_key(layer, key_index) {
            Ok(()) => println!("Key {key_index} reset to default"),
            Err(e) => eprintln!("Failed to reset key: {e}"),
        }
        Ok(())
    })
}

/// Swap two keys
pub fn swap(key1: &str, key2: &str, layer: u8) -> CommandResult {
    let key_a: u8 = key1.parse().unwrap_or(0);
    let key_b: u8 = key2.parse().unwrap_or(0);

    with_keyboard(|keyboard| {
        match keyboard.get_keymatrix(layer, 2) {
            Ok(data) => {
                let code_a = if (key_a as usize) * 4 + 2 < data.len() {
                    data[(key_a as usize) * 4 + 2]
                } else {
                    0
                };
                let code_b = if (key_b as usize) * 4 + 2 < data.len() {
                    data[(key_b as usize) * 4 + 2]
                } else {
                    0
                };

                let name_a = hid::key_name(code_a);
                let name_b = hid::key_name(code_b);
                println!("Swapping key {key_a} ({name_a}) <-> key {key_b} ({name_b})...");

                match keyboard.swap_keys(layer, key_a, code_a, key_b, code_b) {
                    Ok(()) => println!("Keys swapped successfully"),
                    Err(e) => eprintln!("Failed to swap keys: {e}"),
                }
            }
            Err(e) => eprintln!("Failed to read current key mappings: {e}"),
        }
        Ok(())
    })
}

/// Show key matrix mappings
pub fn keymatrix(layer: u8) -> CommandResult {
    with_keyboard(|keyboard| {
        println!("Reading key matrix for layer {layer}...");

        match keyboard.get_keymatrix(layer, 3) {
            Ok(data) => {
                println!("\nKey matrix data ({} bytes):", data.len());
                for (i, chunk) in data.chunks(16).enumerate() {
                    print!("{:04x}: ", i * 16);
                    for b in chunk {
                        print!("{b:02x} ");
                    }
                    print!("  |");
                    for b in chunk {
                        if *b >= 0x20 && *b < 0x7f {
                            print!("{}", *b as char);
                        } else {
                            print!(".");
                        }
                    }
                    println!("|");
                }

                println!("\nKey mappings (format: [type, flags, code, layer]):");
                let key_count = keyboard.key_count() as usize;
                for i in 0..key_count.min(20) {
                    if i * 4 + 3 < data.len() {
                        let k = &data[i * 4..(i + 1) * 4];
                        let hid_code = k[2];
                        let key_name = hid::key_name(hid_code);
                        println!(
                            "  Key {:2}: {:02x} {:02x} {:02x} {:02x}  -> {} (0x{:02x})",
                            i, k[0], k[1], k[2], k[3], key_name, hid_code
                        );
                    }
                }
            }
            Err(e) => eprintln!("Failed to read key matrix: {e}"),
        }
        Ok(())
    })
}
