//! Integration tests for the remap list detection logic.
//!
//! Detection compares each key against factory defaults:
//! - `[0, 0, 0, 0]` = disabled → skip
//! - `[10, 1, 0, 0]` = Fn key (factory default at physical Fn position) → skip
//! - config_type ≠ 0 (mouse/macro/consumer/etc.) → include
//! - byte 1 ≠ 0 (user remap or combo) → include
//! - `[0, 0, code, 0]`: include only if code ≠ factory default for that position
//!
//! Factory defaults are derived from the transport matrix key names
//! (e.g. position 12 = "F1" → HID 0x3A), NOT from device_matrices.json
//! which uses a different position numbering.

use iot_driver::key_action::KeyAction;
use iot_driver::protocol::hid;
use monsgeek_transport::protocol::matrix;

/// Helper: detect whether a 4-byte key config represents a user remap.
/// Mirrors the logic in `commands::keymap::list_remaps` and `tui::load_remaps`.
///
/// `default_hid_code`: the factory default HID keycode for this matrix position,
/// derived from `hid::key_code_from_name(matrix::key_name(i))`.
fn is_user_remap(k: &[u8; 4], default_hid_code: u8) -> bool {
    // Disabled: never a remap
    if *k == [0, 0, 0, 0] {
        return false;
    }

    // Fn key at physical Fn position: factory default
    if matches!(KeyAction::from_config_bytes(*k), KeyAction::Fn) {
        return false;
    }

    // Non-zero config_type (mouse/macro/consumer/etc): always a remap
    if k[0] != 0 {
        return true;
    }

    // Byte 1 non-zero (user remap format or combo): always a remap
    if k[1] != 0 {
        return true;
    }

    // config_type=0, byte1=0, byte2!=0: compare against factory default
    k[2] != default_hid_code
}

/// Resolve the default HID code for a matrix position, same as the production code.
fn default_for(pos: u8) -> u8 {
    hid::key_code_from_name(matrix::key_name(pos)).unwrap_or(0)
}

// ── hid::key_code_from_name resolution (still useful for display) ──

#[test]
fn hid_key_code_standard_keys() {
    assert_eq!(hid::key_code_from_name("Esc"), Some(0x29));
    assert_eq!(hid::key_code_from_name("Tab"), Some(0x2B));
    assert_eq!(hid::key_code_from_name("Caps"), Some(0x39));
    assert_eq!(hid::key_code_from_name("A"), Some(0x04));
    assert_eq!(hid::key_code_from_name("Z"), Some(0x1D));
    assert_eq!(hid::key_code_from_name("F1"), Some(0x3A));
    assert_eq!(hid::key_code_from_name("F12"), Some(0x45));
    assert_eq!(hid::key_code_from_name("Del"), Some(0x4C));
    assert_eq!(hid::key_code_from_name("Home"), Some(0x4A));
}

#[test]
fn hid_key_code_abbreviations() {
    assert_eq!(hid::key_code_from_name("Ent"), Some(0x28));
    assert_eq!(hid::key_code_from_name("Bksp"), Some(0x2A));
    assert_eq!(hid::key_code_from_name("LShf"), Some(0xE1));
    assert_eq!(hid::key_code_from_name("RShf"), Some(0xE5));
    assert_eq!(hid::key_code_from_name("LCtl"), Some(0xE0));
    assert_eq!(hid::key_code_from_name("RCtl"), Some(0xE4));
    assert_eq!(hid::key_code_from_name("Win"), Some(0xE3));
    assert_eq!(hid::key_code_from_name("LAlt"), Some(0xE2));
    assert_eq!(hid::key_code_from_name("RAlt"), Some(0xE6));
}

// ── Byte-pattern detection ──

#[test]
fn factory_default_not_detected() {
    // [0, 0, keycode, 0] where keycode matches factory default → NOT a remap
    assert!(!is_user_remap(&[0, 0, 0x29, 0], 0x29)); // Escape at Escape position
    assert!(!is_user_remap(&[0, 0, 0x04, 0], 0x04)); // A at A position
    assert!(!is_user_remap(&[0, 0, 0xE1, 0], 0xE1)); // LShift at LShift position
}

