//! Device discovery for MonsGeek/Akko keyboards and their rebrands

use std::sync::Arc;

use hidapi::HidApi;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Check if a device is connected via Bluetooth
fn is_bluetooth_bus(device_info: &hidapi::DeviceInfo) -> bool {
    matches!(device_info.bus_type(), hidapi::BusType::Bluetooth)
}

use crate::Transport;
use crate::device_registry;
use crate::error::TransportError;
use crate::flow_control::FlowControlTransport;
use crate::hid_bluetooth::HidBluetoothTransport;
use crate::hid_dongle::HidDongleTransport;
use crate::hid_wired::HidWiredTransport;
use crate::printer::{Printer, PrinterConfig};
use crate::protocol::device;
use crate::types::{
    DiscoveredDevice, DiscoveryEvent, KeyboardProtocol, TransportDeviceInfo, TransportType,
};

/// HID collection and report layout captured from Beiying and RongYuan devices.
/// Descriptor parsing is deliberately passive: callers supply bytes read from
/// sysfs, never from an opened hidraw device.
fn is_beiying_report09_descriptor(bytes: &[u8]) -> bool {
    #[derive(Clone, Copy, Default)]
    struct Global {
        usage_page: u16,
        report_size: u32,
        report_count: u32,
        report_id: Option<u8>,
    }

    let mut global = Global::default();
    let mut global_stack = Vec::new();
    let mut collections = Vec::new();
    let mut usage = None;
    let mut found_input = false;
    let mut found_output = false;
    let mut offset = 0;

    while offset < bytes.len() {
        let prefix = bytes[offset];
        if prefix == 0xFE {
            let Some(&length) = bytes.get(offset + 1) else {
                break;
            };
            offset = offset.saturating_add(3 + usize::from(length));
            continue;
        }

        let data_len = match prefix & 0x03 {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 4,
        };
        let end = offset.saturating_add(1 + data_len);
        let Some(data) = bytes.get(offset + 1..end) else {
            break;
        };
        offset = end;

        let value = data.iter().enumerate().fold(0u32, |value, (index, byte)| {
            value | (u32::from(*byte) << (index * 8))
        });
        let item_type = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0F;

        match item_type {
            0 => match tag {
                // Input, Output, Feature main items.
                8 | 9 | 11 => {
                    let collection = collections.last().copied();
                    let matching_collection = collection == Some((0xFF60, 0x0061));
                    let matching_report = global.report_id == Some(0x09)
                        && global.report_size == 8
                        && global.report_count == 63;
                    if matching_collection && matching_report {
                        found_input |= tag == 8;
                        found_output |= tag == 9;
                    }
                    usage = None;
                }
                // Collection.
                10 => {
                    let current = if value == 1 || collections.is_empty() {
                        usage
                            .map(|u| (global.usage_page, u))
                            .unwrap_or((global.usage_page, 0))
                    } else {
                        *collections.last().expect("non-empty collection stack")
                    };
                    collections.push(current);
                    usage = None;
                }
                // End Collection.
                12 => {
                    collections.pop();
                    usage = None;
                }
                _ => usage = None,
            },
            1 => match tag {
                0 => global.usage_page = value as u16,
                7 => global.report_size = value,
                8 => global.report_id = u8::try_from(value).ok().filter(|id| *id != 0),
                9 => global.report_count = value,
                10 => global_stack.push(global),
                11 => {
                    if let Some(previous) = global_stack.pop() {
                        global = previous;
                    }
                }
                _ => {}
            },
            2 if tag == 0 => usage = Some(value as u16),
            _ => {}
        }
    }

    found_input && found_output
}

fn is_beiying_interface(usage_page: u16, usage: u16, descriptor: &[u8]) -> bool {
    usage_page == 0xFF60 && usage == 0x0061 && is_beiying_report09_descriptor(descriptor)
}

