// SPDX-License-Identifier: GPL-2.0
//! BPF loader using Aya with struct_ops support

use anyhow::{bail, Context, Result};
use aya::maps::{Array, StructOpsMap};
use aya::{Btf, Ebpf};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::Strategy;

/// Directory where we pin BPF links
pub const BPF_PIN_DIR: &str = "/sys/fs/bpf/akko";

/// Get the pin path for a given strategy
pub fn get_pin_path(strategy: &Strategy) -> PathBuf {
    Path::new(BPF_PIN_DIR).join(format!("{}_link", strategy))
}

/// Unload any previously pinned BPF programs
pub fn unload_previous() -> Result<()> {
    let pin_dir = Path::new(BPF_PIN_DIR);

    if !pin_dir.exists() {
        return Ok(());
    }

    info!("Checking for previously loaded BPF programs...");

    if let Ok(entries) = std::fs::read_dir(pin_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                info!("Removing pinned link: {:?}", path);
                if let Err(e) = std::fs::remove_file(&path) {
                    warn!("Failed to remove {:?}: {}", path, e);
                }
            }
        }
    }

    // Try to remove the directory if empty
    let _ = std::fs::remove_dir(pin_dir);

    Ok(())
}

/// Unload command - remove all pinned BPF programs
pub fn unload() -> Result<()> {
    let pin_dir = Path::new(BPF_PIN_DIR);

    if !pin_dir.exists() {
        info!("No BPF programs loaded (pin directory doesn't exist)");
        return Ok(());
    }

    let mut found = false;
    if let Ok(entries) = std::fs::read_dir(pin_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                found = true;
                info!("Unloading: {:?}", path);
                std::fs::remove_file(&path)
                    .with_context(|| format!("Failed to remove {:?}", path))?;
            }
        }
    }

    if !found {
        info!("No BPF programs were loaded");
    } else {
        info!("BPF programs unloaded successfully");
    }

    // Remove the directory
    let _ = std::fs::remove_dir(pin_dir);

    Ok(())
}

/// Check if any BPF programs are currently loaded
pub fn is_loaded() -> bool {
    let pin_dir = Path::new(BPF_PIN_DIR);
    if !pin_dir.exists() {
        return false;
    }

    if let Ok(entries) = std::fs::read_dir(pin_dir) {
        for entry in entries.flatten() {
            if entry.path().is_file() {
                return true;
            }
        }
    }

    false
}

/// Installed BPF library path
pub const BPF_LIB_DIR: &str = "/usr/local/lib/akko";

/// Get the BPF object path for the given strategy
///
/// Searches in order:
/// 1. Installed path (/usr/local/lib/akko/)
/// 2. Development path (relative to source)
pub fn get_bpf_path(strategy: &Strategy, use_rust: bool) -> Result<PathBuf> {
    // Determine the BPF filename
    let filename = if use_rust && matches!(strategy, Strategy::Ondemand) {
        "akko-ebpf.bpf.o"
    } else {
        match strategy {
            Strategy::Keyboard => "akko_keyboard_battery.bpf.o",
            Strategy::Vendor => "akko_bidirectional.bpf.o",
            Strategy::Wq => "akko_wq.bpf.o",
            Strategy::Ondemand => "akko_on_demand.bpf.o",
        }
    };

    // Try installed path first
    let installed_path = Path::new(BPF_LIB_DIR).join(filename);
    if installed_path.exists() {
        return Ok(installed_path);
    }

    // Fall back to development path
    let dev_base = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let dev_relative = if use_rust && matches!(strategy, Strategy::Ondemand) {
        "akko-loader-rs/akko-ebpf.bpf.o"
    } else {
        match strategy {
            Strategy::Keyboard => "option_a_keyboard_inject/akko_keyboard_battery.bpf.o",
            Strategy::Vendor => "option_b_bidirectional/akko_bidirectional.bpf.o",
            Strategy::Wq => "option_b_wq_experimental/akko_wq.bpf.o",
            Strategy::Ondemand => "option_c_on_demand/akko_on_demand.bpf.o",
        }
    };
    let dev_path = dev_base.join(dev_relative);

    if dev_path.exists() {
        return Ok(dev_path);
    }

    // Neither found - provide helpful error
    if use_rust && matches!(strategy, Strategy::Ondemand) {
        bail!(
            "Rust BPF object not found.\nSearched:\n  - {:?}\n  - {:?}\nRun 'make akko-ebpf' in bpf/ directory or 'make install-bpf'.",
            installed_path, dev_path
        );
    }
    bail!(
        "BPF object not found.\nSearched:\n  - {:?}\n  - {:?}\nRun 'make {}' in bpf/ directory or 'make install-bpf'.",
        installed_path, dev_path,
        match strategy {
            Strategy::Keyboard => "option_a",
            Strategy::Vendor => "option_b",
            Strategy::Wq => "option_b_wq",
            Strategy::Ondemand => "option_c",
        }
    );
}

