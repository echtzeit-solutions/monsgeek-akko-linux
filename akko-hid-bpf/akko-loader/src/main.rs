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

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Load BPF programs
    Load {
        /// Path to BPF object file to load.
        /// If not provided, uses default location /usr/local/lib/akko/akko-ebpf.bpf.o
        #[arg(long)]
        bpf_path: Option<PathBuf>,
    },
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
    let bpf_path = match &cli.command {
        Some(Commands::Unload) => {
            setup_logging(cli.verbose);
            return loader::unload();
        }
        Some(Commands::Verify { bpf_path }) => {
            setup_logging(cli.verbose);
            return loader::verify(bpf_path.as_deref());
        }
        Some(Commands::Load { bpf_path }) => {
            // Fall through to load logic below
            bpf_path.clone()
        }
        None => {
            // Default: show status
            return do_status();
        }
    };

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

    // Find vendor interface for BPF to query
    let vendor_info = hid::find_hid_interface(true).ok();
    let vendor_hid_id = vendor_info.as_ref().map(|v| v.hid_id);
    if let Some(ref vi) = vendor_info {
        info!("Found vendor interface: {} (hid_id={})", vi.device_name, vi.hid_id);
    } else {
        warn!("Could not find vendor interface - BPF battery query may fail");
    }

    // Find vendor hidraw for F7 commands
    let vendor_hidraw = vendor_info
        .as_ref()
        .and_then(|v| v.hidraw_path.as_deref().map(|s| s.to_owned()))
        .or_else(hid::find_vendor_hidraw);
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
    loader::load(
        hid_info.hid_id,
        vendor_hid_id,
        initial_battery,
        bpf_path.as_deref(),
    )?;

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

// =============================================================================
// Hardware integration tests (ignored by default)
// =============================================================================

