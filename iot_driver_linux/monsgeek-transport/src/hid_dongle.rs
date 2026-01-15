//! HID Dongle transport implementation for 2.4GHz wireless connection
//!
//! The 2.4GHz dongle has a delayed response buffer quirk where GET_FEATURE
//! returns the PREVIOUS response, not the current one. This transport uses
//! a flush pattern (0xFC command) and response caching to handle this.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use hidapi::HidDevice;
use tracing::debug;

use crate::error::TransportError;
use crate::protocol::{self, cmd, timing, REPORT_SIZE};
use crate::types::{ChecksumType, TransportDeviceInfo, VendorEvent};
use crate::Transport;

/// Maximum number of cached responses
const MAX_CACHE_SIZE: usize = 16;

/// Response cache for out-of-order dongle responses
struct ResponseCache {
    entries: VecDeque<(u8, Vec<u8>)>, // (cmd, response data)
}

impl ResponseCache {
    fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_CACHE_SIZE),
        }
    }

    /// Check if we have a cached response for this command
    fn get(&mut self, cmd: u8) -> Option<Vec<u8>> {
        if let Some(pos) = self.entries.iter().position(|(c, _)| *c == cmd) {
            Some(self.entries.remove(pos).unwrap().1)
        } else {
            None
        }
    }

    /// Add a response to the cache
    fn add(&mut self, cmd: u8, data: Vec<u8>) {
        if self.entries.len() >= MAX_CACHE_SIZE {
            self.entries.pop_front();
        }
        self.entries.push_back((cmd, data));
    }
}

/// HID transport for 2.4GHz wireless dongle connection
///
/// This transport handles the dongle's delayed response buffer by:
/// 1. Checking cached responses first (from previous out-of-order reads)
/// 2. Sending a flush command (0xFC) to push out pending responses
/// 3. Caching any mismatched responses for later correlation
pub struct HidDongleTransport {
    /// Feature interface for commands
    device: Mutex<HidDevice>,
    /// Input interface for events (optional)
    input_device: Option<Mutex<HidDevice>>,
    /// Device information
    info: TransportDeviceInfo,
    /// Response cache for out-of-order responses
    cache: Mutex<ResponseCache>,
}

impl HidDongleTransport {
    /// Create a new dongle transport from HID devices
    pub fn new(
        feature_device: HidDevice,
        input_device: Option<HidDevice>,
        info: TransportDeviceInfo,
    ) -> Self {
        Self {
            device: Mutex::new(feature_device),
            input_device: input_device.map(Mutex::new),
            info,
            cache: Mutex::new(ResponseCache::new()),
        }
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

    /// Send the flush command (0xFC) to push out buffered response
    fn send_flush(&self, device: &HidDevice) -> Result<(), TransportError> {
        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0;
        buf[1] = cmd::DONGLE_FLUSH_NOP;
        protocol::apply_checksum(&mut buf[1..], ChecksumType::Bit7);
        debug!("Sending dongle flush (0xFC)");
        device.send_feature_report(&buf)?;
        Ok(())
    }

    /// Read a feature report
    fn read_response(&self, device: &HidDevice) -> Result<Vec<u8>, TransportError> {
        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0;
        device.get_feature_report(&mut buf)?;
        Ok(buf)
    }
}

#[async_trait]
impl Transport for HidDongleTransport {
    async fn send_command(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        let buf = self.build_command(cmd_byte, data, checksum);
        debug!(
            "Dongle sending command 0x{:02X}: {:02X?}",
            cmd_byte,
            &buf[..9]
        );

        let device = self.device.lock().unwrap();
        device.send_feature_report(&buf)?;
        std::thread::sleep(Duration::from_millis(timing::DONGLE_POST_SEND_DELAY_MS));
        Ok(())
    }

    async fn send_command_with_delay(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
        delay_ms: u64,
    ) -> Result<(), TransportError> {
        let buf = self.build_command(cmd_byte, data, checksum);
        let device = self.device.lock().unwrap();
        device.send_feature_report(&buf)?;
        if delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
        Ok(())
    }

    async fn query_command(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        debug!("Dongle querying command 0x{:02X}", cmd_byte);

        // First check if we have a cached response for this command
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(resp) = cache.get(cmd_byte) {
                debug!("Found cached response for 0x{:02X}", cmd_byte);
                return Ok(resp);
            }
        }

