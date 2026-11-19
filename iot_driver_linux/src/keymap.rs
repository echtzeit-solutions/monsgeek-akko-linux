//! Unified keymap abstraction for CLI and TUI.
//!
//! Provides shared types (`KeyEntry`, `KeyMap`) and I/O helpers (`load`, `set_key`,
//! `reset_key`) so that both the CLI and TUI share identical parsing, filtering,
//! and writing logic.
//!
//! `Layer` and `KeyRef` live in `monsgeek_transport::protocol` and are re-exported here.

use crate::key_action::KeyAction;
use crate::protocol::hid;
use monsgeek_transport::protocol::{HidUsage, KeymatrixLayer, MatrixPos, matrix};

use monsgeek_keyboard::{DksConfig, KeyMode, KeyboardError, ModeByte, RyKeyboard, SNAPTAP_UNBOUND};

// Re-export from monsgeek-transport so existing `use crate::keymap::{Layer, KeyRef}` still works.
pub use monsgeek_transport::protocol::{KeyRef, Layer};

// ---------------------------------------------------------------------------
// Remap detection (shared logic)
// ---------------------------------------------------------------------------

/// Detect whether a 4-byte key config represents a user remap.
///
/// `default_hid_code`: the factory default HID keycode for this matrix position,
/// derived from `hid::key_code_from_name(matrix::key_name(i))`.
pub fn is_user_remap(k: &[u8], default_hid_code: HidUsage) -> bool {
    let Ok(bytes) = <[u8; 4]>::try_from(k) else {
        return false;
    };
    match KeyAction::from_config_bytes(bytes) {
        // An all-zero slot is how the overlay layers spell "transparent", and the
        // Fn key sits at its own physical position by default.
        KeyAction::Disabled | KeyAction::Fn => false,
        // Comparing the decoded action, not raw bytes, means a slot written to the
        // position's own factory keycode reads as unmodified whichever usage slot
        // it landed in.
        action => action != KeyAction::Key(default_hid_code),
    }
}

// ---------------------------------------------------------------------------
// I/O: loading
// ---------------------------------------------------------------------------

/// Number of pages to read for a full key matrix (126 positions × 4 bytes = 504).
const KEYMATRIX_PAGES: usize = 8;

/// Positions in the firmware's keymatrix array. The 8 pages we read cover 128;
/// the last two are past the end of the array.
const MATRIX_POSITIONS: usize = 126;

// ---------------------------------------------------------------------------
// I/O: writing
// ---------------------------------------------------------------------------

/// Write a key config via RyKeyboard.
pub fn set_key(
    kb: &RyKeyboard,
    index: MatrixPos,
    layer: Layer,
    action: &KeyAction,
) -> Result<(), KeyboardError> {
    let config = action.to_config_bytes();
    match layer.keymatrix_layer() {
        Some(km) => kb.set_keymatrix_config(kb.active_profile(), index.get(), km, config, true),
        None => kb.set_fn_config(kb.active_profile(), index.get(), config),
    }
}

/// Factory-default HID keycode for a matrix position, derived from the *generic*
/// matrix name table.
///
/// Only correct for boards that match that table. Prefer
/// [`device_default_keycode`], which consults the device's own layout first.
pub fn default_keycode(pos: MatrixPos) -> HidUsage {
    hid::key_code_from_name(matrix::key_name(pos)).unwrap_or(HidUsage::NONE)
}

/// Factory-default HID keycode for a matrix position on *this* device.
///
/// The device database carries a per-model default table; without it a board whose
/// layout differs from the generic one has every differing key read as customised
/// (the Womier SK75 has LMeta where the generic table has LAlt, and End where it
/// has Home).
pub fn device_default_keycode(kb: &RyKeyboard, pos: MatrixPos) -> HidUsage {
    kb.matrix_default(pos.into())
        .map(HidUsage::new)
        .unwrap_or_else(|| default_keycode(pos))
}

