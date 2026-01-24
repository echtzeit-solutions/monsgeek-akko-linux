//! HID Wired transport implementation for direct USB connection

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use hidapi::HidDevice;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::error::TransportError;
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
                    event_reader_loop(input, tx_clone, shutdown_clone);
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

/// Dedicated event reader loop running in its own thread
///
/// Reads from the HID input device and broadcasts events to all subscribers.
/// Wakes immediately when data arrives (hidapi read_timeout behavior).
/// The 5ms timeout is only for checking the shutdown flag during idle periods.
fn event_reader_loop(
    input_device: HidDevice,
    tx: broadcast::Sender<VendorEvent>,
    shutdown: Arc<AtomicBool>,
) {
    debug!("HID event reader thread started");
    let mut buf = [0u8; 64];

    while !shutdown.load(Ordering::Relaxed) {
        // Read with short timeout - wakes immediately on data
        // Timeout only affects how often we check shutdown flag when idle
        match input_device.read_timeout(&mut buf, 5) {
            Ok(len) if len > 0 => {
                debug!(
                    "Event reader got {} bytes: {:02X?}",
                    len,
                    &buf[..len.min(16)]
                );
                let event = parse_vendor_event(&buf[..len]);
                // Send to all subscribers (ignores if no receivers)
                let _ = tx.send(event);
            }
            Ok(_) => {
                // Timeout, no data - loop continues to check shutdown
            }
            Err(e) => {
                // Log error but keep trying (device might recover)
                warn!("HID event reader error: {}", e);
                // Brief sleep to avoid spinning on persistent errors
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    debug!("HID event reader thread exiting");
}

/// Parse keyboard function notification (type 0x03)
fn parse_kb_func(payload: &[u8]) -> VendorEvent {
    let category = payload.get(1).copied().unwrap_or(0);
    let action = payload.get(2).copied().unwrap_or(0);

    match action {
        0x01 => VendorEvent::WinLockToggle {
            locked: category != 0,
        },
        0x03 => VendorEvent::WasdSwapToggle {
            swapped: category == 8, // category 8 = swapped, category 0 = normal
        },
        0x08 => VendorEvent::FnLayerToggle {
            layer: category, // 0 = default, 1 = alternate
        },
        0x09 => VendorEvent::BacklightToggle,
        0x11 => VendorEvent::DialModeToggle,
        _ => VendorEvent::UnknownKbFunc { category, action },
    }
}

/// Parse vendor event from input report data (EP2 notifications)
///
/// Report format (Report ID 0x05):
/// - Byte 0: Notification type
/// - Byte 1+: Notification data
///
/// Notification types:
/// - 0x00: Wake (all zeros)
/// - 0x01: Profile changed (data = profile 0-3)
/// - 0x03: KB function (category, action)
/// - 0x04: LED effect mode (effect ID 1-20)
/// - 0x05: LED effect speed (0-4)
/// - 0x06: Brightness level (0-4)
/// - 0x07: LED color (0-7)
/// - 0x0F: Settings ACK (0=done, 1=start)
/// - 0x1B: Key depth (magnetism report)
/// - 0x88: Battery status (dongle)
fn parse_vendor_event(data: &[u8]) -> VendorEvent {
    if data.is_empty() {
        return VendorEvent::Unknown(data.to_vec());
    }

    // Skip report ID if present (0x05)
    let payload = if data[0] == 0x05 && data.len() > 1 {
        &data[1..]
    } else {
        data
    };

    if payload.is_empty() {
        return VendorEvent::Unknown(data.to_vec());
    }

    let notif_type = payload[0];
    let value = payload.get(1).copied().unwrap_or(0);

    match notif_type {
        // Wake notification: type 0x00 with all zeros
        0x00 if payload.iter().skip(1).all(|&b| b == 0) => VendorEvent::Wake,

        // Profile changed via Fn+F9..F12
        0x01 => VendorEvent::ProfileChange { profile: value },

        // Keyboard function notification (Win lock, WASD swap, etc.)
        0x03 => parse_kb_func(payload),

        // LED effect mode changed via Fn+Home/PgUp/End/PgDn
        0x04 => VendorEvent::LedEffectMode { effect_id: value },

        // LED effect speed changed via Fn+←/→
        0x05 => VendorEvent::LedEffectSpeed { speed: value },

        // Brightness level changed via Fn+↑/↓
        0x06 => VendorEvent::BrightnessLevel { level: value },

        // LED color changed via Fn+\
        0x07 => VendorEvent::LedColor { color: value },

        // Settings ACK: value=1 means change started, value=0 means change complete
        0x0F => VendorEvent::SettingsAck {
            started: value != 0,
        },

        // Key depth report (magnetism)
        0x1B if payload.len() >= 5 => {
            let depth_raw = u16::from_le_bytes([payload[1], payload[2]]);
            let key_index = payload[3];
            VendorEvent::KeyDepth {
                key_index,
                depth_raw,
            }
        }

        // Battery status (from dongle)
        0x88 if payload.len() >= 5 => VendorEvent::BatteryStatus {
            level: payload[3],
            charging: payload[4] & 0x02 != 0,
            online: payload[4] & 0x01 != 0,
        },

        _ => VendorEvent::Unknown(data.to_vec()),
    }
}
