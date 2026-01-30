//! Flow-control transport layer
//!
//! `FlowControlTransport` wraps a raw `Transport` (which only does send/read
//! of individual HID reports) and adds query semantics: retries, echo matching,
//! command delay, and dongle-specific polling with response caching.
//!
//! ```text
//! [HidWired / HidBluetooth / HidDongle]  ← implements Transport (raw I/O)
//!                |
//!       [FlowControlTransport]            ← adds retries, echo matching, polling
//!                |
//!      [KeyboardInterface / TUI]
//! ```

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{debug, warn};

use crate::error::TransportError;
use crate::protocol::{cmd, dongle_timing, timing};
use crate::types::{
    ChecksumType, TimestampedEvent, TransportDeviceInfo, TransportType, VendorEvent,
};
use crate::Transport;

// Re-export for consumers
pub use crate::command::{HidCommand, HidResponse, ParseError};

/// Maximum number of cached responses (dongle)
const MAX_CACHE_SIZE: usize = 16;

// ============================================================================
// FlowControlTransport
// ============================================================================

/// A concrete transport wrapper that adds flow control (retries, echo matching,
/// dongle polling) on top of a raw `Transport`.
///
/// Consumers that need query semantics (TUI, KeyboardInterface) hold this.
/// The gRPC layer uses the raw `Transport` directly.
pub struct FlowControlTransport {
    inner: Arc<dyn Transport>,
    flow: FlowState,
}

enum FlowState {
    /// Wired / BLE: simple send → delay → read → check echo.
    /// The `query_lock` serializes command-response cycles — without it,
    /// concurrent tasks interleave their sends/reads and get echo mismatches.
    Simple {
        command_delay_ms: u64,
        query_lock: tokio::sync::Mutex<()>,
    },
    /// Dongle: serialized worker, adaptive timing, response cache
    Dongle {
        request_tx: mpsc::Sender<CommandRequest>,
        _worker_running: Arc<AtomicBool>,
        state: Arc<DongleSharedState>,
    },
}

// ---- Dongle internals (moved from hid_dongle.rs) ----

/// Shared state between transport handle and dongle worker
struct DongleSharedState {
    cache: Mutex<ResponseCache>,
    latency_tracker: Mutex<LatencyTracker>,
    consecutive_timeouts: AtomicUsize,
    wake_mode: AtomicBool,
}

struct LatencyTracker {
    samples: VecDeque<u64>,
    window_size: usize,
}

impl LatencyTracker {
    fn new(window_size: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    fn record(&mut self, latency_us: u64) {
        if self.samples.len() >= self.window_size {
            self.samples.pop_front();
        }
        self.samples.push_back(latency_us);
    }

    fn estimate_initial_wait(&self) -> Duration {
        if self.samples.is_empty() {
            return Duration::from_millis(dongle_timing::INITIAL_WAIT_MS);
        }
        let avg = self.samples.iter().sum::<u64>() / self.samples.len() as u64;
        Duration::from_micros(avg / 2)
    }

    fn average_ms(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let avg = self.samples.iter().sum::<u64>() / self.samples.len() as u64;
        avg as f64 / 1000.0
    }
}

struct ResponseCache {
    entries: VecDeque<(u8, Vec<u8>)>,
}

impl ResponseCache {
    fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_CACHE_SIZE),
        }
    }

    fn get(&mut self, cmd: u8) -> Option<Vec<u8>> {
        if let Some(pos) = self.entries.iter().position(|(c, _)| *c == cmd) {
            Some(self.entries.remove(pos).unwrap().1)
        } else {
            None
        }
    }

    fn add(&mut self, cmd: u8, data: Vec<u8>) {
        if self.entries.len() >= MAX_CACHE_SIZE {
            self.entries.pop_front();
        }
        self.entries.push_back((cmd, data));
    }
}

struct CommandRequest {
    cmd: u8,
    data: Vec<u8>,
    checksum: ChecksumType,
    response_tx: oneshot::Sender<Result<Vec<u8>, TransportError>>,
    raw_mode: bool,
    fire_and_forget: bool,
}

// ============================================================================
// Constructor
// ============================================================================

