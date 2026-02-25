// CLI definitions using clap

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "iot_driver")]
#[command(author, version, about = "MonsGeek M1 V5 HE Linux Driver")]
#[command(propagate_version = true)]
pub struct Cli {
    /// Enable transport monitoring (prints all commands/responses)
    #[arg(long, global = true)]
    pub monitor: bool,

    /// Use pcap file instead of real device (passive replay)
    #[arg(long = "file", global = true, value_name = "FILE")]
    pub pcap_file: Option<PathBuf>,

    /// Include standard HID reports (keyboard, consumer, NKRO)
    #[arg(long, global = true)]
    pub all: bool,

    /// Show raw hex dump alongside decoded output
    #[arg(long, global = true)]
    pub hex: bool,

    /// Filter output (all, events, commands, cmd=0xNN)
    #[arg(long, global = true)]
    pub filter: Option<String>,

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

    /// Get polling rate (Hz)
    #[command(visible_aliases = ["poll", "hz"])]
    Rate,

    /// Get keyboard options (Fn layer, WASD swap, etc.)
    #[command(visible_aliases = ["opts", "opt", "o"])]
    Options,

    /// Get supported features and precision
    #[command(visible_aliases = ["feat", "f"])]
    Features,

    /// Get sleep time settings (idle + deep sleep for BT and 2.4GHz)
    #[command(visible_alias = "s")]
    Sleep,

    /// Show all device information
    #[command(visible_alias = "a")]
    All,

    /// Get battery status (for 2.4GHz wireless dongles)
    #[command(visible_aliases = ["bat", "b"])]
    Battery {
        /// Print only battery percentage (for scripts)
        #[arg(short, long)]
        quiet: bool,
        /// Show full vendor response in hex
        #[arg(long)]
        hex: bool,
        /// Continuously monitor (interval in seconds, default: 1)
        #[arg(short, long)]
        watch: Option<Option<u64>>,
        /// Use vendor HID interface directly (skip kernel power_supply)
        #[arg(long)]
        vendor: bool,
    },

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

