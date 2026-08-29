//! WorkspaceSource adapters. Disk/build files → WorkspaceModel. No host JDK.

pub mod cargo;
pub mod composer;
pub mod directory;
pub mod eclipse;
pub mod go_mod;
pub mod gradle;
pub mod maven;
pub mod model;
pub mod pyproject;
pub mod zig_build;

pub use cargo::CargoTomlAdapter;
pub use composer::ComposerAdapter;
pub use directory::DirectoryAdapter;
pub use eclipse::EclipseAdapter;
pub use go_mod::GoModAdapter;
pub use gradle::GradleAdapter;
pub use maven::MavenAdapter;
pub use model::{PackageEntry, WorkspaceModel, WorkspaceSource};
pub use pyproject::PyprojectAdapter;
pub use zig_build::ZigBuildAdapter;

use std::path::Path;

/// Try language-specific adapters, then Directory. First match wins.
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
    if let Some(m) = ComposerAdapter.detect(root) {
        return Some(m);
    }
    if let Some(m) = GoModAdapter.detect(root) {
        return Some(m);
    }
    if let Some(m) = ZigBuildAdapter.detect(root) {
        return Some(m);
    }
    if let Some(m) = CargoTomlAdapter.detect(root) {
        return Some(m);
    }
    if let Some(m) = PyprojectAdapter.detect(root) {
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

    #[test]
    fn detect_composer_go_zig() {
        let php = tempfile::tempdir().unwrap();
        std::fs::write(php.path().join("composer.json"), r#"{"autoload":{"psr-4":{"A\\":"src/"}}}"#).unwrap();
        std::fs::create_dir_all(php.path().join("src")).unwrap();
        assert_eq!(detect_workspace(php.path()).unwrap().kind, "composer");

        let go = tempfile::tempdir().unwrap();
        std::fs::write(go.path().join("go.mod"), "module example.com/x\n").unwrap();
        assert_eq!(detect_workspace(go.path()).unwrap().kind, "go.mod");

        let zig = tempfile::tempdir().unwrap();
        std::fs::write(zig.path().join("build.zig"), "pub fn build() void {}\n").unwrap();
        assert_eq!(detect_workspace(zig.path()).unwrap().kind, "build.zig");

        let cargo = tempfile::tempdir().unwrap();
        std::fs::write(cargo.path().join("Cargo.toml"), "[package]\nname = \"r\"\n").unwrap();
        assert_eq!(detect_workspace(cargo.path()).unwrap().kind, "cargo");

        let py = tempfile::tempdir().unwrap();
        std::fs::write(py.path().join("pyproject.toml"), "[project]\nname = \"p\"\n").unwrap();
        assert_eq!(detect_workspace(py.path()).unwrap().kind, "pyproject");
    }
}
