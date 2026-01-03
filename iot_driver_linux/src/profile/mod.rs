// Device profile module
// Provides device-specific data abstraction for multiple keyboard models

pub mod builtin;
pub mod json;
pub mod registry;
pub mod traits;
pub mod types;

pub use builtin::{M1V5HeProfile, M1V5HeWirelessProfile, M1_V5_HE_LED_MATRIX};
pub use json::{JsonProfile, JsonProfileWrapper, LoadError};
pub use registry::{profile_registry, ProfileRegistry};
pub use traits::{DeviceProfile, DeviceProfileExt};
pub use types::{DeviceFeatures, FnSysLayer, RangeConfig, TravelSettings};