/// Read the kernel-cached report descriptor for a HID device through sysfs.
/// This does not open the hidraw character device or send a USB request.
fn read_sysfs_report_descriptor(hidraw_path: &str) -> Option<Vec<u8>> {
    let devname = std::path::Path::new(hidraw_path).file_name()?.to_str()?;
    if !devname.starts_with("hidraw") || devname.contains('/') || devname.contains("..") {
        return None;
    }
    std::fs::read(format!(
        "/sys/class/hidraw/{devname}/device/report_descriptor"
    ))
    .ok()
}

/// Device discovery abstraction
pub trait DeviceDiscovery: Send + Sync {
    /// List currently available devices
    fn list_devices(&self) -> Result<Vec<DiscoveredDevice>, TransportError>;

    /// Open a specific device
    fn open_device(&self, device: &DiscoveredDevice) -> Result<Arc<dyn Transport>, TransportError>;

    /// Subscribe to hot-plug events
    fn watch(&self) -> Result<broadcast::Receiver<DiscoveryEvent>, TransportError>;
}

/// HID device discovery for wired and dongle connections
pub struct HidDiscovery {
    /// Hot-plug event sender
    event_tx: broadcast::Sender<DiscoveryEvent>,
    /// Optional printer config for monitoring mode - wraps transports automatically
    printer_config: Option<PrinterConfig>,
}

impl Default for HidDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

/// Bluetooth HID usage page and usage for vendor endpoint
mod bluetooth {
    /// Bluetooth vendor usage page (from report descriptor)
    pub const USAGE_PAGE: u16 = 0xFF55;
    /// Bluetooth vendor usage (from report descriptor)
    pub const USAGE_VENDOR: u16 = 0x0202;
}

