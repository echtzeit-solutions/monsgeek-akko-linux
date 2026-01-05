// gRPC server implementation for iot_driver compatibility
// Provides the same interface as the original Windows iot_driver.exe

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use futures::{Stream, StreamExt};
use hidapi::{HidApi, HidDevice};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_udev::{MonitorBuilder, EventType};
use tonic::{Request, Response, Status};
use tracing::{info, warn, error, debug};

use iot_driver::hal::{device_registry, HidInterface};
use iot_driver::protocol::{self, cmd};

#[allow(non_camel_case_types)]  // Proto types use camelCase to match original iot_driver.exe
#[allow(clippy::enum_variant_names)]  // Proto enum variants have Yzw prefix
pub mod driver {
    tonic::include_proto!("driver");
}

pub use driver::driver_grpc_server::{DriverGrpc, DriverGrpcServer};
pub use driver::*;

/// Broadcast channel buffer sizes
const DEVICE_CHANNEL_SIZE: usize = 16;
const VENDOR_CHANNEL_SIZE: usize = 256;

// Device definitions now come from hal::device_registry()

/// Connected HID device handle
struct ConnectedDevice {
    device: HidDevice,
    _vid: u16,
    _pid: u16,
    _path: String,
}

/// Calculate checksum for HID message (uses proto CheckSumType)
fn calculate_checksum(data: &[u8], checksum_type: CheckSumType) -> u8 {
    match checksum_type {
        CheckSumType::Bit7 => {
            let sum: u32 = data.iter().take(7).map(|&b| b as u32).sum();
            (255 - (sum & 0xFF)) as u8
        }
        CheckSumType::Bit8 => {
            let sum: u32 = data.iter().take(8).map(|&b| b as u32).sum();
            (255 - (sum & 0xFF)) as u8
        }
        CheckSumType::None => 0,
    }
}

/// Apply checksum to message
fn apply_checksum(data: &mut [u8], checksum_type: CheckSumType) {
    match checksum_type {
        CheckSumType::Bit7 => {
            if data.len() >= 8 {
                data[7] = calculate_checksum(data, checksum_type);
            }
        }
        CheckSumType::Bit8 => {
            if data.len() >= 9 {
                data[8] = calculate_checksum(data, checksum_type);
            }
        }
        CheckSumType::None => {}
    }
}

/// Parse device path in format "vid-pid-usage_page-usage-interface"
fn parse_device_path(path: &str) -> Option<(u16, u16, u16, u16, i32)> {
    HidInterface::parse_path_key(path)
}

/// Driver service implementation
pub struct DriverService {
    hidapi: Arc<Mutex<HidApi>>,
    devices: Arc<Mutex<HashMap<String, ConnectedDevice>>>,
    device_tx: broadcast::Sender<DjDev>,
    vendor_tx: broadcast::Sender<VenderMsg>,
    vendor_polling: Arc<Mutex<bool>>,
    hotplug_running: Arc<Mutex<bool>>,
}

impl DriverService {
    pub fn new() -> Result<Self, hidapi::HidError> {
        let hidapi = HidApi::new()?;
        let (device_tx, _) = broadcast::channel(DEVICE_CHANNEL_SIZE);
        let (vendor_tx, _) = broadcast::channel(VENDOR_CHANNEL_SIZE);

        Ok(Self {
            hidapi: Arc::new(Mutex::new(hidapi)),
            devices: Arc::new(Mutex::new(HashMap::new())),
            device_tx,
            vendor_tx,
            vendor_polling: Arc::new(Mutex::new(false)),
            hotplug_running: Arc::new(Mutex::new(false)),
        })
    }

