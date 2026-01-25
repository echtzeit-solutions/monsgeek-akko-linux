//! MonsGeek Magnetic Key-to-Joystick Mapper
//!
//! Translates magnetic Hall Effect key readings into virtual joystick axes,
//! with a TUI for configuration and live visualization.

pub mod config;
pub mod joystick;
pub mod mapper;
pub mod tui;

pub use config::{
    AxisCalibration, AxisConfig, AxisId, AxisMappingMode, JoystickConfig, KeyBinding,
};
pub use joystick::{JoystickError, VirtualJoystick, AXIS_MAX, AXIS_MIN};
pub use mapper::AxisMapper;
