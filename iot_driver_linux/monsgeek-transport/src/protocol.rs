//! Protocol constants and utilities for MonsGeek/Akko keyboard communication

use crate::types::ChecksumType;
use std::fmt;
use std::str::FromStr;

/// HID Protocol Commands (FEA_CMD_*)
pub mod cmd {
    // SET commands (0x01 - 0x65)
    pub const SET_RESET: u8 = 0x01;
    pub const SET_REPORT: u8 = 0x03;
    pub const SET_PROFILE: u8 = 0x04;
    pub const SET_DEBOUNCE: u8 = 0x06;
    pub const SET_LEDPARAM: u8 = 0x07;
    pub const SET_SLEDPARAM: u8 = 0x08;
    pub const SET_KBOPTION: u8 = 0x09;
    pub const SET_KEYMATRIX: u8 = 0x0A;
    pub const SET_MACRO: u8 = 0x0B;
    pub const SET_USERPIC: u8 = 0x0C;
    pub const SET_AUDIO_VIZ: u8 = 0x0D;
    pub const SET_SCREEN_COLOR: u8 = 0x0E;
    pub const SET_USERGIF: u8 = 0x12;
    pub const SET_FN: u8 = 0x10;
    pub const SET_SLEEPTIME: u8 = 0x11;
    pub const SET_AUTOOS_EN: u8 = 0x17;
    pub const SET_MAGNETISM_REPORT: u8 = 0x1B;
    pub const SET_MAGNETISM_CAL: u8 = 0x1C;
    pub const SET_MAGNETISM_MAX_CAL: u8 = 0x1E;
    pub const SET_KEY_MAGNETISM_MODE: u8 = 0x1D;
    pub const SET_MULTI_MAGNETISM: u8 = 0x65;

    // GET commands (0x80 - 0xE6)
    pub const GET_REV: u8 = 0x80;
    pub const GET_REPORT: u8 = 0x83;
    pub const GET_PROFILE: u8 = 0x84;
    pub const GET_LEDONOFF: u8 = 0x85;
    pub const GET_DEBOUNCE: u8 = 0x86;
    pub const GET_LEDPARAM: u8 = 0x87;
    pub const GET_SLEDPARAM: u8 = 0x88;
    pub const GET_KBOPTION: u8 = 0x89;
    pub const GET_USERPIC: u8 = 0x8C;
    pub const GET_KEYMATRIX: u8 = 0x8A;
    pub const GET_MACRO: u8 = 0x8B;
    pub const GET_USB_VERSION: u8 = 0x8F;
    pub const GET_FN: u8 = 0x90;
    pub const GET_SLEEPTIME: u8 = 0x91;
    pub const GET_AUTOOS_EN: u8 = 0x97;
    pub const GET_KEY_MAGNETISM_MODE: u8 = 0x9D;
    pub const GET_OLED_VERSION: u8 = 0xAD;
    pub const GET_MLED_VERSION: u8 = 0xAE;
    pub const GET_MULTI_MAGNETISM: u8 = 0xE5;
    pub const GET_FEATURE_LIST: u8 = 0xE6;
    pub const GET_CALIBRATION: u8 = 0xFE;

    // Dongle-specific commands (from dongle firmware RE)
    /// Get dongle info: returns {0xF0, 1, 8, 0,0,0,0, fw_ver}
    pub const GET_DONGLE_INFO: u8 = 0xF0;
    /// Set control byte: stores data[0] → dongle_state.ctrl_byte
    pub const SET_CTRL_BYTE: u8 = 0xF6;
    /// Get dongle status (9-byte response): has_response, kb_battery_info, 0,
    /// kb_charging, 1, rf_ready, 1, pairing_mode, pairing_status.
    /// Handled locally by dongle — NOT forwarded to keyboard.
    pub const GET_DONGLE_STATUS: u8 = 0xF7;
    /// Enter pairing mode: requires 55AA55AA magic
    pub const ENTER_PAIRING: u8 = 0xF8;
    /// Pairing control: sends 3-byte SPI packet {cmd=1, data[0], data[1]}
    pub const PAIRING_CMD: u8 = 0x7A;
    /// Patch info - custom firmware capabilities (battery HID, LED stream, etc.)
    /// Note: Was 0xFB, but that collides with dongle-local GET_RF_INFO.
    pub const GET_PATCH_INFO: u8 = 0xE7;
    /// LED streaming - write RGB data to WS2812 frame buffer via patch.
    /// Sub-commands: page 0-6 = data, 0xFF = commit, 0xFE = release.
    pub const LED_STREAM: u8 = 0xE8;
    /// Animation engine — on-device keyframe playback.
    /// Sub-commands: 0x00-0x07 = ASSIGN, 0x08-0x0F = DEF, 0x10-0x17 = DEF_EXT,
    /// 0xFE = CANCEL, 0xFF = CLEAR.
    pub const ANIM_CMD: u8 = 0xEA;
    /// Peek device memory (patched firmware, capability bit 7). Request:
    /// addr LE32, len, width, checksum slot, nonce. See `patch/memaccess.h`.
    pub const PEEK: u8 = 0xEB;
    /// Poke device memory (patched firmware, capability bit 7). Request:
    /// addr LE32, width, nonce, checksum slot, value LE32, tag "POKE".
    pub const POKE: u8 = 0xEC;
    /// Get RF info: returns {rf_addr[4], fw_ver_minor, fw_ver_major, 0, 0}.
    /// Handled locally by dongle — NOT forwarded to keyboard.
    pub const GET_RF_INFO: u8 = 0xFB;
    /// Get cached keyboard response: copies 64B cached_kb_response into the
    /// USB feature report buffer and clears has_response. Used as flush.
    pub const GET_CACHED_RESPONSE: u8 = 0xFC;
    /// Get dongle ID: returns {0xAA, 0x55, 0x01, 0x00}.
    /// Note: 0xFE is GET_CALIBRATION on keyboard but SET_RESPONSE_SIZE on dongle.
    pub const GET_DONGLE_ID: u8 = 0xFD;
    /// Set response size on dongle (dongle-local, NOT forwarded).
    /// Note: same byte as GET_CALIBRATION (0xFE) on keyboard.
    pub const SET_RESPONSE_SIZE: u8 = 0xFE;

    // Response status
    pub const STATUS_SUCCESS: u8 = 0xAA;

