// JSON profile loader
// Load device profiles from JSON files at runtime

use super::traits::DeviceProfile;
use super::types::{DeviceFeatures, FnSysLayer, RangeConfig, TravelSettings};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Device profile loaded from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonProfile {
    pub id: u32,
    pub vid: u16,
    pub pid: u16,
    pub name: String,
    pub display_name: String,
    #[serde(default = "default_company")]
    pub company: String,
    pub key_count: u8,
    #[serde(default = "default_matrix_size")]
    pub matrix_size: usize,
    #[serde(default = "default_layer_count")]
    pub layer_count: u8,
    pub led_matrix: Vec<u8>,
    pub matrix_key_names: Vec<String>,
    #[serde(default)]
    pub features: DeviceFeatures,
    #[serde(default)]
    pub fn_sys_layer: FnSysLayer,
    pub travel_settings: Option<JsonTravelSettings>,
}

fn default_company() -> String {
    "Unknown".to_string()
}

fn default_matrix_size() -> usize {
    126
}

fn default_layer_count() -> u8 {
    4
}

/// Travel settings in JSON format (camelCase)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonTravelSettings {
    pub travel: JsonRangeConfig,
    pub fire_press: JsonRangeConfig,
    pub fire_lift: JsonRangeConfig,
    pub deadzone: JsonRangeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRangeConfig {
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub default: f32,
}

impl From<JsonRangeConfig> for RangeConfig {
    fn from(json: JsonRangeConfig) -> Self {
        RangeConfig {
            min: json.min,
            max: json.max,
            step: json.step,
            default: json.default,
        }
    }
}

impl From<JsonTravelSettings> for TravelSettings {
    fn from(json: JsonTravelSettings) -> Self {
        TravelSettings {
            travel: json.travel.into(),
            fire_press: json.fire_press.into(),
            fire_lift: json.fire_lift.into(),
            deadzone: json.deadzone.into(),
        }
    }
}

impl JsonProfile {
    /// Load profile from a JSON file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, LoadError> {
        let content =
            std::fs::read_to_string(path.as_ref()).map_err(|e| LoadError::Io(e.to_string()))?;
        Self::load_from_json(&content)
    }

    /// Load profile from a JSON string
    pub fn load_from_json(json: &str) -> Result<Self, LoadError> {
        let profile: JsonProfile =
            serde_json::from_str(json).map_err(|e| LoadError::Parse(e.to_string()))?;
        profile.validate()?;
        Ok(profile)
    }

    /// Validate the profile data
    pub fn validate(&self) -> Result<(), LoadError> {
        // Check matrix sizes match
        if self.led_matrix.len() != self.matrix_size {
            return Err(LoadError::Validation(format!(
                "LED matrix length ({}) doesn't match matrix_size ({})",
                self.led_matrix.len(),
                self.matrix_size
            )));
        }

        if self.matrix_key_names.len() != self.matrix_size {
            return Err(LoadError::Validation(format!(
                "Key names length ({}) doesn't match matrix_size ({})",
                self.matrix_key_names.len(),
                self.matrix_size
            )));
        }

        // Check key count roughly matches active keys
        let active_count = self.led_matrix.iter().filter(|&&x| x != 0).count();
        if active_count > 0 && (active_count as i32 - self.key_count as i32).abs() > 10 {
            // Allow some tolerance for special keys
            return Err(LoadError::Validation(format!(
                "Active LED count ({}) significantly differs from key_count ({})",
                active_count, self.key_count
            )));
        }

        Ok(())
    }

    /// Convert travel settings to internal format
    fn get_travel_settings(&self) -> Option<TravelSettings> {
        self.travel_settings.clone().map(|ts| ts.into())
    }
}

