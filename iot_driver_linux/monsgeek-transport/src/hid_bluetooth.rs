//! HID Bluetooth transport implementation for BLE HID connection
//!
//! This transport communicates with MonsGeek keyboards connected via Bluetooth
//! using the kernel's hid-over-gatt (HOGP) driver, which exposes the device as
//! a standard hidraw device.
//!
//! # Protocol Limitations
//!
//! **IMPORTANT:** The Bluetooth protocol has significant limitations compared to USB:
//!
//! - **GET/SET commands require BLE framing**: The Bluetooth transport uses a
//!   different framing than USB feature reports (see below).
//! - **Events DO work**: Fn key notifications are sent with format `[06, 66, type, value]`
//! - **Battery via BLE**: Use standard Battery Service (0x180F) via BlueZ D-Bus
//!
//! With correct framing, the vendor protocol can work over Bluetooth (Jan 2026).
//!
//! # Technical Investigation (Jan 2026)
//!
//! The Bluetooth HID GATT structure:
//! - **char0032**: Report ID 6 Input (vendor responses, 65 bytes, read/notify)
//! - **char0039**: Report ID 6 Output (vendor commands, 65 bytes, write)
//!
//! Verified behavior:
//! - Vendor commands are sent as ATT Write Command to the vendor Output report.
//! - Vendor responses arrive as notifications on the vendor Input report.
//!
//! # BLE Vendor Framing
//!
//! Over BLE, the device wraps the USB-style vendor payload with an extra leading marker:
//!
//! - **Command/Response**: `[report_id=0x06] [0x55] [cmd] [data...] [checksum...]`
//! - **Event**:            `[report_id=0x06] [0x66] [type] [value...]`
//!
//! The checksum calculation is the same as USB (Bit7/Bit8), but it applies to the slice
//! starting at `[cmd]` (i.e. skipping the 0x55 marker).
//!
//! Note on Windows "BT-over-USB" captures: the leading `0x02/0x04` bytes seen
//! in USBPcap payloads are **HCI packet types/headers**, not HID report IDs.
//!
//! # Key differences from wired USB transport:
//! - Uses Report ID 6 for vendor endpoint (per report descriptor)
//! - Uses hidapi write()/read() instead of feature reports
//! - Event format: `[06, 66, type, value]` vs USB's `[05, type, value]`
//! - Battery status via BLE Battery Service (bluetoothctl or D-Bus)

use std::process::Command;
use std::time::{Duration, Instant};

use hidapi::HidDevice;
use parking_lot::Mutex;
use tokio::sync::broadcast;
use tracing::{debug, trace};

use crate::error::TransportError;
use crate::event_parser::{parse_ble_event, EventReaderConfig, EventSubsystem};
use crate::protocol::{self, ble};
use crate::types::{ChecksumType, TimestampedEvent, TransportDeviceInfo, VendorEvent};
use crate::Transport;

/// HID transport for Bluetooth Low Energy connection
///
/// This transport provides communication with keyboards connected via Bluetooth.
/// The kernel's hid-over-gatt driver exposes BLE HID devices as hidraw, allowing
/// us to use the same hidapi interface with some protocol adjustments.
///
/// # Protocol Differences
///
/// Unlike USB HID which uses feature reports, Bluetooth HID uses output/input
/// reports with a specific Report ID (6) for vendor commands. The write()
/// method sends output reports and read() receives input reports.
/// Raw I/O only — flow control (retries, echo matching) lives in
/// `FlowControlTransport`.
pub struct HidBluetoothTransport {
    /// Vendor device for commands (usage 0x0202, page 0xFF55)
    vendor_device: Mutex<HidDevice>,
    /// Device information
    info: TransportDeviceInfo,
    /// Shared event subsystem (reader thread, broadcast, shutdown)
    events: EventSubsystem,
}

impl HidBluetoothTransport {
    /// Create a new Bluetooth transport from HID device
    ///
    /// # Arguments
    /// * `vendor_device` - HID device for vendor reports (usage 0x0202)
    /// * `input_device` - Optional HID device for input reports (keyboard events)
    /// * `info` - Device information
    pub fn new(
        vendor_device: HidDevice,
        input_device: Option<HidDevice>,
        info: TransportDeviceInfo,
    ) -> Self {
        let events = EventSubsystem::new(
            input_device,
            parse_ble_event,
            EventReaderConfig::bluetooth(),
        );

        Self {
            vendor_device: Mutex::new(vendor_device),
            info,
            events,
        }
    }
}

