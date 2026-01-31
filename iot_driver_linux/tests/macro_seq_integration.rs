//! Integration tests for the macro sequence pipeline.
//!
//! These test the full public API: parsing sequences, expanding to wire
//! events, reconstructing from events, and displaying — exercising the
//! boundary between `macro_seq`, `key_action`, and `protocol::hid`.

use iot_driver::key_action::mods;
use iot_driver::macro_seq::{MacroSeq, MacroStep};

// ── Full pipeline: parse → expand → reconstruct → display ──

#[test]
fn pipeline_simple_text_like_sequence() {
    // Simulates what "set-macro 0 --seq A,B,C" would do
    let input = "A,B,C";
    let mut seq: MacroSeq = input.parse().unwrap();
    seq.default_delay = 10;
    seq.repeat = 1;

    let events = seq.to_events();
    // 3 taps × 2 events each = 6 events
    assert_eq!(events.len(), 6);

    // Verify wire format: each tap is ↓key(10ms) ↑key(10ms)
    assert_eq!(events[0], (0x04, true, 10)); // ↓A
    assert_eq!(events[1], (0x04, false, 10)); // ↑A
    assert_eq!(events[2], (0x05, true, 10)); // ↓B
    assert_eq!(events[3], (0x05, false, 10)); // ↑B
    assert_eq!(events[4], (0x06, true, 10)); // ↓C
    assert_eq!(events[5], (0x06, false, 10)); // ↑C

    // Reconstruct and verify display matches input
    let reconstructed = MacroSeq::from_events(&events, 10, 1);
    assert_eq!(reconstructed.to_string(), input);
}

#[test]
fn pipeline_copy_paste_combos() {
    // Ctrl+A, Ctrl+C, Ctrl+V — select all, copy, paste
    let input = "Ctrl+A,Ctrl+C,Ctrl+V";
    let seq: MacroSeq = input.parse().unwrap();
    let events = seq.to_events();

    // Each combo: ↓LCtrl(0) ↓key(10) ↑key(0) ↑LCtrl(10) = 4 events × 3 = 12
    assert_eq!(events.len(), 12);

    // First combo: Ctrl+A
    assert_eq!(events[0], (0xE0, true, 0)); // ↓LCtrl
    assert_eq!(events[1], (0x04, true, 10)); // ↓A
    assert_eq!(events[2], (0x04, false, 0)); // ↑A
    assert_eq!(events[3], (0xE0, false, 10)); // ↑LCtrl

    let reconstructed = MacroSeq::from_events(&events, 10, 1);
    assert_eq!(reconstructed.to_string(), input);
}

#[test]
fn pipeline_custom_delay() {
    // set-macro 0 --seq "A,B,C" --delay 50
    let input = "A,B,C";
    let mut seq: MacroSeq = input.parse().unwrap();
    seq.default_delay = 50;

    let events = seq.to_events();
    // All delays should be 50ms
    for &(_, _, delay) in &events {
        assert_eq!(delay, 50);
    }

    let reconstructed = MacroSeq::from_events(&events, 50, 1);
    assert_eq!(reconstructed.to_string(), input);
}

#[test]
fn pipeline_mixed_delays() {
    // Per-token delays mixed with defaults
    let input = "A(50ms),B,C(200ms)";
    let mut seq: MacroSeq = input.parse().unwrap();
    seq.default_delay = 10;

    let events = seq.to_events();
    assert_eq!(events.len(), 6);

    // A tap: ↓A(10) ↑A(50)  — paren delay on release
    assert_eq!(events[0], (0x04, true, 10));
    assert_eq!(events[1], (0x04, false, 50));
    // B tap: ↓B(10) ↑B(10)  — default
    assert_eq!(events[2], (0x05, true, 10));
    assert_eq!(events[3], (0x05, false, 10));
    // C tap: ↓C(10) ↑C(200)
    assert_eq!(events[4], (0x06, true, 10));
    assert_eq!(events[5], (0x06, false, 200));

    let reconstructed = MacroSeq::from_events(&events, 10, 1);
    assert_eq!(reconstructed.to_string(), input);
}

#[test]
fn pipeline_standalone_delay_override() {
    // Standalone delay overrides the preceding event's delay
    let input = "A,100ms,B";
    let seq: MacroSeq = input.parse().unwrap();
    let events = seq.to_events();

    assert_eq!(events.len(), 4);
    // ↑A should have 100ms (overridden), not 10ms
    assert_eq!(events[0], (0x04, true, 10));
    assert_eq!(events[1], (0x04, false, 100));
    assert_eq!(events[2], (0x05, true, 10));
    assert_eq!(events[3], (0x05, false, 10));
}

#[test]
fn pipeline_explicit_press_release_hold_pattern() {
    // Hold A for 550ms (game-style hold key)
    let input = "A:Press,550ms,A:Release";
    let seq: MacroSeq = input.parse().unwrap();
    let events = seq.to_events();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0], (0x04, true, 550)); // ↓A, delay overridden to 550ms
    assert_eq!(events[1], (0x04, false, 10)); // ↑A
}

