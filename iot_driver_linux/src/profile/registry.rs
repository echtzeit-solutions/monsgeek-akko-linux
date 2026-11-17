// Profile registry
// Central registry for looking up device profiles by VID/PID

use super::builtin::M1V5HeProfile;
use super::json::{JsonProfileWrapper, LoadError};
use super::traits::DeviceProfile;
use crate::device_loader::{DeviceDatabase, JsonDeviceDefinition};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::debug;

/// Registry for device profiles
/// Provides lookup by VID/PID or device ID
pub struct ProfileRegistry {
    /// Profiles indexed by (VID, PID)
    /// Note: Multiple profiles can share the same VID/PID (different companies)
    by_vid_pid: HashMap<(u16, u16), Vec<Arc<dyn DeviceProfile>>>,
    /// Profiles indexed by ID
    by_id: HashMap<u32, Arc<dyn DeviceProfile>>,
    /// Device database loaded from JSON for feature lookup
    device_db: Option<DeviceDatabase>,
}

/// Collect a profile's per-position key names into an owned vector.
fn profile_key_names(p: &dyn DeviceProfile) -> Vec<String> {
    (0..p.matrix_size())
        .map(|i| p.matrix_key_name(i as u8).to_string())
        .collect()
}

impl ProfileRegistry {
    /// Create an empty registry
    pub fn new() -> Self {
        Self {
            by_vid_pid: HashMap::new(),
            by_id: HashMap::new(),
            device_db: None,
        }
    }

