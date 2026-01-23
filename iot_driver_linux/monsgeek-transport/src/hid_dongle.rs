//! HID Dongle transport implementation for 2.4GHz wireless connection
//!
//! This transport implements polling-based flow control for the dongle's
//! delayed response buffer. Key characteristics:
//! - Flush command (0xFC) required to push responses into readable buffer
//! - Only one command can be in-flight at a time (hardware limitation)
//! - Adaptive timing based on observed latency
//! - Extended timeout for keyboard wake-from-sleep

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use hidapi::HidDevice;
use parking_lot::Mutex;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{debug, warn};

use crate::error::TransportError;
use crate::protocol::{self, cmd, dongle_timing, REPORT_SIZE};
use crate::types::{ChecksumType, TransportDeviceInfo, VendorEvent};
use crate::Transport;

/// Broadcast channel capacity for vendor events
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Maximum number of cached responses
const MAX_CACHE_SIZE: usize = 16;

/// Tracks recent command latencies for adaptive timing
struct LatencyTracker {
    samples: VecDeque<u64>, // Latencies in microseconds
    window_size: usize,
}

impl LatencyTracker {
    fn new(window_size: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    /// Record a successful command latency
    fn record(&mut self, latency_us: u64) {
        if self.samples.len() >= self.window_size {
            self.samples.pop_front();
        }
        self.samples.push_back(latency_us);
    }

    /// Estimate initial wait time based on moving average
    /// Returns ~50% of average latency to start polling early
    fn estimate_initial_wait(&self) -> Duration {
        if self.samples.is_empty() {
            return Duration::from_millis(dongle_timing::INITIAL_WAIT_MS);
        }

        let avg = self.samples.iter().sum::<u64>() / self.samples.len() as u64;
        // Start polling at ~50% of expected latency
        Duration::from_micros(avg / 2)
    }

    /// Get average latency for logging
    fn average_ms(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let avg = self.samples.iter().sum::<u64>() / self.samples.len() as u64;
        avg as f64 / 1000.0
    }
}

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

/// A command request to be processed by the worker
struct CommandRequest {
    cmd: u8,
    data: Vec<u8>,
    checksum: ChecksumType,
    response_tx: oneshot::Sender<Result<Vec<u8>, TransportError>>,
    /// If true, accept any non-zero response (for raw queries)
    raw_mode: bool,
    /// If true, don't wait for any response (fire-and-forget SET commands)
    fire_and_forget: bool,
}

/// Shared state between transport handle and worker
struct DongleState {
    device: Mutex<HidDevice>,
    cache: Mutex<ResponseCache>,
    latency_tracker: Mutex<LatencyTracker>,
    consecutive_timeouts: AtomicUsize,
    wake_mode: AtomicBool,
}

/// HID transport for 2.4GHz wireless dongle connection
///
/// This transport serializes all commands through an async channel,
/// ensuring only one command is in-flight at a time. It uses polling-based
/// flow control with adaptive timing.
pub struct HidDongleTransport {
    /// Shared state
    state: Arc<DongleState>,
    /// Device information
    info: TransportDeviceInfo,
    /// Channel to send command requests to the worker
    request_tx: mpsc::Sender<CommandRequest>,
    /// Flag to track if worker is running
    worker_running: Arc<AtomicBool>,
    /// Broadcast sender for vendor events (if input device available)
    event_tx: Option<broadcast::Sender<VendorEvent>>,
    /// Shutdown flag for event reader thread
    event_shutdown: Arc<AtomicBool>,
}

impl HidDongleTransport {
    /// Create a new dongle transport from HID devices
    ///
    /// This spawns a background worker task that processes commands sequentially.
    pub fn new(
        feature_device: HidDevice,
        input_device: Option<HidDevice>,
        info: TransportDeviceInfo,
    ) -> Self {
        // Create state (input_device goes to event reader thread, not here)
        let state = Arc::new(DongleState {
            device: Mutex::new(feature_device),
            cache: Mutex::new(ResponseCache::new()),
            latency_tracker: Mutex::new(LatencyTracker::new(dongle_timing::LATENCY_WINDOW_SIZE)),
            consecutive_timeouts: AtomicUsize::new(0),
            wake_mode: AtomicBool::new(false),
        });

        let (request_tx, request_rx) = mpsc::channel(dongle_timing::REQUEST_QUEUE_SIZE);
        let worker_running = Arc::new(AtomicBool::new(true));

        // Spawn the command worker as a regular thread (not tokio task)
        // This allows the transport to work without requiring a tokio runtime
        let worker_state = Arc::clone(&state);
        let worker_flag = Arc::clone(&worker_running);
        std::thread::Builder::new()
            .name("dongle-worker".into())
            .spawn(move || {
                // Use futures block_on since we're in a sync thread
                futures::executor::block_on(command_worker(worker_state, request_rx, worker_flag));
            })
            .expect("Failed to spawn dongle worker thread");

        // Spawn event reader thread if input device available
        let event_shutdown = Arc::new(AtomicBool::new(false));
        let event_tx = if let Some(input) = input_device {
            let (tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
            let tx_clone = tx.clone();
            let shutdown_clone = event_shutdown.clone();

            std::thread::Builder::new()
                .name("dongle-event-reader".into())
                .spawn(move || {
                    dongle_event_reader_loop(input, tx_clone, shutdown_clone);
                })
                .expect("Failed to spawn dongle event reader thread");

            Some(tx)
        } else {
            None
        };

        Self {
            state,
            info,
            request_tx,
            worker_running,
            event_tx,
            event_shutdown,
        }
    }

    /// Build command buffer with checksum
    fn build_command(cmd: u8, data: &[u8], checksum: ChecksumType) -> Vec<u8> {
        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0; // Report ID
        buf[1] = cmd;
        let len = std::cmp::min(data.len(), REPORT_SIZE - 2);
        buf[2..2 + len].copy_from_slice(&data[..len]);
        protocol::apply_checksum(&mut buf[1..], checksum);
        buf
    }
}

/// Worker task that processes commands sequentially
async fn command_worker(
    state: Arc<DongleState>,
    mut rx: mpsc::Receiver<CommandRequest>,
    running: Arc<AtomicBool>,
) {
    debug!("Dongle command worker started");

    while let Some(req) = rx.recv().await {
        let result = if req.fire_and_forget {
            // Fire-and-forget: just send command, don't wait for response
            execute_send_only(&state, req.cmd, &req.data, req.checksum)
        } else {
            execute_query(&state, req.cmd, &req.data, req.checksum, req.raw_mode)
        };
        let _ = req.response_tx.send(result);
    }

    running.store(false, Ordering::SeqCst);
    debug!("Dongle command worker stopped");
}

/// Execute a send-only command (no response expected)
/// Used for SET commands which are fire-and-forget
fn execute_send_only(
    state: &DongleState,
    cmd_byte: u8,
    data: &[u8],
    checksum: ChecksumType,
) -> Result<Vec<u8>, TransportError> {
    let device = state.device.lock();

    // Build and send the command
    let buf = HidDongleTransport::build_command(cmd_byte, data, checksum);
    debug!(
        "Dongle sending SET command 0x{:02X} (fire-and-forget)",
        cmd_byte
    );
    device.send_feature_report(&buf)?;

    // Send a flush to ensure command is processed
    send_flush(&device)?;

    // Small delay to let keyboard process the command
    std::thread::sleep(Duration::from_millis(dongle_timing::POLL_CYCLE_MS * 5));

    // Return empty response (no data expected)
    Ok(Vec::new())
}

/// Execute a query using polling-based flow control
fn execute_query(
    state: &DongleState,
    cmd_byte: u8,
    data: &[u8],
    checksum: ChecksumType,
    raw_mode: bool,
) -> Result<Vec<u8>, TransportError> {
    // Check cache first
    {
        let mut cache = state.cache.lock();
        if let Some(resp) = cache.get(cmd_byte) {
            debug!("Found cached response for 0x{:02X}", cmd_byte);
            return Ok(resp);
        }
    }

    let device = state.device.lock();
    let start = Instant::now();

    // Determine timeout based on wake mode
    let timeout = if state.wake_mode.load(Ordering::Relaxed) {
        Duration::from_millis(dongle_timing::WAKE_TIMEOUT_MS)
    } else {
        Duration::from_millis(dongle_timing::QUERY_TIMEOUT_MS)
    };

    // Send the command
    let buf = HidDongleTransport::build_command(cmd_byte, data, checksum);
    debug!("Dongle sending command 0x{:02X}", cmd_byte);
    device.send_feature_report(&buf)?;

    // Get adaptive initial wait
    let initial_wait = state.latency_tracker.lock().estimate_initial_wait();
    std::thread::sleep(initial_wait);

    // Polling loop
    let mut poll_count = 0u32;
    while start.elapsed() < timeout {
        poll_count += 1;

        // Send flush to push response into buffer
        send_flush(&device)?;

        // Read response
        let mut resp = vec![0u8; REPORT_SIZE];
        resp[0] = 0;
        if device.get_feature_report(&mut resp).is_ok() {
            let resp_cmd = resp[1];

            if raw_mode {
                // Raw mode: accept any non-zero, non-flush response
                if resp_cmd != 0
                    && resp_cmd != cmd::DONGLE_FLUSH_NOP
                    && resp.iter().skip(1).any(|&b| b != 0)
                {
                    let latency = start.elapsed();
                    state
                        .latency_tracker
                        .lock()
                        .record(latency.as_micros() as u64);
                    state.consecutive_timeouts.store(0, Ordering::Relaxed);
                    state.wake_mode.store(false, Ordering::Relaxed);
                    debug!(
                        "Dongle raw response 0x{:02X} in {:.2}ms ({} polls)",
                        resp_cmd,
                        latency.as_secs_f64() * 1000.0,
                        poll_count
                    );
                    return Ok(resp[1..].to_vec());
                }
            } else if resp_cmd == cmd_byte {
                // Got our expected response
                let latency = start.elapsed();
                state
                    .latency_tracker
                    .lock()
                    .record(latency.as_micros() as u64);
                state.consecutive_timeouts.store(0, Ordering::Relaxed);
                state.wake_mode.store(false, Ordering::Relaxed);
                debug!(
                    "Dongle response 0x{:02X} in {:.2}ms ({} polls)",
                    cmd_byte,
                    latency.as_secs_f64() * 1000.0,
                    poll_count
                );
                return Ok(resp[1..].to_vec());
            } else if resp_cmd != 0 && resp_cmd != cmd::DONGLE_FLUSH_NOP {
                // Got a valid response for a different command - cache it
                debug!("Caching out-of-order response for 0x{:02X}", resp_cmd);
                state.cache.lock().add(resp_cmd, resp[1..].to_vec());
            }
        }

        // Brief yield to prevent busy-spinning
        std::thread::sleep(Duration::from_millis(dongle_timing::POLL_CYCLE_MS));
    }

    // Timeout handling
    let prev_timeouts = state.consecutive_timeouts.fetch_add(1, Ordering::Relaxed);

    if prev_timeouts == 0 && !state.wake_mode.load(Ordering::Relaxed) {
        // First timeout - might be waking from sleep, enable extended timeout for next attempt
        state.wake_mode.store(true, Ordering::Relaxed);
        warn!(
            "Dongle timeout for 0x{:02X} after {:.0}ms - enabling wake mode",
            cmd_byte,
            start.elapsed().as_secs_f64() * 1000.0
        );
    } else {
        warn!(
            "Dongle timeout for 0x{:02X} after {:.0}ms ({} consecutive)",
            cmd_byte,
            start.elapsed().as_secs_f64() * 1000.0,
            prev_timeouts + 1
        );
    }

    Err(TransportError::Timeout)
}

/// Send the flush command (0xFC) to push out buffered response
fn send_flush(device: &HidDevice) -> Result<(), TransportError> {
    let mut buf = vec![0u8; REPORT_SIZE];
    buf[0] = 0;
    buf[1] = cmd::DONGLE_FLUSH_NOP;
    protocol::apply_checksum(&mut buf[1..], ChecksumType::Bit7);
    device.send_feature_report(&buf)?;
    Ok(())
}

#[async_trait]
impl Transport for HidDongleTransport {
    async fn send_command(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        // SET commands do NOT produce a response - they're fire-and-forget.
        // We serialize through the queue to prevent concurrent commands,
        // but we use a direct execution path that doesn't poll for responses.
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(CommandRequest {
                cmd: cmd_byte,
                data: data.to_vec(),
                checksum,
                response_tx,
                raw_mode: false,
                fire_and_forget: true, // SET commands don't produce responses
            })
            .await
            .map_err(|_| TransportError::Disconnected)?;

        // Wait for worker to process (ensures serialization)
        let _ = response_rx.await;
        Ok(())
    }

