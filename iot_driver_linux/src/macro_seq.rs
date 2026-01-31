//! Macro sequence parser and display.
//!
//! Parses a comma-separated macro sequence syntax into steps, expands
//! them into wire-format events for [`monsgeek_keyboard::KeyboardInterface::set_macro`],
//! and reconstructs the syntax from raw events for display.
//!
//! # Syntax
//!
//! ```text
//! A,B,C                        — tap keys in sequence
//! Ctrl+A,Ctrl+C                — modifier combos
//! A(50ms),100ms,B              — explicit delays
//! A:Press,50ms,A:Release       — explicit press/release
//! ```

use crate::key_action::{self, mods, ParseKeyActionError};
use crate::protocol::hid;
use std::fmt;
use std::str::FromStr;

/// A single step in a macro sequence (user-facing, before expansion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroStep {
    /// Single key tap (press + release).
    Tap { key: u8, delay: Option<u16> },
    /// Modifier combo tap (press mods + key, release key + mods).
    TapCombo {
        mods: u8,
        key: u8,
        delay: Option<u16>,
    },
    /// Explicit key down.
    Down { key: u8, delay: Option<u16> },
    /// Explicit key up.
    Up { key: u8, delay: Option<u16> },
    /// Standalone delay override (applied to preceding event).
    Delay(u16),
}

/// A parsed macro sequence with default delay and repeat count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroSeq {
    pub steps: Vec<MacroStep>,
    pub default_delay: u16,
    pub repeat: u16,
}

/// Error type for parsing a macro sequence.
#[derive(Debug, Clone)]
pub enum ParseMacroSeqError {
    EmptySequence,
    KeyError(ParseKeyActionError),
    InvalidDelay(String),
    InvalidDirection(String),
}

impl fmt::Display for ParseMacroSeqError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySequence => write!(f, "empty macro sequence"),
            Self::KeyError(e) => write!(f, "{e}"),
            Self::InvalidDelay(s) => write!(f, "invalid delay: \"{s}\""),
            Self::InvalidDirection(s) => {
                write!(f, "invalid direction: \"{s}\" (expected Press or Release)")
            }
        }
    }
}

impl std::error::Error for ParseMacroSeqError {}

impl From<ParseKeyActionError> for ParseMacroSeqError {
    fn from(e: ParseKeyActionError) -> Self {
        Self::KeyError(e)
    }
}

/// Parse a delay like `50ms` or `200ms`. Returns milliseconds.
fn parse_delay(s: &str) -> Result<u16, ParseMacroSeqError> {
    let s = s.trim();
    let num_str = s
        .strip_suffix("ms")
        .ok_or_else(|| ParseMacroSeqError::InvalidDelay(s.to_string()))?;
    num_str
        .trim()
        .parse::<u16>()
        .map_err(|_| ParseMacroSeqError::InvalidDelay(s.to_string()))
}

/// Parse a parenthesized delay suffix like `(50ms)` from the end of a token.
/// Returns (token_without_delay, optional_delay).
fn split_paren_delay(s: &str) -> (&str, Option<u16>) {
    if let Some(open) = s.rfind('(') {
        if s.ends_with(')') {
            let inner = &s[open + 1..s.len() - 1];
            if let Ok(delay) = parse_delay(inner) {
                return (&s[..open], Some(delay));
            }
        }
    }
    (s, None)
}

/// Parse a key name (possibly with `+` modifiers) into (mod_bitmask, keycode).
/// Returns (0, keycode) for plain keys, (mods, keycode) for combos.
fn parse_key_spec(s: &str) -> Result<(u8, u8), ParseMacroSeqError> {
    let s = s.trim();
    if s.contains('+') {
        let parts: Vec<&str> = s.split('+').collect();
        if parts.len() < 2 {
            return Err(ParseKeyActionError::EmptyCombo.into());
        }
        let mut mod_bits = 0u8;
        for &part in &parts[..parts.len() - 1] {
            let part = part.trim();
            mod_bits |= key_action::parse_modifier(part)
                .ok_or_else(|| ParseKeyActionError::UnknownModifier(part.to_string()))?;
        }
        let key_str = parts.last().unwrap().trim();
        let key = hid::key_code_from_name(key_str)
            .ok_or_else(|| ParseKeyActionError::UnknownKey(key_str.to_string()))?;
        Ok((mod_bits, key))
    } else {
        let key = hid::key_code_from_name(s)
            .ok_or_else(|| ParseKeyActionError::UnknownKey(s.to_string()))?;
        Ok((0, key))
    }
}

