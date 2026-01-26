//! HID Wired transport implementation for direct USB connection

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use hidapi::HidDevice;
use tokio::sync::broadcast;
use tracing::debug;

use crate::error::TransportError;
use crate::event_parser::{parse_usb_event, run_event_reader_loop, EventReaderConfig};
use crate::protocol::{self, timing, REPORT_SIZE};
use crate::types::{ChecksumType, TransportDeviceInfo, VendorEvent};
use crate::Transport;

/// Broadcast channel capacity for vendor events
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// HID transport for wired USB connection
///
/// This transport provides direct communication with keyboards connected
/// via USB cable. It uses standard HID feature reports for commands.
pub struct HidWiredTransport {
    /// Feature interface for commands
    feature_device: Mutex<HidDevice>,
    /// Device information
    info: TransportDeviceInfo,
    /// Delay after commands (ms)
    command_delay_ms: u64,
    /// Broadcast sender for vendor events (if input device available)
    event_tx: Option<broadcast::Sender<VendorEvent>>,
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
            command_delay_ms: timing::DEFAULT_DELAY_MS,
            event_tx,
            shutdown,
        }
    }

    /// Set delay after commands (default 100ms)
    pub fn set_command_delay(&mut self, ms: u64) {
        self.command_delay_ms = ms;
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
        let buf = protocol::build_command(cmd, data, checksum);
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
        let buf = protocol::build_command(cmd, data, checksum);
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
        let buf = protocol::build_command(cmd, data, checksum);
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
        let buf = protocol::build_command(cmd, data, checksum);
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
        // If we have an event channel, receive from it with timeout
        if let Some(ref tx) = self.event_tx {
            let mut rx = tx.subscribe();
            let timeout = Duration::from_millis(timeout_ms as u64);
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(Ok(event)) => Ok(Some(event)),
                Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                    debug!("Event receiver lagged by {} events", n);
                    // Try again immediately after lag
                    match rx.recv().await {
                        Ok(event) => Ok(Some(event)),
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

    fn subscribe_events(&self) -> Option<broadcast::Receiver<VendorEvent>> {
        self.event_tx.as_ref().map(|tx| tx.subscribe())
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

    async fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        // Wired connection has no battery - return "always full"
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
