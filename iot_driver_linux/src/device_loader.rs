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
        hid_code_to_name(hid_code)
    }

    /// Find matrix position index for a key name (case-insensitive)
    pub fn key_index(&self, name: &str) -> Option<usize> {
        let matrix = self.led_matrix.as_ref()?;
        let target_hid = name_to_hid_code(name)?;
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

/// Convert HID keycode to key name
fn hid_code_to_name(hid: u8) -> Option<&'static str> {
    Some(match hid {
        4 => "A",
        5 => "B",
        6 => "C",
        7 => "D",
        8 => "E",
        9 => "F",
        10 => "G",
        11 => "H",
        12 => "I",
        13 => "J",
        14 => "K",
        15 => "L",
        16 => "M",
        17 => "N",
        18 => "O",
        19 => "P",
        20 => "Q",
        21 => "R",
        22 => "S",
        23 => "T",
        24 => "U",
        25 => "V",
        26 => "W",
        27 => "X",
        28 => "Y",
        29 => "Z",
        30 => "1",
        31 => "2",
        32 => "3",
        33 => "4",
        34 => "5",
        35 => "6",
        36 => "7",
        37 => "8",
        38 => "9",
        39 => "0",
        40 => "Enter",
        41 => "Esc",
        42 => "Backspace",
        43 => "Tab",
        44 => "Space",
        45 => "-",
        46 => "=",
        47 => "[",
        48 => "]",
        49 => "\\",
        50 => "#",
        51 => ";",
        52 => "'",
        53 => "`",
        54 => ",",
        55 => ".",
        56 => "/",
        57 => "CapsLock",
        58 => "F1",
        59 => "F2",
        60 => "F3",
        61 => "F4",
        62 => "F5",
        63 => "F6",
        64 => "F7",
        65 => "F8",
        66 => "F9",
        67 => "F10",
        68 => "F11",
        69 => "F12",
        70 => "PrintScreen",
        71 => "ScrollLock",
        72 => "Pause",
        73 => "Insert",
        74 => "Home",
        75 => "PageUp",
        76 => "Delete",
        77 => "End",
        78 => "PageDown",
        79 => "Right",
        80 => "Left",
        81 => "Down",
        82 => "Up",
        100 => "\\|",
        224 => "LCtrl",
        225 => "LShift",
        226 => "LAlt",
        227 => "LWin",
        228 => "RCtrl",
        229 => "RShift",
        230 => "RAlt",
        231 => "RWin",
        _ => return None,
    })
}

/// Convert key name to HID keycode (case-insensitive)
fn name_to_hid_code(name: &str) -> Option<u8> {
    Some(match name.to_uppercase().as_str() {
        "A" => 4,
        "B" => 5,
        "C" => 6,
        "D" => 7,
        "E" => 8,
        "F" => 9,
        "G" => 10,
        "H" => 11,
        "I" => 12,
        "J" => 13,
        "K" => 14,
        "L" => 15,
        "M" => 16,
        "N" => 17,
        "O" => 18,
        "P" => 19,
        "Q" => 20,
        "R" => 21,
        "S" => 22,
        "T" => 23,
        "U" => 24,
        "V" => 25,
        "W" => 26,
        "X" => 27,
        "Y" => 28,
        "Z" => 29,
        "1" => 30,
        "2" => 31,
        "3" => 32,
        "4" => 33,
        "5" => 34,
        "6" => 35,
        "7" => 36,
        "8" => 37,
        "9" => 38,
        "0" => 39,
        "ENTER" | "RETURN" => 40,
        "ESC" | "ESCAPE" => 41,
        "BACKSPACE" | "BKSP" => 42,
        "TAB" => 43,
        "SPACE" => 44,
        "-" | "MINUS" => 45,
        "=" | "EQUALS" => 46,
        "[" | "LBRACKET" => 47,
        "]" | "RBRACKET" => 48,
        "\\" | "BACKSLASH" => 49,
        "#" | "HASH" => 50,
        ";" | "SEMICOLON" => 51,
        "'" | "QUOTE" => 52,
        "`" | "GRAVE" | "BACKTICK" => 53,
        "," | "COMMA" => 54,
        "." | "PERIOD" | "DOT" => 55,
        "/" | "SLASH" => 56,
        "CAPSLOCK" | "CAPS" => 57,
        "F1" => 58,
        "F2" => 59,
        "F3" => 60,
        "F4" => 61,
        "F5" => 62,
        "F6" => 63,
        "F7" => 64,
        "F8" => 65,
        "F9" => 66,
        "F10" => 67,
        "F11" => 68,
        "F12" => 69,
        "PRINTSCREEN" | "PRTSC" => 70,
        "SCROLLLOCK" => 71,
        "PAUSE" => 72,
        "INSERT" | "INS" => 73,
        "HOME" => 74,
        "PAGEUP" | "PGUP" => 75,
        "DELETE" | "DEL" => 76,
        "END" => 77,
        "PAGEDOWN" | "PGDN" => 78,
        "RIGHT" => 79,
        "LEFT" => 80,
        "DOWN" => 81,
        "UP" => 82,
        "LCTRL" | "LEFTCTRL" => 224,
        "LSHIFT" | "LEFTSHIFT" => 225,
        "LALT" | "LEFTALT" => 226,
        "LWIN" | "LEFTWIN" | "LGUI" => 227,
        "RCTRL" | "RIGHTCTRL" => 228,
        "RSHIFT" | "RIGHTSHIFT" => 229,
        "RALT" | "RIGHTALT" => 230,
        "RWIN" | "RIGHTWIN" | "RGUI" => 231,
        _ => return None,
    })
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
}

