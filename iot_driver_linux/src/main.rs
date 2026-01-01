use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use futures::{Stream, StreamExt};
use hidapi::{HidApi, HidDevice};
use http::header::{ACCEPT, CONTENT_TYPE};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tonic::{transport::Server, Request, Response, Status};
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn, error, debug};

// Use shared protocol definitions from library
use iot_driver::protocol::{self, cmd};

pub mod driver {
    tonic::include_proto!("driver");
}

use driver::driver_grpc_server::{DriverGrpc, DriverGrpcServer};
use driver::*;

/// Supported device entry from the original iot_driver database
#[derive(Debug, Clone)]
struct DeviceEntry {
    vid: u16,
    pid: u16,
    usage: u16,
    usage_page: u16,
    interface_number: i32,
    dongle_common: bool,
}

/// Known devices extracted from iot_driver.exe
fn get_known_devices() -> Vec<DeviceEntry> {
    vec![
        // MonsGeek M1 V5 HE (our keyboard)
        DeviceEntry { vid: protocol::VENDOR_ID, pid: protocol::PRODUCT_ID_M1_V5_WIRED, usage: protocol::USAGE, usage_page: protocol::USAGE_PAGE, interface_number: protocol::INTERFACE, dongle_common: false },
        // Additional devices from iot_driver_devices.txt
        DeviceEntry { vid: protocol::VENDOR_ID, pid: protocol::PRODUCT_ID_M1_V5_WIRELESS, usage: protocol::USAGE, usage_page: protocol::USAGE_PAGE, interface_number: protocol::INTERFACE, dongle_common: false },
        DeviceEntry { vid: protocol::VENDOR_ID, pid: protocol::PRODUCT_ID_DONGLE_1, usage: protocol::USAGE, usage_page: protocol::USAGE_PAGE, interface_number: protocol::INTERFACE, dongle_common: true },
        DeviceEntry { vid: protocol::VENDOR_ID, pid: protocol::PRODUCT_ID_DONGLE_2, usage: protocol::USAGE, usage_page: protocol::USAGE_PAGE, interface_number: protocol::INTERFACE, dongle_common: true },
    ]
}

/// Connected HID device handle
struct ConnectedDevice {
    device: HidDevice,
    vid: u16,
    pid: u16,
    path: String,
}