    /// Get human-readable name for command byte
    pub fn name(cmd: u8) -> &'static str {
        match cmd {
            SET_RESET => "SET_RESET",
            SET_REPORT => "SET_REPORT",
            SET_PROFILE => "SET_PROFILE",
            SET_DEBOUNCE => "SET_DEBOUNCE",
            SET_LEDPARAM => "SET_LEDPARAM",
            SET_SLEDPARAM => "SET_SLEDPARAM",
            SET_KBOPTION => "SET_KBOPTION",
            SET_KEYMATRIX => "SET_KEYMATRIX",
            SET_MACRO => "SET_MACRO",
            SET_USERPIC => "SET_USERPIC",
            SET_AUDIO_VIZ => "SET_AUDIO_VIZ",
            SET_SCREEN_COLOR => "SET_SCREEN_COLOR",
            SET_USERGIF => "SET_USERGIF",
            SET_FN => "SET_FN",
            SET_SLEEPTIME => "SET_SLEEPTIME",
            SET_AUTOOS_EN => "SET_AUTOOS_EN",
            SET_MAGNETISM_REPORT => "SET_MAGNETISM_REPORT",
            SET_MAGNETISM_CAL => "SET_MAGNETISM_CAL",
            SET_MAGNETISM_MAX_CAL => "SET_MAGNETISM_MAX_CAL",
            SET_KEY_MAGNETISM_MODE => "SET_KEY_MAGNETISM_MODE",
            SET_MULTI_MAGNETISM => "SET_MULTI_MAGNETISM",
            GET_REV => "GET_REV",
            GET_REPORT => "GET_REPORT",
            GET_PROFILE => "GET_PROFILE",
            GET_LEDONOFF => "GET_LEDONOFF",
            GET_DEBOUNCE => "GET_DEBOUNCE",
            GET_LEDPARAM => "GET_LEDPARAM",
            GET_SLEDPARAM => "GET_SLEDPARAM",
            GET_KBOPTION => "GET_KBOPTION",
            GET_USERPIC => "GET_USERPIC",
            GET_KEYMATRIX => "GET_KEYMATRIX",
            GET_MACRO => "GET_MACRO",
            GET_USB_VERSION => "GET_USB_VERSION",
            GET_FN => "GET_FN",
            GET_SLEEPTIME => "GET_SLEEPTIME",
            GET_AUTOOS_EN => "GET_AUTOOS_EN",
            GET_KEY_MAGNETISM_MODE => "GET_KEY_MAGNETISM_MODE",
            GET_OLED_VERSION => "GET_OLED_VERSION",
            GET_MLED_VERSION => "GET_MLED_VERSION",
            GET_MULTI_MAGNETISM => "GET_MULTI_MAGNETISM",
            GET_FEATURE_LIST => "GET_FEATURE_LIST",
            GET_CALIBRATION => "GET_CALIBRATION",
            GET_DONGLE_INFO => "GET_DONGLE_INFO",
            SET_CTRL_BYTE => "SET_CTRL_BYTE",
            GET_DONGLE_STATUS => "GET_DONGLE_STATUS",
            ENTER_PAIRING => "ENTER_PAIRING",
            PAIRING_CMD => "PAIRING_CMD",
            GET_PATCH_INFO => "GET_PATCH_INFO",
            LED_STREAM => "LED_STREAM",
            ANIM_CMD => "ANIM_CMD",
            PEEK => "PEEK",
            POKE => "POKE",
            GET_RF_INFO => "GET_RF_INFO",
            GET_CACHED_RESPONSE => "GET_CACHED_RESPONSE",
            GET_DONGLE_ID => "GET_DONGLE_ID",
            STATUS_SUCCESS => "STATUS_SUCCESS",
            _ => "UNKNOWN",
        }
    }
}

/// Magnetism (Hall Effect trigger) sub-commands for GET/SET_MULTI_MAGNETISM
pub mod magnetism {
    /// Press travel (actuation point)
    pub const PRESS_TRAVEL: u8 = 0x00;
    /// Lift travel (release point)
    pub const LIFT_TRAVEL: u8 = 0x01;
    /// Rapid Trigger press sensitivity
    pub const RT_PRESS: u8 = 0x02;
    /// Rapid Trigger lift sensitivity
    pub const RT_LIFT: u8 = 0x03;
    /// DKS (Dynamic Keystroke) travel
    pub const DKS_TRAVEL: u8 = 0x04;
    /// Mod-Tap activation time
    pub const MODTAP_TIME: u8 = 0x05;
    /// Bottom deadzone
    pub const BOTTOM_DEADZONE: u8 = 0x06;
    /// Key mode (Normal, RT, DKS, etc.)
    pub const KEY_MODE: u8 = 0x07;
    /// Snap Tap anti-SOCD enable
    pub const SNAPTAP_ENABLE: u8 = 0x09;
    /// DKS trigger modes/actions (GET, 8 pages — 512-byte interleaved layout)
    pub const DKS_MODES: u8 = 0x0A;
    /// DKS trigger modes/actions (SET simple — 4 packed bytes per key)
    pub const DKS_TRIGGER_MODES_SET: u8 = 0x08;
    /// Top deadzone (firmware >= 1024)
    pub const TOP_DEADZONE: u8 = 0xFB;
    /// Switch type (if replaceable)
    pub const SWITCH_TYPE: u8 = 0xFC;
    /// Raw sensor calibration values
    pub const CALIBRATION: u8 = 0xFE;

    /// Get human-readable name for magnetism sub-command
    pub fn name(subcmd: u8) -> &'static str {
        match subcmd {
            PRESS_TRAVEL => "PRESS_TRAVEL",
            LIFT_TRAVEL => "LIFT_TRAVEL",
            RT_PRESS => "RT_PRESS",
            RT_LIFT => "RT_LIFT",
            DKS_TRAVEL => "DKS_TRAVEL",
            MODTAP_TIME => "MODTAP_TIME",
            BOTTOM_DEADZONE => "BOTTOM_DEADZONE",
            KEY_MODE => "KEY_MODE",
            SNAPTAP_ENABLE => "SNAPTAP_ENABLE",
            DKS_MODES => "DKS_MODES",
            DKS_TRIGGER_MODES_SET => "DKS_TRIGGER_MODES_SET",
            TOP_DEADZONE => "TOP_DEADZONE",
            SWITCH_TYPE => "SWITCH_TYPE",
            CALIBRATION => "CALIBRATION",
            _ => "UNKNOWN",
        }
    }
}

// ---------------------------------------------------------------------------
// Key index spaces
// ---------------------------------------------------------------------------

/// Where a key sits in the scan matrix (column-major, `col * 6 + row`).
///
/// Deliberately not interchangeable with [`HidUsage`]: `matrix::key_name` and
/// `hid::key_name` have the same shape and opposite meanings, and swapping them
/// compiles and returns a wrong-but-plausible name. Distinct from the LED grid
/// position (`row * 16 + col`) and the WS2812 strip index, which are two further
/// index spaces over the same physical keys.
///
/// There is no `From<u8>` on purpose — construction is `MatrixPos::new`, so every
/// crossing from an untyped integer stays greppable.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MatrixPos(u8);

