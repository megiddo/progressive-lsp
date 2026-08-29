//! WorkspaceSource adapters. Disk/build files → WorkspaceModel. No host JDK.

pub mod directory;
pub mod eclipse;
pub mod gradle;
pub mod maven;
pub mod model;

pub use directory::DirectoryAdapter;
pub use eclipse::EclipseAdapter;
pub use gradle::GradleAdapter;
pub use maven::MavenAdapter;
pub use model::{PackageEntry, WorkspaceModel, WorkspaceSource};

use std::path::Path;

/// Try Maven, Gradle, Eclipse, then Directory. First match wins.
pub fn detect_workspace(root: &Path) -> Option<WorkspaceModel> {
    if let Some(m) = MavenAdapter.detect(root) {
        return Some(m);
    }
    if let Some(m) = GradleAdapter.detect(root) {
        return Some(m);
    }
    if let Some(m) = EclipseAdapter.detect(root) {
        return Some(m);
    }
    DirectoryAdapter.detect(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_prefers_maven_over_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pom.xml"),
            "<project><artifactId>root</artifactId></project>\n",
        )
        .unwrap();
        let model = detect_workspace(dir.path()).unwrap();
        assert_eq!(model.kind, "maven");
        assert_eq!(model.packages[0].id.as_str(), "root");
    }

    #[test]
    fn detect_falls_through_to_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/A.java"), "class A {}").unwrap();
        let model = detect_workspace(dir.path()).unwrap();
        assert_eq!(model.kind, "directory");
    }

    #[test]
    fn detect_none_on_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(detect_workspace(dir.path()).is_none());
    }

    #[test]
    fn detect_prefers_gradle_then_eclipse() {
        let gradle = tempfile::tempdir().unwrap();
        std::fs::write(gradle.path().join("settings.gradle.kts"), "include(\"lib\")\n").unwrap();
        std::fs::create_dir_all(gradle.path().join("lib/src/main/java")).unwrap();
        let g = detect_workspace(gradle.path()).unwrap();
        assert_eq!(g.kind, "gradle");

        let eclipse = tempfile::tempdir().unwrap();
        std::fs::write(
            eclipse.path().join(".project"),
            "<projectDescription><name>e</name></projectDescription>\n",
        )
        .unwrap();
        std::fs::create_dir_all(eclipse.path().join("src")).unwrap();
        let e = detect_workspace(eclipse.path()).unwrap();
        assert_eq!(e.kind, "eclipse");
    }
}
