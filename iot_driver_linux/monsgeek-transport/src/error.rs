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
