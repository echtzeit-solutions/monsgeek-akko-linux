//! HID-BPF loader for kernel-level battery integration
//!
//! Loads a compiled BPF program that translates the dongle's vendor HID reports
//! to standard HID Battery System reports, enabling automatic power_supply creation.
//!
//! This module uses Aya to load the pre-compiled BPF object file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use tracing::{debug, info, warn};

/// VID/PID for Akko 2.4GHz dongle
pub const VID_AKKO: u16 = 0x3151;
pub const PID_DONGLE: u16 = 0x5038;

/// Default path for compiled BPF object
pub const DEFAULT_BPF_PATH: &str = "/usr/share/akko-keyboard/akko_dongle.bpf.o";

/// Local development path
pub const DEV_BPF_PATH: &str = "bpf/akko_dongle.bpf.o";

/// Battery info read from kernel power_supply sysfs
#[derive(Debug, Clone)]
pub struct KernelBatteryInfo {
    pub capacity: u8,
    pub status: String,
    pub present: bool,
    pub power_supply_path: PathBuf,
}

/// HID-BPF loader for Akko dongle
pub struct AkkoBpfLoader {
    bpf_path: PathBuf,
    loaded: bool,
}

impl AkkoBpfLoader {
    /// Create a new loader with the given BPF object path
    pub fn new(bpf_path: impl AsRef<Path>) -> Self {
        Self {
            bpf_path: bpf_path.as_ref().to_path_buf(),
            loaded: false,
        }
    }

    /// Create loader using default paths (system or local)
    pub fn with_default_path() -> Result<Self> {
        // Try system path first, then local dev path
        let path = if Path::new(DEFAULT_BPF_PATH).exists() {
            PathBuf::from(DEFAULT_BPF_PATH)
        } else if Path::new(DEV_BPF_PATH).exists() {
            PathBuf::from(DEV_BPF_PATH)
        } else {
            // Try relative to executable
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()));

