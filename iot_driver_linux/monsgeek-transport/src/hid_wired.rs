//! HID Wired transport implementation for direct USB connection

use hidapi::HidDevice;
use parking_lot::Mutex;
use tokio::sync::broadcast;
use tracing::debug;

use crate::error::TransportError;
use crate::event_parser::{parse_usb_event, EventReaderConfig, EventSubsystem};
use crate::protocol::{self, REPORT_SIZE};
use crate::types::{ChecksumType, TimestampedEvent, TransportDeviceInfo, VendorEvent};
use crate::Transport;

/// HID transport for wired USB connection
///
/// This transport provides direct communication with keyboards connected
/// via USB cable. It uses standard HID feature reports for commands.
///
/// Raw I/O only — flow control (retries, echo matching) lives in
/// `FlowControlTransport`.
pub struct HidWiredTransport {
    /// Feature interface for commands
    feature_device: Mutex<HidDevice>,
    /// Device information
    info: TransportDeviceInfo,
    /// Shared event subsystem (reader thread, broadcast, shutdown)
    events: EventSubsystem,
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
        let events = EventSubsystem::new(input_device, parse_usb_event, EventReaderConfig::usb());

        Self {
            feature_device: Mutex::new(feature_device),
            info,
            events,
        }
    }

    /// Read feature report
    fn read_response(&self) -> Result<Vec<u8>, TransportError> {
        let device = self.feature_device.lock();
        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0;
        device.get_feature_report(&mut buf)?;
        Ok(buf)
    }
}

impl Transport for HidWiredTransport {
    fn send_report(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        let buf = protocol::build_command(cmd, data, checksum);
        let device = self.feature_device.lock();
        device.send_feature_report(&buf)?;
        Ok(())
    }

    fn read_report(&self) -> Result<Vec<u8>, TransportError> {
        let resp = self.read_response()?;
        Ok(resp[1..].to_vec())
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
        let device = self.feature_device.lock();
        device.get_product_string().is_ok()
    }

    fn close(&self) -> Result<(), TransportError> {
        Ok(())
    }

    fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        Ok((100, true, false))
    }
}

impl Drop for HidWiredTransport {
    fn drop(&mut self) {
        debug!("HidWiredTransport dropped, signaling event reader shutdown");
    }
}
