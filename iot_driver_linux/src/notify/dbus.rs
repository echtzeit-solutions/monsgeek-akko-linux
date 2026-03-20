//! D-Bus interface for the notification daemon.
//!
//! Bus name: `org.monsgeek.Notify1`
//! Object path: `/org/monsgeek/Notify1`

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use zbus::interface;

use super::keymap;
use super::state::{Notification, NotificationStore};
use crate::effect::{self, EffectLibrary};

/// Shared state between D-Bus interface and render loop.
pub type SharedStore = Arc<Mutex<NotificationStore>>;

/// D-Bus interface implementation.
pub struct NotifyInterface {
    store: SharedStore,
    effects: Arc<EffectLibrary>,
}

impl NotifyInterface {
    pub fn new(store: SharedStore, effects: Arc<EffectLibrary>) -> Self {
        Self { store, effects }
    }
}

#[interface(name = "org.monsgeek.Notify1")]
impl NotifyInterface {
    /// Post a notification. Returns notification ID.
    ///
    /// `vars` maps variable names to color values (e.g. {"color": "red"}).
    async fn notify(
        &self,
        source: &str,
        key: &str,
        effect_name: &str,
        priority: i32,
        ttl_ms: i32,
        vars: BTreeMap<String, String>,
    ) -> zbus::fdo::Result<u64> {
        let target = keymap::parse_key_target(key).map_err(zbus::fdo::Error::InvalidArgs)?;

        let def = self.effects.get(effect_name).ok_or_else(|| {
            zbus::fdo::Error::InvalidArgs(format!("unknown effect: {effect_name}"))
        })?;

        // Extract stagger before passing vars to effect resolver
        let mut effect_vars = vars;
        let stagger_ms: f64 = effect_vars
            .remove("stagger")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);

        let resolved = effect::resolve(def, &effect_vars).map_err(|e| {
            let required = effect::required_variables(def);
            zbus::fdo::Error::InvalidArgs(format!(
                "{e} (required variables: {})",
                required.join(", ")
            ))
        })?;

        // Compute per-key stagger offsets
        let stagger_offsets: HashMap<usize, f64> = if stagger_ms > 0.0 {
            target
                .indices
                .iter()
                .zip(&target.slots)
                .map(|(&idx, &slot)| (idx, slot as f64 * stagger_ms))
                .collect()
        } else {
            HashMap::new()
        };

        // TTL: -1 = use effect default, 0 = no expiry, >0 = explicit ms
        let mut ttl = if ttl_ms > 0 {
            Some(Duration::from_millis(ttl_ms as u64))
        } else if ttl_ms == -1 {
            def.ttl_ms
                .filter(|&ms| ms > 0)
                .map(|ms| Duration::from_millis(ms as u64))
        } else {
            None
        };

        // Extend TTL to account for stagger: last key needs its full animation
        if stagger_ms > 0.0 {
            if let Some(max_slot) = target.slots.iter().max() {
                let extension_ms = *max_slot as f64 * stagger_ms;
                if extension_ms > 0.0 {
                    let ext = Duration::from_secs_f64(extension_ms / 1000.0);
                    ttl = ttl.map(|t| t + ext);
                }
            }
        }

        let notif = Notification {
            id: 0,
            source: source.to_string(),
            effect_name: effect_name.to_string(),
            matrix_indices: target.indices,
            resolved,
            priority,
            ttl,
            created: Instant::now(),
            stagger_offsets,
        };

        let mut store = self.store.lock().await;
        let id = store.add(notif);
        Ok(id)
    }

    /// Acknowledge (dismiss) a notification by ID.
    async fn acknowledge(&self, id: u64) -> zbus::fdo::Result<()> {
        let mut store = self.store.lock().await;
        store.remove(id);
        Ok(())
    }

    /// Acknowledge all notifications on a key.
    async fn acknowledge_key(&self, key: &str) -> zbus::fdo::Result<()> {
        let target = keymap::parse_key_target(key).map_err(zbus::fdo::Error::InvalidArgs)?;
        let mut store = self.store.lock().await;
        store.remove_by_key(&target.indices);
        Ok(())
    }

    /// Acknowledge all notifications from a source.
    async fn acknowledge_source(&self, source: &str) -> zbus::fdo::Result<()> {
        let mut store = self.store.lock().await;
        store.remove_by_source(source);
        Ok(())
    }

    /// List active notifications: Vec<(id, key, source, effect, priority)>.
    async fn list(&self) -> Vec<(u64, String, String, String, i32)> {
        let store = self.store.lock().await;
        store.list()
    }

    /// Clear all notifications.
    async fn clear(&self) -> zbus::fdo::Result<()> {
        let mut store = self.store.lock().await;
        store.clear();
        Ok(())
    }
}
