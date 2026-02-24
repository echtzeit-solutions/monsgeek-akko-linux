//! Keyframe-based LED effect engine.
//!
//! Effects are defined in TOML with named keyframes that specify time, color,
//! brightness, and easing. Color variables (`$name`) allow effects to be
//! reusable templates, resolved at trigger time.
//!
//! # Example TOML
//!
//! ```toml
//! [breathe]
//! color = "$color"
//! keyframes = [
//!     { t = 0,    v = 0.0, easing = "EaseInOut" },
//!     { t = 1000, v = 1.0, easing = "EaseInOut" },
//!     { t = 2000, v = 0.0 },
//! ]
//! ```

pub mod preview;

use keyframe::functions as ease;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ── Rgb ──────────────────────────────────────────────────────────────

/// RGB color tuple.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };

    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Create from HSV (h: 0-360, s: 0-1, v: 0-1).
    pub fn from_hsv(h: f32, s: f32, v: f32) -> Self {
        let h = h % 360.0;
        let s = s.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);
        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;
        let (r, g, b) = match (h / 60.0) as i32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        Self {
            r: ((r + m) * 255.0) as u8,
            g: ((g + m) * 255.0) as u8,
            b: ((b + m) * 255.0) as u8,
        }
    }

    /// Scale brightness by a factor in [0, 1].
    pub fn scale(self, factor: f32) -> Self {
        let f = factor.clamp(0.0, 1.0);
        Self {
            r: (self.r as f32 * f) as u8,
            g: (self.g as f32 * f) as u8,
            b: (self.b as f32 * f) as u8,
        }
    }

    /// Linearly interpolate between two colors.
    pub fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        Rgb {
            r: (a.r as f32 + (b.r as f32 - a.r as f32) * t) as u8,
            g: (a.g as f32 + (b.g as f32 - a.g as f32) * t) as u8,
            b: (a.b as f32 + (b.b as f32 - a.b as f32) * t) as u8,
        }
    }

    /// Parse a color string: "#RRGGBB", "red", "green", etc.
    pub fn parse(s: &str) -> Option<Self> {
        if let Some(hex) = s.strip_prefix('#') {
            if hex.len() == 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                return Some(Self::new(r, g, b));
            }
        }
        match s.to_ascii_lowercase().as_str() {
            "red" => Some(Self::new(255, 0, 0)),
            "green" => Some(Self::new(0, 255, 0)),
            "blue" => Some(Self::new(0, 0, 255)),
            "yellow" => Some(Self::new(255, 255, 0)),
            "cyan" => Some(Self::new(0, 255, 255)),
            "magenta" | "pink" => Some(Self::new(255, 0, 255)),
            "white" => Some(Self::new(255, 255, 255)),
            "orange" => Some(Self::new(255, 165, 0)),
            "purple" => Some(Self::new(128, 0, 255)),
            _ => None,
        }
    }
}

// ── TOML definition types ────────────────────────────────────────────

/// Effect definition as loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectDef {
    #[serde(skip)]
    pub name: String,
    /// Default color or `$variable` name.
    pub color: Option<String>,
    #[serde(default)]
    pub keyframes: Vec<KeyframeDef>,
    /// Special mode (e.g. "rainbow").
    pub mode: Option<String>,
    /// Rainbow speed multiplier.
    pub speed: Option<f32>,
    /// Auto-expire in ms (-1 or absent = no expiry).
    pub ttl_ms: Option<i32>,
    #[serde(default)]
    pub priority: i32,
    pub description: Option<String>,
}

/// A single keyframe in the effect definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyframeDef {
    /// Time in ms from animation start.
    pub t: f64,
    /// Brightness value 0.0-1.0.
    pub v: f64,
    /// Per-keyframe color override (literal or `$variable`).
    pub color: Option<String>,
    /// Easing function to the *next* keyframe.
    #[serde(default = "default_easing")]
    pub easing: String,
}

fn default_easing() -> String {
    "Linear".to_string()
}

/// The effect library — a named collection of effects loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EffectLibrary {
    #[serde(flatten)]
    pub effects: BTreeMap<String, EffectDef>,
}

