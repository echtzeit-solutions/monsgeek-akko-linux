// Firmware API client for checking and downloading firmware updates
// Uses the MonsGeek/Akko Cloud API

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// API base URL
pub const API_BASE: &str = "https://api2.rongyuan.tech:3816/api/v2";

/// Download base URL
pub const DOWNLOAD_BASE: &str = "https://api2.rongyuan.tech:3816/download";

/// API error types
#[derive(Debug)]
pub enum ApiError {
    RequestError(String),
    ParseError(String),
    IoError(std::io::Error),
    ServerError(i32, String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestError(msg) => write!(f, "Request error: {msg}"),
            Self::ParseError(msg) => write!(f, "Parse error: {msg}"),
            Self::IoError(e) => write!(f, "I/O error: {e}"),
            Self::ServerError(code, msg) => write!(f, "Server error {code}: {msg}"),
        }
    }
}

impl std::error::Error for ApiError {}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

/// Parsed firmware version information
#[derive(Debug, Clone, Default)]
pub struct FirmwareVersions {
    /// USB/Main firmware version (hex value)
    pub usb: Option<u16>,
    /// RF receiver firmware version
    pub rf: Option<u16>,
    /// Matrix LED firmware version
    pub mled: Option<u16>,
    /// Nordic BLE firmware version
    pub nord: Option<u16>,
    /// OLED display firmware version
    pub oled: Option<u16>,
    /// Flash firmware version
    pub flash: Option<u16>,
    /// Download path from server
    pub download_path: Option<String>,
    /// Raw version string from server
    pub raw_version: String,
}

impl FirmwareVersions {
    /// Parse version string like "usb_665_rfv_42_mledv_1_nordv_3_oledv_0_flashv_5"
    pub fn parse(version_str: &str) -> Self {
        let mut result = Self {
            raw_version: version_str.to_string(),
            ..Default::default()
        };

        // Split by underscore and parse key-value pairs
        let parts: Vec<&str> = version_str.split('_').collect();

        let mut i = 0;
        while i < parts.len() {
            let key = parts[i].to_lowercase();

            // Look for version value in next part
            if i + 1 < parts.len() {
                if let Ok(val) = u16::from_str_radix(parts[i + 1], 16) {
                    match key.as_str() {
                        "usb" => result.usb = Some(val),
                        "rfv" | "rf" => result.rf = Some(val),
                        "mledv" | "mled" => result.mled = Some(val),
                        "nordv" | "nord" => result.nord = Some(val),
                        "oledv" | "oled" => result.oled = Some(val),
                        "flashv" | "flash" => result.flash = Some(val),
                        _ => {}
                    }
                    i += 2;
                    continue;
                }
            }
            i += 1;
        }

        result
    }