#[test]
fn factory_default_changed_detected() {
    // [0, 0, new_code, 0] where new_code differs from factory default → IS a remap
    assert!(is_user_remap(&[0, 0, 0x04, 0], 0x39)); // A at Caps position (default 0x39)
    assert!(is_user_remap(&[0, 0, 0x29, 0], 0x35)); // Escape at ` position (default 0x35)
}

#[test]
fn identity_map_not_detected_any_layer() {
    // Both base layer and Fn layer use the same defaults: [0, 0, default_code, 0]
    // An identity map should NOT be detected on either layer.
    assert!(!is_user_remap(&[0, 0, 0x3A, 0], 0x3A)); // F1 → F1 (identity)
    assert!(!is_user_remap(&[0, 0, 0x04, 0], 0x04)); // A → A (identity)
}

#[test]
fn disabled_not_detected() {
    assert!(!is_user_remap(&[0, 0, 0, 0], 0x29));
    assert!(!is_user_remap(&[0, 0, 0, 0], 0));
}

#[test]
fn user_remap_detected() {
    // [0, keycode, 0, 0] — user remap format, byte1 non-zero
    assert!(is_user_remap(&[0, 0x04, 0, 0], 0x39)); // remapped to A
    assert!(is_user_remap(&[0, 0x29, 0, 0], 0x35)); // remapped to Escape
}

#[test]
fn combo_detected() {
    // [0, mods, key, 0] — both bytes 1 and 2 non-zero
    assert!(is_user_remap(&[0, 0x01, 0x06, 0], 0x39)); // Ctrl+C
    assert!(is_user_remap(&[0, 0x04, 0x29, 0], 0x35)); // Alt+Escape
}

#[test]
fn macro_detected() {
    // config_type 9 = macro
    assert!(is_user_remap(&[9, 0, 0, 0], 0xE0)); // Macro(0)
    assert!(is_user_remap(&[9, 0, 2, 0], 0xE0)); // Macro(2)
}

#[test]
fn mouse_detected() {
    // config_type 1 = mouse
    assert!(is_user_remap(&[1, 0, 1, 0], 0xE0)); // Mouse1
}

// ── Default resolution from transport matrix ──

#[test]
fn default_for_matches_transport_matrix() {
    // Verify default_for() resolves key names from the transport matrix.
    // Col 0 (positions 0-5)
    assert_eq!(default_for(0), 0x29); // Esc
    assert_eq!(default_for(1), 0x35); // `
    assert_eq!(default_for(2), 0x2B); // Tab
    assert_eq!(default_for(3), 0x39); // Caps
    assert_eq!(default_for(4), 0xE1); // LShf
    assert_eq!(default_for(5), 0xE0); // LCtl
                                      // F-row and modifier positions (verified against firmware GET_KEYMATRIX)
    assert_eq!(default_for(6), 0x3A); // F1 at Col 1 Row 0
    assert_eq!(default_for(11), 0xE3); // Win at Col 1 Row 5
    assert_eq!(default_for(12), 0x3B); // F2 at Col 2 Row 0
    assert_eq!(default_for(17), 0xE2); // LAlt at Col 2 Row 5
    assert_eq!(default_for(41), 0x2C); // Space at Col 6 Row 5
    assert_eq!(default_for(59), 0xE6); // RAlt at Col 9 Row 5
    assert_eq!(default_for(71), 0xE4); // RCtl at Col 11 Row 5
    assert_eq!(default_for(78), 0x4C); // Del at Col 13 Row 0
}

// ── KeyAction parsing from wire bytes ──

#[test]
fn parse_factory_default_format() {
    // [0, 0, code, 0] — keycode at byte 2
    assert_eq!(
        KeyAction::from_config_bytes([0, 0, 0x29, 0]),
        KeyAction::Key(0x29)
    );
}

