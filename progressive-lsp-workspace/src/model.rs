//! WorkspaceModel DTO. Roots and classpath-like entries exist on disk.

use std::path::{Path, PathBuf};

use progressive_lsp_core::PackageId;

pub trait WorkspaceSource {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageEntry {
    pub id: PackageId,
    pub root: PathBuf,
    pub source_roots: Vec<PathBuf>,
    pub classpath: Vec<PathBuf>,
}

impl PackageEntry {
    pub fn new(id: impl AsRef<str>, root: PathBuf) -> Self {
        Self {
            id: PackageId::new(id.as_ref()),
            root,
            source_roots: Vec::new(),
            classpath: Vec::new(),
        }
    }

    pub fn with_source_root(mut self, path: PathBuf) -> Self {
        if path.exists() && !self.source_roots.contains(&path) {
            self.source_roots.push(path);
        }
        self
    }

    pub fn with_classpath(mut self, path: PathBuf) -> Self {
        if path.exists() && !self.classpath.contains(&path) {
            self.classpath.push(path);
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceModel {
    pub kind: String,
    pub root: PathBuf,
    pub packages: Vec<PackageEntry>,
}

impl WorkspaceModel {
    pub fn new(kind: impl Into<String>, root: PathBuf) -> Self {
        Self {
            kind: kind.into(),
            root,
            packages: Vec::new(),
        }
    }

    pub fn add_package(&mut self, pkg: PackageEntry) {
        self.packages.push(pkg);
    }

    pub fn package_ids(&self) -> Vec<PackageId> {
        self.packages.iter().map(|p| p.id.clone()).collect()
    }

    pub fn all_source_roots(&self) -> Vec<PathBuf> {
        self.packages
            .iter()
            .flat_map(|p| p.source_roots.iter().cloned())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_only_keeps_existing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let missing = dir.path().join("nope");
        let pkg = PackageEntry::new("p", dir.path().to_path_buf())
            .with_source_root(src.clone())
            .with_source_root(missing.clone())
            .with_classpath(src.clone())
            .with_classpath(missing);
        assert_eq!(pkg.source_roots, vec![src.clone()]);
        assert_eq!(pkg.classpath, vec![src]);
        let mut model = WorkspaceModel::new("directory", dir.path().to_path_buf());
        assert!(model.is_empty());
        model.add_package(pkg);
        assert_eq!(model.package_ids(), vec![PackageId::new("p")]);
        assert_eq!(model.all_source_roots().len(), 1);
        assert!(!model.is_empty());
        assert_eq!(PackageId::new("q").as_str(), "q");
    }
}
