//! Key action representation for the keyboard protocol.
//!
//! [`KeyAction`] maps 1:1 to the protocol's 4-byte config format
//! `[config_type, b1, b2, b3]` used in GET/SET_KEYMATRIX responses.
//!
//! # Parsing syntax
//!
//! ```text
//! A            → Key(0x04)
//! Escape       → Key(0x29)
//! Esc          → Key(0x29)       (alias)
//! 0x04         → Key(0x04)       (hex literal)
//! Ctrl+C       → Combo(LCtrl+C)
//! Shift+Alt+F3 → Combo(LShift+LAlt+F3)
//! Mouse1       → Mouse(1)
//! Macro(0)     → Macro(index=0, repeat)
//! Macro(2,hold)→ Macro(index=2, hold-to-repeat)
//! Gamepad(1)   → Gamepad(1)
//! Fn           → Fn (layer modifier)
//! Disabled     → Disabled
//! ```

use crate::protocol::hid;
use std::fmt;
use std::str::FromStr;

/// HID modifier bitmask constants (USB HID Report Descriptor modifier byte).
///
/// These match the bit positions in the first byte of a standard HID keyboard
/// report, where each bit corresponds to a modifier key (usage 0xE0-0xE7).
pub mod mods {
    pub const LCTRL: u8 = 0x01;
    pub const LSHIFT: u8 = 0x02;
    pub const LALT: u8 = 0x04;
    pub const LGUI: u8 = 0x08;
    pub const RCTRL: u8 = 0x10;
    pub const RSHIFT: u8 = 0x20;
    pub const RALT: u8 = 0x40;
    pub const RGUI: u8 = 0x80;
}

/// Protocol config_type constants for the 4-byte key config.
mod config_type {
    pub const KEY: u8 = 0;
    pub const MOUSE: u8 = 1;
    pub const CONSUMER: u8 = 3;
    pub const PROFILE_SWITCH: u8 = 8;
    pub const MACRO: u8 = 9;
    pub const SPECIAL_FN: u8 = 10;
    pub const LED_CONTROL: u8 = 13;
    pub const CONNECTION_MODE: u8 = 14;
    pub const KNOB: u8 = 18;
    pub const GAMEPAD: u8 = 21;
}

/// Sub-function IDs for config_type SPECIAL_FN (10).
///
/// Firmware no-ops: sub 0, 4, 6, 7, 0xf-0x16.
mod special_fn {
    pub const FN_KEY: u8 = 1;
    pub const GAME_MODE: u8 = 2;
    pub const WIN_LOCK: u8 = 3;
    // sub 4: no-op
    pub const OS_MODE: u8 = 5;
    // sub 6, 7: no-op
    pub const BT_PAIRING: u8 = 8;
    /// Same as FN_LOCK but without bt_event_queue (silent, no BT notification).
    pub const FN_TOGGLE: u8 = 9;
    pub const WASD_SWAP: u8 = 0x0a;
    pub const NKRO_TOGGLE: u8 = 0x0b;
    /// Fn Lock — toggles flags1 bit 4 with BT notification.
    pub const FN_LOCK: u8 = 0x0c;
    pub const REPORT_MODE: u8 = 0x0d;
    /// Toggles flags2 bit 2 — unknown function.
    pub const FLAGS2_BIT2: u8 = 0x0e;
    // sub 0xf-0x16: no-op
    pub const RCTRL_MOD: u8 = 0x17;
}

/// What action a key performs when pressed.
///
/// Maps 1:1 to the protocol's 4-byte config format `[config_type, b1, b2, b3]`.
/// Use [`from_config_bytes`](KeyAction::from_config_bytes) to decode from wire
/// format and [`to_config_bytes`](KeyAction::to_config_bytes) to encode.
///
/// Implements [`FromStr`] for parsing human-readable syntax and [`Display`]
/// for printing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// Disabled / no action (config_type=0, keycode=0).
    Disabled,
    /// Single HID keycode, no modifiers (config_type=0).
    Key(u8),
    /// Modifier + key combo (config_type=0, modifier bitmask in b1).
    ///
    /// `mods` uses HID modifier bits from the [`mods`] module.
    Combo { mods: u8, key: u8 },
    /// Mouse button (config_type=1).
    Mouse(u8),
    /// Macro assignment (config_type=9).
    ///
    /// `kind`: 0=repeat by count, 1=toggle, 2=hold to repeat.
    Macro { index: u8, kind: u8 },
    /// Consumer/media key — USB HID Consumer Page usage ID (config_type=3).
    Consumer(u16),
    /// Gamepad button (config_type=21).
    Gamepad(u8),
    /// Fn layer modifier key (config_type=10, sub=1).
    Fn,
    /// Special function key (config_type=10, sub != 1).
    ///
    /// Sub-function ID in `sub`, extra data in `b2`/`b3`.
    SpecialFn { sub: u8, b2: u8, b3: u8 },
    /// Profile switch (config_type=8).
    ///
    /// `action`: 1=next, 2=prev, 3=cycle, 4=switch to specific `index`.
    ProfileSwitch { action: u8, index: u8 },
    /// Connection mode switch (config_type=14).
    ///
    /// `b1`=0: mode select (`b2`: 0=BT1, 1=BT2, 2=BT3, 5=2.4G, 6=USB).
    /// `b1`=1: pairing (`b2`: 0=2.4G pair, 1=BT pair).
    ConnectionMode { b1: u8, b2: u8, b3: u8 },
    /// LED brightness/effect control (config_type=13).
    LedControl { data: [u8; 3] },
    /// Knob/encoder action (config_type=18).
    Knob { data: [u8; 3] },
    /// Unknown/unsupported config type (preserved as raw bytes).
    Unknown { config_type: u8, data: [u8; 3] },
}