impl MatrixPos {
    pub const fn new(pos: u8) -> Self {
        Self(pos)
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Physical row within the column-major matrix.
    pub const fn row(self) -> u8 {
        self.0 % matrix::ROWS
    }

    /// Physical column.
    pub const fn col(self) -> u8 {
        self.0 / matrix::ROWS
    }

    /// Generic name for this position, or `"?"` where the matrix has a gap.
    ///
    /// Deliberately not a `Display` impl: a board's own name for a position can
    /// differ from the generic table's (the bug fixed in `2550596`), so a print
    /// site has to say whether it wants this name, a device-resolved one, or the
    /// number.
    pub fn name(self) -> &'static str {
        matrix::key_name(self)
    }
}

impl From<MatrixPos> for u8 {
    fn from(p: MatrixPos) -> Self {
        p.0
    }
}

impl From<MatrixPos> for usize {
    fn from(p: MatrixPos) -> Self {
        p.0 as usize
    }
}

/// A USB HID keyboard usage code — what a key *emits*, as opposed to where it sits.
///
/// Modifiers are ordinary usages `0xE0..=0xE7`; the firmware derives the report's
/// modifier bits from them. A HID *report* modifier bitmask is a different thing
/// again and lives privately in `macro_seq`.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct HidUsage(u8);

impl HidUsage {
    pub const NONE: Self = Self(0);

    pub const fn new(code: u8) -> Self {
        Self(code)
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Usages below 0x04 are reserved error codes; the firmware's `hid_key_press`
    /// returns early for them, so a slot holding one emits nothing.
    pub const fn is_live(self) -> bool {
        self.0 >= 0x04
    }

    /// `0xE0..=0xE7`, which fold into the report's modifier byte rather than a key slot.
    pub const fn is_modifier(self) -> bool {
        0xE0 <= self.0 && self.0 <= 0xE7
    }
}

impl From<HidUsage> for u8 {
    fn from(u: HidUsage) -> Self {
        u.0
    }
}

/// The 16×6 LED grid the firmware addresses by `row * 16 + col`.
pub mod led_grid {
    pub const COLS: u8 = 16;
    pub const ROWS: u8 = 6;
    /// Number of grid cells; the WS2812 strip populates 82 of them.
    pub const LEN: usize = COLS as usize * ROWS as usize;
}

/// A cell of the 16×6 LED grid, addressed row-major as `row * 16 + col`.
///
/// This is the LED streaming and animation-assignment space. It is *not*
/// [`MatrixPos`] (column-major `col * 6 + row`, the config protocol's space) and
/// it is *not* [`StripIdx`] (the WS2812 wiring order). The firmware converts
/// between grid and strip itself, which is why `anim_assign` takes one and
/// `anim_query_keys` returns the other.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct LedPos(u8);

impl LedPos {
    pub const fn new(pos: u8) -> Self {
        Self(pos)
    }

    /// Build from grid coordinates. `None` if either is off the grid.
    pub const fn from_row_col(row: u8, col: u8) -> Option<Self> {
        if row >= led_grid::ROWS || col >= led_grid::COLS {
            return None;
        }
        Some(Self(row * led_grid::COLS + col))
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    pub const fn row(self) -> u8 {
        self.0 / led_grid::COLS
    }

    pub const fn col(self) -> u8 {
        self.0 % led_grid::COLS
    }
}

impl From<LedPos> for u8 {
    fn from(p: LedPos) -> Self {
        p.0
    }
}

impl From<LedPos> for usize {
    fn from(p: LedPos) -> Self {
        p.0 as usize
    }
}

/// A WS2812 LED's position along the strip, in firmware `static_led_pos_tbl`
/// order (serpentine: even rows left-to-right, odd rows right-to-left).
///
/// Only the firmware's table relates this to [`LedPos`]; there is no arithmetic
/// conversion, which is why the two are separate types rather than one.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct StripIdx(u8);

impl StripIdx {
    /// LEDs on the strip.
    pub const COUNT: u8 = 82;

    pub const fn new(idx: u8) -> Self {
        Self(idx)
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Which LED grid cell this strip LED lights, per the firmware's table.
    ///
    /// The only bridge between the two spaces, and deliberately a named method
    /// rather than a `From` impl: `anim_assign` speaks [`LedPos`] while
    /// `anim_query_keys` answers in [`StripIdx`], so a round-trip has to say so.
    /// `None` for strip indices the firmware leaves unmapped.
    pub fn to_led_pos(self) -> Option<LedPos> {
        let &grid = STRIP_TO_LED_GRID.get(self.0 as usize)?;
        ((grid as usize) < led_grid::LEN).then(|| LedPos::new(grid))
    }
}

/// Firmware's `static_led_pos_tbl`, inverted: which LED grid cell each WS2812 on
/// the strip lights. `STRIP_TO_LED_GRID[strip_idx]` is a [`LedPos`] (0xFF = unmapped).
///
/// This table is the *only* relation between the strip and grid spaces — there is
/// no arithmetic conversion, which is why they are separate types.
#[rustfmt::skip]
const STRIP_TO_LED_GRID: [u8; StripIdx::COUNT as usize] = [
    0x00, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x1F, 0x1E,
    0x1D, 0x1C, 0x1B, 0x1A, 0x19, 0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x10, 0x20, 0x22, 0x23,
    0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F, 0x3F, 0x3E, 0x3C, 0x3B,
    0x3A, 0x39, 0x38, 0x37, 0x36, 0x35, 0x34, 0x33, 0x32, 0x30, 0x40, 0x42, 0x43, 0x44, 0x45, 0x46,
    0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4D, 0x4E, 0x4F, 0x5F, 0x5E, 0x5D, 0x5C, 0x5B, 0x5A, 0x56, 0x52,
    0x51, 0x50,
];

impl From<StripIdx> for u8 {
    fn from(i: StripIdx) -> Self {
        i.0
    }
}

impl From<StripIdx> for usize {
    fn from(i: StripIdx) -> Self {
        i.0 as usize
    }
}

/// Key matrix position to name mapping (M1 V5 / SG9000 layout)
///
/// Column-major order, 6 rows per column.  Verified against firmware
/// GET_KEYMATRIX data (factory-default keycodes at each position).
pub mod matrix {
    /// Key names indexed by matrix position (column-major order).
    ///
    /// Row 0 = F-key row, rows 1-4 = main alpha/symbol rows,
    /// row 5 = bottom modifier row.  "?" marks unused matrix slots
    /// (e.g. spacebar columns that don't produce a keypress).
    const KEY_NAMES: &[&str] = &[
        // Col 0 (0-5)
        "Esc", "`", "Tab", "Caps", "LShf", "LCtl", // Col 1 (6-11)
        "F1", "1", "Q", "A", "IntlBs", "Win", // Col 2 (12-17)
        "F2", "2", "W", "S", "Z", "LAlt", // Col 3 (18-23)
        "F3", "3", "E", "D", "X", "?", // Col 4 (24-29)
        "F4", "4", "R", "F", "C", "?", // Col 5 (30-35)
        "F5", "5", "T", "G", "V", "?", // Col 6 (36-41)
        "F6", "6", "Y", "H", "B", "Spc", // Col 7 (42-47)
        "F7", "7", "U", "J", "N", "?", // Col 8 (48-53)
        "F8", "8", "I", "K", "M", "?", // Col 9 (54-59)
        "F9", "9", "O", "L", ",", "RAlt", // Col 10 (60-65)
        "F10", "0", "P", ";", ".", "Fn", // Col 11 (66-71)
        "F11", "-", "[", "'", "/", "RCtl", // Col 12 (72-77)
        "F12", "=", "]", "IntlRo", "RShf", "Left", // Col 13 (78-83)
        "Del", "Bksp", "\\", "Ent", "Up", "Down", // Col 14 (84-89)
        "?", "Home", "PgUp", "PgDn", "End", "Right",
    ];