    /// Create a registry with builtin profiles pre-loaded
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.load_builtins();
        registry.load_device_database();
        registry
    }

    /// Load all builtin profiles
    pub fn load_builtins(&mut self) {
        // M1 V5 HE — same keyboard, three transports, all share device ID 2949
        // Register wired last so it wins the by_id slot
        self.register(Arc::new(M1V5HeProfile::wireless())); // BT PID 0x503A
        self.register(Arc::new(M1V5HeProfile::dongle())); // 2.4GHz PID 0x5038
        self.register(Arc::new(M1V5HeProfile::wired())); // USB PID 0x5030
    }

    /// Load the device database from default paths
    pub fn load_device_database(&mut self) {
        match DeviceDatabase::load_default() {
            Ok(db) => {
                monsgeek_transport::register_known_usb_ids(keyboard_usb_ids(&db));
                self.device_db = Some(db);
            }
            Err(e) => {
                debug!("Device database not loaded: {}", e);
            }
        }
    }

    /// Get device info from the database by VID/PID
    /// This provides access to device features even for devices without builtin profiles
    /// WARNING: Returns arbitrary first match if multiple devices share the same VID/PID.
    /// Prefer `get_device_info_by_id()` when device ID is available.
    pub fn get_device_info(&self, vid: u16, pid: u16) -> Option<&JsonDeviceDefinition> {
        self.device_db
            .as_ref()
            .and_then(|db| db.find_by_vid_pid(vid, pid).into_iter().next())
    }

    /// Get device info from the database by firmware device ID (from GET_USB_VERSION)
    /// This is the correct lookup — the device ID comes from the keyboard itself.
    /// Returns `None` for the few IDs shared by several products; pass the USB IDs to
    /// [`Self::get_device_info_by_id_and_usb`] to break those ties.
    pub fn get_device_info_by_id(&self, device_id: i32) -> Option<&JsonDeviceDefinition> {
        self.device_db
            .as_ref()
            .and_then(|db| db.find_by_id(device_id))
    }

    /// Get device info by firmware device ID, disambiguated by the USB IDs it was
    /// reached through (which over a 2.4GHz dongle belong to the dongle, not the keyboard).
    pub fn get_device_info_by_id_and_usb(
        &self,
        device_id: i32,
        vid: u16,
        pid: u16,
    ) -> Option<&JsonDeviceDefinition> {
        self.device_db
            .as_ref()
            .and_then(|db| db.find_by_id_and_usb(device_id, vid, pid))
    }

    /// Get device info with company preference
    pub fn get_device_info_for_company(
        &self,
        vid: u16,
        pid: u16,
        company: &str,
    ) -> Option<&JsonDeviceDefinition> {
        self.device_db
            .as_ref()
            .and_then(|db| db.find_by_vid_pid_company(vid, pid, company))
    }

    /// Check if device has magnetism (Hall effect switches) from database
    pub fn device_has_magnetism(&self, vid: u16, pid: u16) -> bool {
        self.get_device_info(vid, pid)
            .map(|d| d.has_magnetism())
            .unwrap_or(false)
    }

    /// Get key count from database
    pub fn device_key_count(&self, vid: u16, pid: u16) -> Option<u8> {
        self.get_device_info(vid, pid).and_then(|d| d.key_count)
    }

    /// Get device matrix from the matrix database
    pub fn get_device_matrix(
        &self,
        vid: u16,
        pid: u16,
        device_id: i32,
    ) -> Option<&crate::device_loader::JsonDeviceMatrix> {
        self.device_db
            .as_ref()
            .and_then(|db| db.get_matrix(vid, pid, device_id))
    }

    /// Check if device database is loaded
    pub fn has_device_database(&self) -> bool {
        self.device_db.is_some()
    }

    /// Get device database stats
    pub fn device_database_stats(&self) -> Option<(usize, u32)> {
        self.device_db.as_ref().map(|db| (db.len(), db.version()))
    }

    /// Register a profile in the registry
    pub fn register(&mut self, profile: Arc<dyn DeviceProfile>) {
        let vid_pid = (profile.vid(), profile.pid());
        let id = profile.id();

        // Add to VID/PID index
        self.by_vid_pid
            .entry(vid_pid)
            .or_default()
            .push(profile.clone());

        // Add to ID index
        self.by_id.insert(id, profile);
    }

    /// Find a builtin profile by USB VID/PID (first match).
    ///
    /// Private on purpose: VID/PID is **not unique** across products (e.g. the Womier
    /// SK75 TMR reuses the MonsGeek `0x3151:0x5030`), so a VID/PID match may return an
    /// unrelated board's profile. It is only ever a last-resort fallback inside
    /// [`Self::resolve_matrix_key_names`], never a standalone lookup.
    fn find_by_vid_pid(&self, vid: u16, pid: u16) -> Option<Arc<dyn DeviceProfile>> {
        self.by_vid_pid
            .get(&(vid, pid))
            .and_then(|profiles| profiles.first().cloned())
    }

    /// Find a builtin profile by firmware device ID (the authoritative identifier).
    /// Private: used only by [`Self::resolve_matrix_key_names`].
    fn find_by_id(&self, id: u32) -> Option<Arc<dyn DeviceProfile>> {
        self.by_id.get(&id).cloned()
    }

    /// Resolve per-matrix-position key names for a connected device, most
    /// authoritative source first:
    ///
    /// 1. a builtin [`DeviceProfile`] matched by firmware **device id** (hand-curated
    ///    layouts such as the M1 V5), with any position it leaves unnamed filled from
    ///    the matrix DB — a curated profile is more trustworthy where it speaks, but
    ///    it should not hide keys it simply never listed,
    /// 2. the id-resolved matrix-DB entry from `device_matrices.json` (covers every
    ///    third-party board), then
    /// 3. a builtin profile matched only by **VID/PID**, as a last resort when the
    ///    device id is unavailable.
    ///
    /// The VID/PID builtin must never precede the matrix DB: several distinct products
    /// share a VID/PID, and matching on USB ids alone would stamp one board with
    /// another's layout (the bug where an SK75 TMR was labelled as an M1 V5). This is
    /// the only supported way to obtain key names — the raw by-id / by-vid-pid lookups
    /// are deliberately private so the wrong ordering can't be rebuilt at a call site.
    pub fn resolve_matrix_key_names(
        &self,
        device_id: Option<i32>,
        vid: u16,
        pid: u16,
    ) -> Option<Vec<String>> {
        let matrix = device_id.and_then(|id| self.get_device_matrix(vid, pid, id));
        let db_names = |i: usize| {
            matrix
                .and_then(|m| m.key_name(i))
                .filter(|n| !n.is_empty())
                .map(str::to_string)
        };

        if let Some(p) = device_id.and_then(|id| self.find_by_id(id as u32)) {
            // A hand-curated profile wins position by position, but the matrix DB
            // fills any gap it leaves: the M1 V5 builtin, for instance, names
            // nothing past the arrow cluster, while the DB knows its volume and
            // HID keys at 90/91/96/97.
            let mut names = profile_key_names(p.as_ref());
            let extra = matrix.map_or(0, |m| m.matrix_size());
            names.resize(names.len().max(extra), String::new());
            for (i, name) in names.iter_mut().enumerate() {
                if name.is_empty()
                    && let Some(db) = db_names(i)
                {
                    *name = db;
                }
            }
            return Some(names);
        }
        if let Some(m) = matrix {
            return Some(
                (0..m.matrix_size())
                    .map(|i| db_names(i).unwrap_or_default())
                    .collect(),
            );
        }
        self.find_by_vid_pid(vid, pid)
            .map(|p| profile_key_names(p.as_ref()))
    }

    /// Check if a VID/PID is registered
    pub fn has_vid_pid(&self, vid: u16, pid: u16) -> bool {
        self.by_vid_pid.contains_key(&(vid, pid))
    }

    /// Get all registered VID/PID pairs
    pub fn all_vid_pids(&self) -> Vec<(u16, u16)> {
        self.by_vid_pid.keys().copied().collect()
    }

    /// Get all registered profiles
    pub fn all_profiles(&self) -> Vec<Arc<dyn DeviceProfile>> {
        self.by_id.values().cloned().collect()
    }

    /// Get the number of registered profiles
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Load a profile from a JSON file
    pub fn load_from_file<P: AsRef<Path>>(&mut self, path: P) -> Result<(), LoadError> {
        let wrapper = JsonProfileWrapper::from_file(path)?;
        self.register(Arc::new(wrapper));
        Ok(())
    }

    /// Load all JSON profiles from a directory
    pub fn load_from_directory<P: AsRef<Path>>(&mut self, dir: P) -> Result<usize, LoadError> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(LoadError::Io(format!(
                "{} is not a directory",
                dir.display()
            )));
        }

        let mut count = 0;
        for entry in std::fs::read_dir(dir).map_err(|e| LoadError::Io(e.to_string()))? {
            let entry = entry.map_err(|e| LoadError::Io(e.to_string()))?;
            let path = entry.path();

            if path.extension().map(|e| e == "json").unwrap_or(false) {
                match self.load_from_file(&path) {
                    Ok(()) => count += 1,
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to load profile from {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }

        Ok(count)
    }
}

/// USB IDs of the database's keyboards, for discovery. The database also lists
/// mice and their receivers, and negative ids are placeholder "help" entries.
fn keyboard_usb_ids(db: &DeviceDatabase) -> impl Iterator<Item = (u16, u16)> + '_ {
    db.all_devices()
        .filter(|d| d.id >= 0 && matches!(d.device_type.as_str(), "keyboard" | "hitBoxKeyboard"))
        .map(|d| (d.vid, d.pid))
}