impl EffectLibrary {
    /// Load from a TOML file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        Self::from_toml(&content)
    }

    /// Parse from TOML string.
    pub fn from_toml(content: &str) -> Result<Self, String> {
        let mut lib: EffectLibrary =
            toml::from_str(content).map_err(|e| format!("parse TOML: {e}"))?;
        // Fill in the name field from the map key
        for (name, def) in &mut lib.effects {
            def.name = name.clone();
        }
        Ok(lib)
    }

    /// Load the default effects library from the config directory.
    /// Creates the default file if it doesn't exist.
    pub fn load_default() -> Result<Self, String> {
        let path = default_effects_path();
        if !path.exists() {
            // Create parent dirs and write defaults
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
            }
            std::fs::write(&path, DEFAULT_EFFECTS_TOML)
                .map_err(|e| format!("write default effects: {e}"))?;
            eprintln!("Created default effects: {}", path.display());
        }
        Self::load(&path)
    }

    /// Get an effect by name.
    pub fn get(&self, name: &str) -> Option<&EffectDef> {
        self.effects.get(name)
    }

    /// List all effect names.
    pub fn names(&self) -> Vec<&str> {
        self.effects.keys().map(|s| s.as_str()).collect()
    }
}

/// Path to the default effects TOML file.
pub fn default_effects_path() -> PathBuf {
    dirs_path().join("effects.toml")
}

fn dirs_path() -> PathBuf {
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(config).join("monsgeek")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config/monsgeek")
    } else {
        PathBuf::from("/tmp/monsgeek")
    }
}

// ── Resolved (runtime) types ─────────────────────────────────────────

/// A keyframe with all variables resolved to concrete colors.
#[derive(Debug, Clone)]
pub struct ResolvedKeyframe {
    pub t_ms: f64,
    pub color: Rgb,
    pub brightness: f64,
}

/// Ready-to-evaluate effect with pre-built animation sequences.
#[derive(Debug, Clone)]
pub struct ResolvedEffect {
    pub keyframes: Vec<ResolvedKeyframe>,
    pub duration_ms: f64,
    pub is_rainbow: bool,
    pub rainbow_speed: f32,
    /// Easing function names parallel to keyframes (for brightness interpolation).
    easing_names: Vec<String>,
}

impl ResolvedEffect {
    /// Evaluate the effect at a given elapsed time in ms. Returns the RGB output.
    pub fn evaluate(&self, elapsed_ms: f64) -> Rgb {
        if self.is_rainbow {
            let t = if self.duration_ms > 0.0 {
                (elapsed_ms % self.duration_ms) / self.duration_ms
            } else {
                0.0
            };
            let brightness = self.interpolate_brightness(elapsed_ms);
            let hue = (t * 360.0 * self.rainbow_speed as f64) as f32;
            return Rgb::from_hsv(hue, 1.0, brightness as f32);
        }

        if self.keyframes.is_empty() {
            // Solid: first keyframe color at full brightness (or black)
            return Rgb::BLACK;
        }
        if self.keyframes.len() == 1 {
            return self.keyframes[0]
                .color
                .scale(self.keyframes[0].brightness as f32);
        }

        let brightness = self.interpolate_brightness(elapsed_ms);
        let color = self.interpolate_color(elapsed_ms);
        color.scale(brightness as f32)
    }

    /// Interpolate brightness at time t using easing functions.
    fn interpolate_brightness(&self, elapsed_ms: f64) -> f64 {
        if self.keyframes.is_empty() {
            return 1.0;
        }
        if self.duration_ms <= 0.0 {
            return self.keyframes[0].brightness;
        }

        let t = elapsed_ms % self.duration_ms;

        // Find the surrounding keyframes
        let (i, j) = self.find_segment(t);
        let kf_a = &self.keyframes[i];
        let kf_b = &self.keyframes[j];

        if i == j {
            return kf_a.brightness;
        }

        let seg_duration = kf_b.t_ms - kf_a.t_ms;
        if seg_duration <= 0.0 {
            return kf_a.brightness;
        }

        let local_t = ((t - kf_a.t_ms) / seg_duration).clamp(0.0, 1.0);
        let eased_t = apply_easing(&self.easing_names[i], local_t);
        kf_a.brightness + (kf_b.brightness - kf_a.brightness) * eased_t
    }

