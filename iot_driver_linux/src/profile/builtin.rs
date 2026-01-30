// Builtin device profiles
// Hardcoded profiles for known devices (no external JSON needed)

use super::traits::DeviceProfile;
use super::types::TravelSettings;

/// MonsGeek M1 V5 HE (Wired) builtin profile
pub struct M1V5HeProfile {
    travel_settings: TravelSettings,
}

impl M1V5HeProfile {
    pub fn new() -> Self {
        Self {
            travel_settings: TravelSettings::default(),
        }
    }

    /// Wired variant
    pub fn wired() -> Self {
        Self::new()
    }

    /// Wireless variant
    pub fn wireless() -> Self {
        Self::new()
    }
}

impl Default for M1V5HeProfile {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceProfile for M1V5HeProfile {
    fn id(&self) -> u32 {
        2679 // From webapp device database
    }

    fn vid(&self) -> u16 {
        0x3151
    }

    fn pid(&self) -> u16 {
        0x5030 // Wired variant
    }

    fn name(&self) -> &str {
        "m1v5he_wired"
    }

    fn display_name(&self) -> &str {
        "MonsGeek M1 V5 HE"
    }

    fn company(&self) -> &str {
        "MonsGeek"
    }

    fn key_count(&self) -> u8 {
        98
    }

    fn matrix_size(&self) -> usize {
        126
    }

    fn layer_count(&self) -> u8 {
        16 // M1 V5 HE supports 16 layers
    }

    fn led_matrix(&self) -> &[u8] {
        &M1_V5_HE_LED_MATRIX
    }

    fn matrix_key_name(&self, position: u8) -> &str {
        M1_V5_HE_KEY_NAMES
            .get(position as usize)
            .copied()
            .unwrap_or("?")
    }

    fn has_magnetism(&self) -> bool {
        true
    }

    fn has_sidelight(&self) -> bool {
        false
    }

    fn travel_settings(&self) -> Option<&TravelSettings> {
        Some(&self.travel_settings)
    }

    fn fn_layer_win(&self) -> u8 {
        2
    }

