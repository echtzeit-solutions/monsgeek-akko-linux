// Device Registry for Akko/MonsGeek Keyboards
// Defines supported devices and their capabilities

use crate::protocol;

/// Device definition with capabilities
#[derive(Debug, Clone, Copy)]
pub struct DeviceDefinition {
    pub vid: u16,
    pub pid: u16,
    pub name: &'static str,
    pub display_name: &'static str,
    pub key_count: u8,
    pub has_magnetism: bool,
    pub has_sidelight: bool,
}

/// All supported devices
/// Add new devices here as they are tested
pub const SUPPORTED_DEVICES: &[DeviceDefinition] = &[
    // MonsGeek M1 V5 HE (our primary test device)
    DeviceDefinition {
        vid: protocol::VENDOR_ID,
        pid: protocol::PRODUCT_ID_M1_V5_WIRED,
        name: "m1v5he_wired",
        display_name: "MonsGeek M1 V5 HE",
        key_count: 98,
        has_magnetism: true,
        has_sidelight: false,
    },
    // MonsGeek M1 V5 HE Wireless
    DeviceDefinition {
        vid: protocol::VENDOR_ID,
        pid: protocol::PRODUCT_ID_M1_V5_WIRELESS,
        name: "m1v5he_wireless",
        display_name: "MonsGeek M1 V5 HE (Wireless)",
        key_count: 98,
        has_magnetism: true,
        has_sidelight: false,
    },
    // Wireless dongle (variant 1)
    DeviceDefinition {
        vid: protocol::VENDOR_ID,
        pid: protocol::PRODUCT_ID_DONGLE_1,
        name: "dongle_1",
        display_name: "MonsGeek Wireless Dongle",
        key_count: 0,
        has_magnetism: false,
        has_sidelight: false,
    },
    // Wireless dongle (variant 2)
    DeviceDefinition {
        vid: protocol::VENDOR_ID,
        pid: protocol::PRODUCT_ID_DONGLE_2,
        name: "dongle_2",
        display_name: "MonsGeek Wireless Dongle Alt",
        key_count: 0,
        has_magnetism: false,
        has_sidelight: false,
    },
];

/// Find device definition by VID/PID
pub fn find_device(vid: u16, pid: u16) -> Option<&'static DeviceDefinition> {
    SUPPORTED_DEVICES.iter().find(|d| d.vid == vid && d.pid == pid)
}

/// Check if a VID/PID combination is supported
pub fn is_supported(vid: u16, pid: u16) -> bool {
    find_device(vid, pid).is_some()
}

/// Get all supported PIDs for a given VID
pub fn get_pids_for_vid(vid: u16) -> Vec<u16> {
    SUPPORTED_DEVICES
        .iter()
        .filter(|d| d.vid == vid)
        .map(|d| d.pid)
        .collect()
}

/// Check if device has magnetism (hall effect switches)
pub fn has_magnetism(vid: u16, pid: u16) -> bool {
    find_device(vid, pid).map(|d| d.has_magnetism).unwrap_or(false)
}

/// Get key count for device (0 for dongles)
pub fn key_count(vid: u16, pid: u16) -> u8 {
    find_device(vid, pid).map(|d| d.key_count).unwrap_or(0)
}

// =============================================================================
// LED Matrix Mappings
// =============================================================================
// Each device has a matrix that maps LED positions to HID keycodes.
// The matrix is organized in columns (left to right on keyboard).
// Position 0 = first LED, value = HID keycode (0 = empty/no LED)

