//! Device registry - transport type detection by PID
//!
//! This module provides centralized dongle PID detection for determining
//! transport type. Device identity comes from firmware query (`get_device_id`),
//! not from USB PID.

/// MonsGeek/Akko vendor ID
pub const VENDOR_ID: u16 = 0x3151;

/// Known dongle PIDs (2.4GHz wireless receivers)
///
/// These PIDs indicate the transport type should be HidDongle.
/// The actual device identity is determined by firmware query.
pub const DONGLE_PIDS: &[u16] = &[
    0x5038, // M1 V5 HE dongle
    0x503A, // Legacy dongle variant
    0x503D, // Legacy dongle variant
];

/// Known Bluetooth PIDs (BLE HID connections via HOGP)
///
/// These PIDs indicate the transport type should be HidBluetooth.
/// Bluetooth devices connect via kernel's hid-over-gatt driver.
pub const BLUETOOTH_PIDS: &[u16] = &[
    0x5027, // M1 V5 HE Bluetooth
];

/// Check if PID represents a 2.4GHz dongle
///
/// This is used for transport type detection only. Device identity
/// is determined by `get_device_id()` firmware query, not by PID.
#[inline]
pub fn is_dongle_pid(pid: u16) -> bool {
    DONGLE_PIDS.contains(&pid)
}

/// Check if PID represents a Bluetooth device
///
/// Bluetooth devices use Report ID 6 for vendor commands instead
/// of Report ID 0 used by USB devices.
#[inline]
pub fn is_bluetooth_pid(pid: u16) -> bool {
    BLUETOOTH_PIDS.contains(&pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_dongle_pids() {
        assert!(is_dongle_pid(0x5038));
        assert!(is_dongle_pid(0x503A));
        assert!(is_dongle_pid(0x503D));
    }

    #[test]
    fn test_wired_pids_not_dongle() {
        assert!(!is_dongle_pid(0x5030)); // M1 V5 wired
        assert!(!is_dongle_pid(0x0000));
    }

    #[test]
    fn test_known_bluetooth_pids() {
        assert!(is_bluetooth_pid(0x5027)); // M1 V5 Bluetooth
    }

    #[test]
    fn test_wired_pids_not_bluetooth() {
        assert!(!is_bluetooth_pid(0x5030)); // M1 V5 wired
        assert!(!is_bluetooth_pid(0x5038)); // M1 V5 dongle
    }
}
