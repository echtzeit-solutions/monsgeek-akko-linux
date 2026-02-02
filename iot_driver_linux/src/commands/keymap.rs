//! Key remapping command handlers.

use super::{with_keyboard, CommandResult};
use iot_driver::key_action::KeyAction;
use iot_driver::keymap::{self, KeyRef, Layer};
use iot_driver::protocol::hid;
use monsgeek_transport::protocol::matrix;

/// Remap a key.
///
/// `from` can include a layer prefix: `"Fn+Caps"`, `"L1+A"`, `"42"`.
/// When a layer prefix is present, it takes precedence over the `--layer` flag.
pub fn remap(from: &str, to: &str, layer: u8) -> CommandResult {
    let key_ref: KeyRef = match from.parse() {
        Ok(kr) => kr,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };

    // If from has a layer prefix (not Base when the raw string contains "+"),
    // use that; otherwise use the --layer flag as override.
    let effective_layer = if from.contains('+') {
        key_ref.layer
    } else {
        Layer::from_wire(layer)
    };

    let action: KeyAction = match to.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Invalid target key: {e}");
            return Ok(());
        }
    };

    with_keyboard(|keyboard| {
        let display_ref = KeyRef::new(key_ref.index, effective_layer);
        println!(
            "Remapping {} (index {}) to {action} on {} layer...",
            display_ref,
            key_ref.index,
            effective_layer.name()
        );
        match keymap::set_key_sync(keyboard, key_ref.index, effective_layer, &action) {
            Ok(()) => println!("{display_ref} remapped to {action}"),
            Err(e) => eprintln!("Failed to remap key: {e}"),
        }
        Ok(())
    })
}

/// Reset a key to default.
///
/// `key` can include a layer prefix: `"Fn+Caps"`, `"L1+A"`.
pub fn reset_key(key: &str, layer: u8) -> CommandResult {
    let key_ref: KeyRef = match key.parse() {
        Ok(kr) => kr,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };

    let effective_layer = if key.contains('+') {
        key_ref.layer
    } else {
        Layer::from_wire(layer)
    };

    with_keyboard(|keyboard| {
        let display_ref = KeyRef::new(key_ref.index, effective_layer);
        println!(
            "Resetting {} (index {}) on {}...",
            display_ref,
            key_ref.index,
            effective_layer.name()
        );
        match keymap::reset_key_sync(keyboard, key_ref.index, effective_layer) {
            Ok(()) => println!("{display_ref} reset to default"),
            Err(e) => eprintln!("Failed to reset key: {e}"),
        }
        Ok(())
    })
}

/// Swap two keys
pub fn swap(key1: &str, key2: &str, layer: u8) -> CommandResult {
    let kr_a: KeyRef = match key1.parse() {
        Ok(kr) => kr,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };
    let kr_b: KeyRef = match key2.parse() {
        Ok(kr) => kr,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };

    with_keyboard(|keyboard| {
        match keyboard.get_keymatrix(layer, 8) {
            Ok(data) => {
                let key_a = kr_a.index;
                let key_b = kr_b.index;
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

/// CLI handler: list key remappings.
pub fn remap_list(layer_filter: Option<u8>, show_all: bool) -> CommandResult {
    with_keyboard(|keyboard| {
        let keymap = match keymap::load_sync(keyboard) {
            Ok(km) => km,
            Err(e) => {
                eprintln!("Failed to read key matrix: {e}");
                return Ok(());
            }
        };

        // Collect entries based on --all flag
        let entries: Vec<_> = if show_all {
            keymap.iter().collect()
        } else {
            keymap.remaps().collect()
        };

        // Apply layer filter
        let filter_layer = layer_filter.map(Layer::from_wire);
        let entries: Vec<_> = entries
            .into_iter()
            .filter(|e| filter_layer.is_none_or(|l| e.layer == l))
            .collect();

        if entries.is_empty() {
            if show_all {
                println!("No keys found.");
            } else {
                println!("No remappings found.");
            }
            return Ok(());
        }

        if show_all {
            println!("All key mappings:\n");
        } else {
            println!("Key remappings:\n");
        }

        for layer in Layer::ALL {
            let layer_entries: Vec<_> = entries.iter().filter(|e| e.layer == layer).collect();
            if layer_entries.is_empty() {
                continue;
            }
            println!("{}:", layer.name());
            for e in &layer_entries {
                let ref_display = e.key_ref();
                println!("  {:<12} ({:<3}) -> {}", ref_display, e.index, e.action);
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
            let ref_display = KeyRef::new(i as u8, Layer::Fn);
            let action = KeyAction::from_config_bytes(k);
            println!("  {:<12} ({:<3}) -> {}", ref_display, i, action);
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