/// Get the struct_ops map name for the given strategy
fn get_struct_ops_name(strategy: &Strategy) -> &'static str {
    match strategy {
        Strategy::Keyboard => "akko_keyboard_battery",
        Strategy::Vendor => "akko_bidirectional",
        Strategy::Wq => "akko_wq",
        Strategy::Ondemand => "akko_on_demand",
    }
}

/// Load and register a BPF program for the given strategy
///
/// The BPF link is pinned to the filesystem so it persists after the loader exits.
/// Use `unload()` to remove the pinned link.
pub fn load(strategy: Strategy, hid_id: u32, throttle_secs: u32, use_rust: bool) -> Result<()> {
    let bpf_path = get_bpf_path(&strategy, use_rust)?;
    info!("Loading BPF from {:?}", bpf_path);

    let mut bpf = Ebpf::load_file(&bpf_path)
        .with_context(|| format!("Failed to load BPF object: {bpf_path:?}"))?;

    // Debug: print available programs and maps
    debug!("Available programs:");
    for (name, _prog) in bpf.programs() {
        debug!("  - {}", name);
    }
    debug!("Available maps:");
    for (name, _map) in bpf.maps() {
        debug!("  - {}", name);
    }

    // Load kernel BTF and populate struct_ops with program FDs
    let btf = Btf::from_sys_fs().context("Failed to load kernel BTF")?;
    debug!("Calling load_struct_ops...");
    bpf.load_struct_ops(&btf)
        .context("Failed to load struct_ops programs")?;

    // Configure maps for Ondemand strategy (must be after struct_ops loading)
    if matches!(strategy, Strategy::Ondemand) {
        let throttle_ns: u64 = u64::from(throttle_secs) * 1_000_000_000;
        info!(
            "Configuring throttle interval: {}s ({}ns)",
            throttle_secs, throttle_ns
        );

        // Map names differ between C and Rust BPF
        let config_map_name = if use_rust { "CONFIG_MAP" } else { "config_map" };

        let mut config_map: Array<_, u64> = bpf
            .map_mut(config_map_name)
            .with_context(|| format!("{config_map_name} not found in BPF"))?
            .try_into()
            .context("Failed to convert config_map")?;

        config_map
            .set(0, throttle_ns, 0)
            .context("Failed to set throttle in config_map")?;

        // For Rust BPF, set the vendor hid_id in VENDOR_HID_MAP
        if use_rust {
            let vendor_hid_id = hid_id + 2; // Vendor interface is keyboard + 2
            info!(
                "Setting VENDOR_HID_MAP: vendor_hid_id={} (keyboard={} + 2)",
                vendor_hid_id, hid_id
            );

            let mut vendor_map: Array<_, u32> = bpf
                .map_mut("VENDOR_HID_MAP")
                .context("VENDOR_HID_MAP not found in Rust BPF")?
                .try_into()
                .context("Failed to convert VENDOR_HID_MAP")?;

            vendor_map
                .set(0, vendor_hid_id, 0)
                .context("Failed to set vendor_hid_id")?;
        }
    }

    let struct_ops_name = get_struct_ops_name(&strategy);
    debug!("Looking for struct_ops map: {}", struct_ops_name);

    // Get the struct_ops map
    let map = bpf
        .map_mut(struct_ops_name)
        .with_context(|| format!("struct_ops map '{struct_ops_name}' not found"))?;

    let mut struct_ops: StructOpsMap<_> = map
        .try_into()
        .context("Failed to convert to StructOpsMap")?;

    // Set hid_id at offset 0 (first field in hid_bpf_ops)
    debug!("Setting hid_id={} at offset 0", hid_id);
    struct_ops
        .set_field_i32(0, hid_id as i32)
        .context("Failed to set hid_id")?;

    // Register the struct_ops with the kernel
    info!("Registering struct_ops with kernel...");
    struct_ops
        .register()
        .context("Failed to register struct_ops")?;

    // For link-based struct_ops, create and pin the BPF link
    if struct_ops.is_link() {
        info!("Creating BPF link for link-based struct_ops...");
        let link = struct_ops.attach().context("Failed to attach struct_ops")?;

        // Create pin directory if needed
        let pin_dir = Path::new(BPF_PIN_DIR);
        if !pin_dir.exists() {
            std::fs::create_dir_all(pin_dir).context("Failed to create BPF pin directory")?;
        }

        // Pin the link so it persists after we exit
        let pin_path = get_pin_path(&strategy);
        info!("Pinning BPF link to {:?}", pin_path);
        link.pin(&pin_path)
            .with_context(|| format!("Failed to pin link to {:?}", pin_path))?;

        info!("BPF program loaded and pinned successfully!");
    } else {
        info!("BPF program loaded successfully (non-link struct_ops)");
    }

    // Forget the bpf object so it doesn't get dropped (link is pinned)
    std::mem::forget(bpf);

    Ok(())
}
