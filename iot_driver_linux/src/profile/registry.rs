// Profile registry
// Central registry for looking up device profiles by VID/PID

use super::builtin::{M1V5HeProfile, M1V5HeWirelessProfile};
use super::json::{JsonProfileWrapper, LoadError};
use super::traits::DeviceProfile;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Registry for device profiles
/// Provides lookup by VID/PID or device ID
pub struct ProfileRegistry {
    /// Profiles indexed by (VID, PID)
    /// Note: Multiple profiles can share the same VID/PID (different companies)
    by_vid_pid: HashMap<(u16, u16), Vec<Arc<dyn DeviceProfile>>>,
    /// Profiles indexed by ID
    by_id: HashMap<u32, Arc<dyn DeviceProfile>>,
}

impl ProfileRegistry {
    /// Create an empty registry
    pub fn new() -> Self {
        Self {
            by_vid_pid: HashMap::new(),
            by_id: HashMap::new(),
        }
    }

    /// Create a registry with builtin profiles pre-loaded
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.load_builtins();
        registry
    }

    /// Load all builtin profiles
    pub fn load_builtins(&mut self) {
        // M1 V5 HE Wired
        self.register(Arc::new(M1V5HeProfile::new()));

        // M1 V5 HE Wireless
        self.register(Arc::new(M1V5HeWirelessProfile::new()));
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

    /// Find profile by VID/PID
    /// Returns the first matching profile (use find_by_vid_pid_company for disambiguation)
    pub fn find_by_vid_pid(&self, vid: u16, pid: u16) -> Option<Arc<dyn DeviceProfile>> {
        self.by_vid_pid
            .get(&(vid, pid))
            .and_then(|profiles| profiles.first().cloned())
    }

    /// Find all profiles matching a VID/PID
    pub fn find_all_by_vid_pid(&self, vid: u16, pid: u16) -> Vec<Arc<dyn DeviceProfile>> {
        self.by_vid_pid
            .get(&(vid, pid))
            .cloned()
            .unwrap_or_default()
    }

    /// Find profile by VID/PID with company preference
    pub fn find_by_vid_pid_company(
        &self,
        vid: u16,
        pid: u16,
        preferred_company: &str,
    ) -> Option<Arc<dyn DeviceProfile>> {
        let profiles = self.by_vid_pid.get(&(vid, pid))?;

        // Try to find matching company first
        if let Some(profile) = profiles
            .iter()
            .find(|p| p.company().eq_ignore_ascii_case(preferred_company))
        {
            return Some(profile.clone());
        }

        // Fall back to first match
        profiles.first().cloned()
    }

    /// Find profile by device ID
    pub fn find_by_id(&self, id: u32) -> Option<Arc<dyn DeviceProfile>> {
        self.by_id.get(&id).cloned()
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
    fn test_registry_with_builtins() {
        let registry = ProfileRegistry::with_builtins();

        // Should have at least the M1 V5 HE profiles
        assert!(registry.len() >= 2);

        // Find wired variant
        let profile = registry.find_by_vid_pid(0x3151, 0x5030).unwrap();
        assert_eq!(profile.display_name(), "MonsGeek M1 V5 HE");
        assert_eq!(profile.key_count(), 98);

        // Find wireless variant
        let profile = registry.find_by_vid_pid(0x3151, 0x503A).unwrap();
        assert!(profile.display_name().contains("Wireless"));
    }

    #[test]
    fn test_find_by_id() {
        let registry = ProfileRegistry::with_builtins();

        let profile = registry.find_by_id(2679).unwrap();
        assert_eq!(profile.pid(), 0x5030);
    }

    #[test]
    fn test_all_vid_pids() {
        let registry = ProfileRegistry::with_builtins();

        let vid_pids = registry.all_vid_pids();
        assert!(vid_pids.contains(&(0x3151, 0x5030)));
        assert!(vid_pids.contains(&(0x3151, 0x503A)));
    }

    #[test]
    fn test_company_preference() {
        let registry = ProfileRegistry::with_builtins();

        // Should find MonsGeek profile
        let profile = registry
            .find_by_vid_pid_company(0x3151, 0x5030, "MonsGeek")
            .unwrap();
        assert_eq!(profile.company(), "MonsGeek");
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
