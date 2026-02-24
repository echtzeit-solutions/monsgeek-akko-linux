//! LED notification daemon — turns keyboard LEDs into an ambient notification display.
//!
//! Architecture:
//! - D-Bus daemon owns the LED stream and exposes `org.monsgeek.Notify1`
//! - Notification sources (tmux, email, scripts) post via D-Bus or CLI
//! - Per-key priority stacks: highest-priority notification wins
//! - 30 FPS render loop evaluates keyframe-based effects and sends RGB frames
//!
//! Effects are defined in `~/.config/monsgeek/effects.toml` using the keyframe
//! engine in `crate::effect`.

#[cfg(feature = "notify")]
pub mod daemon;
#[cfg(feature = "notify")]
pub mod dbus;
pub mod keymap;
pub mod state;