/// Reset a key to its firmware default.
///
/// The base layer has **no ROM fallback** (firmware-confirmed): an all-zero
/// keymatrix entry emits keycode 0 and *silences* the key. So the base layer is
/// reset by writing the position's factory-default keycode, not zeros. The overlay
/// layers (Layer1 / Fn) treat a zero entry as a transparent fall-through to the
/// base, so zeros are the correct "default" there.
pub fn reset_key(kb: &RyKeyboard, index: MatrixPos, layer: Layer) -> Result<(), KeyboardError> {
    match layer {
        // The active profile, not 0: every other write in this module targets the
        // profile the board is running, and a base-layer reset must match.
        Layer::Base => kb.set_keymatrix(
            kb.active_profile(),
            index.get(),
            device_default_keycode(kb, index).get(),
            true,
            KeymatrixLayer::BASE,
        ),
        Layer::Layer1 | Layer::Fn => kb.reset_key(layer, index.get()),
    }
}

// ---------------------------------------------------------------------------
// KeyRow — unified per-key config (keymatrix + magnetism), for the Key Mapping tab
// ---------------------------------------------------------------------------

/// A physical key's complete configuration, fused across the keymatrix table
/// (outputs across all layers) and the magnetism table (mode + travel +
/// mode-specific values). Backs the "Key Mapping" TUI tab.
#[derive(Debug, Clone)]
pub struct KeyRow {
    pub index: MatrixPos,
    /// Display name, device-resolved where the profile or matrix DB names it.
    pub position: String,
    /// Base mode (magnetism subcmd 7). Reinterprets the keymatrix layers:
    /// Normal → `outputs[0]` is the key; DKS → `outputs[0..4]` are the combo slots.
    pub mode: KeyMode,
    pub rapid_trigger: bool,
    // Magnetism travel values, raw u16 (device precision).
    pub actuation: u16,
    pub release: u16,
    pub rt_press: u16,
    pub rt_lift: u16,
    pub bottom_dz: u16,
    pub top_dz: u16,
    /// Keymatrix output per layer 0–3 (in DKS mode: the four combo slots).
    pub outputs: [KeyAction; 4],
    /// Whether each keymatrix layer differs from its factory default.
    pub output_remapped: [bool; 4],
    /// Fn-layer binding (separate table), if non-empty.
    pub fn_action: Option<KeyAction>,
    /// Raw keymatrix bytes per layer, exactly as the device holds them. Kept
    /// because re-encoding an action is lossy: `[0,0x29,0,0]` and `[0,0,0x29,0]`
    /// both decode to `Key(0x29)`.
    pub raw: [[u8; 4]; 4],
    /// Raw Fn-table bytes, `None` when the Fn read failed.
    pub fn_raw: Option<[u8; 4]>,
    /// DKS trigger-point travel, raw u16.
    pub dks_travel: u16,
    /// DKS packed binding-row bytes (4 × 2-bit phase actions each).
    pub dks_modes: [u8; 4],
    /// Mod-Tap decision time (ms).
    pub modtap_ms: u16,
    /// Snap-Tap partner key index, if bound.
    pub snaptap_partner: Option<u8>,
}

impl KeyRow {
    /// True when the key differs from a plain factory default (any layer remapped,
    /// a non-Normal mode, RT enabled, or an Fn binding).
    pub fn is_customized(&self) -> bool {
        self.mode != KeyMode::Normal
            || self.rapid_trigger
            || self.output_remapped.iter().any(|&b| b)
            || self.fn_action.is_some()
    }
}

/// Load the fused per-key rows for every physical key. All reads are bulk (no
/// per-key round-trips); mode-specific tables tolerate failure on older firmware.
///
/// `sys` selects the Fn table's OS variant (0 = Windows, 1 = Mac).
/// How far a [`load_key_rows`] call has got, for progress display.
///
/// `done`/`total` count the loader's steps, not device pages — the point is a bar
/// that moves and a label that says which of the seven tables is on the wire. For
/// motion inside a step, sample [`RyKeyboard::queries_completed`], which
/// ticks once per page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadStage {
    pub done: usize,
    pub total: usize,
    pub label: &'static str,
}

impl std::fmt::Display for LoadStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}/{})", self.label, self.done + 1, self.total)
    }
}

/// Steps `load_key_rows` reports, in order.
const LOAD_STAGES: [&str; 7] = [
    "keymatrix layers",
    "Fn layer",
    "trigger settings",
    "DKS travel",
    "DKS modes",
    "mod-tap times",
    "SnapTap bindings",
];