impl HidDiscovery {
    /// Create a new HID discovery instance
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(16);
        Self {
            event_tx,
            printer_config: None,
        }
    }

    /// Create with printer config for monitoring mode
    /// All transports opened via open_device() will be wrapped with Printer
    pub fn with_printer_config(config: PrinterConfig) -> Self {
        let (event_tx, _) = broadcast::channel(16);
        Self {
            event_tx,
            printer_config: Some(config),
        }
    }

    /// Check if this is the USB feature interface (vendor usage page, usage 0x02)
    fn is_usb_feature_interface(device_info: &hidapi::DeviceInfo) -> bool {
        device::is_vendor_usage_page(device_info.usage_page())
            && device_info.usage() == device::USAGE_FEATURE
    }

    /// Check if this is the Bluetooth vendor interface (usage 0x0202, page 0xFF55)
    fn is_bluetooth_vendor_interface(device_info: &hidapi::DeviceInfo) -> bool {
        device_info.usage_page() == bluetooth::USAGE_PAGE
            && device_info.usage() == bluetooth::USAGE_VENDOR
    }

    /// Check if this is the USB input interface (vendor usage page, usage 0x01)
    fn is_usb_input_interface(device_info: &hidapi::DeviceInfo) -> bool {
        device::is_vendor_usage_page(device_info.usage_page())
            && device_info.usage() == device::USAGE_INPUT
    }

    /// Extract USB bus location from a hidraw sysfs path.
    /// e.g., `/sys/devices/.../usb9/9-2/9-2:1.2/0003:3151:5030.065C/hidraw/hidraw10`
    /// returns `"9-2"` — the USB port, shared by all interfaces of the same device.
    fn usb_bus_location(hidraw_path: &str) -> Option<String> {
        // Read the sysfs path for this hidraw device
        let devname = std::path::Path::new(hidraw_path).file_name()?.to_str()?;
        let sysfs = std::fs::read_link(format!("/sys/class/hidraw/{}", devname)).ok()?;
        let sysfs_str = sysfs.to_string_lossy();
        // Find the USB port segment (e.g., "9-2" in ".../usb9/9-2/9-2:1.2/...")
        for component in sysfs_str.split('/') {
            // USB port segments match pattern like "1-2", "3-4.1", "9-2"
            if component.contains('-')
                && !component.contains(':')
                && !component.contains('.')
                && component.chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                return Some(component.to_string());
            }
        }
        None
    }

    /// Check if two hidraw devices are on the same USB physical device.
    fn is_same_usb_device(path_a: &str, path_b: &str) -> bool {
        match (
            Self::usb_bus_location(path_a),
            Self::usb_bus_location(path_b),
        ) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    /// Find the input interface for a USB device, preferring a sibling of `feature_path`.
    fn find_usb_input_device(
        &self,
        api: &HidApi,
        vid: u16,
        pid: u16,
        feature_path: &str,
    ) -> Option<hidapi::DeviceInfo> {
        api.device_list()
            .filter(|d| {
                d.vendor_id() == vid && d.product_id() == pid && Self::is_usb_input_interface(d)
            })
            .find(|d| Self::is_same_usb_device(&d.path().to_string_lossy(), feature_path))
            // Fallback: any matching VID/PID input interface
            .or_else(|| {
                api.device_list().find(|d| {
                    d.vendor_id() == vid && d.product_id() == pid && Self::is_usb_input_interface(d)
                })
            })
            .cloned()
    }

    /// Find the IF1 (composite HID) interface for a dongle device,
    /// preferring a sibling of `feature_path`.
    /// IF1 hosts the patched HID descriptor with Feature Report ID 8.
    fn find_dongle_if1_device(
        api: &HidApi,
        vid: u16,
        pid: u16,
        feature_path: &str,
    ) -> Option<hidapi::DeviceInfo> {
        let is_if1 = |d: &&hidapi::DeviceInfo| {
            d.vendor_id() == vid
                && d.product_id() == pid
                && d.interface_number() == 1
                && !Self::is_usb_feature_interface(d)
                && !Self::is_usb_input_interface(d)
        };
        api.device_list()
            .filter(is_if1)
            .find(|d| Self::is_same_usb_device(&d.path().to_string_lossy(), feature_path))
            .or_else(|| api.device_list().find(is_if1))
            .cloned()
    }

    /// Find the input interface for a Bluetooth device (keyboard HID)
    fn find_bt_input_device(&self, api: &HidApi, vid: u16, pid: u16) -> Option<hidapi::DeviceInfo> {
        // Bluetooth keyboard uses standard HID keyboard usage (0x0006, page 0x0001)
        api.device_list()
            .find(|d| {
                d.vendor_id() == vid
                    && d.product_id() == pid
                    && is_bluetooth_bus(d)
                    && d.usage() == 0x0006
                    && d.usage_page() == 0x0001
            })
            .cloned()
    }
}

