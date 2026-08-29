//! Gradle adapter. Parses settings/build files. No Gradle daemon, no JDK.

use std::path::Path;

use crate::model::{PackageEntry, WorkspaceModel, WorkspaceSource};

#[derive(Clone, Copy, Debug, Default)]
pub struct GradleAdapter;

impl GradleAdapter {
    pub fn parse_includes(settings: &str) -> Vec<String> {
        let mut out = Vec::new();
        for raw_line in settings.lines() {
            let line = raw_line.trim();
            if !line.starts_with("include") {
                continue;
            }
            let rest = line.trim_start_matches("include").trim();
            for token in rest.split([',', '(', ')', '[', ']']) {
                let name = token.trim().trim_matches(['\'', '"', ' ', ':']);
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                    if !out.iter().any(|e| e == name) {
                        out.push(name.to_string());
                    }
                }
            }
        }
        out
    }

    pub fn has_gradle_marker(root: &Path) -> bool {
        root.join("settings.gradle").is_file()
            || root.join("settings.gradle.kts").is_file()
            || root.join("build.gradle").is_file()
            || root.join("build.gradle.kts").is_file()
    }
}

fn read_settings(root: &Path) -> Option<String> {
    for name in ["settings.gradle", "settings.gradle.kts"] {
        let p = root.join(name);
        if p.is_file() {
            return std::fs::read_to_string(p).ok();
        }
    }
    None
}

fn package_at(dir: &Path, id: &str) -> PackageEntry {
    PackageEntry::new(id, dir.to_path_buf()).with_source_root(dir.join("src/main/java"))
}

impl WorkspaceSource for GradleAdapter {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel> {
        if !Self::has_gradle_marker(root) {
            return None;
        }
        let mut model = WorkspaceModel::new("gradle", root.to_path_buf());
        let includes = read_settings(root)
            .map(|s| Self::parse_includes(&s))
            .unwrap_or_default();
        if includes.is_empty() {
            let id = root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("gradle");
            model.add_package(package_at(root, id));
        } else {
            for name in includes {
                let child = root.join(&name);
                if child.is_dir() {
                    model.add_package(package_at(&child, &name));
                }
            }
        }
        if model.packages.iter().all(|p| p.source_roots.is_empty()) && model.packages.len() <= 1 {
            // still valid: marker exists, package may have no src yet
        }
        Some(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_include_styles() {
        let s = r#"
            rootProject.name = "multi"
            include 'lib', "app"
            include(":extra")
        "#;
        let mods = GradleAdapter::parse_includes(s);
        assert!(mods.contains(&"lib".into()));
        assert!(mods.contains(&"app".into()));
        assert!(mods.contains(&"extra".into()));
        let dashed = GradleAdapter::parse_includes(r#"include "my-lib", "my_mod""#);
        assert!(dashed.contains(&"my-lib".into()));
        assert!(dashed.contains(&"my_mod".into()));
        assert!(GradleAdapter::parse_includes("plugins { }").is_empty());
        assert!(GradleAdapter::parse_includes("include ''").is_empty());
    }

    #[test]
    fn detect_settings_gradle_multi() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.gradle"), "include 'lib', 'app'\n").unwrap();
        for name in ["lib", "app"] {
            let src = dir.path().join(name).join("src/main/java");
            std::fs::create_dir_all(&src).unwrap();
        }
        let model = GradleAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "gradle");
        assert_eq!(model.packages.len(), 2);
        assert_eq!(model.packages[0].id.as_str(), "lib");
        assert!(model.packages[1].source_roots[0].ends_with("src/main/java"));
    }

    #[test]
    fn detect_build_gradle_single() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "plugins {}\n").unwrap();
        std::fs::create_dir_all(dir.path().join("src/main/java")).unwrap();
        let model = GradleAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.packages.len(), 1);
        assert!(GradleAdapter::has_gradle_marker(dir.path()));
    }

    #[test]
    fn detect_none_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        assert!(GradleAdapter.detect(dir.path()).is_none());
        assert!(!GradleAdapter::has_gradle_marker(dir.path()));
    }

    #[test]
    fn detect_build_gradle_kts_single() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "plugins {}\n").unwrap();
        std::fs::create_dir_all(dir.path().join("src/main/java")).unwrap();
        let model = GradleAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "gradle");
        assert_eq!(model.packages.len(), 1);
        assert!(GradleAdapter::has_gradle_marker(dir.path()));
    }
}