    /// Format as human-readable string
    pub fn display(&self) -> String {
        let mut parts = Vec::new();

        if let Some(v) = self.usb {
            parts.push(format!(
                "USB: {}.{}.{}",
                (v >> 8) & 0xF,
                (v >> 4) & 0xF,
                v & 0xF
            ));
        }
        if let Some(v) = self.rf {
            parts.push(format!("RF: 0x{v:X}"));
        }
        if let Some(v) = self.mled {
            parts.push(format!("MLED: {v}"));
        }
        if let Some(v) = self.nord {
            parts.push(format!("Nordic: {v}"));
        }
        if let Some(v) = self.oled {
            parts.push(format!("OLED: {v}"));
        }
        if let Some(v) = self.flash {
            parts.push(format!("Flash: {v}"));
        }

        if parts.is_empty() {
            "Unknown".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Check if any update is available compared to current versions
    pub fn has_updates(&self, current: &FirmwareVersions) -> bool {
        (self.usb.is_some() && current.usb.is_some() && self.usb > current.usb)
            || (self.rf.is_some() && current.rf.is_some() && self.rf > current.rf)
            || (self.mled.is_some() && current.mled.is_some() && self.mled > current.mled)
            || (self.nord.is_some() && current.nord.is_some() && self.nord > current.nord)
            || (self.oled.is_some() && current.oled.is_some() && self.oled > current.oled)
            || (self.flash.is_some() && current.flash.is_some() && self.flash > current.flash)
    }

    /// Get list of available updates
    pub fn get_updates(&self, current: &FirmwareVersions) -> Vec<(String, u16, u16)> {
        let mut updates = Vec::new();

        if let (Some(new), Some(old)) = (self.usb, current.usb) {
            if new > old {
                updates.push(("USB".to_string(), old, new));
            }
        }
        if let (Some(new), Some(old)) = (self.rf, current.rf) {
            if new > old {
                updates.push(("RF".to_string(), old, new));
            }
        }
        if let (Some(new), Some(old)) = (self.mled, current.mled) {
            if new > old {
                updates.push(("MLED".to_string(), old, new));
            }
        }
        if let (Some(new), Some(old)) = (self.nord, current.nord) {
            if new > old {
                updates.push(("Nordic".to_string(), old, new));
            }
        }
        if let (Some(new), Some(old)) = (self.oled, current.oled) {
            if new > old {
                updates.push(("OLED".to_string(), old, new));
            }
        }
        if let (Some(new), Some(old)) = (self.flash, current.flash) {
            if new > old {
                updates.push(("Flash".to_string(), old, new));
            }
        }

        updates
    }
}

/// API response for firmware version check
#[derive(Debug)]
pub struct FirmwareCheckResponse {
    /// Parsed firmware versions
    pub versions: FirmwareVersions,
    /// Minimum app version required
    pub lowest_app_version: Option<String>,
}

/// Check firmware version from API (blocking)
#[cfg(feature = "firmware-api")]
pub fn check_firmware_blocking(device_id: u32) -> Result<FirmwareCheckResponse, ApiError> {
    use reqwest::blocking::Client;

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::RequestError(e.to_string()))?;

    let url = format!("{API_BASE}/get_fw_version");

    let mut body = HashMap::new();
    body.insert("dev_id", device_id);

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .map_err(|e| ApiError::RequestError(e.to_string()))?;

    if !response.status().is_success() {
        return Err(ApiError::ServerError(
            response.status().as_u16() as i32,
            response.status().to_string(),
        ));
    }

    let json: serde_json::Value = response
        .json()
        .map_err(|e| ApiError::ParseError(e.to_string()))?;

    // Check error code
    if let Some(err_code) = json.get("errCode").and_then(|v| v.as_i64()) {
        if err_code != 0 {
            return Err(ApiError::ServerError(
                err_code as i32,
                "API error".to_string(),
            ));
        }
    }

    // Parse data
    let data = json
        .get("data")
        .ok_or_else(|| ApiError::ParseError("No data in response".to_string()))?;

    let version_str = data
        .get("version_str")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut versions = FirmwareVersions::parse(version_str);

    // Get download path
    versions.download_path = data
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let lowest_app_version = data
        .get("lowest_app_version_str")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(FirmwareCheckResponse {
        versions,
        lowest_app_version,
    })
}

/// Download firmware file from server (blocking)
#[cfg(feature = "firmware-api")]
pub fn download_firmware_blocking<P: AsRef<Path>>(
    download_path: &str,
    output: P,
) -> Result<usize, ApiError> {
    use reqwest::blocking::Client;

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| ApiError::RequestError(e.to_string()))?;

    let url = format!("{DOWNLOAD_BASE}{download_path}");

    let response = client
        .get(&url)
        .send()
        .map_err(|e| ApiError::RequestError(e.to_string()))?;

    if !response.status().is_success() {
        return Err(ApiError::ServerError(
            response.status().as_u16() as i32,
            response.status().to_string(),
        ));
    }

    let bytes = response
        .bytes()
        .map_err(|e| ApiError::RequestError(e.to_string()))?;

