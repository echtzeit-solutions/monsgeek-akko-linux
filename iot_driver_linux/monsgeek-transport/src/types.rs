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

/// Vendor events from input reports (EP2 notifications)
#[derive(Debug, Clone, PartialEq)]
pub enum VendorEvent {
    /// Key depth/magnetism data (0x1B)
    KeyDepth {
        /// Key matrix index
        key_index: u8,
        /// Raw depth value from hall effect sensor
        depth_raw: u16,
    },
    /// Magnetism reporting started (0x0F with start flag)
    MagnetismStart,
    /// Magnetism reporting stopped (0x0F with stop flag)
    MagnetismStop,

    // === Profile & Settings Notifications ===
    /// Keyboard wake from sleep (0x00 - all zeros payload)
    Wake,
    /// Profile changed via Fn+F9..F12 (0x01)
    ProfileChange {
        /// New profile number (0-3)
        profile: u8,
    },
    /// Settings acknowledgment (0x0F)
    SettingsAck {
        /// true = settings change started, false = completed
        started: bool,
    },

    // === LED Settings Notifications ===
    /// LED effect mode changed via Fn+Home/PgUp/End/PgDn (0x04)
    LedEffectMode {
        /// Effect ID (1-20)
        effect_id: u8,
    },
    /// LED effect speed changed via Fn+←/→ (0x05)
    LedEffectSpeed {
        /// Speed level (0-4)
        speed: u8,
    },
    /// Brightness level changed via Fn+↑/↓ (0x06)
    BrightnessLevel {
        /// Brightness level (0-4)
        level: u8,
    },
    /// LED color changed via Fn+\ (0x07)
    LedColor {
        /// Color index (0-7)
        color: u8,
    },

    // === Keyboard Function Notifications (0x03) ===
    /// Win lock toggled via Fn+L_Win (action 0x01)
    WinLockToggle {
        /// true = locked, false = unlocked
        locked: bool,
    },
    /// WASD/Arrow swap toggled via Fn+W (action 0x03)
    WasdSwapToggle {
        /// true = swapped, false = normal
        swapped: bool,
    },
    /// Backlight toggle via Fn+L (action 0x09)
    BacklightToggle,
    /// Dial mode toggle via dial button (action 0x11)
    DialModeToggle,
    /// Unknown keyboard function notification
    UnknownKbFunc {
        /// Category byte
        category: u8,
        /// Action byte
        action: u8,
    },

    // === Battery & Connection ===
    /// Battery status update (from dongle, 0x88)
    BatteryStatus {
        /// Battery level 0-100
        level: u8,
        /// Device is charging
        charging: bool,
        /// Device is online/connected
        online: bool,
    },

    /// Unknown event type (raw bytes for debugging)
    Unknown(Vec<u8>),
}

impl TransportDeviceInfo {
    /// Check if connected via 2.4GHz dongle
    pub fn is_dongle(&self) -> bool {
        self.transport_type == TransportType::HidDongle
    }

    /// Check if connected via wireless transport (dongle, Bluetooth, etc.)
    pub fn is_wireless(&self) -> bool {
        self.transport_type.is_wireless()
    }
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