/// Check if a token is a standalone delay (e.g. `100ms`).
fn is_delay_token(s: &str) -> bool {
    let s = s.trim();
    s.ends_with("ms") && s[..s.len() - 2].trim().parse::<u16>().is_ok()
}

impl FromStr for MacroSeq {
    type Err = ParseMacroSeqError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ParseMacroSeqError::EmptySequence);
        }

        let mut steps = Vec::new();

        for token in s.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }

            // Standalone delay: "100ms"
            if is_delay_token(token) {
                let ms = parse_delay(token)?;
                steps.push(MacroStep::Delay(ms));
                continue;
            }

            // Check for :Press or :Release suffix (before paren delay)
            let (token_body, paren_delay) = split_paren_delay(token);

            if let Some(colon_pos) = token_body.rfind(':') {
                let key_part = &token_body[..colon_pos];
                let dir_part = &token_body[colon_pos + 1..];

                match dir_part.to_ascii_lowercase().as_str() {
                    "press" | "down" => {
                        let key = resolve_single_key(key_part)?;
                        steps.push(MacroStep::Down {
                            key,
                            delay: paren_delay,
                        });
                    }
                    "release" | "up" => {
                        let key = resolve_single_key(key_part)?;
                        steps.push(MacroStep::Up {
                            key,
                            delay: paren_delay,
                        });
                    }
                    _ => {
                        return Err(ParseMacroSeqError::InvalidDirection(dir_part.to_string()));
                    }
                }
                continue;
            }

            // Key tap or combo tap (possibly with paren delay)
            let (mods, key) = parse_key_spec(token_body)?;
            if mods != 0 {
                steps.push(MacroStep::TapCombo {
                    mods,
                    key,
                    delay: paren_delay,
                });
            } else {
                steps.push(MacroStep::Tap {
                    key,
                    delay: paren_delay,
                });
            }
        }

        if steps.is_empty() {
            return Err(ParseMacroSeqError::EmptySequence);
        }

        Ok(MacroSeq {
            steps,
            default_delay: 10,
            repeat: 1,
        })
    }
}

/// Resolve a name like "Ctrl" to its HID keycode (0xE0) rather than
/// treating it as a modifier bitmask. Used for `:Press`/`:Release` tokens.
fn resolve_single_key(name: &str) -> Result<u8, ParseMacroSeqError> {
    let name = name.trim();

    // Try direct key code lookup first
    if let Some(code) = hid::key_code_from_name(name) {
        return Ok(code);
    }

    // Map modifier names to their HID keycodes
    match name.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "lctrl" | "lcontrol" => Ok(0xE0),
        "shift" | "lshift" | "lshf" => Ok(0xE1),
        "alt" | "lalt" | "option" | "loption" => Ok(0xE2),
        "gui" | "win" | "super" | "cmd" | "lgui" | "lwin" => Ok(0xE3),
        "rctrl" | "rcontrol" => Ok(0xE4),
        "rshift" | "rshf" => Ok(0xE5),
        "ralt" | "roption" | "altgr" => Ok(0xE6),
        "rgui" | "rwin" | "rsuper" | "rcmd" => Ok(0xE7),
        _ => Err(ParseKeyActionError::UnknownKey(name.to_string()).into()),
    }
}

/// Convert modifier bitmask to list of HID keycodes for the modifier keys.
fn mod_bitmask_to_keycodes(mod_bits: u8) -> Vec<u8> {
    let mut codes = Vec::new();
    if mod_bits & mods::LCTRL != 0 {
        codes.push(0xE0);
    }
    if mod_bits & mods::LSHIFT != 0 {
        codes.push(0xE1);
    }
    if mod_bits & mods::LALT != 0 {
        codes.push(0xE2);
    }
    if mod_bits & mods::LGUI != 0 {
        codes.push(0xE3);
    }
    if mod_bits & mods::RCTRL != 0 {
        codes.push(0xE4);
    }
    if mod_bits & mods::RSHIFT != 0 {
        codes.push(0xE5);
    }
    if mod_bits & mods::RALT != 0 {
        codes.push(0xE6);
    }
    if mod_bits & mods::RGUI != 0 {
        codes.push(0xE7);
    }
    codes
}

