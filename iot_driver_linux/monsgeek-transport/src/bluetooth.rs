//! Bluetooth GATT transport (stub for future implementation)
//!
//! This module will provide Bluetooth Low Energy (BLE) GATT transport for
//! wireless keyboard communication without a dongle.
//!
//! # Future Implementation Notes
//!
//! MonsGeek/Akko keyboards use a custom GATT service for communication.
//! The service characteristics mirror the HID feature report structure:
//!
//! - Service UUID: TBD (needs reverse engineering from Windows app/firmware)
//! - Command characteristic: Write with response
//! - Event characteristic: Notify for async events (key depth, battery)
//!
//! # Dependencies
//!
//! When implementing, consider using:
//! - `btleplug` crate for cross-platform BLE support
//! - Platform-specific APIs (BlueZ on Linux) for advanced features

use std::sync::Arc;

use crate::error::TransportError;
use crate::types::{ChecksumType, TransportDeviceInfo, VendorEvent};
use crate::Transport;

/// Bluetooth GATT transport (stub)
///
/// # Note
///
/// This is a placeholder for future Bluetooth support.
/// Enable the `bluetooth` feature to use this transport.
pub struct BluetoothTransport {
    device_info: TransportDeviceInfo,
    // Future fields:
    // device: btleplug::platform::Peripheral,
    // command_char: Characteristic,
    // event_char: Characteristic,
}

impl BluetoothTransport {
    /// Create a new Bluetooth transport (stub)
    ///
    /// # Note
    ///
    /// This currently returns an error as Bluetooth is not yet implemented.
    pub fn new(_device_address: &str) -> Result<Self, TransportError> {
        Err(TransportError::BluetoothError(
            "Bluetooth transport not yet implemented".into(),
        ))
    }

    /// Scan for MonsGeek/Akko keyboards (stub)
    ///
    /// # Returns
    ///
    /// Empty vec as Bluetooth is not yet implemented.
    pub fn scan_devices(_timeout_ms: u32) -> Result<Vec<BluetoothDeviceInfo>, TransportError> {
        Ok(Vec::new())
    }

    /// Pair with a keyboard (stub)
    pub fn pair(&self) -> Result<(), TransportError> {
        Err(TransportError::BluetoothError(
            "Bluetooth transport not yet implemented".into(),
        ))
    }
}

/// Information about a discovered Bluetooth device
#[derive(Debug, Clone)]
pub struct BluetoothDeviceInfo {
    /// Bluetooth device address
    pub address: String,
    /// Device name (if advertised)
    pub name: Option<String>,
    /// Signal strength (RSSI)
    pub rssi: Option<i16>,
}

impl Transport for BluetoothTransport {
    fn send_report(
        &self,
        _cmd: u8,
        _data: &[u8],
        _checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        Err(TransportError::BluetoothError(
            "Bluetooth transport not yet implemented".into(),
        ))
    }

    fn read_report(&self) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::BluetoothError(
            "Bluetooth transport not yet implemented".into(),
        ))
    }

    fn read_event(&self, _timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        Err(TransportError::BluetoothError(
            "Bluetooth transport not yet implemented".into(),
        ))
    }

    fn device_info(&self) -> &TransportDeviceInfo {
        &self.device_info
    }

    fn is_connected(&self) -> bool {
        false
    }

    fn close(&self) -> Result<(), TransportError> {
        Ok(())
    }

    fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        Err(TransportError::BluetoothError(
            "Bluetooth transport not yet implemented".into(),
        ))
    }
}

/// Bluetooth discovery service (stub)
pub struct BluetoothDiscovery;

impl BluetoothDiscovery {
    /// Create a new Bluetooth discovery service
    pub fn new() -> Result<Self, TransportError> {
        Err(TransportError::BluetoothError(
            "Bluetooth transport not yet implemented".into(),
        ))
    }

    /// Start scanning for devices
    pub fn start_scan(&self) -> Result<(), TransportError> {
        Err(TransportError::BluetoothError(
            "Bluetooth transport not yet implemented".into(),
        ))
    }

    /// Stop scanning
    pub fn stop_scan(&self) -> Result<(), TransportError> {
        Ok(())
    }

    /// Get discovered devices
    pub fn devices(&self) -> Vec<BluetoothDeviceInfo> {
        Vec::new()
    }

    /// Connect to a device
    pub fn connect(&self, _address: &str) -> Result<Arc<BluetoothTransport>, TransportError> {
        Err(TransportError::BluetoothError(
            "Bluetooth transport not yet implemented".into(),
        ))
    }
}

impl Default for BluetoothDiscovery {
    fn default() -> Self {
        Self
    }
}
