// gRPC server implementation for iot_driver compatibility
// Provides the same interface as the original Windows iot_driver.exe
//
// Now uses the transport abstraction layer for unified device access
// across wired, 2.4GHz dongle, and Bluetooth connections.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures::{Stream, StreamExt};
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use tokio_stream::wrappers::BroadcastStream;
use tokio_udev::{EventType, MonitorBuilder};
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};

use iot_driver::hal::HidInterface;
use monsgeek_transport::{
    ChecksumType, DeviceDiscovery, HidDiscovery, PrinterConfig, TimestampedEvent, Transport,
    TransportType, VendorEvent,
};

#[allow(non_camel_case_types)] // Proto types use camelCase to match original iot_driver.exe
#[allow(clippy::enum_variant_names)] // Proto enum variants have Yzw prefix
pub mod driver {
    tonic::include_proto!("driver");
}

pub use driver::driver_grpc_server::{DriverGrpc, DriverGrpcServer};
pub use driver::*;

/// Broadcast channel buffer sizes
const DEVICE_CHANNEL_SIZE: usize = 16;
const VENDOR_CHANNEL_SIZE: usize = 256;

/// Connected device with transport
struct ConnectedTransport {
    transport: Arc<dyn Transport>,
    /// Cached device ID from initial scan (avoids re-querying the transport)
    device_id: i32,
    is_dongle: bool,
    vid: u16,
    pid: u16,
}

/// Convert proto CheckSumType to transport ChecksumType
fn proto_to_transport_checksum(proto: CheckSumType) -> ChecksumType {
    match proto {
        CheckSumType::Bit7 => ChecksumType::Bit7,
        CheckSumType::Bit8 => ChecksumType::Bit8,
        CheckSumType::None => ChecksumType::None,
    }
}

/// Parse device path in format "vid-pid-usage_page-usage-interface"
fn parse_device_path(path: &str) -> Option<(u16, u16, u16, u16, i32)> {
    HidInterface::parse_path_key(path)
}

/// Convert a VendorEvent to raw bytes for the gRPC protocol
/// Emits raw USB HID format with report ID prefix expected by webapp
fn vendor_event_to_bytes(event: &VendorEvent) -> Vec<u8> {
    // Prepend USB report ID (0x05) to match raw HID format expected by webapp
    use monsgeek_transport::event_parser::report_id::USB_VENDOR_EVENT as REPORT_ID;

    match event {
        VendorEvent::KeyDepth {
            key_index,
            depth_raw,
        } => {
            // USB format: [0x05, 0x1B, depth_low, depth_high, key_index]
            vec![
                REPORT_ID,
                0x1B,
                (*depth_raw & 0xFF) as u8,
                (*depth_raw >> 8) as u8,
                *key_index,
            ]
        }
        VendorEvent::MagnetismStart => vec![REPORT_ID, 0x0F, 0x01, 0x00],
        VendorEvent::MagnetismStop => vec![REPORT_ID, 0x0F, 0x00, 0x00],
        VendorEvent::Wake => vec![REPORT_ID, 0x00, 0x00, 0x00],
        VendorEvent::ProfileChange { profile } => vec![REPORT_ID, 0x01, *profile, 0x00],
        VendorEvent::SettingsAck { started } => {
            vec![REPORT_ID, 0x0F, if *started { 0x01 } else { 0x00 }, 0x00]
        }
        VendorEvent::LedEffectMode { effect_id } => vec![REPORT_ID, 0x04, *effect_id, 0x00],
        VendorEvent::LedEffectSpeed { speed } => vec![REPORT_ID, 0x05, *speed, 0x00],
        VendorEvent::BrightnessLevel { level } => vec![REPORT_ID, 0x06, *level, 0x00],
        VendorEvent::LedColor { color } => vec![REPORT_ID, 0x07, *color, 0x00],
        VendorEvent::WinLockToggle { locked } => {
            vec![REPORT_ID, 0x03, if *locked { 1 } else { 0 }, 0x01]
        }
        VendorEvent::WasdSwapToggle { swapped } => {
            vec![REPORT_ID, 0x03, if *swapped { 8 } else { 0 }, 0x03]
        }
        VendorEvent::BacklightToggle => vec![REPORT_ID, 0x03, 0x00, 0x09],
        VendorEvent::FnLayerToggle { layer } => vec![REPORT_ID, 0x03, *layer, 0x08],
        VendorEvent::DialModeToggle => vec![REPORT_ID, 0x03, 0x00, 0x11],
        VendorEvent::UnknownKbFunc { category, action } => {
            vec![REPORT_ID, 0x03, *category, *action]
        }
        VendorEvent::BatteryStatus {
            level,
            charging,
            online,
        } => {
            let flags = if *charging { 0x02 } else { 0 } | if *online { 0x01 } else { 0 };
            vec![REPORT_ID, 0x88, 0x00, 0x00, *level, flags]
        }
        VendorEvent::MouseReport {
            buttons,
            x,
            y,
            wheel,
        } => {
            // Mouse report uses report ID 0x02
            vec![
                0x02,
                *buttons,
                0x00,
                (*x & 0xFF) as u8,
                (*x >> 8) as u8,
                (*y & 0xFF) as u8,
                (*y >> 8) as u8,
                (*wheel & 0xFF) as u8,
                (*wheel >> 8) as u8,
            ]
        }
        VendorEvent::Unknown(data) => {
            // Already has report ID if from raw HID
            if data.first() == Some(&REPORT_ID) {
                data.clone()
            } else {
                let mut out = vec![REPORT_ID];
                out.extend(data);
                out
            }
        }
    }
}