    /// Interpolate color at time t (linear RGB lerp between keyframe colors).
    fn interpolate_color(&self, elapsed_ms: f64) -> Rgb {
        if self.keyframes.is_empty() {
            return Rgb::BLACK;
        }
        if self.duration_ms <= 0.0 {
            return self.keyframes[0].color;
        }

        let t = elapsed_ms % self.duration_ms;
        let (i, j) = self.find_segment(t);

        if i == j {
            return self.keyframes[i].color;
        }

        let kf_a = &self.keyframes[i];
        let kf_b = &self.keyframes[j];
        let seg_duration = kf_b.t_ms - kf_a.t_ms;
        if seg_duration <= 0.0 {
            return kf_a.color;
        }

        let local_t = ((t - kf_a.t_ms) / seg_duration).clamp(0.0, 1.0) as f32;
        Rgb::lerp(kf_a.color, kf_b.color, local_t)
    }

    /// Find the keyframe segment indices (i, j) that surround time t.
    fn find_segment(&self, t: f64) -> (usize, usize) {
        let n = self.keyframes.len();
        if n <= 1 {
            return (0, 0);
        }
        for i in 0..n - 1 {
            if t < self.keyframes[i + 1].t_ms {
                return (i, i + 1);
            }
        }
        // At or past the last keyframe
        (n - 1, n - 1)
    }
}

/// Apply an easing function by name.
fn apply_easing(name: &str, t: f64) -> f64 {
    // Use the `keyframe` crate's easing functions.
    // They work on f64 values 0.0->1.0 and return the eased value.
    let t = t.clamp(0.0, 1.0);

    match name {
        "Linear" => t,
        "Hold" | "Step" => 0.0, // hold previous value until next keyframe
        "EaseIn" | "EaseInQuad" => ease::EaseIn.y(t),
        "EaseOut" | "EaseOutQuad" => ease::EaseOut.y(t),
        "EaseInOut" => ease::EaseInOut.y(t),
        "EaseInCubic" => ease::EaseInCubic.y(t),
        "EaseOutCubic" => ease::EaseOutCubic.y(t),
        "EaseInOutCubic" => ease::EaseInOutCubic.y(t),
        "EaseInQuart" => ease::EaseInQuart.y(t),
        "EaseOutQuart" => ease::EaseOutQuart.y(t),
        "EaseInOutQuart" => ease::EaseInOutQuart.y(t),
        "EaseInQuint" => ease::EaseInQuint.y(t),
        "EaseOutQuint" => ease::EaseOutQuint.y(t),
        "EaseInOutQuint" => ease::EaseInOutQuint.y(t),
        // Expo easings not in keyframe crate — approximate with Quint
        "EaseInExpo" => ease::EaseInQuint.y(t),
        "EaseOutExpo" => ease::EaseOutQuint.y(t),
        "EaseInOutExpo" => ease::EaseInOutQuint.y(t),
        _ => t, // fallback to linear
    }
}

// We need the EasingFunction trait to call .y()
use keyframe::EasingFunction;

// ── Resolution (variable substitution) ───────────────────────────────

/// Resolve an effect definition into a ready-to-evaluate effect.
///
/// `vars` maps variable names (without `$`) to color strings.
pub fn resolve(def: &EffectDef, vars: &BTreeMap<String, String>) -> Result<ResolvedEffect, String> {
    let is_rainbow = def.mode.as_deref() == Some("rainbow");
    let rainbow_speed = def.speed.unwrap_or(1.0);

    if def.keyframes.is_empty() {
        // Solid effect — single keyframe at full brightness
        let color = resolve_color(def.color.as_deref(), None, vars)?;
        return Ok(ResolvedEffect {
            keyframes: vec![ResolvedKeyframe {
                t_ms: 0.0,
                color,
                brightness: 1.0,
            }],
            duration_ms: 0.0,
            is_rainbow,
            rainbow_speed,
            easing_names: vec!["Linear".to_string()],
        });
    }

    let mut keyframes = Vec::with_capacity(def.keyframes.len());
    let mut easing_names = Vec::with_capacity(def.keyframes.len());

    for kf in &def.keyframes {
        let color = resolve_color(kf.color.as_deref(), def.color.as_deref(), vars)?;
        keyframes.push(ResolvedKeyframe {
            t_ms: kf.t,
            color,
            brightness: kf.v.clamp(0.0, 1.0),
        });
        easing_names.push(kf.easing.clone());
    }

    let duration_ms = keyframes.last().map(|kf| kf.t_ms).unwrap_or(0.0);

    Ok(ResolvedEffect {
        keyframes,
        duration_ms,
        is_rainbow,
        rainbow_speed,
        easing_names,
    })
}

