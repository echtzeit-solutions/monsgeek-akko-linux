//! Common types for transport layer

/// Transport type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportType {
    /// Direct USB HID connection
    HidWired,
    /// 2.4GHz wireless via USB dongle
    HidDongle,
    /// Bluetooth Low Energy GATT
    Bluetooth,
    /// WebRTC data channel (remote)
    WebRtc,
}

impl TransportType {
    /// Check if this transport is wireless
    pub fn is_wireless(&self) -> bool {
        matches!(self, Self::HidDongle | Self::Bluetooth | Self::WebRtc)
    }
}

/// Device identification information
#[derive(Debug, Clone)]
pub struct TransportDeviceInfo {
    /// USB Vendor ID
    pub vid: u16,
    /// USB Product ID
    pub pid: u16,
    /// Whether this is a wireless dongle
    pub is_dongle: bool,
    /// Transport type
    pub transport_type: TransportType,
    /// Device path or identifier (transport-specific)
    pub device_path: String,
    /// Serial number if available
    pub serial: Option<String>,
    /// Product name if available
    pub product_name: Option<String>,
}

/// Checksum configuration for commands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChecksumType {
    /// Sum bytes 1-7, store 255-(sum&0xFF) at byte 8 (most commands)
    #[default]
    Bit7,
    /// Sum bytes 1-8, store 255-(sum&0xFF) at byte 9 (LED commands)
    Bit8,
    /// No checksum
    None,
}

/// Vendor events from input reports
#[derive(Debug, Clone)]
pub enum VendorEvent {
    /// Key depth/magnetism data
    KeyDepth {
        /// Key matrix index
        key_index: u8,
        /// Raw depth value from hall effect sensor
        depth_raw: u16,
    },
    /// Magnetism reporting started
    MagnetismStart,
    /// Magnetism reporting stopped
    MagnetismStop,
    /// Profile changed
    ProfileChange {
        /// New profile number (0-3)
        profile: u8,
    },
    /// LED effect changed
    LedChange {
        /// New LED mode
        mode: u8,
    },
    /// Battery status update
    BatteryStatus {
        /// Battery level 0-100
        level: u8,
        /// Device is charging
        charging: bool,
        /// Device is online/connected
        online: bool,
    },
    /// Unknown event type
    Unknown(Vec<u8>),
}

/// Discovered device that can be opened
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    /// Device information
    pub info: TransportDeviceInfo,
}

/// Discovery events for hot-plug support
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    /// A device was added
    DeviceAdded(DiscoveredDevice),
    /// A device was removed
    DeviceRemoved(TransportDeviceInfo),
}
