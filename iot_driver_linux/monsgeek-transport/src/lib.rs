//! Transport abstraction layer for MonsGeek/Akko keyboard communication
//!
//! This crate provides a unified interface for communicating with MonsGeek/Akko
//! keyboards across different transport backends:
//!
//! - HID Wired (direct USB connection)
//! - HID Dongle (2.4GHz wireless via USB dongle)
//! - HID Bluetooth (BLE via kernel's hid-over-gatt driver)

pub mod command;
pub mod device_registry;
pub mod error;
pub mod event_parser;
pub mod flow_control;
pub mod printer;
pub mod protocol;
pub mod types;

mod discovery;
mod hid_bluetooth;
mod hid_dongle;
mod hid_wired;
mod sync_adapter;

pub use command::{
    // Packet parsing
    decode_magnetism_data,
    // Speed conversion helpers
    speed_from_wire,
    speed_to_wire,
    try_parse_command,
    try_parse_response,
    DebounceResponse,
    // Dongle status
    DongleStatusQuery,
    DongleStatusResponse,
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
    // LED constants
    BRIGHTNESS_MAX,
    DAZZLE_OFF,
    DAZZLE_ON,
    SPEED_MAX,
};
pub use device_registry::{
    is_bluetooth_pid, is_dongle_pid, BLUETOOTH_PIDS, DONGLE_PIDS, VENDOR_ID,
};
pub use error::TransportError;
pub use printer::{
    DecodedPacket, OutputFormat, PacketFilter, Printer, PrinterConfig, PrinterTransport,
};
pub use protocol::{KeyRef, Layer};
pub use types::{
    ChecksumType, DiscoveredDevice, DiscoveryEvent, DongleStatus, TimestampedEvent,
    TransportDeviceInfo, TransportType, VendorEvent,
};

pub use discovery::{DeviceDiscovery, HidDiscovery, ProbedDevice};
pub use flow_control::FlowControlTransport;
pub use hid_bluetooth::HidBluetoothTransport;
pub use hid_dongle::HidDongleTransport;
pub use hid_wired::HidWiredTransport;
pub use sync_adapter::{list_devices_sync, open_device_sync};

use std::sync::Arc;
use tokio::sync::broadcast;

/// The core transport trait — raw I/O only.
///
/// All methods are synchronous (blocking).  The underlying HID backends
/// (hidapi) are inherently blocking, so the trait reflects that honestly.
/// Flow control (retries, echo matching, dongle polling) lives in
/// `FlowControlTransport`.
pub trait Transport: Send + Sync {
    // ---- Raw I/O (used by FlowControlTransport and gRPC) ----

    /// Frame cmd/data/checksum into a HID report and send it.
    /// No delay, no retry.
    fn send_report(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError>;

    /// Read one HID report.  Returns 64 bytes (report ID stripped).
    /// WARNING: On Linux wired, bare GET_FEATURE without prior SET_FEATURE hangs.
    fn read_report(&self) -> Result<Vec<u8>, TransportError>;

    /// Send GET_CACHED_RESPONSE (0xFC) to push dongle response buffer. No-op on wired/BLE.
    fn send_flush(&self) -> Result<(), TransportError> {
        Ok(()) // default: no-op
    }

    // ---- Housekeeping ----

    /// Read vendor events (key depth, battery status, etc.)
    /// Blocks up to `timeout_ms` milliseconds.
    fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError>;

    /// Get device information
    fn device_info(&self) -> &TransportDeviceInfo;

    /// Check if transport is still connected
    fn is_connected(&self) -> bool;

    /// Close the transport gracefully
    fn close(&self) -> Result<(), TransportError>;

    /// Get battery status
    fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError>;

    /// Query dongle status (F7). Returns None on non-dongle transports.
    fn query_dongle_status(&self) -> Result<Option<DongleStatus>, TransportError> {
        Ok(None)
    }

    /// Subscribe to vendor events via broadcast channel
    fn subscribe_events(&self) -> Option<broadcast::Receiver<TimestampedEvent>> {
        None
    }
}

/// Type alias for a boxed transport
pub type BoxedTransport = Arc<dyn Transport>;