    fn fn_layer_mac(&self) -> u8 {
        2
    }
}

/// M1 V5 HE LED matrix: position -> HID keycode
/// 98 active keys + empty positions = 126 total matrix positions
/// Column-major order: each column has 6 rows (top to bottom)
pub const M1_V5_HE_LED_MATRIX: [u8; 126] = [
    // Col 0: Esc row down to Ctrl
    41,  // 0: Esc
    53,  // 1: `
    43,  // 2: Tab
    57,  // 3: CapsLock
    225, // 4: LShift
    224, // 5: LCtrl
    // Col 1: F1 column
    58,  // 6: F1
    30,  // 7: 1
    20,  // 8: Q
    4,   // 9: A
    0,   // 10: (empty)
    227, // 11: LWin
    // Col 2: F2 column
    59,  // 12: F2
    31,  // 13: 2
    26,  // 14: W
    22,  // 15: S
    29,  // 16: Z
    226, // 17: LAlt
    // Col 3: F3 column
    60, // 18: F3
    32, // 19: 3
    8,  // 20: E
    7,  // 21: D
    27, // 22: X
    0,  // 23: (empty)
    // Col 4: F4 column
    61, // 24: F4
    33, // 25: 4
    21, // 26: R
    9,  // 27: F
    6,  // 28: C
    0,  // 29: (empty)
    // Col 5: F5 column
    62, // 30: F5
    34, // 31: 5
    23, // 32: T
    10, // 33: G
    25, // 34: V
    0,  // 35: (empty)
    // Col 6: F6 column
    63, // 36: F6
    35, // 37: 6
    28, // 38: Y
    11, // 39: H
    5,  // 40: B
    44, // 41: Space
    // Col 7: F7 column
    64, // 42: F7
    36, // 43: 7
    24, // 44: U
    13, // 45: J
    17, // 46: N
    0,  // 47: (empty)
    // Col 8: F8 column
    65, // 48: F8
    37, // 49: 8
    12, // 50: I
    14, // 51: K
    16, // 52: M
    0,  // 53: (empty)
    // Col 9: F9 column
    66,  // 54: F9
    38,  // 55: 9
    18,  // 56: O
    15,  // 57: L
    54,  // 58: ,
    230, // 59: RAlt
    // Col 10: F10 column
    67, // 60: F10
    39, // 61: 0
    19, // 62: P
    51, // 63: ;
    55, // 64: .
    0,  // 65: (Fn - special)
    // Col 11: F11 column
    68,  // 66: F11
    45,  // 67: -
    47,  // 68: [
    52,  // 69: '
    56,  // 70: /
    228, // 71: RCtrl
    // Col 12: F12 column
    69,  // 72: F12
    46,  // 73: =
    48,  // 74: ]
    0,   // 75: (empty)
    229, // 76: RShift
    80,  // 77: Left
    // Col 13: Delete column
    76, // 78: Delete
    42, // 79: Backspace
    49, // 80: Backslash
    40, // 81: Enter
    82, // 82: Up
    81, // 83: Down
    // Col 14: Nav cluster
    0,  // 84: (empty)
    74, // 85: Home
    75, // 86: PgUp
    78, // 87: PgDn
    77, // 88: End
    79, // 89: Right
    // Col 15: Media keys
    233, // 90: VolUp
    234, // 91: VolDn
    0,   // 92: (Mute - special)
    0,   // 93-95: empty
    0, 0, // Remaining positions (96-125) are empty or special
    1, 2, 0, 0, 0, 0, 0, 0, 0, 0, // 96-105
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 106-115
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 116-125
];

/// Key names for M1 V5 HE matrix positions
/// Derived from LED matrix HID codes using standard HID usage table
/// Each name corresponds to the same index in M1_V5_HE_LED_MATRIX
pub const M1_V5_HE_KEY_NAMES: &[&str] = &[
    // Col 0 (0-5): Esc column
    "Esc",    // 0: HID 41
    "`",      // 1: HID 53
    "Tab",    // 2: HID 43
    "Caps",   // 3: HID 57
    "LShift", // 4: HID 225
    "LCtrl",  // 5: HID 224
    // Col 1 (6-11): F1/1/Q/A column
    "F1",   // 6: HID 58
    "1",    // 7: HID 30
    "Q",    // 8: HID 20
    "A",    // 9: HID 4
    "",     // 10: empty
    "LWin", // 11: HID 227
    // Col 2 (12-17): F2/2/W/S/Z column
    "F2",   // 12: HID 59
    "2",    // 13: HID 31
    "W",    // 14: HID 26
    "S",    // 15: HID 22
    "Z",    // 16: HID 29
    "LAlt", // 17: HID 226
    // Col 3 (18-23): F3/3/E/D/X column
    "F3", // 18: HID 60
    "3",  // 19: HID 32
    "E",  // 20: HID 8
    "D",  // 21: HID 7
    "X",  // 22: HID 27
    "",   // 23: empty
    // Col 4 (24-29): F4/4/R/F/C column
    "F4", // 24: HID 61
    "4",  // 25: HID 33
    "R",  // 26: HID 21
    "F",  // 27: HID 9
    "C",  // 28: HID 6
    "",   // 29: empty
    // Col 5 (30-35): F5/5/T/G/V column
    "F5", // 30: HID 62
    "5",  // 31: HID 34
    "T",  // 32: HID 23
    "G",  // 33: HID 10
    "V",  // 34: HID 25
    "",   // 35: empty
    // Col 6 (36-41): F6/6/Y/H/B/Space column
    "F6",    // 36: HID 63
    "6",     // 37: HID 35
    "Y",     // 38: HID 28
    "H",     // 39: HID 11
    "B",     // 40: HID 5
    "Space", // 41: HID 44
    // Col 7 (42-47): F7/7/U/J/N column
    "F7", // 42: HID 64
    "7",  // 43: HID 36
    "U",  // 44: HID 24
    "J",  // 45: HID 13
    "N",  // 46: HID 17
    "",   // 47: empty
    // Col 8 (48-53): F8/8/I/K/M column
    "F8", // 48: HID 65
    "8",  // 49: HID 37
    "I",  // 50: HID 12
    "K",  // 51: HID 14
    "M",  // 52: HID 16
    "",   // 53: empty
    // Col 9 (54-59): F9/9/O/L/,/RAlt column
    "F9",   // 54: HID 66
    "9",    // 55: HID 38
    "O",    // 56: HID 18
    "L",    // 57: HID 15
    ",",    // 58: HID 54
    "RAlt", // 59: HID 230
    // Col 10 (60-65): F10/0/P/;/./Fn column
    "F10", // 60: HID 67
    "0",   // 61: HID 39
    "P",   // 62: HID 19
    ";",   // 63: HID 51
    ".",   // 64: HID 55
    "Fn",  // 65: special (no HID)
    // Col 11 (66-71): F11/-/[/'/RCtrl column
    "F11",   // 66: HID 68
    "-",     // 67: HID 45
    "[",     // 68: HID 47
    "'",     // 69: HID 52
    "/",     // 70: HID 56
    "RCtrl", // 71: HID 228
    // Col 12 (72-77): F12/=/]/RShift/Left column
    "F12",    // 72: HID 69
    "=",      // 73: HID 46
    "]",      // 74: HID 48
    "",       // 75: empty
    "RShift", // 76: HID 229
    "Left",   // 77: HID 80
    // Col 13 (78-83): Del/Bksp/\/Enter/Up/Down column
    "Del",   // 78: HID 76
    "Bksp",  // 79: HID 42
    "\\",    // 80: HID 49
    "Enter", // 81: HID 40
    "Up",    // 82: HID 82
    "Down",  // 83: HID 81
    // Col 14 (84-89): Nav cluster
    "",      // 84: empty
    "Home",  // 85: HID 74
    "PgUp",  // 86: HID 75
    "PgDn",  // 87: HID 78
    "End",   // 88: HID 77
    "Right", // 89: HID 79
    // Col 15 (90-95): Media keys
    "Vol+", // 90: HID 233
    "Vol-", // 91: HID 234
    "Mute", // 92: special
    "",     // 93: empty
    "",     // 94: empty
    "",     // 95: empty
    // Remaining positions (96-125)
    "?", "?", "", "", "", "", "", "", "", "", // 96-105
    "", "", "", "", "", "", "", "", "", "", // 106-115
    "", "", "", "", "", "", "", "", "", "", // 116-125
];

/// MonsGeek M1 V5 HE Wireless variant
pub struct M1V5HeWirelessProfile {
    inner: M1V5HeProfile,
}

impl M1V5HeWirelessProfile {
    pub fn new() -> Self {
        Self {
            inner: M1V5HeProfile::new(),
        }
    }
}

impl Default for M1V5HeWirelessProfile {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceProfile for M1V5HeWirelessProfile {
    fn id(&self) -> u32 {
        2680 // Different ID for wireless
    }

