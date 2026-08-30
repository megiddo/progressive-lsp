//! `TabStrip` / `TabId`: identity + collection. At most one focused tab.

use std::path::{Path, PathBuf};

/// Identity of an open tab. Equality is path equality.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TabId {
    path: PathBuf,
}

impl TabId {
    pub fn from_path(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn as_path(&self) -> &Path {
        &self.path
    }

    pub fn label(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }
}

/// Open tabs with a single optional focus. Close of a missing id is a no-op.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabStrip {
    tabs: Vec<TabId>,
    focused: Option<TabId>,
}

impl TabStrip {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            focused: None,
        }
    }

    pub fn open(&mut self, path: impl AsRef<Path>) -> TabId {
        let id = TabId::from_path(path);
        if !self.tabs.iter().any(|t| t == &id) {
            self.tabs.push(id.clone());
        }
        self.focused = Some(id.clone());
        id
    }

    pub fn focus(&mut self, id: &TabId) {
        if self.tabs.iter().any(|t| t == id) {
            self.focused = Some(id.clone());
        }
    }

    /// Missing id is a no-op. Closing the focused tab focuses a neighbor.
    pub fn close(&mut self, id: &TabId) {
        let Some(idx) = self.tabs.iter().position(|t| t == id) else {
            return;
        };
        self.tabs.remove(idx);
        if self.focused.as_ref() == Some(id) {
            self.focused = if idx > 0 {
                Some(self.tabs[idx - 1].clone())
            } else {
                self.tabs.first().cloned()
            };
        }
    }

    pub fn tabs(&self) -> &[TabId] {
        &self.tabs
    }

    pub fn focused(&self) -> Option<&TabId> {
        self.focused.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn contains(&self, id: &TabId) -> bool {
        self.tabs.iter().any(|t| t == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_id_identity_equality_is_path_equality() {
        let a = TabId::from_path("/ws/src/lib.rs");
        let b = TabId::from_path("/ws/src/lib.rs");
        let c = TabId::from_path("/ws/src/main.rs");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.as_path(), Path::new("/ws/src/lib.rs"));
        assert_eq!(a.label(), "lib.rs");
        assert_eq!(TabId::from_path("/").label(), "/");
        assert_eq!(TabId::from_path("").label(), "");
    }

    #[test]
    fn tab_strip_identity_collection_open_focuses_and_dedups() {
        let mut strip = TabStrip::new();
        assert!(strip.is_empty());
        assert_eq!(strip.len(), 0);
        assert!(strip.focused().is_none());
        assert_eq!(TabStrip::default(), strip);

        let first = strip.open("/ws/a.rs");
        assert_eq!(strip.len(), 1);
        assert_eq!(strip.focused(), Some(&first));
        assert!(strip.contains(&first));
        assert!(!strip.is_empty());

        let again = strip.open("/ws/a.rs");
        assert_eq!(again, first);
        assert_eq!(strip.len(), 1);
        assert_eq!(strip.focused(), Some(&first));

        let second = strip.open("/ws/b.rs");
        assert_eq!(strip.len(), 2);
        assert_eq!(strip.focused(), Some(&second));
        assert_eq!(strip.tabs()[0], first);
        assert_eq!(strip.tabs()[1], second);
    }

    #[test]
    fn tab_strip_identity_collection_focus_is_at_most_one() {
        let mut strip = TabStrip::new();
        let a = strip.open("/ws/a.rs");
        let b = strip.open("/ws/b.rs");
        strip.focus(&a);
        assert_eq!(strip.focused(), Some(&a));
        strip.focus(&b);
        assert_eq!(strip.focused(), Some(&b));
        let missing = TabId::from_path("/ws/missing.rs");
        strip.focus(&missing);
        assert_eq!(strip.focused(), Some(&b));
        assert!(!strip.contains(&missing));
        let focused = strip.focused().cloned();
        assert_eq!(focused.as_ref().map(|_| 1).unwrap_or(0), 1);
    }

    #[test]
    fn tab_strip_identity_collection_close_missing_id_is_noop() {
        let mut strip = TabStrip::new();
        let a = strip.open("/ws/a.rs");
        strip.close(&TabId::from_path("/ws/nope.rs"));
        assert_eq!(strip.len(), 1);
        assert_eq!(strip.focused(), Some(&a));
        let empty = TabStrip::new();
        empty.clone().close(&a);
        assert!(TabStrip::new().is_empty());
    }

    #[test]
    fn tab_strip_identity_collection_close_focused_picks_neighbor() {
        let mut strip = TabStrip::new();
        let a = strip.open("/ws/a.rs");
        let b = strip.open("/ws/b.rs");
        let c = strip.open("/ws/c.rs");
        strip.focus(&b);
        strip.close(&b);
        assert_eq!(strip.len(), 2);
        assert_eq!(strip.focused(), Some(&a));
        assert!(!strip.contains(&b));

        strip.focus(&a);
        strip.close(&a);
        assert_eq!(strip.len(), 1);
        assert_eq!(strip.focused(), Some(&c));

        strip.close(&c);
        assert!(strip.is_empty());
        assert!(strip.focused().is_none());
    }

    #[test]
    fn tab_strip_identity_collection_close_non_focused_keeps_focus() {
        let mut strip = TabStrip::new();
        let a = strip.open("/ws/a.rs");
        let b = strip.open("/ws/b.rs");
        strip.focus(&b);
        strip.close(&a);
        assert_eq!(strip.len(), 1);
        assert_eq!(strip.focused(), Some(&b));
        assert_eq!(strip.tabs()[0], b);
    }

    #[test]
    fn tab_strip_identity_collection_close_last_focused_selects_previous() {
        let mut strip = TabStrip::new();
        let a = strip.open("/ws/a.rs");
        let b = strip.open("/ws/b.rs");
        let c = strip.open("/ws/c.rs");
        assert_eq!(strip.focused(), Some(&c));
        strip.close(&c);
        assert_eq!(strip.focused(), Some(&b));
        assert_eq!(strip.tabs(), &[a, b]);
    }

    #[test]
    fn tab_strip_identity_collection_close_first_when_focused_moves_to_next() {
        let mut strip = TabStrip::new();
        let a = strip.open("/ws/a.rs");
        let b = strip.open("/ws/b.rs");
        strip.focus(&a);
        strip.close(&a);
        assert_eq!(strip.focused(), Some(&b));
        assert_eq!(strip.tabs(), &[b]);
    }
}