    use super::MatrixPos;

    /// Rows per column in the column-major matrix.
    pub const ROWS: u8 = 6;

    /// Number of matrix positions the name table covers. Positions beyond this
    /// exist on some boards (encoders, for one) but have no generic name.
    pub const KEY_COUNT: u8 = KEY_NAMES.len() as u8;

    /// Generic name for a matrix position, `"?"` for gaps.
    pub fn key_name(pos: MatrixPos) -> &'static str {
        KEY_NAMES.get(pos.get() as usize).copied().unwrap_or("?")
    }

    /// Look up a matrix position by key name (case-insensitive).
    ///
    /// Returns None if no matching key name is found. Note the sibling
    /// `hid::key_code_from_name`, which takes the same input and returns a
    /// [`HidUsage`](super::HidUsage) instead — the two are not interchangeable.
    pub fn key_index_from_name(name: &str) -> Option<MatrixPos> {
        let name_lower = name.to_ascii_lowercase();
        KEY_NAMES
            .iter()
            .position(|&n| n.to_ascii_lowercase() == name_lower && n != "?")
            .map(|i| MatrixPos::new(i as u8))
    }
}

// ---------------------------------------------------------------------------
// Layer
// ---------------------------------------------------------------------------

/// A keyboard profile, 0–3.
///
/// The board's whole configuration is per-profile — keymaps, magnetism tables,
/// LED settings — so a command sent against the wrong profile writes real
/// settings somewhere the user is not looking.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Profile(u8);

impl Profile {
    pub const COUNT: u8 = 4;
    /// Every profile, in order.
    pub const ALL: [Self; 4] = [Self(0), Self(1), Self(2), Self(3)];

    /// Zero-based wire value.
    pub const fn get(self) -> u8 {
        self.0
    }

    /// One-based number, as the keyboard's own UI and labels count profiles.
    /// The wire value is zero-based, so this is where the off-by-one lives.
    pub const fn number(self) -> u8 {
        self.0 + 1
    }
}

impl TryFrom<u8> for Profile {
    type Error = String;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        if v < Self::COUNT {
            Ok(Self(v))
        } else {
            Err(format!("profile must be 0-{}, got {v}", Self::COUNT - 1))
        }
    }
}

impl From<Profile> for u8 {
    fn from(p: Profile) -> Self {
        p.0
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Profile {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u8>()
            .map_err(|_| format!("invalid profile: {s:?}"))
            .and_then(Self::try_from)
    }
}

/// A macro storage slot, 0–49.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MacroSlot(u8);

impl MacroSlot {
    pub const COUNT: u8 = 50;

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for MacroSlot {
    type Error = String;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        if v < Self::COUNT {
            Ok(Self(v))
        } else {
            Err(format!("macro slot must be 0-{}, got {v}", Self::COUNT - 1))
        }
    }
}

impl From<MacroSlot> for u8 {
    fn from(m: MacroSlot) -> Self {
        m.0
    }
}

impl fmt::Display for MacroSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for MacroSlot {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u8>()
            .map_err(|_| format!("invalid macro slot: {s:?}"))
            .and_then(Self::try_from)
    }
}

/// An animation definition slot, 0–7.
///
/// The wire format packs the slot into the low three bits of a sub-command byte.
/// Masking an out-of-range id there would silently retarget the write — slot 8
/// would land on slot 0 — so the range is checked here instead.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DefId(u8);

impl DefId {
    pub const COUNT: u8 = 8;
    /// Every slot, in order.
    pub const ALL: [Self; 8] = [
        Self(0),
        Self(1),
        Self(2),
        Self(3),
        Self(4),
        Self(5),
        Self(6),
        Self(7),
    ];
    /// The highest slot.
    pub const LAST: Self = Self(Self::COUNT - 1);

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for DefId {
    type Error = String;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        if v < Self::COUNT {
            Ok(Self(v))
        } else {
            Err(format!(
                "animation slot must be 0-{}, got {v}",
                Self::COUNT - 1
            ))
        }
    }
}

impl From<DefId> for u8 {
    fn from(d: DefId) -> Self {
        d.0
    }
}

impl fmt::Display for DefId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A keymatrix sub-layer, 0–3 — the wire-level address of one of a key's four
/// config slots.
///
/// What a slot *means* depends on the key's mode. In Normal mode slot 0 is the
/// key's output and slot 1 an overlay; in DKS mode the firmware reinterprets all
/// four as the key's DKS bindings. Use [`Self::dks_slot`] when that is the intent,
/// so the call site says which reading it wants.
///
/// The Fn layer is deliberately *not* representable here: it lives in a separate
/// store (SET_FN / GET_FN), not in the keymatrix. [`Layer`] is the type that
/// spans both.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeymatrixLayer(u8);

impl KeymatrixLayer {
    pub const COUNT: u8 = 4;
    pub const BASE: Self = Self(0);
    /// Every sub-layer, in wire order.
    pub const ALL: [Self; 4] = [Self(0), Self(1), Self(2), Self(3)];

