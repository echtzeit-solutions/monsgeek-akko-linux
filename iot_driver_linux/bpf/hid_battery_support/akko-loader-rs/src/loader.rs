// SPDX-License-Identifier: GPL-2.0
//! BPF loader using Aya with struct_ops support

use anyhow::{bail, Context, Result};
use aya::maps::StructOpsMap;
use aya::programs::links::FdLink;
use aya::{Btf, Ebpf};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

use crate::Strategy;

/// Get the BPF object path for the given strategy
pub fn get_bpf_path(strategy: &Strategy) -> Result<PathBuf> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    let relative = match strategy {
        Strategy::Keyboard => "option_a_keyboard_inject/akko_keyboard_battery.bpf.o",
        Strategy::Vendor => "option_b_bidirectional/akko_bidirectional.bpf.o",
        Strategy::Wq => "option_b_wq_experimental/akko_wq.bpf.o",
    };

    let path = base.join(relative);

    if !path.exists() {
        bail!(
            "BPF object not found: {:?}\nRun 'make {}' in bpf/ directory first.",
            path,
            match strategy {
                Strategy::Keyboard => "option_a",
                Strategy::Vendor => "option_b",
                Strategy::Wq => "option_b_wq",
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
    pub fn load(strategy: Strategy, hid_id: u32) -> Result<Self> {
        let bpf_path = get_bpf_path(&strategy)?;
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
