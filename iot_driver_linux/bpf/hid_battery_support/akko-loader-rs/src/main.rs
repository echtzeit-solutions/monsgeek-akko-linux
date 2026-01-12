// SPDX-License-Identifier: GPL-2.0
//! Akko/MonsGeek HID-BPF Battery Loader
//!
//! Loads BPF programs to expose keyboard battery via kernel power_supply.
//! The BPF link is pinned to /sys/fs/bpf/akko so it persists after the loader exits.
//!
//! Usage:
//!   akko-loader           # Load BPF and exit
//!   akko-loader unload    # Unload BPF
//!   akko-loader status    # Show status

mod control;
mod hid;
mod loader;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use tracing::{info, warn};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Akko/MonsGeek Keyboard Battery BPF Loader
#[derive(Parser)]
#[command(name = "akko-loader", version = VERSION, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Loading strategy
    #[arg(short, long, value_enum, default_value = "keyboard")]
    strategy: Strategy,

    /// Override auto-detected HID ID
    #[arg(short = 'i', long)]
    hid_id: Option<u32>,

    /// F7 refresh interval in seconds (default: 10 minutes)
    #[arg(short, long, default_value = "600")]
    refresh: u32,

    /// Use C BPF instead of Rust (akko-ebpf) for ondemand strategy
    #[arg(long)]
    use_c: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Unload BPF programs
    Unload,
    /// Show loader status
    Status,
}

#[derive(Clone, Copy, ValueEnum, Debug)]
pub enum Strategy {
    /// Option A: Inject battery into keyboard interface (recommended)
    Keyboard,
    /// Option B: Use vendor interface with loader F7 refresh
    Vendor,
    /// Option B WQ: Use vendor interface with bpf_wq auto-refresh
    Wq,
    /// Option C: On-demand F7 refresh triggered by UPower reads
    Ondemand,
}

impl std::fmt::Display for Strategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Strategy::Keyboard => write!(f, "keyboard"),
            Strategy::Vendor => write!(f, "vendor"),
            Strategy::Wq => write!(f, "wq"),
            Strategy::Ondemand => write!(f, "ondemand"),
        }
    }
}

fn setup_logging(verbose: bool) {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    fmt().with_env_filter(filter).with_target(false).init();
}

fn do_status() -> Result<()> {
    use std::path::Path;

    let pin_dir = Path::new(loader::BPF_PIN_DIR);

    if !pin_dir.exists() {
        println!("Status: Not loaded");
        return Ok(());
    }

    let mut loaded = Vec::new();
    if let Ok(entries) = std::fs::read_dir(pin_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with("_link") {
                loaded.push(name.trim_end_matches("_link").to_string());
            }
        }
    }

    if loaded.is_empty() {
        println!("Status: Not loaded");
    } else {
        println!("Status: Loaded");
        println!("Strategies: {}", loaded.join(", "));
        println!("Pin directory: {}", loader::BPF_PIN_DIR);

        // Show battery info
        show_power_supplies();
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle subcommands
    match &cli.command {
        Some(Commands::Unload) => {
            setup_logging(cli.verbose);
            return loader::unload();
        }
        Some(Commands::Status) => {
            return do_status();
        }
        None => {}
    }

    setup_logging(cli.verbose);

    // Check root for BPF loading
    if !nix::unistd::geteuid().is_root() {
        bail!("Must run as root to load BPF programs");
    }

    info!("Akko/MonsGeek Keyboard Battery Loader v{}", VERSION);
    info!("Strategy: {}", cli.strategy);

    // Unload any previous BPF programs first
    loader::unload_previous()?;

    // Determine which interface to target
    let want_vendor = matches!(cli.strategy, Strategy::Vendor | Strategy::Wq);

    // Find HID interface
    let hid_info = if let Some(id) = cli.hid_id {
        info!("Using provided hid_id={}", id);
        hid::HidInfo {
            hid_id: id,
            device_name: String::new(),
            hidraw_path: None,
        }
    } else {
        hid::find_hid_interface(want_vendor)?
    };

    info!(
        "Found HID interface: {} (hid_id={})",
        hid_info.device_name, hid_info.hid_id
    );

    // Find vendor hidraw for F7 commands
    let vendor_hidraw = hid_info.hidraw_path.clone().or_else(hid::find_vendor_hidraw);
    if vendor_hidraw.is_none() {
        warn!("Could not find vendor hidraw - battery may show stale values");
    }

    // Send initial F7 to prime battery cache
    if let Some(ref hidraw) = vendor_hidraw {
        match hid::send_f7_command(hidraw) {
            Ok(battery) => info!("Initial F7 sent, battery={}%", battery),
            Err(e) => warn!("Failed to send F7: {}", e),
        }
    }

    // Load BPF program (link will be pinned)
    let use_rust = !cli.use_c;
    loader::load(cli.strategy, hid_info.hid_id, cli.refresh, use_rust)?;

    // Rebind device to activate BPF
    if !hid_info.device_name.is_empty() {
        hid::rebind_device(&hid_info.device_name)?;
    }

    // Show power supplies
    show_power_supplies();

    info!("BPF loaded and pinned. Use 'akko-loader unload' to remove.");

    Ok(())
}

fn show_power_supplies() {
    let ps_dir = std::path::Path::new("/sys/class/power_supply");
    if let Ok(entries) = std::fs::read_dir(ps_dir) {
        println!("\n=== Power supplies ===");
        for entry in entries.flatten() {
            println!("{}", entry.file_name().to_string_lossy());
        }
    }
}