/// M1 V5 HE LED matrix: position -> HID keycode
/// 98 active keys + empty positions = 126 total matrix positions
pub const M1_V5_HE_LED_MATRIX: [u8; 126] = [
    // Col 0: Esc row down to Ctrl
     41,  // 0: Esc
     53,  // 1: `
     43,  // 2: Tab
     57,  // 3: CapsLock
    225,  // 4: LShift
    224,  // 5: LCtrl
    // Col 1: F1 column
     58,  // 6: F1
     30,  // 7: 1
     20,  // 8: Q
      4,  // 9: A
      0,  // 10: (empty)
    227,  // 11: LWin
    // Col 2: F2 column
     59,  // 12: F2
     31,  // 13: 2
     26,  // 14: W
     22,  // 15: S
     29,  // 16: Z
    226,  // 17: LAlt
    // Col 3: F3 column
     60,  // 18: F3
     32,  // 19: 3
      8,  // 20: E
      7,  // 21: D
     27,  // 22: X
      0,  // 23: (empty)
    // Col 4: F4 column
     61,  // 24: F4
     33,  // 25: 4
     21,  // 26: R
      9,  // 27: F
      6,  // 28: C
      0,  // 29: (empty)
    // Col 5: F5 column
     62,  // 30: F5
     34,  // 31: 5
     23,  // 32: T
     10,  // 33: G
     25,  // 34: V
      0,  // 35: (empty)
    // Col 6: F6 column
     63,  // 36: F6
     35,  // 37: 6
     28,  // 38: Y
     11,  // 39: H
      5,  // 40: B
     44,  // 41: Space
    // Col 7: F7 column
     64,  // 42: F7
     36,  // 43: 7
     24,  // 44: U
     13,  // 45: J
     17,  // 46: N
      0,  // 47: (empty)
    // Col 8: F8 column
     65,  // 48: F8
     37,  // 49: 8
     12,  // 50: I
     14,  // 51: K
     16,  // 52: M
      0,  // 53: (empty)
    // Col 9: F9 column
     66,  // 54: F9
     38,  // 55: 9
     18,  // 56: O
     15,  // 57: L
     54,  // 58: ,
    230,  // 59: RAlt
    // Col 10: F10 column
     67,  // 60: F10
     39,  // 61: 0
     19,  // 62: P
     51,  // 63: ;
     55,  // 64: .
      0,  // 65: (Fn - special)
    // Col 11: F11 column
     68,  // 66: F11
     45,  // 67: -
     47,  // 68: [
     52,  // 69: '
     56,  // 70: /
    228,  // 71: RCtrl
    // Col 12: F12 column
     69,  // 72: F12
     46,  // 73: =
     48,  // 74: ]
      0,  // 75: (empty)
    229,  // 76: RShift
     80,  // 77: Left
    // Col 13: Delete column
     76,  // 78: Delete
     42,  // 79: Backspace
     49,  // 80: Backslash
     40,  // 81: Enter
     82,  // 82: Up
     81,  // 83: Down
    // Col 14: Nav cluster
      0,  // 84: (empty)
     74,  // 85: Home
     75,  // 86: PgUp
     78,  // 87: PgDn
     77,  // 88: End
     79,  // 89: Right
    // Col 15: Media keys
    233,  // 90: VolUp (special: 0x03, 0x00, 0xE9)
    234,  // 91: VolDn (special: 0x03, 0x00, 0xEA)
      0,  // 92: (Mute - special)
      0,  // 93-95: empty
      0,
      0,
    // Remaining positions (96-125) are empty or special
      1,   2,   0,   0,   0,   0,   0,   0,   0,   0,  // 96-105
      0,   0,   0,   0,   0,   0,   0,   0,   0,   0,  // 106-115
      0,   0,   0,   0,   0,   0,   0,   0,   0,   0,  // 116-125
];

/// Common HID keycodes for reference
pub mod hid_codes {
    // Letters
    pub const A: u8 = 4;
    pub const B: u8 = 5;
    pub const C: u8 = 6;
    pub const D: u8 = 7;
    pub const E: u8 = 8;
    pub const F: u8 = 9;
    pub const G: u8 = 10;
    pub const H: u8 = 11;
    pub const I: u8 = 12;
    pub const J: u8 = 13;
    pub const K: u8 = 14;
    pub const L: u8 = 15;
    pub const M: u8 = 16;
    pub const N: u8 = 17;
    pub const O: u8 = 18;
    pub const P: u8 = 19;
    pub const Q: u8 = 20;
    pub const R: u8 = 21;
    pub const S: u8 = 22;
    pub const T: u8 = 23;
    pub const U: u8 = 24;
    pub const V: u8 = 25;
    pub const W: u8 = 26;
    pub const X: u8 = 27;
    pub const Y: u8 = 28;
    pub const Z: u8 = 29;

