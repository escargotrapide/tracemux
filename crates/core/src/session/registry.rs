//! Session registry — maps `sid` (UUID) to [`SessionState`].
//!
//! The server holds a single [`Registry`] instance behind
//! `Arc<Registry>`. Connection handlers look up a session by id
//! to subscribe to its [`Fanout`] or query metadata.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use uuid::Uuid;

use super::fanout::Fanout;

/// Per-session shared state.
#[derive(Debug, Clone)]
pub struct SessionState {
    /// Stable session id.
    pub sid: Uuid,
    /// Channel kind tag (e.g. `"serial"`, `"tcp"`).
    pub kind: String,
    /// Channel iface tag (e.g. `"COM3"`, `"10.0.0.1:5555"`).
    pub iface: String,
    /// Optional human label.
    pub label: Option<String>,
    /// Fan-out broadcaster.
    pub fanout: Fanout,
}

impl SessionState {
    /// Construct.
    #[must_use]
    pub fn new(kind: impl Into<String>, iface: impl Into<String>) -> Self {
        Self {
            sid: Uuid::new_v4(),
            kind: kind.into(),
            iface: iface.into(),
            label: None,
            fanout: Fanout::default(),
        }
    }
}

/// Concurrent session registry.
#[derive(Debug, Default)]
pub struct Registry {
    map: RwLock<HashMap<Uuid, Arc<SessionState>>>,
}

impl Registry {
    /// Construct empty.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a session, returning its sid.
    pub fn insert(&self, state: SessionState) -> Uuid {
        let sid = state.sid;
        self.map.write().insert(sid, Arc::new(state));
        sid
    }

    /// Look up a session.
    #[must_use]
    pub fn get(&self, sid: &Uuid) -> Option<Arc<SessionState>> {
        self.map.read().get(sid).cloned()
    }

    /// Remove a session.
    pub fn remove(&self, sid: &Uuid) -> Option<Arc<SessionState>> {
        self.map.write().remove(sid)
    }

    /// All current session ids.
    #[must_use]
    pub fn ids(&self) -> Vec<Uuid> {
        self.map.read().keys().copied().collect()
    }

    /// Number of sessions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.read().len()
    }

    /// Empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.read().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_remove() {
        let r = Registry::new();
        let s = SessionState::new("tcp", "127.0.0.1:1");
        let sid = r.insert(s);
        assert!(r.get(&sid).is_some());
        assert_eq!(r.len(), 1);
        let removed = r.remove(&sid).unwrap();
        assert_eq!(removed.sid, sid);
        assert!(r.is_empty());
    }

    #[test]
    fn ids_lists_all() {
        let r = Registry::new();
        let _ = r.insert(SessionState::new("a", "1"));
        let _ = r.insert(SessionState::new("b", "2"));
        assert_eq!(r.ids().len(), 2);
    }
}
