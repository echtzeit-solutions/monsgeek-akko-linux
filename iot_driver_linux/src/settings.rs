//! Persistent host-side settings (`~/.config/monsgeek/settings.toml`).
//!
//! Small, user-facing knobs that should survive across runs and live alongside
//! the effects library — the audio/screen visualizer refresh rates and the XDG
//! ScreenCast restore token (so screen-reactive mode does not re-prompt the
//! desktop portal picker every time).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::effect::config_dir;
use crate::screen_calib::{ColorCalibration, Region};

/// Default visualizer refresh rate (Hz) for both audio and screen modes.
pub const DEFAULT_RATE_HZ: u32 = 50;

fn default_rate() -> u32 {
    DEFAULT_RATE_HZ
}

/// Persisted host settings. Missing fields fall back to defaults, so older or
/// hand-edited files keep loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Audio visualizer update rate (Hz).
    #[serde(default = "default_rate")]
    pub audio_rate_hz: u32,
    /// Screen visualizer update rate (Hz).
    #[serde(default = "default_rate")]
    pub screen_rate_hz: u32,
    /// XDG ScreenCast restore token, reused to skip the portal picker prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screencast_restore_token: Option<String>,
    /// Screen-sync color calibration (per-channel gain/gamma + saturation).
    #[serde(default)]
    pub screen_calibration: ColorCalibration,
    /// Screen-sync capture region (normalized fractions of the screen).
    #[serde(default)]
    pub screen_region: Region,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio_rate_hz: DEFAULT_RATE_HZ,
            screen_rate_hz: DEFAULT_RATE_HZ,
            screencast_restore_token: None,
            screen_calibration: ColorCalibration::default(),
            screen_region: Region::default(),
        }
    }
}

/// Path to `settings.toml` in the shared config directory.
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.toml")
}

impl Settings {
    /// Load settings, falling back to defaults if the file is missing or invalid
    /// (a corrupt file should never block the app).
    pub fn load() -> Self {
        let path = settings_path();
        let Ok(content) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str(&content) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("settings: parse {}: {e}; using defaults", path.display());
                Self::default()
            }
        }
    }

    /// Persist settings to `settings.toml`, creating the config dir if needed.
    pub fn save(&self) -> Result<(), String> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(&path, content).map_err(|e| format!("write {}: {e}", path.display()))
    }

    /// Load, mutate, and save in one step; logs (does not propagate) save errors.
    pub fn update(f: impl FnOnce(&mut Settings)) {
        let mut s = Self::load();
        f(&mut s);
        if let Err(e) = s.save() {
            tracing::warn!("settings: save failed: {e}");
        }
    }
}
