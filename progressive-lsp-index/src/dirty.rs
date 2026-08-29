//! Dirty set: paths + generation. Generation is monotonic.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirtySet {
    generations: BTreeMap<PathBuf, u64>,
}

impl DirtySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark(&mut self, path: impl Into<PathBuf>, generation: u64) {
        let path = path.into();
        let entry = self.generations.entry(path).or_insert(0);
        if generation > *entry {
            *entry = generation;
        }
    }

    pub fn generation_of(&self, path: &Path) -> Option<u64> {
        self.generations.get(path).copied()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.generations.contains_key(path)
    }

    pub fn is_empty(&self) -> bool {
        self.generations.is_empty()
    }

    pub fn len(&self) -> usize {
        self.generations.len()
    }

    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.generations.keys()
    }

    pub fn take(&mut self, path: &Path) -> Option<u64> {
        self.generations.remove(path)
    }

    pub fn clear(&mut self) {
        self.generations.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_is_monotonic_and_queryable() {
        let mut d = DirtySet::new();
        assert!(d.is_empty());
        d.mark("a.java", 3);
        assert!(!d.is_empty());
        d.mark("a.java", 1);
        d.mark("a.java", 3);
        d.mark("b.java", 4);
        assert_eq!(d.generation_of(Path::new("a.java")), Some(3));
        assert_eq!(d.generation_of(Path::new("b.java")), Some(4));
        assert_eq!(d.generation_of(Path::new("c.java")), None);
        assert!(d.contains(Path::new("a.java")));
        assert!(!d.contains(Path::new("c.java")));
        assert_eq!(d.len(), 2);
        let paths: Vec<_> = d.paths().map(|p| p.to_string_lossy().into_owned()).collect();
        assert_eq!(paths, vec!["a.java", "b.java"]);
        assert_eq!(d.take(Path::new("a.java")), Some(3));
        assert!(!d.contains(Path::new("a.java")));
        d.clear();
        assert!(d.is_empty());
    }
}
