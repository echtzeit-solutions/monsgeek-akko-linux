//! HID Wired transport implementation for direct USB connection

use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use hidapi::HidDevice;
use tracing::debug;

use crate::error::TransportError;
use crate::protocol::{self, timing, REPORT_SIZE};
use crate::types::{ChecksumType, TransportDeviceInfo, VendorEvent};
use crate::Transport;

/// HID transport for wired USB connection
///
/// This transport provides direct communication with keyboards connected
/// via USB cable. It uses standard HID feature reports for commands.
pub struct HidWiredTransport {
    /// Feature interface for commands
    feature_device: Mutex<HidDevice>,
    /// Input interface for events (optional)
    input_device: Option<Mutex<HidDevice>>,
    /// Device information
    info: TransportDeviceInfo,
    /// Delay after commands (ms)
    command_delay_ms: u64,
}

impl HidWiredTransport {
    /// Create a new wired transport from HID devices
    ///
    /// # Arguments
    /// * `feature_device` - HID device for feature reports (commands)
    /// * `input_device` - Optional HID device for input reports (events)
    /// * `info` - Device information
    pub fn new(
        feature_device: HidDevice,
        input_device: Option<HidDevice>,
        info: TransportDeviceInfo,
    ) -> Self {
        Self {
            feature_device: Mutex::new(feature_device),
            input_device: input_device.map(Mutex::new),
            info,
            command_delay_ms: timing::DEFAULT_DELAY_MS,
        }
    }

    /// Set delay after commands (default 100ms)
    pub fn set_command_delay(&mut self, ms: u64) {
        self.command_delay_ms = ms;
    }

    /// Build command buffer with checksum
    fn build_command(&self, cmd: u8, data: &[u8], checksum: ChecksumType) -> Vec<u8> {
        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0; // Report ID
        buf[1] = cmd;
        let len = std::cmp::min(data.len(), REPORT_SIZE - 2);
        buf[2..2 + len].copy_from_slice(&data[..len]);
        protocol::apply_checksum(&mut buf[1..], checksum);
        buf
    }

    /// Send feature report and wait
    fn send_and_wait(&self, buf: &[u8]) -> Result<(), TransportError> {
        let device = self.feature_device.lock().unwrap();
        device.send_feature_report(buf)?;
        std::thread::sleep(Duration::from_millis(self.command_delay_ms));
        Ok(())
    }

    /// Read feature report
    fn read_response(&self) -> Result<Vec<u8>, TransportError> {
        let device = self.feature_device.lock().unwrap();
        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0;
        device.get_feature_report(&mut buf)?;
        Ok(buf)
    }
}

#[async_trait]
impl Transport for HidWiredTransport {
    async fn send_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        let buf = self.build_command(cmd, data, checksum);
        debug!("Sending command 0x{:02X}: {:02X?}", cmd, &buf[..9]);

        for attempt in 0..timing::SEND_RETRIES {
            match self.send_and_wait(&buf) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    debug!("Send attempt {} failed: {}", attempt, e);
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
        let buf = self.build_command(cmd, data, checksum);
        let device = self.feature_device.lock().unwrap();
        device.send_feature_report(&buf)?;
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
        let buf = self.build_command(cmd, data, checksum);
        debug!("Querying command 0x{:02X}: {:02X?}", cmd, &buf[..9]);

        for attempt in 0..timing::QUERY_RETRIES {
            if self.send_and_wait(&buf).is_err() {
                continue;
            }

            match self.read_response() {
                Ok(resp) => {
                    if resp[1] == cmd {
                        debug!("Got response for 0x{:02X}: {:02X?}", cmd, &resp[..9]);
                        return Ok(resp[1..].to_vec());
                    }
                    debug!(
                        "Response mismatch: expected 0x{:02X}, got 0x{:02X}",
                        cmd, resp[1]
                    );
                }
                Err(e) => {
                    debug!("Read attempt {} failed: {}", attempt, e);
                }
            }
        }

        Err(TransportError::Timeout)
    }

    async fn query_raw(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        let buf = self.build_command(cmd, data, checksum);
        debug!("Querying raw command 0x{:02X}: {:02X?}", cmd, &buf[..9]);

        for attempt in 0..timing::QUERY_RETRIES {
            if self.send_and_wait(&buf).is_err() {
                continue;
            }

            match self.read_response() {
                Ok(resp) => {
                    // Accept any non-zero response (no command echo check)
                    if resp.iter().skip(1).any(|&b| b != 0) {
                        debug!("Got raw response: {:02X?}", &resp[..16]);
                        return Ok(resp[1..].to_vec());
                    }
                    debug!("Empty response on attempt {}", attempt);
                }
                Err(e) => {
                    debug!("Read attempt {} failed: {}", attempt, e);
                }
            }
        }

        Err(TransportError::Timeout)
    }

    async fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        let input = match &self.input_device {
            Some(dev) => dev,
            None => return Ok(None),
        };

        let device = input.lock().unwrap();
        let mut buf = vec![0u8; 64];

        match device.read_timeout(&mut buf, timeout_ms as i32) {
            Ok(len) if len > 0 => {
                debug!("Read event ({} bytes): {:02X?}", len, &buf[..len.min(16)]);
                Ok(Some(parse_vendor_event(&buf[..len])))
            }
            Ok(_) => Ok(None), // Timeout
            Err(e) => Err(TransportError::HidError(e.to_string())),
        }
    }

    fn device_info(&self) -> &TransportDeviceInfo {
        &self.info
    }

    async fn is_connected(&self) -> bool {
        // Try to read device info to check connection
        let device = self.feature_device.lock().unwrap();
        device.get_product_string().is_ok()
    }

    async fn close(&self) -> Result<(), TransportError> {
        // HidDevice drops automatically
        Ok(())
    }
}

/// Parse vendor event from input report data
fn parse_vendor_event(data: &[u8]) -> VendorEvent {
    if data.is_empty() {
        return VendorEvent::Unknown(data.to_vec());
    }

    // Skip report ID if present
    let cmd_data = if data[0] == 0x05 && data.len() > 1 {
        &data[1..]
    } else {
        data
    };

    if cmd_data.is_empty() {
        return VendorEvent::Unknown(data.to_vec());
    }

    match cmd_data[0] {
        0x1B if cmd_data.len() >= 5 => {
            // Key depth report
            let depth_raw = u16::from_le_bytes([cmd_data[1], cmd_data[2]]);
            let key_index = cmd_data[3];
            VendorEvent::KeyDepth {
                key_index,
                depth_raw,
            }
        }
        0x0F if cmd_data.len() >= 3 => {
            // Magnetism control
            if cmd_data[1] == 0x01 && cmd_data[2] == 0x00 {
                VendorEvent::MagnetismStart
            } else if cmd_data[1] == 0x00 && cmd_data[2] == 0x00 {
                VendorEvent::MagnetismStop
            } else {
                VendorEvent::Unknown(data.to_vec())
            }
        }
        0x01 if cmd_data.len() >= 2 => {
            // Profile change
            VendorEvent::ProfileChange {
                profile: cmd_data[1],
            }
        }
        0x04..=0x07 if cmd_data.len() >= 2 => {
            // LED change
            VendorEvent::LedChange { mode: cmd_data[1] }
        }
        0x88 if cmd_data.len() >= 5 => {
            // Battery status (from dongle)
            VendorEvent::BatteryStatus {
                level: cmd_data[3],
                charging: cmd_data[4] & 0x02 != 0,
                online: cmd_data[4] & 0x01 != 0,
            }
        }
        _ => VendorEvent::Unknown(data.to_vec()),
    }
}