    /// The `n`th DKS binding of a key. Same wire value as the sub-layer of the
    /// same number — the firmware reinterprets, it does not relocate.
    pub const fn dks_slot(n: u8) -> Option<Self> {
        if n < Self::COUNT { Some(Self(n)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for KeymatrixLayer {
    type Error = String;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        if v < Self::COUNT {
            Ok(Self(v))
        } else {
            Err(format!("keymatrix layer must be 0–3, got {v}"))
        }
    }
}

impl From<KeymatrixLayer> for u8 {
    fn from(l: KeymatrixLayer) -> Self {
        l.0
    }
}

impl fmt::Display for KeymatrixLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Logical key layer on the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// Base layer (GET_KEYMATRIX page 0, SET_KEYMATRIX layer=0).
    Base,
    /// Second base layer (GET_KEYMATRIX page 1, SET_KEYMATRIX layer=1).
    Layer1,
    /// Fn layer (GET_FN / SET_FN).
    Fn,
}

impl Layer {
    /// All layers in display order.
    pub const ALL: [Layer; 3] = [Layer::Base, Layer::Layer1, Layer::Fn];

    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            Layer::Base => "Layer 0",
            Layer::Layer1 => "Layer 1",
            Layer::Fn => "Fn layer",
        }
    }

    /// Short label for compact display.
    pub fn short(self) -> &'static str {
        match self {
            Layer::Base => "L0",
            Layer::Layer1 => "L1",
            Layer::Fn => "Fn",
        }
    }

    /// Which keymatrix sub-layer backs this layer, if any.
    ///
    /// `Fn` returns `None`: it is a separate store (SET_FN / GET_FN), so a caller
    /// has to branch rather than write it as though it were a keymatrix slot.
    pub fn keymatrix_layer(self) -> Option<KeymatrixLayer> {
        match self {
            Layer::Base => Some(KeymatrixLayer(0)),
            Layer::Layer1 => Some(KeymatrixLayer(1)),
            Layer::Fn => None,
        }
    }
}

impl TryFrom<u8> for Layer {
    type Error = String;

    /// The numbering `--layer` accepts, matching [`FromStr`]. Not a wire value —
    /// `Fn` has no keymatrix slot; see [`Layer::keymatrix_layer`].
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Layer::Base),
            1 => Ok(Layer::Layer1),
            2 => Ok(Layer::Fn),
            _ => Err(format!("unknown layer: {v}. Use 0 (base), 1, or 2 (Fn)")),
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.short())
    }
}

impl FromStr for Layer {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "0" | "l0" | "base" => Ok(Layer::Base),
            "1" | "l1" => Ok(Layer::Layer1),
            "2" | "fn" => Ok(Layer::Fn),
            _ => Err(format!("unknown layer: \"{s}\". Use 0/L0/base, 1/L1, 2/fn")),
        }
    }
}

// ---------------------------------------------------------------------------
// KeyRef — parsed key + layer reference
// ---------------------------------------------------------------------------

/// A key position + layer reference, e.g. "Fn+Caps" → (Caps, Fn layer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRef {
    pub index: MatrixPos,
    pub position: &'static str,
    pub layer: Layer,
}

impl KeyRef {
    pub fn new(index: MatrixPos, layer: Layer) -> Self {
        Self {
            index,
            position: matrix::key_name(index),
            layer,
        }
    }
}

impl fmt::Display for KeyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.layer {
            Layer::Base => "",
            Layer::Layer1 => "L1+",
            Layer::Fn => "Fn+",
        };
        // "?" is the table's no-name marker and cannot be typed back in. Show the
        // position instead, in the same `#N` form the keymatrix listing uses and
        // `resolve_key` accepts — so anything printed here can be pasted into a
        // `remap`.
        if self.position == "?" {
            write!(f, "{prefix}#{}", self.index.get())
        } else {
            write!(f, "{prefix}{}", self.position)
        }
    }
}

impl FromStr for KeyRef {
    type Err = String;

    /// Parse a key reference with optional layer prefix:
    ///
    /// - `"Caps"` → KeyRef { index=3, layer=Base }
    /// - `"Fn+Caps"` → KeyRef { index=3, layer=Fn }
    /// - `"L1+A"` → KeyRef { index=9, layer=Layer1 }
    /// - `"42"` → KeyRef { index=42, layer=Base }
    /// - `"Fn+42"` → KeyRef { index=42, layer=Fn }
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Check for layer prefix: "Fn+", "L1+", "L0+"
        let (layer, key_part) = if let Some(rest) = strip_prefix_ci(s, "Fn+") {
            (Layer::Fn, rest)
        } else if let Some(rest) = strip_prefix_ci(s, "L1+") {
            (Layer::Layer1, rest)
        } else if let Some(rest) = strip_prefix_ci(s, "L0+") {
            (Layer::Base, rest)
        } else {
            (Layer::Base, s)
        };

        let index = resolve_key(key_part)?;
        Ok(KeyRef {
            index,
            position: matrix::key_name(index),
            layer,
        })
    }
}

/// Case-insensitive prefix strip that returns the remainder with original casing.
///
/// All prefixes used here ("Fn+", "L1+", "L0+") are pure ASCII, so the byte-length
/// comparison is safe on UTF-8 strings.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len()
        && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Resolve a key name or numeric index to a matrix position.
pub fn resolve_key(key: &str) -> Result<MatrixPos, String> {
    // `#N` is unambiguously a matrix position — the same spelling `--keys` uses,
    // and the only way to name a position the tables have no name for.
    if let Some(n) = key.strip_prefix('#') {
        return n
            .parse::<u8>()
            .map(MatrixPos::new)
            .map_err(|_| format!("invalid matrix position: \"{key}\""));
    }
    // Try numeric index first
    if let Ok(idx) = key.parse::<u8>() {
        return Ok(MatrixPos::new(idx));
    }
    // Try matrix key name (Esc, F3, LShf, etc.)
    if let Some(idx) = matrix::key_index_from_name(key) {
        return Ok(idx);
    }
    Err(format!(
        "unknown key: \"{key}\". Use a matrix position (42 or #42) or a name like F3, Esc, Tab"
    ))
}

// =============================================================================
// Protocol Family (RY5088 vs YiChip)
// =============================================================================

/// Protocol family — determines which command byte mapping to use.
///
/// Transport layer is identical between families (same checksums, same IF2 vendor HID,
/// same bootloader). Only the command byte mapping differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProtocolFamily {
    #[default]
    Ry5088,
    YiChip,
}

