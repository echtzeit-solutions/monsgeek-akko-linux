//! Synchronous convenience wrappers
//!
//! Now that `Transport` and `FlowControlTransport` are natively synchronous,
//! `SyncTransport` is just a thin convenience wrapper.  Kept for backward
//! compatibility with existing consumers (CLI, TUI worker threads).

use std::sync::Arc;

use crate::error::TransportError;
use crate::flow_control::FlowControlTransport;
use crate::types::{ChecksumType, TransportDeviceInfo, VendorEvent};
use crate::{DeviceDiscovery, HidDiscovery, Transport};

/// Convenience wrapper around `FlowControlTransport`.
///
/// Since the transport layer is now fully synchronous, this is a thin
/// delegation layer kept for API compatibility.
pub struct SyncTransport {
    transport: Arc<FlowControlTransport>,
}

impl SyncTransport {
    /// Wrap an existing flow-controlled transport
    pub fn new(transport: Arc<FlowControlTransport>) -> Self {
        Self { transport }
    }

    /// Open any supported device (auto-detecting wired vs dongle)
    pub fn open_any() -> Result<Self, TransportError> {
        let discovery = HidDiscovery::new();
        let devices = discovery.list_devices()?;

        if devices.is_empty() {
            return Err(TransportError::DeviceNotFound(
                "No supported device found".into(),
            ));
        }

        let raw_transport = discovery.open_device(&devices[0])?;
        let transport = Arc::new(FlowControlTransport::new(raw_transport));
        Ok(Self { transport })
    }

    /// Send a command without expecting response
    pub fn send_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        self.transport.send_command(cmd, data, checksum)
    }

    /// Send a command and wait for response
    pub fn query_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        self.transport.query_command(cmd, data, checksum)
    }

    /// Read a vendor event (blocking up to timeout)
    pub fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        self.transport.read_event(timeout_ms)
    }

    /// Get device info
    pub fn device_info(&self) -> &TransportDeviceInfo {
        self.transport.device_info()
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    /// Close the transport
    pub fn close(&self) -> Result<(), TransportError> {
        self.transport.close()
    }

    /// Send command and wait for any non-empty response without echo check
    pub fn query_raw(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        self.transport.query_raw(cmd, data, checksum)
    }

    /// Get the underlying flow-controlled transport
    pub fn inner(&self) -> &Arc<FlowControlTransport> {
        &self.transport
    }
}

/// List all connected devices synchronously
pub fn list_devices_sync() -> Result<Vec<crate::DiscoveredDevice>, TransportError> {
    let discovery = HidDiscovery::new();
    discovery.list_devices()
}

/// Open a specific device synchronously
pub fn open_device_sync(device: &crate::DiscoveredDevice) -> Result<SyncTransport, TransportError> {
    let discovery = HidDiscovery::new();
    let raw_transport = discovery.open_device(device)?;
    let transport = Arc::new(FlowControlTransport::new(raw_transport));
    Ok(SyncTransport { transport })
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
