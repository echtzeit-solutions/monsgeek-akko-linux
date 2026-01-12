// SPDX-License-Identifier: GPL-2.0
//! BPF loader using Aya with struct_ops support

use anyhow::{bail, Context, Result};
use aya::maps::{Array, StructOpsMap};
use aya::programs::links::FdLink;
use aya::{Btf, Ebpf};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

use crate::Strategy;

/// Get the BPF object path for the given strategy
pub fn get_bpf_path(strategy: &Strategy, use_rust: bool) -> Result<PathBuf> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    // Use Rust BPF for ondemand strategy when --rust flag is set
    let relative = if use_rust && matches!(strategy, Strategy::Ondemand) {
        "akko-loader-rs/akko-ebpf.bpf.o"
    } else {
        match strategy {
            Strategy::Keyboard => "option_a_keyboard_inject/akko_keyboard_battery.bpf.o",
            Strategy::Vendor => "option_b_bidirectional/akko_bidirectional.bpf.o",
            Strategy::Wq => "option_b_wq_experimental/akko_wq.bpf.o",
            Strategy::Ondemand => "option_c_on_demand/akko_on_demand.bpf.o",
        }
    };

    let path = base.join(relative);

    if !path.exists() {
        if use_rust && matches!(strategy, Strategy::Ondemand) {
            bail!(
                "Rust BPF object not found: {:?}\nRun 'make akko-ebpf' in bpf/ directory first.",
                path
            );
        }
        bail!(
            "BPF object not found: {:?}\nRun 'make {}' in bpf/ directory first.",
            path,
            match strategy {
                Strategy::Keyboard => "option_a",
                Strategy::Vendor => "option_b",
                Strategy::Wq => "option_b_wq",
                Strategy::Ondemand => "option_c",
            }
        );
    }

    Ok(path)
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

/// Loaded BPF program handle
pub struct LoadedBpf {
    #[allow(dead_code)]
    bpf: Ebpf,
    #[allow(dead_code)]
    link: Option<FdLink>,
    strategy: Strategy,
}

impl LoadedBpf {
    /// Load and register a BPF program for the given strategy
    ///
    /// For Ondemand strategy, throttle_secs configures the minimum interval
    /// between F7 refresh commands.
    pub fn load(strategy: Strategy, hid_id: u32, throttle_secs: u32, use_rust: bool) -> Result<Self> {
        let bpf_path = get_bpf_path(&strategy, use_rust)?;
        info!("Loading BPF from {:?}", bpf_path);

        let mut bpf = Ebpf::load_file(&bpf_path)
            .with_context(|| format!("Failed to load BPF object: {bpf_path:?}"))?;

        // Debug: print available programs and maps
        info!("Available programs:");
        for (name, _prog) in bpf.programs() {
            info!("  - {}", name);
        }
        info!("Available maps:");
        for (name, _map) in bpf.maps() {
            info!("  - {}", name);
        }

        // Load kernel BTF and populate struct_ops with program FDs
        let btf = Btf::from_sys_fs().context("Failed to load kernel BTF")?;
        info!("Calling load_struct_ops...");
        bpf.load_struct_ops(&btf)
            .context("Failed to load struct_ops programs")?;

        // Configure maps for Ondemand strategy (must be after struct_ops loading)
        if matches!(strategy, Strategy::Ondemand) {
            let throttle_ns: u64 = u64::from(throttle_secs) * 1_000_000_000;
            info!("Configuring throttle interval: {}s ({}ns)", throttle_secs, throttle_ns);

            // Map names differ between C and Rust BPF
            let config_map_name = if use_rust { "CONFIG_MAP" } else { "config_map" };

            let mut config_map: Array<_, u64> = bpf
                .map_mut(config_map_name)
                .with_context(|| format!("{config_map_name} not found in BPF"))?
                .try_into()
                .context("Failed to convert config_map")?;

            config_map.set(0, throttle_ns, 0).context("Failed to set throttle in config_map")?;

            // For Rust BPF, set the vendor hid_id in VENDOR_HID_MAP
            // This is needed because Rust BPF can't easily read hid_device.id from kernel struct
            if use_rust {
                let vendor_hid_id = hid_id + 2; // Vendor interface is keyboard + 2
                info!("Setting VENDOR_HID_MAP: vendor_hid_id={} (keyboard={} + 2)", vendor_hid_id, hid_id);

                let mut vendor_map: Array<_, u32> = bpf
                    .map_mut("VENDOR_HID_MAP")
                    .context("VENDOR_HID_MAP not found in Rust BPF")?
                    .try_into()
                    .context("Failed to convert VENDOR_HID_MAP")?;

                vendor_map.set(0, vendor_hid_id, 0).context("Failed to set vendor_hid_id")?;
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
        struct_ops.register().context("Failed to register struct_ops")?;

        // For link-based struct_ops, create the BPF link to activate
        let link = if struct_ops.is_link() {
            info!("Creating BPF link for link-based struct_ops...");
            let link = struct_ops.attach().context("Failed to attach struct_ops")?;
            Some(link)
        } else {
            None
        };

        info!("BPF program loaded and registered successfully!");

        Ok(Self { bpf, link, strategy })
    }

    /// Get the strategy this BPF was loaded with
    #[allow(dead_code)]
    pub fn strategy(&self) -> &Strategy {
        &self.strategy
    }
}

impl Drop for LoadedBpf {
    fn drop(&mut self) {
        info!("Unloading BPF program (strategy: {:?})", self.strategy);
        // Aya handles cleanup automatically when Ebpf is dropped
    }
}