pub fn load_key_rows(
    kb: &RyKeyboard,
    sys: u8,
    on_stage: &dyn Fn(LoadStage),
) -> Result<Vec<KeyRow>, KeyboardError> {
    let stage = |i: usize| {
        on_stage(LoadStage {
            done: i,
            total: LOAD_STAGES.len(),
            label: LOAD_STAGES[i],
        })
    };

    // Every position the firmware's keymatrix array holds, not just the ones the
    // device database names. The encoder's mode toggle sits at 92 and its
    // alternate rotation bindings at 96/97 — all unnamed, and all invisible while
    // this walked `0..matrix_positions()`.
    let key_count = MATRIX_POSITIONS;

    // Keymatrix layers 0–3 (outputs / DKS combos) + the separate Fn table.
    let profile = kb.active_profile();
    stage(0);
    let layers: [Vec<u8>; 4] = [
        kb.get_keymatrix(profile, KeymatrixLayer::ALL[0], KEYMATRIX_PAGES)?,
        kb.get_keymatrix(profile, KeymatrixLayer::ALL[1], KEYMATRIX_PAGES)?,
        kb.get_keymatrix(profile, KeymatrixLayer::ALL[2], KEYMATRIX_PAGES)?,
        kb.get_keymatrix(profile, KeymatrixLayer::ALL[3], KEYMATRIX_PAGES)?,
    ];
    stage(1);
    let fn_layer = kb.get_fn_keymatrix(profile, sys, KEYMATRIX_PAGES).ok();

    // Magnetism table + mode-specific bulk reads.
    stage(2);
    let trig = kb.get_all_triggers()?;
    stage(3);
    let dks_travels = kb.get_dks_travels().unwrap_or_default();
    stage(4);
    let dks_blob = kb.get_dks_trigger_modes_blob().unwrap_or_default();
    stage(5);
    let modtap = kb.get_modtap_times().unwrap_or_default();
    stage(6);
    let snaptap = kb.get_snaptap_binds().unwrap_or_default();

    Ok(build_key_rows(&RawKeyRows {
        key_count,
        names: (0..key_count)
            .map(|i| {
                // Device-resolved names first (profile merged with the matrix DB),
                // falling back to the generic table. Without this a board's own keys
                // — the M1 V5's volume encoder at 90/91, say — read as unnamed.
                // "?" is the no-name marker in both tables, so it must not win here
                // or the row gets an unaddressable label.
                let device = kb.matrix_key_name(i);
                if !device.is_empty() && device != "?" {
                    return device.to_string();
                }
                match matrix::key_name(MatrixPos::new(i as u8)) {
                    "?" => format!("#{i}"),
                    generic => generic.to_string(),
                }
            })
            .collect(),
        defaults: (0..key_count)
            .map(|i| device_default_keycode(kb, MatrixPos::new(i as u8)))
            .collect(),
        layers,
        fn_layer,
        triggers: trig,
        dks_travels,
        dks_blob,
        modtap,
        snaptap,
    }))
}

/// Everything `build_key_rows` needs, so the assembly can be tested without a device.
pub struct RawKeyRows {
    pub key_count: usize,
    /// Display name per matrix position; empty means "no physical key here".
    pub names: Vec<String>,
    /// Factory keycode per matrix position, for deciding what counts as customised.
    pub defaults: Vec<HidUsage>,
    /// Keymatrix layers 0-3.
    pub layers: [Vec<u8>; 4],
    pub fn_layer: Option<Vec<u8>>,
    pub triggers: monsgeek_keyboard::TriggerSettings,
    pub dks_travels: Vec<u16>,
    pub dks_blob: Vec<u8>,
    pub modtap: Vec<u16>,
    pub snaptap: Vec<u8>,
}

