//! Device discovery for MonsGeek/Akko keyboards

use std::sync::Arc;

use async_trait::async_trait;
use hidapi::HidApi;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Check if a device is connected via Bluetooth
fn is_bluetooth_bus(device_info: &hidapi::DeviceInfo) -> bool {
    matches!(device_info.bus_type(), hidapi::BusType::Bluetooth)
}

use crate::device_registry;
use crate::error::TransportError;
use crate::hid_bluetooth::HidBluetoothTransport;
use crate::hid_dongle::HidDongleTransport;
use crate::hid_wired::HidWiredTransport;
use crate::printer::{Printer, PrinterConfig};
use crate::protocol::device;
use crate::types::{DiscoveredDevice, DiscoveryEvent, TransportDeviceInfo, TransportType};
use crate::Transport;

/// Device discovery abstraction
#[async_trait]
pub trait DeviceDiscovery: Send + Sync {
    /// List currently available devices
    async fn list_devices(&self) -> Result<Vec<DiscoveredDevice>, TransportError>;

    /// Open a specific device
    async fn open_device(
        &self,
        device: &DiscoveredDevice,
    ) -> Result<Arc<dyn Transport>, TransportError>;

    /// Subscribe to hot-plug events
    async fn watch(&self) -> Result<broadcast::Receiver<DiscoveryEvent>, TransportError>;
}