impl DeviceDiscovery for HidDiscovery {
    fn list_devices(&self) -> Result<Vec<DiscoveredDevice>, TransportError> {
        let api = HidApi::new().map_err(|e| TransportError::HidError(e.to_string()))?;
        let mut devices = Vec::new();

        for device_info in api.device_list() {
            let vid = device_info.vendor_id();
            let pid = device_info.product_id();

            if !device_registry::is_candidate_device(vid, pid) {
                continue;
            }

            // Determine transport type based on bus type and PID
            let is_bluetooth = is_bluetooth_bus(device_info);

            // Bluetooth and existing RongYuan USB recognition retain their
            // usage checks. Beiying USB interfaces need descriptor confirmation
            // because VID:PID can also identify a RongYuan board.
            let protocol = if is_bluetooth {
                if !Self::is_bluetooth_vendor_interface(device_info) {
                    continue;
                }
                KeyboardProtocol::RongYuan
            } else if Self::is_usb_feature_interface(device_info) {
                KeyboardProtocol::RongYuan
            } else if device_info.usage_page() == 0xFF60 && device_info.usage() == 0x0061 {
                let path = device_info.path().to_string_lossy();
                let Some(descriptor) = read_sysfs_report_descriptor(&path) else {
                    continue;
                };
                if !is_beiying_interface(device_info.usage_page(), device_info.usage(), &descriptor)
                {
                    continue;
                }
                KeyboardProtocol::Beiying
            } else {
                continue;
            };

            // Authoritative PID list first; fall back to a product-string
            // heuristic so dongles with an unrecognized PID are still treated
            // as receivers (and surfaced by `probe`) instead of wired.
            let is_dongle = device_registry::is_dongle_pid(pid)
                || (!is_bluetooth
                    && !device_registry::is_bluetooth_pid(pid)
                    && device_registry::looks_like_dongle(device_info.product_string()));
            let transport_type = if is_bluetooth {
                TransportType::Bluetooth
            } else if is_dongle {
                TransportType::HidDongle
            } else {
                TransportType::HidWired
            };

            let path = device_info.path().to_string_lossy().to_string();
            let serial = device_info.serial_number().map(|s| s.to_string());
            let product_name = device_info.product_string().map(|s| s.to_string());

            debug!(
                "Found device: VID={:04X} PID={:04X} type={:?} bt={} path={}",
                vid, pid, transport_type, is_bluetooth, path
            );

            devices.push(DiscoveredDevice {
                info: TransportDeviceInfo {
                    vid,
                    pid,
                    is_dongle,
                    transport_type,
                    device_path: path,
                    serial,
                    product_name,
                },
                protocol,
            });
        }

        info!("Found {} devices", devices.len());
        Ok(devices)
    }

    fn open_device(&self, device: &DiscoveredDevice) -> Result<Arc<dyn Transport>, TransportError> {
        if device.protocol != KeyboardProtocol::RongYuan {
            return Err(TransportError::Internal(
                "Beiying devices cannot be opened as RongYuan transports".into(),
            ));
        }
        let api = HidApi::new().map_err(|e| TransportError::HidError(e.to_string()))?;

        // Create appropriate transport based on type
        let transport: Arc<dyn Transport> = match device.info.transport_type {
            TransportType::Bluetooth => {
                // Find and open the Bluetooth vendor interface
                let vendor_info = api
                    .device_list()
                    .find(|d| {
                        d.vendor_id() == device.info.vid
                            && d.product_id() == device.info.pid
                            && Self::is_bluetooth_vendor_interface(d)
                    })
                    .ok_or_else(|| {
                        TransportError::DeviceNotFound(format!(
                            "Bluetooth vendor interface for {:04X}:{:04X}",
                            device.info.vid, device.info.pid
                        ))
                    })?;

                let vendor_device = vendor_info
                    .open_device(&api)
                    .map_err(TransportError::from)?;

                // Try to open keyboard input interface for events
                let input_device = self
                    .find_bt_input_device(&api, device.info.vid, device.info.pid)
                    .and_then(|info| info.open_device(&api).ok());

                if input_device.is_some() {
                    debug!("Opened Bluetooth keyboard input interface for events");
                }

                Arc::new(HidBluetoothTransport::new(
                    vendor_device,
                    input_device,
                    device.info.clone(),
                ))
            }
            TransportType::HidWired | TransportType::HidDongle => {
                // Find and open the USB feature interface.
                // Match by device_path to distinguish multiple same-VID/PID devices.
                // The discovered device_path points to the feature interface (IF2),
                // so an exact path match is the primary lookup strategy.
                let feature_info = api
                    .device_list()
                    .find(|d| {
                        d.path().to_string_lossy() == device.info.device_path
                            && Self::is_usb_feature_interface(d)
                    })
                    // Fallback: VID/PID match (for devices discovered before path was tracked)
                    .or_else(|| {
                        api.device_list().find(|d| {
                            d.vendor_id() == device.info.vid
                                && d.product_id() == device.info.pid
                                && Self::is_usb_feature_interface(d)
                        })
                    })
                    .ok_or_else(|| {
                        TransportError::DeviceNotFound(format!(
                            "Feature interface for {:04X}:{:04X}",
                            device.info.vid, device.info.pid
                        ))
                    })?;

                let feature_device = feature_info
                    .open_device(&api)
                    .map_err(TransportError::from)?;

                let feature_path = feature_info.path().to_string_lossy().to_string();

                // Try to open input interface (sibling of feature interface)
                let input_device = self
                    .find_usb_input_device(&api, device.info.vid, device.info.pid, &feature_path)
                    .and_then(|info| info.open_device(&api).ok());

                if input_device.is_some() {
                    debug!("Opened USB input interface for events");
                }

                if device.info.transport_type == TransportType::HidDongle {
                    // Open IF1 for dongle patch discovery (Feature Report ID 8)
                    let if1_device = Self::find_dongle_if1_device(
                        &api,
                        device.info.vid,
                        device.info.pid,
                        &feature_path,
                    )
                    .and_then(|info| match info.open_device(&api) {
                        Ok(dev) => {
                            debug!("Opened dongle IF1 for patch discovery");
                            Some(dev)
                        }
                        Err(e) => {
                            debug!("Failed to open dongle IF1: {}", e);
                            None
                        }
                    });

                    Arc::new(HidDongleTransport::new(
                        feature_device,
                        input_device,
                        if1_device,
                        device.info.clone(),
                    ))
                } else {
                    Arc::new(HidWiredTransport::new(
                        feature_device,
                        input_device,
                        device.info.clone(),
                    ))
                }
            }
            _ => {
                return Err(TransportError::Internal(format!(
                    "Unsupported transport type: {:?}",
                    device.info.transport_type
                )));
            }
        };

        info!(
            "Opened {:?} transport for {:04X}:{:04X}",
            device.info.transport_type, device.info.vid, device.info.pid
        );

        // Wrap with printer if monitoring is enabled
        let transport = match &self.printer_config {
            Some(config) => Printer::wrap(transport, config.clone()),
            None => transport,
        };

        Ok(transport)
    }

