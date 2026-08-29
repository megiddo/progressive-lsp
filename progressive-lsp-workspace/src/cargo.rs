//! `Cargo.toml` adapter. Does not invoke rustc or rust-analyzer.

use std::path::Path;

use crate::model::{PackageEntry, WorkspaceModel, WorkspaceSource};

#[derive(Clone, Copy, Debug, Default)]
pub struct CargoTomlAdapter;

impl CargoTomlAdapter {
    pub fn parse_name(text: &str) -> Option<String> {
        let mut in_package = false;
        for line in text.lines() {
            let line = line.trim();
            if line == "[package]" {
                in_package = true;
                continue;
            }
            if line.starts_with('[') {
                in_package = false;
                continue;
            }
            if in_package {
                if let Some(rest) = line.strip_prefix("name") {
                    let rest = rest.trim().trim_start_matches('=').trim();
                    let name = rest.trim_matches('"').trim_matches('\'').trim();
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
        None
    }

    pub fn parse_edition(text: &str) -> Option<String> {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("edition") {
                let rest = rest.trim().trim_start_matches('=').trim();
                let v = rest.trim_matches('"').trim_matches('\'').trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
        None
    }
}

impl WorkspaceSource for CargoTomlAdapter {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel> {
        let p = root.join("Cargo.toml");
        if !p.is_file() {
            return None;
        }
        let text = std::fs::read_to_string(&p).ok()?;
        let id = Self::parse_name(&text).unwrap_or_else(|| {
            root.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("rust")
                .to_string()
        });
        let mut model = WorkspaceModel::new("cargo", root.to_path_buf());
        let src = root.join("src");
        let mut pkg = PackageEntry::new(id, root.to_path_buf());
        if src.is_dir() {
            pkg = pkg.with_source_root(src);
        } else {
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
    fn parse_and_detect_cargo() {
        let text = "[package]\nname = \"greet\"\nedition = \"2021\"\n";
        assert_eq!(CargoTomlAdapter::parse_name(text).as_deref(), Some("greet"));
        assert_eq!(CargoTomlAdapter::parse_edition(text).as_deref(), Some("2021"));
        assert!(CargoTomlAdapter::parse_name("[workspace]\nmembers=[]\n").is_none());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"app\"\n").unwrap();
        let model = CargoTomlAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "cargo");
        assert_eq!(model.packages[0].id.as_str(), "app");
        assert!(CargoTomlAdapter.detect(tempfile::tempdir().unwrap().path()).is_none());
    }
}