/// Command byte mapping table for a protocol family.
///
/// Contains only the command bytes that differ between RY5088 and YiChip.
/// Shared commands (SET_LEDPARAM, GET_USB_VERSION, GET_MACRO, etc.) use
/// the constants in `cmd::` directly.
#[derive(Debug)]
pub struct CommandTable {
    pub set_reset: u8,
    pub set_profile: u8,
    pub set_debounce: u8,
    pub set_keymatrix: u8,
    pub set_macro: u8,
    pub get_profile: u8,
    pub get_debounce: u8,
    pub get_keymatrix: u8,
    // RY5088-only commands (None on YiChip)
    pub set_report: Option<u8>,
    pub set_kboption: Option<u8>,
    pub set_sleeptime: Option<u8>,
    pub get_report: Option<u8>,
    pub get_kboption: Option<u8>,
    pub get_sleeptime: Option<u8>,
    /// Userpic slot upload/download. YiChip boards reuse the same command bytes
    /// with a different page header (no slot byte, a length field instead) that
    /// this driver does not implement, so they get `None`.
    pub set_userpic: Option<u8>,
    pub get_userpic: Option<u8>,
}

pub static RY5088_COMMANDS: CommandTable = CommandTable {
    set_reset: 0x01,
    set_profile: 0x04,
    set_debounce: 0x06,
    set_keymatrix: 0x0A,
    set_macro: 0x0B,
    get_profile: 0x84,
    get_debounce: 0x86,
    get_keymatrix: 0x8A,
    set_report: Some(0x03),
    set_kboption: Some(0x09),
    set_sleeptime: Some(0x11),
    get_report: Some(0x83),
    get_kboption: Some(0x89),
    get_sleeptime: Some(0x91),
    set_userpic: Some(cmd::SET_USERPIC),
    get_userpic: Some(cmd::GET_USERPIC),
};

pub static YICHIP_COMMANDS: CommandTable = CommandTable {
    set_reset: 0x02,
    set_profile: 0x05,
    set_debounce: 0x11,
    set_keymatrix: 0x09,
    set_macro: 0x08,
    get_profile: 0x85,
    get_debounce: 0x91,
    get_keymatrix: 0x89,
    set_report: None,
    set_kboption: None,
    set_sleeptime: None,
    get_report: None,
    get_kboption: Some(0x86),
    get_sleeptime: None,
    set_userpic: None,
    get_userpic: None,
};

impl ProtocolFamily {
    /// Get the command table for this protocol family.
    pub fn commands(&self) -> &'static CommandTable {
        match self {
            ProtocolFamily::Ry5088 => &RY5088_COMMANDS,
            ProtocolFamily::YiChip => &YICHIP_COMMANDS,
        }
    }

    /// Detect protocol family from device name and PID.
    ///
    /// Detection priority:
    /// 1. Name prefix from device database (`ry5088_` → RY5088, `yc*_` → YiChip)
    /// 2. PID heuristic (0x40xx → YiChip, 0x50xx → RY5088)
    /// 3. Default: RY5088
    pub fn detect(device_name: Option<&str>, pid: u16) -> Self {
        if let Some(name) = device_name {
            let lower = name.to_ascii_lowercase();
            if lower.starts_with("ry5088_") || lower.starts_with("ry1086_") {
                return ProtocolFamily::Ry5088;
            }
            if lower.starts_with("yc500_")
                || lower.starts_with("yc300_")
                || lower.starts_with("yc3121_")
                || lower.starts_with("yc3123_")
            {
                return ProtocolFamily::YiChip;
            }
        }
        // PID heuristic: 0x40xx PIDs are YiChip-based
        if pid & 0xFF00 == 0x4000 {
            return ProtocolFamily::YiChip;
        }
        ProtocolFamily::Ry5088
    }
}

impl fmt::Display for ProtocolFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolFamily::Ry5088 => f.write_str("RY5088"),
            ProtocolFamily::YiChip => f.write_str("YiChip"),
        }
    }
}

/// HID report sizes
pub const REPORT_SIZE: usize = 65;
pub const INPUT_REPORT_SIZE: usize = 64;

/// HID communication timing constants
pub mod timing {
    /// Number of retries for query operations
    pub const QUERY_RETRIES: usize = 5;
    /// Number of retries for send operations
    pub const SEND_RETRIES: usize = 3;
    /// Default delay after HID command (ms) - for wired devices
    pub const DEFAULT_DELAY_MS: u64 = 100;
    /// Wired query read-gap (ms): wait after sending a query before reading, so
    /// the device has computed the response and loaded the report buffer (read
    /// too soon → stale/previous response). Retried up to [`QUERY_RETRIES`], so
    /// it can be small.
    pub const WIRED_READ_GAP_MS: u64 = 10;
    /// Wired write spacing (ms): delay after a fire-and-forget write. Near-zero;
    /// only relevant for back-to-back flash-write bursts, which add their own
    /// per-iteration spacing. Realtime streaming bypasses this entirely (delay 0).
    pub const WIRED_WRITE_SPACING_MS: u64 = 5;
    /// Short delay for fast operations (ms)
    pub const SHORT_DELAY_MS: u64 = 50;
    /// Minimum delay for streaming (ms)
    pub const MIN_DELAY_MS: u64 = 5;
    /// Delay after starting animation upload (ms)
    pub const ANIMATION_START_DELAY_MS: u64 = 500;
}

/// Dongle-specific timing for polling-based flow control
///
/// Based on throughput testing:
/// - Minimum observed latency: ~8-10ms (awake keyboard)
/// - Response requires flush command to push into buffer
/// - Concurrent commands not supported by hardware
pub mod dongle_timing {
    /// Initial wait before first poll attempt (ms)
    /// Adaptive baseline - actual wait is computed from moving average
    pub const INITIAL_WAIT_MS: u64 = 5;

    /// Default timeout for query operations (ms)
    pub const QUERY_TIMEOUT_MS: u64 = 500;

    /// Extended timeout when keyboard may be waking from sleep (ms)
    pub const WAKE_TIMEOUT_MS: u64 = 2000;

    /// Minimum time per poll cycle - flush + read (ms)
    /// Observed ~1.1ms in testing, but allow brief yield
    pub const POLL_CYCLE_MS: u64 = 1;

    /// Moving average window size for latency tracking
    pub const LATENCY_WINDOW_SIZE: usize = 8;

    /// Maximum consecutive timeouts before marking device offline
    pub const MAX_CONSECUTIVE_TIMEOUTS: usize = 3;

    /// Queue capacity for pending command requests
    pub const REQUEST_QUEUE_SIZE: usize = 16;
}

/// RGB/LED data constants
pub mod rgb {
    /// Total RGB data size (126 keys * 3 bytes)
    pub const TOTAL_RGB_SIZE: usize = 378;
    /// Number of pages per frame
    pub const NUM_PAGES: usize = 7;
    /// RGB data per full page
    pub const PAGE_SIZE: usize = 56;
    /// RGB data in last page
    pub const LAST_PAGE_SIZE: usize = 42;
    /// LED matrix positions (keys)
    pub const MATRIX_SIZE: usize = 126;
    /// Number of keys to send per chunk in streaming mode
    pub const CHUNK_SIZE: usize = 18;
    /// Magic value for per-key color commands
    pub const MAGIC_VALUE: u8 = 255;
}

