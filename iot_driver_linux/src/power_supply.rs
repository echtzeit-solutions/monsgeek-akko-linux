//! Power supply sysfs integration for Akko/Monsgeek keyboards
//!
//! Provides battery status via:
//! 1. Kernel HID power_supply (when eBPF filter loaded) - preferred
//! 2. Simple file export (/run/akko-keyboard/battery)
//! 3. Kernel test_power module (if loaded) - appears in UPower

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use crate::hid::BatteryInfo;

/// Path to test_power module parameters
const TEST_POWER_PARAMS: &str = "/sys/module/test_power/parameters";

/// Runtime directory for battery status files
const RUNTIME_DIR: &str = "/run/akko-keyboard";

/// Kernel power_supply sysfs base path
const POWER_SUPPLY_PATH: &str = "/sys/class/power_supply";

// ============================================================================
// Kernel HID Power Supply (via eBPF)
// ============================================================================

/// Find the kernel power_supply path for a HID device by VID/PID.
///
/// When the eBPF filter is loaded, it creates a standard HID battery device
/// which the kernel exposes at `/sys/class/power_supply/hid-0003:VVVV:PPPP.NNNN-battery/`
///
/// Returns the path if found, None otherwise.
pub fn find_hid_battery_power_supply(vid: u16, pid: u16) -> Option<PathBuf> {
    // Pattern: hid-0003:VVVV:PPPP.NNNN-battery (lowercase hex)
    let pattern = format!("hid-0003:{vid:04x}:{pid:04x}");

    let entries = fs::read_dir(POWER_SUPPLY_PATH).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with(&pattern) && name_str.ends_with("-battery") {
            return Some(PathBuf::from(POWER_SUPPLY_PATH).join(name));
        }
    }
    None
}

/// Read battery info from a kernel power_supply sysfs path.
///
/// Reads capacity, status, and present from the standard sysfs interface.
pub fn read_kernel_battery(path: &Path) -> Option<BatteryInfo> {
    let capacity = fs::read_to_string(path.join("capacity"))
        .ok()?
        .trim()
        .parse::<u8>()
        .ok()?;

    let status = fs::read_to_string(path.join("status"))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let present = fs::read_to_string(path.join("present"))
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .map(|v| v == 1)
        .unwrap_or(true);

    Some(BatteryInfo {
        level: capacity,
        online: present,
        charging: status == "Charging",
        idle: false, // Not available from kernel interface
    })
}

/// Power supply status file paths (mimics sysfs structure)
const STATUS_FILE: &str = "status";
const CAPACITY_FILE: &str = "capacity";
const PRESENT_FILE: &str = "present";
const TYPE_FILE: &str = "type";

/// Power supply status values (matching kernel power_supply.h)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSupplyStatus {
    Unknown,
    Charging,
    Discharging,
    NotCharging,
    Full,
}

impl PowerSupplyStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Charging => "Charging",
            Self::Discharging => "Discharging",
            Self::NotCharging => "Not charging",
            Self::Full => "Full",
        }
    }

    /// Get status string for test_power module (lowercase, hyphenated)
    pub fn as_test_power_str(&self) -> &'static str {
        match self {
            Self::Unknown => "discharging", // test_power doesn't have unknown
            Self::Charging => "charging",
            Self::Discharging => "discharging",
            Self::NotCharging => "not-charging",
            Self::Full => "full",
        }
    }
}

/// Integration with the kernel's test_power module.
///
/// When the test_power module is loaded, it creates:
/// - /sys/class/power_supply/test_battery/
/// - /sys/class/power_supply/test_ac/
/// - /sys/class/power_supply/test_usb/
///
/// We can update battery parameters via /sys/module/test_power/parameters/
/// and UPower will automatically pick up the changes.
pub struct TestPowerIntegration {
    available: bool,
}

impl TestPowerIntegration {
    /// Check if test_power module is loaded and create integration
    pub fn new() -> Self {
        let available = Path::new(TEST_POWER_PARAMS).exists();
        Self { available }
    }

    /// Check if test_power integration is available
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Update battery status via test_power module parameters
    pub fn update(&self, info: &BatteryInfo) -> io::Result<()> {
        if !self.available {
            return Ok(());
        }

        // Update capacity (0-100)
        if info.is_valid() {
            self.write_param("battery_capacity", &info.level.to_string())?;
        }

        // Update status
        let status = if info.level > 100 {
            PowerSupplyStatus::Unknown
        } else if info.charging {
            PowerSupplyStatus::Charging
        } else if info.level >= 100 {
            PowerSupplyStatus::Full
        } else {
            PowerSupplyStatus::Discharging
        };
        self.write_param("battery_status", status.as_test_power_str())?;

        // Update present/health (use "good" for present, "dead" for not connected)
        let health = if info.online { "good" } else { "dead" };
        self.write_param("battery_health", health)?;

        Ok(())
    }

    /// Write a parameter to the test_power module
    fn write_param(&self, name: &str, value: &str) -> io::Result<()> {
        let path = PathBuf::from(TEST_POWER_PARAMS).join(name);
        fs::write(&path, value)
    }
}

