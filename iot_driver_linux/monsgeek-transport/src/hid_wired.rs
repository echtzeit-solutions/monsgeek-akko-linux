//! HID Wired transport implementation for direct USB connection

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hidapi::HidDevice;
use tokio::sync::broadcast;
use tracing::debug;

use crate::error::TransportError;
use crate::event_parser::{parse_usb_event, run_event_reader_loop, EventReaderConfig};
use crate::protocol::{self, REPORT_SIZE};
use crate::types::{ChecksumType, TimestampedEvent, TransportDeviceInfo, VendorEvent};
use crate::Transport;

/// Broadcast channel capacity for vendor events
const EVENT_CHANNEL_CAPACITY: usize = 256;

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
    /// Broadcast sender for timestamped vendor events (if input device available)
    event_tx: Option<broadcast::Sender<TimestampedEvent>>,
    /// Shutdown flag for event reader thread
    shutdown: Arc<AtomicBool>,
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
        let shutdown = Arc::new(AtomicBool::new(false));
        let event_tx = if let Some(input) = input_device {
            // Create broadcast channel for events
            let (tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
            let tx_clone = tx.clone();
            let shutdown_clone = shutdown.clone();

            // Spawn dedicated event reader thread
            std::thread::Builder::new()
                .name("hid-event-reader".into())
                .spawn(move || {
                    run_event_reader_loop(
                        input,
                        tx_clone,
                        shutdown_clone,
                        parse_usb_event,
                        EventReaderConfig::usb(),
                    );
                })
                .expect("Failed to spawn HID event reader thread");

            Some(tx)
        } else {
            None
        };

        Self {
            feature_device: Mutex::new(feature_device),
            info,
            event_tx,
            shutdown,
        }
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

impl Transport for HidWiredTransport {
    fn send_report(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        let buf = protocol::build_command(cmd, data, checksum);
        let device = self.feature_device.lock().unwrap();
        device.send_feature_report(&buf)?;
        Ok(())
    }

    fn read_report(&self) -> Result<Vec<u8>, TransportError> {
        let resp = self.read_response()?;
        Ok(resp[1..].to_vec())
    }

    // send_flush: uses default no-op

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
        let device = self.feature_device.lock().unwrap();
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
        // Signal shutdown to event reader thread
        self.shutdown.store(true, Ordering::SeqCst);
        debug!("HidWiredTransport dropped, signaling event reader shutdown");
    }
}