#[test]
fn pipeline_modifier_hold_pattern() {
    // Ctrl:Press, A:Press, A:Release, Ctrl:Release — manual combo
    let input = "Ctrl:Press,A:Press,A:Release,Ctrl:Release";
    let seq: MacroSeq = input.parse().unwrap();
    let events = seq.to_events();

    assert_eq!(events.len(), 4);
    assert_eq!(events[0], (0xE0, true, 10)); // ↓LCtrl
    assert_eq!(events[1], (0x04, true, 10)); // ↓A
    assert_eq!(events[2], (0x04, false, 10)); // ↑A
    assert_eq!(events[3], (0xE0, false, 10)); // ↑LCtrl
}

#[test]
fn pipeline_multi_modifier_combo() {
    let input = "Ctrl+Shift+Alt+A";
    let seq: MacroSeq = input.parse().unwrap();

    assert_eq!(seq.steps.len(), 1);
    match &seq.steps[0] {
        MacroStep::TapCombo { mods: m, key, .. } => {
            assert_eq!(*m, mods::LCTRL | mods::LSHIFT | mods::LALT);
            assert_eq!(*key, 0x04);
        }
        other => panic!("expected TapCombo, got {other:?}"),
    }

    let events = seq.to_events();
    // 3 mod downs + key down + key up + 3 mod ups = 8
    assert_eq!(events.len(), 8);

    let reconstructed = MacroSeq::from_events(&events, 10, 1);
    assert_eq!(reconstructed.to_string(), input);
}

#[test]
fn pipeline_combo_with_delay_roundtrip() {
    let input = "Ctrl+A(50ms),100ms,Ctrl+C(200ms)";
    let seq: MacroSeq = input.parse().unwrap();
    let events = seq.to_events();

    // First combo: ↓LCtrl(0) ↓A(10) ↑A(0) ↑LCtrl(50) — then 100ms override on LCtrl
    // The standalone 100ms overrides ↑LCtrl delay from 50 to 100
    assert_eq!(events[3], (0xE0, false, 100)); // overridden

    // Second combo ends with ↑LCtrl(200)
    let last = events.last().unwrap();
    assert_eq!(last.2, 200);
}

// ── from_events reconstruction from "foreign" event data ──

#[test]
fn reconstruct_text_macro_events() {
    // Simulate what set_text_macro("hi", 10, 1) produces:
    // 'h' = 0x0B, 'i' = 0x0C
    let events = vec![
        (0x0B, true, 10),  // ↓H
        (0x0B, false, 10), // ↑H
        (0x0C, true, 10),  // ↓I
        (0x0C, false, 10), // ↑I
    ];
    let seq = MacroSeq::from_events(&events, 10, 1);
    assert_eq!(seq.to_string(), "H,I");
}

#[test]
fn reconstruct_shifted_text_macro_events() {
    // Simulate "Hi" — uppercase H requires LShift
    // LShift = 0xE1
    let events = vec![
        (0xE1, true, 0),   // ↓LShift
        (0x0B, true, 10),  // ↓H
        (0x0B, false, 0),  // ↑H
        (0xE1, false, 10), // ↑LShift
        (0x0C, true, 10),  // ↓I
        (0x0C, false, 10), // ↑I
    ];
    let seq = MacroSeq::from_events(&events, 10, 1);
    assert_eq!(seq.to_string(), "Shift+H,I");
}

// ── Error cases ──

#[test]
fn parse_error_empty() {
    assert!("".parse::<MacroSeq>().is_err());
}

#[test]
fn parse_error_unknown_key() {
    assert!("Ctrl+Foobar".parse::<MacroSeq>().is_err());
}

#[test]
fn parse_error_bad_direction() {
    assert!("A:Sideways".parse::<MacroSeq>().is_err());
}

#[test]
fn parse_error_bad_delay() {
    assert!("A(notanumber)".parse::<MacroSeq>().is_err());
}

// ── Edge cases ──

#[test]
fn empty_events_reconstruction() {
    let seq = MacroSeq::from_events(&[], 10, 1);
    assert!(seq.steps.is_empty());
    assert_eq!(seq.to_string(), "");
}

#[test]
fn single_event_reconstruction() {
    // A lone key-down with no matching key-up
    let events = vec![(0x04, true, 10)];
    let seq = MacroSeq::from_events(&events, 10, 1);
    assert_eq!(seq.steps.len(), 1);
    assert_eq!(seq.to_string(), "A:Press");
}

#[test]
fn many_taps_roundtrip() {
    let input = "A,B,C,D,E,F,G,H,I,J,K,L,M,N,O,P";
    let seq: MacroSeq = input.parse().unwrap();
    let events = seq.to_events();
    assert_eq!(events.len(), 32); // 16 taps × 2
    let reconstructed = MacroSeq::from_events(&events, 10, 1);
    assert_eq!(reconstructed.to_string(), input);
}

#[test]
fn function_key_sequence() {
    let input = "F1,F5,F12";
    let seq: MacroSeq = input.parse().unwrap();
    let events = seq.to_events();
    let reconstructed = MacroSeq::from_events(&events, 10, 1);
    assert_eq!(reconstructed.to_string(), input);
}