/// Profile loading errors
#[derive(Debug, Clone)]
pub enum LoadError {
    Io(String),
    Parse(String),
    Validation(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "IO error: {e}"),
            LoadError::Parse(e) => write!(f, "Parse error: {e}"),
            LoadError::Validation(e) => write!(f, "Validation error: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Wrapper to implement DeviceProfile for JsonProfile
/// This is needed because JsonProfile stores TravelSettings as Option
/// and we need to return a reference to it
pub struct JsonProfileWrapper {
    profile: JsonProfile,
    travel_settings: Option<TravelSettings>,
}

impl JsonProfileWrapper {
    pub fn new(profile: JsonProfile) -> Self {
        let travel_settings = profile.get_travel_settings();
        Self {
            profile,
            travel_settings,
        }
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, LoadError> {
        let profile = JsonProfile::load_from_file(path)?;
        Ok(Self::new(profile))
    }

    pub fn from_json(json: &str) -> Result<Self, LoadError> {
        let profile = JsonProfile::load_from_json(json)?;
        Ok(Self::new(profile))
    }
}

impl DeviceProfile for JsonProfileWrapper {
    fn id(&self) -> u32 {
        self.profile.id
    }

    fn vid(&self) -> u16 {
        self.profile.vid
    }

    fn pid(&self) -> u16 {
        self.profile.pid
    }

    fn name(&self) -> &str {
        &self.profile.name
    }

    fn display_name(&self) -> &str {
        &self.profile.display_name
    }

    fn company(&self) -> &str {
        &self.profile.company
    }

    fn key_count(&self) -> u8 {
        self.profile.key_count
    }

    fn matrix_size(&self) -> usize {
        self.profile.matrix_size
    }

    fn layer_count(&self) -> u8 {
        self.profile.layer_count
    }

    fn led_matrix(&self) -> &[u8] {
        &self.profile.led_matrix
    }

    fn matrix_key_name(&self, position: u8) -> &str {
        self.profile
            .matrix_key_names
            .get(position as usize)
            .map(|s| s.as_str())
            .unwrap_or("?")
    }

    fn has_magnetism(&self) -> bool {
        self.profile.features.magnetism
    }

    fn has_sidelight(&self) -> bool {
        self.profile.features.sidelight
    }

    fn has_screen(&self) -> bool {
        self.profile.features.screen
    }

    fn has_knob(&self) -> bool {
        self.profile.features.knob
    }

    fn travel_settings(&self) -> Option<&TravelSettings> {
        self.travel_settings.as_ref()
    }

    fn fn_layer_win(&self) -> u8 {
        self.profile.fn_sys_layer.win
    }

    fn fn_layer_mac(&self) -> u8 {
        self.profile.fn_sys_layer.mac
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_JSON: &str = r#"{
        "id": 2679,
        "vid": 12625,
        "pid": 20528,
        "name": "m1v5he_test",
        "displayName": "Test M1 V5 HE",
        "company": "MonsGeek",
        "keyCount": 3,
        "matrixSize": 6,
        "layerCount": 4,
        "ledMatrix": [41, 53, 43, 0, 0, 0],
        "matrixKeyNames": ["Esc", "`", "Tab", "", "", ""],
        "features": {
            "magnetism": true,
            "sidelight": false
        },
        "fnSysLayer": {
            "win": 2,
            "mac": 2
        }
    }"#;

    #[test]
    fn test_load_json_profile() {
        let wrapper = JsonProfileWrapper::from_json(TEST_JSON).unwrap();

        assert_eq!(wrapper.vid(), 0x3151); // 12625 decimal
        assert_eq!(wrapper.pid(), 0x5030); // 20528 decimal
        assert_eq!(wrapper.display_name(), "Test M1 V5 HE");
        assert_eq!(wrapper.key_count(), 3);
        assert!(wrapper.has_magnetism());
        assert!(!wrapper.has_sidelight());
    }

    #[test]
    fn test_matrix_key_names() {
        let wrapper = JsonProfileWrapper::from_json(TEST_JSON).unwrap();

        assert_eq!(wrapper.matrix_key_name(0), "Esc");
        assert_eq!(wrapper.matrix_key_name(1), "`");
        assert_eq!(wrapper.matrix_key_name(2), "Tab");
        assert_eq!(wrapper.matrix_key_name(3), "");
        assert_eq!(wrapper.matrix_key_name(100), "?"); // Out of bounds
    }

    #[test]
    fn test_validation_matrix_size_mismatch() {
        let bad_json = r#"{
            "id": 1,
            "vid": 12625,
            "pid": 20528,
            "name": "bad",
            "displayName": "Bad",
            "keyCount": 3,
            "matrixSize": 10,
            "ledMatrix": [41, 53, 43],
            "matrixKeyNames": ["Esc", "`", "Tab"]
        }"#;

        let result = JsonProfile::load_from_json(bad_json);
        assert!(result.is_err());
        if let Err(LoadError::Validation(msg)) = result {
            assert!(msg.contains("LED matrix length"));
        }
    }

    #[test]
    fn test_with_travel_settings() {
        let json = r#"{
            "id": 1,
            "vid": 12625,
            "pid": 20528,
            "name": "test",
            "displayName": "Test",
            "keyCount": 3,
            "matrixSize": 3,
            "ledMatrix": [41, 53, 43],
            "matrixKeyNames": ["Esc", "`", "Tab"],
            "features": { "magnetism": true },
            "travelSettings": {
                "travel": { "min": 0.1, "max": 3.4, "step": 0.01, "default": 2.5 },
                "firePress": { "min": 0.01, "max": 2.5, "step": 0.01, "default": 1.5 },
                "fireLift": { "min": 0.01, "max": 2.5, "step": 0.01, "default": 1.5 },
                "deadzone": { "min": 0, "max": 1, "step": 0.01, "default": 0.3 }
            }
        }"#;

        let wrapper = JsonProfileWrapper::from_json(json).unwrap();
        let settings = wrapper.travel_settings().unwrap();

        assert!((settings.travel.min - 0.1).abs() < 0.001);
        assert!((settings.travel.max - 3.4).abs() < 0.001);
        assert!((settings.deadzone.default - 0.3).abs() < 0.001);
    }
}