/// Drop guard that decrements vendor subscriber count
struct VendorSubscriberGuard(Arc<AtomicUsize>);

impl Drop for VendorSubscriberGuard {
    fn drop(&mut self) {
        let prev = self.0.fetch_sub(1, Ordering::Release);
        info!("Vendor stream closed, {} subscriber(s) remaining", prev - 1);
    }
}

// Stream wrapper that holds a drop guard alongside the inner stream
pin_project_lite::pin_project! {
    struct GuardedStream<S> {
        #[pin]
        inner: S,
        _guard: VendorSubscriberGuard,
    }
}

impl<S: Stream> Stream for GuardedStream<S> {
    type Item = S::Item;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.project().inner.poll_next(cx)
    }
}

/// Key for the in-memory DB: (dbPath, key)
type DbKey = (String, Vec<u8>);

/// Driver service implementation using transport abstraction layer
pub struct DriverService {
    discovery: Arc<HidDiscovery>,
    devices: Arc<AsyncMutex<HashMap<String, ConnectedTransport>>>,
    device_tx: broadcast::Sender<DeviceList>,
    vendor_tx: broadcast::Sender<VenderMsg>,
    vendor_polling: Arc<AsyncMutex<bool>>,
    vendor_subscribers: Arc<AtomicUsize>,
    hotplug_running: Arc<std::sync::Mutex<bool>>,
    /// In-memory key-value store for webapp DB RPCs
    db: Arc<AsyncMutex<HashMap<DbKey, Vec<u8>>>>,
}

impl DriverService {
    pub fn with_printer_config(
        printer_config: Option<PrinterConfig>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (device_tx, _) = broadcast::channel(DEVICE_CHANNEL_SIZE);
        let (vendor_tx, _) = broadcast::channel(VENDOR_CHANNEL_SIZE);

        // Create discovery with printer config - wrapping happens automatically in open_device()
        let discovery = match printer_config {
            Some(config) => HidDiscovery::with_printer_config(config),
            None => HidDiscovery::new(),
        };

        Ok(Self {
            discovery: Arc::new(discovery),
            devices: Arc::new(AsyncMutex::new(HashMap::new())),
            device_tx,
            vendor_tx,
            vendor_polling: Arc::new(AsyncMutex::new(false)),
            vendor_subscribers: Arc::new(AtomicUsize::new(0)),
            hotplug_running: Arc::new(std::sync::Mutex::new(false)),
            db: Arc::new(AsyncMutex::new(HashMap::new())),
        })
    }