impl FlowControlTransport {
    /// Create a new flow-control wrapper.
    ///
    /// Auto-detects transport type from `device_info().transport_type` and
    /// configures the appropriate flow strategy.
    pub fn new(inner: Arc<dyn Transport>) -> Self {
        let flow = match inner.device_info().transport_type {
            TransportType::HidDongle => {
                let state = Arc::new(DongleSharedState {
                    cache: Mutex::new(ResponseCache::new()),
                    latency_tracker: Mutex::new(LatencyTracker::new(
                        dongle_timing::LATENCY_WINDOW_SIZE,
                    )),
                    consecutive_timeouts: AtomicUsize::new(0),
                    wake_mode: AtomicBool::new(false),
                });

                let (request_tx, request_rx) = mpsc::channel(dongle_timing::REQUEST_QUEUE_SIZE);
                let worker_running = Arc::new(AtomicBool::new(true));

                let worker_inner = Arc::clone(&inner);
                let worker_state = Arc::clone(&state);
                let worker_flag = Arc::clone(&worker_running);
                std::thread::Builder::new()
                    .name("flow-dongle-worker".into())
                    .spawn(move || {
                        futures::executor::block_on(dongle_command_worker(
                            worker_inner,
                            worker_state,
                            request_rx,
                            worker_flag,
                        ));
                    })
                    .expect("Failed to spawn dongle flow-control worker");

                FlowState::Dongle {
                    request_tx,
                    _worker_running: worker_running,
                    state,
                }
            }
            TransportType::Bluetooth => FlowState::Simple {
                command_delay_ms: 150,
                query_lock: tokio::sync::Mutex::new(()),
            },
            _ => FlowState::Simple {
                command_delay_ms: timing::DEFAULT_DELAY_MS,
                query_lock: tokio::sync::Mutex::new(()),
            },
        };

        Self { inner, flow }
    }

    /// Access the wrapped raw transport.
    pub fn inner(&self) -> &Arc<dyn Transport> {
        &self.inner
    }

    // ========================================================================
    // Query methods (flow-controlled)
    // ========================================================================

    /// Send command and wait for echoed response (validates cmd byte match).
    pub async fn query_command(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        match &self.flow {
            FlowState::Simple {
                command_delay_ms,
                query_lock,
            } => {
                let _guard = query_lock.lock().await;
                self.simple_query(cmd_byte, data, checksum, *command_delay_ms, false)
                    .await
            }
            FlowState::Dongle { request_tx, .. } => {
                self.dongle_dispatch(request_tx, cmd_byte, data, checksum, false, false)
                    .await
            }
        }
    }

    /// Send command and wait for any non-empty response (no echo check).
    pub async fn query_raw(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        match &self.flow {
            FlowState::Simple {
                command_delay_ms,
                query_lock,
            } => {
                let _guard = query_lock.lock().await;
                self.simple_query(cmd_byte, data, checksum, *command_delay_ms, true)
                    .await
            }
            FlowState::Dongle { request_tx, .. } => {
                self.dongle_dispatch(request_tx, cmd_byte, data, checksum, true, false)
                    .await
            }
        }
    }

