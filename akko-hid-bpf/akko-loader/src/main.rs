// SPDX-License-Identifier: GPL-2.0
//! Akko/MonsGeek HID-BPF Battery Loader
//!
//! Loads BPF programs to expose keyboard battery via kernel power_supply.
//! The BPF link is pinned to /sys/fs/bpf/akko so it persists after the loader exits.
//!
//! Usage:
//!   akko-loader           # Show status (default)
//!   akko-loader load      # Load BPF and exit
//!   akko-loader unload    # Unload BPF

mod control;
mod hid;
mod loader;

use std::path::PathBuf;

use anyhow::{bail, Result};
use chrono::Local;
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
    #[arg(short = 'i', long, global = true)]
    hid_id: Option<u32>,

    /// F7 refresh throttle interval in seconds (default: 10 minutes)
    #[arg(short, long, default_value = "600", global = true)]
    throttle: u32,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Load BPF programs
    Load,
    /// Unload BPF programs
    Unload,
    /// Verify BPF programs through kernel verifier (CI mode, no hardware required)
    Verify {
        /// Path to BPF object file to verify.
        /// If not provided, uses default location /usr/local/lib/akko/akko-ebpf.bpf.o
        #[arg(long)]
        bpf_path: Option<PathBuf>,
    },
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
        show_power_supplies(None);
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle subcommands - status is default when no subcommand given
    match &cli.command {
        Some(Commands::Unload) => {
            setup_logging(cli.verbose);
            return loader::unload();
        }
        Some(Commands::Verify { bpf_path }) => {
            setup_logging(cli.verbose);
            return loader::verify(bpf_path.as_deref());
        }
        Some(Commands::Load) => {
            // Fall through to load logic below
        }
        None => {
            // Default: show status
            return do_status();
        }
    }

    setup_logging(cli.verbose);

    // Check root for BPF loading
    if !nix::unistd::geteuid().is_root() {
        bail!("Must run as root to load BPF programs");
    }

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    info!(
        "[{}] Akko/MonsGeek Keyboard Battery Loader v{}",
        timestamp, VERSION
    );

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

    // Send initial F7 to prime battery cache and get initial battery level
    let initial_battery = if let Some(ref hidraw) = vendor_hidraw {
        match hid::send_f7_command(hidraw) {
            Ok(battery) => {
                if battery > 100 {
                    warn!(
                        "F7 returned invalid battery value: {}% (raw value out of range)",
                        battery
                    );
                    None
                } else {
                    info!("Initial battery level: {}%", battery);
                    Some(battery)
                }
            }
            Err(e) => {
                warn!("Failed to send F7 command: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Load BPF program (link will be pinned)
    loader::load(hid_info.hid_id, cli.throttle)?;

    // Rebind device to activate BPF
    if !hid_info.device_name.is_empty() {
        hid::rebind_device(&hid_info.device_name)?;
    }

    // Show power supplies with battery info
    show_power_supplies(initial_battery);

    info!("BPF loaded and pinned. Use 'akko-loader unload' to remove.");

    Ok(())
}

fn show_power_supplies(initial_battery: Option<u8>) {
    let ps_dir = std::path::Path::new("/sys/class/power_supply");
    if let Ok(entries) = std::fs::read_dir(ps_dir) {
        println!("\n=== Power supplies ===");
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Read capacity if available
            let capacity_path = entry.path().join("capacity");
            if let Ok(capacity) = std::fs::read_to_string(&capacity_path) {
                let capacity_str = capacity.trim();
                // Validate capacity is in reasonable range
                if let Ok(cap_val) = capacity_str.parse::<u32>() {
                    if cap_val > 100 {
                        println!("{}: {}% (WARNING: invalid value)", name, capacity_str);
                    } else {
                        println!("{}: {}%", name, capacity_str);
                    }
                } else {
                    println!("{}: {}%", name, capacity_str);
                }
            } else {
                println!("{}", name);
            }
        }

        // Show comparison if we have initial battery from F7
        if let Some(battery) = initial_battery {
            println!("\nInitial F7 battery reading: {}%", battery);
        }
    }
}