    /// Start background polling for vendor events from connected devices
    fn start_vendor_polling(&self) {
        let mut polling = self.vendor_polling.lock().unwrap();
        if *polling {
            return;
        }
        *polling = true;
        drop(polling);

        // Auto-open all detected devices so we can read vendor events
        self.open_all_devices();

        let devices = Arc::clone(&self.devices);
        let vendor_tx = self.vendor_tx.clone();
        let vendor_polling = Arc::clone(&self.vendor_polling);

        tokio::spawn(async move {
            info!("Started vendor event polling");

            loop {
                {
                    let polling = vendor_polling.lock().unwrap();
                    if !*polling {
                        break;
                    }
                }

                let mut got_event = false;
                {
                    let devices_guard = devices.lock().unwrap();
                    for (path, connected) in devices_guard.iter() {
                        let mut buf = [0u8; protocol::INPUT_REPORT_SIZE];
                        match connected.device.read_timeout(&mut buf, 10) {
                            Ok(len) if len > 0 => {
                                debug!("Vendor event from {}: {:02x?}", path, &buf[..std::cmp::min(len, 16)]);
                                let event = VenderMsg {
                                    msg: buf[..len].to_vec(),
                                };
                                let _ = vendor_tx.send(event);
                                got_event = true;
                            }
                            _ => {}
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

    /// Open all detected devices so they're available for vendor polling
    fn open_all_devices(&self) {
        let registry = device_registry();
        let hidapi = self.hidapi.lock().unwrap();
        let mut devices = self.devices.lock().unwrap();

        for device_info in hidapi.device_list() {
            // Check if this device matches any known interface
            if let Some(known) = registry.find_matching(device_info) {
                let path = known.path_key();

                // Skip if already opened
                if devices.contains_key(&path) {
                    continue;
                }

                let vid = device_info.vendor_id();
                let pid = device_info.product_id();
                let usage = device_info.usage();

                match device_info.open_device(&hidapi) {
                    Ok(device) => {
                        info!("Auto-opened device for vendor polling: {} (usage=0x{:02x})", path, usage);
                        devices.insert(path.clone(), ConnectedDevice {
                            device,
                            _vid: vid,
                            _pid: pid,
                            _path: path,
                        });
                    }
                    Err(e) => {
                        warn!("Failed to auto-open device: {}", e);
                    }
                }
            }
        }
    }

    /// Start hot-plug monitoring using udev (runs in a separate thread)
    pub fn start_hotplug_monitor(&self) {
        let mut running = self.hotplug_running.lock().unwrap();
        if *running {
            return;
        }
        *running = true;
        drop(running);

        let hidapi = Arc::clone(&self.hidapi);
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
                            // Refresh hidapi and scan for new devices
                            if let Ok(mut api) = hidapi.lock() {
                                let _ = api.refresh_devices();
                            }
                            // Re-scan and broadcast new devices
                            Self::rescan_devices_static(&hidapi, &devices, &device_tx);
                        }
                        EventType::Remove => {
                            info!("Device removed: {:?}", devnode);
                            // Refresh hidapi
                            if let Ok(mut api) = hidapi.lock() {
                                let _ = api.refresh_devices();
                            }
                            // Clean up disconnected devices
                            Self::cleanup_disconnected_static(&hidapi, &devices, &device_tx);
                        }
                        _ => {}
                    }
                }
            }

            info!("Stopped udev hot-plug monitor");
        });
    }

    /// Static helper to re-scan devices (called from async context)
    fn rescan_devices_static(
        hidapi: &Arc<Mutex<HidApi>>,
        devices: &Arc<Mutex<HashMap<String, ConnectedDevice>>>,
        device_tx: &broadcast::Sender<DjDev>,
    ) {
        let registry = device_registry();
        let api = match hidapi.lock() {
            Ok(api) => api,
            Err(_) => return,
        };
        let mut devs = match devices.lock() {
            Ok(d) => d,
            Err(_) => return,
        };

        for device_info in api.device_list() {
            if let Some(known) = registry.find_matching(device_info) {
                let path = known.path_key();

                if devs.contains_key(&path) {
                    continue;
                }

                let vid = device_info.vendor_id();
                let pid = device_info.product_id();

                match device_info.open_device(&api) {
                    Ok(device) => {
                        info!("Hot-plug: opened new device {}", path);

                        // Query device ID
                        let device_id = Self::query_device_id_static(&device).unwrap_or(0);

                        let dev_info = Device {
                            dev_type: DeviceType::YzwKeyboard as i32,
                            is24: false,
                            path: path.clone(),
                            id: device_id,
                            battery: 100,
                            is_online: true,
                            vid: vid as u32,
                            pid: pid as u32,
                        };

                        // Broadcast new device
                        let _ = device_tx.send(DjDev {
                            oneof_dev: Some(dj_dev::OneofDev::Dev(dev_info)),
                        });

                        devs.insert(path.clone(), ConnectedDevice {
                            device,
                            _vid: vid,
                            _pid: pid,
                            _path: path,
                        });
                    }
                    Err(e) => {
                        warn!("Hot-plug: failed to open device: {}", e);
                    }
                }
            }
        }
    }

    /// Static helper to clean up disconnected devices
    fn cleanup_disconnected_static(
        hidapi: &Arc<Mutex<HidApi>>,
        devices: &Arc<Mutex<HashMap<String, ConnectedDevice>>>,
        _device_tx: &broadcast::Sender<DjDev>,
    ) {
        let registry = device_registry();
        let api = match hidapi.lock() {
            Ok(api) => api,
            Err(_) => return,
        };
        let mut devs = match devices.lock() {
            Ok(d) => d,
            Err(_) => return,
        };

        // Get current device paths from hidapi
        let mut current_paths = std::collections::HashSet::new();

        for device_info in api.device_list() {
            if let Some(known) = registry.find_matching(device_info) {
                current_paths.insert(known.path_key());
            }
        }

        // Remove devices no longer present
        let to_remove: Vec<String> = devs.keys()
            .filter(|path| !current_paths.contains(*path))
            .cloned()
            .collect();

        for path in to_remove {
            info!("Hot-plug: removing disconnected device {}", path);
            devs.remove(&path);
            // TODO: Could broadcast device removal here
        }
    }

    /// Query battery status from 2.4GHz dongle
    /// Returns (battery_level, is_online) if successful
    fn query_dongle_battery(device: &HidDevice) -> Option<(u32, bool)> {
        let mut buf = [0u8; 65];
        buf[0] = 0x05;  // Report ID for vendor interface

        match device.get_feature_report(&mut buf) {
            Ok(len) if len >= 4 => {
                let battery = buf[1] as u32;
                let online = buf[3] != 0;

                // Sanity check battery level
                if battery <= 100 {
                    Some((battery, online))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Static helper to query device ID
    fn query_device_id_static(device: &HidDevice) -> Option<i32> {
        let mut cmd_buf = [0u8; 65];
        cmd_buf[0] = 0;
        cmd_buf[1] = cmd::GET_USB_VERSION;

        let sum: u16 = cmd_buf[1..8].iter().map(|&x| x as u16).sum();
        cmd_buf[8] = 255 - (sum & 0xFF) as u8;

        if device.send_feature_report(&cmd_buf).is_err() {
            return None;
        }

        let mut response = [0u8; 65];
        match device.get_feature_report(&mut response) {
            Ok(len) if len >= 6 && response[1] == cmd::GET_USB_VERSION => {
                let device_id = u32::from_le_bytes([
                    response[2], response[3], response[4], response[5]
                ]) as i32;
                Some(device_id)
            }
            _ => None,
        }
    }

    /// Query device ID using GET_USB_VERSION (0x8F) command
    fn query_device_id(&self, device: &HidDevice) -> Option<i32> {
        let mut cmd_buf = [0u8; 65];
        cmd_buf[0] = 0;
        cmd_buf[1] = cmd::GET_USB_VERSION;

        let sum: u16 = cmd_buf[1..8].iter().map(|&x| x as u16).sum();
        cmd_buf[8] = 255 - (sum & 0xFF) as u8;

        for attempt in 0..3 {
            match device.send_feature_report(&cmd_buf) {
                Ok(_) => break,
                Err(e) => {
                    if attempt == 2 {
                        warn!("Failed to send device ID query after 3 attempts: {}", e);
                        return None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }

        let mut response = [0u8; 65];
        match device.get_feature_report(&mut response) {
            Ok(len) if len >= 6 => {
                if response[1] == cmd::GET_USB_VERSION {
                    let device_id = u32::from_le_bytes([
                        response[2], response[3], response[4], response[5]
                    ]) as i32;
                    info!("Device ID: {}", device_id);
                    return Some(device_id);
                }
            }
            Ok(len) => {
                warn!("Short response from device ID query: {} bytes", len);
            }
            Err(e) => {
                warn!("Failed to read device ID: {}", e);
            }
        }
        None
    }

    /// Scan for and connect to known devices (only returns client-facing interfaces)
    pub fn scan_devices(&self) -> Vec<DjDev> {
        let mut found = Vec::new();
        let registry = device_registry();
        let hidapi = self.hidapi.lock().unwrap();

        for device_info in hidapi.device_list() {
            // Check if device matches a known interface
            if let Some(known) = registry.find_matching(device_info) {
                // Only report client-facing interfaces (FEATURE, not INPUT)
                if !known.is_client_facing() {
                    continue;
                }

                let vid = device_info.vendor_id();
                let pid = device_info.product_id();
                let path = known.path_key();
                let hid_path = device_info.path().to_string_lossy().to_string();

                info!("Found device: VID={:04x} PID={:04x} path={}", vid, pid, hid_path);

                // Check if this is a 2.4GHz dongle
                let is_dongle = pid == 0x5038;

                let (device_id, battery, is_online) = match device_info.open_device(&hidapi) {
                    Ok(hid_device) => {
                        let id = self.query_device_id(&hid_device).unwrap_or(0);

                        // Query battery status for dongles
                        let (batt, online) = if is_dongle {
                            Self::query_dongle_battery(&hid_device)
                                .unwrap_or((100, true))
                        } else {
                            (100, true)  // Wired devices always "online" with "full" battery
                        };

                        (id, batt, online)
                    }
                    Err(e) => {
                        warn!("Could not open device to query ID: {}", e);
                        (0, 100, true)
                    }
                };

                let device = Device {
                    dev_type: DeviceType::YzwKeyboard as i32,
                    is24: is_dongle,
                    path: path.clone(),
                    id: device_id,
                    battery,
                    is_online,
                    vid: vid as u32,
                    pid: pid as u32,
                };

                found.push(DjDev {
                    oneof_dev: Some(dj_dev::OneofDev::Dev(device)),
                });
            }
        }

        found
    }

    #[allow(clippy::result_large_err)]
    fn open_device(&self, device_path: &str) -> Result<(), Status> {
        let (vid, pid, _usage_page, _usage, _interface) = parse_device_path(device_path)
            .ok_or_else(|| Status::invalid_argument("Invalid device path format"))?;

        let hidapi = self.hidapi.lock().unwrap();

        for device_info in hidapi.device_list() {
            if device_info.vendor_id() == vid && device_info.product_id() == pid {
                match device_info.open_device(&hidapi) {
                    Ok(device) => {
                        let connected = ConnectedDevice {
                            device,
                            _vid: vid,
                            _pid: pid,
                            _path: device_path.to_string(),
                        };
                        self.devices.lock().unwrap().insert(device_path.to_string(), connected);
                        info!("Opened device: {}", device_path);
                        return Ok(());
                    }
                    Err(e) => {
                        warn!("Failed to open device {}: {}", device_path, e);
                        return Err(Status::internal(format!("Failed to open device: {e}")));
                    }
                }
            }
        }

        Err(Status::not_found("Device not found"))
    }

    #[allow(clippy::result_large_err)]
    fn send_feature(&self, device_path: &str, data: &[u8]) -> Result<(), Status> {
        {
            let devices = self.devices.lock().unwrap();
            if !devices.contains_key(device_path) {
                drop(devices);
                self.open_device(device_path)?;
            }
        }

        let devices = self.devices.lock().unwrap();
        let connected = devices.get(device_path)
            .ok_or_else(|| Status::not_found("Device not connected"))?;

        let mut buf = vec![0u8; 65];
        buf[0] = 0;
        let len = std::cmp::min(data.len(), 64);
        buf[1..1+len].copy_from_slice(&data[..len]);

        debug!("Sending feature report: {:02x?}", &buf[..9]);

        match connected.device.send_feature_report(&buf) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to send feature report: {}", e);
                Err(Status::internal(format!("HID error: {e}")))
            }
        }
    }

    #[allow(clippy::result_large_err)]
    fn read_feature(&self, device_path: &str) -> Result<Vec<u8>, Status> {
        {
            let devices = self.devices.lock().unwrap();
            if !devices.contains_key(device_path) {
                drop(devices);
                self.open_device(device_path)?;
            }
        }

        let devices = self.devices.lock().unwrap();
        let connected = devices.get(device_path)
            .ok_or_else(|| Status::not_found("Device not connected"))?;

        let mut buf = vec![0u8; 65];
        buf[0] = 0;

        match connected.device.get_feature_report(&mut buf) {
            Ok(len) => {
                debug!("Received feature report ({} bytes): {:02x?}", len, &buf[..std::cmp::min(len, 9)]);
                Ok(buf[1..].to_vec())
            }
            Err(e) => {
                error!("Failed to read feature report: {}", e);
                Err(Status::internal(format!("HID error: {e}")))
            }
        }
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

        let initial_devices = self.scan_devices();
        info!("Sending {} initial devices to client", initial_devices.len());

        let rx = self.device_tx.subscribe();

        let initial_list = DeviceList {
            dev_list: initial_devices,
            r#type: DeviceListChangeType::Init as i32,
        };

        let initial_stream = futures::stream::iter(std::iter::once(Ok(initial_list)));

        let broadcast_stream = BroadcastStream::new(rx)
            .filter_map(|result| async move {
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
        debug!("send_raw_feature: path={}, {} bytes", msg.device_path, msg.msg.len());

        match self.send_feature(&msg.device_path, &msg.msg) {
            Ok(_) => Ok(Response::new(ResSend { err: String::new() })),
            Err(e) => Ok(Response::new(ResSend { err: e.message().to_string() })),
        }
    }

    async fn read_raw_feature(
        &self,
        request: Request<ReadMsg>,
    ) -> Result<Response<ResRead>, Status> {
        let msg = request.into_inner();
        debug!("read_raw_feature: path={}", msg.device_path);

        match self.read_feature(&msg.device_path) {
            Ok(data) => Ok(Response::new(ResRead { err: String::new(), msg: data })),
            Err(e) => Ok(Response::new(ResRead { err: e.message().to_string(), msg: vec![] })),
        }
    }

    async fn send_msg(
        &self,
        request: Request<SendMsg>,
    ) -> Result<Response<ResSend>, Status> {
        let msg = request.into_inner();
        debug!("send_msg: path={}, checksum={:?}", msg.device_path, msg.check_sum_type);

        let mut data = msg.msg.clone();
        if data.len() < 64 {
            data.resize(64, 0);
        }

        let checksum_type = CheckSumType::try_from(msg.check_sum_type).unwrap_or(CheckSumType::Bit7);
        apply_checksum(&mut data, checksum_type);

        match self.send_feature(&msg.device_path, &data) {
            Ok(_) => Ok(Response::new(ResSend { err: String::new() })),
            Err(e) => Ok(Response::new(ResSend { err: e.message().to_string() })),
        }
    }

    async fn read_msg(
        &self,
        request: Request<ReadMsg>,
    ) -> Result<Response<ResRead>, Status> {
        let msg = request.into_inner();
        debug!("read_msg: path={}", msg.device_path);

        match self.read_feature(&msg.device_path) {
            Ok(data) => Ok(Response::new(ResRead { err: String::new(), msg: data })),
            Err(e) => Ok(Response::new(ResRead { err: e.message().to_string(), msg: vec![] })),
        }
    }

    async fn get_item_from_db(
        &self,
        _request: Request<GetItem>,
    ) -> Result<Response<Item>, Status> {
        Ok(Response::new(Item { value: vec![], err_str: String::new() }))
    }

    async fn insert_db(
        &self,
        _request: Request<InsertDb>,
    ) -> Result<Response<ResSend>, Status> {
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
        Ok(Response::new(AllList { data: vec![], err_str: String::new() }))
    }

    async fn get_all_values_from_db(
        &self,
        _request: Request<GetAll>,
    ) -> Result<Response<AllList>, Status> {
        Ok(Response::new(AllList { data: vec![], err_str: String::new() }))
    }

    async fn get_version(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Version>, Status> {
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
        Ok(Response::new(MicrophoneMuteStatus { is_mute: false, err: String::new() }))
    }

    async fn get_microphone_mute(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<MicrophoneMuteStatus>, Status> {
        Ok(Response::new(MicrophoneMuteStatus { is_mute: false, err: String::new() }))
    }

    async fn change_wireless_loop_status(
        &self,
        _request: Request<WirelessLoopStatus>,
    ) -> Result<Response<ResSend>, Status> {
        Ok(Response::new(ResSend { err: String::new() }))
    }

    async fn set_light_type(
        &self,
        _request: Request<SetLight>,
    ) -> Result<Response<Empty>, Status> {
        Ok(Response::new(Empty {}))
    }

    async fn watch_vender(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::watchVenderStream>, Status> {
        info!("watch_vender called - starting vendor event stream");

        self.start_vendor_polling();

        let rx = self.vendor_tx.subscribe();
        let stream = BroadcastStream::new(rx)
            .filter_map(|result| async move {
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
        Ok(Response::new(WeatherRes { res: "{}".to_string() }))
    }
}