    /// Set polling rate (125, 250, 500, 1000, 2000, 4000, 8000 Hz)
    #[command(visible_aliases = ["sr", "setpoll"])]
    SetRate {
        /// Polling rate (e.g., 1000, 1000hz, 1khz, 1k)
        rate: String,
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

    /// Set sleep time settings
    ///
    /// Sets idle and deep sleep timeouts for Bluetooth and 2.4GHz modes.
    /// Values can be specified as seconds (120), minutes (2m), or hours (1h).
    /// Use "0" or "off" to disable a timeout.
    #[command(visible_alias = "ss")]
    SetSleep {
        /// Idle timeout for both BT and 2.4GHz (e.g., "2m", "120", "off")
        #[arg(long)]
        idle: Option<String>,

        /// Deep sleep timeout for both BT and 2.4GHz (e.g., "28m", "1680", "off")
        #[arg(long)]
        deep: Option<String>,

        /// Bluetooth idle timeout (overrides --idle for BT)
        #[arg(long)]
        idle_bt: Option<String>,

        /// 2.4GHz idle timeout (overrides --idle for 2.4GHz)
        #[arg(long)]
        idle_24g: Option<String>,

        /// Bluetooth deep sleep timeout (overrides --deep for BT)
        #[arg(long)]
        deep_bt: Option<String>,

        /// 2.4GHz deep sleep timeout (overrides --deep for 2.4GHz)
        #[arg(long)]
        deep_24g: Option<String>,

        /// Set all timeouts uniformly: idle,deep (e.g., "2m,28m")
        #[arg(short, long)]
        uniform: Option<String>,
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

    /// Set release point for all keys
    #[command(visible_alias = "srl")]
    SetRelease {
        /// Release point in mm (e.g., 0.5, 1.0, 2.0)
        mm: f32,
    },

    /// Set bottom deadzone for all keys
    #[command(visible_alias = "sbd")]
    SetBottomDeadzone {
        /// Bottom deadzone in mm (e.g., 0.2, 0.3)
        mm: f32,
    },

    /// Set top deadzone for all keys
    #[command(visible_alias = "std")]
    SetTopDeadzone {
        /// Top deadzone in mm (e.g., 0.1, 0.2)
        mm: f32,
    },

    /// Set trigger settings for a specific key
    #[command(visible_alias = "skt")]
    SetKeyTrigger {
        /// Key index (0-125)
        key: u8,
        /// Actuation point in mm (optional)
        #[arg(long)]
        actuation: Option<f32>,
        /// Release point in mm (optional)
        #[arg(long)]
        release: Option<f32>,
        /// Key mode: normal, rt, dks, snaptap (optional)
        #[arg(long)]
        mode: Option<String>,
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
    /// Remap a key (supports layer prefix: Fn+Caps, L1+A)
    #[command(visible_alias = "set-key")]
    Remap {
        /// Source key: name, index, or with layer prefix (Fn+Caps, L1+A, 42)
        from: String,
        /// Target HID keycode or key name
        to: String,
        /// Layer (0=base, 1=layer1, 2=fn) — overridden by prefix in FROM
        #[arg(short, long, default_value = "0")]
        layer: u8,
    },

    /// Reset a key to default (supports layer prefix: Fn+Caps, L1+A)
    #[command(visible_alias = "rk")]
    ResetKey {
        /// Key position: name, index, or with layer prefix (Fn+Caps, L1+A)
        key: String,
        /// Layer (0=base, 1=layer1, 2=fn) — overridden by prefix in KEY
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

    /// List key remappings (non-default bindings)
    #[command(visible_alias = "remaps")]
    RemapList {
        /// Layer: 0=base, 1=layer1, 2=fn, omit=all
        #[arg(short, long)]
        layer: Option<u8>,
        /// Show all keys including factory defaults and disabled positions
        #[arg(short, long)]
        all: bool,
    },

    /// Show the Fn layer key bindings (media keys, LED controls, etc.)
    #[command(visible_alias = "fnl")]
    FnLayout {
        /// OS mode: "win" or "mac"
        #[arg(long, default_value = "win")]
        sys: String,
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
        /// Text to type, or sequence string when --seq is used
        text: String,
        /// Default delay between events in ms
        #[arg(short, long, default_value = "10")]
        delay: u16,
        /// How many times to repeat the macro
        #[arg(short, long, default_value = "1")]
        repeat: u16,
        /// Parse text as a comma-separated key sequence (e.g. "Ctrl+A,Ctrl+C")
        #[arg(short, long)]
        seq: bool,
    },

    /// Clear macro from a key
    ClearMacro {
        /// Key position or name
        key: String,
    },

    /// Assign a macro to a key (base layer by default, --fn for Fn layer)
    AssignMacro {
        /// Key name (e.g. F3, Esc) or matrix index
        key: String,
        /// Macro slot number (0-7)
        macro_index: String,
        /// Assign to Fn+key instead of the base layer
        #[arg(long, alias = "fn-layer")]
        r#fn: bool,
    },

    // === Animation Commands ===
    /// Upload or download a userpic image (mode 13, persistent per-key colors)
    Userpic {
        /// Image file to upload (PNG, JPG, etc.) — omit to download
        file: Option<String>,
        /// Userpic slot (0-4)
        #[arg(short, long, default_value = "0", value_parser = clap::value_parser!(u8).range(0..5))]
        slot: u8,
        /// Output file for download (default: userpic_<slot>.png)
        #[arg(short, long)]
        output: Option<String>,
        /// Use nearest-neighbor scaling (sharp pixels, good for pixel art)
        #[arg(long)]
        nearest: bool,
    },

    /// Test LED streaming (one LED at a time, cycling colors)
    StreamTest {
        /// Frames per second
        #[arg(long, default_value = "10")]
        fps: f32,
        /// LED power budget in milliamps (0 = unlimited)
        #[arg(long, default_value = "400")]
        power_budget: u32,
    },

    /// Stream a GIF to keyboard LEDs via patch protocol (0xFC)
    Stream {
        /// GIF file path
        file: String,
        /// Override FPS (default: use GIF frame delays)
        #[arg(long)]
        fps: Option<f32>,
        /// Loop animation continuously
        #[arg(long)]
        r#loop: bool,
        /// LED power budget in milliamps (0 = unlimited)
        #[arg(long, default_value = "400")]
        power_budget: u32,
    },

    /// Set LED mode by name or number
    Mode {
        /// Mode name (breathing, wave, rainbow, etc.) or number (0-24)
        mode: String,
        /// Userpic slot for mode 13 (UserPicture)
        #[arg(short, long, default_value = "0")]
        layer: u8,
    },

    /// List all available LED modes
    Modes,

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
    #[cfg(feature = "screen-capture")]
    #[command(visible_alias = "screencolor")]
    Screen {
        /// Capture framerate (1-60, default 2)
        #[arg(short, long, default_value = "2")]
        fps: u32,
    },

    // === Debug Commands ===
    /// Test new transport abstraction layer
    #[command(visible_alias = "tt")]
    TestTransport,

    /// Monitor real-time key depth (magnetism) from keyboard
    #[command(visible_alias = "keydepth")]
    Depth {
        /// Show raw hex bytes for each report
        #[arg(short, long)]
        raw: bool,
        /// Show zero-depth reports (keys at rest)
        #[arg(short, long)]
        zero: bool,
        /// Verbose status updates
        #[arg(short, long)]
        verbose: bool,
    },

    // === Firmware Commands ===
    /// Firmware update tools
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

    /// Run joystick mapper (maps magnetic keys to virtual joystick axes)
    #[command(visible_alias = "joy")]
    Joystick {
        /// Config file path (default: ~/.config/monsgeek/joystick.toml)
        #[arg(short, long)]
        config: Option<std::path::PathBuf>,
        /// Run without TUI (headless mode)
        #[arg(long)]
        headless: bool,
    },

    // === Effect Commands ===
    /// LED effect commands (list, show, preview, play)
    #[command(subcommand, visible_alias = "fx")]
    Effect(EffectCommands),

    // === Notification Commands ===
    /// Start the LED notification daemon (D-Bus server + render loop)
    #[cfg(feature = "notify")]
    #[command(visible_alias = "nd")]
    NotifyDaemon {
        /// Render frames per second (1-60)
        #[arg(long, default_value = "30")]
        fps: u32,
        /// LED power budget in milliamps (0 = unlimited)
        #[arg(long, default_value = "400")]
        power_budget: u32,
    },

    /// Post a notification to the daemon (requires running notify-daemon)
    #[cfg(feature = "notify")]
    #[command(visible_alias = "n")]
    Notify {
        /// Target key: name (F1, Esc), position (0,5), or index (#42)
        key: String,
        /// Effect name (breathe, pulse, police, etc.)
        effect: String,
        /// Color/variable bindings: name=value (e.g. color=red, status=green)
        #[arg(long = "var", short = 'V')]
        vars: Vec<String>,
        /// Priority (higher wins conflicts, default 0)
        #[arg(long, default_value = "0")]
        priority: i32,
        /// Time-to-live in milliseconds (-1 = use effect default)
        #[arg(long, default_value = "-1")]
        ttl: i32,
        /// Source identifier
        #[arg(long, default_value = "custom")]
        source: String,
    },

    /// Acknowledge/dismiss notification(s)
    #[cfg(feature = "notify")]
    NotifyAck {
        /// Dismiss by notification ID
        #[arg(long)]
        id: Option<u64>,
        /// Dismiss all on key
        #[arg(long)]
        key: Option<String>,
        /// Dismiss all from source
        #[arg(long)]
        source: Option<String>,
        /// Dismiss all notifications
        #[arg(long)]
        all: bool,
    },

    /// List active notifications
    #[cfg(feature = "notify")]
    NotifyList,

    /// Clear all notifications
    #[cfg(feature = "notify")]
    NotifyClear,
}

/// Effect commands
#[derive(Subcommand)]
pub enum EffectCommands {
    /// List all available effects
    #[command(visible_alias = "ls")]
    List,

    /// Show details of an effect
    Show {
        /// Effect name
        name: String,
    },

    /// Preview an effect in the terminal
    Preview {
        /// Effect name
        name: String,
        /// Target keys (e.g. F1 F2 Esc) — defaults to F1-F4
        keys: Vec<String>,
        /// Variable bindings: name=value
        #[arg(long = "var", short = 'V')]
        vars: Vec<String>,
        /// Preview FPS (1-60)
        #[arg(long, default_value = "15")]
        fps: u32,
    },

    /// Play an effect on keyboard hardware
    Play {
        /// Effect name
        name: String,
        /// Target keys (at least one required)
        keys: Vec<String>,
        /// Variable bindings: name=value
        #[arg(long = "var", short = 'V')]
        vars: Vec<String>,
    },
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

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum, Default)]
pub enum PcapOutputFormat {
    /// Human-readable text output
    #[default]
    Text,
    /// JSON output (one object per line)
    Json,
}

/// Firmware commands
#[derive(Subcommand)]
pub enum FirmwareCommands {
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

    /// Flash firmware to device (DANGEROUS - overwrites firmware!)
    #[command(visible_alias = "fl")]
    Flash {
        /// Path to firmware file (.bin or .zip)
        file: PathBuf,

        /// HID device path (required when multiple devices found)
        #[arg(long)]
        device: Option<String>,

        /// Flash dongle firmware instead of keyboard firmware
        #[arg(long)]
        dongle: bool,

        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },
}
