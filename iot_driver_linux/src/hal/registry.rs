// Device Registry - Known HID device interfaces
//
// Single source of truth for all known device interfaces.
// Both hid.rs and grpc.rs should use this registry.

use super::constants;
use super::interface::{HidInterface, InterfaceType};
use std::sync::OnceLock;

/// All known HID interfaces for supported devices
const KNOWN_INTERFACES: &[HidInterface] = &[
    // M1 V5 Wired - FEATURE (commands)
    HidInterface {
        vid: constants::VENDOR_ID,
        pid: constants::PRODUCT_ID_M1_V5_WIRED,
        usage: constants::USAGE_FEATURE,
        usage_page: constants::USAGE_PAGE,
        interface_number: constants::INTERFACE_FEATURE,
        interface_type: InterfaceType::Feature,
    },
    // M1 V5 Wired - INPUT (key depth events)
    HidInterface {
        vid: constants::VENDOR_ID,
        pid: constants::PRODUCT_ID_M1_V5_WIRED,
        usage: constants::USAGE_INPUT,
        usage_page: constants::USAGE_PAGE,
        interface_number: constants::INTERFACE_INPUT,
        interface_type: InterfaceType::Input,
    },
    // M1 V5 Wireless - FEATURE (commands)
    HidInterface {
        vid: constants::VENDOR_ID,
        pid: constants::PRODUCT_ID_M1_V5_WIRELESS,
        usage: constants::USAGE_FEATURE,
        usage_page: constants::USAGE_PAGE,
        interface_number: constants::INTERFACE_FEATURE,
        interface_type: InterfaceType::Feature,
    },
    // M1 V5 Wireless - INPUT (key depth events)
    HidInterface {
        vid: constants::VENDOR_ID,
        pid: constants::PRODUCT_ID_M1_V5_WIRELESS,
        usage: constants::USAGE_INPUT,
        usage_page: constants::USAGE_PAGE,
        interface_number: constants::INTERFACE_INPUT,
        interface_type: InterfaceType::Input,
    },
    // Legacy dongle PIDs (untested, may be other models)
    HidInterface {
        vid: constants::VENDOR_ID,
        pid: constants::PRODUCT_ID_DONGLE_LEGACY_1,
        usage: constants::USAGE_FEATURE,
        usage_page: constants::USAGE_PAGE,
        interface_number: constants::INTERFACE_FEATURE,
        interface_type: InterfaceType::Feature,
    },
    HidInterface {
        vid: constants::VENDOR_ID,
        pid: constants::PRODUCT_ID_DONGLE_LEGACY_1,
        usage: constants::USAGE_INPUT,
        usage_page: constants::USAGE_PAGE,
        interface_number: constants::INTERFACE_INPUT,
        interface_type: InterfaceType::Input,
    },
    HidInterface {
        vid: constants::VENDOR_ID,
        pid: constants::PRODUCT_ID_DONGLE_LEGACY_2,
        usage: constants::USAGE_FEATURE,
        usage_page: constants::USAGE_PAGE,
        interface_number: constants::INTERFACE_FEATURE,
        interface_type: InterfaceType::Feature,
    },
    HidInterface {
        vid: constants::VENDOR_ID,
        pid: constants::PRODUCT_ID_DONGLE_LEGACY_2,
        usage: constants::USAGE_INPUT,
        usage_page: constants::USAGE_PAGE,
        interface_number: constants::INTERFACE_INPUT,
        interface_type: InterfaceType::Input,
    },
];

/// Registry of known HID device interfaces
pub struct DeviceRegistry {
    interfaces: &'static [HidInterface],
}

impl DeviceRegistry {
    /// Create a new registry with built-in known devices
    fn new() -> Self {
        Self {
            interfaces: KNOWN_INTERFACES,
        }
    }

    /// Get all known interfaces
    pub fn all_interfaces(&self) -> &[HidInterface] {
        self.interfaces
    }

    /// Get only client-facing interfaces (FEATURE interfaces)
    pub fn client_facing_interfaces(&self) -> impl Iterator<Item = &HidInterface> {
        self.interfaces.iter().filter(|i| i.is_client_facing())
    }

    /// Get all interfaces for a specific device by VID/PID
    pub fn find_by_vid_pid(&self, vid: u16, pid: u16) -> Vec<&HidInterface> {
        self.interfaces
            .iter()
            .filter(|i| i.vid == vid && i.pid == pid)
            .collect()
    }

    /// Find the FEATURE interface for a device
    pub fn find_feature_interface(&self, vid: u16, pid: u16) -> Option<&HidInterface> {
        self.interfaces.iter().find(|i| {
            i.vid == vid && i.pid == pid && i.interface_type == InterfaceType::Feature
        })
    }

    /// Find the INPUT interface for a device
    pub fn find_input_interface(&self, vid: u16, pid: u16) -> Option<&HidInterface> {
        self.interfaces.iter().find(|i| {
            i.vid == vid && i.pid == pid && i.interface_type == InterfaceType::Input
        })
    }

    /// Check if a VID/PID is a known device
    pub fn is_known_device(&self, vid: u16, pid: u16) -> bool {
        self.interfaces.iter().any(|i| i.vid == vid && i.pid == pid)
    }

    /// Find interface matching a hidapi DeviceInfo
    pub fn find_matching(&self, info: &hidapi::DeviceInfo) -> Option<&HidInterface> {
        self.interfaces.iter().find(|i| i.matches(info))
    }

    /// Get all known VID/PID pairs (unique)
    pub fn known_vid_pids(&self) -> Vec<(u16, u16)> {
        let mut pairs: Vec<(u16, u16)> = self
            .interfaces
            .iter()
            .map(|i| (i.vid, i.pid))
            .collect();
        pairs.sort();
        pairs.dedup();
        pairs
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Global singleton registry
static REGISTRY: OnceLock<DeviceRegistry> = OnceLock::new();

/// Get the global device registry
pub fn device_registry() -> &'static DeviceRegistry {
    REGISTRY.get_or_init(DeviceRegistry::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_devices() {
        let reg = device_registry();
        assert!(!reg.all_interfaces().is_empty());
    }

    #[test]
    fn test_find_feature_interface() {
        let reg = device_registry();
        let iface = reg.find_feature_interface(0x3151, 0x5030);
        assert!(iface.is_some());
        let iface = iface.unwrap();
        assert_eq!(iface.usage, 0x02);
        assert_eq!(iface.interface_number, 2);
    }

    #[test]
    fn test_find_input_interface() {
        let reg = device_registry();
        let iface = reg.find_input_interface(0x3151, 0x5030);
        assert!(iface.is_some());
        let iface = iface.unwrap();
        assert_eq!(iface.usage, 0x01);
        assert_eq!(iface.interface_number, 1);
    }

    #[test]
    fn test_client_facing() {
        let reg = device_registry();
        let client_facing: Vec<_> = reg.client_facing_interfaces().collect();
        // Should have Feature interfaces only
        assert!(client_facing.iter().all(|i| i.interface_type == InterfaceType::Feature));
    }
}
