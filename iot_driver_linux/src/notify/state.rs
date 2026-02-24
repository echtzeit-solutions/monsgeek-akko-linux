//! Notification state management — per-key priority stacks.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::keymap::MATRIX_LEN;
use crate::effect::{ResolvedEffect, Rgb};

/// A notification posted to the daemon.
#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u64,
    pub source: String,
    pub effect_name: String,
    pub matrix_indices: Vec<usize>,
    pub resolved: ResolvedEffect,
    pub priority: i32,
    pub ttl: Option<Duration>,
    pub created: Instant,
}

impl Notification {
    /// Check if this notification has expired.
    pub fn is_expired(&self) -> bool {
        match self.ttl {
            Some(ttl) => self.created.elapsed() >= ttl,
            None => false,
        }
    }

    /// Evaluate the effect at the current time.
    pub fn evaluate(&self) -> Rgb {
        let elapsed_ms = self.created.elapsed().as_secs_f64() * 1000.0;
        self.resolved.evaluate(elapsed_ms)
    }
}

/// Per-key priority stack: maps priority -> notification ID.
/// Higher priority wins (BTreeMap last entry).
type PriorityStack = BTreeMap<i32, u64>;

/// Central notification store.
pub struct NotificationStore {
    /// All notifications by ID.
    notifications: BTreeMap<u64, Notification>,
    /// Per-key priority stacks (indexed by matrix position 0-95).
    key_stacks: Vec<PriorityStack>,
    /// Next notification ID.
    next_id: u64,
}

impl Default for NotificationStore {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationStore {
    pub fn new() -> Self {
        Self {
            notifications: BTreeMap::new(),
            key_stacks: vec![BTreeMap::new(); MATRIX_LEN],
            next_id: 1,
        }
    }

    /// Add a notification. Returns its assigned ID.
    pub fn add(&mut self, mut notif: Notification) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        notif.id = id;

        for &idx in &notif.matrix_indices {
            if idx < MATRIX_LEN {
                self.key_stacks[idx].insert(notif.priority, id);
            }
        }

        self.notifications.insert(id, notif);
        id
    }

    /// Remove a notification by ID.
    pub fn remove(&mut self, id: u64) -> Option<Notification> {
        if let Some(notif) = self.notifications.remove(&id) {
            for &idx in &notif.matrix_indices {
                if idx < MATRIX_LEN {
                    if let Some(&stack_id) = self.key_stacks[idx].get(&notif.priority) {
                        if stack_id == id {
                            self.key_stacks[idx].remove(&notif.priority);
                        }
                    }
                }
            }
            Some(notif)
        } else {
            None
        }
    }

    /// Remove all notifications for given key indices.
    pub fn remove_by_key(&mut self, matrix_indices: &[usize]) -> Vec<u64> {
        let mut removed_ids = Vec::new();
        for &idx in matrix_indices {
            if idx >= MATRIX_LEN {
                continue;
            }
            let ids: Vec<u64> = self.key_stacks[idx].values().copied().collect();
            for id in ids {
                if self.remove(id).is_some() {
                    removed_ids.push(id);
                }
            }
        }
        removed_ids
    }

    /// Remove all notifications from a given source.
    pub fn remove_by_source(&mut self, source: &str) -> Vec<u64> {
        let ids: Vec<u64> = self
            .notifications
            .iter()
            .filter(|(_, n)| n.source == source)
            .map(|(&id, _)| id)
            .collect();
        let mut removed = Vec::new();
        for id in ids {
            if self.remove(id).is_some() {
                removed.push(id);
            }
        }
        removed
    }

    /// Clear all notifications.
    pub fn clear(&mut self) {
        self.notifications.clear();
        for stack in &mut self.key_stacks {
            stack.clear();
        }
    }

    /// Expire notifications that have exceeded their TTL.
    pub fn expire(&mut self) -> Vec<u64> {
        let expired: Vec<u64> = self
            .notifications
            .iter()
            .filter(|(_, n)| n.is_expired())
            .map(|(&id, _)| id)
            .collect();
        let mut removed = Vec::new();
        for id in expired {
            if self.remove(id).is_some() {
                removed.push(id);
            }
        }
        removed
    }

