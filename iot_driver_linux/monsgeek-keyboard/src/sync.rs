//! Synchronous helpers for keyboard discovery.

use crate::error::KeyboardError;
use monsgeek_transport::DiscoveredDevice;

/// List all connected devices
pub fn list_keyboards() -> Result<Vec<DiscoveredDevice>, KeyboardError> {
    Ok(monsgeek_transport::list_devices_sync()?)
}
