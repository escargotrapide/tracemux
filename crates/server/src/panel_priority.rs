//! Panel-priority routing.
//!
//! UI declares which panels are foreground / visible / hidden via
//! the wire-protocol `panel_state` message. The server stores those
//! hints here and consults them when deciding which
//! [`crate::coalesce::Bucket`] to use.

use std::collections::HashMap;
use std::sync::Mutex;

use uuid::Uuid;

use crate::coalesce::Bucket;

/// Visibility hint advertised by a UI panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Panel is foreground (active tab).
    Foreground,
    /// Panel is visible but not foreground.
    Visible,
    /// Panel is hidden / minimised.
    Hidden,
}

impl Visibility {
    /// Coalescing bucket implied by this visibility.
    #[must_use]
    pub const fn bucket(self) -> Bucket {
        match self {
            Self::Foreground => Bucket::Live,
            Self::Visible => Bucket::Visible,
            Self::Hidden => Bucket::Hidden,
        }
    }
}

/// Identifier for one UI panel (per connection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PanelId(pub Uuid);

/// Registry of `PanelId -> Visibility` hints.
#[derive(Debug, Default)]
pub struct PanelPriority {
    inner: Mutex<HashMap<PanelId, Visibility>>,
}

impl PanelPriority {
    /// Empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the visibility of `panel`.
    ///
    /// Lock-poisoning is treated as "reset and retry".
    pub fn set(&self, panel: PanelId, vis: Visibility) {
        if let Ok(mut g) = self.inner.lock() {
            g.insert(panel, vis);
        }
    }

    /// Remove a panel (e.g. on close).
    pub fn remove(&self, panel: PanelId) {
        if let Ok(mut g) = self.inner.lock() {
            g.remove(&panel);
        }
    }

    /// Visibility of `panel`, or [`Visibility::Hidden`] if unknown.
    #[must_use]
    pub fn get(&self, panel: PanelId) -> Visibility {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.get(&panel).copied())
            .unwrap_or(Visibility::Hidden)
    }

    /// Coalescing bucket implied by `panel`'s visibility.
    #[must_use]
    pub fn bucket(&self, panel: PanelId) -> Bucket {
        self.get(panel).bucket()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_panel_is_hidden() {
        let p = PanelPriority::new();
        let id = PanelId(Uuid::new_v4());
        assert_eq!(p.get(id), Visibility::Hidden);
        assert_eq!(p.bucket(id), Bucket::Hidden);
    }

    #[test]
    fn set_and_remove() {
        let p = PanelPriority::new();
        let id = PanelId(Uuid::new_v4());
        p.set(id, Visibility::Foreground);
        assert_eq!(p.get(id), Visibility::Foreground);
        assert_eq!(p.bucket(id), Bucket::Live);
        p.remove(id);
        assert_eq!(p.get(id), Visibility::Hidden);
    }
}
