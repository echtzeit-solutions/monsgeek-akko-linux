//! Integration tests for the remap list detection logic.
//!
//! These exercise the remap detection algorithm using the public building blocks
//! (`hid::key_code_from_name`, `KeyAction`, `matrix::key_name`) without
//! requiring a physical keyboard device.

use iot_driver::key_action::KeyAction;
use iot_driver::protocol::hid;
use monsgeek_transport::protocol::matrix;

/// Replicate the matrix_default_keycode logic from commands/keymap.rs
/// so we can test the detection algorithm end-to-end.
fn matrix_default_keycode(name: &str) -> Option<u8> {
    hid::key_code_from_name(name).or_else(|| match name {
        "Spc" => Some(0x2C),
        _ => None,
    })
}

// ── matrix_default_keycode resolution ──

#[test]
fn default_keycode_standard_keys() {
    // Keys whose matrix name matches a canonical HID name
    assert_eq!(matrix_default_keycode("Esc"), Some(0x29));
    assert_eq!(matrix_default_keycode("Tab"), Some(0x2B));
    assert_eq!(matrix_default_keycode("Caps"), Some(0x39));
    assert_eq!(matrix_default_keycode("A"), Some(0x04));
    assert_eq!(matrix_default_keycode("Z"), Some(0x1D));
    assert_eq!(matrix_default_keycode("F1"), Some(0x3A));
    assert_eq!(matrix_default_keycode("F12"), Some(0x45));
    assert_eq!(matrix_default_keycode("Del"), Some(0x4C));
    assert_eq!(matrix_default_keycode("Home"), Some(0x4A));
    assert_eq!(matrix_default_keycode("PgUp"), Some(0x4B));
    assert_eq!(matrix_default_keycode("PgDn"), Some(0x4E));
    assert_eq!(matrix_default_keycode("End"), Some(0x4D));
}

#[test]
fn default_keycode_abbreviations() {
    // Matrix-specific abbreviations that need alias support
    assert_eq!(matrix_default_keycode("Ent"), Some(0x28)); // Enter
    assert_eq!(matrix_default_keycode("IntlRo"), Some(0x87)); // International Ro
    assert_eq!(matrix_default_keycode("Bksp"), Some(0x2A)); // Backspace
    assert_eq!(matrix_default_keycode("LShf"), Some(0xE1)); // Left Shift
    assert_eq!(matrix_default_keycode("RShf"), Some(0xE5)); // Right Shift
    assert_eq!(matrix_default_keycode("LCtl"), Some(0xE0)); // Left Control
    assert_eq!(matrix_default_keycode("RCtl"), Some(0xE4)); // Right Control
    assert_eq!(matrix_default_keycode("Win"), Some(0xE3)); // Windows/GUI
    assert_eq!(matrix_default_keycode("LAlt"), Some(0xE2)); // Left Alt
    assert_eq!(matrix_default_keycode("RAlt"), Some(0xE6)); // Right Alt
    assert_eq!(matrix_default_keycode("Spc"), Some(0x2C)); // Space (custom fallback)
}

#[test]
fn default_keycode_unknown_returns_none() {
    assert_eq!(matrix_default_keycode("?"), None);
    assert_eq!(matrix_default_keycode("FooBar"), None);
}

// ── All named matrix positions resolve to a default keycode ──

#[test]
fn all_matrix_names_have_defaults() {
    // Every non-"?" matrix position should resolve to a valid HID code.
    // This catches regressions where new matrix entries lack aliases.
    let mut missing = Vec::new();
    for i in 0..126u8 {
        let name = matrix::key_name(i);
        if name == "?" {
            continue;
        }
        if matrix_default_keycode(name).is_none() {
            missing.push((i, name));
        }
    }
    assert!(
        missing.is_empty(),
        "Matrix positions without default keycode: {missing:?}\n\
         Add aliases to hid::key_code_from_name or matrix_default_keycode()"
    );
}

