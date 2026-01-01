// gRPC server implementation for iot_driver compatibility
// Provides the same interface as the original Windows iot_driver.exe

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use futures::{Stream, StreamExt};
use hidapi::{HidApi, HidDevice};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tonic::{Request, Response, Status};
use tracing::{info, warn, error, debug};

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

/// Supported device entry from the original iot_driver database
#[derive(Debug, Clone)]
pub struct DeviceEntry {
    pub vid: u16,
    pub pid: u16,
    pub usage: u16,
    pub usage_page: u16,
    pub interface_number: i32,
    pub _dongle_common: bool,
}

/// Known devices extracted from iot_driver.exe
pub fn get_known_devices() -> Vec<DeviceEntry> {
    vec![
        DeviceEntry { vid: protocol::VENDOR_ID, pid: protocol::PRODUCT_ID_M1_V5_WIRED, usage: protocol::USAGE, usage_page: protocol::USAGE_PAGE, interface_number: protocol::INTERFACE, _dongle_common: false },
        DeviceEntry { vid: protocol::VENDOR_ID, pid: protocol::PRODUCT_ID_M1_V5_WIRELESS, usage: protocol::USAGE, usage_page: protocol::USAGE_PAGE, interface_number: protocol::INTERFACE, _dongle_common: false },
        DeviceEntry { vid: protocol::VENDOR_ID, pid: protocol::PRODUCT_ID_DONGLE_1, usage: protocol::USAGE, usage_page: protocol::USAGE_PAGE, interface_number: protocol::INTERFACE, _dongle_common: true },
        DeviceEntry { vid: protocol::VENDOR_ID, pid: protocol::PRODUCT_ID_DONGLE_2, usage: protocol::USAGE, usage_page: protocol::USAGE_PAGE, interface_number: protocol::INTERFACE, _dongle_common: true },
    ]
}

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

/// Parse device path in format "vid-pid-usage_page-interface"
fn parse_device_path(path: &str) -> Option<(u16, u16, u16, i32)> {
    let parts: Vec<&str> = path.split('-').collect();
    if parts.len() >= 4 {
        let vid = parts[0].parse().ok()?;
        let pid = parts[1].parse().ok()?;
        let usage_page = parts[2].parse().ok()?;
        let interface = parts[3].parse().ok()?;
        Some((vid, pid, usage_page, interface))
    } else {
        None
    }
}

/// Driver service implementation
pub struct DriverService {
    hidapi: Arc<Mutex<HidApi>>,
    devices: Arc<Mutex<HashMap<String, ConnectedDevice>>>,
    device_tx: broadcast::Sender<DjDev>,
    vendor_tx: broadcast::Sender<VenderMsg>,
    vendor_polling: Arc<Mutex<bool>>,
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

    /// Scan for and connect to known devices
    pub fn scan_devices(&self) -> Vec<DjDev> {
        let mut found = Vec::new();
        let known_devices = get_known_devices();
        let hidapi = self.hidapi.lock().unwrap();

        for device_info in hidapi.device_list() {
            let vid = device_info.vendor_id();
            let pid = device_info.product_id();
            let usage_page = device_info.usage_page();
            let usage = device_info.usage();
            let interface = device_info.interface_number();

            for known in &known_devices {
                if vid == known.vid && pid == known.pid
                    && usage_page == known.usage_page
                    && usage == known.usage
                    && interface == known.interface_number {

                    let path = format!("{vid}-{pid}-{usage_page}-{interface}");
                    let hid_path = device_info.path().to_string_lossy().to_string();

                    info!("Found device: VID={:04x} PID={:04x} path={}", vid, pid, hid_path);

                    let device_id = match device_info.open_device(&hidapi) {
                        Ok(hid_device) => {
                            self.query_device_id(&hid_device).unwrap_or(0)
                        }
                        Err(e) => {
                            warn!("Could not open device to query ID: {}", e);
                            0
                        }
                    };

                    let device = Device {
                        dev_type: DeviceType::YzwKeyboard as i32,
                        is24: false,
                        path: path.clone(),
                        id: device_id,
                        battery: 100,
                        is_online: true,
                        vid: vid as u32,
                        pid: pid as u32,
                    };

                    found.push(DjDev {
                        oneof_dev: Some(dj_dev::OneofDev::Dev(device)),
                    });
                }
            }
        }

        found
    }

    #[allow(clippy::result_large_err)]
    fn open_device(&self, device_path: &str) -> Result<(), Status> {
        let (vid, pid, _usage_page, _interface) = parse_device_path(device_path)
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
