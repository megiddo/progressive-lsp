//! Open buffers before vendor. Drain order is the priority invariant.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IndexClass {
    Open = 0,
    Recent = 1,
    SamePackage = 2,
    Other = 3,
    Vendor = 4,
}

impl IndexClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Recent => "recent",
            Self::SamePackage => "same_package",
            Self::Other => "other",
            Self::Vendor => "vendor",
        }
    }

    pub fn classify(path: &Path, open: &BTreeSet<PathBuf>, recent: &BTreeSet<PathBuf>, package_prefix: Option<&str>) -> Self {
        if is_vendor(path) {
            return Self::Vendor;
        }
        if open.contains(path) {
            return Self::Open;
        }
        if recent.contains(path) {
            return Self::Recent;
        }
        if let Some(prefix) = package_prefix {
            if path.to_string_lossy().contains(prefix) {
                return Self::SamePackage;
            }
        }
        Self::Other
    }
}

pub fn is_vendor(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains("/node_modules/")
        || s.starts_with("node_modules/")
        || s.contains("/vendor/")
        || s.starts_with("vendor/")
        || s.contains("/zig-cache/")
        || s.contains("/.zig-cache/")
        || s.starts_with("zig-cache/")
}

#[derive(Clone, Debug, Default)]
pub struct PriorityIndex {
    open: BTreeSet<PathBuf>,
    recent: BTreeSet<PathBuf>,
    package_prefix: Option<String>,
}

impl PriorityIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_open(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.recent.remove(&path);
        self.open.insert(path);
    }

    pub fn mark_closed(&mut self, path: &Path) {
        if self.open.remove(path) {
            self.recent.insert(path.to_path_buf());
        }
    }

    pub fn set_package_prefix(&mut self, prefix: impl Into<String>) {
        self.package_prefix = Some(prefix.into());
    }

    pub fn is_open(&self, path: &Path) -> bool {
        self.open.contains(path)
    }

    pub fn classify(&self, path: &Path) -> IndexClass {
        IndexClass::classify(path, &self.open, &self.recent, self.package_prefix.as_deref())
    }

    /// Drain dirty paths in priority order. Generation values are not reordered.
    pub fn order(&self, dirty: impl IntoIterator<Item = PathBuf>) -> VecDeque<PathBuf> {
        let mut items: Vec<PathBuf> = dirty.into_iter().collect();
        items.sort_by_key(|p| self.classify(p));
        items.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_before_vendor_and_generation_order_stable() {
        let mut p = PriorityIndex::new();
        p.mark_open("src/Open.java");
        p.set_package_prefix("com/example");
        p.mark_closed(Path::new("missing.java"));
        let ordered = p.order([
            PathBuf::from("vendor/Lib.java"),
            PathBuf::from("src/Open.java"),
            PathBuf::from("other/X.java"),
            PathBuf::from("src/com/example/Same.java"),
        ]);
        let got: Vec<_> = ordered.into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
        assert_eq!(
            got,
            vec![
                "src/Open.java",
                "src/com/example/Same.java",
                "other/X.java",
                "vendor/Lib.java",
            ]
        );
        assert_eq!(p.classify(Path::new("src/Open.java")), IndexClass::Open);
        assert_eq!(p.classify(Path::new("vendor/Lib.java")), IndexClass::Vendor);
        assert!(p.is_open(Path::new("src/Open.java")));
        p.mark_closed(Path::new("src/Open.java"));
        assert!(!p.is_open(Path::new("src/Open.java")));
        assert_eq!(p.classify(Path::new("src/Open.java")), IndexClass::Recent);
        assert_eq!(IndexClass::Open.as_str(), "open");
        assert_eq!(IndexClass::Recent.as_str(), "recent");
        assert_eq!(IndexClass::SamePackage.as_str(), "same_package");
        assert_eq!(IndexClass::Other.as_str(), "other");
        assert_eq!(IndexClass::Vendor.as_str(), "vendor");
        assert!(is_vendor(Path::new("a/node_modules/x.java")));
        assert!(is_vendor(Path::new("node_modules/x.java")));
        assert!(is_vendor(Path::new("vendor/Lib.java")));
        assert!(is_vendor(Path::new("zig-cache/x")));
        assert!(is_vendor(Path::new("a/zig-cache/x")));
        assert!(is_vendor(Path::new("a/.zig-cache/x")));
        assert!(!is_vendor(Path::new("src/A.java")));
    }
}
