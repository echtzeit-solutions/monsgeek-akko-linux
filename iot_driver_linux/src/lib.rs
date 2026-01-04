// MonsGeek M1 V5 HE Linux Driver - Shared Library
// Protocol definitions, device registry, and HID communication

pub mod audio_reactive;
pub mod color;
pub mod device_loader;
pub mod devices;
pub mod firmware;
pub mod firmware_api;
pub mod gif;
pub mod hal;
pub mod hid;
pub mod profile;
pub mod protocol;
pub mod screen_capture;
pub mod tui;

pub use device_loader::{DeviceDatabase, JsonDeviceDefinition};
pub use devices::{find_device, is_supported, DeviceDefinition, SUPPORTED_DEVICES};
pub use hal::{device_registry, DeviceRegistry, HidInterface, InterfaceType};
pub use hid::{ConnectedDeviceInfo, DeviceInfo, MonsGeekDevice, TriggerSettings, VendorEventType, key_mode};
pub use profile::{profile_registry, DeviceProfile, DeviceProfileExt, ProfileRegistry};
pub use protocol::cmd;
pub use protocol::magnetism;
pub use protocol::music_viz;
