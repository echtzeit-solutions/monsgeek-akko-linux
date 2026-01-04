// HAL Interface Types - HID interface abstraction
//
// Provides type-safe interface definitions for HID devices.

use super::constants;

/// Type of HID interface
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterfaceType {
    /// FEATURE interface (USAGE=0x02, interface=2) - for sending commands
    Feature,
    /// INPUT interface (USAGE=0x01, interface=1) - for receiving key depth events
    Input,
}

impl InterfaceType {
    /// Returns true if this interface should be reported to clients (gRPC, etc.)
    /// Only FEATURE interfaces are user-facing; INPUT is internal.
    pub fn is_client_facing(&self) -> bool {
        matches!(self, InterfaceType::Feature)
    }

    /// Get the HID usage value for this interface type
    pub fn usage(&self) -> u16 {
        match self {
            InterfaceType::Feature => constants::USAGE_FEATURE,
            InterfaceType::Input => constants::USAGE_INPUT,
        }
    }

    /// Get the interface number for this interface type
    pub fn interface_number(&self) -> i32 {
        match self {
            InterfaceType::Feature => constants::INTERFACE_FEATURE,
            InterfaceType::Input => constants::INTERFACE_INPUT,
        }
    }
}

/// Complete HID interface specification
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidInterface {
    pub vid: u16,
    pub pid: u16,
    pub usage: u16,
    pub usage_page: u16,
    pub interface_number: i32,
    pub interface_type: InterfaceType,
}

impl HidInterface {
    /// Create a new FEATURE interface for a device
    pub const fn feature(vid: u16, pid: u16) -> Self {
        Self {
            vid,
            pid,
            usage: constants::USAGE_FEATURE,
            usage_page: constants::USAGE_PAGE,
            interface_number: constants::INTERFACE_FEATURE,
            interface_type: InterfaceType::Feature,
        }
    }

    /// Create a new INPUT interface for a device
    pub const fn input(vid: u16, pid: u16) -> Self {
        Self {
            vid,
            pid,
            usage: constants::USAGE_INPUT,
            usage_page: constants::USAGE_PAGE,
            interface_number: constants::INTERFACE_INPUT,
            interface_type: InterfaceType::Input,
        }
    }

    /// Check if a hidapi DeviceInfo matches this interface specification
    pub fn matches(&self, info: &hidapi::DeviceInfo) -> bool {
        info.vendor_id() == self.vid
            && info.product_id() == self.pid
            && info.usage_page() == self.usage_page
            && info.usage() == self.usage
            && info.interface_number() == self.interface_number
    }

    /// Generate a unique path key for this interface
    /// Format: "{vid}-{pid}-{usage_page}-{usage}-{interface}"
    pub fn path_key(&self) -> String {
        format!(
            "{}-{}-{}-{}-{}",
            self.vid, self.pid, self.usage_page, self.usage, self.interface_number
        )
    }

    /// Parse a path key back into components
    /// Returns (vid, pid, usage_page, usage, interface_number)
    pub fn parse_path_key(s: &str) -> Option<(u16, u16, u16, u16, i32)> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() >= 5 {
            let vid = parts[0].parse().ok()?;
            let pid = parts[1].parse().ok()?;
            let usage_page = parts[2].parse().ok()?;
            let usage = parts[3].parse().ok()?;
            let interface = parts[4].parse().ok()?;
            Some((vid, pid, usage_page, usage, interface))
        } else {
            None
        }
    }

    /// Returns true if this interface should be reported to clients
    pub fn is_client_facing(&self) -> bool {
        self.interface_type.is_client_facing()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_interface() {
        let iface = HidInterface::feature(0x3151, 0x5030);
        assert_eq!(iface.usage, 0x02);
        assert_eq!(iface.interface_number, 2);
        assert!(iface.is_client_facing());
    }

    #[test]
    fn test_input_interface() {
        let iface = HidInterface::input(0x3151, 0x5030);
        assert_eq!(iface.usage, 0x01);
        assert_eq!(iface.interface_number, 1);
        assert!(!iface.is_client_facing());
    }

    #[test]
    fn test_path_key_roundtrip() {
        let iface = HidInterface::feature(0x3151, 0x5030);
        let key = iface.path_key();
        assert_eq!(key, "12625-20528-65535-2-2");

        let parsed = HidInterface::parse_path_key(&key);
        assert_eq!(parsed, Some((0x3151, 0x5030, 0xFFFF, 0x02, 2)));
    }
}
