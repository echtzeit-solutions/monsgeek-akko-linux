// HAL Constants - Single source of truth for device identification
//
// All HID device constants live here. Other modules import from hal::constants.

/// Vendor ID for MonsGeek/Akko devices
pub const VENDOR_ID: u16 = 0x3151;

/// Product IDs for known devices
pub const PRODUCT_ID_M1_V5_WIRED: u16 = 0x5030;
pub const PRODUCT_ID_M1_V5_WIRELESS: u16 = 0x503A;
pub const PRODUCT_ID_DONGLE_1: u16 = 0x503D;
pub const PRODUCT_ID_DONGLE_2: u16 = 0x5040;

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