#[test]
fn parse_user_remap_format() {
    // [0, code, 0, 0] — keycode at byte 1
    assert_eq!(
        KeyAction::from_config_bytes([0, 0x04, 0, 0]),
        KeyAction::Key(0x04)
    );
    assert_eq!(
        KeyAction::from_config_bytes([0, 0x29, 0, 0]),
        KeyAction::Key(0x29)
    );
}

#[test]
fn parse_combo_format() {
    // [0, mods, key, 0]
    assert_eq!(
        KeyAction::from_config_bytes([0, 0x01, 0x06, 0]),
        KeyAction::Combo {
            mods: 0x01,
            key: 0x06
        }
    );
}

#[test]
fn parse_disabled() {
    assert_eq!(
        KeyAction::from_config_bytes([0, 0, 0, 0]),
        KeyAction::Disabled
    );
}

// ── Simulated full matrix scan (byte-pattern detection) ──

#[test]
fn simulated_remap_scan() {
    // Build a fake keymatrix buffer using correct firmware byte formats.
    // Factory defaults derived from transport matrix::key_name → hid::key_code_from_name.
    let key_count = 6;
    let defaults: Vec<u8> = (0..key_count as u8).map(default_for).collect();

    let mut data = vec![0u8; key_count * 4];

    // Position 0: Esc — factory default [0, 0, 0x29, 0] → NOT a remap
    data[0..4].copy_from_slice(&[0, 0, 0x29, 0]);
    // Position 1: ` — factory default [0, 0, 0x35, 0] → NOT a remap
    data[4..8].copy_from_slice(&[0, 0, 0x35, 0]);
    // Position 2: Tab — factory default [0, 0, 0x2B, 0] → NOT a remap
    data[8..12].copy_from_slice(&[0, 0, 0x2B, 0]);
    // Position 3: Caps — remapped to A [0, 0, 0x04, 0] → REMAP (0x04 != default 0x39)
    data[12..16].copy_from_slice(&[0, 0, 0x04, 0]);
    // Position 4: LShf — factory default [0, 0, 0xE1, 0] → NOT a remap
    data[16..20].copy_from_slice(&[0, 0, 0xE1, 0]);
    // Position 5: LCtl — macro assignment [9, 0, 0, 0] → REMAP
    data[20..24].copy_from_slice(&[9, 0, 0, 0]);

    let mut remaps = Vec::new();
    for i in 0..key_count {
        let name = matrix::key_name(i as u8);
        if name == "?" {
            continue;
        }
        let k: [u8; 4] = [
            data[i * 4],
            data[i * 4 + 1],
            data[i * 4 + 2],
            data[i * 4 + 3],
        ];

        if is_user_remap(&k, defaults[i]) {
            let action = KeyAction::from_config_bytes(k);
            remaps.push((i, name, action));
        }
    }

    assert_eq!(remaps.len(), 2);
    assert_eq!(remaps[0], (3, "Caps", KeyAction::Key(0x04))); // A
    assert_eq!(
        remaps[1],
        (5, "LCtl", KeyAction::Macro { index: 0, kind: 0 })
    );
}