    fn watch(&self) -> Result<broadcast::Receiver<DiscoveryEvent>, TransportError> {
        // TODO: Implement udev hot-plug monitoring
        // For now, just return a receiver that won't get events
        warn!("Hot-plug monitoring not yet implemented");
        Ok(self.event_tx.subscribe())
    }
}

/// Result of probing a device
#[derive(Debug, Clone)]
pub struct ProbedDevice {
    /// The discovered device info
    pub device: DiscoveredDevice,
    /// Whether the device responded to a probe query
    pub responsive: bool,
    /// Device ID if probe succeeded (from GET_USB_VERSION)
    pub device_id: Option<u32>,
    /// Firmware version if probe succeeded
    pub version: Option<u16>,
}

impl HidDiscovery {
    /// Probe all discovered devices to find which ones actually respond
    ///
    /// This opens each device, sends a GET_USB_VERSION query, and returns
    /// information about which devices responded. Useful when multiple
    /// transports are available (e.g., dongle + Bluetooth) but only one
    /// is actually connected to the keyboard.
    ///
    /// Returns a list of all discovered devices with their probe results,
    /// sorted by preference (Bluetooth > Dongle > Wired) with responsive
    /// devices first.
    pub fn probe_devices(&self) -> Result<Vec<ProbedDevice>, TransportError> {
        use crate::ChecksumType;
        use crate::protocol::cmd;

        let devices = self.list_devices()?;
        let mut probed = Vec::with_capacity(devices.len());

        for device in devices {
            let probe_result = match self.open_device(&device) {
                Ok(raw_transport) => {
                    // Wrap with FlowControlTransport for probing
                    let transport = FlowControlTransport::new(raw_transport);
                    // Try to query device ID
                    match transport.query_command(cmd::GET_USB_VERSION, &[], ChecksumType::Bit7) {
                        Ok(resp) if resp.len() >= 5 && resp[0] == cmd::GET_USB_VERSION => {
                            let device_id =
                                u32::from_le_bytes([resp[1], resp[2], resp[3], resp[4]]);
                            let version = if resp.len() >= 9 {
                                Some(u16::from_le_bytes([resp[7], resp[8]]))
                            } else {
                                None
                            };
                            info!(
                                "Probe {:?}: responsive, ID={}, version={:?}",
                                device.info.transport_type, device_id, version
                            );
                            (true, Some(device_id), version)
                        }
                        Ok(resp) => {
                            debug!(
                                "Probe {:?}: unexpected response {:02X?}",
                                device.info.transport_type,
                                &resp[..resp.len().min(8)]
                            );
                            (false, None, None)
                        }
                        Err(e) => {
                            debug!("Probe {:?}: error {}", device.info.transport_type, e);
                            (false, None, None)
                        }
                    }
                }
                Err(e) => {
                    debug!(
                        "Probe {:?}: failed to open: {}",
                        device.info.transport_type, e
                    );
                    (false, None, None)
                }
            };

            probed.push(ProbedDevice {
                device,
                responsive: probe_result.0,
                device_id: probe_result.1,
                version: probe_result.2,
            });
        }

        // Sort: responsive first, then by transport preference (BT > Dongle > Wired)
        probed.sort_by(|a, b| {
            // Responsive devices first
            match (a.responsive, b.responsive) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    // Same responsiveness, sort by transport type preference
                    let priority = |t: &TransportType| match t {
                        TransportType::Bluetooth => 0,
                        TransportType::HidDongle => 1,
                        TransportType::HidWired => 2,
                        _ => 3,
                    };
                    priority(&a.device.info.transport_type)
                        .cmp(&priority(&b.device.info.transport_type))
                }
            }
        });

        info!(
            "Probed {} devices: {} responsive",
            probed.len(),
            probed.iter().filter(|p| p.responsive).count()
        );

        Ok(probed)
    }

    /// Open the best available device
    ///
    /// Probes all devices and returns a transport for the best one:
    /// - If only one device responds, use that one
    /// - If multiple respond, prefer Bluetooth > Dongle > Wired
    /// - If none respond but devices exist, try to open the preferred transport type anyway
    pub fn open_preferred(&self) -> Result<Arc<dyn Transport>, TransportError> {
        let probed = self.probe_devices()?;

        if probed.is_empty() {
            return Err(TransportError::DeviceNotFound("No devices found".into()));
        }

        // Get responsive devices
        let responsive: Vec<_> = probed.iter().filter(|p| p.responsive).collect();

        let chosen = if responsive.len() == 1 {
            // Only one responds - use it
            info!(
                "Single responsive device: {:?}",
                responsive[0].device.info.transport_type
            );
            &responsive[0].device
        } else if !responsive.is_empty() {
            // Multiple respond - use the preferred one (already sorted)
            info!(
                "Multiple responsive devices ({}), choosing {:?}",
                responsive.len(),
                responsive[0].device.info.transport_type
            );
            &responsive[0].device
        } else {
            // None respond - try the preferred transport type anyway
            // (maybe it just needs more time, or it's a different command set)
            warn!(
                "No devices responded to probe, trying {:?} anyway",
                probed[0].device.info.transport_type
            );
            &probed[0].device
        };

        self.open_device(chosen)
    }

    /// Get all responsive devices (for multi-device scenarios)
    ///
    /// Returns transports for all devices that responded to the probe.
    pub fn open_all_responsive(&self) -> Result<Vec<Arc<dyn Transport>>, TransportError> {
        let probed = self.probe_devices()?;
        let mut transports = Vec::new();

        for p in probed.iter().filter(|p| p.responsive) {
            match self.open_device(&p.device) {
                Ok(t) => transports.push(t),
                Err(e) => warn!("Failed to reopen {:?}: {}", p.device.info.transport_type, e),
            }
        }

        if transports.is_empty() && !probed.is_empty() {
            return Err(TransportError::DeviceNotFound(
                "No devices responded to probe".into(),
            ));
        }

        Ok(transports)
    }

    /// Probe all devices and return them with human-readable labels.
    ///
    /// Each device is probed for device_id/version and assigned a sequential index.
    /// The `model_name_fn` callback resolves device_id to a model name string;
    /// if it returns None, the USB product string or "Unknown" is used.
    pub fn list_labeled_devices<F>(
        &self,
        model_name_fn: F,
    ) -> Result<Vec<(ProbedDevice, crate::types::DeviceLabel)>, TransportError>
    where
        F: Fn(Option<u32>, u16, u16) -> Option<String>,
    {
        let probed = self.probe_devices()?;
        let mut labeled = Vec::with_capacity(probed.len());

        for (index, p) in probed.into_iter().enumerate() {
            let transport_name = match p.device.info.transport_type {
                TransportType::HidWired => "usb",
                TransportType::HidDongle => "dongle",
                TransportType::Bluetooth => "bt",
                TransportType::WebRtc => "webrtc",
            };

            let model_name = model_name_fn(p.device_id, p.device.info.vid, p.device.info.pid)
                .or_else(|| p.device.info.product_name.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            let label = crate::types::DeviceLabel {
                index,
                model_name,
                transport_name,
                device_id: p.device_id,
                version: p.version,
                hid_path: p.device.info.device_path.clone(),
            };

            labeled.push((p, label));
        }

        Ok(labeled)
    }
}

