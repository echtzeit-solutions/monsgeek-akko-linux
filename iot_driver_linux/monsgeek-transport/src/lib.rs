//! Transport abstraction layer for MonsGeek/Akko keyboard communication
//!
//! This crate provides a unified interface for communicating with MonsGeek/Akko
//! keyboards across different transport backends:
//!
//! - HID Wired (direct USB connection)
//! - HID Dongle (2.4GHz wireless via USB dongle)
//! - HID Bluetooth (BLE via kernel's hid-over-gatt driver)
//! - WebRTC (future, for remote access)

pub mod command;
pub mod device_registry;
pub mod error;
pub mod event_parser;
pub mod printer;
pub mod protocol;
pub mod types;

mod discovery;
mod hid_bluetooth;
mod hid_dongle;
mod hid_wired;
mod sync_adapter;

#[cfg(feature = "bluetooth")]
pub mod bluetooth;

#[cfg(feature = "webrtc")]
pub mod webrtc;

pub use command::{
    // Packet parsing
    decode_magnetism_data,
    try_parse_command,
    try_parse_response,
    // Battery
    BatteryRefresh,
    BatteryResponse,
    DebounceResponse,
    HidCommand,
    HidResponse,
    LedMode,
    LedParamsResponse,
    // Magnetism data decoding
    MagnetismData,
    ParseError,
    // Packet dispatchers for pcap analysis
    ParsedCommand,
    ParsedResponse,
    PollingRate,
    PollingRateResponse,
    ProfileResponse,
    QueryDebounce,
    // Queries
    QueryLedParams,
    QueryPollingRate,
    QueryProfile,
    QuerySleepTime,
    QueryVersion,
    Rgb,
    SetDebounce,
    // LED
    SetLedParams,
    // Magnetism
    SetMagnetismReport,
    SetPollingRate,
    // Settings
    SetProfile,
    SetSleepTime,
    SleepTimeResponse,
    TransportExt,
};
pub use device_registry::{
    is_bluetooth_pid, is_dongle_pid, BLUETOOTH_PIDS, DONGLE_PIDS, VENDOR_ID,
};
pub use error::TransportError;
pub use printer::{
    DecodedPacket, OutputFormat, PacketFilter, Printer, PrinterConfig, PrinterTransport,
};
pub use types::{
    ChecksumType, DiscoveredDevice, DiscoveryEvent, TimestampedEvent, TransportDeviceInfo,
    TransportType, VendorEvent,
};

pub use discovery::{DeviceDiscovery, HidDiscovery, ProbedDevice};
pub use hid_bluetooth::HidBluetoothTransport;
pub use hid_dongle::HidDongleTransport;
pub use hid_wired::HidWiredTransport;
pub use sync_adapter::{list_devices_sync, open_device_sync, SyncTransport};

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::broadcast;

/// The core transport trait - all backends implement this
///
/// This trait provides a unified interface for sending commands to keyboards
/// and receiving responses/events, regardless of the underlying transport.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a command without expecting a specific response
    ///
    /// # Arguments
    /// * `cmd` - Command byte (e.g., `protocol::cmd::SET_LEDPARAM`)
    /// * `data` - Command data (without command byte)
    /// * `checksum` - Checksum type to apply
    async fn send_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError>;

    /// Send a command with custom delay (for streaming/fast updates)
    ///
    /// # Arguments
    /// * `cmd` - Command byte
    /// * `data` - Command data
    /// * `checksum` - Checksum type
    /// * `delay_ms` - Delay after send in milliseconds
    async fn send_command_with_delay(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
        delay_ms: u64,
    ) -> Result<(), TransportError>;

    /// Send a command and wait for its response
    ///
    /// This handles transport-specific response correlation (e.g., dongle caching).
    ///
    /// # Arguments
    /// * `cmd` - Command byte
    /// * `data` - Command data
    /// * `checksum` - Checksum type
    ///
    /// # Returns
    /// Response data (64 bytes, excluding report ID)
    async fn query_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError>;

    /// Send a command and wait for any non-empty response (no command echo check)
    ///
    /// Used for commands like magnetism queries where the response doesn't echo
    /// the command byte.
    ///
    /// # Arguments
    /// * `cmd` - Command byte
    /// * `data` - Command data
    /// * `checksum` - Checksum type
    ///
    /// # Returns
    /// Raw response data (64 bytes, excluding report ID)
    async fn query_raw(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError>;

    /// Read vendor events (key depth, battery status, etc.)
    ///
    /// # Arguments
    /// * `timeout_ms` - Timeout in milliseconds (0 for non-blocking)
    ///
    /// # Returns
    /// `None` on timeout, `Some(event)` if data received
    async fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError>;

    /// Get device information
    fn device_info(&self) -> &TransportDeviceInfo;

    /// Check if transport is still connected
    async fn is_connected(&self) -> bool;

    /// Close the transport gracefully
    async fn close(&self) -> Result<(), TransportError>;

    /// Get battery status (dongle-specific: sends F7 refresh, reads report 0x05)
    ///
    /// Returns (level, online, idle) tuple.
    /// For wired connections, returns (100, true, false) as there's no battery.
    async fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError>;

    /// Subscribe to vendor events via broadcast channel
    ///
    /// Returns a receiver for asynchronous vendor event notifications.
    /// Events are pushed from a dedicated reader thread with ~5ms latency.
    /// Each event includes a timestamp (seconds since transport opened).
    /// Returns None if the transport doesn't support event subscriptions
    /// (e.g., no input endpoint available).
    fn subscribe_events(&self) -> Option<broadcast::Receiver<TimestampedEvent>> {
        None // Default: not supported
    }
}

/// Type alias for a boxed transport
pub type BoxedTransport = Arc<dyn Transport>;
