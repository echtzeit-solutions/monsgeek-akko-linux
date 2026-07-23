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
    /// Maximum polling rate in Hz. Recorded per USB product by the vendor, not per model.
    #[serde(default)]
    pub report_rate: Option<u16>,
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

    /// Polling rates this model accepts, fastest first.
    ///
    /// The vendor caps the offered rates at the model's maximum, so the list is always a
    /// suffix of [`crate::protocol::polling_rate::RATES`]. Empty when the maximum is
    /// unknown. Check [`Self::polling_rate_support`] too — a model can have a known
    /// maximum and still expose no control.
    pub fn polling_rates(&self) -> &'static [u16] {
        use crate::protocol::polling_rate::RATES;
        let Some(max) = self.report_rate else {
            return &[];
        };
        match RATES.iter().position(|&hz| hz == max) {
            Some(i) => &RATES[i..],
            None => &[],
        }
    }

    /// Whether the polling rate can be read and changed on this device.
    ///
    /// Mirrors the vendor app's `isSupportReportRate`: never over Bluetooth, always for
    /// mice, and for keyboards only above 1 kHz and either on an explicit exception list
    /// or running new enough firmware. Devices that fail this have nothing useful to say
    /// to GET_REPORT, which is why the SK75 TMR (v3.00) shows no control in the vendor
    /// app either.
    ///
    /// Returns the requirement rather than a verdict because the firmware version only
    /// becomes known once the device answers GET_USB_VERSION.
    pub fn polling_rate_support(&self, over_bluetooth: bool) -> PollingRateSupport {
        if over_bluetooth {
            return PollingRateSupport::Unsupported;
        }
        if self.device_type == "mouse" {
            return PollingRateSupport::Always;
        }
        // A keyboard with no known maximum, or one capped at 1 kHz, has no control.
        if self.report_rate.is_none_or(|hz| hz <= 1000) {
            return PollingRateSupport::Unsupported;
        }
        let company = self.company.as_deref().unwrap_or("");
        if POLLING_RATE_NO_CONTROL.contains(&(company, self.id)) {
            return PollingRateSupport::Unsupported;
        }
        if company == "XinMengK65Keyboard"
            || (company == "cherry" && self.device_type == "keyboard")
            || POLLING_RATE_NO_VERSION_GATE.contains(&(company, self.id))
        {
            return PollingRateSupport::Always;
        }
        PollingRateSupport::FromFirmware(POLLING_RATE_MIN_FW_VERSION)
    }
}

/// Whether a device exposes a polling rate control, and from which firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PollingRateSupport {
    /// No control at all — the model, or this transport, does not implement it.
    #[default]
    Unsupported,
    Always,
    /// Implemented from this GET_USB_VERSION value onwards.
    FromFirmware(u16),
}

impl PollingRateSupport {
    pub fn is_available(self, fw_version: u16) -> bool {
        match self {
            Self::Unsupported => false,
            Self::Always => true,
            Self::FromFirmware(min) => fw_version >= min,
        }
    }
}

/// Firmware version (as reported by GET_USB_VERSION) from which the polling rate command
/// is implemented. 0x0400 = v4.00.
pub const POLLING_RATE_MIN_FW_VERSION: u16 = 0x0400;

/// Models that report a >1 kHz maximum but expose no polling rate control regardless.
const POLLING_RATE_NO_CONTROL: &[(&str, i32)] = &[("HawkGamingHK610S", 3677)];

