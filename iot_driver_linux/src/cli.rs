// CLI definitions using clap

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "iot_driver")]
#[command(author, version, about = "MonsGeek M1 V5 HE Linux Driver")]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    // === Query Commands ===
    /// Get device ID and firmware version
    #[command(visible_aliases = ["version", "ver", "v"])]
    Info,

    /// Get current profile (0-3)
    #[command(visible_aliases = ["prof", "p"])]
    Profile,

    /// Get LED settings (mode, brightness, speed, color)
    #[command(visible_aliases = ["light", "l"])]
    Led,

    /// Get debounce time (ms)
    #[command(visible_aliases = ["deb", "d"])]
    Debounce,

    /// Get keyboard options (Fn layer, WASD swap, etc.)
    #[command(visible_aliases = ["opts", "opt", "o"])]
    Options,

    /// Get supported features and precision
    #[command(visible_aliases = ["feat", "f"])]
    Features,

    /// Get sleep timeout
    #[command(visible_alias = "s")]
    Sleep,

    /// Show all device information
    #[command(visible_alias = "a")]
    All,

    // === Set Commands ===
    /// Set active profile
    #[command(visible_alias = "sp")]
    SetProfile {
        /// Profile number (0-3)
        #[arg(value_parser = clap::value_parser!(u8).range(0..4))]
        profile: u8,
    },

    /// Set debounce time
    #[command(visible_alias = "sd")]
    SetDebounce {
        /// Debounce time in milliseconds (0-50)
        #[arg(value_parser = clap::value_parser!(u8).range(0..51))]
        ms: u8,
    },

    /// Set LED mode and parameters
    #[command(visible_alias = "sl")]
    SetLed {
        /// LED mode (0-25 or name like 'breathing', 'wave', 'rainbow')
        mode: String,
        /// Brightness (0-4)
        #[arg(default_value = "4")]
        brightness: u8,
        /// Speed (0-4)
        #[arg(default_value = "2")]
        speed: u8,
        /// Red component (0-255)
        #[arg(default_value = "255")]
        r: u8,
        /// Green component (0-255)
        #[arg(default_value = "255")]
        g: u8,
        /// Blue component (0-255)
        #[arg(default_value = "255")]
        b: u8,
    },

    /// Set sleep timeout
    #[command(visible_alias = "ss")]
    SetSleep {
        /// Sleep timeout in seconds
        seconds: u16,
    },

    /// Factory reset keyboard
    Reset,

    /// Run calibration (min + max)
    #[command(visible_alias = "cal")]
    Calibrate,

    // === Trigger Commands ===
    /// Show current trigger settings
    #[command(visible_alias = "gt")]
    Triggers,

    /// Set actuation point for all keys
    #[command(visible_alias = "sa")]
    SetActuation {
        /// Actuation point in mm (e.g., 0.5, 1.0, 2.0)
        mm: f32,
    },

    /// Enable/disable Rapid Trigger or set sensitivity
    #[command(visible_aliases = ["rapid-trigger", "rt"])]
    SetRt {
        /// "on", "off", or sensitivity in mm (e.g., 0.1, 0.2)
        value: String,
    },

    // === Per-key Color Commands ===
    /// Set all keys to a single color
    #[command(visible_aliases = ["color-all", "sc"])]
    SetColorAll {
        /// Red (0-255)
        r: u8,
        /// Green (0-255)
        g: u8,
        /// Blue (0-255)
        b: u8,
        /// Layer (0-3)
        #[arg(short, long, default_value = "0")]
        layer: u8,
    },

    // === Key Remapping ===
    /// Remap a key
    #[command(visible_alias = "set-key")]
    Remap {
        /// Source key (matrix position 0-125 or key name)
        from: String,
        /// Target HID keycode or key name
        to: String,
        /// Layer (0-3)
        #[arg(short, long, default_value = "0")]
        layer: u8,
    },

    /// Reset a key to default
    #[command(visible_alias = "rk")]
    ResetKey {
        /// Key position (0-125 or key name)
        key: String,
        /// Layer (0-3)
        #[arg(short, long, default_value = "0")]
        layer: u8,
    },

    /// Swap two keys
    Swap {
        /// First key
        key1: String,
        /// Second key
        key2: String,
        /// Layer (0-3)
        #[arg(short, long, default_value = "0")]
        layer: u8,
    },

    /// Show key matrix mappings
    #[command(visible_alias = "km")]
    Keymatrix {
        /// Layer (0-3)
        #[arg(default_value = "0")]
        layer: u8,
    },

    // === Macro Commands ===
    /// Get macro for a key
    #[command(visible_alias = "get-macro")]
    Macro {
        /// Key position or name
        key: String,
    },

    /// Set a text macro for a key
    #[command(visible_alias = "set-text-macro")]
    SetMacro {
        /// Key position or name
        key: String,
        /// Text to type when key is pressed
        text: String,
    },

    /// Clear macro from a key
    ClearMacro {
        /// Key position or name
        key: String,
    },

    // === Animation Commands ===
    /// Upload GIF animation to keyboard memory
    Gif {
        /// GIF file path, or --test for test animation
        #[arg(required_unless_present = "test")]
        file: Option<String>,
        /// Mapping mode
        #[arg(value_enum, default_value = "scale")]
        mode: MappingMode,
        /// Generate test rainbow animation
        #[arg(long)]
        test: bool,
        /// Number of frames for test animation
        #[arg(long, default_value = "20")]
        frames: usize,
        /// Frame delay in ms for test animation
        #[arg(long, default_value = "50")]
        delay: u16,
    },

    /// Stream GIF animation in real-time
    GifStream {
        /// GIF file path
        file: String,
        /// Mapping mode
        #[arg(value_enum, default_value = "scale")]
        mode: MappingMode,
        /// Loop animation continuously
        #[arg(long)]
        r#loop: bool,
    },

    /// Set LED mode by name or number
    Mode {
        /// Mode name (breathing, wave, rainbow, etc.) or number (0-25)
        mode: String,
        /// Layer to store per-key colors (for modes 13, 25)
        #[arg(short, long, default_value = "0")]
        layer: u8,
    },

    /// List all available LED modes
    Modes,

    // === Demo Commands ===
    /// Real-time rainbow sweep animation
    Rainbow,

    /// Checkerboard pattern demo
    #[command(visible_alias = "checker")]
    Checkerboard,

    /// Sweeping line animation demo
    Sweep,

    /// Set all keys to red (demo)
    Red,

    /// Real-time wave animation demo
    Wave,

    // === Audio Commands ===
    /// Run audio reactive LED mode
    Audio {
        /// Color mode: spectrum, solid, gradient
        #[arg(value_enum, short, long, default_value = "spectrum")]
        mode: AudioMode,
        /// Base hue for solid mode (0-360)
        #[arg(long, default_value = "0")]
        hue: f32,
        /// Sensitivity multiplier (0.5-2.0)
        #[arg(long, default_value = "1.0")]
        sensitivity: f32,
    },

    /// Test audio capture (list devices)
    AudioTest,

    /// Show real-time audio levels
    AudioLevels,

    // === Screen Color Commands ===
    /// Run screen color reactive LED mode (streams average screen color to keyboard)
    #[command(visible_alias = "screencolor")]
    Screen {
        /// Capture framerate (1-60, default 2)
        #[arg(short, long, default_value = "2")]
        fps: u32,
    },

    // === Firmware Commands (DRY-RUN ONLY) ===
    /// Firmware update tools (dry-run only, no actual flashing)
    #[command(subcommand, visible_alias = "fw")]
    Firmware(FirmwareCommands),

    // === Utility Commands ===
    /// List all HID devices
    #[command(visible_alias = "ls")]
    List,

    /// Send raw command byte (hex)
    #[command(visible_aliases = ["cmd", "hex"])]
    Raw {
        /// Command byte in hex (e.g., 8f, 87)
        cmd: String,
    },

    /// Run gRPC server on port 3814
    #[command(visible_alias = "server")]
    Serve,

    /// Run interactive terminal UI
    Tui,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum MappingMode {
    /// Scale image to fit keyboard grid
    Scale,
    /// Tile/wrap smaller images
    Tile,
    /// Center image on keyboard
    Center,
    /// 1:1 pixel mapping
    Direct,
}

