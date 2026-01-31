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
    pub const MACRO: u8 = 9;
    pub const GAMEPAD: u8 = 21;
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
    /// Gamepad button (config_type=21).
    Gamepad(u8),
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
            KeyAction::Macro { index, kind } => [config_type::MACRO, kind, index, 0],
            KeyAction::Gamepad(btn) => [config_type::GAMEPAD, 0, btn, 0],
            KeyAction::Unknown { config_type, data } => [config_type, data[0], data[1], data[2]],
        }
    }

    /// Decode from the 4-byte config format returned by GET_KEYMATRIX.
    pub fn from_config_bytes(bytes: [u8; 4]) -> Self {
        match bytes[0] {
            config_type::KEY => {
                if bytes[2] == 0 && bytes[1] == 0 {
                    KeyAction::Disabled
                } else if bytes[1] != 0 {
                    KeyAction::Combo {
                        mods: bytes[1],
                        key: bytes[2],
                    }
                } else {
                    KeyAction::Key(bytes[2])
                }
            }
            config_type::MOUSE => KeyAction::Mouse(bytes[2]),
            config_type::MACRO => KeyAction::Macro {
                index: bytes[2],
                kind: bytes[1],
            },
            config_type::GAMEPAD => KeyAction::Gamepad(bytes[2]),
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
            KeyAction::Mouse(btn) => write!(f, "Mouse{btn}"),
            KeyAction::Macro { index, kind } => match kind {
                0 => write!(f, "Macro({index})"),
                1 => write!(f, "Macro({index},toggle)"),
                2 => write!(f, "Macro({index},hold)"),
                k => write!(f, "Macro({index},type{k})"),
            },
            KeyAction::Gamepad(btn) => write!(f, "Gamepad({btn})"),
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
    fn wire_roundtrip_gamepad() {
        let a = KeyAction::Gamepad(7);
        assert_eq!(a.to_config_bytes(), [21, 0, 7, 0]);
        assert_eq!(KeyAction::from_config_bytes(a.to_config_bytes()), a);
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
        ];
        for input in cases {
            let action: KeyAction = input.parse().unwrap();
            let displayed = action.to_string();
            let reparsed: KeyAction = displayed.parse().unwrap();
            assert_eq!(action, reparsed, "roundtrip failed for {input:?}");
        }
    }
}