/// HID device discovery for wired and dongle connections
pub struct HidDiscovery {
    /// Known VID/PID pairs to look for
    known_devices: Vec<(u16, u16)>,
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
            known_devices: vec![
                (device::VENDOR_ID, device::PID_M1_V5_WIRED),
                (device::VENDOR_ID, device::PID_M1_V5_DONGLE),
                (device::VENDOR_ID, device::PID_M1_V5_BLUETOOTH),
            ],
            event_tx,
            printer_config: None,
        }
    }

    /// Create with printer config for monitoring mode
    /// All transports opened via open_device() will be wrapped with Printer
    pub fn with_printer_config(config: PrinterConfig) -> Self {
        let (event_tx, _) = broadcast::channel(16);
        Self {
            known_devices: vec![
                (device::VENDOR_ID, device::PID_M1_V5_WIRED),
                (device::VENDOR_ID, device::PID_M1_V5_DONGLE),
                (device::VENDOR_ID, device::PID_M1_V5_BLUETOOTH),
            ],
            event_tx,
            printer_config: Some(config),
        }
    }

    /// Add a VID/PID pair to discover
    pub fn add_device(&mut self, vid: u16, pid: u16) {
        if !self.known_devices.contains(&(vid, pid)) {
            self.known_devices.push((vid, pid));
        }
    }

    /// Check if a device matches our known devices
    fn is_known_device(&self, vid: u16, pid: u16) -> bool {
        self.known_devices.contains(&(vid, pid))
    }

    /// Check if this is the USB feature interface (usage 0x02, page 0xFFFF)
    fn is_usb_feature_interface(device_info: &hidapi::DeviceInfo) -> bool {
        device_info.usage_page() == device::USAGE_PAGE
            && device_info.usage() == device::USAGE_FEATURE
    }

    /// Check if this is the Bluetooth vendor interface (usage 0x0202, page 0xFF55)
    fn is_bluetooth_vendor_interface(device_info: &hidapi::DeviceInfo) -> bool {
        device_info.usage_page() == bluetooth::USAGE_PAGE
            && device_info.usage() == bluetooth::USAGE_VENDOR
    }

    /// Check if this is the USB input interface (usage 0x01)
    fn is_usb_input_interface(device_info: &hidapi::DeviceInfo) -> bool {
        device_info.usage_page() == device::USAGE_PAGE && device_info.usage() == device::USAGE_INPUT
    }

    /// Find the input interface for a USB device
    fn find_usb_input_device(
        &self,
        api: &HidApi,
        vid: u16,
        pid: u16,
    ) -> Option<hidapi::DeviceInfo> {
        api.device_list()
            .find(|d| {
                d.vendor_id() == vid && d.product_id() == pid && Self::is_usb_input_interface(d)
            })
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

#[async_trait]
impl DeviceDiscovery for HidDiscovery {
    async fn list_devices(&self) -> Result<Vec<DiscoveredDevice>, TransportError> {
        let api = HidApi::new().map_err(|e| TransportError::HidError(e.to_string()))?;
        let mut devices = Vec::new();

        for device_info in api.device_list() {
            let vid = device_info.vendor_id();
            let pid = device_info.product_id();

            if !self.is_known_device(vid, pid) {
                continue;
            }

            // Determine transport type based on bus type and PID
            let is_bluetooth = is_bluetooth_bus(device_info);

            // For Bluetooth: look for vendor interface (usage 0x0202, page 0xFF55)
            // For USB: look for feature interface (usage 0x02, page 0xFFFF)
            let is_target_interface = if is_bluetooth {
                Self::is_bluetooth_vendor_interface(device_info)
            } else {
                Self::is_usb_feature_interface(device_info)
            };

            if !is_target_interface {
                continue;
            }

            let is_dongle = device_registry::is_dongle_pid(pid);
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
            });
        }

        info!("Found {} devices", devices.len());
        Ok(devices)
    }

    async fn open_device(
        &self,
        device: &DiscoveredDevice,
    ) -> Result<Arc<dyn Transport>, TransportError> {
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
                // Find and open the USB feature interface
                let feature_info = api
                    .device_list()
                    .find(|d| {
                        d.vendor_id() == device.info.vid
                            && d.product_id() == device.info.pid
                            && Self::is_usb_feature_interface(d)
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

                // Try to open input interface
                let input_device = self
                    .find_usb_input_device(&api, device.info.vid, device.info.pid)
                    .and_then(|info| info.open_device(&api).ok());

                if input_device.is_some() {
                    debug!("Opened USB input interface for events");
                }

                if device.info.transport_type == TransportType::HidDongle {
                    Arc::new(HidDongleTransport::new(
                        feature_device,
                        input_device,
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

    async fn watch(&self) -> Result<broadcast::Receiver<DiscoveryEvent>, TransportError> {
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
    pub async fn probe_devices(&self) -> Result<Vec<ProbedDevice>, TransportError> {
        use crate::protocol::cmd;
        use crate::ChecksumType;

        let devices = self.list_devices().await?;
        let mut probed = Vec::with_capacity(devices.len());

        for device in devices {
            let probe_result = match self.open_device(&device).await {
                Ok(transport) => {
                    // Try to query device ID with short timeout
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(800),
                        transport.query_command(cmd::GET_USB_VERSION, &[], ChecksumType::Bit7),
                    )
                    .await
                    {
                        Ok(Ok(resp)) if resp.len() >= 5 && resp[0] == cmd::GET_USB_VERSION => {
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
                        Ok(Ok(resp)) => {
                            debug!(
                                "Probe {:?}: unexpected response {:02X?}",
                                device.info.transport_type,
                                &resp[..resp.len().min(8)]
                            );
                            (false, None, None)
                        }
                        Ok(Err(e)) => {
                            debug!("Probe {:?}: error {}", device.info.transport_type, e);
                            (false, None, None)
                        }
                        Err(_) => {
                            debug!("Probe {:?}: timeout", device.info.transport_type);
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
    pub async fn open_preferred(&self) -> Result<Arc<dyn Transport>, TransportError> {
        let probed = self.probe_devices().await?;

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

        self.open_device(chosen).await
    }

    /// Get all responsive devices (for multi-device scenarios)
    ///
    /// Returns transports for all devices that responded to the probe.
    pub async fn open_all_responsive(&self) -> Result<Vec<Arc<dyn Transport>>, TransportError> {
        let probed = self.probe_devices().await?;
        let mut transports = Vec::new();

        for p in probed.iter().filter(|p| p.responsive) {
            match self.open_device(&p.device).await {
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
}