        // Send the command
        let buf = self.build_command(cmd_byte, data, checksum);
        let device = self.device.lock().unwrap();

        device.send_feature_report(&buf)?;
        std::thread::sleep(Duration::from_millis(timing::DONGLE_POST_SEND_DELAY_MS));

        // Flush and read with retry pattern
        for attempt in 0..timing::QUERY_RETRIES {
            // Send flush to push response out
            self.send_flush(&device)?;
            std::thread::sleep(Duration::from_millis(if attempt == 0 {
                timing::DONGLE_POST_FLUSH_DELAY_MS
            } else {
                timing::SHORT_DELAY_MS
            }));

            // Read response
            match self.read_response(&device) {
                Ok(resp) => {
                    let resp_cmd = resp[1];
                    debug!(
                        "Dongle read attempt {}: cmd=0x{:02X} (want 0x{:02X})",
                        attempt, resp_cmd, cmd_byte
                    );

                    if resp_cmd == cmd_byte {
                        // Got our response!
                        return Ok(resp[1..].to_vec());
                    } else if resp_cmd != 0 && resp_cmd != cmd::DONGLE_FLUSH_NOP {
                        // Got a valid response for a different command - cache it
                        debug!("Caching out-of-order response for 0x{:02X}", resp_cmd);
                        let mut cache = self.cache.lock().unwrap();
                        cache.add(resp_cmd, resp[1..].to_vec());
                    }
                }
                Err(e) => {
                    debug!("Dongle read attempt {} failed: {}", attempt, e);
                }
            }
        }

        Err(TransportError::Timeout)
    }

    async fn query_raw(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        debug!("Dongle querying raw command 0x{:02X}", cmd_byte);

        // Send the command
        let buf = self.build_command(cmd_byte, data, checksum);
        let device = self.device.lock().unwrap();

        device.send_feature_report(&buf)?;
        std::thread::sleep(Duration::from_millis(timing::DONGLE_POST_SEND_DELAY_MS));

        // Flush and read with retry pattern
        for attempt in 0..timing::QUERY_RETRIES {
            // Send flush to push response out
            self.send_flush(&device)?;
            std::thread::sleep(Duration::from_millis(if attempt == 0 {
                timing::DONGLE_POST_FLUSH_DELAY_MS
            } else {
                timing::SHORT_DELAY_MS
            }));

            // Read response
            match self.read_response(&device) {
                Ok(resp) => {
                    // Accept any non-zero response (no command echo check)
                    // But skip flush responses (0xFC)
                    if resp[1] != 0
                        && resp[1] != cmd::DONGLE_FLUSH_NOP
                        && resp.iter().skip(1).any(|&b| b != 0)
                    {
                        debug!("Dongle got raw response: {:02X?}", &resp[..16]);
                        return Ok(resp[1..].to_vec());
                    }
                    debug!("Dongle empty/flush response on attempt {}", attempt);
                }
                Err(e) => {
                    debug!("Dongle read attempt {} failed: {}", attempt, e);
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
                debug!("Dongle event ({} bytes): {:02X?}", len, &buf[..len.min(16)]);
                Ok(Some(parse_dongle_event(&buf[..len])))
            }
            Ok(_) => Ok(None),
            Err(e) => Err(TransportError::HidError(e.to_string())),
        }
    }

    fn device_info(&self) -> &TransportDeviceInfo {
        &self.info
    }

    async fn is_connected(&self) -> bool {
        let device = self.device.lock().unwrap();
        device.get_product_string().is_ok()
    }

    async fn close(&self) -> Result<(), TransportError> {
        Ok(())
    }
}

/// Parse vendor event from dongle input report
fn parse_dongle_event(data: &[u8]) -> VendorEvent {
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
        0x88 if cmd_data.len() >= 5 => {
            // Battery status from dongle
            VendorEvent::BatteryStatus {
                level: cmd_data[3],
                charging: cmd_data[4] & 0x02 != 0,
                online: cmd_data[4] & 0x01 != 0,
            }
        }
        _ => VendorEvent::Unknown(data.to_vec()),
    }
}
