//! Per-connection multiplexer.
//!
//! A WSS connection carries multiple logical channels (e.g. one per
//! session subscription). [`Mux`] hands out monotonically increasing
//! [`ChannelId`]s and tracks active ones.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

/// Numeric channel identifier (unique per connection, never reused).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelId(pub u32);

/// Per-channel metadata.
#[derive(Debug, Clone)]
pub struct ChannelInfo {
    /// Stable kind tag (`"raw"`, `"lines"`, `"frames"`, ...).
    pub kind: String,
    /// Optional target session UUID as string.
    pub sid: Option<String>,
}

/// Per-connection multiplexer.
#[derive(Debug, Default)]
pub struct Mux {
    next: AtomicU32,
    channels: Mutex<HashMap<ChannelId, ChannelInfo>>,
}

impl Mux {
    /// Empty mux.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate a new channel id and register `info`.
    ///
    /// # Panics
    /// Panics if more than `u32::MAX` channels are allocated on a
    /// single connection.
    pub fn open(&self, info: ChannelInfo) -> ChannelId {
        let id = ChannelId(self.next.fetch_add(1, Ordering::AcqRel));
        if let Ok(mut g) = self.channels.lock() {
            g.insert(id, info);
        }
        id
    }

    /// Close `id`. Returns the removed metadata, if any.
    pub fn close(&self, id: ChannelId) -> Option<ChannelInfo> {
        self.channels.lock().ok().and_then(|mut g| g.remove(&id))
    }

    /// Lookup metadata.
    #[must_use]
    pub fn get(&self, id: ChannelId) -> Option<ChannelInfo> {
        self.channels.lock().ok().and_then(|g| g.get(&id).cloned())
    }

    /// Number of active channels.
    #[must_use]
    pub fn len(&self) -> usize {
        self.channels.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Whether the mux has no active channels.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_monotonic_and_unique() {
        let m = Mux::new();
        let a = m.open(ChannelInfo { kind: "raw".into(), sid: None });
        let b = m.open(ChannelInfo { kind: "lines".into(), sid: None });
        assert!(b.0 > a.0);
        assert_eq!(m.len(), 2);
        let info = m.close(a).unwrap();
        assert_eq!(info.kind, "raw");
        assert!(m.get(a).is_none());
        assert_eq!(m.len(), 1);
    }
}
