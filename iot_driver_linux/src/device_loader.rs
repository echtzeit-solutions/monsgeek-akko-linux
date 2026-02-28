// Device Loader - Load device definitions from JSON files
// Supports loading from embedded JSON or external file

use crate::profile::types::FnSysLayer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};

/// Travel range configuration from JSON
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JsonRangeConfig {
    pub min: f32,
    pub max: f32,
    #[serde(default)]
    pub step: Option<f32>,
    #[serde(default)]
    pub default: Option<f32>,
}

/// Travel settings from JSON
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonTravelSetting {
    pub travel: Option<JsonRangeConfig>,
    pub fire_press: Option<JsonRangeConfig>,
    pub fire_lift: Option<JsonRangeConfig>,
    pub deadzone: Option<JsonRangeConfig>,
}

/// Device definition loaded from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonDeviceDefinition {
    /// Device ID (can be negative for special devices)
    pub id: i32,
    pub vid: u16,
    pub pid: u16,
    #[serde(default)]
    pub vid_hex: String,
    #[serde(default)]
    pub pid_hex: String,
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(rename = "type", default = "default_type")]
    pub device_type: String,
    #[serde(default)]
    pub sources: Vec<String>,
    // Feature fields
    #[serde(default)]
    pub key_count: Option<u8>,
    #[serde(default)]
    pub key_layout_name: Option<String>,
    #[serde(default)]
    pub layer: Option<u8>,
    #[serde(default)]
    pub fn_sys_layer: Option<FnSysLayer>,
    /// True if device has magnetic (Hall effect) switches
    #[serde(default)]
    pub magnetism: Option<bool>,
    /// True if device explicitly does NOT have magnetic switches
    /// (opposite of magnetism, used in some device definitions)
    #[serde(default)]
    pub no_magnetic_switch: Option<bool>,
    #[serde(default)]
    pub has_light_layout: Option<bool>,
    #[serde(default)]
    pub has_side_light: Option<bool>,
    #[serde(default)]
    pub hot_swap: Option<bool>,
    #[serde(default)]
    pub travel_setting: Option<JsonTravelSetting>,
    /// LED matrix mapping position index to HID keycode
    /// Used for LED effects and depth report key identification
    #[serde(default)]
    pub led_matrix: Option<Vec<u8>>,
    /// Chip family (e.g., "RY5088", "YC3123")
    #[serde(default)]
    pub chip_family: Option<String>,
}

impl JsonDeviceDefinition {
    /// Check if this device has magnetism (Hall effect switches)
    /// Returns true if magnetism is explicitly true, or if no_magnetic_switch is explicitly false
    pub fn has_magnetism(&self) -> bool {
        if let Some(magnetism) = self.magnetism {
            return magnetism;
        }
        if let Some(no_magnetic) = self.no_magnetic_switch {
            return !no_magnetic;
        }
        false
    }

    /// Get the company name, falling back to "Unknown" if not set
    pub fn company_or_unknown(&self) -> &str {
        self.company.as_deref().unwrap_or("Unknown")
    }

    /// Get key name for a matrix position index
    pub fn key_name(&self, index: usize) -> Option<&'static str> {
        let matrix = self.led_matrix.as_ref()?;
        let hid_code = *matrix.get(index)?;
        let name = crate::protocol::hid::key_name(hid_code);
        if name == "?" || name == "None" {
            None
        } else {
            Some(name)
        }
    }

    /// Find matrix position index for a key name (case-insensitive)
    pub fn key_index(&self, name: &str) -> Option<usize> {
        let matrix = self.led_matrix.as_ref()?;
        let target_hid = crate::protocol::hid::key_code_from_name(name)?;
        matrix.iter().position(|&hid| hid == target_hid)
    }

    /// Get all key indices for WASD keys
    pub fn wasd_indices(&self) -> Option<(usize, usize, usize, usize)> {
        Some((
            self.key_index("W")?,
            self.key_index("A")?,
            self.key_index("S")?,
            self.key_index("D")?,
        ))
    }
}

/// Wrapper for the versioned devices.json format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonDeviceFile {
    pub version: u32,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub source_file: Option<String>,
    #[serde(default)]
    pub device_arrays: Vec<String>,
    #[serde(default)]
    pub device_count: Option<u32>,
    #[serde(default)]
    pub key_layout_count: Option<u32>,
    pub devices: Vec<JsonDeviceDefinition>,
}

