//! LED and RGB lighting types and utilities

/// Maximum brightness value (4 levels: 0-4)
pub const BRIGHTNESS_MAX: u8 = 4;

/// Maximum speed value (5 levels: 0-4)
pub const SPEED_MAX: u8 = 4;

/// Option flag for dazzle (rainbow) mode off
pub const DAZZLE_OFF: u8 = 0x00;

/// Option flag for dazzle (rainbow) mode on
pub const DAZZLE_ON: u8 = 0x04;

/// RGB color value
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    /// Create a new RGB color
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Create color from HSV values
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

    /// Black (all LEDs off)
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };
    /// White (all LEDs full)
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
    };
    /// Red
    pub const RED: Self = Self { r: 255, g: 0, b: 0 };
    /// Green
    pub const GREEN: Self = Self { r: 0, g: 255, b: 0 };
    /// Blue
    pub const BLUE: Self = Self { r: 0, g: 0, b: 255 };
}

/// LED effect mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LedMode {
    Off = 0,
    Constant = 1,
    Breathing = 2,
    Neon = 3,
    Wave = 4,
    Ripple = 5,
    Raindrop = 6,
    Snake = 7,
    Reactive = 8,
    Converge = 9,
    SineWave = 10,
    Kaleidoscope = 11,
    LineWave = 12,
    UserPicture = 13,
    Laser = 14,
    CircleWave = 15,
    Starry = 16,
    Aurora = 17,
    FlashAway = 18,
    Layered = 19,
    MusicPatterns = 20,
    ScreenSync = 21,
    MusicBars = 22,
}

impl LedMode {
    /// Get mode from numeric value
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Off),
            1 => Some(Self::Constant),
            2 => Some(Self::Breathing),
            3 => Some(Self::Neon),
            4 => Some(Self::Wave),
            5 => Some(Self::Ripple),
            6 => Some(Self::Raindrop),
            7 => Some(Self::Snake),
            8 => Some(Self::Reactive),
            9 => Some(Self::Converge),
            10 => Some(Self::SineWave),
            11 => Some(Self::Kaleidoscope),
            12 => Some(Self::LineWave),
            13 => Some(Self::UserPicture),
            14 => Some(Self::Laser),
            15 => Some(Self::CircleWave),
            16 => Some(Self::Starry),
            17 => Some(Self::Aurora),
            18 => Some(Self::FlashAway),
            19 => Some(Self::Layered),
            20 => Some(Self::MusicPatterns),
            21 => Some(Self::ScreenSync),
            22 => Some(Self::MusicBars),
            _ => None,
        }
    }

    /// Get the display name for this mode
    pub fn name(&self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Constant => "Constant",
            Self::Breathing => "Breathing",
            Self::Neon => "Neon",
            Self::Wave => "Wave",
            Self::Ripple => "Ripple",
            Self::Raindrop => "Raindrop",
            Self::Snake => "Snake",
            Self::Reactive => "Reactive",
            Self::Converge => "Converge",
            Self::SineWave => "Sine Wave",
            Self::Kaleidoscope => "Kaleidoscope",
            Self::LineWave => "Line Wave",
            Self::UserPicture => "User Picture",
            Self::Laser => "Laser",
            Self::CircleWave => "Circle Wave",
            Self::Starry => "Starry",
            Self::Aurora => "Aurora",
            Self::FlashAway => "Flash Away",
            Self::Layered => "Layered",
            Self::MusicPatterns => "Music Patterns",
            Self::ScreenSync => "Screen Sync",
            Self::MusicBars => "Music Bars",
        }
    }
}

/// LED parameters
#[derive(Debug, Clone)]
pub struct LedParams {
    /// Current mode
    pub mode: LedMode,
    /// Brightness (0-100)
    pub brightness: u8,
    /// Speed (mode-specific)
    pub speed: u8,
    /// Color (for modes that use it)
    pub color: RgbColor,
    /// Direction (for wave modes)
    pub direction: u8,
}

impl Default for LedParams {
    fn default() -> Self {
        Self {
            mode: LedMode::Constant,
            brightness: 100,
            speed: 50,
            color: RgbColor::WHITE,
            direction: 0,
        }
    }
}