impl KeyAction {
    /// Encode to the 4-byte config format used in GET/SET_KEYMATRIX.
    pub fn to_config_bytes(self) -> [u8; 4] {
        match self {
            KeyAction::Disabled => [0, 0, 0, 0],
            KeyAction::Key(code) => [0, 0, code, 0],
            KeyAction::Combo { mods, key } => [0, mods, key, 0],
            KeyAction::Mouse(btn) => [config_type::MOUSE, 0, btn, 0],
            KeyAction::Consumer(code) => [config_type::CONSUMER, 0, code as u8, (code >> 8) as u8],
            KeyAction::Macro { index, kind } => [config_type::MACRO, kind, index, 0],
            KeyAction::Gamepad(btn) => [config_type::GAMEPAD, 0, btn, 0],
            KeyAction::Fn => [config_type::SPECIAL_FN, special_fn::FN_KEY, 0, 0],
            KeyAction::SpecialFn { sub, b2, b3 } => [config_type::SPECIAL_FN, sub, b2, b3],
            KeyAction::ProfileSwitch { action, index } => {
                [config_type::PROFILE_SWITCH, 0, action, index]
            }
            KeyAction::ConnectionMode { b1, b2, b3 } => [config_type::CONNECTION_MODE, b1, b2, b3],
            KeyAction::LedControl { data } => [config_type::LED_CONTROL, data[0], data[1], data[2]],
            KeyAction::Knob { data } => [config_type::KNOB, data[0], data[1], data[2]],
            KeyAction::Unknown { config_type, data } => [config_type, data[0], data[1], data[2]],
        }
    }

    /// Decode from the 4-byte config format returned by GET_KEYMATRIX.
    ///
    /// The firmware uses two key-code positions:
    /// - Default/factory keys: `[0, 0, keycode, 0]` — code at byte 2.
    /// - User remaps:          `[0, keycode, 0, 0]` — code at byte 1 (byte 2 = 0).
    /// - Modifier combos:      `[0, mod_mask, keycode, 0]` — both bytes non-zero.
    pub fn from_config_bytes(bytes: [u8; 4]) -> Self {
        match bytes[0] {
            config_type::KEY => {
                if bytes[1] == 0 && bytes[2] == 0 {
                    KeyAction::Disabled
                } else if bytes[1] != 0 && bytes[2] != 0 {
                    // Both non-zero: modifier combo
                    KeyAction::Combo {
                        mods: bytes[1],
                        key: bytes[2],
                    }
                } else if bytes[2] != 0 {
                    // Default/factory format: keycode at byte 2
                    KeyAction::Key(bytes[2])
                } else {
                    // User remap format: keycode at byte 1
                    KeyAction::Key(bytes[1])
                }
            }
            config_type::MOUSE => KeyAction::Mouse(bytes[2]),
            config_type::CONSUMER => {
                let code = bytes[2] as u16 | (bytes[3] as u16) << 8;
                KeyAction::Consumer(code)
            }
            config_type::MACRO => KeyAction::Macro {
                index: bytes[2],
                kind: bytes[1],
            },
            config_type::GAMEPAD => KeyAction::Gamepad(bytes[2]),
            config_type::PROFILE_SWITCH => KeyAction::ProfileSwitch {
                action: bytes[2],
                index: bytes[3],
            },
            config_type::SPECIAL_FN if bytes[1] == special_fn::FN_KEY => KeyAction::Fn,
            config_type::SPECIAL_FN => KeyAction::SpecialFn {
                sub: bytes[1],
                b2: bytes[2],
                b3: bytes[3],
            },
            config_type::LED_CONTROL => KeyAction::LedControl {
                data: [bytes[1], bytes[2], bytes[3]],
            },
            config_type::CONNECTION_MODE => KeyAction::ConnectionMode {
                b1: bytes[1],
                b2: bytes[2],
                b3: bytes[3],
            },
            config_type::KNOB => KeyAction::Knob {
                data: [bytes[1], bytes[2], bytes[3]],
            },
            ct => KeyAction::Unknown {
                config_type: ct,
                data: [bytes[1], bytes[2], bytes[3]],
            },
        }
    }