    async fn send_command_with_delay(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
        delay_ms: u64,
    ) -> Result<(), TransportError> {
        self.send_command(cmd_byte, data, checksum).await?;
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        Ok(())
    }

    async fn query_command(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(CommandRequest {
                cmd: cmd_byte,
                data: data.to_vec(),
                checksum,
                response_tx,
                raw_mode: false,
                fire_and_forget: false,
            })
            .await
            .map_err(|_| TransportError::Disconnected)?;

        response_rx
            .await
            .map_err(|_| TransportError::Disconnected)?
    }

    async fn query_raw(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(CommandRequest {
                cmd: cmd_byte,
                data: data.to_vec(),
                checksum,
                response_tx,
                raw_mode: true,
                fire_and_forget: false,
            })
            .await
            .map_err(|_| TransportError::Disconnected)?;

        response_rx
            .await
            .map_err(|_| TransportError::Disconnected)?
    }

    async fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        // If we have an event channel, receive from it with timeout
        if let Some(ref tx) = self.event_tx {
            let mut rx = tx.subscribe();
            let timeout = Duration::from_millis(timeout_ms as u64);
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(Ok(event)) => Ok(Some(event)),
                Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                    debug!("Dongle event receiver lagged by {} events", n);
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
        if !self.worker_running.load(Ordering::Relaxed) {
            return false;
        }
        let device = self.state.device.lock();
        device.get_product_string().is_ok()
    }

