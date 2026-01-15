//! Synchronous adapter for async transports
//!
//! This module provides blocking wrappers around the async transport layer,
//! enabling use in synchronous code (like TUI worker threads) without
//! requiring a full async runtime refactor.
//!
//! Note: When running within an async runtime (e.g., under `#[tokio::main]`),
//! these functions use `futures::executor::block_on` instead of creating
//! a new runtime to avoid nesting issues.

use std::sync::Arc;

use crate::error::TransportError;
use crate::types::{ChecksumType, TransportDeviceInfo, VendorEvent};
use crate::{DeviceDiscovery, HidDiscovery, Transport};

/// Block on a future, handling both runtime and non-runtime contexts
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    // Use futures crate's block_on which doesn't require a runtime
    futures::executor::block_on(f)
}

/// Synchronous wrapper around any async transport
///
/// This provides blocking versions of all Transport methods.
pub struct SyncTransport {
    transport: Arc<dyn Transport>,
}

impl SyncTransport {
    /// Wrap an existing async transport for synchronous use
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self { transport }
    }

    /// Open any supported device (auto-detecting wired vs dongle)
    pub fn open_any() -> Result<Self, TransportError> {
        let transport = block_on(async {
            let discovery = HidDiscovery::new();
            let devices = discovery.list_devices().await?;

            if devices.is_empty() {
                return Err(TransportError::DeviceNotFound(
                    "No supported device found".into(),
                ));
            }

            // Open the first device
            discovery.open_device(&devices[0]).await
        })?;

        Ok(Self { transport })
    }

    /// Send a command without expecting response (blocking)
    pub fn send_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        block_on(self.transport.send_command(cmd, data, checksum))
    }

    /// Send a command and wait for response (blocking)
    pub fn query_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        block_on(self.transport.query_command(cmd, data, checksum))
    }

    /// Read a vendor event (blocking)
    pub fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        block_on(self.transport.read_event(timeout_ms))
    }

    /// Get device info
    pub fn device_info(&self) -> &TransportDeviceInfo {
        self.transport.device_info()
    }

    /// Check if connected (blocking)
    pub fn is_connected(&self) -> bool {
        block_on(self.transport.is_connected())
    }

    /// Close the transport (blocking)
    pub fn close(&self) -> Result<(), TransportError> {
        block_on(self.transport.close())
    }

    /// Get the underlying async transport
    pub fn inner(&self) -> &Arc<dyn Transport> {
        &self.transport
    }
}

/// List all connected devices synchronously
pub fn list_devices_sync() -> Result<Vec<crate::DiscoveredDevice>, TransportError> {
    block_on(async {
        let discovery = HidDiscovery::new();
        discovery.list_devices().await
    })
}

/// Open a specific device synchronously
pub fn open_device_sync(device: &crate::DiscoveredDevice) -> Result<SyncTransport, TransportError> {
    let transport = block_on(async {
        let discovery = HidDiscovery::new();
        discovery.open_device(device).await
    })?;

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