impl From<MappingMode> for iot_driver::gif::MappingMode {
    fn from(m: MappingMode) -> Self {
        match m {
            MappingMode::Scale => iot_driver::gif::MappingMode::ScaleToFit,
            MappingMode::Tile => iot_driver::gif::MappingMode::Tile,
            MappingMode::Center => iot_driver::gif::MappingMode::Center,
            MappingMode::Direct => iot_driver::gif::MappingMode::Direct,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum, Default)]
pub enum AudioMode {
    /// Rainbow frequency spectrum visualization
    #[default]
    Spectrum,
    /// Single color pulse effect
    Solid,
    /// Gradient color effect
    Gradient,
}

impl AudioMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AudioMode::Spectrum => "spectrum",
            AudioMode::Solid => "solid",
            AudioMode::Gradient => "gradient",
        }
    }
}

/// Firmware commands (DRY-RUN ONLY - no actual flashing)
#[derive(Subcommand)]
pub enum FirmwareCommands {
    /// Show current device firmware version
    #[command(visible_alias = "i")]
    Info,

    /// Validate a firmware file (parse, checksum, structure)
    #[command(visible_alias = "val")]
    Validate {
        /// Path to firmware file (.bin or .zip)
        file: PathBuf,
    },

    /// Dry-run: simulate firmware update (NO ACTUAL FLASHING)
    #[command(visible_alias = "dr")]
    DryRun {
        /// Path to firmware file
        file: PathBuf,

        /// Show detailed command sequence
        #[arg(short, long)]
        verbose: bool,
    },

    /// Check for firmware updates from MonsGeek server
    #[command(visible_alias = "chk")]
    Check {
        /// Device ID (auto-detected if not specified)
        #[arg(long)]
        device_id: Option<u32>,
    },

    /// Download firmware from MonsGeek server
    #[command(visible_alias = "dl")]
    Download {
        /// Device ID (auto-detected if not specified)
        #[arg(long)]
        device_id: Option<u32>,

        /// Output file path
        #[arg(short, long, default_value = "firmware.zip")]
        output: PathBuf,
    },
}