    async fn close(&self) -> Result<(), TransportError> {
        // Dropping the sender will cause the worker to exit
        Ok(())
    }

    async fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        let device = self.state.device.lock();

        // Send F7 command to trigger battery refresh
        let buf = Self::build_command(cmd::BATTERY_REFRESH, &[], ChecksumType::Bit7);
        device.send_feature_report(&buf)?;

        // Read cached value via feature report 0x05
        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0x05;

        device.get_feature_report(&mut buf)?;

        // Parse battery response:
        // [1] = level (0-100), [3] = idle flag, [4] = online flag
        let level = buf[1];
        let idle = buf.len() > 3 && buf[3] != 0;
        let online = buf.len() > 4 && buf[4] != 0;

        // Sanity check
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
        // Signal event reader thread to shutdown
        self.event_shutdown.store(true, Ordering::SeqCst);

        // Log final latency stats
        let tracker = self.state.latency_tracker.lock();
        if !tracker.samples.is_empty() {
            debug!(
                "Dongle transport closing - avg latency: {:.2}ms",
                tracker.average_ms()
            );
        }
        debug!("HidDongleTransport dropped, signaling event reader shutdown");
    }
}

/// Dedicated event reader loop for dongle running in its own thread
///
/// Reads from the HID input device and broadcasts events to all subscribers.
/// Wakes immediately when data arrives (hidapi read_timeout behavior).
/// The 5ms timeout is only for checking the shutdown flag during idle periods.
fn dongle_event_reader_loop(
    input_device: HidDevice,
    tx: broadcast::Sender<VendorEvent>,
    shutdown: Arc<AtomicBool>,
) {
    debug!("Dongle event reader thread started");
    let mut buf = [0u8; 64];

    while !shutdown.load(Ordering::Relaxed) {
        // Read with short timeout - wakes immediately on data
        // Timeout only affects how often we check shutdown flag when idle
        match input_device.read_timeout(&mut buf, 5) {
            Ok(len) if len > 0 => {
                debug!(
                    "Dongle event reader got {} bytes: {:02X?}",
                    len,
                    &buf[..len.min(16)]
                );
                let event = parse_dongle_event(&buf[..len]);
                // Send to all subscribers (ignores if no receivers)
                let _ = tx.send(event);
            }
            Ok(_) => {
                // Timeout, no data - loop continues to check shutdown
            }
            Err(e) => {
                // Log error but keep trying (device might recover)
                warn!("Dongle event reader error: {}", e);
                // Brief sleep to avoid spinning on persistent errors
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    debug!("Dongle event reader thread exiting");
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
        0x09 => VendorEvent::BacklightToggle,
        0x11 => VendorEvent::DialModeToggle,
        _ => VendorEvent::UnknownKbFunc { category, action },
    }
}

/// Parse vendor event from dongle input report (EP2 notifications)
///
/// The dongle forwards the same notification format as wired connection.
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
/// - 0x88: Battery status (dongle-specific)
fn parse_dongle_event(data: &[u8]) -> VendorEvent {
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

        // Settings ACK (magnetism start/stop shares this type)
        0x0F => {
            // Magnetism control uses specific byte patterns
            if payload.len() >= 3 {
                if payload[1] == 0x01 && payload[2] == 0x00 {
                    return VendorEvent::MagnetismStart;
                } else if payload[1] == 0x00 && payload[2] == 0x00 {
                    return VendorEvent::MagnetismStop;
                }
            }
            // Generic settings ack
            VendorEvent::SettingsAck {
                started: value != 0,
            }
        }

        // Key depth report (magnetism)
        0x1B if payload.len() >= 5 => {
            let depth_raw = u16::from_le_bytes([payload[1], payload[2]]);
            let key_index = payload[3];
            VendorEvent::KeyDepth {
                key_index,
                depth_raw,
            }
        }

        // Battery status from dongle (dongle-specific notification)
        0x88 if payload.len() >= 5 => VendorEvent::BatteryStatus {
            level: payload[3],
            charging: payload[4] & 0x02 != 0,
            online: payload[4] & 0x01 != 0,
        },

        _ => VendorEvent::Unknown(data.to_vec()),
    }
}