/// Resolve a color string, substituting variables.
///
/// Priority: per-keyframe color > effect-level color > black.
fn resolve_color(
    kf_color: Option<&str>,
    effect_color: Option<&str>,
    vars: &BTreeMap<String, String>,
) -> Result<Rgb, String> {
    let color_str = kf_color.or(effect_color);

    let Some(s) = color_str else {
        return Ok(Rgb::BLACK);
    };

    if let Some(var_name) = s.strip_prefix('$') {
        let value = vars
            .get(var_name)
            .ok_or_else(|| format!("unresolved variable: ${var_name}"))?;
        Rgb::parse(value).ok_or_else(|| format!("invalid color for ${var_name}: {value}"))
    } else {
        Rgb::parse(s).ok_or_else(|| format!("invalid color: {s}"))
    }
}

/// List required variables for an effect (variables referenced but not provided).
pub fn required_variables(def: &EffectDef) -> Vec<String> {
    let mut vars = Vec::new();
    if let Some(ref c) = def.color {
        if let Some(name) = c.strip_prefix('$') {
            vars.push(name.to_string());
        }
    }
    for kf in &def.keyframes {
        if let Some(ref c) = kf.color {
            if let Some(name) = c.strip_prefix('$') {
                if !vars.contains(&name.to_string()) {
                    vars.push(name.to_string());
                }
            }
        }
    }
    vars
}

// ── Default effects ──────────────────────────────────────────────────

pub const DEFAULT_EFFECTS_TOML: &str = r##"# MonsGeek LED Effects Library
# Each section defines a named effect with keyframes.
# Colors can be literals ("red", "#FF0000") or variables ("$color").
# Variables are resolved at trigger time with --var name=value.

[breathe]
color = "$color"
description = "Smooth fade in/out"
keyframes = [
    { t = 0,    v = 0.0, easing = "EaseInOut" },
    { t = 1000, v = 1.0, easing = "EaseInOut" },
    { t = 2000, v = 0.0 },
]

[flash]
color = "$color"
description = "On/off blink"
keyframes = [
    { t = 0,   v = 1.0, easing = "Hold" },
    { t = 500, v = 0.0, easing = "Hold" },
    { t = 1000, v = 1.0 },
]

[pulse]
color = "$color"
description = "Quick flash then slow fade"
keyframes = [
    { t = 0,   v = 0.0, easing = "EaseOutQuad" },
    { t = 80,  v = 1.0, easing = "EaseInQuint" },
    { t = 800, v = 0.0 },
]

[solid]
color = "$color"
description = "Constant color"
priority = -10

[police]
description = "Red/blue alternating flash"
keyframes = [
    { t = 0,   color = "red",  v = 1.0, easing = "Hold" },
    { t = 200, color = "blue", v = 1.0, easing = "Hold" },
    { t = 400, color = "red",  v = 1.0 },
]

[rainbow]
mode = "rainbow"
speed = 1.0
description = "Hue rotation"
keyframes = [
    { t = 0,    v = 1.0 },
    { t = 3000, v = 1.0 },
]