    /// Start background polling for vendor events from connected devices
    fn start_vendor_polling(&self) {
        let devices = Arc::clone(&self.devices);
        let vendor_tx = self.vendor_tx.clone();
        let vendor_polling = Arc::clone(&self.vendor_polling);
        let vendor_subscribers = Arc::clone(&self.vendor_subscribers);

        tokio::spawn(async move {
            {
                let mut polling = vendor_polling.lock().await;
                if *polling {
                    return;
                }
                *polling = true;
            }

            info!("Started vendor event polling");

            // Track persistent receivers per device path
            // Using subscribe_events() gives us a receiver that persists across the loop,
            // so we don't miss events between iterations (unlike read_event() which
            // creates a new receiver each call)
            let mut receivers: HashMap<String, broadcast::Receiver<TimestampedEvent>> =
                HashMap::new();

            loop {
                // Stop polling when no subscribers remain
                if vendor_subscribers.load(Ordering::Relaxed) == 0 {
                    let mut polling = vendor_polling.lock().await;
                    // Double-check under lock to avoid race with new subscriber
                    if vendor_subscribers.load(Ordering::Acquire) == 0 {
                        *polling = false;
                        break;
                    }
                }

                {
                    let polling = vendor_polling.lock().await;
                    if !*polling {
                        break;
                    }
                }

                // Update receivers for new/removed devices
                {
                    let devices_guard = devices.lock().await;

                    // Remove receivers for disconnected devices
                    receivers.retain(|path, _| devices_guard.contains_key(path));

                    // Add receivers for new devices
                    for (path, connected) in devices_guard.iter() {
                        if !receivers.contains_key(path) {
                            if let Some(rx) = connected.transport.subscribe_events() {
                                debug!("Subscribed to events for device {}", path);
                                receivers.insert(path.clone(), rx);
                            }
                        }
                    }
                }

                // Poll all receivers with short timeout
                let mut got_event = false;
                let mut closed = Vec::new();
                for (path, rx) in receivers.iter_mut() {
                    match tokio::time::timeout(std::time::Duration::from_millis(1), rx.recv()).await
                    {
                        Ok(Ok(timestamped)) => {
                            debug!("Vendor event from {}: {:?}", path, timestamped.event);
                            let msg = vendor_event_to_bytes(&timestamped.event);
                            let _ = vendor_tx.send(VenderMsg { msg });
                            got_event = true;
                        }
                        Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                            debug!("Event receiver for {} lagged by {} events", path, n);
                        }
                        Ok(Err(broadcast::error::RecvError::Closed)) => {
                            debug!("Event channel closed for {}, removing receiver", path);
                            closed.push(path.clone());
                        }
                        Err(_) => {} // Timeout, no event
                    }
                }
                for path in closed {
                    receivers.remove(&path);
                }

                if !got_event {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            }

            info!("Stopped vendor event polling");
        });
    }

