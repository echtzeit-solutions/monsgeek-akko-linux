//! Unified keymap abstraction for CLI and TUI.
//!
//! Provides shared types (`KeyEntry`, `KeyMap`) and I/O helpers
//! (`load_sync`/`load_async`, `set_key_sync`/`set_key_async`) so that both the CLI
//! and TUI share identical parsing, filtering, and writing logic.
//!
//! `Layer` and `KeyRef` live in `monsgeek_transport::protocol` and are re-exported here.

use crate::key_action::KeyAction;
use crate::protocol::hid;
use monsgeek_transport::protocol::matrix;

use monsgeek_keyboard::{KeyboardError, KeyboardInterface, SyncKeyboard};

// Re-export from monsgeek-transport so existing `use crate::keymap::{Layer, KeyRef}` still works.
pub use monsgeek_transport::protocol::{KeyRef, Layer};

// ---------------------------------------------------------------------------
// KeyEntry — single key in a keymap snapshot
// ---------------------------------------------------------------------------

/// A single key in a keymap snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyEntry {
    pub index: u8,
    pub position: &'static str,
    pub layer: Layer,
    pub action: KeyAction,
    /// True when the key differs from factory default.
    pub is_remapped: bool,
}

impl KeyEntry {
    /// Format the key reference part (e.g. "Fn+Caps").
    pub fn key_ref(&self) -> KeyRef {
        KeyRef {
            index: self.index,
            position: self.position,
            layer: self.layer,
        }
    }
}

// ---------------------------------------------------------------------------
// RawKeyMapData — decoupled I/O for testability
// ---------------------------------------------------------------------------

/// Raw bytes read from the keyboard, before parsing.
pub struct RawKeyMapData {
    /// GET_KEYMATRIX(0) — base layer 0.
    pub base0: Vec<u8>,
    /// GET_KEYMATRIX(1) — base layer 1.
    pub base1: Vec<u8>,
    /// GET_FN — Fn layer (None if read failed).
    pub fn_layer: Option<Vec<u8>>,
    /// Number of physical keys.
    pub key_count: usize,
}

// ---------------------------------------------------------------------------
// KeyMap — complete keymap snapshot
// ---------------------------------------------------------------------------

/// A complete keymap snapshot across all layers.
pub struct KeyMap {
    entries: Vec<KeyEntry>,
}

impl KeyMap {
    /// Parse raw keymatrix data into a `KeyMap`.
    pub fn from_raw(raw: &RawKeyMapData) -> Self {
        // Build factory default keycodes from the transport's matrix key names.
        let defaults: Vec<u8> = (0..raw.key_count as u8)
            .map(|i| hid::key_code_from_name(matrix::key_name(i)).unwrap_or(0))
            .collect();

        let mut entries = Vec::new();

        // Parse base layers 0 and 1
        for (layer, data) in [(Layer::Base, &raw.base0), (Layer::Layer1, &raw.base1)] {
            for i in 0..raw.key_count {
                if i * 4 + 3 >= data.len() {
                    break;
                }
                let name = matrix::key_name(i as u8);
                if name == "?" {
                    continue;
                }

                let k = &data[i * 4..(i + 1) * 4];
                let action = KeyAction::from_config_bytes([k[0], k[1], k[2], k[3]]);
                let is_remapped = is_user_remap(k, defaults[i]);

                entries.push(KeyEntry {
                    index: i as u8,
                    position: name,
                    layer,
                    action,
                    is_remapped,
                });
            }
        }

        // Parse Fn layer
        if let Some(fn_data) = &raw.fn_layer {
            for i in 0..raw.key_count {
                if i * 4 + 3 >= fn_data.len() {
                    break;
                }
                let name = matrix::key_name(i as u8);
                if name == "?" {
                    continue;
                }

                let k = &fn_data[i * 4..(i + 1) * 4];
                if k == [0, 0, 0, 0] {
                    continue; // empty slot in Fn layer
                }

                let action = KeyAction::from_config_bytes([k[0], k[1], k[2], k[3]]);

                // For Fn layer, all non-empty entries are "remapped" (they represent bindings)
                entries.push(KeyEntry {
                    index: i as u8,
                    position: name,
                    layer: Layer::Fn,
                    action,
                    is_remapped: true,
                });
            }
        }

        KeyMap { entries }
    }

    /// All entries across all layers.
    pub fn iter(&self) -> impl Iterator<Item = &KeyEntry> {
        self.entries.iter()
    }

    /// Only entries where `is_remapped == true`.
    pub fn remaps(&self) -> impl Iterator<Item = &KeyEntry> {
        self.entries.iter().filter(|e| e.is_remapped)
    }

    /// Entries for a single layer.
    pub fn layer(&self, layer: Layer) -> impl Iterator<Item = &KeyEntry> {
        self.entries.iter().filter(move |e| e.layer == layer)
    }

    /// Remapped entries for a single layer.
    pub fn layer_remaps(&self, layer: Layer) -> impl Iterator<Item = &KeyEntry> {
        self.entries
            .iter()
            .filter(move |e| e.layer == layer && e.is_remapped)
    }