[build-status]
color = "$status"
description = "Build result indicator"
keyframes = [
    { t = 0,    v = 0.0, easing = "EaseOutQuad" },
    { t = 100,  v = 1.0, easing = "Hold" },
    { t = 2000, v = 1.0, easing = "EaseInQuint" },
    { t = 3000, v = 0.0 },
]
ttl_ms = 3000
"##;

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_default_toml() {
        let lib = EffectLibrary::from_toml(DEFAULT_EFFECTS_TOML).unwrap();
        assert!(lib.effects.contains_key("breathe"));
        assert!(lib.effects.contains_key("police"));
        assert!(lib.effects.contains_key("rainbow"));
        assert!(lib.effects.contains_key("solid"));
        assert_eq!(lib.effects["breathe"].keyframes.len(), 3);
    }

    #[test]
    fn test_resolve_breathe() {
        let lib = EffectLibrary::from_toml(DEFAULT_EFFECTS_TOML).unwrap();
        let def = lib.get("breathe").unwrap();
        let mut vars = BTreeMap::new();
        vars.insert("color".to_string(), "red".to_string());
        let resolved = resolve(def, &vars).unwrap();
        assert_eq!(resolved.duration_ms, 2000.0);
        assert_eq!(resolved.keyframes[0].color, Rgb::new(255, 0, 0));
    }

    #[test]
    fn test_resolve_police_no_vars() {
        let lib = EffectLibrary::from_toml(DEFAULT_EFFECTS_TOML).unwrap();
        let def = lib.get("police").unwrap();
        let vars = BTreeMap::new();
        let resolved = resolve(def, &vars).unwrap();
        assert_eq!(resolved.keyframes[0].color, Rgb::new(255, 0, 0)); // red
        assert_eq!(resolved.keyframes[1].color, Rgb::new(0, 0, 255)); // blue
    }

    #[test]
    fn test_resolve_missing_variable() {
        let lib = EffectLibrary::from_toml(DEFAULT_EFFECTS_TOML).unwrap();
        let def = lib.get("breathe").unwrap();
        let vars = BTreeMap::new();
        assert!(resolve(def, &vars).is_err());
    }

    #[test]
    fn test_evaluate_solid() {
        let mut vars = BTreeMap::new();
        vars.insert("color".to_string(), "green".to_string());
        let lib = EffectLibrary::from_toml(DEFAULT_EFFECTS_TOML).unwrap();
        let resolved = resolve(lib.get("solid").unwrap(), &vars).unwrap();
        let c = resolved.evaluate(500.0);
        assert_eq!(c, Rgb::new(0, 255, 0));
    }

    #[test]
    fn test_evaluate_breathe_midpoint() {
        let mut vars = BTreeMap::new();
        vars.insert("color".to_string(), "white".to_string());
        let lib = EffectLibrary::from_toml(DEFAULT_EFFECTS_TOML).unwrap();
        let resolved = resolve(lib.get("breathe").unwrap(), &vars).unwrap();
        // At t=1000 (peak), brightness should be 1.0
        let peak = resolved.evaluate(1000.0);
        assert_eq!(peak, Rgb::new(255, 255, 255));
        // At t=0, brightness should be 0.0
        let start = resolved.evaluate(0.0);
        assert_eq!(start, Rgb::BLACK);
    }

    #[test]
    fn test_evaluate_rainbow() {
        let lib = EffectLibrary::from_toml(DEFAULT_EFFECTS_TOML).unwrap();
        let vars = BTreeMap::new();
        let resolved = resolve(lib.get("rainbow").unwrap(), &vars).unwrap();
        assert!(resolved.is_rainbow);
        let c = resolved.evaluate(0.0);
        // At hue=0 with full brightness, should be red-ish
        assert!(c.r > 200);
    }

    #[test]
    fn test_required_variables() {
        let lib = EffectLibrary::from_toml(DEFAULT_EFFECTS_TOML).unwrap();
        assert_eq!(
            required_variables(lib.get("breathe").unwrap()),
            vec!["color"]
        );
        assert!(required_variables(lib.get("police").unwrap()).is_empty());
        assert_eq!(
            required_variables(lib.get("build-status").unwrap()),
            vec!["status"]
        );
    }

    #[test]
    fn test_rgb_parse() {
        assert_eq!(Rgb::parse("#FF0000"), Some(Rgb::new(255, 0, 0)));
        assert_eq!(Rgb::parse("red"), Some(Rgb::new(255, 0, 0)));
        assert_eq!(Rgb::parse("unknown"), None);
    }

    #[test]
    fn test_rgb_lerp() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(100, 200, 50);
        let mid = Rgb::lerp(a, b, 0.5);
        assert_eq!(mid, Rgb::new(50, 100, 25));
    }

    #[test]
    fn test_hold_easing() {
        // Hold easing should keep the previous value until the next keyframe
        assert_eq!(apply_easing("Hold", 0.5), 0.0);
        assert_eq!(apply_easing("Hold", 0.99), 0.0);
    }

    #[test]
    fn test_police_color_alternation() {
        let lib = EffectLibrary::from_toml(DEFAULT_EFFECTS_TOML).unwrap();
        let vars = BTreeMap::new();
        let resolved = resolve(lib.get("police").unwrap(), &vars).unwrap();
        // At t=0 should be red
        let c0 = resolved.evaluate(0.0);
        assert_eq!(c0, Rgb::new(255, 0, 0));
        // At t=100 should be between red and blue (linear color interp)
        let c100 = resolved.evaluate(100.0);
        assert!(c100.r > 0 || c100.b > 0); // some color present
    }
}
