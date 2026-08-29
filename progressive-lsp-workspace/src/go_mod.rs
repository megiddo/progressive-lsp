//! `go.mod` adapter. Reads module path. Does not invoke `go` or gopls.

use std::path::Path;

use crate::model::{PackageEntry, WorkspaceModel, WorkspaceSource};

#[derive(Clone, Copy, Debug, Default)]
pub struct GoModAdapter;

impl GoModAdapter {
    pub fn parse_module(text: &str) -> Option<String> {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("module ") {
                let name = rest.trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
        None
    }

    pub fn parse_go_directive(text: &str) -> Option<String> {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("go ") {
                let v = rest.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
        None
    }
}

impl WorkspaceSource for GoModAdapter {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel> {
        let gom = root.join("go.mod");
        if !gom.is_file() {
            return None;
        }
        let text = std::fs::read_to_string(&gom).ok()?;
        let id = Self::parse_module(&text).unwrap_or_else(|| {
            root.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("go")
                .to_string()
        });
        let mut model = WorkspaceModel::new("go.mod", root.to_path_buf());
        let pkg = PackageEntry::new(id, root.to_path_buf()).with_source_root(root.to_path_buf());
        model.add_package(pkg);
        Some(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_module_and_go() {
        let text = "module example.com/greet\n\ngo 1.22\n";
        assert_eq!(GoModAdapter::parse_module(text).as_deref(), Some("example.com/greet"));
        assert_eq!(GoModAdapter::parse_go_directive(text).as_deref(), Some("1.22"));
        assert!(GoModAdapter::parse_module("require x").is_none());
        assert!(GoModAdapter::parse_go_directive("module x").is_none());
        assert!(GoModAdapter::parse_module("module   ").is_none());
    }

    #[test]
    fn detect_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/app\ngo 1.22\n").unwrap();
        let model = GoModAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "go.mod");
        assert_eq!(model.packages[0].id.as_str(), "example.com/app");
        assert!(GoModAdapter.detect(tempfile::tempdir().unwrap().path()).is_none());
    }

    #[test]
    fn detect_without_module_line_uses_dir_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "go 1.22\n").unwrap();
        let model = GoModAdapter.detect(dir.path()).unwrap();
        assert!(!model.packages[0].id.as_str().is_empty());
    }
}
