//! `*.csproj` adapter. Manifest only. No host `dotnet`.

use std::path::Path;

use crate::model::{PackageEntry, WorkspaceModel, WorkspaceSource};

#[derive(Clone, Copy, Debug, Default)]
pub struct CsprojAdapter;

impl CsprojAdapter {
    pub fn find_csproj(root: &Path) -> Option<std::path::PathBuf> {
        let Ok(entries) = std::fs::read_dir(root) else {
            return None;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("csproj") {
                return Some(path);
            }
        }
        None
    }
}

impl WorkspaceSource for CsprojAdapter {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel> {
        let csproj = Self::find_csproj(root)?;
        let id = csproj
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("csharp")
            .to_string();
        let mut model = WorkspaceModel::new("csproj", root.to_path_buf());
        let pkg = PackageEntry::new(id, root.to_path_buf()).with_source_root(root.to_path_buf());
        model.add_package(pkg);
        Some(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_csproj() {
        let dir = tempfile::tempdir().unwrap();
        assert!(CsprojAdapter::find_csproj(dir.path()).is_none());
        assert!(CsprojAdapter.detect(dir.path()).is_none());
        std::fs::write(dir.path().join("App.csproj"), "<Project></Project>\n").unwrap();
        let model = CsprojAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "csproj");
        assert_eq!(model.packages[0].id.as_str(), "App");
    }
}