// ── Base layer remap detection ──

#[test]
fn base_layer_default_key_not_remapped() {
    // Esc at matrix position 0 has default keycode 0x29
    let default_code = matrix_default_keycode("Esc").unwrap();
    let action = KeyAction::from_config_bytes([0, 0, default_code, 0]);
    assert_eq!(action, KeyAction::Key(0x29));
    // Should NOT be detected as remapped
    assert_eq!(action, KeyAction::Key(default_code));
}

#[test]
fn base_layer_different_key_is_remapped() {
    // CapsLock (matrix position 3) remapped to Escape
    let default_code = matrix_default_keycode("Caps").unwrap();
    assert_eq!(default_code, 0x39); // CapsLock
    let remapped = KeyAction::from_config_bytes([0, 0, 0x29, 0]); // Escape
    assert_ne!(remapped, KeyAction::Key(default_code));
}

#[test]
fn base_layer_combo_is_remapped() {
    // A key remapped to Ctrl+C
    let default_code = matrix_default_keycode("A").unwrap();
    let combo = KeyAction::from_config_bytes([0, 0x01, 0x06, 0]); // LCtrl+C
    assert_ne!(combo, KeyAction::Key(default_code));
    assert!(matches!(combo, KeyAction::Combo { .. }));
}

#[test]
fn base_layer_macro_is_remapped() {
    // F1 remapped to Macro(0)
    let default_code = matrix_default_keycode("F1").unwrap();
    let macro_action = KeyAction::from_config_bytes([9, 0, 0, 0]);
    assert_ne!(macro_action, KeyAction::Key(default_code));
    assert!(matches!(macro_action, KeyAction::Macro { .. }));
}

#[test]
fn base_layer_mouse_is_remapped() {
    let default_code = matrix_default_keycode("F5").unwrap();
    let mouse = KeyAction::from_config_bytes([1, 0, 1, 0]);
    assert_ne!(mouse, KeyAction::Key(default_code));
    assert_eq!(mouse, KeyAction::Mouse(1));
}

// ── Fn layer remap detection ──

#[test]
fn fn_layer_disabled_is_default() {
    let action = KeyAction::from_config_bytes([0, 0, 0, 0]);
    assert_eq!(action, KeyAction::Disabled);
    // Fn defaults are all Disabled, so this should NOT appear
}

#[test]
fn fn_layer_any_key_is_remapped() {
    let action = KeyAction::from_config_bytes([0, 0, 0x3A, 0]); // F1
    assert_ne!(action, KeyAction::Disabled);
    // Should appear in Fn layer remaps
}

#[test]
fn fn_layer_macro_is_remapped() {
    let action = KeyAction::from_config_bytes([9, 0, 2, 0]); // Macro(2)
    assert_ne!(action, KeyAction::Disabled);
    assert!(matches!(action, KeyAction::Macro { index: 2, .. }));
}

// ── Simulated full matrix scan ──

