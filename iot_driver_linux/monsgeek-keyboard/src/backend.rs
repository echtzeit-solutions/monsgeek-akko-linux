//! The protocol-neutral face of a keyboard.
//!
//! Each vendor protocol is a backend: a type that owns its own transport and
//! implements [`Keyboard`]. Commands that only need what every backend offers take
//! `&dyn Keyboard`; features one protocol alone has stay on that backend's type and
//! are reached through [`downcast_ref`](trait.Keyboard.html#method.downcast_ref).
//!
//! Signatures use plain units (Hz, ms, seconds) rather than any protocol's wire
//! codes, so a backend translates at its own boundary and the caller never sees
//! a wire byte.

use std::any::Any;

use monsgeek_transport::TransportDeviceInfo;
use monsgeek_transport::protocol::Profile;

use crate::KeyboardError;
use crate::settings::{FirmwareVersion, SleepTimeSettings};

/// Operations every keyboard backend provides.
///
/// A backend that lacks one returns [`KeyboardError::NotSupported`]; it never
/// answers with a made-up value or reports success for a write it did not make.
pub trait Keyboard: Any + Send + Sync {
    /// USB identity and connection type of the opened interface.
    fn device_info(&self) -> &TransportDeviceInfo;

    /// Product name, or `VID:PID` when the device reports none.
    fn device_name(&self) -> String {
        let info = self.device_info();
        info.product_name
            .clone()
            .unwrap_or_else(|| format!("{:04X}:{:04X}", info.vid, info.pid))
    }

    fn get_version(&self) -> Result<FirmwareVersion, KeyboardError>;

    /// The profile the keyboard is currently switched to.
    fn get_profile(&self) -> Result<Profile, KeyboardError>;

    /// Switch the keyboard to a profile.
    fn set_profile(&self, profile: Profile) -> Result<(), KeyboardError>;

    /// USB polling rate in Hz.
    fn get_polling_rate(&self) -> Result<u16, KeyboardError>;

    /// Set the USB polling rate in Hz. Rates the device does not offer are
    /// rejected with [`KeyboardError::InvalidParameter`] or
    /// [`KeyboardError::NotSupported`].
    fn set_polling_rate(&self, hz: u16) -> Result<(), KeyboardError>;

    /// Key debounce time in milliseconds.
    fn get_debounce(&self) -> Result<u8, KeyboardError>;

    fn set_debounce(&self, ms: u8) -> Result<(), KeyboardError>;

    /// Wireless sleep timeouts in seconds.
    fn get_sleep_time(&self) -> Result<SleepTimeSettings, KeyboardError>;

    /// Set the wireless sleep timeouts. A backend with fewer separate timeouts
    /// than [`SleepTimeSettings`] holds rejects settings it cannot represent.
    fn set_sleep_time(&self, settings: &SleepTimeSettings) -> Result<(), KeyboardError>;

    /// Restore factory defaults.
    fn reset(&self) -> Result<(), KeyboardError>;
}

impl dyn Keyboard {
    /// The concrete backend, for features only that protocol has.
    pub fn downcast_ref<T: Keyboard>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref()
    }

    /// Owned form of [`downcast_ref`](Self::downcast_ref); hands the keyboard back
    /// when it is a different backend.
    pub fn downcast<T: Keyboard>(self: Box<Self>) -> Result<Box<T>, Box<Self>> {
        if (*self).downcast_ref::<T>().is_some() {
            let any: Box<dyn Any> = self;
            Ok(any.downcast().expect("type checked above"))
        } else {
            Err(self)
        }
    }
}