impl Default for ProfileRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

/// Global profile registry singleton
/// Use `profile_registry()` to access
static REGISTRY: std::sync::OnceLock<ProfileRegistry> = std::sync::OnceLock::new();

/// Get the global profile registry
/// Initializes with builtin profiles on first access
pub fn profile_registry() -> &'static ProfileRegistry {
    REGISTRY.get_or_init(ProfileRegistry::with_builtins)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_ids_are_the_databases_keyboards() {
        let Ok(db) = DeviceDatabase::load_from_file("../data/devices.json") else {
            eprintln!("data/devices.json not found, skipping");
            return;
        };
        let ids: std::collections::HashSet<_> = keyboard_usb_ids(&db).collect();
        assert!(ids.contains(&(0x38EE, 0x0016)), "Akko RAID 75 HE");
        assert!(ids.contains(&(0x3151, 0x5030)), "M1 V5 HE wired");
        assert!(!ids.contains(&(0x374A, 0xA321)), "a mouse");
        assert!(
            !ids.contains(&(0x046A, 0x012E)),
            "a negative-id placeholder"
        );
    }

    #[test]
    fn test_registry_with_builtins() {
        let registry = ProfileRegistry::with_builtins();

        // Wired and wireless share device ID 2949, so by_id has 1 entry
        // but by_vid_pid has 2 entries (0x5030 + 0x503A)
        assert!(!registry.is_empty());

        // Find wired variant
        let profile = registry.find_by_vid_pid(0x3151, 0x5030).unwrap();
        assert_eq!(profile.display_name(), "MonsGeek M1 V5 HE");
        // Physical inputs, not matrix positions: 81 switches + the encoder.
        assert_eq!(profile.key_count(), 82);

        // Find wireless and dongle variants
        let profile = registry.find_by_vid_pid(0x3151, 0x503A).unwrap();
        assert!(profile.display_name().contains("Wireless"));
        let profile = registry.find_by_vid_pid(0x3151, 0x5038).unwrap();
        assert!(profile.display_name().contains("Dongle"));
    }

    #[test]
    fn test_find_by_id() {
        let registry = ProfileRegistry::with_builtins();

        let profile = registry.find_by_id(2949).unwrap();
        assert_eq!(profile.pid(), 0x5030);
    }

    /// The Womier SK75 TMR (device id 3804) shares USB `0x3151:0x5030` with the
    /// MonsGeek M1 V5 HE. Key-name resolution must key off the device id (matrix DB),
    /// not the shared VID/PID (M1 V5 builtin) — otherwise the SK75 gets stamped with
    /// the M1 V5 layout (empty position 84, Home at 85). Needs `data/*.json`.
    #[test]
    fn test_resolve_key_names_prefers_id_over_shared_vid_pid() {
        let registry = ProfileRegistry::with_builtins();

        // M1 V5 resolves via its own builtin (matched by id): position 84 is a gap.
        let m1v5 = registry
            .resolve_matrix_key_names(Some(2949), 0x3151, 0x5030)
            .expect("M1 V5 key names");
        assert_eq!(m1v5[84], "");

        // SK75 shares the VID/PID but must resolve to its own matrix-DB layout.
        let sk75 = registry
            .resolve_matrix_key_names(Some(3804), 0x3151, 0x5030)
            .expect("SK75 key names (needs data/device_matrices.json)");
        assert_eq!(sk75[84], "Home");
        assert_eq!(sk75[85], "End");
    }

    /// The M1 V5 builtin names nothing past the arrow cluster, but the matrix DB
    /// knows its volume keys — a curated profile shouldn't hide them.
    #[test]
    fn builtin_gaps_are_filled_from_the_matrix_db() {
        let registry = ProfileRegistry::with_builtins();
        let names = registry
            .resolve_matrix_key_names(Some(2949), 0x3151, 0x5030)
            .expect("M1 V5 key names");
        // Curated names still win where the builtin has one.
        assert_eq!(names[85], "Home");
        // Gaps fall through to the DB.
        assert_eq!(names[90], "VolUp");
        assert_eq!(names[91], "VolDn");
    }

    #[test]
    fn test_all_vid_pids() {
        let registry = ProfileRegistry::with_builtins();

        let vid_pids = registry.all_vid_pids();
        assert!(vid_pids.contains(&(0x3151, 0x5030)));
        assert!(vid_pids.contains(&(0x3151, 0x503A)));
    }

    #[test]
    fn test_global_registry() {
        let registry = profile_registry();
        assert!(!registry.is_empty());

        // Should return the same instance
        let registry2 = profile_registry();
        assert_eq!(registry.len(), registry2.len());
    }
}