    /// Returns the HID keycode if this is a simple Key action.
    pub fn hid_code(&self) -> Option<u8> {
        match self {
            KeyAction::Key(code) => Some(*code),
            _ => None,
        }
    }
}

impl fmt::Display for KeyAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyAction::Disabled => write!(f, "Disabled"),
            KeyAction::Key(code) => write!(f, "{}", hid::key_name(*code)),
            KeyAction::Combo { mods, key } => {
                let mod_names: &[(u8, &str)] = &[
                    (mods::LCTRL, "Ctrl"),
                    (mods::LSHIFT, "Shift"),
                    (mods::LALT, "Alt"),
                    (mods::LGUI, "GUI"),
                    (mods::RCTRL, "RCtrl"),
                    (mods::RSHIFT, "RShift"),
                    (mods::RALT, "RAlt"),
                    (mods::RGUI, "RGUI"),
                ];
                let mut first = true;
                for &(bit, name) in mod_names {
                    if mods & bit != 0 {
                        if !first {
                            write!(f, "+")?;
                        }
                        write!(f, "{name}")?;
                        first = false;
                    }
                }
                write!(f, "+{}", hid::key_name(*key))
            }
            KeyAction::Consumer(code) => {
                let name = match code {
                    0x00B5 => "Next Track",
                    0x00B6 => "Previous Track",
                    0x00B7 => "Stop",
                    0x00CD => "Play/Pause",
                    0x00E2 => "Mute",
                    0x00E9 => "Volume Up",
                    0x00EA => "Volume Down",
                    0x006F => "Brightness Up",
                    0x0070 => "Brightness Down",
                    0x0183 => "Word Processor",
                    0x018A => "Mail",
                    0x0192 => "Calculator",
                    0x0194 => "My Computer",
                    0x0221 => "Search",
                    0x0223 => "Browser Home",
                    _ => "",
                };
                if name.is_empty() {
                    write!(f, "Consumer(0x{code:04X})")
                } else {
                    write!(f, "{name}")
                }
            }
            KeyAction::Mouse(btn) => write!(f, "Mouse{btn}"),
            KeyAction::Macro { index, kind } => match kind {
                0 => write!(f, "Macro({index})"),
                1 => write!(f, "Macro({index},toggle)"),
                2 => write!(f, "Macro({index},hold)"),
                k => write!(f, "Macro({index},type{k})"),
            },
            KeyAction::Gamepad(btn) => write!(f, "Gamepad({btn})"),
            KeyAction::Fn => write!(f, "Fn"),
            KeyAction::SpecialFn { sub, b2, b3 } => {
                let name = match *sub {
                    special_fn::GAME_MODE => "Game Mode",
                    special_fn::WIN_LOCK => "Win Lock",
                    special_fn::OS_MODE => match b2 {
                        0 => "OS: Windows",
                        1 => "OS: Mac",
                        2 => "OS: iOS",
                        3 => "OS: Cycle",
                        _ => return write!(f, "OS Mode({b2})"),
                    },
                    special_fn::BT_PAIRING => "BT Pairing",
                    special_fn::FN_TOGGLE => "Fn Toggle",
                    special_fn::WASD_SWAP => "WASD Swap",
                    special_fn::NKRO_TOGGLE => "NKRO Toggle",
                    special_fn::FN_LOCK => "Fn Lock",
                    special_fn::REPORT_MODE => "Report Mode",
                    special_fn::FLAGS2_BIT2 => "SpecialFn(0x0e)",
                    special_fn::RCTRL_MOD => "RCtrl Modifier",
                    _ => return write!(f, "SpecialFn({sub},{b2},{b3})"),
                };
                write!(f, "{name}")
            }
            KeyAction::ProfileSwitch { action, index } => match action {
                1 => write!(f, "Profile Next"),
                2 => write!(f, "Profile Prev"),
                3 => write!(f, "Profile Cycle"),
                4 => write!(f, "Profile {}", index + 1),
                _ => write!(f, "ProfileSwitch({action},{index})"),
            },
            KeyAction::ConnectionMode { b1, b2, .. } => {
                if *b1 == 1 {
                    match b2 {
                        0 => write!(f, "Pair 2.4G"),
                        1 => write!(f, "Pair BT"),
                        _ => write!(f, "Pair({b2})"),
                    }
                } else {
                    // b2 is 0-indexed BT slot; b2=3,4 are no-ops in firmware
                    match b2 {
                        0 => write!(f, "BT1"),
                        1 => write!(f, "BT2"),
                        2 => write!(f, "BT3"),
                        5 => write!(f, "2.4GHz"),
                        6 => write!(f, "USB"),
                        _ => write!(f, "Connection({b2})"),
                    }
                }
            }
            KeyAction::LedControl { data } => {
                let name = match (data[0], data[1], data[2]) {
                    (1, _, _) => "LED Mode Cycle",
                    (2, 1, 0) => "LED Brightness Up",
                    (2, 2, 0) => "LED Brightness Down",
                    (3, 1, 0) => "LED Speed Up",
                    (3, 2, 0) => "LED Speed Down",
                    (5, _, _) => "LED Direction",
                    (6, _, _) => "LED Layer Select",
                    _ => "",
                };
                if name.is_empty() {
                    write!(f, "LedControl({},{},{})", data[0], data[1], data[2])
                } else {
                    write!(f, "{name}")
                }
            }
            KeyAction::Knob { data } => {
                write!(f, "Knob({},{},{})", data[0], data[1], data[2])
            }
            KeyAction::Unknown {
                config_type,
                data: [b1, b2, b3],
            } => write!(
                f,
                "Unknown(type={config_type},data=[{b1:#04x},{b2:#04x},{b3:#04x}])"
            ),
        }
    }
}

