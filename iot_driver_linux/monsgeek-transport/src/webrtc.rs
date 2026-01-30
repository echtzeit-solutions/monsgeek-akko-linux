//! WebRTC transport for remote keyboard access
//!
//! This module provides WebRTC-based transport for accessing keyboards
//! remotely over the internet or local network. This enables use cases like:
//!
//! - Remote keyboard configuration from a web browser
//! - Cloud-based keyboard management
//! - Sharing keyboard state across devices
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐         ┌─────────────┐         ┌─────────────┐
//! │ Web Client  │◄─────►│ Signaling   │◄─────►│ Local Agent │
//! │ (Browser)   │  WS    │ Server      │  WS    │ (this crate)│
//! └─────────────┘        └─────────────┘        └─────────────┘
//!                                                      │
//!                                                      ▼
//!                                               ┌─────────────┐
//!                                               │  Keyboard   │
//!                                               │  (HID/BT)   │
//!                                               └─────────────┘
//! ```
//!
//! # Future Implementation Notes
//!
//! - Data channel for reliable command/response exchange
//! - Signaling via WebSocket to establish peer connection
//! - ICE for NAT traversal
//! - DTLS for security
//!
//! # Dependencies
//!
//! When implementing, consider using:
//! - `webrtc-rs` crate for WebRTC stack
//! - `tokio-tungstenite` for WebSocket signaling

use async_trait::async_trait;
use std::sync::Arc;

use crate::error::TransportError;
use crate::types::{ChecksumType, TransportDeviceInfo, VendorEvent};
use crate::Transport;

/// WebRTC transport for remote keyboard access (stub)
///
/// This transport allows remote access to keyboards via WebRTC data channels.
/// Commands are serialized and sent over the data channel, with responses
/// received asynchronously.
///
/// # Note
///
/// This is a placeholder for future WebRTC support.
/// Enable the `webrtc` feature to use this transport.
pub struct WebRtcTransport {
    device_info: TransportDeviceInfo,
    // Future fields:
    // peer_connection: RTCPeerConnection,
    // data_channel: RTCDataChannel,
    // pending_requests: HashMap<u32, oneshot::Sender<Vec<u8>>>,
}

/// Configuration for WebRTC transport
#[derive(Debug, Clone)]
pub struct WebRtcConfig {
    /// Signaling server URL (WebSocket)
    pub signaling_url: String,
    /// Room/session ID for peer discovery
    pub room_id: String,
    /// ICE servers for NAT traversal
    pub ice_servers: Vec<String>,
    /// Whether this is the offering peer (initiator)
    pub is_offerer: bool,
}

impl Default for WebRtcConfig {
    fn default() -> Self {
        Self {
            signaling_url: "wss://localhost:8443/signaling".into(),
            room_id: String::new(),
            ice_servers: vec!["stun:stun.l.google.com:19302".into()],
            is_offerer: false,
        }
    }
}

impl WebRtcTransport {
    /// Create a new WebRTC transport (stub)
    ///
    /// # Arguments
    ///
    /// * `config` - WebRTC configuration
    /// * `underlying` - The underlying transport to bridge (for local agent)
    ///
    /// # Note
    ///
    /// This currently returns an error as WebRTC is not yet implemented.
    pub async fn new(
        _config: WebRtcConfig,
        _underlying: Option<Arc<dyn Transport>>,
    ) -> Result<Self, TransportError> {
        Err(TransportError::WebRtcError(
            "WebRTC transport not yet implemented".into(),
        ))
    }

    /// Create a WebRTC transport as a client connecting to a remote keyboard
    pub async fn connect_remote(_config: WebRtcConfig) -> Result<Self, TransportError> {
        Err(TransportError::WebRtcError(
            "WebRTC transport not yet implemented".into(),
        ))
    }

    /// Create a WebRTC transport as a server exposing a local keyboard
    pub async fn expose_local(
        _config: WebRtcConfig,
        _transport: Arc<dyn Transport>,
    ) -> Result<Self, TransportError> {
        Err(TransportError::WebRtcError(
            "WebRTC transport not yet implemented".into(),
        ))
    }

    /// Get the connection state
    pub fn connection_state(&self) -> WebRtcState {
        WebRtcState::Disconnected
    }

    /// Get the peer ID (if connected)
    pub fn peer_id(&self) -> Option<&str> {
        None
    }
}

/// WebRTC connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebRtcState {
    /// Not connected
    Disconnected,
    /// Connecting (signaling in progress)
    Connecting,
    /// Connected and ready
    Connected,
    /// Connection failed
    Failed,
    /// Connection closed
    Closed,
}

#[async_trait]
impl Transport for WebRtcTransport {
    async fn send_report(
        &self,
        _cmd: u8,
        _data: &[u8],
        _checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        Err(TransportError::WebRtcError(
            "WebRTC transport not yet implemented".into(),
        ))
    }

    async fn read_report(&self) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::WebRtcError(
            "WebRTC transport not yet implemented".into(),
        ))
    }

    async fn read_event(&self, _timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        Err(TransportError::WebRtcError(
            "WebRTC transport not yet implemented".into(),
        ))
    }

    fn device_info(&self) -> &TransportDeviceInfo {
        &self.device_info
    }

    async fn is_connected(&self) -> bool {
        false
    }

    async fn close(&self) -> Result<(), TransportError> {
        Ok(())
    }

    async fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        Err(TransportError::WebRtcError(
            "WebRTC transport not yet implemented".into(),
        ))
    }
}

/// WebRTC signaling message types
#[derive(Debug, Clone)]
pub enum SignalingMessage {
    /// SDP offer
    Offer { sdp: String },
    /// SDP answer
    Answer { sdp: String },
    /// ICE candidate
    IceCandidate { candidate: String },
    /// Peer joined the room
    PeerJoined { peer_id: String },
    /// Peer left the room
    PeerLeft { peer_id: String },
}

/// WebRTC agent that exposes local keyboards to remote clients
pub struct WebRtcAgent {
    // Future fields:
    // config: WebRtcConfig,
    // local_transports: HashMap<String, Arc<dyn Transport>>,
    // connections: HashMap<String, WebRtcTransport>,
}

impl WebRtcAgent {
    /// Create a new WebRTC agent
    pub fn new(_config: WebRtcConfig) -> Result<Self, TransportError> {
        Err(TransportError::WebRtcError(
            "WebRTC transport not yet implemented".into(),
        ))
    }

    /// Start the agent (connect to signaling server)
    pub async fn start(&self) -> Result<(), TransportError> {
        Err(TransportError::WebRtcError(
            "WebRTC transport not yet implemented".into(),
        ))
    }

    /// Stop the agent
    pub async fn stop(&self) -> Result<(), TransportError> {
        Ok(())
    }

    /// Register a local transport to be exposed
    pub fn register_transport(
        &mut self,
        _name: &str,
        _transport: Arc<dyn Transport>,
    ) -> Result<(), TransportError> {
        Err(TransportError::WebRtcError(
            "WebRTC transport not yet implemented".into(),
        ))
    }
}
