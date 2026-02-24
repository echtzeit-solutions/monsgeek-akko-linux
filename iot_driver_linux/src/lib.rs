// MonsGeek M1 V5 HE Linux Driver - Shared Library
// Protocol definitions, device registry, and HID communication

pub mod audio_reactive;
pub mod bpf_loader;
pub mod color;
pub mod device_loader;
pub mod devices;
pub mod effect;
pub mod firmware;
pub mod firmware_api;
pub mod flash;
pub mod hal;
pub mod hid;
pub mod key_action;
pub mod keymap;
pub mod macro_seq;
pub mod pcap_analyzer;
pub mod power_supply;
pub mod profile;
pub mod protocol;
#[cfg(feature = "screen-capture")]
pub mod screen_capture;
pub mod tui;

pub use bpf_loader::{AkkoBpfLoader, BpfStatus, KernelBatteryInfo};
pub use device_loader::{DeviceDatabase, JsonDeviceDefinition};
pub use devices::{find_device, is_supported, DeviceDefinition, SUPPORTED_DEVICES};
pub use hal::{device_registry, DeviceRegistry, HidInterface, InterfaceType};
pub use hid::{
    key_mode, BatteryInfo, ConnectedDeviceInfo, DeviceInfo, TriggerSettings, VendorEventType,
};
pub use power_supply::{
    BatteryState, PowerSupply, PowerSupplyManager, PowerSupplyStatus, TestPowerIntegration,
};
pub use profile::{profile_registry, DeviceProfile, DeviceProfileExt, ProfileRegistry};
pub mod notify;

pub use protocol::cmd;
pub use protocol::magnetism;
pub use protocol::music_viz;