/// Fuse the raw tables into one row per physical key.
pub fn build_key_rows(raw: &RawKeyRows) -> Vec<KeyRow> {
    let key_count = raw.key_count;
    let layers = &raw.layers;
    let fn_layer = &raw.fn_layer;
    let trig = &raw.triggers;
    let (dks_travels, dks_blob, modtap, snaptap) =
        (&raw.dks_travels, &raw.dks_blob, &raw.modtap, &raw.snaptap);

    let mut rows = Vec::new();
    for i in 0..key_count {
        let name = raw.names.get(i).cloned().unwrap_or_else(|| format!("#{i}"));
        // A position the database does not name is still real storage. Drop it only
        // when it is also entirely empty — those are the matrix's physical gaps.
        // Filtering on the name alone hid the encoder's mode toggle and its
        // alternate bindings, which are unnamed but very much set.
        let unnamed = name.starts_with('#');
        let empty = layers
            .iter()
            .all(|d| d.get(i * 4..i * 4 + 4).is_none_or(|k| k == [0, 0, 0, 0]))
            && fn_layer
                .as_ref()
                .and_then(|d| d.get(i * 4..i * 4 + 4))
                .is_none_or(|k| k == [0, 0, 0, 0]);
        if unnamed && empty {
            continue;
        }
        let default = raw.defaults.get(i).copied().unwrap_or(HidUsage::NONE);
        let mode_byte = ModeByte::from_u8(trig.key_modes.get(i).copied().unwrap_or(0));

        let mut outputs = [KeyAction::Disabled; 4];
        let mut output_remapped = [false; 4];
        let mut raw = [[0u8; 4]; 4];
        for (l, data) in layers.iter().enumerate() {
            if i * 4 + 4 <= data.len() {
                let k = &data[i * 4..i * 4 + 4];
                raw[l] = [k[0], k[1], k[2], k[3]];
                outputs[l] = KeyAction::from_config_bytes([k[0], k[1], k[2], k[3]]);
                // Only the base layer has a factory-default keycode; the overlay /
                // DKS layers count as "set" iff non-empty.
                output_remapped[l] = if l == 0 {
                    is_user_remap(k, default)
                } else {
                    k != [0, 0, 0, 0]
                };
            }
        }

        let fn_raw = fn_layer
            .as_ref()
            .and_then(|d| d.get(i * 4..i * 4 + 4))
            .map(|k| [k[0], k[1], k[2], k[3]]);
        let fn_action = fn_raw
            .filter(|k| k != &[0, 0, 0, 0])
            .map(KeyAction::from_config_bytes);

        let snap = snaptap.get(i).copied().unwrap_or(SNAPTAP_UNBOUND);
        rows.push(KeyRow {
            index: MatrixPos::new(i as u8),
            position: name,
            mode: mode_byte.base,
            rapid_trigger: mode_byte.rapid_trigger,
            actuation: trig.press_travel.get(i).copied().unwrap_or(0),
            release: trig.lift_travel.get(i).copied().unwrap_or(0),
            rt_press: trig.rt_press.get(i).copied().unwrap_or(0),
            rt_lift: trig.rt_lift.get(i).copied().unwrap_or(0),
            bottom_dz: trig.bottom_deadzone.get(i).copied().unwrap_or(0),
            top_dz: trig.top_deadzone.get(i).copied().unwrap_or(0),
            outputs,
            output_remapped,
            raw,
            fn_raw,
            fn_action,
            dks_travel: dks_travels.get(i).copied().unwrap_or(0),
            dks_modes: DksConfig::trigger_modes_from_blob(dks_blob, i),
            modtap_ms: modtap.get(i).copied().unwrap_or(0),
            snaptap_partner: (snap != SNAPTAP_UNBOUND && (snap as usize) < key_count)
                .then_some(snap),
        });
    }
    rows
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Layer --

    #[test]
    fn layer_parse_variants() {
        assert_eq!("0".parse::<Layer>().unwrap(), Layer::Base);
        assert_eq!("L0".parse::<Layer>().unwrap(), Layer::Base);
        assert_eq!("base".parse::<Layer>().unwrap(), Layer::Base);
        assert_eq!("1".parse::<Layer>().unwrap(), Layer::Layer1);
        assert_eq!("l1".parse::<Layer>().unwrap(), Layer::Layer1);
        assert_eq!("2".parse::<Layer>().unwrap(), Layer::Fn);
        assert_eq!("fn".parse::<Layer>().unwrap(), Layer::Fn);
        assert_eq!("FN".parse::<Layer>().unwrap(), Layer::Fn);
    }

    #[test]
    fn layer_display() {
        assert_eq!(Layer::Base.to_string(), "L0");
        assert_eq!(Layer::Layer1.to_string(), "L1");
        assert_eq!(Layer::Fn.to_string(), "Fn");
    }

    // -- KeyRef --

    #[test]
    fn keyref_parse_bare_name() {
        let kr: KeyRef = "Caps".parse().unwrap();
        assert_eq!(kr.index, MatrixPos::new(3));
        assert_eq!(kr.layer, Layer::Base);
        assert_eq!(kr.position, "Caps");
    }

    #[test]
    fn keyref_parse_fn_prefix() {
        let kr: KeyRef = "Fn+Caps".parse().unwrap();
        assert_eq!(kr.index, MatrixPos::new(3));
        assert_eq!(kr.layer, Layer::Fn);
    }

    #[test]
    fn keyref_parse_l1_prefix() {
        let kr: KeyRef = "L1+Caps".parse().unwrap();
        assert_eq!(kr.index, MatrixPos::new(3));
        assert_eq!(kr.layer, Layer::Layer1);
    }

    #[test]
    fn keyref_parse_numeric() {
        let kr: KeyRef = "42".parse().unwrap();
        assert_eq!(kr.index, MatrixPos::new(42));
        assert_eq!(kr.layer, Layer::Base);
    }

    #[test]
    fn keyref_parse_fn_numeric() {
        let kr: KeyRef = "Fn+42".parse().unwrap();
        assert_eq!(kr.index, MatrixPos::new(42));
        assert_eq!(kr.layer, Layer::Fn);
    }

    #[test]
    fn keyref_parse_case_insensitive() {
        let kr: KeyRef = "fn+caps".parse().unwrap();
        assert_eq!(kr.index, MatrixPos::new(3));
        assert_eq!(kr.layer, Layer::Fn);
    }

    #[test]
    fn keyref_display_base() {
        let kr = KeyRef::new(MatrixPos::new(3), Layer::Base);
        assert_eq!(kr.to_string(), "Caps");
    }

    #[test]
    fn keyref_display_fn() {
        let kr = KeyRef::new(MatrixPos::new(3), Layer::Fn);
        assert_eq!(kr.to_string(), "Fn+Caps");
    }

    #[test]
    fn keyref_display_l1() {
        let kr = KeyRef::new(MatrixPos::new(3), Layer::Layer1);
        assert_eq!(kr.to_string(), "L1+Caps");
    }

    // -- build_key_rows --

    /// Six positions of the real matrix (Esc ` Tab Caps LShf LCtl), so the default
    /// keycodes and names come out of the same tables the driver uses.
    fn make_raw(
        key_count: usize,
        base0: &[[u8; 4]],
        base1: &[[u8; 4]],
        fn_layer: &[[u8; 4]],
    ) -> RawKeyRows {
        let to_vec = |entries: &[[u8; 4]]| -> Vec<u8> {
            let mut v = vec![0u8; key_count * 4];
            for (i, e) in entries.iter().enumerate() {
                if i < key_count {
                    v[i * 4..i * 4 + 4].copy_from_slice(e);
                }
            }
            v
        };
        RawKeyRows {
            key_count,
            names: (0..key_count)
                .map(|i| matrix::key_name(MatrixPos::new(i as u8)).to_string())
                .collect(),
            defaults: (0..key_count)
                .map(|i| default_keycode(MatrixPos::new(i as u8)))
                .collect(),
            layers: [
                to_vec(base0),
                to_vec(base1),
                vec![0; key_count * 4],
                vec![0; key_count * 4],
            ],
            fn_layer: (!fn_layer.is_empty()).then(|| to_vec(fn_layer)),
            triggers: Default::default(),
            dks_travels: Vec::new(),
            dks_blob: Vec::new(),
            modtap: Vec::new(),
            snaptap: Vec::new(),
        }
    }

    const DEFAULTS: [[u8; 4]; 6] = [
        [0, 0, 0x29, 0], // Esc
        [0, 0, 0x35, 0], // `
        [0, 0, 0x2B, 0], // Tab
        [0, 0, 0x39, 0], // Caps
        [0, 0, 0xE1, 0], // LShf
        [0, 0, 0xE0, 0], // LCtl
    ];

    #[test]
    fn build_key_rows_flags_only_the_remapped_key() {
        let mut base0 = DEFAULTS;
        base0[3] = [0, 0, 0x04, 0]; // Caps -> A
        let empty = [[0u8; 4]; 6];
        let rows = build_key_rows(&make_raw(6, &base0, &empty, &[]));

        let remapped: Vec<_> = rows.iter().filter(|r| r.output_remapped[0]).collect();
        assert_eq!(remapped.len(), 1);
        assert_eq!(remapped[0].index, MatrixPos::new(3));
        assert_eq!(remapped[0].outputs[0], KeyAction::Key(HidUsage::new(0x04)));
        assert!(rows.iter().all(|r| !r.output_remapped[1]));
    }

    /// The base layer is judged against the position's factory keycode, but the
    /// overlay layers have no default at all — anything non-empty there is a
    /// binding, even if it happens to match the base.
    #[test]
    fn overlay_layers_are_set_when_non_empty() {
        let mut l1 = [[0u8; 4]; 6];
        l1[3] = DEFAULTS[3]; // Layer1 bound to the same key the base emits
        let rows = build_key_rows(&make_raw(6, &DEFAULTS, &l1, &[]));

        let caps = rows.iter().find(|r| r.index == MatrixPos::new(3)).unwrap();
        assert!(!caps.output_remapped[0], "base matches its default");
        assert!(
            caps.output_remapped[1],
            "an explicit overlay entry is a binding"
        );
    }

    #[test]
    fn build_key_rows_picks_up_fn_bindings() {
        let mut fn_layer = [[0u8; 4]; 6];
        fn_layer[3] = [3, 0, 0xE9, 0]; // Fn+Caps -> Volume Up
        let rows = build_key_rows(&make_raw(6, &DEFAULTS, &DEFAULTS, &fn_layer));

        let caps = rows.iter().find(|r| r.index == MatrixPos::new(3)).unwrap();
        assert_eq!(caps.fn_action, Some(KeyAction::Consumer(0x00E9)));
        assert!(caps.is_customized());
        // Empty Fn slots stay unbound rather than reading as a binding.
        assert!(
            rows.iter()
                .filter(|r| r.index != MatrixPos::new(3))
                .all(|r| r.fn_action.is_none())
        );
    }

    /// The raw bytes are kept verbatim, since re-encoding moves a lone usage slot.
    #[test]
    fn build_key_rows_keeps_device_bytes() {
        let mut base0 = DEFAULTS;
        base0[3] = [0, 0x29, 0, 0]; // usage in slot 1, as an older driver wrote it
        let rows = build_key_rows(&make_raw(6, &base0, &DEFAULTS, &[]));
        let caps = rows.iter().find(|r| r.index == MatrixPos::new(3)).unwrap();
        assert_eq!(caps.raw[0], [0, 0x29, 0, 0]);
        assert_eq!(caps.outputs[0].to_config_bytes(), [0, 0, 0x29, 0]);
    }

    /// A board whose layout differs from the generic table needs its own defaults,
    /// or every differing key reads as customised. The Womier SK75 has LMeta where
    /// the generic table has LAlt (0xE3 vs 0xE2) and End where it has Home.
    #[test]
    fn device_defaults_decide_what_counts_as_customised() {
        // Generic table's answer for those positions, which is wrong for the SK75.
        assert_eq!(
            default_keycode(MatrixPos::new(17)),
            HidUsage::new(0xE2),
            "generic table says LAlt"
        );

        // Holding the device's real default is not a customisation...
        assert!(!is_user_remap(&[0, 0, 0xE3, 0], HidUsage::new(0xE3)));
        // ...but judging it against the generic table's default says it is.
        assert!(is_user_remap(
            &[0, 0, 0xE3, 0],
            default_keycode(MatrixPos::new(17))
        ));
    }

    // -- is_user_remap (re-tested here for the shared version) --

    #[test]
    fn remap_detection_disabled() {
        assert!(!is_user_remap(&[0, 0, 0, 0], HidUsage::new(0x29)));
    }

    #[test]
    fn remap_detection_identity() {
        assert!(!is_user_remap(&[0, 0, 0x29, 0], HidUsage::new(0x29)));
    }

    #[test]
    fn remap_detection_changed() {
        assert!(is_user_remap(&[0, 0, 0x04, 0], HidUsage::new(0x39)));
    }

    #[test]
    fn remap_detection_macro() {
        assert!(is_user_remap(&[9, 0, 0, 0], HidUsage::new(0xE0)));
    }

    #[test]
    fn remap_detection_fn_key() {
        assert!(!is_user_remap(&[10, 1, 0, 0], HidUsage::new(0xE4)));
    }

    /// A slot written to the position's own factory keycode is not a remap, whichever
    /// usage slot it landed in — the old byte-1-non-zero rule called this customised.
    #[test]
    fn remap_detection_default_written_to_slot_one() {
        assert!(!is_user_remap(&[0, 0x29, 0, 0], HidUsage::new(0x29)));
    }

    /// A chord is always a remap: no factory position emits more than one usage.
    #[test]
    fn remap_detection_chord() {
        assert!(is_user_remap(&[0, 0xE0, 0x06, 0], HidUsage::new(0x06)));
    }
}
