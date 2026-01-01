// Device Loader - Load device definitions from JSON files
// Supports loading from embedded JSON or external file

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Device definition loaded from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonDeviceDefinition {
    pub id: u32,
    pub vid: u16,
    pub pid: u16,
    #[serde(default)]
    pub vid_hex: String,
    #[serde(default)]
    pub pid_hex: String,
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub company: String,
    #[serde(rename = "type", default = "default_type")]
    pub device_type: String,
}

fn default_type() -> String {
    "keyboard".to_string()
}

/// Device database loaded from JSON
#[derive(Debug)]
pub struct DeviceDatabase {
    /// All devices indexed by ID
    devices_by_id: HashMap<u32, JsonDeviceDefinition>,
    /// Devices indexed by (VID, PID) -> list of matching devices
    devices_by_vid_pid: HashMap<(u16, u16), Vec<u32>>,
    /// Devices indexed by company
    devices_by_company: HashMap<String, Vec<u32>>,
}

impl DeviceDatabase {
    /// Create empty database
    pub fn new() -> Self {
        Self {
            devices_by_id: HashMap::new(),
            devices_by_vid_pid: HashMap::new(),
            devices_by_company: HashMap::new(),
        }
    }

    /// Load devices from JSON file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Failed to read file: {}", e))?;
        Self::load_from_json(&content)
    }

    /// Load devices from JSON string
    pub fn load_from_json(json: &str) -> Result<Self, String> {
        let devices: Vec<JsonDeviceDefinition> = serde_json::from_str(json)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;

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
        let company = device.company.clone();

        // Index by VID/PID
        self.devices_by_vid_pid
            .entry(vid_pid)
            .or_insert_with(Vec::new)
            .push(id);

        // Index by company
        self.devices_by_company
            .entry(company)
            .or_insert_with(Vec::new)
            .push(id);

        // Store device
        self.devices_by_id.insert(id, device);
    }

    /// Find device by ID
    pub fn find_by_id(&self, id: u32) -> Option<&JsonDeviceDefinition> {
        self.devices_by_id.get(&id)
    }

    /// Find all devices with matching VID/PID
    pub fn find_by_vid_pid(&self, vid: u16, pid: u16) -> Vec<&JsonDeviceDefinition> {
        self.devices_by_vid_pid
            .get(&(vid, pid))
            .map(|ids| ids.iter().filter_map(|id| self.devices_by_id.get(id)).collect())
            .unwrap_or_default()
    }

    /// Find first device with matching VID/PID (prioritize by company)
    pub fn find_by_vid_pid_company(&self, vid: u16, pid: u16, preferred_company: &str) -> Option<&JsonDeviceDefinition> {
        let matches = self.find_by_vid_pid(vid, pid);

        // Try to find matching company first
        if let Some(dev) = matches.iter().find(|d| d.company.eq_ignore_ascii_case(preferred_company)) {
            return Some(dev);
        }

        // Fall back to first match
        matches.into_iter().next()
    }

    /// Get all devices for a company
    pub fn find_by_company(&self, company: &str) -> Vec<&JsonDeviceDefinition> {
        self.devices_by_company
            .get(company)
            .map(|ids| ids.iter().filter_map(|id| self.devices_by_id.get(id)).collect())
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
}

impl Default for DeviceDatabase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_JSON: &str = r#"[
        {"id": 2949, "vid": 12625, "pid": 20528, "name": "m1v5he", "displayName": "M1 V5 TMR", "company": "MonsGeek"},
        {"id": 2585, "vid": 12625, "pid": 20528, "name": "m3v5", "displayName": "M3 V5", "company": "MonsGeek"},
        {"id": 100, "vid": 1234, "pid": 5678, "name": "akko_k1", "displayName": "K1", "company": "akko"}
    ]"#;

    #[test]
    fn test_load_json() {
        let db = DeviceDatabase::load_from_json(TEST_JSON).unwrap();
        assert_eq!(db.len(), 3);
    }

    #[test]
    fn test_find_by_id() {
        let db = DeviceDatabase::load_from_json(TEST_JSON).unwrap();
        let dev = db.find_by_id(2949).unwrap();
        assert_eq!(dev.display_name, "M1 V5 TMR");
        assert_eq!(dev.company, "MonsGeek");
    }

    #[test]
    fn test_find_by_vid_pid() {
        let db = DeviceDatabase::load_from_json(TEST_JSON).unwrap();
        let matches = db.find_by_vid_pid(12625, 20528);
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_find_by_company() {
        let db = DeviceDatabase::load_from_json(TEST_JSON).unwrap();
        let monsgeek = db.find_by_company("MonsGeek");
        assert_eq!(monsgeek.len(), 2);
    }
}