impl Default for TestPowerIntegration {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared battery state for atomic updates
#[derive(Debug)]
pub struct BatteryState {
    pub level: AtomicU8,
    pub online: AtomicBool,
    pub charging: AtomicBool,
    pub idle: AtomicBool,
}

impl Default for BatteryState {
    fn default() -> Self {
        Self {
            level: AtomicU8::new(255), // Unknown
            online: AtomicBool::new(false),
            charging: AtomicBool::new(false),
            idle: AtomicBool::new(false),
        }
    }
}

impl BatteryState {
    pub fn update(&self, info: &BatteryInfo) {
        self.level.store(info.level, Ordering::Relaxed);
        self.online.store(info.online, Ordering::Relaxed);
        self.charging.store(info.charging, Ordering::Relaxed);
        self.idle.store(info.idle, Ordering::Relaxed);
    }

    pub fn get_info(&self) -> BatteryInfo {
        BatteryInfo {
            level: self.level.load(Ordering::Relaxed),
            online: self.online.load(Ordering::Relaxed),
            charging: self.charging.load(Ordering::Relaxed),
            idle: self.idle.load(Ordering::Relaxed),
        }
    }

    pub fn status(&self) -> PowerSupplyStatus {
        let level = self.level.load(Ordering::Relaxed);
        let charging = self.charging.load(Ordering::Relaxed);

        if level > 100 {
            PowerSupplyStatus::Unknown
        } else if charging {
            PowerSupplyStatus::Charging
        } else if level >= 100 {
            PowerSupplyStatus::Full
        } else {
            PowerSupplyStatus::Discharging
        }
    }
}

/// Power supply sysfs-like interface
pub struct PowerSupply {
    /// Device name (e.g., "akko-m1v5")
    name: String,
    /// Base directory for sysfs-like files
    base_path: PathBuf,
    /// Shared battery state
    state: Arc<BatteryState>,
}

impl PowerSupply {
    /// Create a new power supply interface for a device
    pub fn new(device_name: &str) -> io::Result<Self> {
        let base_path = PathBuf::from(RUNTIME_DIR).join(device_name);

        // Create directory structure
        fs::create_dir_all(&base_path)?;

        let ps = Self {
            name: device_name.to_string(),
            base_path,
            state: Arc::new(BatteryState::default()),
        };

        // Write static files
        ps.write_file(TYPE_FILE, "Battery")?;
        ps.write_file(PRESENT_FILE, "0")?;
        ps.write_file(CAPACITY_FILE, "0")?;
        ps.write_file(STATUS_FILE, PowerSupplyStatus::Unknown.as_str())?;

        Ok(ps)
    }

    /// Get shared state handle for updates from other threads
    pub fn state(&self) -> Arc<BatteryState> {
        Arc::clone(&self.state)
    }

    /// Update battery status from BatteryInfo
    pub fn update(&self, info: &BatteryInfo) -> io::Result<()> {
        self.state.update(info);
        self.sync_to_files()
    }

    /// Sync current state to sysfs-like files
    pub fn sync_to_files(&self) -> io::Result<()> {
        let info = self.state.get_info();
        let status = self.state.status();

        self.write_file(PRESENT_FILE, if info.online { "1" } else { "0" })?;

        if info.is_valid() {
            self.write_file(CAPACITY_FILE, &info.level.to_string())?;
        } else {
            self.write_file(CAPACITY_FILE, "0")?;
        }

        self.write_file(STATUS_FILE, status.as_str())?;

        Ok(())
    }

    /// Write a value to a sysfs-like file
    fn write_file(&self, name: &str, value: &str) -> io::Result<()> {
        let path = self.base_path.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        writeln!(file, "{value}")?;
        Ok(())
    }

    /// Get the base path for this power supply
    pub fn path(&self) -> &Path {
        &self.base_path
    }

    /// Get the device name
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for PowerSupply {
    fn drop(&mut self) {
        // Clean up files on drop
        let _ = fs::remove_dir_all(&self.base_path);
    }
}

/// Manager for multiple power supply devices
pub struct PowerSupplyManager {
    supplies: Vec<PowerSupply>,
}

impl PowerSupplyManager {
    pub fn new() -> Self {
        Self {
            supplies: Vec::new(),
        }
    }

    /// Register a new power supply for a device
    pub fn register(&mut self, device_name: &str) -> io::Result<Arc<BatteryState>> {
        let ps = PowerSupply::new(device_name)?;
        let state = ps.state();
        self.supplies.push(ps);
        Ok(state)
    }

    /// Find power supply by name
    pub fn find(&self, name: &str) -> Option<&PowerSupply> {
        self.supplies.iter().find(|ps| ps.name() == name)
    }

    /// Update all power supplies
    pub fn sync_all(&self) -> io::Result<()> {
        for ps in &self.supplies {
            ps.sync_to_files()?;
        }
        Ok(())
    }
}

impl Default for PowerSupplyManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_battery_state() {
        let state = BatteryState::default();
        assert_eq!(state.level.load(Ordering::Relaxed), 255);

        let info = BatteryInfo {
            level: 75,
            online: true,
            charging: false,
            idle: true,
        };
        state.update(&info);

        assert_eq!(state.level.load(Ordering::Relaxed), 75);
        assert!(state.online.load(Ordering::Relaxed));
        assert!(!state.charging.load(Ordering::Relaxed));
        assert!(state.idle.load(Ordering::Relaxed));
        assert_eq!(state.status(), PowerSupplyStatus::Discharging);
    }

    #[test]
    fn test_status_strings() {
        assert_eq!(PowerSupplyStatus::Charging.as_str(), "Charging");
        assert_eq!(PowerSupplyStatus::Discharging.as_str(), "Discharging");
        assert_eq!(PowerSupplyStatus::Full.as_str(), "Full");
    }
}