/// Convert a HID keycode for a modifier (0xE0-0xE7) to its bitmask bit.
fn keycode_to_mod_bit(code: u8) -> Option<u8> {
    match code {
        0xE0 => Some(mods::LCTRL),
        0xE1 => Some(mods::LSHIFT),
        0xE2 => Some(mods::LALT),
        0xE3 => Some(mods::LGUI),
        0xE4 => Some(mods::RCTRL),
        0xE5 => Some(mods::RSHIFT),
        0xE6 => Some(mods::RALT),
        0xE7 => Some(mods::RGUI),
        _ => None,
    }
}

/// Check if a HID keycode is a modifier key (0xE0-0xE7).
fn is_modifier(code: u8) -> bool {
    (0xE0..=0xE7).contains(&code)
}

impl MacroSeq {
    /// Expand the macro sequence into wire-format events: `(keycode, is_down, delay_ms)`.
    pub fn to_events(&self) -> Vec<(u8, bool, u16)> {
        let d = self.default_delay;
        let mut events: Vec<(u8, bool, u16)> = Vec::new();

        for (i, step) in self.steps.iter().enumerate() {
            match step {
                MacroStep::Tap { key, delay } => {
                    let release_delay = delay.unwrap_or(d);
                    events.push((*key, true, d));
                    events.push((*key, false, release_delay));
                }
                MacroStep::TapCombo { mods, key, delay } => {
                    let mod_keys = mod_bitmask_to_keycodes(*mods);
                    // Press modifiers with 0ms delay
                    for &mk in &mod_keys {
                        events.push((mk, true, 0));
                    }
                    // Press key with default delay
                    events.push((*key, true, d));
                    // Release key with 0ms delay
                    events.push((*key, false, 0));
                    // Release modifiers in reverse order
                    let last_mod = mod_keys.len().saturating_sub(1);
                    for (j, &mk) in mod_keys.iter().rev().enumerate() {
                        let mod_delay = if j == last_mod { delay.unwrap_or(d) } else { 0 };
                        events.push((mk, false, mod_delay));
                    }
                }
                MacroStep::Down { key, delay } => {
                    events.push((*key, true, delay.unwrap_or(d)));
                }
                MacroStep::Up { key, delay } => {
                    events.push((*key, false, delay.unwrap_or(d)));
                }
                MacroStep::Delay(ms) => {
                    // Standalone delay: override the preceding event's delay
                    if let Some(last) = events.last_mut() {
                        last.2 = *ms;
                    }
                    // If there's no preceding event, the delay is dropped
                    // (no-op at start of sequence). This is intentional—
                    // the standalone delay syntax is defined as overriding
                    // the *preceding* event's delay.
                    let _ = i; // suppress unused warning
                }
            }
        }

        events
    }