/// Error type for parsing a [`KeyAction`] from a string.
#[derive(Debug, Clone)]
pub enum ParseKeyActionError {
    UnknownKey(String),
    UnknownModifier(String),
    InvalidHexCode,
    InvalidMouseButton,
    InvalidMacroIndex,
    InvalidGamepadButton,
    EmptyCombo,
}

impl fmt::Display for ParseKeyActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey(name) => write!(f, "unknown key: \"{name}\""),
            Self::UnknownModifier(name) => write!(f, "unknown modifier: \"{name}\""),
            Self::InvalidHexCode => write!(f, "invalid hex keycode"),
            Self::InvalidMouseButton => write!(f, "invalid mouse button number"),
            Self::InvalidMacroIndex => write!(f, "invalid macro index"),
            Self::InvalidGamepadButton => write!(f, "invalid gamepad button number"),
            Self::EmptyCombo => write!(f, "empty key combo"),
        }
    }
}

impl std::error::Error for ParseKeyActionError {}

/// Parse a modifier name to its bitmask value.
pub fn parse_modifier(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "lctrl" | "lcontrol" => Some(mods::LCTRL),
        "shift" | "lshift" | "lshf" => Some(mods::LSHIFT),
        "alt" | "lalt" | "option" | "loption" => Some(mods::LALT),
        "gui" | "win" | "super" | "cmd" | "lgui" | "lwin" => Some(mods::LGUI),
        "rctrl" | "rcontrol" => Some(mods::RCTRL),
        "rshift" | "rshf" => Some(mods::RSHIFT),
        "ralt" | "roption" | "altgr" => Some(mods::RALT),
        "rgui" | "rwin" | "rsuper" | "rcmd" => Some(mods::RGUI),
        _ => None,
    }
}

impl FromStr for KeyAction {
    type Err = ParseKeyActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Disabled / None
        match s.to_ascii_lowercase().as_str() {
            "disabled" | "none" | "off" => return Ok(KeyAction::Disabled),
            "fn" => return Ok(KeyAction::Fn),
            _ => {}
        }

        // Mouse button: "Mouse1", "mouse(2)"
        if let Some(rest) = s.strip_prefix("Mouse").or_else(|| s.strip_prefix("mouse")) {
            let inner = rest.trim_start_matches('(').trim_end_matches(')');
            let btn: u8 = inner
                .parse()
                .map_err(|_| ParseKeyActionError::InvalidMouseButton)?;
            return Ok(KeyAction::Mouse(btn));
        }

        // Macro: "Macro(0)", "Macro(1,toggle)", "Macro(2,hold)"
        if let Some(rest) = s.strip_prefix("Macro").or_else(|| s.strip_prefix("macro")) {
            let inner = rest.trim_start_matches('(').trim_end_matches(')');
            let parts: Vec<&str> = inner.split(',').collect();
            let index: u8 = parts[0]
                .trim()
                .parse()
                .map_err(|_| ParseKeyActionError::InvalidMacroIndex)?;
            let kind = if parts.len() > 1 {
                match parts[1].trim().to_ascii_lowercase().as_str() {
                    "toggle" => 1,
                    "hold" => 2,
                    "repeat" | "count" => 0,
                    other => other.parse().unwrap_or(0),
                }
            } else {
                0
            };
            return Ok(KeyAction::Macro { index, kind });
        }