#[cfg(test)]
mod hw_tests {
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};
    use std::path::PathBuf;

    /// Find the first hid-* power supply capacity path.
    fn find_hid_capacity_path() -> Option<std::path::PathBuf> {
        let ps_dir = std::path::Path::new("/sys/class/power_supply");
        let entries = std::fs::read_dir(ps_dir).ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("hid-") {
                continue;
            }
            let cap = entry.path().join("capacity");
            if cap.exists() {
                return Some(cap);
            }
        }
        None
    }

    fn read_capacity(path: &std::path::Path) -> Option<u8> {
        let s = std::fs::read_to_string(path).ok()?;
        s.trim().parse::<u8>().ok()
    }

    fn wait_for_nonzero_capacity(path: &std::path::Path, timeout: Duration) -> Option<u8> {
        let start = Instant::now();
        // Avoid hammering sysfs too fast: each read triggers a hid_hw_request and trace output.
        // We only need "eventually non-zero", so a slower poll is fine (and less confusing).
        while start.elapsed() < timeout {
            if let Some(v) = read_capacity(path) {
                if v > 0 && v <= 100 {
                    return Some(v);
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        None
    }

    fn capture_tracepipe_snippet() -> Option<String> {
        // Optional, best-effort: requires debugfs/tracing.
        let trace_pipe = "/sys/kernel/debug/tracing/trace_pipe";
        if !std::path::Path::new(trace_pipe).exists() {
            return None;
        }
        let out = Command::new("sh")
            .arg("-lc")
            .arg(format!(
                "timeout 1s cat {trace_pipe} 2>/dev/null | grep -E 'akko_rev' || true"
            ))
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// Reproduce the "buffered unrelated report" issue:
    /// - Load hid-bpf
    /// - Read sysfs capacity (should be non-zero)
    /// - Spam unrelated dongle commands without reading responses
    /// - Read sysfs capacity again; regression is returning 0
    /// - If it regresses, send userspace F7 again and confirm sysfs updates
    #[test]
    #[ignore]
    fn pathological_buffered_responses_sysfs() {
        if !nix::unistd::geteuid().is_root() {
            panic!("This test must be run as root (needs BPF load + sysfs rebind)");
        }

        // Reset any previous pinned program.
        loader::unload_previous().expect("unload_previous");

        // Discover keyboard + vendor interfaces.
        let kb = hid::find_hid_interface(false).expect("find keyboard hid interface");
        let vendor = hid::find_hid_interface(true).expect("find vendor hid interface");
        let vendor_hid_id = Some(vendor.hid_id);
        let vendor_hidraw = vendor
            .hidraw_path
            .as_deref()
            .map(|s| s.to_owned())
            .or_else(hid::find_vendor_hidraw)
            .expect("find vendor hidraw");

        eprintln!("[test] vendor_hidraw={vendor_hidraw} vendor_hid_id={} kb_hid_id={}", vendor.hid_id, kb.hid_id);

        // Prime/observe battery via explicit userspace packet.
        let userspace_battery = hid::send_f7_command(&vendor_hidraw).expect("userspace F7");
        eprintln!("[test] userspace F7 battery={userspace_battery}%");

        // IMPORTANT: force loading the dev-bundled BPF object, not the installed one.
        // The loader search order prefers /usr/local/lib/akko/akko-ebpf.bpf.o if it exists,
        // which is often an older build and can reintroduce verifier failures.
        let bpf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("akko-ebpf.bpf.o");
        eprintln!("[test] loading BPF from {}", bpf_path.display());
        loader::load(kb.hid_id, vendor_hid_id, Some(userspace_battery), Some(&bpf_path))
            .expect("load BPF");

        // Rebind to apply changes.
        hid::rebind_device(&kb.device_name).expect("rebind device");
        eprintln!("[test] rebind done, sleeping 500ms to let sysfs settle");
        std::thread::sleep(Duration::from_millis(500));

        // Locate kernel power_supply capacity node.
        let cap_path = find_hid_capacity_path().expect("find /sys/class/power_supply/hid-*/capacity");
        eprintln!("[test] cap_path={}", cap_path.display());

        // Baseline sysfs read should become non-zero and typically match userspace.
        let baseline = wait_for_nonzero_capacity(&cap_path, Duration::from_secs(2))
            .unwrap_or_else(|| panic!("sysfs capacity stayed at 0; trace:\n{}", capture_tracepipe_snippet().unwrap_or_default()));

        eprintln!(
            "baseline: userspace={}%, sysfs={}%, cap_path={}",
            userspace_battery,
            baseline,
            cap_path.display()
        );

        // Spam an unrelated GET-type command without reading its responses to fill the dongle buffer.
        // GET_USB_VERSION (0x8F) is a harmless query and produces a response.
        eprintln!("[test] spamming 200x GET_USB_VERSION (0x8F) without reading");
        for _ in 0..200u32 {
            let _ = hid::send_dongle_command_no_read(&vendor_hidraw, 0x8F);
        }
        eprintln!("[test] spam complete, sleeping 1000ms before re-reading sysfs");
        std::thread::sleep(Duration::from_millis(1000));

        // Now read sysfs again (give it a short window to settle).
        let after_spam = wait_for_nonzero_capacity(&cap_path, Duration::from_secs(2)).unwrap_or(0);
        eprintln!(
            "after_spam: sysfs={}%, trace:\n{}",
            after_spam,
            capture_tracepipe_snippet().unwrap_or_default()
        );

        if after_spam == 0 {
            // Workaround check: explicit userspace F7 should make kernel update again.
            let userspace_battery2 = hid::send_f7_command(&vendor_hidraw).expect("userspace F7 retry");
            let updated = wait_for_nonzero_capacity(&cap_path, Duration::from_secs(2))
                .unwrap_or_else(|| panic!("sysfs did not update after userspace F7; trace:\n{}", capture_tracepipe_snippet().unwrap_or_default()));
            eprintln!(
                "recovered: userspace2={}%, sysfs_updated={}%",
                userspace_battery2, updated
            );
            assert!(updated > 0 && updated <= 100);
        } else {
            assert!(after_spam <= 100);
        }
    }
}
