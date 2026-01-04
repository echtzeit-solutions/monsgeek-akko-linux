// Hardware Abstraction Layer (HAL) for HID device access
//
// This module provides a unified interface for:
// - Device constants (VID/PID/USAGE)
// - Interface type definitions (Feature vs Input)
// - Device registry (known device interfaces)
// - Device handle (unified access to device)

pub mod constants;
pub mod interface;
pub mod registry;

// Re-export commonly used types
pub use constants::*;
pub use interface::{HidInterface, InterfaceType};
pub use registry::{device_registry, DeviceRegistry};
