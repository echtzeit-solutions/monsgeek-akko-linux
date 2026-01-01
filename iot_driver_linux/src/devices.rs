// Device Registry for Akko/MonsGeek Keyboards
// Defines supported devices and their capabilities

use crate::protocol;

/// Device definition with capabilities
#[derive(Debug, Clone, Copy)]
pub struct DeviceDefinition {
    pub vid: u16,
    pub pid: u16,
    pub name: &'static str,
    pub display_name: &'static str,
    pub key_count: u8,
    pub has_magnetism: bool,
    pub has_sidelight: bool,
}

/// All supported devices
/// Add new devices here as they are tested
pub const SUPPORTED_DEVICES: &[DeviceDefinition] = &[
    // MonsGeek M1 V5 HE (our primary test device)
    DeviceDefinition {
        vid: protocol::VENDOR_ID,
        pid: protocol::PRODUCT_ID_M1_V5_WIRED,
        name: "m1v5he_wired",
        display_name: "MonsGeek M1 V5 HE",
        key_count: 98,
        has_magnetism: true,
        has_sidelight: false,
    },
    // MonsGeek M1 V5 HE Wireless
    DeviceDefinition {
        vid: protocol::VENDOR_ID,
        pid: protocol::PRODUCT_ID_M1_V5_WIRELESS,
        name: "m1v5he_wireless",
        display_name: "MonsGeek M1 V5 HE (Wireless)",
        key_count: 98,
        has_magnetism: true,
        has_sidelight: false,
    },
    // Wireless dongle (variant 1)
    DeviceDefinition {
        vid: protocol::VENDOR_ID,
        pid: protocol::PRODUCT_ID_DONGLE_1,
        name: "dongle_1",
        display_name: "MonsGeek Wireless Dongle",
        key_count: 0,
        has_magnetism: false,
        has_sidelight: false,
    },
    // Wireless dongle (variant 2)
    DeviceDefinition {
        vid: protocol::VENDOR_ID,
        pid: protocol::PRODUCT_ID_DONGLE_2,
        name: "dongle_2",
        display_name: "MonsGeek Wireless Dongle Alt",
        key_count: 0,
        has_magnetism: false,
        has_sidelight: false,
    },
];

/// Find device definition by VID/PID
pub fn find_device(vid: u16, pid: u16) -> Option<&'static DeviceDefinition> {
    SUPPORTED_DEVICES.iter().find(|d| d.vid == vid && d.pid == pid)
}

/// Check if a VID/PID combination is supported
pub fn is_supported(vid: u16, pid: u16) -> bool {
    find_device(vid, pid).is_some()
}

/// Get all supported PIDs for a given VID
pub fn get_pids_for_vid(vid: u16) -> Vec<u16> {
    SUPPORTED_DEVICES
        .iter()
        .filter(|d| d.vid == vid)
        .map(|d| d.pid)
        .collect()
}

/// Check if device has magnetism (hall effect switches)
pub fn has_magnetism(vid: u16, pid: u16) -> bool {
    find_device(vid, pid).map(|d| d.has_magnetism).unwrap_or(false)
}

/// Get key count for device (0 for dongles)
pub fn key_count(vid: u16, pid: u16) -> u8 {
    find_device(vid, pid).map(|d| d.key_count).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_m1v5he() {
        let dev = find_device(0x3151, 0x5030);
        assert!(dev.is_some());
        let dev = dev.unwrap();
        assert_eq!(dev.display_name, "MonsGeek M1 V5 HE");
        assert_eq!(dev.key_count, 98);
        assert!(dev.has_magnetism);
    }

    #[test]
    fn test_is_supported() {
        assert!(is_supported(0x3151, 0x5030));
        assert!(is_supported(0x3151, 0x503A));
        assert!(!is_supported(0x1234, 0x5678));
    }

    #[test]
    fn test_get_pids() {
        let pids = get_pids_for_vid(0x3151);
        assert_eq!(pids.len(), 4);
        assert!(pids.contains(&0x5030));
        assert!(pids.contains(&0x503A));
    }
}
