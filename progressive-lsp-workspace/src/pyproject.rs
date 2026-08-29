//! `pyproject.toml` adapter. Does not invoke CPython or ty.

use std::path::Path;

use crate::model::{PackageEntry, WorkspaceModel, WorkspaceSource};

#[derive(Clone, Copy, Debug, Default)]
pub struct PyprojectAdapter;

impl PyprojectAdapter {
    pub fn parse_name(text: &str) -> Option<String> {
        let mut in_project = false;
        for line in text.lines() {
            let line = line.trim();
            if line == "[project]" || line == "[tool.poetry]" {
                in_project = true;
                continue;
            }
            if line.starts_with('[') {
                in_project = false;
                continue;
            }
            if in_project {
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
}

impl WorkspaceSource for PyprojectAdapter {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel> {
        let p = root.join("pyproject.toml");
        if !p.is_file() {
            return None;
        }
        let text = std::fs::read_to_string(&p).ok()?;
        let id = Self::parse_name(&text).unwrap_or_else(|| {
            root.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("python")
                .to_string()
        });
        let mut model = WorkspaceModel::new("pyproject", root.to_path_buf());
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
    fn parse_and_detect_pyproject() {
        assert_eq!(
            PyprojectAdapter::parse_name("[project]\nname = \"greet\"\n").as_deref(),
            Some("greet")
        );
        assert_eq!(
            PyprojectAdapter::parse_name("[tool.poetry]\nname = 'app'\n").as_deref(),
            Some("app")
        );
        assert!(PyprojectAdapter::parse_name("[build-system]\nrequires=[]\n").is_none());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\nname = \"pkg\"\n").unwrap();
        let model = PyprojectAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "pyproject");
        assert_eq!(model.packages[0].id.as_str(), "pkg");
        assert!(PyprojectAdapter.detect(tempfile::tempdir().unwrap().path()).is_none());
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\n").unwrap();
        let model = PyprojectAdapter.detect(dir.path()).unwrap();
        assert!(!model.packages[0].id.as_str().is_empty());
    }
}
