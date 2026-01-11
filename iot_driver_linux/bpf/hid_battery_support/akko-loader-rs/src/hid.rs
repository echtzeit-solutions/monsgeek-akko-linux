// SPDX-License-Identifier: GPL-2.0
//! HID device discovery and operations for Akko/MonsGeek dongle

use anyhow::{bail, Context, Result};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::thread;
use std::time::Duration;

/// VID/PID for Akko 2.4GHz dongle
pub const VID_AKKO: u16 = 0x3151;
pub const PID_DONGLE: u16 = 0x5038;

/// HID interface information
#[derive(Debug, Clone)]
pub struct HidInfo {
    pub hid_id: u32,
    pub device_name: String,
    pub hidraw_path: Option<String>,
}

/// Find HID interface by VID/PID and descriptor pattern
pub fn find_hid_interface(want_vendor: bool) -> Result<HidInfo> {
    let hid_devices = Path::new("/sys/bus/hid/devices");

    if !hid_devices.exists() {
        bail!("HID sysfs not available at /sys/bus/hid/devices");
    }

    let target = if want_vendor { "vendor" } else { "keyboard" };
    tracing::info!("Searching for {} interface VID={:04x} PID={:04x}...", target, VID_AKKO, PID_DONGLE);

    for entry in fs::read_dir(hid_devices)?.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Parse device name: 0003:VVVV:PPPP.IIII
        let parts: Vec<&str> = name_str.split(':').collect();
        if parts.len() < 3 {
            continue;
        }

        // Check VID
        let vid = match u16::from_str_radix(parts[1], 16) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Parse PID and ID (PID.ID format)
        let pid_id: Vec<&str> = parts[2].split('.').collect();
        if pid_id.len() != 2 {
            continue;
        }

        let pid = match u16::from_str_radix(pid_id[0], 16) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let hid_id = match u32::from_str_radix(pid_id[1], 16) {
            Ok(i) => i,
            Err(_) => continue,
        };

        if vid != VID_AKKO || pid != PID_DONGLE {
            continue;
        }

        tracing::debug!("Checking {}...", name_str);

        // Read report descriptor
        let rdesc_path = entry.path().join("report_descriptor");
        let rdesc = match fs::read(&rdesc_path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        tracing::debug!(
            "  Descriptor size={}, first bytes: {:02x} {:02x} {:02x}",
            rdesc.len(),
            rdesc.first().unwrap_or(&0),
            rdesc.get(1).unwrap_or(&0),
            rdesc.get(2).unwrap_or(&0)
        );

        // Match interface type by descriptor pattern
        let is_vendor = rdesc.len() >= 3
            && rdesc[0] == 0x06
            && rdesc[1] == 0xFF
            && rdesc[2] == 0xFF;

        let is_keyboard = rdesc.len() >= 4
            && rdesc[0] == 0x05
            && rdesc[1] == 0x01
            && rdesc[2] == 0x09
            && rdesc[3] == 0x06;

        if (want_vendor && is_vendor) || (!want_vendor && is_keyboard) {
            tracing::info!(
                "Found {} interface: {} (hid_id={})",
                target,
                name_str,
                hid_id
            );

            let hidraw_path = if want_vendor {
                find_hidraw_for_hid(&entry.path())
            } else {
                None
            };

            return Ok(HidInfo {
                hid_id,
                device_name: name_str.to_string(),
                hidraw_path,
            });
        }
    }

    bail!(
        "Could not find {} interface. Is the dongle connected?",
        target
    );
}

/// Find vendor interface hidraw for F7 commands (used by all strategies)
pub fn find_vendor_hidraw() -> Option<String> {
    let hid_devices = Path::new("/sys/bus/hid/devices");
    if !hid_devices.exists() {
        return None;
    }

    for entry in fs::read_dir(hid_devices).ok()?.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Parse device name: 0003:VVVV:PPPP.IIII
        let parts: Vec<&str> = name_str.split(':').collect();
        if parts.len() < 3 {
            continue;
        }

        let vid = u16::from_str_radix(parts[1], 16).ok()?;
        let pid_id: Vec<&str> = parts[2].split('.').collect();
        if pid_id.len() != 2 {
            continue;
        }
        let pid = u16::from_str_radix(pid_id[0], 16).ok()?;

        if vid != VID_AKKO || pid != PID_DONGLE {
            continue;
        }

        // Read descriptor to check if vendor interface
        let rdesc_path = entry.path().join("report_descriptor");
        let rdesc = fs::read(&rdesc_path).ok()?;

        let is_vendor = rdesc.len() >= 3
            && rdesc[0] == 0x06
            && rdesc[1] == 0xFF
            && rdesc[2] == 0xFF;

        if is_vendor {
            return find_hidraw_for_hid(&entry.path());
        }
    }

    None
}

