//! Synchronous convenience helpers for device discovery + open.

use std::sync::Arc;

use crate::error::TransportError;
use crate::flow_control::FlowControlTransport;
use crate::{DeviceDiscovery, HidDiscovery};

/// List all connected devices synchronously
pub fn list_devices_sync() -> Result<Vec<crate::DiscoveredDevice>, TransportError> {
    let discovery = HidDiscovery::new();
    discovery.list_devices()
}

/// Open a specific device synchronously, returning a flow-controlled transport.
pub fn open_device_sync(
    device: &crate::DiscoveredDevice,
) -> Result<Arc<FlowControlTransport>, TransportError> {
    let discovery = HidDiscovery::new();
    let raw_transport = discovery.open_device(device)?;
    Ok(Arc::new(FlowControlTransport::new(raw_transport)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_devices() {
        // This test will pass even without devices connected
        let result = list_devices_sync();
        assert!(result.is_ok());
    }
}