#[test]
fn simulated_remap_scan() {
    // Build a fake keymatrix buffer for a few keys with known remaps
    let key_count = 6; // Just test first column: Esc, `, Tab, Caps, LShf, LCtl
    let mut data = vec![0u8; key_count * 4];

    // Position 0: Esc (0x29) → ` (0x35) — REMAPPED
    data[0..4].copy_from_slice(&[0, 0, 0x35, 0]);
    // Position 1: ` (0x35) → Esc (0x29) — REMAPPED
    data[4..8].copy_from_slice(&[0, 0, 0x29, 0]);
    // Position 2: Tab (0x2B) → Tab (0x2B) — DEFAULT
    data[8..12].copy_from_slice(&[0, 0, 0x2B, 0]);
    // Position 3: Caps (0x39) → Escape (0x29) — REMAPPED
    data[12..16].copy_from_slice(&[0, 0, 0x29, 0]);
    // Position 4: LShf (0xE1) → LShift (0xE1) — DEFAULT
    data[16..20].copy_from_slice(&[0, 0, 0xE1, 0]);
    // Position 5: LCtl (0xE0) → Macro(0) — REMAPPED
    data[20..24].copy_from_slice(&[9, 0, 0, 0]);

    let mut remaps = Vec::new();
    for i in 0..key_count {
        let name = matrix::key_name(i as u8);
        if name == "?" {
            continue;
        }
        let k = &data[i * 4..(i + 1) * 4];
        let action = KeyAction::from_config_bytes([k[0], k[1], k[2], k[3]]);

        let is_remapped = match matrix_default_keycode(name) {
            Some(default_code) => action != KeyAction::Key(default_code),
            None => action != KeyAction::Disabled,
        };

        if is_remapped {
            remaps.push((i, name, action));
        }
    }

    assert_eq!(remaps.len(), 4);
    assert_eq!(remaps[0], (0, "Esc", KeyAction::Key(0x35))); // ` (backtick)
    assert_eq!(remaps[1], (1, "`", KeyAction::Key(0x29))); // Escape
    assert_eq!(remaps[2], (3, "Caps", KeyAction::Key(0x29))); // Escape
    assert_eq!(
        remaps[3],
        (5, "LCtl", KeyAction::Macro { index: 0, kind: 0 })
    );
}

#[test]
fn simulated_fn_layer_scan() {
    // Fn layer: all Disabled by default, only non-Disabled entries are remaps
    let key_count = 6;
    let mut data = vec![0u8; key_count * 4];

    // Position 0: Esc → Disabled (default) — NOT REMAPPED
    data[0..4].copy_from_slice(&[0, 0, 0, 0]);
    // Position 1: ` → Disabled — NOT REMAPPED
    data[4..8].copy_from_slice(&[0, 0, 0, 0]);
    // Position 2: Tab → Macro(1) — REMAPPED
    data[8..12].copy_from_slice(&[9, 0, 1, 0]);
    // Position 3: Caps → Mouse1 — REMAPPED
    data[12..16].copy_from_slice(&[1, 0, 1, 0]);
    // Position 4: LShf → Disabled — NOT REMAPPED
    data[16..20].copy_from_slice(&[0, 0, 0, 0]);
    // Position 5: LCtl → F13 — REMAPPED
    data[20..24].copy_from_slice(&[0, 0, 0x68, 0]);

    let mut remaps = Vec::new();
    for i in 0..key_count {
        let name = matrix::key_name(i as u8);
        if name == "?" {
            continue;
        }
        let k = &data[i * 4..(i + 1) * 4];
        let action = KeyAction::from_config_bytes([k[0], k[1], k[2], k[3]]);

        if action != KeyAction::Disabled {
            remaps.push((i, name, action));
        }
    }

    assert_eq!(remaps.len(), 3);
    assert_eq!(
        remaps[0],
        (2, "Tab", KeyAction::Macro { index: 1, kind: 0 })
    );
    assert_eq!(remaps[1], (3, "Caps", KeyAction::Mouse(1)));
    assert_eq!(remaps[2], (5, "LCtl", KeyAction::Key(0x68)));
}

// ── Display formatting ──

#[test]
fn remap_display_format() {
    // Verify that KeyAction Display produces readable output for remaps
    assert_eq!(format!("{}", KeyAction::Key(0x29)), "Escape");
    assert_eq!(format!("{}", KeyAction::Key(0x35)), "`");
    assert_eq!(format!("{}", KeyAction::Mouse(1)), "Mouse1");
    assert_eq!(
        format!("{}", KeyAction::Macro { index: 0, kind: 0 }),
        "Macro(0)"
    );
    assert_eq!(
        format!(
            "{}",
            KeyAction::Combo {
                mods: 0x01,
                key: 0x06
            }
        ),
        "Ctrl+C"
    );
}
