// CLI definitions using clap

use clap::{Parser, Subcommand, ValueEnum};
use monsgeek_transport::protocol::{Layer, Profile};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "iot_driver")]
#[command(author, version, about = "MonsGeek M1 V5 HE Linux Driver")]
#[command(propagate_version = true)]
pub struct Cli {
    /// Enable transport monitoring (prints all commands/responses)
    #[arg(long, global = true)]
    pub monitor: bool,

    /// Record decoded traffic to a JSONL file (implies --monitor)
    #[arg(long, global = true, value_name = "FILE")]
    pub record: Option<std::path::PathBuf>,

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

    /// Select device by index, transport (usb/dongle/bt), or HID path
    #[arg(short = 'D', long, global = true, value_name = "DEVICE")]
    pub device: Option<String>,

    /// Operate on a specific profile (0-3) instead of the board's active one.
    ///
    /// Keymap commands address the profile directly. Trigger and magnetism settings
    /// carry no profile byte, so those commands briefly switch the keyboard to the
    /// requested profile, apply, and switch back — announced when it happens.
    /// Use `set-profile` to change which profile the keyboard runs.
    #[arg(short = 'P', long, global = true, value_name = "N")]
    pub profile: Option<Profile>,

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
        profile: Profile,
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

    /// Set keyboard options (Fn layer, WASD swap, anti-mistouch, RT stability, OS mode)
    ///
    /// Only the flags you pass are changed; the rest are read back from the
    /// keyboard first, since the wire command carries the whole option set.
    #[command(visible_aliases = ["so", "setopts", "setopt"])]
    SetOptions {
        /// OS mode
        #[arg(long)]
        os: Option<OsModeArg>,

        /// Fn layer index (0 or 1)
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..2))]
        fn_layer: Option<u8>,

        /// Anti-mistouch (on/off)
        #[arg(long, value_parser = clap::builder::BoolishValueParser::new())]
        anti_mistouch: Option<bool>,

        /// Rapid Trigger stability window in ms (0-125, 25 ms steps)
        #[arg(long, value_parser = clap::value_parser!(u16).range(0..126))]
        rt_stability: Option<u16>,

        /// WASD/arrow swap (on/off) - the same toggle as Fn+W
        #[arg(long, value_parser = clap::builder::BoolishValueParser::new())]
        wasd_swap: Option<bool>,
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
        /// Base key mode (optional; RT flag is set separately via --rt)
        #[arg(long, value_enum)]
        mode: Option<KeyModeArg>,
        /// Enable/disable the Rapid-Trigger flag (optional; combines with --mode)
        #[arg(long)]
        rt: Option<bool>,
    },

    /// Set the base mode for all keys at once
    #[command(visible_alias = "sma")]
    SetModeAll {
        /// Base key mode for every key
        #[arg(value_enum)]
        mode: KeyModeArg,
        /// Also enable the Rapid-Trigger flag on every key
        #[arg(long)]
        rt: bool,
    },

    /// Bind, clear, or show a Snap-Tap (SOCD) key pair
    #[command(visible_alias = "st")]
    SetSnaptap {
        /// Key index
        key: u8,
        /// Partner key index to bind with (bidirectional)
        #[arg(long, conflicts_with = "clear")]
        with: Option<u8>,
        /// Clear this key's binding (and its partner's back-reference)
        #[arg(long, conflicts_with = "with")]
        clear: bool,
    },

    /// Set the Mod-Tap tap-vs-hold decision time for a key
    #[command(visible_alias = "mtt")]
    SetModtapTime {
        /// Key index
        key: u8,
        /// Decision time in milliseconds (10 ms steps, 0-2550)
        ms: u16,
    },

    /// Show or configure DKS (Dynamic Keystroke) for a key
    Dks {
        /// Key index
        key: u8,
        /// DKS activation travel in mm (e.g. 0.7)
        #[arg(long)]
        travel_mm: Option<f32>,
        /// Four packed trigger-mode bytes, comma-separated hex (e.g. "09,00,00,00")
        #[arg(long)]
        modes: Option<String>,
        /// Four slots: `output` or `output:actions` separated by `;`
        /// An output is any key action — a key, a chord, a media key, `Macro(0)`.
        /// Actions are four comma-separated values (none/single/until_next/across).
        /// Example: `A:1,0,0,0;B;Ctrl+C:2,0,0,0;VolumeUp`
        #[arg(long)]
        slots: Option<String>,
        /// Enable/disable Rapid Trigger while setting DKS
        #[arg(long)]
        rt: Option<bool>,
    },

    /// Diagnostic: run one write op on a key and report which keys changed (dev tool).
    #[command(hide = true)]
    DksRoundtrip {
        /// Key matrix index to exercise (default 0 — usually Esc)
        #[arg(default_value_t = 0)]
        key: u8,
        /// Which write path to exercise: dks | travel | modes | combo | mode-all | keytrig
        #[arg(long, default_value = "dks")]
        op: String,
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
        /// LED layer (0-3) — an LED bank, unrelated to key layers
        #[arg(short, long, default_value = "0")]
        led_layer: u8,
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
        layer: Layer,
    },

    /// Reset a key to default (supports layer prefix: Fn+Caps, L1+A)
    #[command(visible_alias = "rk")]
    ResetKey {
        /// Key position: name, index, or with layer prefix (Fn+Caps, L1+A)
        key: String,
        /// Layer (0=base, 1=layer1, 2=fn) — overridden by prefix in KEY
        #[arg(short, long, default_value = "0")]
        layer: Layer,
    },

    /// Swap two keys
    Swap {
        /// First key
        key1: String,
        /// Second key
        key2: String,
        /// Profile to swap within (0-3). Swap only ever touches keymatrix layer 0.
        #[arg(short, long, default_value = "0")]
        profile: Profile,
    },

    /// Show per-key bindings across layers (customised keys only by default)
    #[command(visible_aliases = ["km", "remaps", "remap-list", "fnl", "fn-layout"])]
    Keymatrix {
        /// Layers to show: base|l0|0, l1|1, fn|2 (comma-separated; default all).
        /// DKS keys have no Layer1 — their slots 1-3 are outputs, not a layer.
        #[arg(short, long, value_delimiter = ',', value_name = "LAYER")]
        layer: Vec<LayerArg>,
        /// Also show keys and slots that are at their factory default
        #[arg(short, long)]
        unset: bool,
        /// Key selector: classes and/or names, e.g. alpha,function,w,a,s,d.
        /// Prefix with ! to exclude; #N is a matrix position; N..M a range.
        #[arg(short, long, value_delimiter = ',', value_name = "SEL")]
        keys: Vec<iot_driver::keyclass::KeySelector>,
        /// OS variant of the Fn layer
        #[arg(long, value_enum, default_value_t = SysArg::Win)]
        sys: SysArg,
        /// Append the raw 4-byte config to each row
        #[arg(long)]
        raw: bool,
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
        userpic_slot: u8,
    },

    /// List all available LED modes
    Modes,

    // === Audio Commands ===
    /// Run audio reactive LED mode (native on-device music visualizer)
    Audio {
        /// Visualizer: bars (MusicBars) or patterns (MusicPatterns)
        #[arg(value_enum, short, long, default_value = "bars")]
        mode: AudioMode,
        /// Style variant within the mode (bars: 0-2, patterns: 0-4)
        #[arg(long, default_value = "0")]
        style: u8,
        /// Sensitivity multiplier (0.5-2.0)
        #[arg(long, default_value = "1.0")]
        sensitivity: f32,
        /// Update rate in Hz (CPU vs fidelity; clamped 5-120)
        #[arg(long, default_value = "50")]
        rate: u32,
        /// Capture device (exact name or case-insensitive substring); default auto-detects the system monitor source. See `audio-test` for candidates.
        #[arg(short, long)]
        device: Option<String>,
    },

    /// Test audio capture (list devices)
    AudioTest,

    /// Show real-time audio levels
    AudioLevels {
        /// Capture device (exact name or case-insensitive substring); default auto-detects the system monitor source.
        #[arg(short, long)]
        device: Option<String>,
    },

    // === Screen Color Commands ===
    /// Run screen color reactive LED mode (streams average screen color to keyboard)
    #[cfg(feature = "screen-capture")]
    #[command(visible_alias = "screencolor")]
    Screen {
        /// Capture framerate (1-60, default 2)
        #[arg(short, long, default_value = "2")]
        fps: u32,
    },

    // === Dongle Commands ===
    /// Dongle-specific commands (info, pair, status)
    #[command(subcommand)]
    Dongle(DongleCommands),

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

    /// Read device memory as a hex dump (patched firmware only)
    ///
    /// Reads flash, SRAM, the ARM system control block and the known register
    /// blocks; anything else is refused. Reading a register can have side
    /// effects. Examples: `peek 0x2000045c 80` (SRAM), `peek 0x40020410`
    /// (a GPIO register), `peek 0xe000ed28` (fault status).
    Peek {
        /// Address, decimal or 0x… hex
        addr: String,
        /// Number of bytes to read
        #[arg(default_value = "64")]
        len: String,
        /// Access width in bytes: 1, 2 or 4 (default: 4 for registers, 1 for memory)
        #[arg(short, long)]
        width: Option<u8>,
    },

    /// Write one value to device memory (patched firmware only; SRAM and registers)
    ///
    /// Flash and the system control block are refused. A wrong write to a
    /// register can crash the keyboard until it is unplugged. Example:
    /// `poke 0x40020418 0x2000 --yes` sets GPIOB pin 13.
    Poke {
        /// Address, decimal or 0x… hex
        addr: String,
        /// Value, decimal or 0x… hex
        value: String,
        /// Access width in bytes: 1, 2 or 4 (default: 4 for registers, 1 for memory)
        #[arg(short, long)]
        width: Option<u8>,
        /// Confirm the write
        #[arg(long)]
        yes: bool,
    },

    // === Firmware Commands ===
    /// Firmware update tools
    #[command(subcommand, visible_alias = "fw")]
    Firmware(FirmwareCommands),

    // === Utility Commands ===
    /// List all HID devices
    #[command(visible_alias = "ls")]
    List,

    /// Generate a GitHub-ready diagnostic report (Markdown)
    #[command(visible_alias = "diag")]
    Probe {
        /// Also write the report to this file (still printed to stdout)
        #[arg(short, long, value_name = "FILE")]
        output: Option<std::path::PathBuf>,
    },

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
        /// Print daemon activity to stderr
        #[arg(long, short)]
        verbose: bool,
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
        #[arg(long = "var", short = 'v')]
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

    /// Query animation engine status (running defs, frame count)
    AnimStatus,
}