fn default_type() -> String {
    "keyboard".to_string()
}

/// Device matrix entry from device_matrices.json
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonDeviceMatrix {
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub key_layout_name: Option<String>,
    pub key_count: u16,
    pub match_method: String,
    pub matrix: Vec<u8>,
    pub key_names: Vec<Option<String>>,
}

impl JsonDeviceMatrix {
    /// Get key name for a matrix position
    pub fn key_name(&self, index: usize) -> Option<&str> {
        self.key_names
            .get(index)
            .and_then(|n| n.as_deref())
            .filter(|s| !s.is_empty())
    }

    /// Get matrix position for a key name (case-insensitive)
    pub fn key_index(&self, name: &str) -> Option<usize> {
        let target = name.to_lowercase();
        self.key_names.iter().position(|n| {
            n.as_deref()
                .map(|s| s.to_lowercase() == target)
                .unwrap_or(false)
        })
    }

    /// Get HID code at position
    pub fn hid_code(&self, index: usize) -> Option<u8> {
        self.matrix.get(index).copied().filter(|&c| c != 0)
    }
}

/// Wrapper for device_matrices.json file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonDeviceMatricesFile {
    pub version: u32,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub stats: Option<serde_json::Value>,
    #[serde(default)]
    pub hid_to_key: Option<HashMap<String, Option<String>>>,
    pub devices: HashMap<String, JsonDeviceMatrix>,
}

/// Device database loaded from JSON
#[derive(Debug)]
pub struct DeviceDatabase {
    /// All devices indexed by ID
    devices_by_id: HashMap<i32, JsonDeviceDefinition>,
    /// Devices indexed by (VID, PID) -> list of matching device IDs
    devices_by_vid_pid: HashMap<(u16, u16), Vec<i32>>,
    /// Devices indexed by company
    devices_by_company: HashMap<String, Vec<i32>>,
    /// Version of the loaded database
    version: u32,
    /// Device matrices (loaded from device_matrices.json)
    matrices: HashMap<i32, JsonDeviceMatrix>,
}

/// Default paths to search for devices.json
const DEFAULT_DEVICE_DB_PATHS: &[&str] = &[
    "/usr/local/share/akko/devices.json",
    "/usr/share/akko/devices.json",
    "data/devices.json",
    "../data/devices.json", // When running from iot_driver_linux/
];

/// Default paths to search for device_matrices.json
const DEFAULT_MATRIX_DB_PATHS: &[&str] = &[
    "/usr/local/share/akko/device_matrices.json",
    "/usr/share/akko/device_matrices.json",
    "data/device_matrices.json",
    "../data/device_matrices.json",
];

impl DeviceDatabase {
    /// Create empty database
    pub fn new() -> Self {
        Self {
            devices_by_id: HashMap::new(),
            devices_by_vid_pid: HashMap::new(),
            devices_by_company: HashMap::new(),
            version: 0,
            matrices: HashMap::new(),
        }
    }

    /// Load from default paths (tries each in order)
    pub fn load_default() -> Result<Self, String> {
        let mut db = None;
        for path in DEFAULT_DEVICE_DB_PATHS {
            let p = Path::new(path);
            if p.exists() {
                match Self::load_from_file(p) {
                    Ok(loaded) => {
                        info!(
                            "Loaded device database from {} ({} devices, version {})",
                            path,
                            loaded.len(),
                            loaded.version
                        );
                        db = Some(loaded);
                        break;
                    }
                    Err(e) => {
                        warn!("Failed to load device database from {}: {}", path, e);
                    }
                }
            }
        }

        let mut db = db.ok_or_else(|| {
            format!(
                "Device database not found in any of: {:?}",
                DEFAULT_DEVICE_DB_PATHS
            )
        })?;

        // Also try to load matrices
        for path in DEFAULT_MATRIX_DB_PATHS {
            let p = Path::new(path);
            if p.exists() {
                match db.load_matrices_from_file(p) {
                    Ok(count) => {
                        info!("Loaded {} device matrices from {}", count, path);
                        break;
                    }
                    Err(e) => {
                        warn!("Failed to load device matrices from {}: {}", path, e);
                    }
                }
            }
        }

        Ok(db)
    }

