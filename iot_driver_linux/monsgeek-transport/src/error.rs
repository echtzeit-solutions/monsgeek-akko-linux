//! Transport error types

use thiserror::Error;

/// Errors that can occur during transport operations
#[derive(Error, Debug)]
pub enum TransportError {
    // Common errors
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Device disconnected")]
    Disconnected,

    #[error("Communication timeout")]
    Timeout,

    #[error("Invalid response: expected cmd 0x{expected:02X}, got 0x{actual:02X}")]
    InvalidResponse { expected: u8, actual: u8 },

    #[error("Checksum mismatch")]
    ChecksumError,

    // HID-specific errors
    #[error("HID error: {0}")]
    HidError(String),

    #[error("HID permission denied: {0}")]
    HidPermissionDenied(String),

    // Bluetooth-specific errors
    #[error("Bluetooth error: {0}")]
    BluetoothError(String),

    #[error("GATT characteristic not found: {0}")]
    GattCharacteristicNotFound(String),

    #[error("Bluetooth pairing required")]
    PairingRequired,

    // WebRTC-specific errors
    #[error("WebRTC error: {0}")]
    WebRtcError(String),

    #[error("Session expired")]
    SessionExpired,

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Network error: {0}")]
    NetworkError(String),

    // Dongle-specific
    #[error("Dongle buffer overflow")]
    DongleBufferOverflow,

    #[error("Keyboard not responding via dongle")]
    KeyboardOffline,

    // Generic
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<hidapi::HidError> for TransportError {
    fn from(e: hidapi::HidError) -> Self {
        let msg = e.to_string();
        if msg.contains("Permission denied") || msg.contains("EPERM") {
            TransportError::HidPermissionDenied(msg)
        } else {
            TransportError::HidError(msg)
        }
    }
}
