//! HID Dongle transport implementation for 2.4GHz wireless connection
//!
//! Raw I/O only — the dongle requires a flush command (0xFC) to push
//! responses into the readable buffer.  Flow control (polling loop,
//! adaptive timing, response caching, serialization) lives in
//! `FlowControlTransport`.

use hidapi::HidDevice;
use parking_lot::Mutex;
use tokio::sync::broadcast;
use tracing::debug;

use crate::error::TransportError;
use crate::event_parser::{parse_usb_event, EventReaderConfig, EventSubsystem};
use crate::protocol::{self, cmd, REPORT_SIZE};
use crate::types::{
    ChecksumType, DongleStatus, TimestampedEvent, TransportDeviceInfo, VendorEvent,
};
use crate::Transport;

/// HID transport for 2.4GHz wireless dongle connection
///
/// Raw I/O only — provides `send_report`, `read_report`, and `send_flush`.
/// Flow control (polling, retries, caching, serialization) lives in
/// `FlowControlTransport`.
pub struct HidDongleTransport {
    /// Feature device for HID reports
    device: Mutex<HidDevice>,
    /// Device information
    info: TransportDeviceInfo,
    /// Shared event subsystem (reader thread, broadcast, shutdown)
    events: EventSubsystem,
}

impl HidDongleTransport {
    /// Create a new dongle transport from HID devices
    pub fn new(
        feature_device: HidDevice,
        input_device: Option<HidDevice>,
        info: TransportDeviceInfo,
    ) -> Self {
        let events =
            EventSubsystem::new(input_device, parse_usb_event, EventReaderConfig::dongle());

        Self {
            device: Mutex::new(feature_device),
            info,
            events,
        }
    }
}

impl Transport for HidDongleTransport {
    fn send_report(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        let buf = protocol::build_command(cmd_byte, data, checksum);
        let device = self.device.lock();
        device.send_feature_report(&buf)?;
        Ok(())
    }

    fn read_report(&self) -> Result<Vec<u8>, TransportError> {
        let device = self.device.lock();
        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0;
        device.get_feature_report(&mut buf)?;
        Ok(buf[1..].to_vec())
    }

    fn send_flush(&self) -> Result<(), TransportError> {
        let device = self.device.lock();
        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0;
        buf[1] = cmd::GET_CACHED_RESPONSE;
        protocol::apply_checksum(&mut buf[1..], ChecksumType::Bit7);
        device.send_feature_report(&buf)?;
        Ok(())
    }

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
        let device = self.device.lock();
        device.get_product_string().is_ok()
    }

    fn close(&self) -> Result<(), TransportError> {
        Ok(())
    }

    fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        let status = self
            .query_dongle_status()?
            .ok_or_else(|| TransportError::Internal("No dongle status".into()))?;
        Ok((status.battery_level, status.rf_ready, status.charging))
    }

    fn query_dongle_status(&self) -> Result<Option<DongleStatus>, TransportError> {
        let device = self.device.lock();

        // Send GET_DONGLE_STATUS (0xF7) — handled locally by dongle, not forwarded
        let buf = protocol::build_command(cmd::GET_DONGLE_STATUS, &[], ChecksumType::Bit7);
        device.send_feature_report(&buf)?;

        // Read response on Report ID 0 (dongle IF2 only has Report ID 0)
        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0; // Report ID 0
        device.get_feature_report(&mut buf)?;

        // F7 response layout (buf[0]=Report ID, buf[1..]=data):
        //   buf[1] = has_response
        //   buf[2] = kb_battery_info (0-100%)
        //   buf[3] = 0 (reserved)
        //   buf[4] = kb_charging
        //   buf[5] = 1 (hardcoded)
        //   buf[6] = rf_ready (0=waiting, 1=ready)
        let level = buf[2];
        if level > 100 {
            return Err(TransportError::Internal(format!(
                "Invalid battery level: {level}"
            )));
        }

        Ok(Some(DongleStatus {
            has_response: buf[1] != 0,
            rf_ready: buf[6] != 0,
            battery_level: level,
            charging: buf[4] != 0,
        }))
    }
}

impl Drop for HidDongleTransport {
    fn drop(&mut self) {
        debug!("HidDongleTransport dropped, signaling event reader shutdown");
    }
}
