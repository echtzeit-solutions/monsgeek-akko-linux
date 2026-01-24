// gRPC server implementation for iot_driver compatibility
// Provides the same interface as the original Windows iot_driver.exe
//
// Now uses the transport abstraction layer for unified device access
// across wired, 2.4GHz dongle, and Bluetooth connections.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use futures::{Stream, StreamExt};
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use tokio_stream::wrappers::BroadcastStream;
use tokio_udev::{EventType, MonitorBuilder};
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};

use iot_driver::hal::HidInterface;
use monsgeek_transport::{
    ChecksumType, DeviceDiscovery, HidDiscovery, Transport, TransportType, VendorEvent,
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

/// Pending command for split send/read API
#[derive(Clone)]
struct PendingCommand {
    cmd: u8,
    data: Vec<u8>,
    checksum: ChecksumType,
}

/// Connected device with transport
struct ConnectedTransport {
    transport: Arc<dyn Transport>,
    /// Pending command waiting for read_msg
    pending: Option<PendingCommand>,
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
fn vendor_event_to_bytes(event: &VendorEvent) -> Vec<u8> {
    match event {
        VendorEvent::KeyDepth {
            key_index,
            depth_raw,
        } => {
            // Format: [0x1B, key_index, depth_low, depth_high]
            vec![
                0x1B,
                *key_index,
                (*depth_raw & 0xFF) as u8,
                (*depth_raw >> 8) as u8,
            ]
        }
        VendorEvent::MagnetismStart => vec![0x0F, 0x01],
        VendorEvent::MagnetismStop => vec![0x0F, 0x00],
        VendorEvent::Wake => vec![0x00],
        VendorEvent::ProfileChange { profile } => vec![0x01, *profile],
        VendorEvent::SettingsAck { started } => vec![0x0F, if *started { 0x01 } else { 0x00 }],
        VendorEvent::LedEffectMode { effect_id } => vec![0x04, *effect_id],
        VendorEvent::LedEffectSpeed { speed } => vec![0x05, *speed],
        VendorEvent::BrightnessLevel { level } => vec![0x06, *level],
        VendorEvent::LedColor { color } => vec![0x07, *color],
        VendorEvent::WinLockToggle { locked } => vec![0x03, if *locked { 1 } else { 0 }, 0x01],
        VendorEvent::WasdSwapToggle { swapped } => vec![0x03, if *swapped { 8 } else { 0 }, 0x03],
        VendorEvent::BacklightToggle => vec![0x03, 0, 0x09],
        VendorEvent::FnLayerToggle { layer } => vec![0x03, *layer, 0x08],
        VendorEvent::DialModeToggle => vec![0x03, 0, 0x11],
        VendorEvent::UnknownKbFunc { category, action } => vec![0x03, *category, *action],
        VendorEvent::BatteryStatus {
            level,
            charging,
            online,
        } => {
            vec![
                0x88,
                *level,
                if *charging { 1 } else { 0 },
                if *online { 1 } else { 0 },
            ]
        }
        VendorEvent::Unknown(data) => data.clone(),
    }
}

/// Driver service implementation using transport abstraction layer
pub struct DriverService {
    discovery: Arc<HidDiscovery>,
    devices: Arc<AsyncMutex<HashMap<String, ConnectedTransport>>>,
    device_tx: broadcast::Sender<DjDev>,
    vendor_tx: broadcast::Sender<VenderMsg>,
    vendor_polling: Arc<AsyncMutex<bool>>,
    hotplug_running: Arc<std::sync::Mutex<bool>>,
}

impl DriverService {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (device_tx, _) = broadcast::channel(DEVICE_CHANNEL_SIZE);
        let (vendor_tx, _) = broadcast::channel(VENDOR_CHANNEL_SIZE);

        Ok(Self {
            discovery: Arc::new(HidDiscovery::new()),
            devices: Arc::new(AsyncMutex::new(HashMap::new())),
            device_tx,
            vendor_tx,
            vendor_polling: Arc::new(AsyncMutex::new(false)),
            hotplug_running: Arc::new(std::sync::Mutex::new(false)),
        })
    }

    /// Start background polling for vendor events from connected devices
    fn start_vendor_polling(&self) {
        let devices = Arc::clone(&self.devices);
        let vendor_tx = self.vendor_tx.clone();
        let vendor_polling = Arc::clone(&self.vendor_polling);

        tokio::spawn(async move {
            {
                let mut polling = vendor_polling.lock().await;
                if *polling {
                    return;
                }
                *polling = true;
            }

            info!("Started vendor event polling");

            loop {
                {
                    let polling = vendor_polling.lock().await;
                    if !*polling {
                        break;
                    }
                }

                let mut got_event = false;
                {
                    let devices_guard = devices.lock().await;
                    for (path, connected) in devices_guard.iter() {
                        // Try to read event with short timeout
                        match connected.transport.read_event(10).await {
                            Ok(Some(event)) => {
                                debug!("Vendor event from {}: {:?}", path, event);
                                // Convert VendorEvent to raw bytes for gRPC protocol
                                let msg = vendor_event_to_bytes(&event);
                                let _ = vendor_tx.send(VenderMsg { msg });
                                got_event = true;
                            }
                            Ok(None) => {}
                            Err(e) => {
                                debug!("Event read error for {}: {}", path, e);
                            }
                        }
                    }
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
                            // Clean up disconnected devices
                            rt.block_on(Self::cleanup_disconnected_static(&discovery, &devices));
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
        device_tx: &broadcast::Sender<DjDev>,
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
                    let _ = device_tx.send(DjDev {
                        oneof_dev: Some(dj_dev::OneofDev::Dev(dev_info)),
                    });

                    devs.insert(
                        path,
                        ConnectedTransport {
                            transport,
                            pending: None,
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
            devs.remove(&path);
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

    /// Query device ID using GET_USB_VERSION command
    async fn query_device_id_static(transport: &Arc<dyn Transport>) -> Option<i32> {
        use monsgeek_transport::protocol::cmd;

        match transport
            .query_command(cmd::GET_USB_VERSION, &[], ChecksumType::Bit7)
            .await
        {
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
                warn!("Failed to query device ID: {}", e);
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

            // Open device to query ID and battery
            let (device_id, battery, is_online) = match self.discovery.open_device(&dev).await {
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

                    // Store transport for later use
                    {
                        let mut devices = self.devices.lock().await;
                        devices.insert(
                            path.clone(),
                            ConnectedTransport {
                                transport,
                                pending: None,
                            },
                        );
                    }

                    (id, batt, online)
                }
                Err(e) => {
                    warn!("Could not open device to query ID: {}", e);
                    (0, 100, true)
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
                        pending: None,
                    },
                );

                info!("Opened device: {}", device_path);
                return Ok(());
            }
        }

        Err(Status::not_found("Device not found"))
    }

    /// Send a command (stores for later read_msg to complete the query)
    async fn send_command(
        &self,
        device_path: &str,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), Status> {
        // Ensure device is open
        self.open_device(device_path).await?;

        let mut devices = self.devices.lock().await;
        let connected = devices
            .get_mut(device_path)
            .ok_or_else(|| Status::not_found("Device not connected"))?;

        if data.is_empty() {
            return Err(Status::invalid_argument("Empty command data"));
        }

        let cmd = data[0];
        let payload = if data.len() > 1 {
            data[1..].to_vec()
        } else {
            Vec::new()
        };

        debug!("Storing pending command 0x{:02x} for {}", cmd, device_path);

        // Store pending command for read_msg
        connected.pending = Some(PendingCommand {
            cmd,
            data: payload,
            checksum,
        });

        // Actually send the command (fire-and-forget style)
        // The transport layer handles dongle flush patterns internally
        connected
            .transport
            .send_command(cmd, &connected.pending.as_ref().unwrap().data, checksum)
            .await
            .map_err(|e| Status::internal(format!("Send error: {}", e)))?;

        Ok(())
    }

    /// Read response to previously sent command
    async fn read_response(&self, device_path: &str) -> Result<Vec<u8>, Status> {
        // Ensure device is open
        self.open_device(device_path).await?;

        let mut devices = self.devices.lock().await;
        let connected = devices
            .get_mut(device_path)
            .ok_or_else(|| Status::not_found("Device not connected"))?;

        // Get pending command
        let pending = connected.pending.take().ok_or_else(|| {
            Status::failed_precondition("No pending command - call send_msg first")
        })?;

        debug!("Executing query for pending command 0x{:02x}", pending.cmd);

        // Use query_command which handles all transport-specific logic
        // (dongle flush, retries, etc.)
        let response = connected
            .transport
            .query_command(pending.cmd, &pending.data, pending.checksum)
            .await
            .map_err(|e| Status::internal(format!("Query error: {}", e)))?;

        debug!(
            "Response for 0x{:02x}: {:02x?}",
            pending.cmd,
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
                Ok(dev) => Some(Ok(DeviceList {
                    dev_list: vec![dev],
                    r#type: DeviceListChangeType::Add as i32,
                })),
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

    async fn get_item_from_db(&self, _request: Request<GetItem>) -> Result<Response<Item>, Status> {
        Ok(Response::new(Item {
            value: vec![],
            err_str: String::new(),
        }))
    }

    async fn insert_db(&self, _request: Request<InsertDb>) -> Result<Response<ResSend>, Status> {
        Ok(Response::new(ResSend { err: String::new() }))
    }

    async fn delete_item_from_db(
        &self,
        _request: Request<DeleteItem>,
    ) -> Result<Response<ResSend>, Status> {
        Ok(Response::new(ResSend { err: String::new() }))
    }

    async fn get_all_keys_from_db(
        &self,
        _request: Request<GetAll>,
    ) -> Result<Response<AllList>, Status> {
        Ok(Response::new(AllList {
            data: vec![],
            err_str: String::new(),
        }))
    }

    async fn get_all_values_from_db(
        &self,
        _request: Request<GetAll>,
    ) -> Result<Response<AllList>, Status> {
        Ok(Response::new(AllList {
            data: vec![],
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

        self.start_vendor_polling();

        let rx = self.vendor_tx.subscribe();
        let stream = BroadcastStream::new(rx).filter_map(|result| async move {
            match result {
                Ok(event) => Some(Ok(event)),
                Err(_) => None,
            }
        });

        Ok(Response::new(Box::pin(stream)))
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
