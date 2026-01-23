//! Device discovery for MonsGeek/Akko keyboards

use std::sync::Arc;

use async_trait::async_trait;
use hidapi::HidApi;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::device_registry;
use crate::error::TransportError;
use crate::hid_dongle::HidDongleTransport;
use crate::hid_wired::HidWiredTransport;
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
}

impl Default for HidDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl HidDiscovery {
    /// Create a new HID discovery instance
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(16);
        Self {
            known_devices: vec![
                (device::VENDOR_ID, device::PID_M1_V5_WIRED),
                (device::VENDOR_ID, device::PID_M1_V5_DONGLE),
            ],
            event_tx,
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

    /// Check if this is the feature interface (usage 0x02)
    fn is_feature_interface(device_info: &hidapi::DeviceInfo) -> bool {
        device_info.usage_page() == device::USAGE_PAGE
            && device_info.usage() == device::USAGE_FEATURE
    }

    /// Check if this is the input interface (usage 0x01)
    fn is_input_interface(device_info: &hidapi::DeviceInfo) -> bool {
        device_info.usage_page() == device::USAGE_PAGE && device_info.usage() == device::USAGE_INPUT
    }

    /// Find the input interface for a device
    fn find_input_device(&self, api: &HidApi, vid: u16, pid: u16) -> Option<hidapi::DeviceInfo> {
        api.device_list()
            .find(|d| d.vendor_id() == vid && d.product_id() == pid && Self::is_input_interface(d))
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

            // Only report feature interfaces
            if !Self::is_feature_interface(device_info) {
                continue;
            }

            let is_dongle = device_registry::is_dongle_pid(pid);
            let transport_type = if is_dongle {
                TransportType::HidDongle
            } else {
                TransportType::HidWired
            };

            let path = device_info.path().to_string_lossy().to_string();
            let serial = device_info.serial_number().map(|s| s.to_string());
            let product_name = device_info.product_string().map(|s| s.to_string());

            debug!(
                "Found device: VID={:04X} PID={:04X} type={:?} path={}",
                vid, pid, transport_type, path
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

        // Find and open the feature interface
        let feature_info = api
            .device_list()
            .find(|d| {
                d.vendor_id() == device.info.vid
                    && d.product_id() == device.info.pid
                    && Self::is_feature_interface(d)
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
            .find_input_device(&api, device.info.vid, device.info.pid)
            .and_then(|info| info.open_device(&api).ok());

        if input_device.is_some() {
            debug!("Opened input interface for events");
        }

        // Create appropriate transport
        let transport: Arc<dyn Transport> = match device.info.transport_type {
            TransportType::HidWired => Arc::new(HidWiredTransport::new(
                feature_device,
                input_device,
                device.info.clone(),
            )),
            TransportType::HidDongle => Arc::new(HidDongleTransport::new(
                feature_device,
                input_device,
                device.info.clone(),
            )),
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

        Ok(transport)
    }

    async fn watch(&self) -> Result<broadcast::Receiver<DiscoveryEvent>, TransportError> {
        // TODO: Implement udev hot-plug monitoring
        // For now, just return a receiver that won't get events
        warn!("Hot-plug monitoring not yet implemented");
        Ok(self.event_tx.subscribe())
    }
}
