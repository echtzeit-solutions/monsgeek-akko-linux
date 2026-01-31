//! Macro command handlers.

use super::{with_keyboard, CommandResult};
use iot_driver::protocol::hid;
use monsgeek_keyboard::parse_macro_events;
use monsgeek_transport::protocol::matrix;

/// Get macro for a key
pub fn get_macro(key: &str) -> CommandResult {
    let macro_index: u8 = key.parse().unwrap_or(0);
    with_keyboard(|keyboard| {
        println!("Reading macro {macro_index}...");
        match keyboard.get_macro(macro_index) {
            Ok(data) => {
                let (repeat_count, events) = parse_macro_events(&data);

                if events.is_empty() {
                    println!("Macro {macro_index} is empty");
                } else {
                    println!("Repeat count: {repeat_count}");
                    println!("Events ({}):", events.len());
                    for (i, evt) in events.iter().enumerate() {
                        let arrow = if evt.is_down { "↓" } else { "↑" };
                        let key_name = hid::key_name(evt.keycode);
                        let delay_str = if evt.delay_ms > 0 {
                            format!(" +{}ms", evt.delay_ms)
                        } else {
                            String::new()
                        };
                        println!(
                            "  {i:3}: {arrow} {key_name} (0x{:02x}){delay_str}",
                            evt.keycode
                        );
                    }

                    // Try to reconstruct text preview
                    let preview = text_preview_from_events(&events);
                    if !preview.is_empty() {
                        println!("\nText preview: \"{preview}\"");
                    }
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
        Ok(())
    })
}

/// Set a text macro for a key
pub fn set_macro(key: &str, text: &str) -> CommandResult {
    let macro_index: u8 = key.parse().unwrap_or(0);

    with_keyboard(|keyboard| {
        println!("Setting macro {macro_index} to type: \"{text}\"");

        match keyboard.set_text_macro(macro_index, text, 10, 1) {
            Ok(()) => {
                println!("Macro {macro_index} set successfully!");
                println!("Assign this macro to a key with: assign-macro <key> {macro_index}");
            }
            Err(e) => eprintln!("Failed to set macro: {e}"),
        }
        Ok(())
    })
}

/// Clear macro from a key
pub fn clear_macro(key: &str) -> CommandResult {
    let macro_index: u8 = key.parse().unwrap_or(0);

    with_keyboard(|keyboard| {
        println!("Clearing macro {macro_index}...");

        match keyboard.set_macro(macro_index, &[], 1) {
            Ok(()) => println!("Macro {macro_index} cleared!"),
            Err(e) => eprintln!("Failed to clear macro: {e}"),
        }
        Ok(())
    })
}

/// Assign a macro to a key (base layer or Fn layer)
pub fn assign_macro(key: &str, macro_index_str: &str, fn_layer: bool) -> CommandResult {
    let macro_index: u8 = macro_index_str.parse().unwrap_or(0);

    // Resolve key name to matrix index
    let key_index = if let Ok(idx) = key.parse::<u8>() {
        idx
    } else if let Some(idx) = matrix::key_index_from_name(key) {
        idx
    } else {
        eprintln!(
            "Unknown key name: \"{key}\". Use a matrix index number or key name like F3, Esc, etc."
        );
        return Ok(());
    };

    with_keyboard(|keyboard| {
        let key_name = matrix::key_name(key_index);
        let layer = if fn_layer { "Fn+" } else { "" };
        println!("Assigning macro {macro_index} to {layer}{key_name} (index {key_index})...");

        // Use profile 0, macro_type 0 (repeat by count)
        let result = if fn_layer {
            keyboard.assign_macro_to_fn_key(0, key_index, macro_index, 0)
        } else {
            keyboard.assign_macro_to_key(0, key_index, macro_index, 0)
        };
        match result {
            Ok(()) => println!("Macro {macro_index} assigned to {layer}{key_name}"),
            Err(e) => eprintln!("Failed to assign macro: {e}"),
        }
        Ok(())
    })
}

/// Try to reconstruct typed text from macro events
fn text_preview_from_events(events: &[monsgeek_keyboard::MacroEvent]) -> String {
    let mut result = String::new();
    let mut shift_held = false;

    for evt in events {
        // Track shift state
        if evt.keycode == 0xE1 || evt.keycode == 0xE5 {
            shift_held = evt.is_down;
            continue;
        }

        // Only process key-down events
        if !evt.is_down {
            continue;
        }

        let ch = hid_keycode_to_char(evt.keycode, shift_held);
        if let Some(c) = ch {
            result.push(c);
        }
    }
    result
}

/// Convert HID keycode to character (inverse of char_to_hid)
fn hid_keycode_to_char(keycode: u8, shift: bool) -> Option<char> {
    match keycode {
        0x04..=0x1D => {
            // A-Z
            let base = (keycode - 0x04 + b'a') as char;
            Some(if shift {
                base.to_ascii_uppercase()
            } else {
                base
            })
        }
        0x1E..=0x26 => {
            // 1-9
            if shift {
                Some(b"!@#$%^&*("[(keycode - 0x1E) as usize] as char)
            } else {
                Some((b'1' + keycode - 0x1E) as char)
            }
        }
        0x27 => Some(if shift { ')' } else { '0' }),
        0x28 => Some('\n'), // Enter
        0x2B => Some('\t'), // Tab
        0x2C => Some(' '),  // Space
        0x2D => Some(if shift { '_' } else { '-' }),
        0x2E => Some(if shift { '+' } else { '=' }),
        0x2F => Some(if shift { '{' } else { '[' }),
        0x30 => Some(if shift { '}' } else { ']' }),
        0x31 => Some(if shift { '|' } else { '\\' }),
        0x33 => Some(if shift { ':' } else { ';' }),
        0x34 => Some(if shift { '"' } else { '\'' }),
        0x35 => Some(if shift { '~' } else { '`' }),
        0x36 => Some(if shift { '<' } else { ',' }),
        0x37 => Some(if shift { '>' } else { '.' }),
        0x38 => Some(if shift { '?' } else { '/' }),
        _ => None,
    }
}