/// Bluetooth Low Energy protocol constants
pub mod ble {
    /// Vendor report ID for BLE HID
    pub const VENDOR_REPORT_ID: u8 = 0x06;
    /// Marker byte for command/response channel
    pub const CMDRESP_MARKER: u8 = 0x55;
    /// Marker byte for event channel
    pub const EVENT_MARKER: u8 = 0x66;
    /// Buffer size for BLE reports (65 bytes + report ID)
    pub const REPORT_SIZE: usize = 66;
    /// Default command delay for BLE (higher than USB due to latency)
    pub const DEFAULT_DELAY_MS: u64 = 150;
}

/// Precision version thresholds
///
/// These constants define firmware version boundaries for different
/// precision levels in travel/trigger settings.
pub mod precision {
    /// Version threshold for fine precision (0.005mm steps)
    /// Firmware versions >= 1280 (0x500) support fine precision
    pub const FINE_VERSION: u16 = 1280;
    /// Version threshold for medium precision (0.01mm steps)
    /// Firmware versions >= 768 (0x300) support medium precision
    pub const MEDIUM_VERSION: u16 = 768;
}

/// Device identification constants
pub mod device {
    pub use crate::device_registry::VENDOR_ID;

    /// HID usage page for vendor-defined (USB)
    pub const USAGE_PAGE: u16 = 0xFFFF;
    /// Alternative vendor usage page seen on some models
    pub const USAGE_PAGE_ALT: u16 = 0xFF00;
    /// HID usage for feature interface (USB)
    pub const USAGE_FEATURE: u16 = 0x02;
    /// HID usage for input interface (USB)
    pub const USAGE_INPUT: u16 = 0x01;

    /// Feature interface number
    pub const INTERFACE_FEATURE: i32 = 2;
    /// Input interface number
    pub const INTERFACE_INPUT: i32 = 1;

    /// Check if a usage page is a vendor usage page (0xFFFF or 0xFF00)
    #[inline]
    pub fn is_vendor_usage_page(page: u16) -> bool {
        page == USAGE_PAGE || page == USAGE_PAGE_ALT
    }

    pub use crate::device_registry::{is_bluetooth_pid, is_dongle_pid};
}

/// Calculate checksum for HID message
pub fn calculate_checksum(data: &[u8], checksum_type: ChecksumType) -> u8 {
    match checksum_type {
        ChecksumType::Bit7 => {
            let sum: u32 = data.iter().take(7).map(|&b| b as u32).sum();
            (255 - (sum & 0xFF)) as u8
        }
        ChecksumType::Bit8 => {
            let sum: u32 = data.iter().take(8).map(|&b| b as u32).sum();
            (255 - (sum & 0xFF)) as u8
        }
        ChecksumType::None => 0,
    }
}

/// Apply checksum to message buffer
pub fn apply_checksum(data: &mut [u8], checksum_type: ChecksumType) {
    match checksum_type {
        ChecksumType::Bit7 => {
            if data.len() >= 8 {
                data[7] = calculate_checksum(data, checksum_type);
            }
        }
        ChecksumType::Bit8 => {
            if data.len() >= 9 {
                data[8] = calculate_checksum(data, checksum_type);
            }
        }
        ChecksumType::None => {}
    }
}

/// Build a USB command buffer with checksum
///
/// Format: `[report_id=0] [cmd] [data...] [checksum...]`
pub fn build_command(cmd: u8, data: &[u8], checksum_type: ChecksumType) -> Vec<u8> {
    let mut buf = vec![0u8; REPORT_SIZE];
    buf[0] = 0; // Report ID
    buf[1] = cmd;
    let len = std::cmp::min(data.len(), REPORT_SIZE - 2);
    buf[2..2 + len].copy_from_slice(&data[..len]);
    apply_checksum(&mut buf[1..], checksum_type);
    buf
}

/// Build a BLE command buffer with checksum
///
/// BLE uses a different framing than USB:
/// Format: `[report_id=0x06] [0x55 marker] [cmd] [data...] [checksum...]`
///
/// The checksum is calculated starting from the cmd byte (skipping the 0x55 marker).
pub fn build_ble_command(cmd: u8, data: &[u8], checksum_type: ChecksumType) -> Vec<u8> {
    let mut buf = vec![0u8; ble::REPORT_SIZE];
    buf[0] = ble::VENDOR_REPORT_ID; // Report ID 6 for BLE
    buf[1] = ble::CMDRESP_MARKER; // 0x55 marker
    buf[2] = cmd;
    let len = std::cmp::min(data.len(), ble::REPORT_SIZE - 3);
    buf[3..3 + len].copy_from_slice(&data[..len]);
    // Apply checksum starting from cmd byte (index 2)
    apply_checksum(&mut buf[2..], checksum_type);
    buf
}

/// The LED grid ([`LedPos`], row-major) and the WS2812 strip ([`StripIdx`],
/// wiring order) are two index spaces over the same 82 LEDs. Nothing round-trips
/// between them without `STRIP_TO_LED_GRID`, so these pin the table.
#[cfg(test)]
mod led_index_space_tests {
    use super::{LedPos, STRIP_TO_LED_GRID, StripIdx, led_grid};
    use std::collections::HashSet;

    #[test]
    fn every_strip_led_lights_a_distinct_grid_cell() {
        let mut seen = HashSet::new();
        for i in 0..StripIdx::COUNT {
            let grid = StripIdx::new(i)
                .to_led_pos()
                .unwrap_or_else(|| panic!("strip {i} maps off the 16x6 grid"));
            assert!(
                seen.insert(grid),
                "grid cell {} claimed by two strip LEDs",
                grid.get()
            );
        }
    }

    /// The two spaces disagree for most LEDs, which is why reading an
    /// `anim_query_keys` result as a grid position lights the wrong key.
    #[test]
    fn strip_order_is_not_grid_order() {
        let same = (0..StripIdx::COUNT)
            .filter(|&i| STRIP_TO_LED_GRID[i as usize] == i)
            .count();
        assert!(
            same < StripIdx::COUNT as usize / 2,
            "{same}/{} LEDs share an index — the two spaces would be interchangeable",
            StripIdx::COUNT
        );
    }

