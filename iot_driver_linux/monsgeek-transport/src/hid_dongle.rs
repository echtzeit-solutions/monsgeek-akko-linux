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
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::error::TransportError;
use crate::protocol::{self, cmd, dongle_timing, REPORT_SIZE};
use crate::types::{ChecksumType, TransportDeviceInfo, VendorEvent};
use crate::Transport;

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
    input_device: Option<Mutex<HidDevice>>,
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
        let state = Arc::new(DongleState {
            device: Mutex::new(feature_device),
            input_device: input_device.map(Mutex::new),
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

        Self {
            state,
            info,
            request_tx,
            worker_running,
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
        let input = match &self.state.input_device {
            Some(dev) => dev,
            None => return Ok(None),
        };

        let device = input.lock();
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
}

impl Drop for HidDongleTransport {
    fn drop(&mut self) {
        // Log final latency stats
        let tracker = self.state.latency_tracker.lock();
        if !tracker.samples.is_empty() {
            debug!(
                "Dongle transport closing - avg latency: {:.2}ms",
                tracker.average_ms()
            );
        }
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