/// Models exempt from the firmware version gate — they implement the command on older
/// firmware. Transcribed from the vendor app; extend as new exceptions appear there.
const POLLING_RATE_NO_VERSION_GATE: &[(&str, i32)] = &[
    ("rongyuan", 3195),
    ("rongyuan", 2342),
    ("PIIFOXDRIVER", 2499),
    ("ZENITHPROSoftware", 2310),
    ("XinMengK65Keyboard", 3198),
    ("SARU", 3243),
    ("腹灵", 3085),
    ("EWEADNV", 2652),
    ("EWEADNV", 2574),
    ("EWEADNV", 2578),
    ("EWEADNV", 2653),
    ("EWEADNV", 2710),
    ("EWEADNV", 2711),
    ("AJAZZMOUSE", 2255),
    ("AJAZZMOUSE", 2336),
    ("AJAZZMOUSE", 2343),
    ("AJAZZMOUSE", 2371),
    ("AttackShark", 2472),
    ("AttackShark", 2370),
    ("gamakay2", 2501),
    ("gamakay2", 2334),
    ("GVSTONE", 2727),
    ("GVSTONE", 2801),
    ("蚂蚁电竞", 2651),
    ("蚂蚁电竞", 2629),
    ("蚂蚁电竞", 2425),
    ("蚂蚁电竞", 2642),
    ("蚂蚁电竞", 2281),
    ("蚂蚁电竞", 2516),
    ("蚂蚁电竞", 1846),
];

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
    pub vid: u16,
    pub pid: u16,
    #[serde(default)]
    pub key_layout_name: Option<String>,
    pub key_count: u16,
    pub match_method: String,
    pub matrix: Vec<u8>,
    pub key_names: Vec<Option<String>>,
    /// Matrix positions that are non-analog (GPIO/encoder, not magnetic switches).
    /// These should be excluded from calibration progress display.
    #[serde(default)]
    pub non_analog_positions: Option<Vec<u8>>,
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

    /// Get the firmware matrix size (highest occupied position + 1).
    /// This is the number of positions the firmware uses for calibration/magnetism data.
    pub fn matrix_size(&self) -> usize {
        self.matrix
            .iter()
            .rposition(|&v| v != 0)
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    /// Check if a matrix position is non-analog (GPIO/encoder, not a magnetic switch).
    pub fn is_non_analog(&self, position: u8) -> bool {
        self.non_analog_positions
            .as_ref()
            .map(|p| p.contains(&position))
            .unwrap_or(false)
    }

    /// Get the number of analog (magnetic) key positions.
    /// Excludes empty positions and non-analog positions.
    pub fn analog_key_count(&self) -> usize {
        let total_keys = self.matrix.iter().filter(|&&h| h != 0).count();
        let non_analog = self
            .non_analog_positions
            .as_ref()
            .map(|p| p.len())
            .unwrap_or(0);
        total_keys.saturating_sub(non_analog)
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
    /// Every device definition; the indexes below hold positions into this list.
    devices: Vec<JsonDeviceDefinition>,
    /// Device ID -> devices claiming it (usually one; a few IDs are reused across products)
    devices_by_id: HashMap<i32, Vec<usize>>,
    /// (VID, PID) -> devices behind that USB product
    devices_by_vid_pid: HashMap<(u16, u16), Vec<usize>>,
    /// Company -> its devices
    devices_by_company: HashMap<String, Vec<usize>>,
    /// Version of the loaded database
    version: u32,
    /// Device matrices (loaded from device_matrices.json), keyed by device ID.
    /// A handful of IDs are claimed by more than one USB product, hence the Vec.
    matrices: HashMap<i32, Vec<JsonDeviceMatrix>>,
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
            devices: Vec::new(),
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
        for (key, matrix) in file.devices {
            // Keys are "vid:pid:id"; the device ID is what the keyboard reports.
            let Some(id) = key.rsplit(':').next().and_then(|s| s.parse::<i32>().ok()) else {
                warn!("Invalid device matrix key: {key}");
                continue;
            };
            self.matrices.entry(id).or_default().push(matrix);
            count += 1;
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
        let slot = self.devices.len();
        self.devices.push(device);

        // Index by device ID. A few IDs are claimed by more than one USB product
        // (e.g. 790 = yc500_5108bplus_uk_soc @3151:4015 and yc580_yz21 @3151:4010),
        // so every claimant is kept and `find_by_id_and_usb` picks between them.
        self.devices_by_id.entry(id).or_default().push(slot);
        self.devices_by_vid_pid
            .entry(vid_pid)
            .or_default()
            .push(slot);

        // Index by company (skip empty company names)
        if !company.is_empty() {
            self.devices_by_company
                .entry(company)
                .or_default()
                .push(slot);
        }
    }

    /// Find device by ID, when that ID identifies exactly one product.
    ///
    /// Returns `None` for an ID several products claim — use [`Self::find_by_id_and_usb`]
    /// there, which can break the tie.
    pub fn find_by_id(&self, id: i32) -> Option<&JsonDeviceDefinition> {
        match self.devices_by_id.get(&id)?.as_slice() {
            [only] => self.devices.get(*only),
            _ => None,
        }
    }

    /// Find device by the ID it reports, disambiguated by the USB IDs we reached it through.
    ///
    /// The device ID comes from the keyboard itself, so it leads; `vid`/`pid` identify the
    /// USB endpoint, which over a 2.4GHz dongle is the dongle rather than the keyboard.
    /// They are therefore only consulted when several products claim the same ID.
    pub fn find_by_id_and_usb(&self, id: i32, vid: u16, pid: u16) -> Option<&JsonDeviceDefinition> {
        let slots = self.devices_by_id.get(&id)?;
        match slots.as_slice() {
            [only] => self.devices.get(*only),
            claimants => claimants
                .iter()
                .filter_map(|&s| self.devices.get(s))
                .find(|d| d.vid == vid && d.pid == pid),
        }
    }

    /// Find all devices with matching VID/PID
    pub fn find_by_vid_pid(&self, vid: u16, pid: u16) -> Vec<&JsonDeviceDefinition> {
        self.devices_by_vid_pid
            .get(&(vid, pid))
            .map(|slots| slots.iter().filter_map(|&s| self.devices.get(s)).collect())
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
            .map(|slots| slots.iter().filter_map(|&s| self.devices.get(s)).collect())
            .unwrap_or_default()
    }

    /// Get all unique companies
    pub fn get_companies(&self) -> Vec<&str> {
        self.devices_by_company.keys().map(|s| s.as_str()).collect()
    }

    /// Get all devices
    pub fn all_devices(&self) -> impl Iterator<Item = &JsonDeviceDefinition> {
        self.devices.iter()
    }

    /// Get device count
    pub fn len(&self) -> usize {
        self.devices.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
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

    /// Get the key matrix for a keyboard reporting `device_id` from GET_USB_VERSION.
    ///
    /// The device ID always comes from the keyboard itself, so it is the primary key —
    /// `vid`/`pid` identify whichever USB endpoint we are talking through, which over a
    /// 2.4GHz dongle is the dongle rather than the keyboard. USB IDs therefore only break
    /// ties for the handful of IDs that more than one product claims; when they cannot
    /// (an ambiguous ID reached over a dongle) we return nothing rather than guess a
    /// layout that may belong to a different keyboard.
    ///
    /// Mirrors the precedence in [`crate::devices::get_device_info_with_id`], so the
    /// device database and the matrix database always agree on what is connected.
    pub fn get_matrix(&self, vid: u16, pid: u16, device_id: i32) -> Option<&JsonDeviceMatrix> {
        match self.matrices.get(&device_id)?.as_slice() {
            [only] => Some(only),
            claimants => claimants.iter().find(|m| m.vid == vid && m.pid == pid),
        }
    }

    /// Get key name for a device's matrix position
    pub fn device_key_name(
        &self,
        vid: u16,
        pid: u16,
        device_id: i32,
        position: usize,
    ) -> Option<&str> {
        self.get_matrix(vid, pid, device_id)
            .and_then(|m| m.key_name(position))
    }

    /// Get matrix position for a key name on a device
    pub fn device_key_index(
        &self,
        vid: u16,
        pid: u16,
        device_id: i32,
        name: &str,
    ) -> Option<usize> {
        self.get_matrix(vid, pid, device_id)
            .and_then(|m| m.key_index(name))
    }

    /// Get HID code for a device's matrix position
    pub fn device_hid_code(
        &self,
        vid: u16,
        pid: u16,
        device_id: i32,
        position: usize,
    ) -> Option<u8> {
        self.get_matrix(vid, pid, device_id)
            .and_then(|m| m.hid_code(position))
    }

    /// Get number of loaded matrices
    pub fn matrices_len(&self) -> usize {
        self.matrices.values().map(Vec::len).sum()
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

    const M1_V5_TMR_VID: u16 = 0x3151;
    const M1_V5_TMR_PID: u16 = 0x5030;

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

    // Two devices sharing ID 790 across different PIDs, plus an ID unique to one product.
    const TEST_MATRICES_JSON: &str = r#"{
        "version": 3,
        "devices": {
            "12625:16405:790": {
                "name": "yc500_5108bplus_uk_soc", "displayName": "5108B+",
                "vid": 12625, "pid": 16405, "keyCount": 3, "matchMethod": "exactName",
                "matrix": [41, 43, 57], "keyNames": ["Esc", "Tab", "CapsLock"]
            },
            "12625:16400:790": {
                "name": "yc580_yz21", "displayName": "YZ21",
                "vid": 12625, "pid": 16400, "keyCount": 2, "matchMethod": "exactName",
                "matrix": [4, 5], "keyNames": ["A", "B"]
            },
            "12625:20528:3804": {
                "name": "ry5088_womier_sk75he_europe_3m_8k_8k", "displayName": "SK75 TMR",
                "vid": 12625, "pid": 20528, "keyCount": 2, "matchMethod": "exactName",
                "matrix": [74, 77], "keyNames": ["Home", "End"]
            }
        }
    }"#;

    fn keyboard_with(company: &str, id: i32, report_rate: Option<u16>) -> JsonDeviceDefinition {
        let rate = report_rate
            .map(|r| format!(r#","reportRate":{r}"#))
            .unwrap_or_default();
        let json = format!(
            r#"[{{"id": {id}, "vid": 12625, "pid": 20528, "name": "kb", "displayName": "KB",
                  "type": "keyboard", "company": "{company}"{rate}}}]"#
        );
        DeviceDatabase::load_from_json(&json)
            .unwrap()
            .all_devices()
            .next()
            .unwrap()
            .clone()
    }

    #[test]
    fn polling_rates_are_capped_at_the_model_maximum() {
        assert_eq!(
            keyboard_with("MonsGeek", 1, Some(8000)).polling_rates(),
            &[8000, 4000, 2000, 1000, 500, 250, 125]
        );
        // A 1kHz board must never be offered 2kHz and up.
        assert_eq!(
            keyboard_with("MonsGeek", 1, Some(1000)).polling_rates(),
            &[1000, 500, 250, 125]
        );
        // Unknown maximum, or one that is not a real rate: offer nothing rather than guess.
        assert!(keyboard_with("MonsGeek", 1, None)
            .polling_rates()
            .is_empty());
        assert!(keyboard_with("MonsGeek", 1, Some(3000))
            .polling_rates()
            .is_empty());
    }

    #[test]
    fn polling_rate_control_is_gated_like_the_vendor_app() {
        use PollingRateSupport::*;

        // The SK75 TMR case from issue #20: 8kHz-capable USB product, but v3.00 firmware
        // predates the command, so the vendor app shows no control either.
        let sk75 = keyboard_with("WOMIER", 3804, Some(8000));
        assert_eq!(sk75.polling_rate_support(false), FromFirmware(0x0400));
        assert!(!sk75.polling_rate_support(false).is_available(0x0300));
        assert!(sk75.polling_rate_support(false).is_available(0x0407));

        // Never over Bluetooth, whatever the firmware.
        assert_eq!(sk75.polling_rate_support(true), Unsupported);

        // Capped at 1kHz, or no known maximum: no control at all.
        assert_eq!(
            keyboard_with("MonsGeek", 1, Some(1000)).polling_rate_support(false),
            Unsupported
        );
        assert_eq!(
            keyboard_with("MonsGeek", 1, None).polling_rate_support(false),
            Unsupported
        );

        // Exceptions bypass the version gate; one model is excluded outright.
        assert_eq!(
            keyboard_with("AttackShark", 2472, Some(8000)).polling_rate_support(false),
            Always
        );
        assert_eq!(
            keyboard_with("cherry", 1234, Some(8000)).polling_rate_support(false),
            Always
        );
        assert_eq!(
            keyboard_with("HawkGamingHK610S", 3677, Some(8000)).polling_rate_support(false),
            Unsupported
        );
    }

    #[test]
    fn device_lookup_keeps_every_claimant_of_a_shared_id() {
        // Same shape as the real collision: one ID, two unrelated keyboards.
        let db = DeviceDatabase::load_from_json(
            r#"[
                {"id": 790, "vid": 12625, "pid": 16405, "name": "yc500_5108bplus_uk_soc",
                 "displayName": "5108B Plus", "type": "keyboard", "company": "akko"},
                {"id": 790, "vid": 12625, "pid": 16400, "name": "yc580_yz21",
                 "displayName": "YZ-21", "type": "keyboard", "company": "YUNZII"},
                {"id": 3804, "vid": 12625, "pid": 20528, "name": "ry5088_womier_sk75he_europe_3m_8k_8k",
                 "displayName": "SK75 TMR", "type": "keyboard", "company": "WOMIER"}
            ]"#,
        )
        .unwrap();

        // Neither claimant is dropped at load time.
        assert_eq!(db.len(), 3);
        assert_eq!(
            db.find_by_vid_pid(12625, 16405)[0].name,
            "yc500_5108bplus_uk_soc"
        );
        assert_eq!(db.find_by_vid_pid(12625, 16400)[0].name, "yc580_yz21");

        // An unambiguous ID resolves without USB IDs (i.e. also over a dongle).
        assert_eq!(db.find_by_id(3804).unwrap().display_name, "SK75 TMR");
        // A shared one needs the tie-break and refuses to guess without it.
        assert!(db.find_by_id(790).is_none());
        assert_eq!(
            db.find_by_id_and_usb(790, 12625, 16400)
                .unwrap()
                .display_name,
            "YZ-21"
        );
        assert!(db.find_by_id_and_usb(790, 0x3151, 0x5038).is_none());
    }

    #[test]
    fn matrix_lookup_disambiguates_shared_device_ids() {
        let mut db = DeviceDatabase::new();
        assert_eq!(db.load_matrices_from_json(TEST_MATRICES_JSON).unwrap(), 3);

        // An unambiguous device ID resolves through ANY transport — over a 2.4GHz dongle
        // the vid/pid we see belong to the dongle, not to the keyboard reporting the ID.
        const DONGLE: (u16, u16) = (0x3151, 0x5038);
        assert_eq!(
            db.get_matrix(DONGLE.0, DONGLE.1, 3804)
                .unwrap()
                .display_name,
            "SK75 TMR"
        );

        // Where two products claim one ID, the USB IDs are the only tie-breaker.
        assert_eq!(
            db.get_matrix(12625, 16405, 790).unwrap().name,
            "yc500_5108bplus_uk_soc"
        );
        assert_eq!(db.get_matrix(12625, 16400, 790).unwrap().name, "yc580_yz21");

        // Ambiguous ID with no matching product (e.g. reached over a dongle): report
        // nothing rather than a layout that may belong to the other keyboard.
        assert!(db.get_matrix(DONGLE.0, DONGLE.1, 790).is_none());

        assert_eq!(db.device_key_name(12625, 20528, 3804, 1), Some("End"));
        assert_eq!(db.device_hid_code(12625, 20528, 3804, 0), Some(74));
        assert_eq!(db.device_key_index(12625, 16400, 790, "b"), Some(1));
    }

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
            .get_matrix(m1v5.vid, m1v5.pid, m1v5.id)
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
        let matrix = db
            .get_matrix(M1_V5_TMR_VID, M1_V5_TMR_PID, 2247)
            .expect("M1 V5 TMR matrix not found");
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
        assert_eq!(
            db.device_key_name(M1_V5_TMR_VID, M1_V5_TMR_PID, 2247, 28),
            Some("C")
        );
        assert_eq!(
            db.device_key_name(M1_V5_TMR_VID, M1_V5_TMR_PID, 2247, 0),
            Some("Esc")
        );
        assert_eq!(
            db.device_hid_code(M1_V5_TMR_VID, M1_V5_TMR_PID, 2247, 28),
            Some(6)
        );

        // Test key index lookup (uses key_names from JSON, not HID canonical names)
        assert_eq!(matrix.key_index("C"), Some(28));
        assert_eq!(matrix.key_index("Esc"), Some(0));
        assert_eq!(
            db.device_key_index(M1_V5_TMR_VID, M1_V5_TMR_PID, 2247, "C"),
            Some(28)
        );

        println!(
            "M1 V5 TMR: position 28 = HID {} = {}",
            matrix.matrix[28],
            matrix.key_name(28).unwrap_or("?")
        );
    }
}