    #[test]
    fn out_of_range_strip_has_no_grid_cell() {
        assert_eq!(StripIdx::new(StripIdx::COUNT).to_led_pos(), None);
        assert_eq!(StripIdx::new(200).to_led_pos(), None);
        // Strip 0 is Esc, the top-left grid cell.
        assert_eq!(StripIdx::new(0).to_led_pos(), Some(LedPos::new(0)));
    }

    #[test]
    fn grid_positions_round_trip_through_row_col() {
        for row in 0..led_grid::ROWS {
            for col in 0..led_grid::COLS {
                let pos = LedPos::from_row_col(row, col).expect("on the grid");
                assert_eq!((pos.row(), pos.col()), (row, col));
            }
        }
        assert_eq!(LedPos::from_row_col(led_grid::ROWS, 0), None);
        assert_eq!(LedPos::from_row_col(0, led_grid::COLS), None);
    }
}

/// `Layer` spans two stores and `KeymatrixLayer` only one, so the two are not
/// interchangeable. Before this split a `FN_WIRE_LAYER = 2` sentinel stood in for
/// "the Fn store" in a `u8` that also held keymatrix slot 2, and callers
/// disagreed about which numbering they were speaking.
#[cfg(test)]
mod layer_tests {
    use super::{KeymatrixLayer, Layer};

    #[test]
    fn only_the_base_layers_have_a_keymatrix_slot() {
        assert_eq!(
            Layer::Base.keymatrix_layer().map(KeymatrixLayer::get),
            Some(0)
        );
        assert_eq!(
            Layer::Layer1.keymatrix_layer().map(KeymatrixLayer::get),
            Some(1)
        );
        // Fn lives in SET_FN / GET_FN, so it has no keymatrix slot at all.
        assert_eq!(Layer::Fn.keymatrix_layer(), None);
    }

    /// Out of range must be rejected, not wrapped or saturated onto a real layer —
    /// the old `from_wire` mapped *everything* >= 2 onto `Fn`.
    #[test]
    fn out_of_range_layers_are_rejected() {
        for v in 0..=3u8 {
            assert_eq!(KeymatrixLayer::try_from(v).map(|l| l.get()), Ok(v));
            assert_eq!(
                KeymatrixLayer::dks_slot(v).map(KeymatrixLayer::get),
                Some(v)
            );
        }
        for v in 4..=u8::MAX {
            assert!(
                KeymatrixLayer::try_from(v).is_err(),
                "{v} must not be a layer"
            );
            assert_eq!(
                KeymatrixLayer::dks_slot(v),
                None,
                "{v} must not be a DKS slot"
            );
        }

        for (v, expect) in [(0, Layer::Base), (1, Layer::Layer1), (2, Layer::Fn)] {
            assert_eq!(Layer::try_from(v), Ok(expect));
        }
        for v in 3..=u8::MAX {
            assert!(Layer::try_from(v).is_err(), "{v} must not be a layer");
        }
    }

    /// Parsing and display agree with the numbering `TryFrom` accepts.
    #[test]
    fn layer_names_and_numbers_agree() {
        for (v, layer) in [(0, Layer::Base), (1, Layer::Layer1), (2, Layer::Fn)] {
            assert_eq!(v.to_string().parse::<Layer>(), Ok(layer));
            assert_eq!(
                layer.short().to_ascii_lowercase().parse::<Layer>(),
                Ok(layer)
            );
        }
    }
}

/// Every bounds-checked index rejects out-of-range input rather than wrapping,
/// saturating, or masking. `DefId` in particular used to reach the wire as
/// `id & 0x07`, so slot 8 silently retargeted slot 0.
#[cfg(test)]
mod bounded_index_tests {
    use super::{DefId, KeymatrixLayer, Layer, MacroSlot, Profile};

    /// `(type name, count, round-trips, rejects)` for each bounded index.
    macro_rules! check_bounds {
        ($ty:ty, $count:expr) => {{
            for v in 0..$count {
                assert_eq!(
                    <$ty>::try_from(v).map(|x| x.get()),
                    Ok(v),
                    "{}: {v} should be in range",
                    stringify!($ty)
                );
            }
            for v in $count..=u8::MAX {
                assert!(
                    <$ty>::try_from(v).is_err(),
                    "{}: {v} should be rejected, not wrapped",
                    stringify!($ty)
                );
            }
        }};
    }

    #[test]
    fn bounded_indexes_reject_out_of_range() {
        check_bounds!(Profile, Profile::COUNT);
        check_bounds!(DefId, DefId::COUNT);
        check_bounds!(MacroSlot, MacroSlot::COUNT);
        check_bounds!(KeymatrixLayer, KeymatrixLayer::COUNT);
        // `Layer` is an enum, so it has no `get`; check it the same way by hand.
        for v in 0..3u8 {
            assert!(Layer::try_from(v).is_ok(), "layer {v} should be in range");
        }
        for v in 3..=u8::MAX {
            assert!(Layer::try_from(v).is_err(), "layer {v} should be rejected");
        }
    }

    /// The `ALL` tables list exactly the in-range values, in order — the loops
    /// that iterate slots depend on it.
    #[test]
    fn all_tables_cover_the_whole_range() {
        assert_eq!(Profile::ALL.len(), Profile::COUNT as usize);
        assert_eq!(DefId::ALL.len(), DefId::COUNT as usize);
        assert_eq!(KeymatrixLayer::ALL.len(), KeymatrixLayer::COUNT as usize);
        for (i, p) in Profile::ALL.iter().enumerate() {
            assert_eq!(p.get(), i as u8);
            // Profiles are shown one-based; the wire value is zero-based.
            assert_eq!(p.number(), i as u8 + 1);
        }
        for (i, d) in DefId::ALL.iter().enumerate() {
            assert_eq!(d.get(), i as u8);
        }
        assert_eq!(DefId::LAST, DefId::ALL[DefId::ALL.len() - 1]);
    }

    /// A slot typed by a user must be reported, not silently redirected —
    /// `parse().unwrap_or(0)` used to send a typo to macro slot 0.
    #[test]
    fn parsing_a_bad_slot_is_an_error_not_slot_zero() {
        for bad in ["", "abc", "-1", "50", "999", " 1"] {
            assert!(
                bad.parse::<MacroSlot>().is_err(),
                "macro slot {bad:?} should not parse"
            );
        }
        assert_eq!("0".parse::<MacroSlot>().map(|m| m.get()), Ok(0));
        assert_eq!("49".parse::<MacroSlot>().map(|m| m.get()), Ok(49));

        for bad in ["", "x", "4", "-1"] {
            assert!(
                bad.parse::<Profile>().is_err(),
                "profile {bad:?} should not parse"
            );
        }
        assert_eq!("3".parse::<Profile>().map(|p| p.get()), Ok(3));
    }
}