    /// Reconstruct a `MacroSeq` from raw macro events (as returned by
    /// `parse_macro_events`). This does pattern matching to recover
    /// high-level steps from the flat event list.
    pub fn from_events(events: &[(u8, bool, u16)], default_delay: u16, repeat: u16) -> Self {
        let mut steps = Vec::new();
        let mut i = 0;
        let len = events.len();

        while i < len {
            let (code, is_down, delay) = events[i];

            // Try to match combo tap: ↓Mod(0ms)... ↓Key(d) ↑Key(0ms) ↑Mod...(d2)
            if is_down && is_modifier(code) && delay == 0 {
                if let Some((combo_step, consumed)) = try_match_combo(&events[i..], default_delay) {
                    steps.push(combo_step);
                    i += consumed;
                    continue;
                }
            }

            // Try to match simple tap: ↓K(default_delay) ↑K(d2) (consecutive, same keycode)
            // Only match as tap if the down event uses the default delay —
            // a non-default down delay means an explicit Press with custom timing.
            if is_down && delay == default_delay && i + 1 < len {
                let (next_code, next_down, next_delay) = events[i + 1];
                if !next_down && next_code == code {
                    // It's a tap
                    let tap_delay = if next_delay != default_delay {
                        Some(next_delay)
                    } else {
                        None
                    };
                    steps.push(MacroStep::Tap {
                        key: code,
                        delay: tap_delay,
                    });
                    i += 2;
                    continue;
                }
            }

            // Explicit press or release
            if is_down {
                let step_delay = if delay != default_delay {
                    Some(delay)
                } else {
                    None
                };
                steps.push(MacroStep::Down {
                    key: code,
                    delay: step_delay,
                });
            } else {
                let step_delay = if delay != default_delay {
                    Some(delay)
                } else {
                    None
                };
                steps.push(MacroStep::Up {
                    key: code,
                    delay: step_delay,
                });
            }
            i += 1;
        }

        MacroSeq {
            steps,
            default_delay,
            repeat,
        }
    }
}

/// Try to match a combo tap pattern starting at `events[0]`.
/// Pattern: ↓Mod1(0)... ↓ModN(0) ↓Key(d) ↑Key(0) ↑ModN(0)... ↑Mod1(d2)
/// Returns the MacroStep and number of events consumed.
fn try_match_combo(events: &[(u8, bool, u16)], default_delay: u16) -> Option<(MacroStep, usize)> {
    // Collect modifier presses (all must be down with 0ms delay)
    let mut mod_count = 0;
    let mut mod_bits = 0u8;
    let mut mod_codes: Vec<u8> = Vec::new();

    for &(code, is_down, delay) in events.iter() {
        if is_down && is_modifier(code) && delay == 0 {
            mod_bits |= keycode_to_mod_bit(code)?;
            mod_codes.push(code);
            mod_count += 1;
        } else {
            break;
        }
    }

    if mod_count == 0 {
        return None;
    }

    // Next should be ↓Key(d)
    let key_down_idx = mod_count;
    if key_down_idx >= events.len() {
        return None;
    }
    let (key_code, key_is_down, _key_delay) = events[key_down_idx];
    if !key_is_down || is_modifier(key_code) {
        return None;
    }

    // Next should be ↑Key(0)
    let key_up_idx = key_down_idx + 1;
    if key_up_idx >= events.len() {
        return None;
    }
    let (up_code, up_is_down, up_delay) = events[key_up_idx];
    if up_code != key_code || up_is_down || up_delay != 0 {
        return None;
    }

    // Next should be ↑Mods in reverse order
    let mod_release_start = key_up_idx + 1;
    for (j, &expected_code) in mod_codes.iter().rev().enumerate() {
        let idx = mod_release_start + j;
        if idx >= events.len() {
            return None;
        }
        let (rel_code, rel_is_down, rel_delay) = events[idx];
        if rel_code != expected_code || rel_is_down {
            return None;
        }
        // All but the last mod release should have 0ms delay
        if j < mod_codes.len() - 1 && rel_delay != 0 {
            return None;
        }
    }

    let last_mod_idx = mod_release_start + mod_codes.len() - 1;
    let last_mod_delay = events[last_mod_idx].2;
    let combo_delay = if last_mod_delay != default_delay {
        Some(last_mod_delay)
    } else {
        None
    };

    let total_consumed = last_mod_idx + 1;
    Some((
        MacroStep::TapCombo {
            mods: mod_bits,
            key: key_code,
            delay: combo_delay,
        },
        total_consumed,
    ))
}

/// Format a key name for display, using the HID name.
fn fmt_key(code: u8) -> &'static str {
    hid::key_name(code)
}

/// Format a modifier bitmask as `Ctrl+Shift+...` prefix.
fn fmt_mods(mod_bits: u8) -> String {
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
    let mut parts = Vec::new();
    for &(bit, name) in mod_names {
        if mod_bits & bit != 0 {
            parts.push(name);
        }
    }
    parts.join("+")
}