    /// Load device matrices from a JSON file
    pub fn load_matrices_from_file<P: AsRef<Path>>(&mut self, path: P) -> Result<usize, String> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Failed to read matrices file: {e}"))?;
        self.load_matrices_from_json(&content)
    }

    /// Load device matrices from JSON string
    pub fn load_matrices_from_json(&mut self, json: &str) -> Result<usize, String> {
        let file: JsonDeviceMatricesFile = serde_json::from_str(json)
            .map_err(|e| format!("Failed to parse matrices JSON: {e}"))?;

        let mut count = 0;
        for (id_str, matrix) in file.devices {
            if let Ok(id) = id_str.parse::<i32>() {
                self.matrices.insert(id, matrix);
                count += 1;
            } else {
                warn!("Invalid device ID in matrices: {}", id_str);
            }
        }
        Ok(count)
    }

    /// Load devices from JSON file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Failed to read file: {e}"))?;
        Self::load_from_json(&content)
    }

    /// Load devices from JSON string
    /// Supports both the new versioned format and the old array format
    pub fn load_from_json(json: &str) -> Result<Self, String> {
        // Try versioned format first
        if let Ok(file) = serde_json::from_str::<JsonDeviceFile>(json) {
            let mut db = Self::new();
            db.version = file.version;
            for device in file.devices {
                db.add_device(device);
            }
            return Ok(db);
        }

        // Fall back to old array format
        let devices: Vec<JsonDeviceDefinition> =
            serde_json::from_str(json).map_err(|e| format!("Failed to parse JSON: {e}"))?;

        let mut db = Self::new();
        for device in devices {
            db.add_device(device);
        }
        Ok(db)
    }

    /// Add a device to the database
    pub fn add_device(&mut self, device: JsonDeviceDefinition) {
        let id = device.id;
        let vid_pid = (device.vid, device.pid);
        let company = device.company.clone().unwrap_or_default();

        // Index by VID/PID
        self.devices_by_vid_pid.entry(vid_pid).or_default().push(id);

        // Index by company (skip empty company names)
        if !company.is_empty() {
            self.devices_by_company.entry(company).or_default().push(id);
        }

        // Store device
        self.devices_by_id.insert(id, device);
    }

    /// Find device by ID
    pub fn find_by_id(&self, id: i32) -> Option<&JsonDeviceDefinition> {
        self.devices_by_id.get(&id)
    }

    /// Find all devices with matching VID/PID
    pub fn find_by_vid_pid(&self, vid: u16, pid: u16) -> Vec<&JsonDeviceDefinition> {
        self.devices_by_vid_pid
            .get(&(vid, pid))
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.devices_by_id.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Find first device with matching VID/PID (prioritize by company)
    pub fn find_by_vid_pid_company(
        &self,
        vid: u16,
        pid: u16,
        preferred_company: &str,
    ) -> Option<&JsonDeviceDefinition> {
        let matches = self.find_by_vid_pid(vid, pid);

        // Try to find matching company first
        if let Some(dev) = matches.iter().find(|d| {
            d.company
                .as_deref()
                .map(|c| c.eq_ignore_ascii_case(preferred_company))
                .unwrap_or(false)
        }) {
            return Some(dev);
        }

        // Fall back to first match
        matches.into_iter().next()
    }

    /// Get all devices for a company
    pub fn find_by_company(&self, company: &str) -> Vec<&JsonDeviceDefinition> {
        self.devices_by_company
            .get(company)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.devices_by_id.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all unique companies
    pub fn get_companies(&self) -> Vec<&str> {
        self.devices_by_company.keys().map(|s| s.as_str()).collect()
    }

    /// Get all devices
    pub fn all_devices(&self) -> impl Iterator<Item = &JsonDeviceDefinition> {
        self.devices_by_id.values()
    }

    /// Get device count
    pub fn len(&self) -> usize {
        self.devices_by_id.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.devices_by_id.is_empty()
    }

    /// Get all unique VID/PID combinations
    pub fn get_all_vid_pids(&self) -> Vec<(u16, u16)> {
        self.devices_by_vid_pid.keys().cloned().collect()
    }

    /// Check if VID/PID is in database
    pub fn has_vid_pid(&self, vid: u16, pid: u16) -> bool {
        self.devices_by_vid_pid.contains_key(&(vid, pid))
    }

    /// Get database version
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Get device matrix by device ID
    pub fn get_matrix(&self, device_id: i32) -> Option<&JsonDeviceMatrix> {
        self.matrices.get(&device_id)
    }

    /// Get key name for a device's matrix position
    pub fn device_key_name(&self, device_id: i32, position: usize) -> Option<&str> {
        self.matrices
            .get(&device_id)
            .and_then(|m| m.key_name(position))
    }

    /// Get matrix position for a key name on a device
    pub fn device_key_index(&self, device_id: i32, name: &str) -> Option<usize> {
        self.matrices
            .get(&device_id)
            .and_then(|m| m.key_index(name))
    }

    /// Get HID code for a device's matrix position
    pub fn device_hid_code(&self, device_id: i32, position: usize) -> Option<u8> {
        self.matrices
            .get(&device_id)
            .and_then(|m| m.hid_code(position))
    }

    /// Get number of loaded matrices
    pub fn matrices_len(&self) -> usize {
        self.matrices.len()
    }
}

impl Default for DeviceDatabase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Legacy array format
    const TEST_JSON_LEGACY: &str = r#"[
        {"id": 2949, "vid": 12625, "pid": 20528, "name": "m1v5he", "displayName": "M1 V5 TMR", "company": "MonsGeek"},
        {"id": 2585, "vid": 12625, "pid": 20528, "name": "m3v5", "displayName": "M3 V5", "company": "MonsGeek"},
        {"id": 100, "vid": 1234, "pid": 5678, "name": "akko_k1", "displayName": "K1", "company": "akko"}
    ]"#;

    // New versioned format with features
    const TEST_JSON_VERSIONED: &str = r#"{
        "version": 1,
        "devices": [
            {
                "id": 2248,
                "vid": 12625,
                "pid": 20528,
                "name": "m1v5he",
                "displayName": "M1 V5 TMR",
                "type": "keyboard",
                "company": "MonsGeek",
                "keyCount": 82,
                "keyLayoutName": "Common82_M1_V5_TMR",
                "layer": 4,
                "fnSysLayer": {"win": 2, "mac": 2},
                "magnetism": true,
                "hasLightLayout": true,
                "hotSwap": true
            },
            {
                "id": -100,
                "vid": 12625,
                "pid": 16405,
                "name": "help",
                "displayName": "Help Device",
                "type": "keyboard",
                "company": null
            },
            {
                "id": 1000,
                "vid": 12625,
                "pid": 16405,
                "name": "non_magnetic",
                "displayName": "Non-Magnetic",
                "type": "keyboard",
                "company": "akko",
                "noMagneticSwitch": true
            }
        ]
    }"#;

    #[test]
    fn test_load_legacy_json() {
        let db = DeviceDatabase::load_from_json(TEST_JSON_LEGACY).unwrap();
        assert_eq!(db.len(), 3);
        assert_eq!(db.version(), 0); // Legacy format has no version
    }

    #[test]
    fn test_load_versioned_json() {
        let db = DeviceDatabase::load_from_json(TEST_JSON_VERSIONED).unwrap();
        assert_eq!(db.len(), 3);
        assert_eq!(db.version(), 1);
    }

    #[test]
    fn test_find_by_id() {
        let db = DeviceDatabase::load_from_json(TEST_JSON_VERSIONED).unwrap();
        let dev = db.find_by_id(2248).unwrap();
        assert_eq!(dev.display_name, "M1 V5 TMR");
        assert_eq!(dev.company.as_deref(), Some("MonsGeek"));
    }

    #[test]
    fn test_find_by_negative_id() {
        let db = DeviceDatabase::load_from_json(TEST_JSON_VERSIONED).unwrap();
        let dev = db.find_by_id(-100).unwrap();
        assert_eq!(dev.display_name, "Help Device");
        assert!(dev.company.is_none());
    }

    #[test]
    fn test_find_by_vid_pid() {
        let db = DeviceDatabase::load_from_json(TEST_JSON_LEGACY).unwrap();
        let matches = db.find_by_vid_pid(12625, 20528);
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_find_by_company() {
        let db = DeviceDatabase::load_from_json(TEST_JSON_LEGACY).unwrap();
        let monsgeek = db.find_by_company("MonsGeek");
        assert_eq!(monsgeek.len(), 2);
    }

    #[test]
    fn test_device_features() {
        let db = DeviceDatabase::load_from_json(TEST_JSON_VERSIONED).unwrap();

        // Device with magnetism
        let dev = db.find_by_id(2248).unwrap();
        assert!(dev.has_magnetism());
        assert_eq!(dev.key_count, Some(82));
        assert!(dev.hot_swap.unwrap_or(false));
        assert_eq!(dev.layer, Some(4));

        // Device with noMagneticSwitch: true means has_magnetism() is false
        let dev = db.find_by_id(1000).unwrap();
        assert!(!dev.has_magnetism());
    }

    #[test]
    fn test_fn_sys_layer() {
        let db = DeviceDatabase::load_from_json(TEST_JSON_VERSIONED).unwrap();
        let dev = db.find_by_id(2248).unwrap();
        let fn_layer = dev.fn_sys_layer.as_ref().unwrap();
        assert_eq!(fn_layer.win, 2);
        assert_eq!(fn_layer.mac, 2);
    }

    #[test]
    #[ignore] // Run manually with: cargo test test_load_actual_db -- --ignored
    fn test_load_actual_db() {
        // This test loads the actual devices.json file
        // Path is relative to crate root (iot_driver_linux/)
        let db = DeviceDatabase::load_from_file("../data/devices.json")
            .or_else(|_| DeviceDatabase::load_from_file("data/devices.json"))
            .expect("Could not load devices.json from ../data/ or data/");
        assert!(db.len() > 100, "Expected many devices, got {}", db.len());
        assert_eq!(db.version(), 1);

        // Find M1 V5 TMR (our primary test device)
        let m1v5_matches = db.find_by_vid_pid(0x3151, 0x5030);
        assert!(!m1v5_matches.is_empty(), "M1 V5 TMR should be in database");

        // Check that magnetism is detected
        let m1v5 = m1v5_matches
            .iter()
            .find(|d| d.display_name.contains("TMR"))
            .expect("M1 V5 TMR not found");
        assert!(m1v5.has_magnetism(), "M1 V5 TMR should have magnetism");
        assert_eq!(m1v5.key_count, Some(82));

        println!(
            "Loaded {} devices from version {} database",
            db.len(),
            db.version()
        );
    }

    #[test]
    fn test_led_matrix() {
        // Test JSON with LED matrix
        let json = r#"{
            "version": 2,
            "devices": [{
                "id": 1,
                "vid": 12625,
                "pid": 20528,
                "name": "test",
                "displayName": "Test",
                "ledMatrix": [41, 53, 43, 57, 225, 224, 58, 30, 20, 4, 29, 225, 224, 227, 59, 26, 22, 27]
            }]
        }"#;
        // Matrix positions: 0=Esc(41), 1=`(53), 2=Tab(43), 3=Caps(57), 4=LShift(225), 5=LCtrl(224)
        //                   6=F1(58), 7=1(30), 8=Q(20), 9=A(4), 10=Z(29), 11=LShift(225), 12=LCtrl(224)
        //                   13=LWin(227), 14=F2(59), 15=W(26), 16=S(22), 17=X(27)

        let db = DeviceDatabase::load_from_json(json).unwrap();
        let dev = db.find_by_id(1).unwrap();

        // Test key name lookup (canonical HID names from protocol::hid::key_name)
        assert_eq!(dev.key_name(0), Some("Escape"));
        assert_eq!(dev.key_name(9), Some("A"));
        assert_eq!(dev.key_name(15), Some("W"));
        assert_eq!(dev.key_name(16), Some("S"));
        assert_eq!(dev.key_name(100), None); // Out of bounds

        // Test key index lookup (supports aliases)
        assert_eq!(dev.key_index("Escape"), Some(0));
        assert_eq!(dev.key_index("A"), Some(9));
        assert_eq!(dev.key_index("W"), Some(15));
        assert_eq!(dev.key_index("S"), Some(16));
        assert_eq!(dev.key_index("nonexistent"), None);

        // Test case insensitivity
        assert_eq!(dev.key_index("esc"), Some(0));
        assert_eq!(dev.key_index("ESC"), Some(0));
        assert_eq!(dev.key_index("Escape"), Some(0));
    }

    #[test]
    fn test_hid_code_conversion() {
        use crate::protocol::hid::{key_code_from_name, key_name};

        // Letters
        assert_eq!(key_name(4), "A");
        assert_eq!(key_name(26), "W");
        assert_eq!(key_name(22), "S");
        assert_eq!(key_name(7), "D");

        // Numbers
        assert_eq!(key_name(30), "1");
        assert_eq!(key_name(39), "0");

        // Modifiers
        assert_eq!(key_name(224), "LCtrl");
        assert_eq!(key_name(225), "LShift");

        // Reverse
        assert_eq!(key_code_from_name("A"), Some(4));
        assert_eq!(key_code_from_name("w"), Some(26)); // Case insensitive
        assert_eq!(key_code_from_name("ESCAPE"), Some(41));
        assert_eq!(key_code_from_name("lshift"), Some(225));
    }

    #[test]
    #[ignore] // Run manually with: cargo test test_m1v5_matrix -- --ignored --nocapture
    fn test_m1v5_matrix() {
        let mut db = DeviceDatabase::load_from_file("../data/devices.json")
            .or_else(|_| DeviceDatabase::load_from_file("data/devices.json"))
            .expect("Could not load devices.json");

        // Load matrices (resolved from class hierarchy, not inline in devices.json)
        db.load_matrices_from_file("../data/device_matrices.json")
            .or_else(|_| db.load_matrices_from_file("data/device_matrices.json"))
            .expect("Could not load device_matrices.json");

        // Find M1 V5 HE device (id 2819)
        let m1v5 = db
            .find_by_vid_pid(0x3151, 0x5030)
            .into_iter()
            .find(|d| d.display_name == "M1 V5 HE")
            .expect("M1 V5 HE not found in devices.json");

        println!("Testing device: {} (id={})", m1v5.display_name, m1v5.id);

        // Look up matrix from device_matrices.json
        let matrix = db
            .get_matrix(m1v5.id)
            .expect("M1 V5 HE matrix not found in device_matrices.json");

        println!(
            "Matrix: {} keys, {} positions",
            matrix.key_count,
            matrix.matrix.len()
        );

        // Verify WASD indices match expected
        let w = matrix.key_index("W").expect("W not found");
        let a = matrix.key_index("A").expect("A not found");
        let s = matrix.key_index("S").expect("S not found");
        let d = matrix.key_index("D").expect("D not found");
        println!("WASD indices: W={}, A={}, S={}, D={}", w, a, s, d);

        assert_eq!(w, 14, "W should be at index 14");
        assert_eq!(a, 9, "A should be at index 9");
        assert_eq!(s, 15, "S should be at index 15");
        assert_eq!(d, 21, "D should be at index 21");
    }

    #[test]
    #[ignore] // Run manually with: cargo test test_device_matrices -- --ignored --nocapture
    fn test_device_matrices() {
        let mut db = DeviceDatabase::load_from_file("../data/devices.json")
            .or_else(|_| DeviceDatabase::load_from_file("data/devices.json"))
            .expect("Could not load devices.json");

        // Load matrices
        let count = db
            .load_matrices_from_file("../data/device_matrices.json")
            .or_else(|_| db.load_matrices_from_file("data/device_matrices.json"))
            .expect("Could not load device_matrices.json");

        println!("Loaded {} device matrices", count);
        assert!(count > 300, "Expected 300+ matrices, got {}", count);

        // Test M1 V5 TMR (device 2247)
        let matrix = db.get_matrix(2247).expect("M1 V5 TMR matrix not found");
        assert_eq!(matrix.display_name, "M1 V5 TMR");
        assert_eq!(matrix.key_count, 85);

        // Test key lookup - position 28 should be C
        assert_eq!(matrix.hid_code(28), Some(6), "Position 28 should be HID 6");
        assert_eq!(
            matrix.key_name(28),
            Some("C"),
            "Position 28 should be C key"
        );

        // Test helper methods on database
        assert_eq!(db.device_key_name(2247, 28), Some("C"));
        assert_eq!(db.device_key_name(2247, 0), Some("Esc"));
        assert_eq!(db.device_hid_code(2247, 28), Some(6));

        // Test key index lookup (uses key_names from JSON, not HID canonical names)
        assert_eq!(matrix.key_index("C"), Some(28));
        assert_eq!(matrix.key_index("Esc"), Some(0));
        assert_eq!(db.device_key_index(2247, "C"), Some(28));

        println!(
            "M1 V5 TMR: position 28 = HID {} = {}",
            matrix.matrix[28],
            matrix.key_name(28).unwrap_or("?")
        );
    }
}