/// Format a device list for display (e.g., to stderr when multiple devices found).
pub fn format_device_list(labels: &[crate::types::DeviceLabel]) -> String {
    let mut out = String::new();
    for label in labels {
        out.push_str(&format!("{label}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(hex: &str) -> Vec<u8> {
        hex.split_ascii_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).expect("valid hex fixture"))
            .collect()
    }

    #[test]
    fn captured_descriptors_classify_beiying_without_changing_ry_recognition() {
        let beiying = fixture(include_str!("../tests/fixtures/beiying_tac75_if2.hex"));
        let rongyuan = fixture(include_str!("../tests/fixtures/rongyuan_m1_v5_if2.hex"));

        assert!(is_beiying_interface(0xFF60, 0x0061, &beiying));
        assert!(!is_beiying_interface(0xFF60, 0x0061, &rongyuan));
        assert!(!is_beiying_interface(0xFF60, 0x0062, &beiying));
        assert!(!is_beiying_interface(0xFFFF, 0x0002, &beiying));

        // Existing RongYuan acceptance is still based on its usage page/usage
        // check; this captured report descriptor is not required for it.
        assert!(device::is_vendor_usage_page(0xFFFF));
        assert_eq!(device::USAGE_FEATURE, 0x0002);
        assert!(!is_beiying_report09_descriptor(&rongyuan));
    }

    #[test]
    fn beiying_report09_match_requires_both_report_directions() {
        let mut descriptor = fixture(include_str!("../tests/fixtures/beiying_tac75_if2.hex"));
        let output_item = descriptor
            .windows(2)
            .position(|bytes| bytes == [0x91, 0x02])
            .expect("captured output report item");
        descriptor.drain(output_item..output_item + 2);

        assert!(!is_beiying_report09_descriptor(&descriptor));
    }

    #[test]
    fn beiying_device_is_never_opened_as_a_rongyuan_transport() {
        let device = DiscoveredDevice {
            info: TransportDeviceInfo {
                vid: 0x38EE,
                pid: 0x0016,
                is_dongle: false,
                transport_type: TransportType::HidWired,
                device_path: String::new(),
                serial: None,
                product_name: Some("TAC75 HE".into()),
            },
            protocol: KeyboardProtocol::Beiying,
        };

        let result = HidDiscovery::new().open_device(&device);
        assert!(matches!(
            result,
            Err(TransportError::Internal(message))
                if message == "Beiying devices cannot be opened as RongYuan transports"
        ));
    }
}