    /// Fire-and-forget command with default delay.
    pub async fn send_command(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        match &self.flow {
            FlowState::Simple {
                command_delay_ms,
                query_lock,
            } => {
                let _guard = query_lock.lock().await;
                self.inner.send_report(cmd_byte, data, checksum).await?;
                if *command_delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(*command_delay_ms)).await;
                }
                Ok(())
            }
            FlowState::Dongle { request_tx, .. } => {
                self.dongle_dispatch(request_tx, cmd_byte, data, checksum, false, true)
                    .await?;
                Ok(())
            }
        }
    }

    /// Fire-and-forget command with custom delay.
    pub async fn send_command_with_delay(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
        delay_ms: u64,
    ) -> Result<(), TransportError> {
        match &self.flow {
            FlowState::Simple { query_lock, .. } => {
                let _guard = query_lock.lock().await;
                self.inner.send_report(cmd_byte, data, checksum).await?;
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                Ok(())
            }
            FlowState::Dongle { request_tx, .. } => {
                self.dongle_dispatch(request_tx, cmd_byte, data, checksum, false, true)
                    .await?;
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                Ok(())
            }
        }
    }

    // ---- Simple flow ----

    async fn simple_query(
        &self,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
        delay_ms: u64,
        raw_mode: bool,
    ) -> Result<Vec<u8>, TransportError> {
        for attempt in 0..timing::QUERY_RETRIES {
            if self
                .inner
                .send_report(cmd_byte, data, checksum)
                .await
                .is_err()
            {
                debug!("Send attempt {} failed for 0x{:02X}", attempt, cmd_byte);
                continue;
            }

            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            match self.inner.read_report().await {
                Ok(resp) => {
                    if raw_mode {
                        return Ok(resp);
                    }
                    if !resp.is_empty() && resp[0] == cmd_byte {
                        return Ok(resp);
                    }
                    debug!(
                        "Response mismatch: expected 0x{:02X}, got 0x{:02X}",
                        cmd_byte,
                        resp.first().copied().unwrap_or(0)
                    );
                }
                Err(e) => {
                    debug!("Read attempt {} failed: {}", attempt, e);
                }
            }
        }

        Err(TransportError::Timeout)
    }

    // ---- Dongle dispatch ----

    async fn dongle_dispatch(
        &self,
        request_tx: &mpsc::Sender<CommandRequest>,
        cmd_byte: u8,
        data: &[u8],
        checksum: ChecksumType,
        raw_mode: bool,
        fire_and_forget: bool,
    ) -> Result<Vec<u8>, TransportError> {
        let (response_tx, response_rx) = oneshot::channel();

        request_tx
            .send(CommandRequest {
                cmd: cmd_byte,
                data: data.to_vec(),
                checksum,
                response_tx,
                raw_mode,
                fire_and_forget,
            })
            .await
            .map_err(|_| TransportError::Disconnected)?;

        response_rx
            .await
            .map_err(|_| TransportError::Disconnected)?
    }
}

// ============================================================================
// Transport delegation (so FlowControlTransport can be used as dyn Transport)
// ============================================================================

#[async_trait]
impl Transport for FlowControlTransport {
    // ---- Raw I/O: delegate to inner ----

    async fn send_report(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        self.inner.send_report(cmd, data, checksum).await
    }

    async fn read_report(&self) -> Result<Vec<u8>, TransportError> {
        self.inner.read_report().await
    }

    async fn send_flush(&self) -> Result<(), TransportError> {
        self.inner.send_flush().await
    }

    // ---- Housekeeping: delegate to inner ----

    async fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        self.inner.read_event(timeout_ms).await
    }

    fn subscribe_events(&self) -> Option<broadcast::Receiver<TimestampedEvent>> {
        self.inner.subscribe_events()
    }

    fn device_info(&self) -> &TransportDeviceInfo {
        self.inner.device_info()
    }

    async fn is_connected(&self) -> bool {
        self.inner.is_connected().await
    }

    async fn close(&self) -> Result<(), TransportError> {
        self.inner.close().await
    }

    async fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        self.inner.get_battery_status().await
    }
}

// ============================================================================
// TransportExt on FlowControlTransport
// ============================================================================

/// Extension trait for sending typed commands via FlowControlTransport
impl FlowControlTransport {
    /// Send a typed command (fire-and-forget)
    pub async fn send<C: HidCommand + Send + Sync>(&self, cmd: &C) -> Result<(), TransportError> {
        self.send_command(C::CMD, &cmd.to_data(), C::CHECKSUM).await
    }

    /// Query and parse a typed response (validates command echo)
    pub async fn query<C, R>(&self, cmd: &C) -> Result<R, TransportError>
    where
        C: HidCommand + Send + Sync,
        R: HidResponse,
    {
        let resp = self
            .query_command(C::CMD, &cmd.to_data(), C::CHECKSUM)
            .await?;
        R::parse(&resp).map_err(|e| match e {
            ParseError::CommandMismatch { expected, got } => TransportError::InvalidResponse {
                expected,
                actual: got,
            },
            _ => TransportError::Internal(e.to_string()),
        })
    }