impl fmt::Display for MacroStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MacroStep::Tap { key, delay } => {
                write!(f, "{}", fmt_key(*key))?;
                if let Some(d) = delay {
                    write!(f, "({d}ms)")?;
                }
                Ok(())
            }
            MacroStep::TapCombo { mods, key, delay } => {
                write!(f, "{}+{}", fmt_mods(*mods), fmt_key(*key))?;
                if let Some(d) = delay {
                    write!(f, "({d}ms)")?;
                }
                Ok(())
            }
            MacroStep::Down { key, delay } => {
                write!(f, "{}:Press", fmt_key(*key))?;
                if let Some(d) = delay {
                    write!(f, "({d}ms)")?;
                }
                Ok(())
            }
            MacroStep::Up { key, delay } => {
                write!(f, "{}:Release", fmt_key(*key))?;
                if let Some(d) = delay {
                    write!(f, "({d}ms)")?;
                }
                Ok(())
            }
            MacroStep::Delay(ms) => write!(f, "{ms}ms"),
        }
    }
}

impl fmt::Display for MacroSeq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            write!(f, "{step}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Parsing tests ---

    #[test]
    fn parse_single_tap() {
        let seq: MacroSeq = "A".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![MacroStep::Tap {
                key: 0x04,
                delay: None
            }]
        );
    }

    #[test]
    fn parse_multiple_taps() {
        let seq: MacroSeq = "A,B,C".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![
                MacroStep::Tap {
                    key: 0x04,
                    delay: None
                },
                MacroStep::Tap {
                    key: 0x05,
                    delay: None
                },
                MacroStep::Tap {
                    key: 0x06,
                    delay: None
                },
            ]
        );
    }

    #[test]
    fn parse_tap_with_delay() {
        let seq: MacroSeq = "A(50ms)".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![MacroStep::Tap {
                key: 0x04,
                delay: Some(50)
            }]
        );
    }

    #[test]
    fn parse_combo() {
        let seq: MacroSeq = "Ctrl+A".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![MacroStep::TapCombo {
                mods: mods::LCTRL,
                key: 0x04,
                delay: None,
            }]
        );
    }

    #[test]
    fn parse_combo_with_delay() {
        let seq: MacroSeq = "Ctrl+A(50ms)".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![MacroStep::TapCombo {
                mods: mods::LCTRL,
                key: 0x04,
                delay: Some(50),
            }]
        );
    }

    #[test]
    fn parse_multi_mod_combo() {
        let seq: MacroSeq = "Ctrl+Shift+A".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![MacroStep::TapCombo {
                mods: mods::LCTRL | mods::LSHIFT,
                key: 0x04,
                delay: None,
            }]
        );
    }

    #[test]
    fn parse_standalone_delay() {
        let seq: MacroSeq = "A,100ms,B".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![
                MacroStep::Tap {
                    key: 0x04,
                    delay: None
                },
                MacroStep::Delay(100),
                MacroStep::Tap {
                    key: 0x05,
                    delay: None
                },
            ]
        );
    }

    #[test]
    fn parse_explicit_press_release() {
        let seq: MacroSeq = "A:Press,50ms,A:Release".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![
                MacroStep::Down {
                    key: 0x04,
                    delay: None
                },
                MacroStep::Delay(50),
                MacroStep::Up {
                    key: 0x04,
                    delay: None
                },
            ]
        );
    }

    #[test]
    fn parse_explicit_press_with_delay() {
        let seq: MacroSeq = "A:Press(50ms)".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![MacroStep::Down {
                key: 0x04,
                delay: Some(50)
            }]
        );
    }

    #[test]
    fn parse_modifier_press_release() {
        let seq: MacroSeq = "Ctrl:Press,A:Press,A:Release,Ctrl:Release".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![
                MacroStep::Down {
                    key: 0xE0,
                    delay: None
                },
                MacroStep::Down {
                    key: 0x04,
                    delay: None
                },
                MacroStep::Up {
                    key: 0x04,
                    delay: None
                },
                MacroStep::Up {
                    key: 0xE0,
                    delay: None
                },
            ]
        );
    }

    #[test]
    fn parse_special_keys() {
        let seq: MacroSeq = "Enter,Escape,Space".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![
                MacroStep::Tap {
                    key: 0x28,
                    delay: None
                },
                MacroStep::Tap {
                    key: 0x29,
                    delay: None
                },
                MacroStep::Tap {
                    key: 0x2C,
                    delay: None
                },
            ]
        );
    }

    #[test]
    fn parse_f_keys() {
        let seq: MacroSeq = "F1,F12".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![
                MacroStep::Tap {
                    key: 0x3A,
                    delay: None
                },
                MacroStep::Tap {
                    key: 0x45,
                    delay: None
                },
            ]
        );
    }

    #[test]
    fn parse_aliases() {
        let seq: MacroSeq = "Esc".parse().unwrap();
        assert_eq!(
            seq.steps,
            vec![MacroStep::Tap {
                key: 0x29,
                delay: None
            }]
        );
    }

    #[test]
    fn parse_empty_is_error() {
        assert!("".parse::<MacroSeq>().is_err());
    }

    #[test]
    fn parse_bad_key_is_error() {
        assert!("Foobar".parse::<MacroSeq>().is_err());
    }

    #[test]
    fn parse_bad_direction_is_error() {
        assert!("A:Sideways".parse::<MacroSeq>().is_err());
    }

    #[test]
    fn parse_whitespace_tolerance() {
        let seq: MacroSeq = " A , B , C ".parse().unwrap();
        assert_eq!(seq.steps.len(), 3);
    }

    // --- Display tests ---

    #[test]
    fn display_tap() {
        let step = MacroStep::Tap {
            key: 0x04,
            delay: None,
        };
        assert_eq!(step.to_string(), "A");
    }

    #[test]
    fn display_tap_with_delay() {
        let step = MacroStep::Tap {
            key: 0x04,
            delay: Some(50),
        };
        assert_eq!(step.to_string(), "A(50ms)");
    }

    #[test]
    fn display_combo() {
        let step = MacroStep::TapCombo {
            mods: mods::LCTRL,
            key: 0x04,
            delay: None,
        };
        assert_eq!(step.to_string(), "Ctrl+A");
    }

    #[test]
    fn display_combo_with_delay() {
        let step = MacroStep::TapCombo {
            mods: mods::LCTRL,
            key: 0x04,
            delay: Some(50),
        };
        assert_eq!(step.to_string(), "Ctrl+A(50ms)");
    }

    #[test]
    fn display_press() {
        let step = MacroStep::Down {
            key: 0x04,
            delay: None,
        };
        assert_eq!(step.to_string(), "A:Press");
    }

    #[test]
    fn display_release() {
        let step = MacroStep::Up {
            key: 0x04,
            delay: None,
        };
        assert_eq!(step.to_string(), "A:Release");
    }

    #[test]
    fn display_delay() {
        let step = MacroStep::Delay(100);
        assert_eq!(step.to_string(), "100ms");
    }

    #[test]
    fn display_full_sequence() {
        let seq = MacroSeq {
            steps: vec![
                MacroStep::TapCombo {
                    mods: mods::LCTRL,
                    key: 0x04,
                    delay: Some(50),
                },
                MacroStep::TapCombo {
                    mods: mods::LCTRL,
                    key: 0x06,
                    delay: None,
                },
            ],
            default_delay: 10,
            repeat: 1,
        };
        assert_eq!(seq.to_string(), "Ctrl+A(50ms),Ctrl+C");
    }

    // --- Event expansion tests ---

    #[test]
    fn expand_simple_tap() {
        let seq = MacroSeq {
            steps: vec![MacroStep::Tap {
                key: 0x04,
                delay: None,
            }],
            default_delay: 10,
            repeat: 1,
        };
        let events = seq.to_events();
        assert_eq!(
            events,
            vec![
                (0x04, true, 10),  // ↓A +10ms
                (0x04, false, 10), // ↑A +10ms
            ]
        );
    }

    #[test]
    fn expand_tap_with_delay() {
        let seq = MacroSeq {
            steps: vec![MacroStep::Tap {
                key: 0x04,
                delay: Some(50),
            }],
            default_delay: 10,
            repeat: 1,
        };
        let events = seq.to_events();
        assert_eq!(
            events,
            vec![
                (0x04, true, 10),  // ↓A +10ms
                (0x04, false, 50), // ↑A +50ms
            ]
        );
    }

    #[test]
    fn expand_combo() {
        let seq = MacroSeq {
            steps: vec![MacroStep::TapCombo {
                mods: mods::LCTRL,
                key: 0x04,
                delay: None,
            }],
            default_delay: 10,
            repeat: 1,
        };
        let events = seq.to_events();
        assert_eq!(
            events,
            vec![
                (0xE0, true, 0),   // ↓LCtrl +0ms
                (0x04, true, 10),  // ↓A +10ms
                (0x04, false, 0),  // ↑A +0ms
                (0xE0, false, 10), // ↑LCtrl +10ms
            ]
        );
    }

    #[test]
    fn expand_combo_with_delay() {
        let seq = MacroSeq {
            steps: vec![MacroStep::TapCombo {
                mods: mods::LCTRL,
                key: 0x04,
                delay: Some(50),
            }],
            default_delay: 10,
            repeat: 1,
        };
        let events = seq.to_events();
        assert_eq!(
            events,
            vec![
                (0xE0, true, 0),   // ↓LCtrl +0ms
                (0x04, true, 10),  // ↓A +10ms
                (0x04, false, 0),  // ↑A +0ms
                (0xE0, false, 50), // ↑LCtrl +50ms
            ]
        );
    }

    #[test]
    fn expand_multi_mod_combo() {
        let seq = MacroSeq {
            steps: vec![MacroStep::TapCombo {
                mods: mods::LCTRL | mods::LSHIFT,
                key: 0x04,
                delay: None,
            }],
            default_delay: 10,
            repeat: 1,
        };
        let events = seq.to_events();
        assert_eq!(
            events,
            vec![
                (0xE0, true, 0),   // ↓LCtrl +0ms
                (0xE1, true, 0),   // ↓LShift +0ms
                (0x04, true, 10),  // ↓A +10ms
                (0x04, false, 0),  // ↑A +0ms
                (0xE1, false, 0),  // ↑LShift +0ms
                (0xE0, false, 10), // ↑LCtrl +10ms
            ]
        );
    }

    #[test]
    fn expand_standalone_delay() {
        let seq = MacroSeq {
            steps: vec![
                MacroStep::Tap {
                    key: 0x04,
                    delay: None,
                },
                MacroStep::Delay(100),
                MacroStep::Tap {
                    key: 0x05,
                    delay: None,
                },
            ],
            default_delay: 10,
            repeat: 1,
        };
        let events = seq.to_events();
        // Standalone delay overrides preceding event's delay
        assert_eq!(
            events,
            vec![
                (0x04, true, 10),   // ↓A +10ms
                (0x04, false, 100), // ↑A +100ms (overridden by standalone delay)
                (0x05, true, 10),   // ↓B +10ms
                (0x05, false, 10),  // ↑B +10ms
            ]
        );
    }

    #[test]
    fn expand_explicit_press_release() {
        let seq = MacroSeq {
            steps: vec![
                MacroStep::Down {
                    key: 0x04,
                    delay: Some(50),
                },
                MacroStep::Up {
                    key: 0x04,
                    delay: None,
                },
            ],
            default_delay: 10,
            repeat: 1,
        };
        let events = seq.to_events();
        assert_eq!(
            events,
            vec![
                (0x04, true, 50),  // ↓A +50ms
                (0x04, false, 10), // ↑A +10ms
            ]
        );
    }

    // --- from_events reconstruction tests ---

    #[test]
    fn reconstruct_simple_tap() {
        let events = vec![(0x04, true, 10), (0x04, false, 10)];
        let seq = MacroSeq::from_events(&events, 10, 1);
        assert_eq!(
            seq.steps,
            vec![MacroStep::Tap {
                key: 0x04,
                delay: None
            }]
        );
    }

    #[test]
    fn reconstruct_tap_with_non_default_delay() {
        let events = vec![(0x04, true, 10), (0x04, false, 50)];
        let seq = MacroSeq::from_events(&events, 10, 1);
        assert_eq!(
            seq.steps,
            vec![MacroStep::Tap {
                key: 0x04,
                delay: Some(50)
            }]
        );
    }

    #[test]
    fn reconstruct_combo() {
        let events = vec![
            (0xE0, true, 0),
            (0x04, true, 10),
            (0x04, false, 0),
            (0xE0, false, 10),
        ];
        let seq = MacroSeq::from_events(&events, 10, 1);
        assert_eq!(
            seq.steps,
            vec![MacroStep::TapCombo {
                mods: mods::LCTRL,
                key: 0x04,
                delay: None,
            }]
        );
    }

    #[test]
    fn reconstruct_combo_with_delay() {
        let events = vec![
            (0xE0, true, 0),
            (0x04, true, 10),
            (0x04, false, 0),
            (0xE0, false, 50),
        ];
        let seq = MacroSeq::from_events(&events, 10, 1);
        assert_eq!(
            seq.steps,
            vec![MacroStep::TapCombo {
                mods: mods::LCTRL,
                key: 0x04,
                delay: Some(50),
            }]
        );
    }

    #[test]
    fn reconstruct_explicit_press_release() {
        // Events that don't match tap or combo patterns
        let events = vec![
            (0x04, true, 50),
            (0x05, true, 10),
            (0x04, false, 10),
            (0x05, false, 10),
        ];
        let seq = MacroSeq::from_events(&events, 10, 1);
        assert_eq!(
            seq.steps,
            vec![
                MacroStep::Down {
                    key: 0x04,
                    delay: Some(50)
                },
                MacroStep::Down {
                    key: 0x05,
                    delay: None
                },
                MacroStep::Up {
                    key: 0x04,
                    delay: None
                },
                MacroStep::Up {
                    key: 0x05,
                    delay: None
                },
            ]
        );
    }

    // --- Roundtrip tests (parse → expand → reconstruct → display) ---

    #[test]
    fn roundtrip_simple_taps() {
        let input = "A,B,C";
        let seq: MacroSeq = input.parse().unwrap();
        let events = seq.to_events();
        let reconstructed = MacroSeq::from_events(&events, seq.default_delay, seq.repeat);
        assert_eq!(reconstructed.to_string(), input);
    }

    #[test]
    fn roundtrip_combo() {
        let input = "Ctrl+A";
        let seq: MacroSeq = input.parse().unwrap();
        let events = seq.to_events();
        let reconstructed = MacroSeq::from_events(&events, seq.default_delay, seq.repeat);
        assert_eq!(reconstructed.to_string(), input);
    }

    #[test]
    fn roundtrip_combo_with_delay() {
        let input = "Ctrl+A(50ms)";
        let seq: MacroSeq = input.parse().unwrap();
        let events = seq.to_events();
        let reconstructed = MacroSeq::from_events(&events, seq.default_delay, seq.repeat);
        assert_eq!(reconstructed.to_string(), input);
    }

    #[test]
    fn roundtrip_mixed() {
        let input = "Ctrl+A(50ms),Ctrl+C";
        let seq: MacroSeq = input.parse().unwrap();
        let events = seq.to_events();
        let reconstructed = MacroSeq::from_events(&events, seq.default_delay, seq.repeat);
        assert_eq!(reconstructed.to_string(), input);
    }

    #[test]
    fn roundtrip_multi_mod_combo() {
        let input = "Ctrl+Shift+A";
        let seq: MacroSeq = input.parse().unwrap();
        let events = seq.to_events();
        let reconstructed = MacroSeq::from_events(&events, seq.default_delay, seq.repeat);
        assert_eq!(reconstructed.to_string(), input);
    }

    #[test]
    fn roundtrip_explicit_press_release() {
        let input = "A:Press(50ms),A:Release";
        let seq: MacroSeq = input.parse().unwrap();
        let events = seq.to_events();
        let reconstructed = MacroSeq::from_events(&events, seq.default_delay, seq.repeat);
        assert_eq!(reconstructed.to_string(), input);
    }

    #[test]
    fn roundtrip_taps_with_delays() {
        let input = "A(50ms),B(100ms)";
        let seq: MacroSeq = input.parse().unwrap();
        let events = seq.to_events();
        let reconstructed = MacroSeq::from_events(&events, seq.default_delay, seq.repeat);
        assert_eq!(reconstructed.to_string(), input);
    }
}
