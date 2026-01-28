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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hidapi::HidDevice;
use parking_lot::Mutex;
use tokio::sync::broadcast;
use tracing::{debug, trace, warn};

use crate::error::TransportError;
use crate::event_parser::{parse_ble_event, run_event_reader_loop, EventReaderConfig};
use crate::protocol::{self, ble, timing};
use crate::types::{ChecksumType, TimestampedEvent, TransportDeviceInfo, VendorEvent};
use crate::Transport;

/// Broadcast channel capacity for vendor events
const EVENT_CHANNEL_CAPACITY: usize = 256;

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
pub struct HidBluetoothTransport {
    /// Vendor device for commands (usage 0x0202, page 0xFF55)
    vendor_device: Mutex<HidDevice>,
    /// Device information
    info: TransportDeviceInfo,
    /// Delay after commands (ms)
    command_delay_ms: u64,
    /// Broadcast sender for timestamped vendor events
    event_tx: Option<broadcast::Sender<TimestampedEvent>>,
    /// Shutdown flag for event reader thread
    shutdown: Arc<AtomicBool>,
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
        let shutdown = Arc::new(AtomicBool::new(false));
        let event_tx = if let Some(input) = input_device {
            // Create broadcast channel for events
            let (tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
            let tx_clone = tx.clone();
            let shutdown_clone = shutdown.clone();

            // Spawn dedicated event reader thread
            std::thread::Builder::new()
                .name("bt-event-reader".into())
                .spawn(move || {
                    run_event_reader_loop(
                        input,
                        tx_clone,
                        shutdown_clone,
                        parse_ble_event,
                        EventReaderConfig::bluetooth(),
                    );
                })
                .expect("Failed to spawn Bluetooth event reader thread");

            Some(tx)
        } else {
            None
        };

        Self {
            vendor_device: Mutex::new(vendor_device),
            info,
            command_delay_ms: ble::DEFAULT_DELAY_MS,
            event_tx,
            shutdown,
        }
    }

    /// Set delay after commands (default 150ms for BLE)
    pub fn set_command_delay(&mut self, ms: u64) {
        self.command_delay_ms = ms;
    }

    /// Send output report and wait
    fn send_and_wait(&self, buf: &[u8]) -> Result<(), TransportError> {
        let device = self.vendor_device.lock();
        device.write(buf)?;
        std::thread::sleep(Duration::from_millis(self.command_delay_ms));
        Ok(())
    }

    /// Read vendor response (0x55 framed) for a specific command.
    ///
    /// We may receive other notifications (0x66 events) while waiting; those are ignored here.
    fn read_response(&self, expected_cmd: u8, timeout_ms: u32) -> Result<Vec<u8>, TransportError> {
        let device = self.vendor_device.lock();
        let mut buf = vec![0u8; ble::REPORT_SIZE];
        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms as u64);

        while std::time::Instant::now() < deadline {
            match device.read_timeout(&mut buf, 50) {
                Ok(0) => continue, // Timeout, try again
                Ok(n) => {
                    debug!("BLE read {} bytes: {:02X?}", n, &buf[..n.min(16)]);

                    // We expect: [0x06][0x55][cmd]...
                    if n >= 3
                        && buf[0] == ble::VENDOR_REPORT_ID
                        && buf[1] == ble::CMDRESP_MARKER
                        && buf[2] == expected_cmd
                    {
                        // Prefer non-empty data, but also accept a normal USB-style status OK (0xAA)
                        // at position 3 (since buf[2]=cmd).
                        let window_end = n.min(ble::REPORT_SIZE);
                        let has_data = buf[3..window_end].iter().any(|&b| b != 0);
                        let has_ok = buf.get(3).copied() == Some(0xAA);
                        if has_data || has_ok {
                            return Ok(buf[..window_end].to_vec());
                        }
                        debug!("Got 0x55-framed empty response, waiting...");
                        continue;
                    }

                    // Ignore 0x66-framed events while waiting for response
                    if n >= 2 && buf[0] == ble::VENDOR_REPORT_ID && buf[1] == ble::EVENT_MARKER {
                        continue;
                    }
                }
                Err(e) => {
                    warn!("BLE read error: {}", e);
                    return Err(TransportError::from(e));
                }
            }
        }

        Err(TransportError::Timeout)
    }
}

#[async_trait]
impl Transport for HidBluetoothTransport {
    async fn send_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        let buf = protocol::build_ble_command(cmd, data, checksum);
        debug!("BLE sending command 0x{:02X}: {:02X?}", cmd, &buf[..10]);