#[test]
fn simulated_remap_byte2_format() {
    // Tests the critical case: firmware stores remap as [0, 0, new_code, 0]
    // which is the same byte format as factory defaults.
    // Only detectable by comparing against the factory default keycode.
    let key_count = 6;
    let defaults: Vec<u8> = (0..key_count as u8).map(default_for).collect();

    let mut data = vec![0u8; key_count * 4];

    // Position 0: Esc → remapped to Tab [0, 0, 0x2B, 0] — REMAP (0x2B != 0x29)
    data[0..4].copy_from_slice(&[0, 0, 0x2B, 0]);
    // Position 1: ` → factory default [0, 0, 0x35, 0] — NOT a remap
    data[4..8].copy_from_slice(&[0, 0, 0x35, 0]);
    // Position 2: Tab → remapped to Esc [0, 0, 0x29, 0] — REMAP (0x29 != 0x2B)
    data[8..12].copy_from_slice(&[0, 0, 0x29, 0]);
    // Position 3: Caps → remapped to A [0, 0, 0x04, 0] — REMAP (0x04 != 0x39)
    data[12..16].copy_from_slice(&[0, 0, 0x04, 0]);
    // Position 4: LShf → factory default [0, 0, 0xE1, 0] — NOT a remap
    data[16..20].copy_from_slice(&[0, 0, 0xE1, 0]);
    // Position 5: LCtl → factory default [0, 0, 0xE0, 0] — NOT a remap
    data[20..24].copy_from_slice(&[0, 0, 0xE0, 0]);

    let mut remaps = Vec::new();
    for i in 0..key_count {
        let name = matrix::key_name(i as u8);
        if name == "?" {
            continue;
        }
        let k: [u8; 4] = [
            data[i * 4],
            data[i * 4 + 1],
            data[i * 4 + 2],
            data[i * 4 + 3],
        ];

        if is_user_remap(&k, defaults[i]) {
            let action = KeyAction::from_config_bytes(k);
            remaps.push((i, name, action));
        }
    }

    assert_eq!(remaps.len(), 3);
    assert_eq!(remaps[0], (0, "Esc", KeyAction::Key(0x2B))); // Tab
    assert_eq!(remaps[1], (2, "Tab", KeyAction::Key(0x29))); // Escape
    assert_eq!(remaps[2], (3, "Caps", KeyAction::Key(0x04))); // A
}

#[test]
fn simulated_layer1_identity_maps_filtered() {
    // Layer 1 also stores identity maps [0, 0, default_code, 0] for each key.
    // These should be filtered out the same as layer 0.
    let key_count = 6;
    let defaults: Vec<u8> = (0..key_count as u8).map(default_for).collect();

    let mut data = vec![0u8; key_count * 4];

    // All positions: identity maps (same code as default)
    for i in 0..key_count {
        data[i * 4..i * 4 + 4].copy_from_slice(&[0, 0, defaults[i], 0]);
    }

    let mut remaps = Vec::new();
    for i in 0..key_count {
        let name = matrix::key_name(i as u8);
        if name == "?" {
            continue;
        }
        let k: [u8; 4] = [
            data[i * 4],
            data[i * 4 + 1],
            data[i * 4 + 2],
            data[i * 4 + 3],
        ];

        if is_user_remap(&k, defaults[i]) {
            let action = KeyAction::from_config_bytes(k);
            remaps.push((i, name, action));
        }
    }

    assert_eq!(
        remaps.len(),
        0,
        "Identity maps should be filtered on both layers"
    );
}

