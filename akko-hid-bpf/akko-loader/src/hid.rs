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
        "Could not find {target} interface. Is the dongle connected?"
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
            return Some(format!("/dev/{name_str}"));
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
        .with_context(|| format!("Failed to open {hidraw_path}"))?;

    let fd = file.as_raw_fd();

    // Phase 1: Drain any pre-existing stale responses from the dongle buffer.
    // The dongle can have buffered responses from previous commands (like 0x8F).
    // We flush+read repeatedly until we stop getting new data.
    tracing::debug!("Draining stale responses...");
    for drain_attempt in 0..10 {
        send_dongle_flush_fd(fd)?;
        thread::sleep(Duration::from_millis(10));
        let mut drain_buf = [0u8; 65];
        drain_buf[0] = 0x05;
        let ret = unsafe { libc::ioctl(fd, hidiocgfeature(65), drain_buf.as_mut_ptr()) };
        if ret >= 0 {
            tracing::debug!(
                "Drain {}: cmd=0x{:02x} data={:02x} {:02x} {:02x}",
                drain_attempt,
                drain_buf[0],
                drain_buf[1],
                drain_buf[2],
                drain_buf[3]
            );
        }
    }
    thread::sleep(Duration::from_millis(20));

    // Phase 2: Send F7 command via SET_FEATURE
    let mut buf = [0u8; 65];
    buf[0] = 0x00; // Report ID
    buf[1] = 0xF7; // F7 command
    // Add Bit7 checksum (matches other dongle commands; improves reliability)
    let sum: u16 = buf[1..8].iter().map(|&x| x as u16).sum();
    buf[8] = 255 - (sum & 0xFF) as u8;

    let ret = unsafe { libc::ioctl(fd, hidiocsfeature(65), buf.as_mut_ptr()) };
    if ret < 0 {
        bail!(
            "SET_FEATURE failed: {}",
            std::io::Error::last_os_error()
        );
    }

    // Dongle quirk: some replies appear to be queued (eg responses to other
    // commands like 0x8F). However, battery read sometimes works *without*
    // flushing first (see iot_driver's vendor battery path). So we:
    // - try a direct GET_FEATURE first (no flush) after a short delay
    // - if that doesn't look like battery, fall back to a flush+poll drain loop.
    //
    // Battery response signature (observed):
    // - buf[0] == 0x00 or 0x05 (firmware quirk)
    // - buf[1] in 1..=100
    // - buf[2] == 0x00
    // - buf[5] == 0x01 && buf[6] == 0x01
    const MAX_ATTEMPTS: usize = 20;
    let mut last_head = [0u8; 8];
    let looks_like_battery = |buf: &[u8; 65]| -> Option<u8> {
        let report_ok = buf[0] == 0x00 || buf[0] == 0x05;
        let level = buf[1];
        if report_ok && level > 0 && level <= 100 && buf[2] == 0x00 && buf[5] == 0x01 && buf[6] == 0x01 {
            Some(level)
        } else {
            None
        }
    };

    // First attempt: no flush, just a direct GET after allowing time for the dongle to query RF.
    thread::sleep(Duration::from_millis(80));
    buf.fill(0);
    buf[0] = 0x05;
    let ret0 = unsafe { libc::ioctl(fd, hidiocgfeature(65), buf.as_mut_ptr()) };
    if ret0 >= 0 {
        last_head.copy_from_slice(&buf[..8]);
        if let Some(level) = looks_like_battery(&buf) {
            return Ok(level);
        }
    }

    // Fallback: flush+poll/drain.
    for attempt in 0..MAX_ATTEMPTS {
        // Push next queued response (if any) into the readable buffer
        send_dongle_flush_fd(fd)?;
        thread::sleep(Duration::from_millis(if attempt == 0 { 100 } else { 20 }));

        buf.fill(0);
        buf[0] = 0x05;
        let ret = unsafe { libc::ioctl(fd, hidiocgfeature(65), buf.as_mut_ptr()) };
        if ret < 0 {
            continue;
        }
        last_head.copy_from_slice(&buf[..8]);
        if let Some(level) = looks_like_battery(&buf) {
            return Ok(level);
        }
    }

    bail!(
        "No valid battery response after {MAX_ATTEMPTS} attempts; last head={:02x?}",
        last_head
    )
}

/// Send the dongle flush/NOP command (0xFC) to push buffered response out.
///
/// This uses the same Bit7 checksum scheme as other dongle commands.
pub fn send_dongle_flush(hidraw_path: &str) -> Result<()> {
    let file = File::options()
        .read(true)
        .write(true)
        .open(hidraw_path)
        .with_context(|| format!("Failed to open {hidraw_path}"))?;
    send_dongle_flush_fd(file.as_raw_fd())
}

fn send_dongle_flush_fd(fd: i32) -> Result<()> {
    let mut buf = [0u8; 65];
    buf[0] = 0x00;
    buf[1] = 0xFC;
    let sum: u16 = buf[1..8].iter().map(|&x| x as u16).sum();
    buf[8] = 255 - (sum & 0xFF) as u8;

    let ret = unsafe { libc::ioctl(fd, hidiocsfeature(65), buf.as_mut_ptr()) };
    if ret < 0 {
        bail!(
            "SET_FEATURE flush failed: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

/// Send a dongle command (SET_FEATURE) without reading any response.
///
/// Useful for reproducing the "buffered unrelated report" behavior by spamming
/// GET-type commands (which will accumulate responses in the dongle buffer).
pub fn send_dongle_command_no_read(hidraw_path: &str, cmd: u8) -> Result<()> {
    let file = File::options()
        .read(true)
        .write(true)
        .open(hidraw_path)
        .with_context(|| format!("Failed to open {hidraw_path}"))?;

    let fd = file.as_raw_fd();
    let mut buf = [0u8; 65];
    buf[0] = 0x00;
    buf[1] = cmd;
    let sum: u16 = buf[1..8].iter().map(|&x| x as u16).sum();
    buf[8] = 255 - (sum & 0xFF) as u8;

    let ret = unsafe { libc::ioctl(fd, hidiocsfeature(65), buf.as_mut_ptr()) };
    if ret < 0 {
        bail!(
            "SET_FEATURE cmd=0x{cmd:02x} failed: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

/// Rebind HID device to apply descriptor changes
pub fn rebind_device(device_name: &str) -> Result<()> {
    tracing::info!("Rebinding device {}...", device_name);

    let unbind_path = "/sys/bus/hid/drivers/hid-generic/unbind";
    let bind_path = "/sys/bus/hid/drivers/hid-generic/bind";

    // Unbind
    if let Ok(mut f) = File::create(unbind_path) {
        let _ = write!(f, "{device_name}");
    }

    thread::sleep(Duration::from_millis(100));

    // Bind
    if let Ok(mut f) = File::create(bind_path) {
        let _ = write!(f, "{device_name}");
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