    /// Look up a single entry by index and layer.
    pub fn get(&self, index: u8, layer: Layer) -> Option<&KeyEntry> {
        self.entries
            .iter()
            .find(|e| e.index == index && e.layer == layer)
    }
}

// ---------------------------------------------------------------------------
// Remap detection (shared logic)
// ---------------------------------------------------------------------------

/// Detect whether a 4-byte key config represents a user remap.
///
/// `default_hid_code`: the factory default HID keycode for this matrix position,
/// derived from `hid::key_code_from_name(matrix::key_name(i))`.
pub fn is_user_remap(k: &[u8], default_hid_code: u8) -> bool {
    if k.len() < 4 {
        return false;
    }

    // Disabled: never a remap
    if k[0] == 0 && k[1] == 0 && k[2] == 0 && k[3] == 0 {
        return false;
    }

    // Fn key at physical Fn position: factory default
    if matches!(
        KeyAction::from_config_bytes([k[0], k[1], k[2], k[3]]),
        KeyAction::Fn
    ) {
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

// ---------------------------------------------------------------------------
// I/O: loading
// ---------------------------------------------------------------------------

/// Number of pages to read for a full key matrix (126 positions × 4 bytes = 504).
const KEYMATRIX_PAGES: usize = 8;

/// Load from SyncKeyboard (CLI).
pub fn load_sync(keyboard: &SyncKeyboard) -> Result<KeyMap, KeyboardError> {
    let key_count = keyboard.key_count() as usize;
    let base0 = keyboard.get_keymatrix(0, KEYMATRIX_PAGES)?;
    let base1 = keyboard.get_keymatrix(1, KEYMATRIX_PAGES)?;
    let fn_layer = keyboard.get_fn_keymatrix(0, 0, KEYMATRIX_PAGES).ok();

    Ok(KeyMap::from_raw(&RawKeyMapData {
        base0,
        base1,
        fn_layer,
        key_count,
    }))
}

/// Load from KeyboardInterface (TUI async).
pub async fn load_async(keyboard: &KeyboardInterface) -> Result<KeyMap, KeyboardError> {
    let key_count = keyboard.key_count() as usize;
    let base0 = keyboard.get_keymatrix(0, KEYMATRIX_PAGES).await?;
    let base1 = keyboard.get_keymatrix(1, KEYMATRIX_PAGES).await?;
    let fn_layer = keyboard.get_fn_keymatrix(0, 0, KEYMATRIX_PAGES).await.ok();

    Ok(KeyMap::from_raw(&RawKeyMapData {
        base0,
        base1,
        fn_layer,
        key_count,
    }))
}

// ---------------------------------------------------------------------------
// I/O: writing
// ---------------------------------------------------------------------------

/// Write a key config via SyncKeyboard (CLI).
pub fn set_key_sync(
    kb: &SyncKeyboard,
    index: u8,
    layer: Layer,
    action: &KeyAction,
) -> Result<(), KeyboardError> {
    kb.set_key_config(0, index, layer.wire_layer(), action.to_config_bytes())
}

/// Write a key config via KeyboardInterface (TUI async).
pub async fn set_key_async(
    kb: &KeyboardInterface,
    index: u8,
    layer: Layer,
    action: &KeyAction,
) -> Result<(), KeyboardError> {
    kb.set_key_config(0, index, layer.wire_layer(), action.to_config_bytes())
        .await
}

/// Reset a key to default via SyncKeyboard (CLI).
pub fn reset_key_sync(kb: &SyncKeyboard, index: u8, layer: Layer) -> Result<(), KeyboardError> {
    kb.reset_key(layer.wire_layer(), index)
}

/// Reset a key to default via KeyboardInterface (TUI async).
pub async fn reset_key_async(
    kb: &KeyboardInterface,
    index: u8,
    layer: Layer,
) -> Result<(), KeyboardError> {
    kb.reset_key(layer.wire_layer(), index).await
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

    #[test]
    fn layer_wire_roundtrip() {
        for layer in Layer::ALL {
            assert_eq!(Layer::from_wire(layer.wire_layer()), layer);
        }
    }

    // -- KeyRef --

    #[test]
    fn keyref_parse_bare_name() {
        let kr: KeyRef = "Caps".parse().unwrap();
        assert_eq!(kr.index, 3);
        assert_eq!(kr.layer, Layer::Base);
        assert_eq!(kr.position, "Caps");
    }

    #[test]
    fn keyref_parse_fn_prefix() {
        let kr: KeyRef = "Fn+Caps".parse().unwrap();
        assert_eq!(kr.index, 3);
        assert_eq!(kr.layer, Layer::Fn);
    }

    #[test]
    fn keyref_parse_l1_prefix() {
        let kr: KeyRef = "L1+Caps".parse().unwrap();
        assert_eq!(kr.index, 3);
        assert_eq!(kr.layer, Layer::Layer1);
    }

    #[test]
    fn keyref_parse_numeric() {
        let kr: KeyRef = "42".parse().unwrap();
        assert_eq!(kr.index, 42);
        assert_eq!(kr.layer, Layer::Base);
    }

    #[test]
    fn keyref_parse_fn_numeric() {
        let kr: KeyRef = "Fn+42".parse().unwrap();
        assert_eq!(kr.index, 42);
        assert_eq!(kr.layer, Layer::Fn);
    }

    #[test]
    fn keyref_parse_case_insensitive() {
        let kr: KeyRef = "fn+caps".parse().unwrap();
        assert_eq!(kr.index, 3);
        assert_eq!(kr.layer, Layer::Fn);
    }

    #[test]
    fn keyref_display_base() {
        let kr = KeyRef::new(3, Layer::Base);
        assert_eq!(kr.to_string(), "Caps");
    }

    #[test]
    fn keyref_display_fn() {
        let kr = KeyRef::new(3, Layer::Fn);
        assert_eq!(kr.to_string(), "Fn+Caps");
    }

    #[test]
    fn keyref_display_l1() {
        let kr = KeyRef::new(3, Layer::Layer1);
        assert_eq!(kr.to_string(), "L1+Caps");
    }

    // -- KeyMap::from_raw --

    fn make_raw(
        key_count: usize,
        base0: &[[u8; 4]],
        base1: &[[u8; 4]],
        fn_layer: &[[u8; 4]],
    ) -> RawKeyMapData {
        let to_vec = |entries: &[[u8; 4]]| -> Vec<u8> {
            let mut v = vec![0u8; key_count * 4];
            for (i, e) in entries.iter().enumerate() {
                if i < key_count {
                    v[i * 4..i * 4 + 4].copy_from_slice(e);
                }
            }
            v
        };
        RawKeyMapData {
            base0: to_vec(base0),
            base1: to_vec(base1),
            fn_layer: if fn_layer.is_empty() {
                None
            } else {
                Some(to_vec(fn_layer))
            },
            key_count,
        }
    }

    #[test]
    fn keymap_detects_remap() {
        // Position 3 = Caps (default 0x39). Remap to A (0x04).
        let raw = make_raw(
            6,
            &[
                [0, 0, 0x29, 0], // Esc (default)
                [0, 0, 0x35, 0], // ` (default)
                [0, 0, 0x2B, 0], // Tab (default)
                [0, 0, 0x04, 0], // Caps → A (REMAP)
                [0, 0, 0xE1, 0], // LShf (default)
                [0, 0, 0xE0, 0], // LCtl (default)
            ],
            &[
                [0, 0, 0x29, 0],
                [0, 0, 0x35, 0],
                [0, 0, 0x2B, 0],
                [0, 0, 0x39, 0], // Caps identity on L1
                [0, 0, 0xE1, 0],
                [0, 0, 0xE0, 0],
            ],
            &[],
        );

        let km = KeyMap::from_raw(&raw);
        let remaps: Vec<_> = km.remaps().collect();
        assert_eq!(remaps.len(), 1);
        assert_eq!(remaps[0].index, 3);
        assert_eq!(remaps[0].layer, Layer::Base);
        assert_eq!(remaps[0].action, KeyAction::Key(0x04));
    }

    #[test]
    fn keymap_fn_layer_entries() {
        let raw = make_raw(
            6,
            &[
                [0, 0, 0x29, 0],
                [0, 0, 0x35, 0],
                [0, 0, 0x2B, 0],
                [0, 0, 0x39, 0],
                [0, 0, 0xE1, 0],
                [0, 0, 0xE0, 0],
            ],
            &[
                [0, 0, 0x29, 0],
                [0, 0, 0x35, 0],
                [0, 0, 0x2B, 0],
                [0, 0, 0x39, 0],
                [0, 0, 0xE1, 0],
                [0, 0, 0xE0, 0],
            ],
            &[
                [0, 0, 0, 0],    // empty
                [3, 0, 0xE9, 0], // Volume Up
                [0, 0, 0, 0],    // empty
                [0, 0, 0, 0],    // empty
                [0, 0, 0, 0],    // empty
                [0, 0, 0, 0],    // empty
            ],
        );

        let km = KeyMap::from_raw(&raw);
        let fn_entries: Vec<_> = km.layer(Layer::Fn).collect();
        assert_eq!(fn_entries.len(), 1);
        assert_eq!(fn_entries[0].index, 1);
        assert_eq!(fn_entries[0].layer, Layer::Fn);
        assert_eq!(fn_entries[0].action, KeyAction::Consumer(0x00E9));
    }

    // -- is_user_remap (re-tested here for the shared version) --

    #[test]
    fn remap_detection_disabled() {
        assert!(!is_user_remap(&[0, 0, 0, 0], 0x29));
    }

    #[test]
    fn remap_detection_identity() {
        assert!(!is_user_remap(&[0, 0, 0x29, 0], 0x29));
    }

    #[test]
    fn remap_detection_changed() {
        assert!(is_user_remap(&[0, 0, 0x04, 0], 0x39));
    }

    #[test]
    fn remap_detection_macro() {
        assert!(is_user_remap(&[9, 0, 0, 0], 0xE0));
    }

    #[test]
    fn remap_detection_fn_key() {
        assert!(!is_user_remap(&[10, 1, 0, 0], 0xE4));
    }
}