/// Find hidraw device for a HID interface
fn find_hidraw_for_hid(hid_path: &Path) -> Option<String> {
    let hidraw_dir = hid_path.join("hidraw");

    if !hidraw_dir.exists() {
        return None;
    }

    for entry in fs::read_dir(hidraw_dir).ok()?.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with("hidraw") {
            return Some(format!("/dev/{}", name_str));
        }
    }

    None
}

// HIDRAW ioctl definitions
// From linux/hidraw.h:
// #define HIDIOCGFEATURE(len) _IOC(_IOC_WRITE|_IOC_READ, 'H', 0x07, len)
// #define HIDIOCSFEATURE(len) _IOC(_IOC_WRITE|_IOC_READ, 'H', 0x06, len)

const HIDRAW_MAGIC: u8 = b'H';

fn hidiocgfeature(len: usize) -> nix::libc::c_ulong {
    // _IOC(_IOC_WRITE|_IOC_READ, 'H', 0x07, len)
    // _IOC_WRITE = 1, _IOC_READ = 2, combined = 3
    // Direction: 2 bits at 30-31, Size: 14 bits at 16-29, Type: 8 bits at 8-15, Nr: 8 bits at 0-7
    let dir: u32 = 3; // _IOC_WRITE | _IOC_READ
    let size = (len as u32) & 0x3FFF;
    let typ = HIDRAW_MAGIC as u32;
    let nr: u32 = 0x07;
    ((dir << 30) | (size << 16) | (typ << 8) | nr) as nix::libc::c_ulong
}

fn hidiocsfeature(len: usize) -> nix::libc::c_ulong {
    let dir: u32 = 3;
    let size = (len as u32) & 0x3FFF;
    let typ = HIDRAW_MAGIC as u32;
    let nr: u32 = 0x06;
    ((dir << 30) | (size << 16) | (typ << 8) | nr) as nix::libc::c_ulong
}

/// Send F7 command to refresh battery data
/// Returns the battery percentage if successful
pub fn send_f7_command(hidraw_path: &str) -> Result<u8> {
    tracing::debug!("Sending F7 command...");

    let file = File::options()
        .read(true)
        .write(true)
        .open(hidraw_path)
        .with_context(|| format!("Failed to open {}", hidraw_path))?;

    let fd = file.as_raw_fd();

    // Send F7 command via SET_FEATURE
    let mut buf = [0u8; 65];
    buf[0] = 0x00; // Report ID
    buf[1] = 0xF7; // F7 command

    let ret = unsafe { libc::ioctl(fd, hidiocsfeature(65), buf.as_mut_ptr()) };
    if ret < 0 {
        bail!(
            "SET_FEATURE failed: {}",
            std::io::Error::last_os_error()
        );
    }

    // Wait a bit for the dongle to query the keyboard
    thread::sleep(Duration::from_millis(100));

    // Read battery via GET_FEATURE Report 5
    buf.fill(0);
    buf[0] = 0x05; // Report ID 5

    let ret = unsafe { libc::ioctl(fd, hidiocgfeature(65), buf.as_mut_ptr()) };
    if ret < 0 {
        bail!(
            "GET_FEATURE failed: {}",
            std::io::Error::last_os_error()
        );
    }

    // Battery is at offset 1 (Report ID 0 is at offset 0 due to firmware quirk)
    // The firmware returns Report ID 0x00 instead of 0x05
    let battery = if buf[0] == 0x00 && buf[1] <= 100 {
        buf[1]
    } else if buf[0] == 0x05 && buf[1] <= 100 {
        buf[1]
    } else {
        tracing::warn!("Unexpected battery response: {:02x?}", &buf[..8]);
        0
    };

    Ok(battery)
}

/// Rebind HID device to apply descriptor changes
pub fn rebind_device(device_name: &str) -> Result<()> {
    tracing::info!("Rebinding device {}...", device_name);

    let unbind_path = "/sys/bus/hid/drivers/hid-generic/unbind";
    let bind_path = "/sys/bus/hid/drivers/hid-generic/bind";

    // Unbind
    if let Ok(mut f) = File::create(unbind_path) {
        let _ = write!(f, "{}", device_name);
    }

    thread::sleep(Duration::from_millis(100));

    // Bind
    if let Ok(mut f) = File::create(bind_path) {
        let _ = write!(f, "{}", device_name);
    }

    thread::sleep(Duration::from_millis(100));
    tracing::info!("Device rebound");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_numbers() {
        // Verify ioctl number calculation
        // These should match the C macros
        let get = hidiocgfeature(65);
        let set = hidiocsfeature(65);

        // Expected values from C:
        // HIDIOCGFEATURE(65) = 0xC0414807
        // HIDIOCSFEATURE(65) = 0xC0414806
        println!("GET_FEATURE(65) = 0x{:08X}", get);
        println!("SET_FEATURE(65) = 0x{:08X}", set);

        assert_eq!(get, 0xC0414807, "GET_FEATURE ioctl mismatch");
        assert_eq!(set, 0xC0414806, "SET_FEATURE ioctl mismatch");
    }
}