/// Calculate checksum for HID message
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
        let (device_tx, _) = broadcast::channel(16);
        let (vendor_tx, _) = broadcast::channel(256); // Larger buffer for frequent events

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
            return; // Already polling
        }
        *polling = true;
        drop(polling);

        let devices = Arc::clone(&self.devices);
        let vendor_tx = self.vendor_tx.clone();
        let vendor_polling = Arc::clone(&self.vendor_polling);

        tokio::spawn(async move {
            info!("Started vendor event polling");

            loop {
                // Check if we should stop polling
                {
                    let polling = vendor_polling.lock().unwrap();
                    if !*polling {
                        break;
                    }
                }

                // Try to read from any connected device
                let mut got_event = false;
                {
                    let devices_guard = devices.lock().unwrap();
                    for (path, connected) in devices_guard.iter() {
                        // Non-blocking read with short timeout
                        let mut buf = [0u8; protocol::INPUT_REPORT_SIZE];
                        match connected.device.read_timeout(&mut buf, 10) {
                            Ok(len) if len > 0 => {
                                debug!("Vendor event from {}: {:02x?}", path, &buf[..std::cmp::min(len, 16)]);
                                let event = VenderMsg {
                                    msg: buf[..len].to_vec(),
                                };
                                if vendor_tx.send(event).is_err() {
                                    // No receivers, but that's ok
                                }
                                got_event = true;
                            }
                            _ => {}
                        }
                    }
                }

                // If no events, sleep a bit to avoid busy loop
                if !got_event {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            }

            info!("Stopped vendor event polling");
        });
    }

    /// Query device ID using GET_USB_VERSION (0x8F) command
    fn query_device_id(&self, device: &HidDevice) -> Option<i32> {
        // Send GET_USB_VERSION command (0x8F)
        let mut cmd = [0u8; 65];
        cmd[0] = 0; // Report ID
        cmd[1] = cmd::GET_USB_VERSION;

        // Calculate checksum (Bit7 type)
        let sum: u16 = cmd[1..8].iter().map(|&x| x as u16).sum();
        cmd[8] = (255 - (sum & 0xFF) as u8) as u8;

        // Try sending with retries (Linux hidraw quirk)
        for attempt in 0..3 {
            match device.send_feature_report(&cmd) {
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

        // Read response
        let mut response = [0u8; 65];
        match device.get_feature_report(&mut response) {
            Ok(len) if len >= 6 => {
                // Response format: [report_id, cmd_echo, id_byte0, id_byte1, id_byte2, id_byte3, ...]
                // Device ID is little-endian uint32 at bytes 1-4 (after report ID)
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

            // Check if this matches a known device
            for known in &known_devices {
                if vid == known.vid && pid == known.pid
                    && usage_page == known.usage_page
                    && usage == known.usage
                    && interface == known.interface_number {

                    let path = format!("{}-{}-{}-{}", vid, pid, usage_page, interface);
                    let hid_path = device_info.path().to_string_lossy().to_string();

                    info!("Found device: VID={:04x} PID={:04x} path={}", vid, pid, hid_path);

                    // Query device ID by opening temporarily
                    let device_id = match device_info.open_device(&hidapi) {
                        Ok(hid_device) => {
                            match self.query_device_id(&hid_device) {
                                Some(real_id) => {
                                    info!("Device ID: {}", real_id);
                                    real_id
                                }
                                None => {
                                    warn!("Could not query device ID, using fallback 0");
                                    0
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Could not open device to query ID: {}", e);
                            0
                        }
                    };

                    // Build device proto with webapp-compatible format
                    let device = Device {
                        dev_type: DeviceType::YzwKeyboard as i32,
                        is24: false,  // Wired USB device
                        path: path.clone(),
                        id: device_id,
                        battery: 100,  // Assume full for wired
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

    /// Open a device by path
    fn open_device(&self, device_path: &str) -> Result<(), Status> {
        let (vid, pid, _usage_page, _interface) = parse_device_path(device_path)
            .ok_or_else(|| Status::invalid_argument("Invalid device path format"))?;

        let hidapi = self.hidapi.lock().unwrap();

        // Find the device in the HID list
        for device_info in hidapi.device_list() {
            if device_info.vendor_id() == vid && device_info.product_id() == pid {
                match device_info.open_device(&hidapi) {
                    Ok(device) => {
                        let connected = ConnectedDevice {
                            device,
                            vid,
                            pid,
                            path: device_path.to_string(),
                        };

                        self.devices.lock().unwrap().insert(device_path.to_string(), connected);
                        info!("Opened device: {}", device_path);
                        return Ok(());
                    }
                    Err(e) => {
                        warn!("Failed to open device {}: {}", device_path, e);
                        return Err(Status::internal(format!("Failed to open device: {}", e)));
                    }
                }
            }
        }

        Err(Status::not_found("Device not found"))
    }

    /// Send feature report to device
    fn send_feature(&self, device_path: &str, data: &[u8]) -> Result<(), Status> {
        // Try to open device if not already open
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

        // Prepare 64-byte buffer with report ID 0
        let mut buf = vec![0u8; 65]; // Report ID + 64 bytes
        buf[0] = 0; // Report ID
        let len = std::cmp::min(data.len(), 64);
        buf[1..1+len].copy_from_slice(&data[..len]);

        debug!("Sending feature report: {:02x?}", &buf[..9]);

        match connected.device.send_feature_report(&buf) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to send feature report: {}", e);
                Err(Status::internal(format!("HID error: {}", e)))
            }
        }
    }

    /// Read feature report from device
    fn read_feature(&self, device_path: &str) -> Result<Vec<u8>, Status> {
        // Try to open device if not already open
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

        let mut buf = vec![0u8; 65]; // Report ID + 64 bytes
        buf[0] = 0; // Report ID

        match connected.device.get_feature_report(&mut buf) {
            Ok(len) => {
                debug!("Received feature report ({} bytes): {:02x?}", len, &buf[..std::cmp::min(len, 9)]);
                Ok(buf[1..].to_vec()) // Skip report ID
            }
            Err(e) => {
                error!("Failed to read feature report: {}", e);
                Err(Status::internal(format!("HID error: {}", e)))
            }
        }
    }
}

#[tonic::async_trait]
impl DriverGrpc for DriverService {
    type watchDevListStream = Pin<Box<dyn Stream<Item = Result<DeviceList, Status>> + Send>>;
    type watchSystemInfoStream = Pin<Box<dyn Stream<Item = Result<SystemInfo, Status>> + Send>>;
    type upgradeOTAGATTStream = Pin<Box<dyn Stream<Item = Result<Progress, Status>> + Send>>;
    type watchVenderStream = Pin<Box<dyn Stream<Item = Result<VenderMsg, Status>> + Send>>;  // Note: typo matches original

    async fn watch_dev_list(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::watchDevListStream>, Status> {
        info!("watch_dev_list called");

        // Get currently connected devices
        let initial_devices = self.scan_devices();
        info!("Sending {} initial devices to client", initial_devices.len());

        // Subscribe FIRST, then we'll send initial devices
        let rx = self.device_tx.subscribe();

        // Create DeviceList containing all initial devices
        let initial_list = DeviceList {
            dev_list: initial_devices,
            r#type: DeviceListChangeType::Init as i32,
        };

        // Create stream that first yields initial device list, then broadcast updates
        let initial_stream = futures::stream::iter(
            std::iter::once(Ok(initial_list))
        );

        let broadcast_stream = BroadcastStream::new(rx)
            .filter_map(|result| async move {
                match result {
                    Ok(dev) => {
                        // Wrap single device update in DeviceList
                        Some(Ok(DeviceList {
                            dev_list: vec![dev],
                            r#type: DeviceListChangeType::Add as i32,
                        }))
                    },
                    Err(_) => None,
                }
            });

        // Chain: initial device list first, then live updates
        let combined = initial_stream.chain(broadcast_stream);

        Ok(Response::new(Box::pin(combined)))
    }

    async fn watch_system_info(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::watchSystemInfoStream>, Status> {
        info!("watch_system_info called");
        let stream = futures::stream::empty();
        Ok(Response::new(Box::pin(stream)))
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

        // Apply checksum if needed
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
        // Stub implementation
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
            base_version: "222".to_string(), // Match original IOTVersion
            time_stamp: "2024-12-29".to_string(),
        }))
    }

    async fn upgrade_otagatt(
        &self,
        _request: Request<OtaUpgrade>,
    ) -> Result<Response<Self::upgradeOTAGATTStream>, Status> {
        let stream = futures::stream::empty();
        Ok(Response::new(Box::pin(stream)))
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

        // Start polling if not already running
        self.start_vendor_polling();

        // Subscribe to vendor events
        let rx = self.vendor_tx.subscribe();
        let stream = BroadcastStream::new(rx)
            .filter_map(|result| async move {
                match result {
                    Ok(event) => Some(Ok(event)),
                    Err(_) => None, // Lagged or closed
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

/// CLI test function - send a command and print response
/// Uses retry pattern due to Linux HID feature report buffering
fn cli_test(hidapi: &HidApi, cmd: u8) -> Result<(), Box<dyn std::error::Error>> {
    let known_devices = get_known_devices();

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

                println!("Found device: VID={:04x} PID={:04x} path={}",
                    vid, pid, device_info.path().to_string_lossy());

                let device = device_info.open_device(hidapi)?;

                // Prepare command with checksum (Bit7 mode)
                let mut buf = vec![0u8; 65];
                buf[0] = 0; // Report ID
                buf[1] = cmd; // Command byte
                // Checksum at byte 7 of payload (buf[8])
                let sum: u32 = buf[1..8].iter().map(|&b| b as u32).sum();
                buf[8] = (255 - (sum & 0xFF)) as u8;

                println!("Sending command 0x{:02x} ({})...", cmd, cmd::name(cmd));
                println!("  TX: {:02x?}", &buf[1..12]);

                // Linux HID feature reports have buffering - need retry pattern
                // Send command, wait, then retry reading until we get our response
                const MAX_RETRIES: usize = 5;
                let mut resp = vec![0u8; 65];
                let mut success = false;

                for attempt in 0..MAX_RETRIES {
                    // Send the command
                    device.send_feature_report(&buf)?;
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    // Read response
                    resp[0] = 0;
                    let _len = device.get_feature_report(&mut resp)?;

                    let cmd_echo = resp[1];
                    println!("  Attempt {}: echo=0x{:02x} data={:02x?}",
                        attempt + 1, cmd_echo, &resp[1..12]);

                    if cmd_echo == cmd {
                        success = true;
                        break;
                    }
                }

                if !success {
                    println!("\nFailed to get response after {} attempts", MAX_RETRIES);
                    return Err("No valid response".into());
                }

                println!("\nResponse (0x{:02x} = {}):", resp[1], cmd::name(resp[1]));

                // Parse based on command type
                match cmd {
                    cmd::GET_USB_VERSION => {
                        // Device ID at bytes 2-5 (little-endian uint32)
                        let device_id = (resp[2] as u32)
                            | ((resp[3] as u32) << 8)
                            | ((resp[4] as u32) << 16)
                            | ((resp[5] as u32) << 24);
                        // Version at bytes 8-9 (little-endian uint16)
                        let version = (resp[8] as u16) | ((resp[9] as u16) << 8);
                        println!("  Device ID:  {} (0x{:04X})", device_id, device_id);
                        println!("  Version:    {} (v{}.{:02})", version, version / 100, version % 100);
                    }
                    cmd::GET_PROFILE => {
                        println!("  Profile:    {}", resp[2]);
                    }
                    cmd::GET_DEBOUNCE => {
                        println!("  Debounce:   {} ms", resp[2]);
                    }
                    cmd::GET_LEDPARAM => {
                        let mode = resp[2];
                        let brightness = resp[3];
                        let speed = protocol::LED_SPEED_MAX - resp[4].min(protocol::LED_SPEED_MAX);
                        let options = resp[5];
                        let r = resp[6];
                        let g = resp[7];
                        let b = resp[8];
                        let dazzle = (options & protocol::LED_OPTIONS_MASK) == protocol::LED_DAZZLE_ON;
                        println!("  LED Mode:   {} ({})", mode, cmd::led_mode_name(mode));
                        println!("  Brightness: {}/4", brightness);
                        println!("  Speed:      {}/4", speed);
                        println!("  Color RGB:  ({}, {}, {}) #{:02X}{:02X}{:02X}", r, g, b, r, g, b);
                        if dazzle {
                            println!("  Dazzle:     ON (rainbow cycle)");
                        }
                    }
                    cmd::GET_KBOPTION => {
                        println!("  Fn Layer:   {}", resp[3]);
                        println!("  Anti-ghost: {}", resp[4]);
                        println!("  RTStab:     {} ms", resp[5] as u32 * 25);
                        println!("  WASD Swap:  {}", resp[6]);
                    }
                    cmd::GET_FEATURE_LIST => {
                        println!("  Features:   {:02x?}", &resp[2..12]);
                        let precision = match resp[3] {
                            0 => "0.1mm",
                            1 => "0.05mm",
                            2 => "0.01mm",
                            _ => "unknown",
                        };
                        println!("  Precision:  {}", precision);
                    }
                    cmd::GET_SLEEPTIME => {
                        let sleep_s = (resp[2] as u16) | ((resp[3] as u16) << 8);
                        println!("  Sleep:      {} seconds ({} min)", sleep_s, sleep_s / 60);
                    }
                    _ => {
                        println!("  Raw data:   {:02x?}", &resp[1..17]);
                    }
                }

                return Ok(());
            }
        }
    }

    Err("No compatible device found".into())
}

/// List all HID devices
fn cli_list(hidapi: &HidApi) {
    println!("All HID devices:");
    for device_info in hidapi.device_list() {
        println!("  VID={:04x} PID={:04x} usage={:04x} page={:04x} if={} path={}",
            device_info.vendor_id(),
            device_info.product_id(),
            device_info.usage(),
            device_info.usage_page(),
            device_info.interface_number(),
            device_info.path().to_string_lossy()
        );
    }
}

fn print_help() {
    eprintln!("MonsGeek M1 V5 HE Linux Driver");
    eprintln!();
    eprintln!("Usage: iot_driver <command>");
    eprintln!();
    eprintln!("Query Commands:");
    eprintln!("  info, version    Get device ID and firmware version");
    eprintln!("  profile          Get current profile (0-3)");
    eprintln!("  led              Get LED settings (mode, brightness, speed, color)");
    eprintln!("  debounce         Get debounce time (ms)");
    eprintln!("  options          Get keyboard options (Fn layer, WASD swap, etc.)");
    eprintln!("  features         Get supported features and precision");
    eprintln!("  sleep            Get sleep timeout");
    eprintln!("  all              Show all device information");
    eprintln!();
    eprintln!("Set Commands:");
    eprintln!("  set-profile <0-3>              Set active profile");
    eprintln!("  set-debounce <ms>              Set debounce time (0-50)");
    eprintln!("  set-led <mode> [b] [s] [r g b] Set LED (mode 0-25, brightness, speed, RGB)");
    eprintln!("  set-sleep <seconds>            Set sleep timeout");
    eprintln!("  reset                          Factory reset keyboard");
    eprintln!("  calibrate                      Run calibration (min + max)");
    eprintln!();
    eprintln!("Trigger Commands (Hall Effect):");
    eprintln!("  triggers                       Show current trigger settings");
    eprintln!("  set-actuation <mm>             Set actuation point for all keys");
    eprintln!("  set-rt <on|off|mm>             Enable/disable Rapid Trigger (or set sensitivity)");
    eprintln!();
    eprintln!("Other Commands:");
    eprintln!("  list             List all HID devices");
    eprintln!("  raw <hex>        Send raw command byte (hex)");
    eprintln!("  serve            Run gRPC server on port 3814");
    eprintln!("  tui              Run interactive terminal UI");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  iot_driver info");
    eprintln!("  iot_driver set-profile 1");
    eprintln!("  iot_driver set-led 2 4 2 255 0 128   # Breathing, max bright, mid speed, purple");
    eprintln!("  iot_driver raw 8f");
}

/// Run multiple commands and show all info
fn cli_all(hidapi: &HidApi) -> Result<(), Box<dyn std::error::Error>> {
    println!("MonsGeek M1 V5 HE - Device Information");
    println!("======================================\n");

    // Get all info
    let commands = [
        (cmd::GET_USB_VERSION, "Device Info"),
        (cmd::GET_PROFILE, "Profile"),
        (cmd::GET_DEBOUNCE, "Debounce"),
        (cmd::GET_LEDPARAM, "LED"),
        (cmd::GET_KBOPTION, "Options"),
        (cmd::GET_FEATURE_LIST, "Features"),
    ];

    for (cmd_byte, _name) in commands {
        let _ = cli_test(hidapi, cmd_byte);
        println!();
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // CLI mode
    if args.len() > 1 {
        let hidapi = HidApi::new()?;

        match args[1].to_lowercase().as_str() {
            // Query commands with friendly names
            "info" | "version" | "ver" | "v" => {
                cli_test(&hidapi, cmd::GET_USB_VERSION)?;
                return Ok(());
            }
            "profile" | "prof" | "p" => {
                cli_test(&hidapi, cmd::GET_PROFILE)?;
                return Ok(());
            }
            "led" | "light" | "l" => {
                cli_test(&hidapi, cmd::GET_LEDPARAM)?;
                return Ok(());
            }
            "debounce" | "deb" | "d" => {
                cli_test(&hidapi, cmd::GET_DEBOUNCE)?;
                return Ok(());
            }
            "options" | "opts" | "opt" | "o" => {
                cli_test(&hidapi, cmd::GET_KBOPTION)?;
                return Ok(());
            }
            "features" | "feat" | "f" => {
                cli_test(&hidapi, cmd::GET_FEATURE_LIST)?;
                return Ok(());
            }
            "sleep" | "s" => {
                cli_test(&hidapi, cmd::GET_SLEEPTIME)?;
                return Ok(());
            }
            "all" | "a" => {
                cli_all(&hidapi)?;
                return Ok(());
            }

            // SET commands
            "set-profile" | "sp" => {
                if args.len() < 3 {
                    eprintln!("Usage: iot_driver set-profile <0-3>");
                    return Ok(());
                }
                let profile: u8 = args[2].parse().unwrap_or(0);
                if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                    if device.set_profile(profile) {
                        println!("Profile set to {}", profile);
                    } else {
                        eprintln!("Failed to set profile");
                    }
                } else {
                    eprintln!("No device found");
                }
                return Ok(());
            }
            "set-debounce" | "sd" => {
                if args.len() < 3 {
                    eprintln!("Usage: iot_driver set-debounce <ms>");
                    return Ok(());
                }
                let ms: u8 = args[2].parse().unwrap_or(0);
                if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                    if device.set_debounce(ms) {
                        println!("Debounce set to {} ms", ms);
                    } else {
                        eprintln!("Failed to set debounce");
                    }
                } else {
                    eprintln!("No device found");
                }
                return Ok(());
            }
            "set-led" | "sl" => {
                if args.len() < 3 {
                    eprintln!("Usage: iot_driver set-led <mode> [brightness] [speed] [r g b]");
                    eprintln!("  mode: 0-25 (0=off, 1=constant, 2=breathing, etc.)");
                    eprintln!("  brightness: 0-4 (default: 4)");
                    eprintln!("  speed: 0-4 (default: 2)");
                    eprintln!("  r g b: 0-255 (default: 255 255 255)");
                    return Ok(());
                }
                let mode: u8 = args[2].parse().unwrap_or(1);
                let brightness: u8 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4);
                let speed: u8 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(2);
                let r: u8 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(255);
                let g: u8 = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(255);
                let b: u8 = args.get(7).and_then(|s| s.parse().ok()).unwrap_or(255);
                if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                    if device.set_led(mode, brightness, speed, r, g, b, false) {
                        println!("LED set: mode={} ({}) brightness={} speed={} color=#{:02X}{:02X}{:02X}",
                            mode, cmd::led_mode_name(mode), brightness, speed, r, g, b);
                    } else {
                        eprintln!("Failed to set LED");
                    }
                } else {
                    eprintln!("No device found");
                }
                return Ok(());
            }
            "set-sleep" | "ss" => {
                if args.len() < 3 {
                    eprintln!("Usage: iot_driver set-sleep <seconds>");
                    return Ok(());
                }
                let seconds: u16 = args[2].parse().unwrap_or(300);
                if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                    if device.set_sleep(seconds, seconds) {
                        println!("Sleep timeout set to {} seconds ({} min)", seconds, seconds / 60);
                    } else {
                        eprintln!("Failed to set sleep timeout");
                    }
                } else {
                    eprintln!("No device found");
                }
                return Ok(());
            }
            "reset" => {
                print!("This will factory reset the keyboard. Are you sure? (y/N) ");
                use std::io::{self, Write};
                io::stdout().flush().unwrap();
                let mut input = String::new();
                io::stdin().read_line(&mut input).unwrap();
                if input.trim().to_lowercase() == "y" {
                    if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                        if device.reset() {
                            println!("Keyboard reset to factory defaults");
                        } else {
                            eprintln!("Failed to reset keyboard");
                        }
                    } else {
                        eprintln!("No device found");
                    }
                } else {
                    println!("Reset cancelled");
                }
                return Ok(());
            }
            "calibrate" | "cal" => {
                if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                    println!("Starting calibration...");
                    println!("Step 1: Calibrating minimum (released) position...");
                    println!("        Keep all keys released!");
                    device.calibrate_min(true);
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    device.calibrate_min(false);
                    println!("        Done.");
                    println!();
                    println!("Step 2: Calibrating maximum (pressed) position...");
                    println!("        Press and hold ALL keys firmly for 3 seconds!");
                    device.calibrate_max(true);
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    device.calibrate_max(false);
                    println!("        Done.");
                    println!();
                    println!("Calibration complete!");
                } else {
                    eprintln!("No device found");
                }
                return Ok(());
            }

            // Trigger commands
            "triggers" | "get-triggers" | "gt" => {
                if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                    let info = device.read_info();
                    let factor = iot_driver::hid::MonsGeekDevice::precision_factor_from_version(info.version);
                    println!("Trigger Settings (firmware v{}, precision: {})",
                        info.version,
                        iot_driver::hid::MonsGeekDevice::precision_str_from_version(info.version));
                    println!();

                    if let Some(triggers) = device.get_all_triggers() {
                        // Decode 16-bit travel values (little-endian, 2 bytes per key)
                        let decode_u16 = |data: &[u8], idx: usize| -> u16 {
                            if idx * 2 + 1 < data.len() {
                                u16::from_le_bytes([data[idx * 2], data[idx * 2 + 1]])
                            } else {
                                0
                            }
                        };

                        let first_press = decode_u16(&triggers.press_travel, 0);
                        let first_lift = decode_u16(&triggers.lift_travel, 0);
                        let first_rt_press = decode_u16(&triggers.rt_press, 0);
                        let first_rt_lift = decode_u16(&triggers.rt_lift, 0);
                        let first_mode = triggers.key_modes.first().copied().unwrap_or(0);

                        let num_keys = triggers.key_modes.len().min(triggers.press_travel.len() / 2);

                        println!("First key settings (as sample):");
                        println!("  Actuation:     {:.1}mm (raw: {})", first_press as f32 / factor, first_press);
                        println!("  Release:       {:.1}mm (raw: {})", first_lift as f32 / factor, first_lift);
                        println!("  RT Press:      {:.2}mm (raw: {})", first_rt_press as f32 / factor, first_rt_press);
                        println!("  RT Release:    {:.2}mm (raw: {})", first_rt_lift as f32 / factor, first_rt_lift);
                        println!("  Mode:          {} ({})", first_mode,
                            protocol::magnetism::mode_name(first_mode));
                        println!();

                        // Check if all keys have same settings
                        let all_same_press = (0..num_keys).all(|i| decode_u16(&triggers.press_travel, i) == first_press);
                        let all_same_mode = triggers.key_modes.iter().take(num_keys).all(|&v| v == first_mode);

                        if all_same_press && all_same_mode {
                            println!("All {} keys have identical settings", num_keys);
                        } else {
                            println!("Keys have varying settings ({} keys total)", num_keys);
                            // Show first 10 different keys
                            println!("\nFirst 10 key values:");
                            for i in 0..10.min(num_keys) {
                                let press = decode_u16(&triggers.press_travel, i);
                                let mode = triggers.key_modes.get(i).copied().unwrap_or(0);
                                println!("  Key {:2}: {:.1}mm mode={}", i, press as f32 / factor, mode);
                            }
                        }
                    } else {
                        eprintln!("Failed to read trigger settings");
                    }
                } else {
                    eprintln!("No device found");
                }
                return Ok(());
            }
            "set-actuation" | "sa" => {
                if args.len() < 3 {
                    eprintln!("Usage: iot_driver set-actuation <mm>");
                    eprintln!("  Example: iot_driver set-actuation 2.0");
                    return Ok(());
                }
                let mm: f32 = args[2].parse().unwrap_or(2.0);
                if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                    let info = device.read_info();
                    let factor = iot_driver::hid::MonsGeekDevice::precision_factor_from_version(info.version);
                    let raw = (mm * factor) as u16;
                    if device.set_actuation_all_u16(raw) {
                        println!("Actuation point set to {:.2}mm (raw: {}) for all keys", mm, raw);
                    } else {
                        eprintln!("Failed to set actuation point");
                    }
                } else {
                    eprintln!("No device found");
                }
                return Ok(());
            }
            "set-rt" | "rapid-trigger" | "rt" => {
                if args.len() < 3 {
                    eprintln!("Usage: iot_driver set-rt <on|off|sensitivity_mm>");
                    eprintln!("  Examples:");
                    eprintln!("    iot_driver set-rt on       # Enable RT with default 0.3mm");
                    eprintln!("    iot_driver set-rt off      # Disable RT");
                    eprintln!("    iot_driver set-rt 0.2      # Enable RT with 0.2mm sensitivity");
                    return Ok(());
                }
                if let Ok(device) = iot_driver::hid::MonsGeekDevice::open() {
                    let info = device.read_info();
                    let factor = iot_driver::hid::MonsGeekDevice::precision_factor_from_version(info.version);

                    match args[2].to_lowercase().as_str() {
                        "off" | "0" | "disable" => {
                            if device.set_rapid_trigger_all(false) {
                                println!("Rapid Trigger disabled for all keys");
                            } else {
                                eprintln!("Failed to disable Rapid Trigger");
                            }
                        }
                        "on" | "enable" => {
                            let sensitivity = (0.3 * factor) as u16;
                            device.set_rapid_trigger_all(true);
                            device.set_rt_press_all_u16(sensitivity);
                            device.set_rt_lift_all_u16(sensitivity);
                            println!("Rapid Trigger enabled with 0.3mm sensitivity for all keys");
                        }
                        _ => {
                            let mm: f32 = args[2].parse().unwrap_or(0.3);
                            let sensitivity = (mm * factor) as u16;
                            device.set_rapid_trigger_all(true);
                            device.set_rt_press_all_u16(sensitivity);
                            device.set_rt_lift_all_u16(sensitivity);
                            println!("Rapid Trigger enabled with {:.2}mm sensitivity for all keys", mm);
                        }
                    }
                } else {
                    eprintln!("No device found");
                }
                return Ok(());
            }

            // Utility commands
            "list" | "ls" => {
                cli_list(&hidapi);
                return Ok(());
            }
            "raw" | "cmd" | "hex" => {
                if args.len() < 3 {
                    eprintln!("Usage: iot_driver raw <hex_byte>");
                    eprintln!("Example: iot_driver raw 8f  (GET_USB_VERSION)");
                    return Ok(());
                }
                let cmd_byte = u8::from_str_radix(&args[2], 16)?;
                cli_test(&hidapi, cmd_byte)?;
                return Ok(());
            }
            "serve" | "server" => {
                // Fall through to server mode
            }
            "help" | "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            _ => {
                print_help();
                return Ok(());
            }
        }
    } else {
        print_help();
        return Ok(());
    }

    // Server mode
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("iot_driver=debug".parse().unwrap())
        )
        .init();

    let addr = "127.0.0.1:3814".parse()?;

    info!("Starting IOT Driver Linux on {}", addr);
    println!("addr :: {}", addr);
    println!("SSSSSSSSSSSTTTTTTTTTTTTTTTTTAAAAAAAAAAAARRRRRRRRRRRTTTTTTTTTTT!!!!!!!");

    let service = DriverService::new()
        .map_err(|e| format!("Failed to initialize HID API: {}", e))?;

    // Scan for devices on startup
    let devices = service.scan_devices();
    info!("Found {} devices on startup", devices.len());
    for dev in &devices {
        if let Some(dj_dev::OneofDev::Dev(d)) = &dev.oneof_dev {
            info!("  - VID={:04x} PID={:04x} ID={} path={}", d.vid, d.pid, d.id, d.path);
        }
    }

    // CORS layer for browser access - must allow all gRPC-Web headers
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any)
        .expose_headers(Any);

    // Wrap service with gRPC-Web support for browser clients
    let grpc_service = tonic_web::enable(DriverGrpcServer::new(service));

    info!("Server ready with gRPC-Web support");

    Server::builder()
        .accept_http1(true)  // Required for gRPC-Web
        .layer(cors)
        .add_service(grpc_service)
        .serve(addr)
        .await?;

    Ok(())
}