    let size = bytes.len();
    fs::write(output, &bytes)?;

    Ok(size)
}

/// Check firmware version from API (async)
#[cfg(feature = "firmware-api-async")]
pub async fn check_firmware(device_id: u32) -> Result<FirmwareCheckResponse, ApiError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::RequestError(e.to_string()))?;

    let url = format!("{API_BASE}/get_fw_version");

    let mut body = HashMap::new();
    body.insert("dev_id", device_id);

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::RequestError(e.to_string()))?;

    if !response.status().is_success() {
        return Err(ApiError::ServerError(
            response.status().as_u16() as i32,
            response.status().to_string(),
        ));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| ApiError::ParseError(e.to_string()))?;

    // Check error code
    if let Some(err_code) = json.get("errCode").and_then(|v| v.as_i64()) {
        if err_code != 0 {
            return Err(ApiError::ServerError(
                err_code as i32,
                "API error".to_string(),
            ));
        }
    }

    // Parse data
    let data = json
        .get("data")
        .ok_or_else(|| ApiError::ParseError("No data in response".to_string()))?;

    let version_str = data
        .get("version_str")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut versions = FirmwareVersions::parse(version_str);

    versions.download_path = data
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let lowest_app_version = data
        .get("lowest_app_version_str")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(FirmwareCheckResponse {
        versions,
        lowest_app_version,
    })
}

/// Download firmware file from server (async)
#[cfg(feature = "firmware-api-async")]
pub async fn download_firmware<P: AsRef<Path>>(
    download_path: &str,
    output: P,
) -> Result<usize, ApiError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| ApiError::RequestError(e.to_string()))?;

    let url = format!("{DOWNLOAD_BASE}{download_path}");

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| ApiError::RequestError(e.to_string()))?;

    if !response.status().is_success() {
        return Err(ApiError::ServerError(
            response.status().as_u16() as i32,
            response.status().to_string(),
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| ApiError::RequestError(e.to_string()))?;

    let size = bytes.len();
    fs::write(output, &bytes)?;

    Ok(size)
}

/// Known device IDs
pub mod device_ids {
    /// MonsGeek M1 V5 HE / M1 V5 TMR
    /// Note: The device reports this ID directly via GET_USB_VERSION command
    pub const M1_V5_HE: u32 = 2949;

    /// Get device ID from VID/PID if known (fallback when device query fails)
    /// Prefer using MonsGeekDevice::get_api_device_id() which queries the device directly
    pub fn from_vid_pid(vid: u16, pid: u16) -> Option<u32> {
        match (vid, pid) {
            (0x3151, 0x5030) => Some(M1_V5_HE),
            (0x3151, 0x503A) => Some(M1_V5_HE), // Wireless mode
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version_string() {
        let versions = FirmwareVersions::parse("usb_665_rfv_42_mledv_1_nordv_3_oledv_0_flashv_5");

        assert_eq!(versions.usb, Some(0x665));
        assert_eq!(versions.rf, Some(0x42));
        assert_eq!(versions.mled, Some(0x1));
        assert_eq!(versions.nord, Some(0x3));
        assert_eq!(versions.oled, Some(0x0));
        assert_eq!(versions.flash, Some(0x5));
    }

    #[test]
    fn test_partial_version_string() {
        let versions = FirmwareVersions::parse("usb_123");

        assert_eq!(versions.usb, Some(0x123));
        assert_eq!(versions.rf, None);
    }

    #[test]
    fn test_has_updates() {
        let current = FirmwareVersions {
            usb: Some(0x100),
            rf: Some(0x10),
            ..Default::default()
        };

        let available = FirmwareVersions {
            usb: Some(0x110),
            rf: Some(0x10),
            ..Default::default()
        };

        assert!(available.has_updates(&current));

        let no_update = FirmwareVersions {
            usb: Some(0x100),
            rf: Some(0x10),
            ..Default::default()
        };

        assert!(!no_update.has_updates(&current));
    }
}