    /// Query without command echo validation (for special responses)
    pub async fn query_no_echo<C, R>(&self, cmd: &C) -> Result<R, TransportError>
    where
        C: HidCommand + Send + Sync,
        R: HidResponse,
    {
        let resp = self.query_raw(C::CMD, &cmd.to_data(), C::CHECKSUM).await?;
        R::parse(&resp).map_err(|e| TransportError::Internal(e.to_string()))
    }
}

// ============================================================================
// Dongle worker
// ============================================================================

async fn dongle_command_worker(
    inner: Arc<dyn Transport>,
    state: Arc<DongleSharedState>,
    mut rx: mpsc::Receiver<CommandRequest>,
    running: Arc<AtomicBool>,
) {
    debug!("Dongle flow-control worker started");

    while let Some(req) = rx.recv().await {
        let result = if req.fire_and_forget {
            execute_send_only(&inner, &state, req.cmd, &req.data, req.checksum)
        } else {
            execute_query(
                &inner,
                &state,
                req.cmd,
                &req.data,
                req.checksum,
                req.raw_mode,
            )
        };
        let _ = req.response_tx.send(result);
    }

    running.store(false, Ordering::SeqCst);
    debug!("Dongle flow-control worker stopped");
}

fn execute_send_only(
    inner: &Arc<dyn Transport>,
    _state: &DongleSharedState,
    cmd_byte: u8,
    data: &[u8],
    checksum: ChecksumType,
) -> Result<Vec<u8>, TransportError> {
    debug!(
        "Dongle sending SET command 0x{:02X} (fire-and-forget)",
        cmd_byte
    );

    futures::executor::block_on(async {
        inner.send_report(cmd_byte, data, checksum).await?;
        inner.send_flush().await?;
        Ok::<(), TransportError>(())
    })?;

    std::thread::sleep(Duration::from_millis(dongle_timing::POLL_CYCLE_MS * 5));
    Ok(Vec::new())
}

fn execute_query(
    inner: &Arc<dyn Transport>,
    state: &DongleSharedState,
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

    let start = Instant::now();

    let timeout = if state.wake_mode.load(Ordering::Relaxed) {
        Duration::from_millis(dongle_timing::WAKE_TIMEOUT_MS)
    } else {
        Duration::from_millis(dongle_timing::QUERY_TIMEOUT_MS)
    };

    // Send command
    debug!("Dongle sending command 0x{:02X}", cmd_byte);
    futures::executor::block_on(inner.send_report(cmd_byte, data, checksum))?;

    // Get adaptive initial wait
    let initial_wait = state.latency_tracker.lock().estimate_initial_wait();
    std::thread::sleep(initial_wait);

    // Polling loop
    let mut poll_count = 0u32;
    while start.elapsed() < timeout {
        poll_count += 1;

        // Flush + read
        futures::executor::block_on(inner.send_flush())?;

        if let Ok(resp) = futures::executor::block_on(inner.read_report()) {
            let resp_cmd = resp.first().copied().unwrap_or(0);

            if raw_mode {
                if resp_cmd != cmd::DONGLE_FLUSH_NOP {
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
                    return Ok(resp);
                }
            } else if resp_cmd == cmd_byte {
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
                return Ok(resp);
            } else if resp_cmd != 0 && resp_cmd != cmd::DONGLE_FLUSH_NOP {
                debug!("Caching out-of-order response for 0x{:02X}", resp_cmd);
                state.cache.lock().add(resp_cmd, resp);
            }
        }

        std::thread::sleep(Duration::from_millis(dongle_timing::POLL_CYCLE_MS));
    }

    // Timeout handling
    let prev_timeouts = state.consecutive_timeouts.fetch_add(1, Ordering::Relaxed);

    if prev_timeouts == 0 && !state.wake_mode.load(Ordering::Relaxed) {
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

impl Drop for FlowControlTransport {
    fn drop(&mut self) {
        if let FlowState::Dongle { state, .. } = &self.flow {
            let tracker = state.latency_tracker.lock();
            if !tracker.samples.is_empty() {
                debug!(
                    "FlowControlTransport dropping - avg dongle latency: {:.2}ms",
                    tracker.average_ms()
                );
            }
        }
    }
}