        // Gamepad: "Gamepad(1)", "gamepad1"
        if let Some(rest) = s
            .strip_prefix("Gamepad")
            .or_else(|| s.strip_prefix("gamepad"))
        {
            let inner = rest.trim_start_matches('(').trim_end_matches(')');
            let btn: u8 = inner
                .parse()
                .map_err(|_| ParseKeyActionError::InvalidGamepadButton)?;
            return Ok(KeyAction::Gamepad(btn));
        }

        // Hex literal: "0x04", "0X2C"
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            let code =
                u8::from_str_radix(hex, 16).map_err(|_| ParseKeyActionError::InvalidHexCode)?;
            return Ok(if code == 0 {
                KeyAction::Disabled
            } else {
                KeyAction::Key(code)
            });
        }

        // Modifier+Key combo: "Ctrl+C", "Shift+Alt+F3"
        if s.contains('+') {
            let parts: Vec<&str> = s.split('+').collect();
            if parts.len() < 2 {
                return Err(ParseKeyActionError::EmptyCombo);
            }

            // Try parsing all-but-last as modifiers, last as key
            let mut mod_bits = 0u8;
            for &part in &parts[..parts.len() - 1] {
                let part = part.trim();
                mod_bits |= parse_modifier(part)
                    .ok_or_else(|| ParseKeyActionError::UnknownModifier(part.to_string()))?;
            }

            let key_str = parts.last().unwrap().trim();
            let key = hid::key_code_from_name(key_str)
                .ok_or_else(|| ParseKeyActionError::UnknownKey(key_str.to_string()))?;

            return if mod_bits == 0 {
                Ok(KeyAction::Key(key))
            } else {
                Ok(KeyAction::Combo {
                    mods: mod_bits,
                    key,
                })
            };
        }

        // Plain key name: "A", "Enter", "F3", "CapsLock"
        let code = hid::key_code_from_name(s)
            .ok_or_else(|| ParseKeyActionError::UnknownKey(s.to_string()))?;
        Ok(if code == 0 {
            KeyAction::Disabled
        } else {
            KeyAction::Key(code)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- FromStr tests ---

    #[test]
    fn parse_disabled() {
        assert_eq!(
            "Disabled".parse::<KeyAction>().unwrap(),
            KeyAction::Disabled
        );
        assert_eq!("none".parse::<KeyAction>().unwrap(), KeyAction::Disabled);
        assert_eq!("off".parse::<KeyAction>().unwrap(), KeyAction::Disabled);
        assert_eq!("0x00".parse::<KeyAction>().unwrap(), KeyAction::Disabled);
    }

    #[test]
    fn parse_plain_key() {
        assert_eq!("A".parse::<KeyAction>().unwrap(), KeyAction::Key(0x04));
        assert_eq!("a".parse::<KeyAction>().unwrap(), KeyAction::Key(0x04));
        assert_eq!("Enter".parse::<KeyAction>().unwrap(), KeyAction::Key(0x28));
        assert_eq!("Escape".parse::<KeyAction>().unwrap(), KeyAction::Key(0x29));
        assert_eq!("Esc".parse::<KeyAction>().unwrap(), KeyAction::Key(0x29));
        assert_eq!("F3".parse::<KeyAction>().unwrap(), KeyAction::Key(0x3C));
        assert_eq!("F12".parse::<KeyAction>().unwrap(), KeyAction::Key(0x45));
        assert_eq!("Space".parse::<KeyAction>().unwrap(), KeyAction::Key(0x2C));
    }

    #[test]
    fn parse_hex() {
        assert_eq!("0x04".parse::<KeyAction>().unwrap(), KeyAction::Key(0x04));
        assert_eq!("0x29".parse::<KeyAction>().unwrap(), KeyAction::Key(0x29));
        assert_eq!("0xE0".parse::<KeyAction>().unwrap(), KeyAction::Key(0xE0));
    }

    #[test]
    fn parse_f13_through_f24() {
        assert_eq!("F13".parse::<KeyAction>().unwrap(), KeyAction::Key(0x68));
        assert_eq!("F24".parse::<KeyAction>().unwrap(), KeyAction::Key(0x73));
    }

    #[test]
    fn parse_combo() {
        assert_eq!(
            "Ctrl+C".parse::<KeyAction>().unwrap(),
            KeyAction::Combo {
                mods: mods::LCTRL,
                key: 0x06
            }
        );
        assert_eq!(
            "Shift+Alt+F3".parse::<KeyAction>().unwrap(),
            KeyAction::Combo {
                mods: mods::LSHIFT | mods::LALT,
                key: 0x3C
            }
        );
        assert_eq!(
            "RCtrl+RShift+A".parse::<KeyAction>().unwrap(),
            KeyAction::Combo {
                mods: mods::RCTRL | mods::RSHIFT,
                key: 0x04
            }
        );
    }

    #[test]
    fn parse_mouse() {
        assert_eq!("Mouse1".parse::<KeyAction>().unwrap(), KeyAction::Mouse(1));
        assert_eq!(
            "mouse(3)".parse::<KeyAction>().unwrap(),
            KeyAction::Mouse(3)
        );
    }

    #[test]
    fn parse_macro() {
        assert_eq!(
            "Macro(0)".parse::<KeyAction>().unwrap(),
            KeyAction::Macro { index: 0, kind: 0 }
        );
        assert_eq!(
            "Macro(1,toggle)".parse::<KeyAction>().unwrap(),
            KeyAction::Macro { index: 1, kind: 1 }
        );
        assert_eq!(
            "Macro(2,hold)".parse::<KeyAction>().unwrap(),
            KeyAction::Macro { index: 2, kind: 2 }
        );
    }

    #[test]
    fn parse_gamepad() {
        assert_eq!(
            "Gamepad(5)".parse::<KeyAction>().unwrap(),
            KeyAction::Gamepad(5)
        );
    }

    #[test]
    fn parse_fn() {
        assert_eq!("Fn".parse::<KeyAction>().unwrap(), KeyAction::Fn);
        assert_eq!("fn".parse::<KeyAction>().unwrap(), KeyAction::Fn);
    }

    #[test]
    fn parse_error_unknown_key() {
        assert!("Foobar".parse::<KeyAction>().is_err());
    }

    #[test]
    fn parse_error_unknown_modifier() {
        assert!("Hyper+A".parse::<KeyAction>().is_err());
    }

    // --- Display tests ---

    #[test]
    fn display_disabled() {
        assert_eq!(KeyAction::Disabled.to_string(), "Disabled");
    }

    #[test]
    fn display_key() {
        assert_eq!(KeyAction::Key(0x04).to_string(), "A");
        assert_eq!(KeyAction::Key(0x29).to_string(), "Escape");
        assert_eq!(KeyAction::Key(0x28).to_string(), "Enter");
    }

    #[test]
    fn display_combo() {
        assert_eq!(
            KeyAction::Combo {
                mods: mods::LCTRL,
                key: 0x06
            }
            .to_string(),
            "Ctrl+C"
        );
        assert_eq!(
            KeyAction::Combo {
                mods: mods::LSHIFT | mods::LALT,
                key: 0x3C
            }
            .to_string(),
            "Shift+Alt+F3"
        );
    }

    #[test]
    fn display_mouse() {
        assert_eq!(KeyAction::Mouse(1).to_string(), "Mouse1");
    }

    #[test]
    fn display_macro() {
        assert_eq!(
            KeyAction::Macro { index: 0, kind: 0 }.to_string(),
            "Macro(0)"
        );
        assert_eq!(
            KeyAction::Macro { index: 1, kind: 1 }.to_string(),
            "Macro(1,toggle)"
        );
        assert_eq!(
            KeyAction::Macro { index: 2, kind: 2 }.to_string(),
            "Macro(2,hold)"
        );
    }

    #[test]
    fn display_gamepad() {
        assert_eq!(KeyAction::Gamepad(5).to_string(), "Gamepad(5)");
    }

    #[test]
    fn display_fn() {
        assert_eq!(KeyAction::Fn.to_string(), "Fn");
    }

    #[test]
    fn display_unknown() {
        let u = KeyAction::Unknown {
            config_type: 22,
            data: [0x01, 0x02, 0x03],
        };
        assert_eq!(u.to_string(), "Unknown(type=22,data=[0x01,0x02,0x03])");
    }

    // --- Wire roundtrip tests ---

    #[test]
    fn wire_roundtrip_disabled() {
        let a = KeyAction::Disabled;
        assert_eq!(KeyAction::from_config_bytes(a.to_config_bytes()), a);
    }

    #[test]
    fn wire_roundtrip_key() {
        let a = KeyAction::Key(0x04);
        assert_eq!(a.to_config_bytes(), [0, 0, 0x04, 0]);
        assert_eq!(KeyAction::from_config_bytes(a.to_config_bytes()), a);
    }

    #[test]
    fn wire_user_remap_format() {
        // Firmware stores user remaps with keycode at byte 1 (byte 2 = 0)
        assert_eq!(
            KeyAction::from_config_bytes([0, 0x04, 0, 0]),
            KeyAction::Key(0x04) // A
        );
        assert_eq!(
            KeyAction::from_config_bytes([0, 0x29, 0, 0]),
            KeyAction::Key(0x29) // Escape
        );
    }

    #[test]
    fn wire_roundtrip_combo() {
        let a = KeyAction::Combo {
            mods: mods::LCTRL | mods::LSHIFT,
            key: 0x06,
        };
        assert_eq!(a.to_config_bytes(), [0, 0x03, 0x06, 0]);
        assert_eq!(KeyAction::from_config_bytes(a.to_config_bytes()), a);
    }

    #[test]
    fn wire_roundtrip_mouse() {
        let a = KeyAction::Mouse(1);
        assert_eq!(a.to_config_bytes(), [1, 0, 1, 0]);
        assert_eq!(KeyAction::from_config_bytes(a.to_config_bytes()), a);
    }

    #[test]
    fn wire_roundtrip_macro() {
        let a = KeyAction::Macro { index: 3, kind: 1 };
        assert_eq!(a.to_config_bytes(), [9, 1, 3, 0]);
        assert_eq!(KeyAction::from_config_bytes(a.to_config_bytes()), a);
    }

    #[test]
    fn wire_roundtrip_fn() {
        let a = KeyAction::Fn;
        assert_eq!(a.to_config_bytes(), [10, 1, 0, 0]);
        assert_eq!(KeyAction::from_config_bytes(a.to_config_bytes()), a);
    }

    #[test]
    fn wire_special_fn_decoded() {
        // config_type=10 with sub != 1 should decode as SpecialFn
        let bytes = [10, 0x0a, 0, 0]; // WASD Swap
        let a = KeyAction::from_config_bytes(bytes);
        assert_eq!(
            a,
            KeyAction::SpecialFn {
                sub: 0x0a,
                b2: 0,
                b3: 0
            }
        );
        assert_eq!(a.to_config_bytes(), bytes);
        assert_eq!(a.to_string(), "WASD Swap");
    }

    #[test]
    fn wire_roundtrip_gamepad() {
        let a = KeyAction::Gamepad(7);
        assert_eq!(a.to_config_bytes(), [21, 0, 7, 0]);
        assert_eq!(KeyAction::from_config_bytes(a.to_config_bytes()), a);
    }

    #[test]
    fn wire_roundtrip_profile_switch() {
        // Profile 3 (index 2)
        let bytes = [8, 0, 4, 2];
        let a = KeyAction::from_config_bytes(bytes);
        assert_eq!(
            a,
            KeyAction::ProfileSwitch {
                action: 4,
                index: 2
            }
        );
        assert_eq!(a.to_config_bytes(), bytes);
        assert_eq!(a.to_string(), "Profile 3");
    }

    #[test]
    fn wire_roundtrip_connection_mode() {
        // BT1 (0-indexed slot 0)
        let bytes = [14, 0, 0, 0];
        let a = KeyAction::from_config_bytes(bytes);
        assert_eq!(
            a,
            KeyAction::ConnectionMode {
                b1: 0,
                b2: 0,
                b3: 0
            }
        );
        assert_eq!(a.to_config_bytes(), bytes);
        assert_eq!(a.to_string(), "BT1");

        // BT2, BT3
        assert_eq!(
            KeyAction::from_config_bytes([14, 0, 1, 0]).to_string(),
            "BT2"
        );
        assert_eq!(
            KeyAction::from_config_bytes([14, 0, 2, 0]).to_string(),
            "BT3"
        );

        // 2.4GHz
        assert_eq!(
            KeyAction::from_config_bytes([14, 0, 5, 0]).to_string(),
            "2.4GHz"
        );

        // USB
        assert_eq!(
            KeyAction::from_config_bytes([14, 0, 6, 0]).to_string(),
            "USB"
        );
    }

    #[test]
    fn wire_roundtrip_knob() {
        let bytes = [18, 1, 2, 3];
        let a = KeyAction::from_config_bytes(bytes);
        assert_eq!(a, KeyAction::Knob { data: [1, 2, 3] });
        assert_eq!(a.to_config_bytes(), bytes);
    }

    #[test]
    fn display_special_fn_variants() {
        assert_eq!(
            KeyAction::SpecialFn {
                sub: 2,
                b2: 0,
                b3: 0
            }
            .to_string(),
            "Game Mode"
        );
        assert_eq!(
            KeyAction::SpecialFn {
                sub: 3,
                b2: 0,
                b3: 0
            }
            .to_string(),
            "Win Lock"
        );
        assert_eq!(
            KeyAction::SpecialFn {
                sub: 8,
                b2: 0,
                b3: 0
            }
            .to_string(),
            "BT Pairing"
        );
        assert_eq!(
            KeyAction::SpecialFn {
                sub: 0x0c,
                b2: 0,
                b3: 0
            }
            .to_string(),
            "Fn Lock"
        );
        assert_eq!(
            KeyAction::SpecialFn {
                sub: 0x17,
                b2: 0,
                b3: 0
            }
            .to_string(),
            "RCtrl Modifier"
        );
    }

    #[test]
    fn wire_unknown_preserved() {
        let bytes = [22, 0x01, 0x02, 0x03];
        let a = KeyAction::from_config_bytes(bytes);
        assert_eq!(a.to_config_bytes(), bytes);
    }

    // --- Parse → Display roundtrip ---

    #[test]
    fn parse_display_roundtrip() {
        let cases = [
            "Disabled",
            "A",
            "Escape",
            "F3",
            "Ctrl+C",
            "Shift+Alt+F3",
            "Mouse1",
            "Macro(0)",
            "Macro(1,toggle)",
            "Gamepad(5)",
            "Fn",
        ];
        for input in cases {
            let action: KeyAction = input.parse().unwrap();
            let displayed = action.to_string();
            let reparsed: KeyAction = displayed.parse().unwrap();
            assert_eq!(action, reparsed, "roundtrip failed for {input:?}");
        }
    }

    // --- Consumer key tests ---

    #[test]
    fn parse_consumer() {
        // [3, 0, 0xe9, 0] → Consumer(0x00E9) = Volume Up
        assert_eq!(
            KeyAction::from_config_bytes([3, 0, 0xE9, 0]),
            KeyAction::Consumer(0x00E9)
        );
        // [3, 0, 146, 1] → Consumer(0x0192) = Calculator (146 + 1*256 = 402 = 0x192)
        assert_eq!(
            KeyAction::from_config_bytes([3, 0, 0x92, 0x01]),
            KeyAction::Consumer(0x0192)
        );
    }

    #[test]
    fn display_consumer_known() {
        assert_eq!(KeyAction::Consumer(0x00E9).to_string(), "Volume Up");
        assert_eq!(KeyAction::Consumer(0x00CD).to_string(), "Play/Pause");
        assert_eq!(KeyAction::Consumer(0x0192).to_string(), "Calculator");
        assert_eq!(KeyAction::Consumer(0x00B5).to_string(), "Next Track");
        assert_eq!(KeyAction::Consumer(0x00E2).to_string(), "Mute");
    }

    #[test]
    fn display_consumer_unknown() {
        assert_eq!(KeyAction::Consumer(0x1234).to_string(), "Consumer(0x1234)");
    }

    #[test]
    fn wire_roundtrip_consumer() {
        let a = KeyAction::Consumer(0x00E9);
        assert_eq!(a.to_config_bytes(), [3, 0, 0xE9, 0]);
        assert_eq!(KeyAction::from_config_bytes(a.to_config_bytes()), a);

        let b = KeyAction::Consumer(0x0192);
        assert_eq!(b.to_config_bytes(), [3, 0, 0x92, 0x01]);
        assert_eq!(KeyAction::from_config_bytes(b.to_config_bytes()), b);
    }

    // --- LedControl tests ---

    #[test]
    fn parse_led_control() {
        assert_eq!(
            KeyAction::from_config_bytes([13, 2, 1, 0]),
            KeyAction::LedControl { data: [2, 1, 0] }
        );
    }

    #[test]
    fn display_led_control_known() {
        assert_eq!(
            KeyAction::LedControl { data: [2, 1, 0] }.to_string(),
            "LED Brightness Up"
        );
        assert_eq!(
            KeyAction::LedControl { data: [2, 2, 0] }.to_string(),
            "LED Brightness Down"
        );
        assert_eq!(
            KeyAction::LedControl { data: [3, 1, 0] }.to_string(),
            "LED Speed Up"
        );
        assert_eq!(
            KeyAction::LedControl { data: [3, 2, 0] }.to_string(),
            "LED Speed Down"
        );
    }

    #[test]
    fn display_led_control_unknown() {
        assert_eq!(
            KeyAction::LedControl { data: [99, 1, 0] }.to_string(),
            "LedControl(99,1,0)"
        );
    }

    #[test]
    fn wire_roundtrip_led_control() {
        let a = KeyAction::LedControl { data: [2, 1, 0] };
        assert_eq!(a.to_config_bytes(), [13, 2, 1, 0]);
        assert_eq!(KeyAction::from_config_bytes(a.to_config_bytes()), a);
    }
}
