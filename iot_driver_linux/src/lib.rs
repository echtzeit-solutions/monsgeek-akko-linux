// MonsGeek M1 V5 HE Linux Driver - Shared Library
// Protocol definitions, device registry, and HID communication

pub mod protocol;
pub mod devices;
pub mod hid;
pub mod device_loader;

pub use protocol::cmd;
pub use protocol::magnetism;
pub use devices::{DeviceDefinition, SUPPORTED_DEVICES, find_device, is_supported};
pub use hid::{MonsGeekDevice, DeviceInfo, VendorEventType, ConnectedDeviceInfo, TriggerSettings};
pub use device_loader::{DeviceDatabase, JsonDeviceDefinition};