/// Dongle commands
#[derive(Subcommand)]
pub enum DongleCommands {
    /// Show all dongle information (F0 + F7 + FB + FD)
    #[command(visible_alias = "i")]
    Info,

    /// Show detailed dongle status (F7)
    Status,
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
        #[arg(long = "var", short = 'v')]
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
        #[arg(long = "var", short = 'v')]
        vars: Vec<String>,
    },
}

/// OS variant of the Fn layer. The keyboard stores separate Windows and Mac tables.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum SysArg {
    Win,
    Mac,
}

/// OS mode for SET_KBOPTION (firmware stores 2 bits)
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum OsModeArg {
    #[value(alias = "win")]
    Windows,
    #[value(alias = "macos")]
    Mac,
    Ios,
    Android,
}

impl OsModeArg {
    pub fn as_u8(self) -> u8 {
        match self {
            Self::Windows => 0,
            Self::Mac => 1,
            Self::Ios => 2,
            Self::Android => 3,
        }
    }
}

impl SysArg {
    pub fn wire(self) -> u8 {
        match self {
            Self::Win => 0,
            Self::Mac => 1,
        }
    }
}

/// `Layer` for clap. `Layer`'s own `FromStr` already accepts `0|l0|base`, `1|l1`,
/// `2|fn`; this exists so `--help` can enumerate the choices.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum LayerArg {
    Base,
    L1,
    Fn,
}

