//! `build.zig` adapter. Discovers Zig packages. Does not invoke `zig` or zls.

use std::path::Path;

use crate::model::{PackageEntry, WorkspaceModel, WorkspaceSource};

#[derive(Clone, Copy, Debug, Default)]
pub struct ZigBuildAdapter;

impl ZigBuildAdapter {
    pub fn has_marker(root: &Path) -> bool {
        root.join("build.zig").is_file() || root.join("build.zig.zon").is_file()
    }
}

impl WorkspaceSource for ZigBuildAdapter {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel> {
        if !Self::has_marker(root) {
            return None;
        }
        let id = root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("zig")
            .to_string();
        let mut model = WorkspaceModel::new("build.zig", root.to_path_buf());
        let src = root.join("src");
        let mut pkg = PackageEntry::new(id, root.to_path_buf());
        pkg = pkg.with_source_root(src);
        if pkg.source_roots.is_empty() {
            pkg = pkg.with_source_root(root.to_path_buf());
        }
        model.add_package(pkg);
        Some(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_build_zig() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.zig"), "pub fn build(b: *std.Build) void {}\n").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let model = ZigBuildAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "build.zig");
        assert_eq!(model.packages.len(), 1);
        assert!(ZigBuildAdapter::has_marker(dir.path()));
        assert!(!ZigBuildAdapter::has_marker(tempfile::tempdir().unwrap().path()));
        assert!(ZigBuildAdapter.detect(tempfile::tempdir().unwrap().path()).is_none());
    }

    #[test]
    fn detect_zon_without_src_uses_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.zig.zon"), ".{\n.name = .demo,\n}\n").unwrap();
        let model = ZigBuildAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "build.zig");
        assert!(!model.packages[0].source_roots.is_empty());
    }
}
