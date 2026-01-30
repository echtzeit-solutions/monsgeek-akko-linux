// Device profile trait definition
// Provides abstraction for device-specific data

use super::types::TravelSettings;
use crate::hal::constants::MATRIX_SIZE_M1_V5;

/// Device profile trait - provides all device-specific data
///
/// This trait is object-safe for dynamic dispatch (Arc<dyn DeviceProfile>).
/// Implementations can be either builtin Rust structs or loaded from JSON.
pub trait DeviceProfile: Send + Sync {
    // === Identity ===

    /// Unique device ID (matches webapp database)
    fn id(&self) -> u32;

    /// USB Vendor ID
    fn vid(&self) -> u16;

    /// USB Product ID
    fn pid(&self) -> u16;

    /// Internal name (e.g., "m1v5he_wired")
    fn name(&self) -> &str;

    /// User-facing display name (e.g., "MonsGeek M1 V5 HE")
    fn display_name(&self) -> &str;

    /// Company/brand name
    fn company(&self) -> &str {
        "MonsGeek"
    }

    // === Layout ===

    /// Number of physical keys
    fn key_count(&self) -> u8;

    /// Total matrix positions (typically 126)
    fn matrix_size(&self) -> usize {
        MATRIX_SIZE_M1_V5
    }

    /// Number of key layers
    fn layer_count(&self) -> u8 {
        4
    }

    // === Matrix Mappings ===

    /// LED matrix: position -> HID keycode (0 = empty/no LED)
    fn led_matrix(&self) -> &[u8];

    /// Get key name for a matrix position
    fn matrix_key_name(&self, position: u8) -> &str;

    /// Get all active matrix positions with their HID codes
    fn active_positions(&self) -> Vec<(usize, u8)> {
        self.led_matrix()
            .iter()
            .enumerate()
            .filter(|(_, &hid)| hid != 0)
            .map(|(pos, &hid)| (pos, hid))
            .collect()
    }

    /// Find LED matrix position for a HID keycode
    fn hid_to_position(&self, hid_code: u8) -> Option<usize> {
        if hid_code == 0 {
            return None;
        }
        self.led_matrix().iter().position(|&h| h == hid_code)
    }

    // === Features ===

    /// Whether device has magnetism (Hall effect switches)
    fn has_magnetism(&self) -> bool;

    /// Whether device has sidelight LEDs
    fn has_sidelight(&self) -> bool {
        false
    }

    /// Whether device has a screen
    fn has_screen(&self) -> bool {
        false
    }

    /// Whether device has a rotary knob
    fn has_knob(&self) -> bool {
        false
    }

    // === Magnetism Settings ===

    /// Travel settings for magnetic switches (if supported)
    fn travel_settings(&self) -> Option<&TravelSettings> {
        None
    }

    /// Precision factor for depth readings based on firmware version
    fn precision_factor(&self, firmware_version: u16) -> f32 {
        // Default precision factors based on firmware version
        // Version format: major * 256 + minor (e.g., 1280 = 5.0)
        if firmware_version >= 1280 {
            200.0 // 0.005mm precision
        } else if firmware_version >= 768 {
            100.0 // 0.01mm precision
        } else {
            10.0 // 0.1mm precision
        }
    }

    // === Fn Layer ===

    /// Fn system layer for Windows
    fn fn_layer_win(&self) -> u8 {
        1
    }

    /// Fn system layer for Mac
    fn fn_layer_mac(&self) -> u8 {
        1
    }
}

/// Extension trait for DeviceProfile with utility methods
pub trait DeviceProfileExt: DeviceProfile {
    /// Check if this profile matches a VID/PID pair
    fn matches(&self, vid: u16, pid: u16) -> bool {
        self.vid() == vid && self.pid() == pid
    }

    /// Get a short description of the device
    fn description(&self) -> String {
        format!(
            "{} ({:04X}:{:04X}, {} keys)",
            self.display_name(),
            self.vid(),
            self.pid(),
            self.key_count()
        )
    }
}

// Blanket implementation for all DeviceProfile implementors
impl<T: DeviceProfile + ?Sized> DeviceProfileExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock profile for testing
    struct MockProfile;

    impl DeviceProfile for MockProfile {
        fn id(&self) -> u32 {
            1
        }
        fn vid(&self) -> u16 {
            0x3151
        }
        fn pid(&self) -> u16 {
            0x5030
        }
        fn name(&self) -> &str {
            "mock"
        }
        fn display_name(&self) -> &str {
            "Mock Device"
        }
        fn key_count(&self) -> u8 {
            98
        }
        fn led_matrix(&self) -> &[u8] {
            &[41, 53, 43, 0, 0, 0] // Esc, `, Tab, empty...
        }
        fn matrix_key_name(&self, pos: u8) -> &str {
            match pos {
                0 => "Esc",
                1 => "`",
                2 => "Tab",
                _ => "?",
            }
        }
        fn has_magnetism(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_device_profile_ext() {
        let profile = MockProfile;

        assert!(profile.matches(0x3151, 0x5030));
        assert!(!profile.matches(0x1234, 0x5678));

        let desc = profile.description();
        assert!(desc.contains("Mock Device"));
        assert!(desc.contains("3151:5030"));
        assert!(desc.contains("98 keys"));
    }

    #[test]
    fn test_active_positions() {
        let profile = MockProfile;
        let active = profile.active_positions();

        assert_eq!(active.len(), 3);
        assert_eq!(active[0], (0, 41)); // Esc
        assert_eq!(active[1], (1, 53)); // `
        assert_eq!(active[2], (2, 43)); // Tab
    }

    #[test]
    fn test_precision_factor() {
        let profile = MockProfile;

        assert_eq!(profile.precision_factor(1280), 200.0);
        assert_eq!(profile.precision_factor(1000), 100.0);
        assert_eq!(profile.precision_factor(500), 10.0);
    }
}
