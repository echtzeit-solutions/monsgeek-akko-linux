//! Device registry - which USB devices to talk to, and their transport type by PID
//!
//! Device identity comes from firmware query (`get_device_id`), not from USB PID.

use std::collections::HashSet;
use std::sync::OnceLock;

/// MonsGeek vendor ID. Every PID under it is treated as a candidate.
pub const VENDOR_ID: u16 = 0x3151;

/// USB IDs from the device database. Rebranded boards (Akko 0x38EE and others)
/// speak the same protocol under their own VIDs, but those VIDs also cover
/// unrelated products, so only listed VID:PID pairs are accepted there.
static KNOWN_USB_IDS: OnceLock<HashSet<(u16, u16)>> = OnceLock::new();

/// Register the database's USB IDs for discovery. Only the first call takes
/// effect; returns whether this one did.
pub fn register_known_usb_ids(ids: impl IntoIterator<Item = (u16, u16)>) -> bool {
    KNOWN_USB_IDS.set(ids.into_iter().collect()).is_ok()
}

/// Is this a device discovery should look at: any [`VENDOR_ID`] product, or a
/// VID:PID registered from the device database.
pub fn is_candidate_device(vid: u16, pid: u16) -> bool {
    vid == VENDOR_ID
        || KNOWN_USB_IDS
            .get()
            .is_some_and(|ids| ids.contains(&(vid, pid)))
}

/// Known dongle PIDs (2.4GHz wireless receivers)
///
/// These PIDs indicate the transport type should be HidDongle.
/// The actual device identity is determined by firmware query.
pub const DONGLE_PIDS: &[u16] = &[
    0x5038, // M1 V5 HE dongle
    0x503A, // Legacy dongle variant
    0x503D, // Legacy dongle variant
    0x502B, // M1 V5 TMR dongle
    0x5006, // Attack Shark K86 dongle
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

/// Heuristic: does this device's USB product string look like a 2.4GHz dongle?
///
/// Used as a fallback for dongles whose PID is not yet in [`DONGLE_PIDS`], so
/// new receivers are classified as `HidDongle` instead of `HidWired`. The PID
/// list remains the authoritative fast path; this only kicks in for unknown
/// PIDs. Protocol-level confirmation (a `GET_DONGLE_INFO` response) is the
/// ground truth before adding a PID to the list — see the `probe` command.
///
/// Observed strings: dongle = "MonsGeek 2.4G Wireless Keyboard"; wired =
/// "Monsgeek Keyboard" / "Monsgeek Multi-modes Keyboard".
pub fn looks_like_dongle(product: Option<&str>) -> bool {
    let Some(p) = product else { return false };
    let p = p.to_ascii_lowercase();
    ["wireless", "2.4g", "dongle", "receiver"]
        .iter()
        .any(|needle| p.contains(needle))
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

    #[test]
    fn candidates_are_any_monsgeek_pid_or_a_registered_pair() {
        assert!(register_known_usb_ids([(0x38EE, 0x0016)]));
        assert!(!register_known_usb_ids([(0x1234, 0x5678)]));

        assert!(is_candidate_device(VENDOR_ID, 0x5030));
        assert!(is_candidate_device(VENDOR_ID, 0xFFFF));
        assert!(is_candidate_device(0x38EE, 0x0016));
        // another product under a registered VID is not a candidate
        assert!(!is_candidate_device(0x38EE, 0x0017));
        assert!(!is_candidate_device(0x1234, 0x5678));
    }

    #[test]
    fn test_looks_like_dongle() {
        // Real product strings observed on the bus
        assert!(looks_like_dongle(Some("MonsGeek 2.4G Wireless Keyboard")));
        assert!(looks_like_dongle(Some("Akko Wireless Dongle")));
        assert!(looks_like_dongle(Some("USB Receiver")));
        // Wired keyboards must not match
        assert!(!looks_like_dongle(Some("Monsgeek Keyboard")));
        assert!(!looks_like_dongle(Some("Monsgeek Multi-modes Keyboard")));
        assert!(!looks_like_dongle(None));
    }
}
