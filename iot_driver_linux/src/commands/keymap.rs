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
        match keyboard.get_keymatrix(layer, 8) {
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

/// Read both layers from the keyboard and return only user-modified keys.
///
/// Detection compares each key against factory defaults:
/// - `[0, 0, 0, 0]` = disabled → skip
/// - `[10, 1, 0, 0]` = Fn key (factory default at physical Fn position) → skip
/// - config_type ≠ 0 (mouse/macro/consumer/etc.) → include
/// - byte 1 ≠ 0 (user remap or combo) → include
/// - `[0, 0, code, 0]`: include only if code ≠ factory default for that position
///
/// Factory defaults are derived from the transport matrix key names
/// (e.g. position 12 = "F1" → HID 0x3A). Both layers use the same defaults
/// since both store identity maps `[0, 0, default_code, 0]` in their base state.
pub fn list_remaps(
    keyboard: &monsgeek_keyboard::SyncKeyboard,
) -> Result<Vec<Remap>, monsgeek_keyboard::KeyboardError> {
    let key_count = keyboard.key_count() as usize;

    // Build factory default keycodes from the transport's matrix key names.
    // Each position has a name (e.g. "F1", "A") that resolves to its default HID code.
    let defaults: Vec<u8> = (0..key_count as u8)
        .map(|i| hid::key_code_from_name(matrix::key_name(i)).unwrap_or(0))
        .collect();

    let mut remaps = Vec::new();

    for layer in 0..2u8 {
        let data = keyboard.get_keymatrix(layer, 8)?;

        for i in 0..key_count {
            if i * 4 + 3 >= data.len() {
                break;
            }
            let name = matrix::key_name(i as u8);
            if name == "?" {
                continue;
            }

            let k = &data[i * 4..(i + 1) * 4];

            // Disabled entries are never remaps
            if k == [0, 0, 0, 0] {
                continue;
            }

            let action = KeyAction::from_config_bytes([k[0], k[1], k[2], k[3]]);

            // Fn key at physical position is factory default
            if matches!(action, KeyAction::Fn) {
                continue;
            }

            // Non-key config types or user byte format (byte1 != 0) are always remaps
            if k[0] != 0 || k[1] != 0 {
                remaps.push(Remap {
                    index: i as u8,
                    position: name,
                    layer,
                    action,
                });
                continue;
            }

            // config_type=0, byte1=0, byte2!=0: compare against factory default
            if k[2] == defaults[i] {
                continue; // matches factory default (identity map)
            }

            remaps.push(Remap {
                index: i as u8,
                position: name,
                layer,
                action,
            });
        }
    }

    Ok(remaps)
}

/// CLI handler: list key remappings.
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

        for layer in 0..2u8 {
            let layer_remaps: Vec<&&Remap> = remaps.iter().filter(|r| r.layer == layer).collect();
            if layer_remaps.is_empty() {
                continue;
            }
            println!("Layer {layer}:");
            for r in &layer_remaps {
                println!("  {:<12} ({:<3}) → {}", r.position, r.index, r.action);
            }
            println!();
        }

        Ok(())
    })
}

/// Show the Fn layer key bindings (media keys, LED controls, etc.)
pub fn fn_layout(sys: &str) -> CommandResult {
    with_keyboard(|keyboard| {
        let sys_code: u8 = match sys {
            "mac" => 1,
            _ => 0,
        };
        let data = match keyboard.get_fn_keymatrix(0, sys_code, 8) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Failed to read Fn layer: {e}");
                return Ok(());
            }
        };

        println!("Fn layer ({sys} mode):\n");
        for i in 0..(data.len() / 4) {
            let k: [u8; 4] = [
                data[i * 4],
                data[i * 4 + 1],
                data[i * 4 + 2],
                data[i * 4 + 3],
            ];
            if k == [0, 0, 0, 0] {
                continue;
            }
            let name = matrix::key_name(i as u8);
            let action = KeyAction::from_config_bytes(k);
            println!("  {:<12} ({:<3}) -> {}", name, i, action);
        }
        Ok(())
    })
}

/// Show key matrix mappings
pub fn keymatrix(layer: u8) -> CommandResult {
    with_keyboard(|keyboard| {
        println!("Reading key matrix for layer {layer}...");

        match keyboard.get_keymatrix(layer, 8) {
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
