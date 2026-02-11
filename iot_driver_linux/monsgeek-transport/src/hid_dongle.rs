//! HID Dongle transport implementation for 2.4GHz wireless connection
//!
//! Raw I/O only — the dongle requires a flush command (0xFC) to push
//! responses into the readable buffer.  Flow control (polling loop,
//! adaptive timing, response caching, serialization) lives in
//! `FlowControlTransport`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hidapi::HidDevice;
use parking_lot::Mutex;
use tokio::sync::broadcast;
use tracing::debug;

use crate::error::TransportError;
use crate::event_parser::{parse_usb_event, run_event_reader_loop, EventReaderConfig};
use crate::protocol::{self, cmd, REPORT_SIZE};
use crate::types::{ChecksumType, TimestampedEvent, TransportDeviceInfo, VendorEvent};
use crate::Transport;

/// Broadcast channel capacity for vendor events
const EVENT_CHANNEL_CAPACITY: usize = 256;

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
    /// Broadcast sender for timestamped vendor events (if input device available)
    event_tx: Option<broadcast::Sender<TimestampedEvent>>,
    /// Shutdown flag for event reader thread
    event_shutdown: Arc<AtomicBool>,
}

impl HidDongleTransport {
    /// Create a new dongle transport from HID devices
    pub fn new(
        feature_device: HidDevice,
        input_device: Option<HidDevice>,
        info: TransportDeviceInfo,
    ) -> Self {
        // Spawn event reader thread if input device available
        let event_shutdown = Arc::new(AtomicBool::new(false));
        let event_tx = if let Some(input) = input_device {
            let (tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
            let tx_clone = tx.clone();
            let shutdown_clone = event_shutdown.clone();

            std::thread::Builder::new()
                .name("dongle-event-reader".into())
                .spawn(move || {
                    run_event_reader_loop(
                        input,
                        tx_clone,
                        shutdown_clone,
                        parse_usb_event,
                        EventReaderConfig::dongle(),
                    );
                })
                .expect("Failed to spawn dongle event reader thread");

            Some(tx)
        } else {
            None
        };

        Self {
            device: Mutex::new(feature_device),
            info,
            event_tx,
            event_shutdown,
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
        buf[1] = cmd::DONGLE_FLUSH_NOP;
        protocol::apply_checksum(&mut buf[1..], ChecksumType::Bit7);
        device.send_feature_report(&buf)?;
        Ok(())
    }

    fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        if let Some(ref tx) = self.event_tx {
            let mut rx = tx.subscribe();
            let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
            loop {
                match rx.try_recv() {
                    Ok(timestamped) => return Ok(Some(timestamped.event)),
                    Err(broadcast::error::TryRecvError::Empty) => {
                        if Instant::now() >= deadline {
                            return Ok(None);
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                    Err(broadcast::error::TryRecvError::Closed) => return Ok(None),
                }
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

    fn is_connected(&self) -> bool {
        let device = self.device.lock();
        device.get_product_string().is_ok()
    }

    fn close(&self) -> Result<(), TransportError> {
        Ok(())
    }

    fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        let device = self.device.lock();

        let buf = protocol::build_command(cmd::BATTERY_REFRESH, &[], ChecksumType::Bit7);
        device.send_feature_report(&buf)?;

        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0x05;
        device.get_feature_report(&mut buf)?;

        let level = buf[1];
        let idle = buf.len() > 3 && buf[3] != 0;
        let online = buf.len() > 4 && buf[4] != 0;

        if level > 100 {
            return Err(TransportError::Internal(format!(
                "Invalid battery level: {level}"
            )));
        }

        Ok((level, online, idle))
    }
}

impl Drop for HidDongleTransport {
    fn drop(&mut self) {
        self.event_shutdown.store(true, Ordering::SeqCst);
        debug!("HidDongleTransport dropped, signaling event reader shutdown");
    }
}
