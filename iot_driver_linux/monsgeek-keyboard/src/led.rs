//! LED and RGB lighting types and utilities

pub use monsgeek_transport::command::{
    speed_from_wire, speed_to_wire, LedMode, Rgb as RgbColor, BRIGHTNESS_MAX, DAZZLE_OFF,
    DAZZLE_ON, SPEED_MAX,
};
use monsgeek_transport::command::{
    LedParamsResponse as TransportLedParamsResponse, SetLedParams as TransportSetLedParams,
};

/// LED parameters
#[derive(Debug, Clone)]
pub struct LedParams {
    /// Current mode
    pub mode: LedMode,
    /// Brightness (0-100)
    pub brightness: u8,
    /// Speed (mode-specific)
    pub speed: u8,
    /// Color (for modes that use it)
    pub color: RgbColor,
    /// Direction (for wave modes)
    pub direction: u8,
}

impl Default for LedParams {
    fn default() -> Self {
        Self {
            mode: LedMode::Constant,
            brightness: 100,
            speed: 50,
            color: RgbColor::WHITE,
            direction: 0,
        }
    }
}

impl LedParams {
    /// Convert to transport SetLedParams command
    pub fn to_transport_cmd(&self) -> TransportSetLedParams {
        let dazzle = (self.direction & 0x0F) == DAZZLE_ON;
        let layer = self.direction >> 4;

        TransportSetLedParams {
            mode: self.mode,
            speed: self.speed,
            brightness: self.brightness,
            color: self.color,
            dazzle,
            layer,
        }
    }

    /// Create from transport LedParamsResponse
    pub fn from_transport_response(resp: &TransportLedParamsResponse) -> Self {
        Self {
            mode: resp.mode,
            speed: resp.speed,
            brightness: resp.brightness,
            color: resp.color,
            direction: resp.option_raw,
        }
    }
}