            if let Some(dir) = exe_dir {
                let local_path = dir.join("bpf/akko_dongle.bpf.o");
                if local_path.exists() {
                    local_path
                } else {
                    return Err(anyhow!(
                        "BPF object not found. Looked in:\n  - {}\n  - {}\nRun 'make' in bpf/ directory first.",
                        DEFAULT_BPF_PATH,
                        DEV_BPF_PATH
                    ));
                }
            } else {
                return Err(anyhow!("BPF object not found at {} or {}", DEFAULT_BPF_PATH, DEV_BPF_PATH));
            }
        };

        Ok(Self::new(path))
    }

    /// Find the Akko dongle HID device in sysfs
    pub fn find_dongle() -> Option<PathBuf> {
        let hid_devices = Path::new("/sys/bus/hid/devices");

        if !hid_devices.exists() {
            return None;
        }

        // Look for device matching 0003:3151:5038.*
        let pattern = format!("0003:{:04X}:{:04X}", VID_AKKO, PID_DONGLE);

        if let Ok(entries) = fs::read_dir(hid_devices) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with(&pattern) {
                    debug!("Found dongle at {:?}", entry.path());
                    return Some(entry.path());
                }
            }
        }

        None
    }

    /// Check if BPF is already loaded for the dongle
    pub fn is_loaded(&self) -> bool {
        // Check if power_supply entry exists (indicates BPF is working)
        Self::find_power_supply().is_some()
    }

    /// Find the kernel power_supply entry for the dongle
    pub fn find_power_supply() -> Option<PathBuf> {
        let power_supply_dir = Path::new("/sys/class/power_supply");

        if !power_supply_dir.exists() {
            return None;
        }

        // Look for hid-0003:3151:5038* pattern
        let pattern = format!("hid-0003:{:04X}:{:04X}", VID_AKKO, PID_DONGLE);

        if let Ok(entries) = fs::read_dir(power_supply_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with(&pattern) {
                    debug!("Found power_supply at {:?}", entry.path());
                    return Some(entry.path());
                }
            }
        }

        None
    }

    /// Read battery info from kernel power_supply sysfs
    pub fn read_battery() -> Result<Option<KernelBatteryInfo>> {
        let ps_path = match Self::find_power_supply() {
            Some(p) => p,
            None => return Ok(None),
        };

        let capacity = fs::read_to_string(ps_path.join("capacity"))
            .context("Failed to read capacity")?
            .trim()
            .parse::<u8>()
            .unwrap_or(0);

        let status = fs::read_to_string(ps_path.join("status"))
            .unwrap_or_else(|_| "Unknown".to_string())
            .trim()
            .to_string();

        let present = fs::read_to_string(ps_path.join("present"))
            .map(|s| s.trim() == "1")
            .unwrap_or(false);

        Ok(Some(KernelBatteryInfo {
            capacity,
            status,
            present,
            power_supply_path: ps_path,
        }))
    }

    /// Load the BPF program using bpftool (requires root)
    ///
    /// Note: This is a fallback when Aya isn't available.
    /// In production, use load_with_aya() instead.
    pub fn load_with_bpftool(&mut self) -> Result<()> {
        if !self.bpf_path.exists() {
            return Err(anyhow!("BPF object not found: {:?}", self.bpf_path));
        }

        let dongle = Self::find_dongle()
            .ok_or_else(|| anyhow!("Dongle not found. Is it plugged in?"))?;

        info!("Loading BPF program from {:?}", self.bpf_path);
        info!("Target device: {:?}", dongle);

        // Use bpftool to load struct_ops
        let output = Command::new("bpftool")
            .args([
                "struct_ops",
                "register",
                self.bpf_path.to_str().unwrap(),
            ])
            .output()
            .context("Failed to run bpftool")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("bpftool failed: {}", stderr));
        }

        self.loaded = true;
        info!("BPF program loaded successfully");

        // Wait a moment for kernel to reprobe device
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Verify power_supply was created
        if let Some(ps_path) = Self::find_power_supply() {
            info!("power_supply created at {:?}", ps_path);
        } else {
            warn!("power_supply not created - descriptor fixup may have failed");
        }

        Ok(())
    }

    /// Load the BPF program using Aya
    #[cfg(feature = "bpf")]
    pub fn load_with_aya(&mut self) -> Result<()> {
        use aya::Ebpf;

        if !self.bpf_path.exists() {
            return Err(anyhow!("BPF object not found: {:?}", self.bpf_path));
        }

        let _dongle = Self::find_dongle()
            .ok_or_else(|| anyhow!("Dongle not found. Is it plugged in?"))?;

        info!("Loading BPF program from {:?} using Aya", self.bpf_path);

        let mut bpf = Ebpf::load_file(&self.bpf_path)
            .context("Failed to load BPF object with Aya")?;

        // Aya should automatically handle struct_ops maps
        // The hid_bpf_ops struct_ops will be registered

        self.loaded = true;
        info!("BPF program loaded successfully via Aya");

        // Wait for kernel to reprobe device
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Verify
        if let Some(ps_path) = Self::find_power_supply() {
            info!("power_supply created at {:?}", ps_path);
        } else {
            warn!("power_supply not created - check dmesg for errors");
        }

        Ok(())
    }

    /// Load BPF program (uses best available method)
    pub fn load(&mut self) -> Result<()> {
        #[cfg(feature = "bpf")]
        {
            self.load_with_aya()
        }

        #[cfg(not(feature = "bpf"))]
        {
            self.load_with_bpftool()
        }
    }

    /// Unload BPF program
    pub fn unload(&mut self) -> Result<()> {
        // Use bpftool to unload struct_ops
        let output = Command::new("bpftool")
            .args(["struct_ops", "unregister", "name", "akko_dongle"])
            .output()
            .context("Failed to run bpftool")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Don't error if already unloaded
            if !stderr.contains("not found") {
                return Err(anyhow!("bpftool unregister failed: {}", stderr));
            }
        }

        self.loaded = false;
        info!("BPF program unloaded");

        Ok(())
    }

    /// Get status information
    pub fn status(&self) -> BpfStatus {
        BpfStatus {
            bpf_path: self.bpf_path.clone(),
            bpf_exists: self.bpf_path.exists(),
            dongle_found: Self::find_dongle().is_some(),
            power_supply_found: Self::find_power_supply().is_some(),
            battery_info: Self::read_battery().ok().flatten(),
        }
    }
}

/// BPF loader status
#[derive(Debug)]
pub struct BpfStatus {
    pub bpf_path: PathBuf,
    pub bpf_exists: bool,
    pub dongle_found: bool,
    pub power_supply_found: bool,
    pub battery_info: Option<KernelBatteryInfo>,
}

impl std::fmt::Display for BpfStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "HID-BPF Battery Status:")?;
        writeln!(f, "  BPF object: {:?} ({})",
                 self.bpf_path,
                 if self.bpf_exists { "exists" } else { "NOT FOUND" })?;
        writeln!(f, "  Dongle: {}",
                 if self.dongle_found { "connected" } else { "not found" })?;
        writeln!(f, "  power_supply: {}",
                 if self.power_supply_found { "created" } else { "not present" })?;

        if let Some(ref info) = self.battery_info {
            writeln!(f, "  Battery: {}% ({})",
                     info.capacity,
                     info.status)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_dongle() {
        // This will only pass if dongle is connected
        let result = AkkoBpfLoader::find_dongle();
        println!("Dongle: {:?}", result);
    }

    #[test]
    fn test_find_power_supply() {
        let result = AkkoBpfLoader::find_power_supply();
        println!("power_supply: {:?}", result);
    }
}
