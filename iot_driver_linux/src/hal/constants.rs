// HAL Constants - Single source of truth for device identification
//
// All HID device constants live here. Other modules import from hal::constants.

/// Vendor ID for MonsGeek/Akko devices
pub const VENDOR_ID: u16 = 0x3151;

/// Product IDs for known devices
pub const PRODUCT_ID_M1_V5_WIRED: u16 = 0x5030;
pub const PRODUCT_ID_M1_V5_WIRELESS: u16 = 0x5038; // 2.4GHz dongle
pub const PRODUCT_ID_M1_V5_BLUETOOTH: u16 = 0x5027; // Bluetooth HID (BLE)
pub const PRODUCT_ID_DONGLE_LEGACY_1: u16 = 0x503A; // possibly other model
pub const PRODUCT_ID_DONGLE_LEGACY_2: u16 = 0x503D; // possibly other model

/// All known Bluetooth PIDs (BLE HID devices)
pub const BLUETOOTH_PIDS: &[u16] = &[
    PRODUCT_ID_M1_V5_BLUETOOTH, // 0x5027
];

/// Check if PID represents a Bluetooth device
#[inline]
pub fn is_bluetooth_pid(pid: u16) -> bool {
    BLUETOOTH_PIDS.contains(&pid)
}

/// All known dongle PIDs (2.4GHz wireless receivers)
pub const DONGLE_PIDS: &[u16] = &[
    PRODUCT_ID_M1_V5_WIRELESS,  // 0x5038
    PRODUCT_ID_DONGLE_LEGACY_1, // 0x503A
    PRODUCT_ID_DONGLE_LEGACY_2, // 0x503D
];

/// Check if PID represents a 2.4GHz dongle
#[inline]
pub fn is_dongle_pid(pid: u16) -> bool {
    DONGLE_PIDS.contains(&pid)
}

/// Vendor-specific HID usage page (0xFFFF)
pub const USAGE_PAGE: u16 = 0xFFFF;

/// HID Usage for FEATURE interface (interface 2) - for sending commands
pub const USAGE_FEATURE: u16 = 0x02;

/// HID Usage for INPUT interface (interface 1) - for receiving key depth, events
pub const USAGE_INPUT: u16 = 0x01;

/// Interface number for FEATURE interface
pub const INTERFACE_FEATURE: i32 = 2;

/// Interface number for INPUT interface
pub const INTERFACE_INPUT: i32 = 1;

/// Backward-compatible alias for USAGE_FEATURE
pub const USAGE: u16 = USAGE_FEATURE;

/// Backward-compatible alias for PRODUCT_ID_M1_V5_WIRED
pub const PRODUCT_ID: u16 = PRODUCT_ID_M1_V5_WIRED;

/// Backward-compatible alias for INTERFACE_FEATURE
pub const INTERFACE: i32 = INTERFACE_FEATURE;
