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
use clap::{Parser, Subcommand};
use tracing::{info, warn};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Akko/MonsGeek Keyboard Battery BPF Loader
#[derive(Parser)]
#[command(name = "akko-loader", version = VERSION, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Override auto-detected HID ID
    #[arg(short = 'i', long)]
    hid_id: Option<u32>,

    /// F7 refresh throttle interval in seconds (default: 10 minutes)
    #[arg(short, long, default_value = "600")]
    throttle: u32,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Load BPF programs (default if no subcommand)
    Load,
    /// Unload BPF programs
    Unload,
    /// Show loader status
    Status,
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

    let mut loaded = false;
    if let Ok(entries) = std::fs::read_dir(pin_dir) {
        for entry in entries.flatten() {
            if entry.path().is_file() {
                loaded = true;
                break;
            }
        }
    }

    if !loaded {
        println!("Status: Not loaded");
    } else {
        println!("Status: Loaded");
        println!("Pin directory: {}", loader::BPF_PIN_DIR);
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
        Some(Commands::Load) | None => {
            // Fall through to load logic below
        }
    }

    setup_logging(cli.verbose);

    // Check root for BPF loading
    if !nix::unistd::geteuid().is_root() {
        bail!("Must run as root to load BPF programs");
    }

    info!("Akko/MonsGeek Keyboard Battery Loader v{}", VERSION);

    // Unload any previous BPF programs first
    loader::unload_previous()?;

    // Find keyboard HID interface
    let hid_info = if let Some(id) = cli.hid_id {
        info!("Using provided hid_id={}", id);
        hid::HidInfo {
            hid_id: id,
            device_name: String::new(),
            hidraw_path: None,
        }
    } else {
        hid::find_hid_interface(false)?
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
    loader::load(hid_info.hid_id, cli.throttle)?;

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
