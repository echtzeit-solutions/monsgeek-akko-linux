//! Keyboard interface error types

use monsgeek_transport::TransportError;
use thiserror::Error;

/// Errors from keyboard operations
#[derive(Error, Debug)]
pub enum KeyboardError {
    /// Transport layer error
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    /// Invalid parameter value
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// Feature not supported by this device
    #[error("Feature not supported: {0}")]
    NotSupported(String),

    /// Device returned unexpected response
    #[error("Unexpected response: {0}")]
    UnexpectedResponse(String),

    /// Operation timed out
    #[error("Operation timed out")]
    Timeout,

    /// Device is offline (wireless only)
    #[error("Device is offline")]
    Offline,

    /// Device not found
    #[error("Device not found: {0}")]
    NotFound(String),
}
