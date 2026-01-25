//! Virtual joystick device using evdev/uinput
//!
//! Creates a virtual gamepad device that appears as a standard joystick
//! to games and applications.

use crate::config::AxisId;
use evdev::{
    uinput::{VirtualDevice, VirtualDeviceBuilder},
    AbsInfo, AbsoluteAxisType, AttributeSet, InputEvent, Key, UinputAbsSetup,
};
use std::collections::HashMap;
use thiserror::Error;

/// Joystick axis value range (standard for most games)
pub const AXIS_MIN: i32 = -32767;
pub const AXIS_MAX: i32 = 32767;

/// Errors from virtual joystick operations
#[derive(Debug, Error)]
pub enum JoystickError {
    #[error("Failed to create virtual device: {0}")]
    CreateDevice(#[source] std::io::Error),
    #[error("Failed to emit event: {0}")]
    EmitEvent(#[source] std::io::Error),
    #[error("Device not initialized")]
    NotInitialized,
}

/// Virtual joystick device
pub struct VirtualJoystick {
    device: VirtualDevice,
    /// Current axis values (for change detection)
    axis_values: HashMap<AxisId, i32>,
}

impl VirtualJoystick {
    /// Create a new virtual joystick device
    ///
    /// # Arguments
    /// * `name` - Device name (shown in `evtest` and game controller settings)
    /// * `axes` - Which axes to enable on the device
    pub fn new(name: &str, axes: &[AxisId]) -> Result<Self, JoystickError> {
        let mut builder = VirtualDeviceBuilder::new()
            .map_err(JoystickError::CreateDevice)?
            .name(name);

        // Add gamepad buttons for Steam Input compatibility
        let mut keys = AttributeSet::<Key>::new();
        keys.insert(Key::BTN_SOUTH);
        keys.insert(Key::BTN_EAST);
        keys.insert(Key::BTN_NORTH);
        keys.insert(Key::BTN_WEST);
        builder = builder
            .with_keys(&keys)
            .map_err(JoystickError::CreateDevice)?;

        // Add requested absolute axes
        for &axis_id in axes {
            let code = axis_id_to_code(axis_id);
            let abs_setup = UinputAbsSetup::new(code, AbsInfo::new(0, AXIS_MIN, AXIS_MAX, 0, 0, 1));
            builder = builder
                .with_absolute_axis(&abs_setup)
                .map_err(JoystickError::CreateDevice)?;
        }

        let device = builder.build().map_err(JoystickError::CreateDevice)?;

        let mut axis_values = HashMap::new();
        for &axis_id in axes {
            axis_values.insert(axis_id, 0);
        }

        Ok(Self {
            device,
            axis_values,
        })
    }

    /// Set an axis value
    ///
    /// Only emits events if the value has changed.
    ///
    /// # Arguments
    /// * `axis` - Which axis to update
    /// * `value` - Value in range [-32767, 32767]
    pub fn set_axis(&mut self, axis: AxisId, value: i32) -> Result<(), JoystickError> {
        let clamped = value.clamp(AXIS_MIN, AXIS_MAX);

        // Only emit if changed
        if self.axis_values.get(&axis) == Some(&clamped) {
            return Ok(());
        }

        self.axis_values.insert(axis, clamped);

        let code = axis_id_to_code(axis);
        let event = InputEvent::new_now(evdev::EventType::ABSOLUTE, code.0, clamped);

        self.device
            .emit(&[event])
            .map_err(JoystickError::EmitEvent)?;

        Ok(())
    }

    /// Set multiple axis values at once (more efficient)
    pub fn set_axes(&mut self, values: &[(AxisId, i32)]) -> Result<(), JoystickError> {
        let mut events = Vec::new();

        for &(axis, value) in values {
            let clamped = value.clamp(AXIS_MIN, AXIS_MAX);

            // Only include if changed
            if self.axis_values.get(&axis) != Some(&clamped) {
                self.axis_values.insert(axis, clamped);
                let code = axis_id_to_code(axis);
                events.push(InputEvent::new_now(
                    evdev::EventType::ABSOLUTE,
                    code.0,
                    clamped,
                ));
            }
        }

        if !events.is_empty() {
            self.device
                .emit(&events)
                .map_err(JoystickError::EmitEvent)?;
        }

        Ok(())
    }

    /// Get the device path (e.g., /dev/input/eventX)
    pub fn device_path(&mut self) -> Option<std::path::PathBuf> {
        self.device
            .enumerate_dev_nodes_blocking()
            .ok()?
            .next()?
            .ok()
    }

    /// Get current axis value
    pub fn get_axis(&self, axis: AxisId) -> i32 {
        self.axis_values.get(&axis).copied().unwrap_or(0)
    }
}

/// Convert our AxisId to evdev AbsoluteAxisType
fn axis_id_to_code(axis: AxisId) -> AbsoluteAxisType {
    match axis {
        AxisId::X => AbsoluteAxisType::ABS_X,
        AxisId::Y => AbsoluteAxisType::ABS_Y,
        AxisId::RX => AbsoluteAxisType::ABS_RX,
        AxisId::RY => AbsoluteAxisType::ABS_RY,
        AxisId::Z => AbsoluteAxisType::ABS_Z,
        AxisId::RZ => AbsoluteAxisType::ABS_RZ,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Requires uinput access (run with: cargo test -- --ignored)
    fn test_create_joystick() {
        let axes = vec![AxisId::X, AxisId::Y];
        let joystick = VirtualJoystick::new("Test Joystick", &axes);
        assert!(joystick.is_ok());
    }
}