impl Transport for HidBluetoothTransport {
    fn send_report(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        let buf = protocol::build_ble_command(cmd, data, checksum);
        let device = self.vendor_device.lock();
        device.write(&buf)?;
        Ok(())
    }

    fn read_report(&self) -> Result<Vec<u8>, TransportError> {
        let device = self.vendor_device.lock();
        let mut buf = vec![0u8; ble::REPORT_SIZE];
        let deadline = Instant::now() + Duration::from_millis(500);

        while Instant::now() < deadline {
            match device.read_timeout(&mut buf, 50) {
                Ok(0) => continue,
                Ok(n) => {
                    // Return 0x55-framed responses stripped to [cmd..] format
                    if n >= 3 && buf[0] == ble::VENDOR_REPORT_ID && buf[1] == ble::CMDRESP_MARKER {
                        return Ok(buf[2..n.min(ble::REPORT_SIZE)].to_vec());
                    }
                    // Skip 0x66-framed events
                    if n >= 2 && buf[0] == ble::VENDOR_REPORT_ID && buf[1] == ble::EVENT_MARKER {
                        continue;
                    }
                    // Return anything else
                    return Ok(buf[..n].to_vec());
                }
                Err(e) => return Err(TransportError::from(e)),
            }
        }
        Err(TransportError::Timeout)
    }

    // send_flush: uses default no-op

    fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        self.events.read_event(timeout_ms)
    }

    fn subscribe_events(&self) -> Option<broadcast::Receiver<TimestampedEvent>> {
        self.events.subscribe()
    }

    fn device_info(&self) -> &TransportDeviceInfo {
        &self.info
    }

    fn is_connected(&self) -> bool {
        let device = self.vendor_device.lock();
        device.get_product_string().is_ok()
    }

    fn close(&self) -> Result<(), TransportError> {
        Ok(())
    }

    fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        if let Some(ref serial) = self.info.serial {
            if let Some(level) = query_bluez_battery(serial) {
                debug!("BLE battery from BlueZ: {}%", level);
                return Ok((level, true, false));
            }
        }

        if let Some(ref name) = self.info.product_name {
            if let Some(level) = query_bluez_battery_by_name(name) {
                debug!("BLE battery from BlueZ (by name): {}%", level);
                return Ok((level, true, false));
            }
        }

        trace!("Could not get BLE battery from BlueZ");
        Ok((0, true, false))
    }
}

impl Drop for HidBluetoothTransport {
    fn drop(&mut self) {
        debug!("HidBluetoothTransport dropped, signaling event reader shutdown");
    }
}

/// Query BlueZ for battery level by MAC address
///
/// Uses `bluetoothctl info <mac>` to get the Battery Percentage.
fn query_bluez_battery(mac_or_serial: &str) -> Option<u8> {
    // Try to extract/normalize MAC address
    // Serial might be MAC with colons, or some other format
    let mac = if mac_or_serial.contains(':') && mac_or_serial.len() >= 17 {
        // Already looks like a MAC address
        mac_or_serial.to_string()
    } else {
        // Try to find the device by listing all and matching
        return None;
    };

    let output = Command::new("bluetoothctl")
        .args(["info", &mac])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Look for "Battery Percentage: 0x5e (94)"
    for line in stdout.lines() {
        if line.contains("Battery Percentage:") {
            // Parse the decimal value in parentheses
            if let Some(start) = line.rfind('(') {
                if let Some(end) = line.rfind(')') {
                    if let Ok(level) = line[start + 1..end].parse::<u8>() {
                        return Some(level.min(100));
                    }
                }
            }
            // Try parsing hex value after "0x"
            if let Some(hex_start) = line.find("0x") {
                let hex_str = &line[hex_start + 2..];
                let hex_end = hex_str
                    .find(|c: char| !c.is_ascii_hexdigit())
                    .unwrap_or(hex_str.len());
                if let Ok(level) = u8::from_str_radix(&hex_str[..hex_end], 16) {
                    return Some(level.min(100));
                }
            }
        }
    }
    None
}

/// Query BlueZ for battery level by device name
///
/// Lists all paired devices and finds one matching the name.
fn query_bluez_battery_by_name(name: &str) -> Option<u8> {
    // List all paired devices
    let output = Command::new("bluetoothctl")
        .args(["devices"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Format: "Device F4:EE:25:AF:3A:38 M1 V5 HE BT1"
    for line in stdout.lines() {
        if line.contains(name) || (name.contains("M1") && line.contains("M1")) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[0] == "Device" {
                let mac = parts[1];
                if let Some(level) = query_bluez_battery(mac) {
                    return Some(level);
                }
            }
        }
    }
    None
}