    fn vid(&self) -> u16 {
        0x3151
    }

    fn pid(&self) -> u16 {
        0x503A // Wireless variant
    }

    fn name(&self) -> &str {
        "m1v5he_wireless"
    }

    fn display_name(&self) -> &str {
        "MonsGeek M1 V5 HE (Wireless)"
    }

    fn company(&self) -> &str {
        self.inner.company()
    }

    fn key_count(&self) -> u8 {
        self.inner.key_count()
    }

    fn matrix_size(&self) -> usize {
        self.inner.matrix_size()
    }

    fn layer_count(&self) -> u8 {
        self.inner.layer_count()
    }

    fn led_matrix(&self) -> &[u8] {
        self.inner.led_matrix()
    }

    fn matrix_key_name(&self, position: u8) -> &str {
        self.inner.matrix_key_name(position)
    }

    fn has_magnetism(&self) -> bool {
        self.inner.has_magnetism()
    }

    fn has_sidelight(&self) -> bool {
        self.inner.has_sidelight()
    }

    fn travel_settings(&self) -> Option<&TravelSettings> {
        self.inner.travel_settings()
    }

    fn fn_layer_win(&self) -> u8 {
        self.inner.fn_layer_win()
    }

    fn fn_layer_mac(&self) -> u8 {
        self.inner.fn_layer_mac()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_m1v5he_profile() {
        let profile = M1V5HeProfile::new();

        assert_eq!(profile.vid(), 0x3151);
        assert_eq!(profile.pid(), 0x5030);
        assert_eq!(profile.key_count(), 98);
        assert!(profile.has_magnetism());

        // Verify LED matrix
        assert_eq!(profile.led_matrix().len(), 126);
        assert_eq!(profile.led_matrix()[0], 41); // Esc

        // Verify key names match matrix positions
        assert_eq!(profile.matrix_key_name(0), "Esc");
        assert_eq!(profile.matrix_key_name(1), "`");
        assert_eq!(profile.matrix_key_name(2), "Tab");
        assert_eq!(profile.matrix_key_name(9), "A");
        assert_eq!(profile.matrix_key_name(15), "S");
        assert_eq!(profile.matrix_key_name(41), "Space");
        assert_eq!(profile.matrix_key_name(52), "M");
        assert_eq!(profile.matrix_key_name(58), ",");
    }

    #[test]
    fn test_key_names_count() {
        assert_eq!(M1_V5_HE_KEY_NAMES.len(), 126);
        assert_eq!(M1_V5_HE_LED_MATRIX.len(), 126);
    }

    #[test]
    fn test_active_keys_count() {
        let profile = M1V5HeProfile::new();
        let active = profile.active_positions();

        // Count non-empty positions in the matrix
        let expected = M1_V5_HE_LED_MATRIX.iter().filter(|&&x| x != 0).count();
        assert_eq!(active.len(), expected);
    }

    #[test]
    fn test_wireless_profile() {
        let profile = M1V5HeWirelessProfile::new();

        assert_eq!(profile.vid(), 0x3151);
        assert_eq!(profile.pid(), 0x503A);
        assert_eq!(profile.display_name(), "MonsGeek M1 V5 HE (Wireless)");

        // Should share the same matrix layout
        assert_eq!(profile.led_matrix()[0], 41);
        assert_eq!(profile.matrix_key_name(0), "Esc");
    }
}