/// Default paths to search for devices.json
const DEFAULT_DEVICE_DB_PATHS: &[&str] = &[
    "/usr/local/share/akko/devices.json",
    "/usr/share/akko/devices.json",
    "data/devices.json",
    "../data/devices.json", // When running from iot_driver_linux/
];

impl DeviceDatabase {
    /// Create empty database
    pub fn new() -> Self {
        Self {
            devices_by_id: HashMap::new(),
            devices_by_vid_pid: HashMap::new(),
            devices_by_company: HashMap::new(),
            version: 0,
        }
    }

    /// Load from default paths (tries each in order)
    pub fn load_default() -> Result<Self, String> {
        for path in DEFAULT_DEVICE_DB_PATHS {
            let p = Path::new(path);
            if p.exists() {
                match Self::load_from_file(p) {
                    Ok(db) => {
                        info!(
                            "Loaded device database from {} ({} devices, version {})",
                            path,
                            db.len(),
                            db.version
                        );
                        return Ok(db);
                    }
                    Err(e) => {
                        warn!("Failed to load device database from {}: {}", path, e);
                    }
                }
            }
        }
        Err(format!(
            "Device database not found in any of: {:?}",
            DEFAULT_DEVICE_DB_PATHS
        ))
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

        // Test key name lookup
        assert_eq!(dev.key_name(0), Some("Esc"));
        assert_eq!(dev.key_name(9), Some("A"));
        assert_eq!(dev.key_name(15), Some("W"));
        assert_eq!(dev.key_name(16), Some("S"));
        assert_eq!(dev.key_name(100), None); // Out of bounds

        // Test key index lookup
        assert_eq!(dev.key_index("Esc"), Some(0));
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
        // Letters
        assert_eq!(hid_code_to_name(4), Some("A"));
        assert_eq!(hid_code_to_name(26), Some("W"));
        assert_eq!(hid_code_to_name(22), Some("S"));
        assert_eq!(hid_code_to_name(7), Some("D"));

        // Numbers
        assert_eq!(hid_code_to_name(30), Some("1"));
        assert_eq!(hid_code_to_name(39), Some("0"));

        // Modifiers
        assert_eq!(hid_code_to_name(224), Some("LCtrl"));
        assert_eq!(hid_code_to_name(225), Some("LShift"));

        // Reverse
        assert_eq!(name_to_hid_code("A"), Some(4));
        assert_eq!(name_to_hid_code("w"), Some(26)); // Case insensitive
        assert_eq!(name_to_hid_code("ESCAPE"), Some(41));
        assert_eq!(name_to_hid_code("lshift"), Some(225));
    }

    #[test]
    #[ignore] // Run manually with: cargo test test_m1v5_matrix -- --ignored --nocapture
    fn test_m1v5_matrix() {
        let db = DeviceDatabase::load_from_file("../data/devices.json")
            .or_else(|_| DeviceDatabase::load_from_file("data/devices.json"))
            .expect("Could not load devices.json");

        // Find M1 V5 HE with matrix
        let m1v5 = db
            .find_by_vid_pid(0x3151, 0x5030)
            .into_iter()
            .find(|d| d.led_matrix.is_some())
            .expect("M1 V5 with LED matrix not found");

        println!("Testing device: {} ({})", m1v5.display_name, m1v5.name);

        // Verify WASD indices match expected
        let (w, a, s, d) = m1v5.wasd_indices().expect("WASD keys not found in matrix");
        println!("WASD indices: W={}, A={}, S={}, D={}", w, a, s, d);

        assert_eq!(w, 14, "W should be at index 14");
        assert_eq!(a, 9, "A should be at index 9");
        assert_eq!(s, 15, "S should be at index 15");
        assert_eq!(d, 21, "D should be at index 21");
    }
}