    /// Start hot-plug monitoring using udev (runs in a separate thread)
    pub fn start_hotplug_monitor(&self) {
        let mut running = self.hotplug_running.lock().unwrap();
        if *running {
            return;
        }
        *running = true;
        drop(running);

        let discovery = Arc::clone(&self.discovery);
        let devices = Arc::clone(&self.devices);
        let device_tx = self.device_tx.clone();
        let hotplug_running = Arc::clone(&self.hotplug_running);

        // Use a standard thread since udev types aren't Send
        std::thread::spawn(move || {
            info!("Starting udev hot-plug monitor for hidraw devices");

            let builder = match MonitorBuilder::new() {
                Ok(b) => b,
                Err(e) => {
                    error!("Failed to create udev monitor: {}", e);
                    return;
                }
            };

            let builder = match builder.match_subsystem("hidraw") {
                Ok(b) => b,
                Err(e) => {
                    error!("Failed to set udev subsystem filter: {}", e);
                    return;
                }
            };

            let socket = match builder.listen() {
                Ok(m) => m,
                Err(e) => {
                    error!("Failed to start udev monitor: {}", e);
                    return;
                }
            };

            // Use blocking iteration with poll
            use std::os::unix::io::AsRawFd;
            let fd = socket.as_raw_fd();

            // Create a runtime for async operations
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    error!("Failed to create tokio runtime: {}", e);
                    return;
                }
            };

            loop {
                {
                    let running = hotplug_running.lock().unwrap();
                    if !*running {
                        break;
                    }
                }

                // Poll with timeout so we can check the running flag
                let mut fds = [libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                }];

                let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, 1000) };

                if ret <= 0 {
                    continue; // Timeout or error, check running flag
                }

                // Event available
                if let Some(event) = socket.iter().next() {
                    let devnode = event.devnode().map(|p| p.to_string_lossy().to_string());
                    debug!("udev event: {:?} for {:?}", event.event_type(), devnode);

                    match event.event_type() {
                        EventType::Add => {
                            info!("Device added: {:?}", devnode);
                            // Re-scan and broadcast new devices
                            rt.block_on(Self::rescan_devices_static(
                                &discovery, &devices, &device_tx,
                            ));
                        }
                        EventType::Remove => {
                            info!("Device removed: {:?}", devnode);
                            // Clean up disconnected devices and broadcast removal
                            rt.block_on(Self::cleanup_disconnected_static(
                                &discovery, &devices, &device_tx,
                            ));
                        }
                        _ => {}
                    }
                }
            }

            info!("Stopped udev hot-plug monitor");
        });
    }

    /// Static helper to re-scan devices (called from udev thread)
    async fn rescan_devices_static(
        discovery: &Arc<HidDiscovery>,
        devices: &Arc<AsyncMutex<HashMap<String, ConnectedTransport>>>,
        device_tx: &broadcast::Sender<DeviceList>,
    ) {
        let discovered = match discovery.list_devices().await {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to list devices: {}", e);
                return;
            }
        };

        let mut devs = devices.lock().await;

        for dev in discovered {
            let path = Self::make_path_key(&dev.info);

            if devs.contains_key(&path) {
                continue;
            }

            match discovery.open_device(&dev).await {
                Ok(transport) => {
                    info!("Hot-plug: opened new device {}", path);

                    // Query device ID
                    let device_id = Self::query_device_id_static(&transport).await.unwrap_or(0);

                    // Query battery for dongles
                    let (battery, is_online) = if dev.info.is_dongle {
                        match transport.get_battery_status().await {
                            Ok((level, online, _idle)) => (level as u32, online),
                            Err(_) => (100, true),
                        }
                    } else {
                        (100, true)
                    };

                    let dev_info = Device {
                        dev_type: DeviceType::YzwKeyboard as i32,
                        is24: dev.info.is_dongle,
                        path: path.clone(),
                        id: device_id,
                        battery,
                        is_online,
                        vid: dev.info.vid as u32,
                        pid: dev.info.pid as u32,
                    };

                    // Broadcast new device
                    let _ = device_tx.send(DeviceList {
                        dev_list: vec![DjDev {
                            oneof_dev: Some(dj_dev::OneofDev::Dev(dev_info)),
                        }],
                        r#type: DeviceListChangeType::Add as i32,
                    });

                    // Transport is already wrapped by HidDiscovery if monitoring enabled
                    devs.insert(
                        path,
                        ConnectedTransport {
                            transport,
                            device_id,
                            is_dongle: dev.info.is_dongle,
                            vid: dev.info.vid,
                            pid: dev.info.pid,
                        },
                    );
                }
                Err(e) => {
                    warn!("Hot-plug: failed to open device: {}", e);
                }
            }
        }
    }

    /// Static helper to clean up disconnected devices
    async fn cleanup_disconnected_static(
        discovery: &Arc<HidDiscovery>,
        devices: &Arc<AsyncMutex<HashMap<String, ConnectedTransport>>>,
        device_tx: &broadcast::Sender<DeviceList>,
    ) {
        let discovered = match discovery.list_devices().await {
            Ok(d) => d,
            Err(_) => return,
        };

        let mut devs = devices.lock().await;

        // Get current device paths
        let current_paths: std::collections::HashSet<String> = discovered
            .iter()
            .map(|d| Self::make_path_key(&d.info))
            .collect();

        // Remove devices no longer present
        let to_remove: Vec<String> = devs
            .keys()
            .filter(|path| !current_paths.contains(*path))
            .cloned()
            .collect();

        for path in to_remove {
            info!("Hot-plug: removing disconnected device {}", path);
            if let Some(removed) = devs.remove(&path) {
                // Broadcast removal with cached device info so webapp can identify it
                let removed_dev = DjDev {
                    oneof_dev: Some(dj_dev::OneofDev::Dev(Device {
                        dev_type: DeviceType::YzwKeyboard as i32,
                        is24: removed.is_dongle,
                        path: path.clone(),
                        id: removed.device_id,
                        battery: 0,
                        is_online: false,
                        vid: removed.vid as u32,
                        pid: removed.pid as u32,
                    })),
                };
                let _ = device_tx.send(DeviceList {
                    dev_list: vec![removed_dev],
                    r#type: DeviceListChangeType::Remove as i32,
                });
            }
        }
    }

    /// Create a path key compatible with the original protocol format
    fn make_path_key(info: &monsgeek_transport::TransportDeviceInfo) -> String {
        // Format: vid-pid-usage_page-usage-interface
        // For compatibility with browser client
        let usage_page = 0xFFFF_u16;
        let usage = 0x02_u16;
        let interface = match info.transport_type {
            TransportType::Bluetooth => 0,
            TransportType::HidWired | TransportType::HidDongle => 1,
            _ => 0,
        };
        format!(
            "{:04x}-{:04x}-{:04x}-{:04x}-{}",
            info.vid, info.pid, usage_page, usage, interface
        )
    }

    /// Query device ID using GET_USB_VERSION command (raw send+read)
    async fn query_device_id_static(transport: &Arc<dyn Transport>) -> Option<i32> {
        use monsgeek_transport::protocol::cmd;

        if let Err(e) = transport
            .send_report(cmd::GET_USB_VERSION, &[], ChecksumType::Bit7)
            .await
        {
            warn!("Failed to send device ID query: {}", e);
            return None;
        }

        let _ = transport.send_flush().await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        match transport.read_report().await {
            Ok(resp) if resp.len() >= 5 && resp[0] == cmd::GET_USB_VERSION => {
                let device_id = u32::from_le_bytes([resp[1], resp[2], resp[3], resp[4]]) as i32;
                info!("Device ID: {}", device_id);
                Some(device_id)
            }
            Ok(resp) => {
                warn!(
                    "Unexpected response to device ID query: {:02x?}",
                    &resp[..resp.len().min(16)]
                );
                None
            }
            Err(e) => {
                warn!("Failed to read device ID response: {}", e);
                None
            }
        }
    }

    /// Scan for and connect to known devices (only returns client-facing interfaces)
    pub async fn scan_devices(&self) -> Vec<DjDev> {
        let mut found = Vec::new();

        let discovered = match self.discovery.list_devices().await {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to list devices: {}", e);
                return found;
            }
        };

        for dev in discovered {
            let path = Self::make_path_key(&dev.info);

            info!(
                "Found device: VID={:04x} PID={:04x} type={:?}",
                dev.info.vid, dev.info.pid, dev.info.transport_type
            );

            // Skip devices already open — reuse existing transport
            let already_open = {
                let devices = self.devices.lock().await;
                devices.contains_key(&path)
            };

            let (device_id, battery, is_online) = if already_open {
                debug!("Device {} already open, using cached info", path);
                let devices = self.devices.lock().await;
                let connected = devices.get(&path).unwrap();
                // Use cached device_id — never re-query a live transport
                // (would interleave with TUI/webapp commands)
                (connected.device_id, 100, true)
            } else {
                // Open new device
                match self.discovery.open_device(&dev).await {
                    Ok(transport) => {
                        debug!(
                            "Opened device (type={:?}), querying device ID...",
                            dev.info.transport_type
                        );

                        let id = Self::query_device_id_static(&transport).await.unwrap_or(0);

                        // Query battery status for dongles
                        let (batt, online) = if dev.info.is_dongle {
                            match transport.get_battery_status().await {
                                Ok((level, online, _idle)) => (level as u32, online),
                                Err(_) => (100, true),
                            }
                        } else {
                            (100, true)
                        };

                        // Transport is already wrapped by HidDiscovery if monitoring enabled
                        {
                            let mut devices = self.devices.lock().await;
                            devices.insert(
                                path.clone(),
                                ConnectedTransport {
                                    transport,
                                    device_id: id,
                                    is_dongle: dev.info.is_dongle,
                                    vid: dev.info.vid,
                                    pid: dev.info.pid,
                                },
                            );
                        }

                        (id, batt, online)
                    }
                    Err(e) => {
                        warn!("Could not open device to query ID: {}", e);
                        (0, 100, true)
                    }
                }
            };

            if dev.info.is_dongle {
                // 2.4GHz dongle - use DangleCommon format
                let keyboard_status = DangleStatus {
                    dangle_dev: Some(dangle_status::DangleDev::Status(Status24 {
                        battery,
                        is_online,
                    })),
                };
                let mouse_status = DangleStatus {
                    dangle_dev: Some(dangle_status::DangleDev::Empty(Empty {})),
                };
                let dongle = DangleCommon {
                    keyboard: Some(keyboard_status),
                    mouse: Some(mouse_status),
                    path: path.clone(),
                    keyboard_id: device_id as u32,
                    mouse_id: 0,
                    vid: dev.info.vid as u32,
                    pid: dev.info.pid as u32,
                };
                found.push(DjDev {
                    oneof_dev: Some(dj_dev::OneofDev::DangleCommonDev(dongle)),
                });
            } else {
                // Wired or Bluetooth device - use Device format
                let device = Device {
                    dev_type: DeviceType::YzwKeyboard as i32,
                    is24: false,
                    path: path.clone(),
                    id: device_id,
                    battery,
                    is_online,
                    vid: dev.info.vid as u32,
                    pid: dev.info.pid as u32,
                };
                found.push(DjDev {
                    oneof_dev: Some(dj_dev::OneofDev::Dev(device)),
                });
            }
        }

        found
    }

    async fn open_device(&self, device_path: &str) -> Result<(), Status> {
        // Check if already open
        {
            let devices = self.devices.lock().await;
            if devices.contains_key(device_path) {
                return Ok(());
            }
        }

        // Parse path to get VID/PID
        let (vid, pid, _usage_page, _usage, _interface) = parse_device_path(device_path)
            .ok_or_else(|| Status::invalid_argument("Invalid device path format"))?;

        // Find and open the device
        let discovered = self
            .discovery
            .list_devices()
            .await
            .map_err(|e| Status::internal(format!("Discovery error: {}", e)))?;

        for dev in discovered {
            if dev.info.vid == vid && dev.info.pid == pid {
                let transport = self
                    .discovery
                    .open_device(&dev)
                    .await
                    .map_err(|e| Status::internal(format!("Failed to open device: {}", e)))?;

                let mut devices = self.devices.lock().await;
                devices.insert(
                    device_path.to_string(),
                    ConnectedTransport {
                        transport,
                        device_id: 0,
                        is_dongle: dev.info.is_dongle,
                        vid: dev.info.vid,
                        pid: dev.info.pid,
                    },
                );

                info!("Opened device: {}", device_path);
                return Ok(());
            }
        }

        Err(Status::not_found("Device not found"))
    }

    /// Send command immediately to the device
    ///
    /// The webapp handles its own flow control (retries, echo matching,
    /// polling cadence). We send immediately so fire-and-forget commands
    /// (where the webapp never calls readMsg) still reach the device.
    /// send_flush is a no-op on wired/BLE but pushes the dongle buffer.
    async fn send_command(
        &self,
        device_path: &str,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), Status> {
        // Ensure device is open
        self.open_device(device_path).await?;

        let devices = self.devices.lock().await;
        let connected = devices
            .get(device_path)
            .ok_or_else(|| Status::not_found("Device not connected"))?;

        if data.is_empty() {
            return Err(Status::invalid_argument("Empty command data"));
        }

        let cmd = data[0];
        let payload = if data.len() > 1 { &data[1..] } else { &[] };

        debug!("Sending command 0x{:02x} to {}", cmd, device_path);

        connected
            .transport
            .send_report(cmd, payload, checksum)
            .await
            .map_err(|e| Status::internal(format!("Send error: {}", e)))?;

        connected
            .transport
            .send_flush()
            .await
            .map_err(|e| Status::internal(format!("Flush error: {}", e)))?;

        Ok(())
    }

    /// Read response from device
    ///
    /// The webapp calls this after sendMsg to retrieve the keyboard's response.
    /// The command was already sent in send_command, so we just read.
    async fn read_response(&self, device_path: &str) -> Result<Vec<u8>, Status> {
        // Ensure device is open
        self.open_device(device_path).await?;

        let devices = self.devices.lock().await;
        let connected = devices
            .get(device_path)
            .ok_or_else(|| Status::not_found("Device not connected"))?;

        let response = connected
            .transport
            .read_report()
            .await
            .map_err(|e| Status::internal(format!("Read error: {}", e)))?;

        debug!(
            "Read response: {:02x?}",
            &response[..response.len().min(16)]
        );

        Ok(response)
    }
}