    /// Get the active (highest-priority) notification for a matrix index.
    pub fn active_for_key(&self, matrix_idx: usize) -> Option<&Notification> {
        if matrix_idx >= MATRIX_LEN {
            return None;
        }
        self.key_stacks[matrix_idx]
            .values()
            .next_back()
            .and_then(|id| self.notifications.get(id))
    }

    /// List all active notifications as (id, key_str, source, effect_name, priority).
    pub fn list(&self) -> Vec<(u64, String, String, String, i32)> {
        self.notifications
            .values()
            .map(|n| {
                let key_str = n
                    .matrix_indices
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                (
                    n.id,
                    key_str,
                    n.source.clone(),
                    n.effect_name.clone(),
                    n.priority,
                )
            })
            .collect()
    }

    /// Number of active notifications.
    pub fn len(&self) -> usize {
        self.notifications.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notifications.is_empty()
    }
}

/// Render one frame: evaluate each key's active notification at the current time.
pub fn render_frame(store: &NotificationStore) -> [(u8, u8, u8); MATRIX_LEN] {
    let mut frame = [(0u8, 0u8, 0u8); MATRIX_LEN];

    for (idx, pixel) in frame.iter_mut().enumerate() {
        if let Some(notif) = store.active_for_key(idx) {
            let rgb = notif.evaluate();
            *pixel = (rgb.r, rgb.g, rgb.b);
        }
    }

    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect;

    fn make_notif(indices: Vec<usize>, priority: i32, source: &str) -> Notification {
        let mut vars = BTreeMap::new();
        vars.insert("color".to_string(), "red".to_string());
        let lib = effect::EffectLibrary::from_toml(effect::DEFAULT_EFFECTS_TOML).unwrap();
        let resolved = effect::resolve(lib.get("solid").unwrap(), &vars).unwrap();
        Notification {
            id: 0,
            source: source.to_string(),
            effect_name: "solid".to_string(),
            matrix_indices: indices,
            resolved,
            priority,
            ttl: None,
            created: Instant::now(),
        }
    }

    #[test]
    fn test_add_and_active() {
        let mut store = NotificationStore::new();
        let n = make_notif(vec![1], 0, "test");
        let id = store.add(n);
        assert_eq!(id, 1);
        assert!(store.active_for_key(1).is_some());
        assert_eq!(store.active_for_key(1).unwrap().id, 1);
    }

    #[test]
    fn test_priority_ordering() {
        let mut store = NotificationStore::new();
        let low = make_notif(vec![5], -10, "tmux");
        let high = make_notif(vec![5], 10, "email");
        let _low_id = store.add(low);
        let high_id = store.add(high);
        assert_eq!(store.active_for_key(5).unwrap().id, high_id);
    }

    #[test]
    fn test_remove_reveals_lower() {
        let mut store = NotificationStore::new();
        let low = make_notif(vec![5], -10, "tmux");
        let high = make_notif(vec![5], 10, "email");
        let low_id = store.add(low);
        let high_id = store.add(high);
        store.remove(high_id);
        assert_eq!(store.active_for_key(5).unwrap().id, low_id);
    }

    #[test]
    fn test_remove_by_source() {
        let mut store = NotificationStore::new();
        store.add(make_notif(vec![1], 0, "tmux"));
        store.add(make_notif(vec![2], 0, "tmux"));
        store.add(make_notif(vec![3], 0, "email"));
        let removed = store.remove_by_source("tmux");
        assert_eq!(removed.len(), 2);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_ttl_expiry() {
        let mut store = NotificationStore::new();
        let mut n = make_notif(vec![1], 0, "test");
        n.ttl = Some(Duration::from_millis(0));
        n.created = Instant::now() - Duration::from_secs(1);
        store.add(n);
        let expired = store.expire();
        assert_eq!(expired.len(), 1);
        assert!(store.is_empty());
    }
}