    // Numbers
    pub const NUM_1: u8 = 30;
    pub const NUM_2: u8 = 31;
    pub const NUM_3: u8 = 32;
    pub const NUM_4: u8 = 33;
    pub const NUM_5: u8 = 34;
    pub const NUM_6: u8 = 35;
    pub const NUM_7: u8 = 36;
    pub const NUM_8: u8 = 37;
    pub const NUM_9: u8 = 38;
    pub const NUM_0: u8 = 39;

    // Special keys
    pub const ENTER: u8 = 40;
    pub const ESC: u8 = 41;
    pub const BACKSPACE: u8 = 42;
    pub const TAB: u8 = 43;
    pub const SPACE: u8 = 44;
    pub const MINUS: u8 = 45;
    pub const EQUALS: u8 = 46;
    pub const LEFT_BRACKET: u8 = 47;
    pub const RIGHT_BRACKET: u8 = 48;
    pub const BACKSLASH: u8 = 49;
    pub const SEMICOLON: u8 = 51;
    pub const QUOTE: u8 = 52;
    pub const BACKTICK: u8 = 53;
    pub const COMMA: u8 = 54;
    pub const PERIOD: u8 = 55;
    pub const SLASH: u8 = 56;
    pub const CAPS_LOCK: u8 = 57;

    // F keys
    pub const F1: u8 = 58;
    pub const F2: u8 = 59;
    pub const F3: u8 = 60;
    pub const F4: u8 = 61;
    pub const F5: u8 = 62;
    pub const F6: u8 = 63;
    pub const F7: u8 = 64;
    pub const F8: u8 = 65;
    pub const F9: u8 = 66;
    pub const F10: u8 = 67;
    pub const F11: u8 = 68;
    pub const F12: u8 = 69;

    // Navigation
    pub const HOME: u8 = 74;
    pub const PAGE_UP: u8 = 75;
    pub const DELETE: u8 = 76;
    pub const END: u8 = 77;
    pub const PAGE_DOWN: u8 = 78;
    pub const RIGHT: u8 = 79;
    pub const LEFT: u8 = 80;
    pub const DOWN: u8 = 81;
    pub const UP: u8 = 82;

    // Modifiers
    pub const LEFT_CTRL: u8 = 224;
    pub const LEFT_SHIFT: u8 = 225;
    pub const LEFT_ALT: u8 = 226;
    pub const LEFT_WIN: u8 = 227;
    pub const RIGHT_CTRL: u8 = 228;
    pub const RIGHT_SHIFT: u8 = 229;
    pub const RIGHT_ALT: u8 = 230;
}

/// Find LED matrix position for a HID keycode
/// Returns None if the key is not in the matrix
pub fn hid_to_led_position(hid_code: u8) -> Option<usize> {
    if hid_code == 0 {
        return None;
    }
    M1_V5_HE_LED_MATRIX.iter().position(|&h| h == hid_code)
}

/// Get all active LED positions (non-empty matrix entries)
pub fn get_active_led_positions() -> Vec<(usize, u8)> {
    M1_V5_HE_LED_MATRIX
        .iter()
        .enumerate()
        .filter(|(_, &hid)| hid != 0)
        .map(|(pos, &hid)| (pos, hid))
        .collect()
}

/// Build a full 126-position RGB array from a sparse key->color map
/// Keys not in the map will be set to black (0,0,0)
pub fn build_led_array_from_keys(key_colors: &[(u8, (u8, u8, u8))]) -> [(u8, u8, u8); 126] {
    let mut result = [(0u8, 0u8, 0u8); 126];
    for &(hid_code, color) in key_colors {
        if let Some(pos) = hid_to_led_position(hid_code) {
            result[pos] = color;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_m1v5he() {
        let dev = find_device(0x3151, 0x5030);
        assert!(dev.is_some());
        let dev = dev.unwrap();
        assert_eq!(dev.display_name, "MonsGeek M1 V5 HE");
        assert_eq!(dev.key_count, 98);
        assert!(dev.has_magnetism);
    }

    #[test]
    fn test_is_supported() {
        assert!(is_supported(0x3151, 0x5030));
        assert!(is_supported(0x3151, 0x503A));
        assert!(!is_supported(0x1234, 0x5678));
    }

    #[test]
    fn test_get_pids() {
        let pids = get_pids_for_vid(0x3151);
        assert_eq!(pids.len(), 4);
        assert!(pids.contains(&0x5030));
        assert!(pids.contains(&0x503A));
    }
}
