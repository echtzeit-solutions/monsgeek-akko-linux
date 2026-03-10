// Device Registry for Akko/MonsGeek Keyboards
// Provides integration with the device database (devices.json) for feature lookup.
// No hardcoded device entries — all metadata comes from the JSON database.

use crate::hal;
use crate::profile::registry::profile_registry;

/// Check if a VID/PID combination is supported (VID-based: any 0x3151 device)
pub fn is_supported(vid: u16, _pid: u16) -> bool {
    vid == hal::VENDOR_ID
}

/// Resolve device info using the best available identifier.
///
/// Lookup order:
/// 1. Device ID in JSON database (unique, correct for shared-PID devices)
/// 2. VID/PID in JSON database (ambiguous if multiple devices share the PID)
fn resolve_json_device(device_id: Option<i32>, vid: u16, pid: u16) -> Option<DeviceInfo> {
    let registry = profile_registry();

    // Try device ID in JSON database (unique match)
    if let Some(id) = device_id {
        if let Some(d) = registry.get_device_info_by_id(id) {
            return Some(DeviceInfo::from_json(d));
        }
    }

    // Fall back to VID/PID in database (may be ambiguous)
    registry
        .get_device_info(vid, pid)
        .map(DeviceInfo::from_json)
}

/// Check if device has magnetism (hall effect switches)
pub fn has_magnetism(vid: u16, pid: u16) -> bool {
    has_magnetism_with_id(None, vid, pid)
}

/// Check if device has magnetism, with firmware device ID for accurate lookup
pub fn has_magnetism_with_id(device_id: Option<i32>, vid: u16, pid: u16) -> bool {
    resolve_json_device(device_id, vid, pid)
        .map(|d| d.has_magnetism)
        .unwrap_or(false)
}

/// Get key count for device (0 for dongles/unknown)
pub fn key_count(vid: u16, pid: u16) -> u8 {
    key_count_with_id(None, vid, pid)
}

/// Get key count, with firmware device ID for accurate lookup
pub fn key_count_with_id(device_id: Option<i32>, vid: u16, pid: u16) -> u8 {
    resolve_json_device(device_id, vid, pid)
        .map(|d| d.key_count)
        .unwrap_or(0)
}

/// Get device display name
pub fn get_display_name(vid: u16, pid: u16) -> Option<String> {
    get_display_name_with_id(None, vid, pid)
}

/// Get device display name, with firmware device ID for accurate lookup
pub fn get_display_name_with_id(device_id: Option<i32>, vid: u16, pid: u16) -> Option<String> {
    resolve_json_device(device_id, vid, pid).map(|d| d.display_name)
}

/// Get device info from the database (if available)
pub fn get_device_info(vid: u16, pid: u16) -> Option<DeviceInfo> {
    resolve_json_device(None, vid, pid)
}

/// Get device info with firmware device ID for accurate lookup
pub fn get_device_info_with_id(device_id: Option<i32>, vid: u16, pid: u16) -> Option<DeviceInfo> {
    resolve_json_device(device_id, vid, pid)
}

/// Device info struct returned by get_device_info
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub display_name: String,
    pub company: Option<String>,
    pub key_count: u8,
    pub has_magnetism: bool,
    pub has_sidelight: bool,
    pub layer_count: Option<u8>,
}

impl DeviceInfo {
    /// Convert from JSON device definition
    fn from_json(d: &crate::device_loader::JsonDeviceDefinition) -> Self {
        Self {
            name: d.name.clone(),
            display_name: d.display_name.clone(),
            company: d.company.clone(),
            key_count: d.key_count.unwrap_or(0),
            has_magnetism: d.has_magnetism(),
            has_sidelight: d.has_side_light.unwrap_or(false),
            layer_count: d.layer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported() {
        assert!(is_supported(0x3151, 0x5030)); // Wired
        assert!(is_supported(0x3151, 0x5038)); // 2.4GHz dongle
        assert!(is_supported(0x3151, 0x5027)); // Bluetooth
        assert!(is_supported(0x3151, 0x502D)); // FUN 60 Pro (unknown PID still supported)
        assert!(is_supported(0x3151, 0xFFFF)); // Any VID=0x3151 device
        assert!(!is_supported(0x1234, 0x5678));
    }

    #[test]
    fn test_device_id_lookup() {
        // M1 V5 TMR (id=2949) should return correct metadata even with shared PID
        let info = get_device_info_with_id(Some(2949), 0x3151, 0x5030);
        if let Some(info) = info {
            // Database should return something reasonable
            assert!(info.key_count > 0);
            assert!(info.has_magnetism);
        }

        // FUN 60 Pro (id=2304) shares PID 0x502D with 46 devices
        let info = get_device_info_with_id(Some(2304), 0x3151, 0x502D);
        if let Some(info) = info {
            assert_eq!(info.key_count, 61); // FUN 60 Pro has 61 keys
            assert!(info.has_magnetism);
            assert!(info.display_name.contains("FUN60"));
        }

        // AttackShark K85 (id=1466) also shares PID 0x502D but has 82 keys
        let info = get_device_info_with_id(Some(1466), 0x3151, 0x502D);
        if let Some(info) = info {
            assert_eq!(info.key_count, 82);
            assert!(info.display_name.contains("K85"));
        }
    }
}