impl From<LayerArg> for monsgeek_transport::protocol::Layer {
    fn from(a: LayerArg) -> Self {
        match a {
            LayerArg::Base => Self::Base,
            LayerArg::L1 => Self::Layer1,
            LayerArg::Fn => Self::Fn,
        }
    }
}

/// Base per-key trigger mode, selectable on the CLI. The Rapid-Trigger flag is
/// orthogonal and set separately via `--rt`.
#[derive(Copy, Clone, PartialEq, Eq, ValueEnum, Default)]
pub enum KeyModeArg {
    /// Simple actuation/release
    #[default]
    Normal,
    /// Dynamic Keystroke
    Dks,
    /// Mod-Tap
    ModTap,
    /// Toggle (hold variant)
    ToggleHold,
    /// Toggle (tap-toggle variant)
    ToggleDots,
    /// Snap Tap / SOCD
    SnapTap,
}

impl From<KeyModeArg> for monsgeek_keyboard::KeyMode {
    fn from(m: KeyModeArg) -> Self {
        use monsgeek_keyboard::KeyMode;
        match m {
            KeyModeArg::Normal => KeyMode::Normal,
            KeyModeArg::Dks => KeyMode::DynamicKeystroke,
            KeyModeArg::ModTap => KeyMode::ModTap,
            KeyModeArg::ToggleHold => KeyMode::ToggleHold,
            KeyModeArg::ToggleDots => KeyMode::ToggleDots,
            KeyModeArg::SnapTap => KeyMode::SnapTap,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum, Default)]
pub enum AudioMode {
    /// MusicBars (mode 22). On v407 identical to patterns; --style 0=vertical, 1=mirror, 2=left
    #[default]
    Bars,
    /// MusicPatterns (mode 20). Same on-device renderer as bars; --style 0-2
    Patterns,
}

impl AudioMode {
    /// LED mode byte for this visualizer (MusicBars=22 / MusicPatterns=20).
    pub fn led_mode(&self) -> u8 {
        match self {
            AudioMode::Bars => iot_driver::protocol::cmd::LedMode::MusicBars.as_u8(),
            AudioMode::Patterns => iot_driver::protocol::cmd::LedMode::MusicPatterns.as_u8(),
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