#[tonic::async_trait]
#[allow(non_camel_case_types)]
impl DriverGrpc for DriverService {
    type watchDevListStream = Pin<Box<dyn Stream<Item = Result<DeviceList, Status>> + Send>>;
    type watchSystemInfoStream = Pin<Box<dyn Stream<Item = Result<SystemInfo, Status>> + Send>>;
    type upgradeOTAGATTStream = Pin<Box<dyn Stream<Item = Result<Progress, Status>> + Send>>;
    type watchVenderStream = Pin<Box<dyn Stream<Item = Result<VenderMsg, Status>> + Send>>;

    async fn watch_dev_list(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::watchDevListStream>, Status> {
        info!("watch_dev_list called");

        let initial_devices = self.scan_devices().await;
        info!(
            "Sending {} initial devices to client",
            initial_devices.len()
        );

        let rx = self.device_tx.subscribe();

        let initial_list = DeviceList {
            dev_list: initial_devices,
            r#type: DeviceListChangeType::Init as i32,
        };

        let initial_stream = futures::stream::iter(std::iter::once(Ok(initial_list)));

        let broadcast_stream = BroadcastStream::new(rx).filter_map(|result| async move {
            match result {
                Ok(device_list) => Some(Ok(device_list)),
                Err(_) => None,
            }
        });

        let combined = initial_stream.chain(broadcast_stream);
        Ok(Response::new(Box::pin(combined)))
    }

    async fn watch_system_info(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::watchSystemInfoStream>, Status> {
        info!("watch_system_info called");
        Ok(Response::new(Box::pin(futures::stream::empty())))
    }

    async fn send_raw_feature(
        &self,
        request: Request<SendMsg>,
    ) -> Result<Response<ResSend>, Status> {
        let msg = request.into_inner();
        debug!(
            "send_raw_feature: path={}, {} bytes",
            msg.device_path,
            msg.msg.len()
        );

        // For raw feature, don't apply checksum
        match self
            .send_command(&msg.device_path, &msg.msg, ChecksumType::None)
            .await
        {
            Ok(_) => Ok(Response::new(ResSend { err: String::new() })),
            Err(e) => Ok(Response::new(ResSend {
                err: e.message().to_string(),
            })),
        }
    }

    async fn read_raw_feature(
        &self,
        request: Request<ReadMsg>,
    ) -> Result<Response<ResRead>, Status> {
        let msg = request.into_inner();
        debug!("read_raw_feature: path={}", msg.device_path);

        match self.read_response(&msg.device_path).await {
            Ok(data) => Ok(Response::new(ResRead {
                err: String::new(),
                msg: data,
            })),
            Err(e) => Ok(Response::new(ResRead {
                err: e.message().to_string(),
                msg: vec![],
            })),
        }
    }

    async fn send_msg(&self, request: Request<SendMsg>) -> Result<Response<ResSend>, Status> {
        let msg = request.into_inner();
        debug!(
            "send_msg: path={}, checksum={:?}",
            msg.device_path, msg.check_sum_type
        );

        let checksum_type =
            CheckSumType::try_from(msg.check_sum_type).unwrap_or(CheckSumType::Bit7);
        let transport_checksum = proto_to_transport_checksum(checksum_type);

        match self
            .send_command(&msg.device_path, &msg.msg, transport_checksum)
            .await
        {
            Ok(_) => Ok(Response::new(ResSend { err: String::new() })),
            Err(e) => Ok(Response::new(ResSend {
                err: e.message().to_string(),
            })),
        }
    }

    async fn read_msg(&self, request: Request<ReadMsg>) -> Result<Response<ResRead>, Status> {
        let msg = request.into_inner();
        debug!("read_msg: path={}", msg.device_path);

        match self.read_response(&msg.device_path).await {
            Ok(data) => Ok(Response::new(ResRead {
                err: String::new(),
                msg: data,
            })),
            Err(e) => Ok(Response::new(ResRead {
                err: e.message().to_string(),
                msg: vec![],
            })),
        }
    }

    async fn get_item_from_db(&self, request: Request<GetItem>) -> Result<Response<Item>, Status> {
        let req = request.into_inner();
        let key_str = String::from_utf8_lossy(&req.key);
        let db = self.db.lock().await;
        let db_key = (req.db_path.clone(), req.key.clone());
        let value = db.get(&db_key).cloned().unwrap_or_default();
        info!(
            "get_item_from_db: dbPath={:?}, key={:?}, found={} bytes",
            req.db_path,
            key_str,
            value.len()
        );
        Ok(Response::new(Item {
            value,
            err_str: String::new(),
        }))
    }

    async fn insert_db(&self, request: Request<InsertDb>) -> Result<Response<ResSend>, Status> {
        let req = request.into_inner();
        let key_str = String::from_utf8_lossy(&req.key);
        let value_preview = String::from_utf8_lossy(&req.value[..req.value.len().min(200)]);
        info!(
            "insert_db: dbPath={:?}, key={:?}, value_len={}, preview={:?}",
            req.db_path,
            key_str,
            req.value.len(),
            value_preview
        );
        let mut db = self.db.lock().await;
        db.insert((req.db_path, req.key), req.value);
        Ok(Response::new(ResSend { err: String::new() }))
    }

    async fn delete_item_from_db(
        &self,
        request: Request<DeleteItem>,
    ) -> Result<Response<ResSend>, Status> {
        let req = request.into_inner();
        let key_str = String::from_utf8_lossy(&req.key);
        info!(
            "delete_item_from_db: dbPath={:?}, key={:?}",
            req.db_path, key_str
        );
        let mut db = self.db.lock().await;
        db.remove(&(req.db_path, req.key));
        Ok(Response::new(ResSend { err: String::new() }))
    }

    async fn get_all_keys_from_db(
        &self,
        request: Request<GetAll>,
    ) -> Result<Response<AllList>, Status> {
        let req = request.into_inner();
        let db = self.db.lock().await;
        let keys: Vec<Vec<u8>> = db
            .keys()
            .filter(|(path, _)| *path == req.db_path)
            .map(|(_, key)| key.clone())
            .collect();
        info!(
            "get_all_keys_from_db: dbPath={:?}, returning {} keys",
            req.db_path,
            keys.len()
        );
        Ok(Response::new(AllList {
            data: keys,
            err_str: String::new(),
        }))
    }

    async fn get_all_values_from_db(
        &self,
        request: Request<GetAll>,
    ) -> Result<Response<AllList>, Status> {
        let req = request.into_inner();
        let db = self.db.lock().await;
        let values: Vec<Vec<u8>> = db
            .iter()
            .filter(|((path, _), _)| *path == req.db_path)
            .map(|(_, value)| value.clone())
            .collect();
        info!(
            "get_all_values_from_db: dbPath={:?}, returning {} values",
            req.db_path,
            values.len()
        );
        Ok(Response::new(AllList {
            data: values,
            err_str: String::new(),
        }))
    }

    async fn get_version(&self, _request: Request<Empty>) -> Result<Response<Version>, Status> {
        Ok(Response::new(Version {
            base_version: "222".to_string(),
            time_stamp: "2024-12-29".to_string(),
        }))
    }

    async fn upgrade_otagatt(
        &self,
        _request: Request<OtaUpgrade>,
    ) -> Result<Response<Self::upgradeOTAGATTStream>, Status> {
        Ok(Response::new(Box::pin(futures::stream::empty())))
    }

    async fn mute_microphone(
        &self,
        _request: Request<MuteMicrophone>,
    ) -> Result<Response<ResSend>, Status> {
        Ok(Response::new(ResSend { err: String::new() }))
    }

    async fn toggle_microphone_mute(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<MicrophoneMuteStatus>, Status> {
        Ok(Response::new(MicrophoneMuteStatus {
            is_mute: false,
            err: String::new(),
        }))
    }

    async fn get_microphone_mute(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<MicrophoneMuteStatus>, Status> {
        Ok(Response::new(MicrophoneMuteStatus {
            is_mute: false,
            err: String::new(),
        }))
    }

    async fn change_wireless_loop_status(
        &self,
        _request: Request<WirelessLoopStatus>,
    ) -> Result<Response<ResSend>, Status> {
        Ok(Response::new(ResSend { err: String::new() }))
    }

    async fn set_light_type(&self, _request: Request<SetLight>) -> Result<Response<Empty>, Status> {
        Ok(Response::new(Empty {}))
    }

    async fn watch_vender(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::watchVenderStream>, Status> {
        info!("watch_vender called - starting vendor event stream");

        self.vendor_subscribers.fetch_add(1, Ordering::Release);
        self.start_vendor_polling();

        let rx = self.vendor_tx.subscribe();
        let subscribers = Arc::clone(&self.vendor_subscribers);
        let guard = VendorSubscriberGuard(subscribers);

        let stream = BroadcastStream::new(rx).filter_map(|result| async move {
            match result {
                Ok(event) => Some(Ok(event)),
                Err(_) => None,
            }
        });

        // Wrap stream so the guard is dropped when the stream is dropped
        let guarded_stream = GuardedStream {
            inner: stream,
            _guard: guard,
        };

        Ok(Response::new(Box::pin(guarded_stream)))
    }

    async fn get_weather(
        &self,
        _request: Request<WeatherReq>,
    ) -> Result<Response<WeatherRes>, Status> {
        Ok(Response::new(WeatherRes {
            res: "{}".to_string(),
        }))
    }
}
