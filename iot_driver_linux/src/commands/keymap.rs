//! Key remapping command handlers.

use super::{with_keyboard, CommandResult};
use iot_driver::key_action::KeyAction;
use iot_driver::protocol::hid;
use monsgeek_transport::protocol::matrix;

/// Resolve a key name or numeric index to a matrix position.
fn resolve_matrix_index(key: &str) -> Result<u8, String> {
    // Try numeric index first
    if let Ok(idx) = key.parse::<u8>() {
        return Ok(idx);
    }
    // Try matrix key name (Esc, F3, LShf, etc.)
    if let Some(idx) = matrix::key_index_from_name(key) {
        return Ok(idx);
    }
    Err(format!(
        "unknown key: \"{key}\". Use a matrix index (0-95) or name like F3, Esc, Tab"
    ))
}

/// Remap a key
pub fn remap(from: &str, to: &str, layer: u8) -> CommandResult {
    let key_index = match resolve_matrix_index(from) {
        Ok(idx) => idx,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };

    let action: KeyAction = match to.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Invalid target key: {e}");
            return Ok(());
        }
    };

    let hid_code = match action.hid_code() {
        Some(code) => code,
        None => {
            eprintln!(
                "Only simple key remaps are supported (got {action}). \
                 Use assign-macro for macros."
            );
            return Ok(());
        }
    };

    with_keyboard(|keyboard| {
        let from_name = matrix::key_name(key_index);
        println!("Remapping {from_name} (index {key_index}) to {action} (0x{hid_code:02x}) on layer {layer}...");
        match keyboard.set_keymatrix(layer, key_index, hid_code, true, 0) {
            Ok(()) => println!("{from_name} remapped to {action}"),
            Err(e) => eprintln!("Failed to remap key: {e}"),
        }
        Ok(())
    })
}

/// Reset a key to default
pub fn reset_key(key: &str, layer: u8) -> CommandResult {
    let key_index = match resolve_matrix_index(key) {
        Ok(idx) => idx,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };

    with_keyboard(|keyboard| {
        let key_name = matrix::key_name(key_index);
        println!("Resetting {key_name} (index {key_index}) on layer {layer}...");
        match keyboard.reset_key(layer, key_index) {
            Ok(()) => println!("{key_name} reset to default"),
            Err(e) => eprintln!("Failed to reset key: {e}"),
        }
        Ok(())
    })
}

/// Swap two keys
pub fn swap(key1: &str, key2: &str, layer: u8) -> CommandResult {
    let key_a = match resolve_matrix_index(key1) {
        Ok(idx) => idx,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };
    let key_b = match resolve_matrix_index(key2) {
        Ok(idx) => idx,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };

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

                let name_a = matrix::key_name(key_a);
                let name_b = matrix::key_name(key_b);
                let action_a = hid::key_name(code_a);
                let action_b = hid::key_name(code_b);
                println!("Swapping {name_a} ({action_a}) <-> {name_b} ({action_b})...");

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

/// A single key remapping (non-default binding).
pub struct Remap {
    /// Matrix position index.
    pub index: u8,
    /// Human-readable matrix key name (e.g. "Esc", "A").
    pub position: &'static str,
    /// Layer number (0 = base, 1 = Fn).
    pub layer: u8,
    /// Current action assigned to this key.
    pub action: KeyAction,
}

/// Resolve the expected default HID keycode for a matrix position name.
///
/// Uses `hid::key_code_from_name` plus a small fallback table for
/// matrix-specific abbreviations that don't match canonical HID names.
pub fn matrix_default_keycode(name: &str) -> Option<u8> {
    hid::key_code_from_name(name).or(match name {
        "Spc" => Some(0x2C), // Space
        _ => None,
    })
}

/// Read both layers from the keyboard and return only non-default keys.
///
/// Detection logic per matrix position `i`:
/// - Skip positions named `"?"` (unused matrix slots).
/// - **Base layer (0):** Compare current action against `KeyAction::Key(default_code)`.
///   Include if different, or if the action is not a simple Key.
/// - **Fn layer (1):** Include any entry that isn't `Disabled`
///   (Fn defaults are all disabled for user-assignable keys).
pub fn list_remaps(
    keyboard: &monsgeek_keyboard::SyncKeyboard,
) -> Result<Vec<Remap>, monsgeek_keyboard::KeyboardError> {
    let key_count = keyboard.key_count() as usize;
    let mut remaps = Vec::new();

    for layer in 0..2u8 {
        let data = keyboard.get_keymatrix(layer, 3)?;

        for i in 0..key_count {
            if i * 4 + 3 >= data.len() {
                break;
            }
            let name = matrix::key_name(i as u8);
            if name == "?" {
                continue;
            }

            let k = &data[i * 4..(i + 1) * 4];
            let action = KeyAction::from_config_bytes([k[0], k[1], k[2], k[3]]);

            let is_remapped = if layer == 0 {
                // Base layer: compare against expected default
                match matrix_default_keycode(name) {
                    Some(default_code) => action != KeyAction::Key(default_code),
                    None => action != KeyAction::Disabled,
                }
            } else {
                // Fn layer: anything that isn't Disabled is a remap
                action != KeyAction::Disabled
            };

            if is_remapped {
                remaps.push(Remap {
                    index: i as u8,
                    position: name,
                    layer,
                    action,
                });
            }
        }
    }

    Ok(remaps)
}

/// CLI handler: list all key remappings.
pub fn remap_list(layer_filter: Option<u8>) -> CommandResult {
    with_keyboard(|keyboard| {
        let remaps = match list_remaps(keyboard) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Failed to read key matrix: {e}");
                return Ok(());
            }
        };

        let remaps: Vec<&Remap> = remaps
            .iter()
            .filter(|r| layer_filter.is_none_or(|l| r.layer == l))
            .collect();

        if remaps.is_empty() {
            println!("No remappings found.");
            return Ok(());
        }

        println!("Key remappings:\n");

        // Base layer
        let base: Vec<&&Remap> = remaps.iter().filter(|r| r.layer == 0).collect();
        if !base.is_empty() {
            println!("Base layer:");
            for r in &base {
                println!("  {:<12} ({:<3}) → {}", r.position, r.index, r.action);
            }
            println!();
        }

        // Fn layer
        let fnl: Vec<&&Remap> = remaps.iter().filter(|r| r.layer == 1).collect();
        if !fnl.is_empty() {
            println!("Fn layer:");
            for r in &fnl {
                println!("  {:<12} ({:<3}) → {}", r.position, r.index, r.action);
            }
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
                let key_count = keyboard.key_count() as usize;
                println!("\nKey mappings (layer {layer}):");
                for i in 0..key_count {
                    if i * 4 + 3 >= data.len() {
                        break;
                    }
                    let k = &data[i * 4..(i + 1) * 4];
                    let pos_name = matrix::key_name(i as u8);
                    let action = KeyAction::from_config_bytes([k[0], k[1], k[2], k[3]]);

                    // Skip uninteresting entries (unknown matrix position, default mapping)
                    if pos_name == "?" && action == KeyAction::Disabled {
                        continue;
                    }

                    let detail = if matches!(action, KeyAction::Key(_)) {
                        format!(" (0x{:02x})", k[2])
                    } else {
                        format!(" [{:02x} {:02x} {:02x} {:02x}]", k[0], k[1], k[2], k[3])
                    };
                    println!("  {:3} {:<6} -> {action}{detail}", i, pos_name);
                }
            }
            Err(e) => eprintln!("Failed to read key matrix: {e}"),
        }
        Ok(())
    })
}
