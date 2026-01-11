// SPDX-License-Identifier: GPL-2.0
//! Akko/MonsGeek HID-BPF Battery Loader
//!
//! Loads BPF programs to expose keyboard battery via kernel power_supply.
//!
//! Strategies:
//! - keyboard: Inject battery into keyboard interface (recommended)
//! - vendor: Use vendor interface with loader F7 refresh
//! - wq: Use vendor interface with bpf_wq auto-refresh

mod control;
mod hid;
mod loader;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Stop running loader (no sudo needed)
    Stop,
    /// Show loader status (no sudo needed)
    Status,
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum Strategy {
    /// Option A: Inject battery into keyboard interface (recommended)
    Keyboard,
    /// Option B: Use vendor interface with loader F7 refresh
    Vendor,
    /// Option B WQ: Use vendor interface with bpf_wq auto-refresh
    Wq,
}

impl std::fmt::Display for Strategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Strategy::Keyboard => write!(f, "keyboard"),
            Strategy::Vendor => write!(f, "vendor"),
            Strategy::Wq => write!(f, "wq"),
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

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle subcommands that don't need root
    match &cli.command {
        Some(Commands::Stop) => return control::do_stop(),
        Some(Commands::Status) => return control::do_status(),
        None => {}
    }

    setup_logging(cli.verbose);

    // Check root for BPF loading
    if !nix::unistd::geteuid().is_root() {
        bail!("Must run as root to load BPF programs");
    }

    info!("Akko/MonsGeek Keyboard Battery Loader v{}", VERSION);
    info!("Strategy: {}", cli.strategy);

    // Kill any previous loaders
    control::kill_previous_loaders()?;

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

    // Find vendor hidraw for F7 commands (needed for ALL strategies)
    // F7 refreshes battery data from keyboard via dongle's RF link
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

    // Load BPF program with Aya
    let _bpf = loader::LoadedBpf::load(cli.strategy, hid_info.hid_id)?;

    // Write PID file for stop/status commands
    control::write_pid_file()?;

    // Rebind device if we have a device name
    if !hid_info.device_name.is_empty() {
        hid::rebind_device(&hid_info.device_name)?;
    }

    // Show power supplies
    show_power_supplies();

    // Setup signal handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .ok();

    // Main loop
    info!("Running... Stop with: akko-loader stop (or Ctrl+C)");

    let mut seconds_since_f7 = 0u32;

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_secs(1));
        seconds_since_f7 += 1;

        // Check for stop file (non-root stop mechanism)
        if control::check_stop_file() {
            info!("Stop file detected, exiting...");
            break;
        }

        // Periodic F7 refresh to update battery data (all strategies except Wq)
        if !matches!(cli.strategy, Strategy::Wq) && seconds_since_f7 >= cli.refresh {
            if let Some(ref hidraw) = vendor_hidraw {
                if let Ok(battery) = hid::send_f7_command(hidraw) {
                    info!("F7 refresh, battery={}%", battery);
                }
            }
            seconds_since_f7 = 0;
        }
    }

    info!("Unloading BPF program...");
    control::cleanup_files();
    // _bpf drops here, Aya handles cleanup
    info!("Done");

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