        for attempt in 0..timing::SEND_RETRIES {
            match self.send_and_wait(&buf) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    debug!("BLE send attempt {} failed: {}", attempt, e);
                    if attempt == timing::SEND_RETRIES - 1 {
                        return Err(e);
                    }
                    std::thread::sleep(Duration::from_millis(timing::SHORT_DELAY_MS));
                }
            }
        }
        Ok(())
    }

    async fn send_command_with_delay(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
        delay_ms: u64,
    ) -> Result<(), TransportError> {
        let buf = protocol::build_ble_command(cmd, data, checksum);
        let device = self.vendor_device.lock();
        device.write(&buf)?;
        if delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
        Ok(())
    }

    async fn query_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        let buf = protocol::build_ble_command(cmd, data, checksum);
        debug!("BLE querying command 0x{:02X}: {:02X?}", cmd, &buf[..10]);

        for attempt in 0..timing::QUERY_RETRIES {
            {
                let device = self.vendor_device.lock();
                if device.write(&buf).is_err() {
                    continue;
                }
            }

            // Wait a bit for keyboard to process
            std::thread::sleep(Duration::from_millis(50));

            match self.read_response(cmd, 500) {
                Ok(resp) => {
                    debug!(
                        "BLE got response for 0x{:02X}: {:02X?}",
                        cmd,
                        &resp[..10.min(resp.len())]
                    );
                    // Return USB-style payload (strip report id + BLE marker)
                    // [0]=report id, [1]=0x55, [2]=cmd -> return [cmd..] like other transports
                    if resp.len() >= 3
                        && resp[0] == ble::VENDOR_REPORT_ID
                        && resp[1] == ble::CMDRESP_MARKER
                    {
                        return Ok(resp[2..].to_vec());
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    debug!("BLE read attempt {} failed: {}", attempt, e);
                }
            }
        }

        warn!(
            "BLE query 0x{:02X} timed out - BLE may not support this query",
            cmd
        );
        Err(TransportError::Timeout)
    }

    async fn query_raw(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        let buf = protocol::build_ble_command(cmd, data, checksum);
        debug!(
            "BLE querying raw command 0x{:02X}: {:02X?}",
            cmd,
            &buf[..10]
        );

        for attempt in 0..timing::QUERY_RETRIES {
            {
                let device = self.vendor_device.lock();
                if device.write(&buf).is_err() {
                    continue;
                }
            }

            std::thread::sleep(Duration::from_millis(50));

            let device = self.vendor_device.lock();
            let mut resp = vec![0u8; ble::REPORT_SIZE];

            match device.read_timeout(&mut resp, 500) {
                Ok(n) if n > 0 => {
                    // Prefer 0x55-framed responses; otherwise accept any non-zero blob
                    if resp.len() >= 2
                        && resp[0] == ble::VENDOR_REPORT_ID
                        && resp[1] == ble::CMDRESP_MARKER
                    {
                        debug!(
                            "BLE got raw 0x55 response: {:02X?}",
                            &resp[..16.min(resp.len())]
                        );
                        return Ok(resp[2..].to_vec()); // strip report id + 0x55
                    }
                    if resp.iter().any(|&b| b != 0) {
                        debug!("BLE got raw response: {:02X?}", &resp[..16.min(resp.len())]);
                        return Ok(resp);
                    }
                }
                Ok(_) => {
                    debug!("BLE empty response on attempt {}", attempt);
                }
                Err(e) => {
                    debug!("BLE read attempt {} failed: {}", attempt, e);
                }
            }
        }

        Err(TransportError::Timeout)
    }

    async fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        if let Some(ref tx) = self.event_tx {
            let mut rx = tx.subscribe();
            let timeout = Duration::from_millis(timeout_ms as u64);
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(Ok(timestamped)) => Ok(Some(timestamped.event)),
                Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                    debug!("BLE event receiver lagged by {} events", n);
                    match rx.recv().await {
                        Ok(timestamped) => Ok(Some(timestamped.event)),
                        Err(_) => Ok(None),
                    }
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => Ok(None),
                Err(_) => Ok(None), // Timeout
            }
        } else {
            Ok(None)
        }
    }

    fn subscribe_events(&self) -> Option<broadcast::Receiver<TimestampedEvent>> {
        self.event_tx.as_ref().map(|tx| tx.subscribe())
    }

    fn device_info(&self) -> &TransportDeviceInfo {
        &self.info
    }

    async fn is_connected(&self) -> bool {
        let device = self.vendor_device.lock();
        device.get_product_string().is_ok()
    }

    async fn close(&self) -> Result<(), TransportError> {
        Ok(())
    }

    async fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        // For Bluetooth, query via BlueZ Battery1 interface
        // The keyboard sends battery level as BLE notifications on handle 0x0e
        // which BlueZ exposes via org.bluez.Battery1

        // Extract MAC address from serial (format: "F4:EE:25:AF:3A:38" or similar)
        if let Some(ref serial) = self.info.serial {
            if let Some(level) = query_bluez_battery(serial) {
                debug!("BLE battery from BlueZ: {}%", level);
                return Ok((level, true, false));
            }
        }

        // Fallback: try to find by product name
        if let Some(ref name) = self.info.product_name {
            if let Some(level) = query_bluez_battery_by_name(name) {
                debug!("BLE battery from BlueZ (by name): {}%", level);
                return Ok((level, true, false));
            }
        }

        // Battery query failed - device might be disconnected or BlueZ doesn't have it
        trace!("Could not get BLE battery from BlueZ");
        Ok((0, true, false)) // Level 0 indicates unknown
    }
}

impl Drop for HidBluetoothTransport {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
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