#[test]
fn simulated_fn_layer_scan() {
    // Fn layer: a mix of disabled (identity maps filtered), non-zero config types,
    // and user byte1 remaps.
    let key_count = 6;
    let defaults: Vec<u8> = (0..key_count as u8).map(default_for).collect();

    let mut data = vec![0u8; key_count * 4];

    // Position 0: Disabled [0, 0, 0, 0] → NOT a remap
    data[0..4].copy_from_slice(&[0, 0, 0, 0]);
    // Position 1: Identity map [0, 0, 0x35, 0] → NOT a remap (same as default)
    data[4..8].copy_from_slice(&[0, 0, defaults[1], 0]);
    // Position 2: Macro(1) [9, 0, 1, 0] → REMAP (config_type != 0)
    data[8..12].copy_from_slice(&[9, 0, 1, 0]);
    // Position 3: Mouse1 [1, 0, 1, 0] → REMAP (config_type != 0)
    data[12..16].copy_from_slice(&[1, 0, 1, 0]);
    // Position 4: Identity map [0, 0, 0xE1, 0] → NOT a remap (same as default)
    data[16..20].copy_from_slice(&[0, 0, defaults[4], 0]);
    // Position 5: User remap to F13 [0, 0x68, 0, 0] → REMAP (byte1 != 0)
    data[20..24].copy_from_slice(&[0, 0x68, 0, 0]);

    let mut remaps = Vec::new();
    for i in 0..key_count {
        let name = matrix::key_name(i as u8);
        if name == "?" {
            continue;
        }
        let k: [u8; 4] = [
            data[i * 4],
            data[i * 4 + 1],
            data[i * 4 + 2],
            data[i * 4 + 3],
        ];

        if is_user_remap(&k, defaults[i]) {
            let action = KeyAction::from_config_bytes(k);
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

// ── Fn key false-positive fix ──

#[test]
fn fn_key_not_detected_as_remap() {
    // [10, 1, 0, 0] = Fn key — factory default at the physical Fn position
    assert!(!is_user_remap(&[10, 1, 0, 0], 0xE4));
    assert!(!is_user_remap(&[10, 1, 0, 0], 0));
}

// ── Consumer / LED control parsing (Fn layer entries) ──

#[test]
fn parse_consumer_volume_up() {
    let action = KeyAction::from_config_bytes([3, 0, 0xE9, 0]);
    assert_eq!(action, KeyAction::Consumer(0x00E9));
    assert_eq!(action.to_string(), "Volume Up");
}

#[test]
fn parse_consumer_calculator() {
    // 146 + 1*256 = 402 = 0x192
    let action = KeyAction::from_config_bytes([3, 0, 0x92, 0x01]);
    assert_eq!(action, KeyAction::Consumer(0x0192));
    assert_eq!(action.to_string(), "Calculator");
}

#[test]
fn parse_led_brightness() {
    let action = KeyAction::from_config_bytes([13, 2, 1, 0]);
    assert_eq!(action, KeyAction::LedControl { data: [2, 1, 0] });
    assert_eq!(action.to_string(), "LED Brightness Up");
}

#[test]
fn simulated_fn_layer_with_consumer_and_led() {
    // Simulated Fn layer data with consumer keys and LED controls
    let key_count = 6;
    let mut data = vec![0u8; key_count * 4];

    // Position 0: Disabled → skip
    data[0..4].copy_from_slice(&[0, 0, 0, 0]);
    // Position 1: Consumer Volume Up [3, 0, 0xE9, 0]
    data[4..8].copy_from_slice(&[3, 0, 0xE9, 0]);
    // Position 2: Consumer Play/Pause [3, 0, 0xCD, 0]
    data[8..12].copy_from_slice(&[3, 0, 0xCD, 0]);
    // Position 3: LED Brightness Up [13, 2, 1, 0]
    data[12..16].copy_from_slice(&[13, 2, 1, 0]);
    // Position 4: Unknown type 14 [14, 1, 0, 0]
    data[16..20].copy_from_slice(&[14, 1, 0, 0]);
    // Position 5: Fn key [10, 1, 0, 0] — should NOT be detected as remap
    data[20..24].copy_from_slice(&[10, 1, 0, 0]);

    let mut entries = Vec::new();
    for i in 0..key_count {
        let k: [u8; 4] = [
            data[i * 4],
            data[i * 4 + 1],
            data[i * 4 + 2],
            data[i * 4 + 3],
        ];
        if k == [0, 0, 0, 0] {
            continue;
        }
        let action = KeyAction::from_config_bytes(k);
        entries.push((i, action));
    }

    // 5 non-zero entries: Consumer x2, LedControl, Unknown(14), Fn
    assert_eq!(entries.len(), 5);
    assert_eq!(entries[0].1, KeyAction::Consumer(0x00E9));
    assert_eq!(entries[0].1.to_string(), "Volume Up");
    assert_eq!(entries[1].1, KeyAction::Consumer(0x00CD));
    assert_eq!(entries[1].1.to_string(), "Play/Pause");
    assert!(matches!(entries[2].1, KeyAction::LedControl { .. }));
    assert_eq!(entries[2].1.to_string(), "LED Brightness Up");
    assert!(matches!(
        entries[3].1,
        KeyAction::Unknown {
            config_type: 14,
            ..
        }
    ));
    assert_eq!(entries[4].1, KeyAction::Fn);
}
