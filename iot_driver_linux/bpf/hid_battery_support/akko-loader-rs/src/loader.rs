// SPDX-License-Identifier: GPL-2.0
//! BPF loader using Aya with struct_ops support

use anyhow::{bail, Context, Result};
use aya::maps::{Array, StructOpsMap};
use aya::{Btf, Ebpf};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Directory where we pin BPF links
pub const BPF_PIN_DIR: &str = "/sys/fs/bpf/akko";

/// Installed BPF library path
pub const BPF_LIB_DIR: &str = "/usr/local/lib/akko";

/// BPF object filename
const BPF_FILENAME: &str = "akko-ebpf.bpf.o";

/// struct_ops map name in the BPF object
const STRUCT_OPS_NAME: &str = "akko_on_demand";

/// Get the pin path for the BPF link
fn get_pin_path() -> PathBuf {
    Path::new(BPF_PIN_DIR).join("ondemand_link")
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
                    .with_context(|| format!("Failed to remove {path:?}"))?;
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

/// Get the BPF object path
///
/// Searches in order:
/// 1. Installed path (/usr/local/lib/akko/)
/// 2. Development path (relative to source)
fn get_bpf_path() -> Result<PathBuf> {
    // Try installed path first
    let installed_path = Path::new(BPF_LIB_DIR).join(BPF_FILENAME);
    if installed_path.exists() {
        return Ok(installed_path);
    }

    // Fall back to development path
    let dev_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(BPF_FILENAME);
    if dev_path.exists() {
        return Ok(dev_path);
    }

    bail!(
        "BPF object not found.\nSearched:\n  - {installed_path:?}\n  - {dev_path:?}\nRun 'make akko-ebpf' in bpf/ directory or 'make install-bpf'."
    );
}

/// Load and register the BPF program
///
/// The BPF link is pinned to the filesystem so it persists after the loader exits.
/// Use `unload()` to remove the pinned link.
pub fn load(hid_id: u32, throttle_secs: u32) -> Result<()> {
    let bpf_path = get_bpf_path()?;
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

    // Configure throttle interval
    let throttle_ns: u64 = u64::from(throttle_secs) * 1_000_000_000;
    info!(
        "Configuring throttle interval: {}s ({}ns)",
        throttle_secs, throttle_ns
    );

    let mut config_map: Array<_, u64> = bpf
        .map_mut("CONFIG_MAP")
        .context("CONFIG_MAP not found in BPF")?
        .try_into()
        .context("Failed to convert CONFIG_MAP")?;

    config_map
        .set(0, throttle_ns, 0)
        .context("Failed to set throttle in CONFIG_MAP")?;

    // Set the vendor hid_id (vendor interface is keyboard + 2)
    let vendor_hid_id = hid_id + 2;
    info!(
        "Setting VENDOR_HID_MAP: vendor_hid_id={} (keyboard={} + 2)",
        vendor_hid_id, hid_id
    );

    let mut vendor_map: Array<_, u32> = bpf
        .map_mut("VENDOR_HID_MAP")
        .context("VENDOR_HID_MAP not found in BPF")?
        .try_into()
        .context("Failed to convert VENDOR_HID_MAP")?;

    vendor_map
        .set(0, vendor_hid_id, 0)
        .context("Failed to set vendor_hid_id")?;

    debug!("Looking for struct_ops map: {}", STRUCT_OPS_NAME);

    // Get the struct_ops map
    let map = bpf
        .map_mut(STRUCT_OPS_NAME)
        .with_context(|| format!("struct_ops map '{STRUCT_OPS_NAME}' not found"))?;

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
        let pin_path = get_pin_path();
        info!("Pinning BPF link to {:?}", pin_path);
        link.pin(&pin_path)
            .with_context(|| format!("Failed to pin link to {pin_path:?}"))?;

        info!("BPF program loaded and pinned successfully!");
    } else {
        info!("BPF program loaded successfully (non-link struct_ops)");
    }

    // Forget the bpf object so it doesn't get dropped (link is pinned)
    std::mem::forget(bpf);

    Ok(())
}
