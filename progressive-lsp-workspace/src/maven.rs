//! Maven adapter. Reads pom.xml. Does not invoke `mvn` or a JDK.

use std::path::{Path, PathBuf};

use crate::model::{PackageEntry, WorkspaceModel, WorkspaceSource};

#[derive(Clone, Copy, Debug, Default)]
pub struct MavenAdapter;

impl MavenAdapter {
    pub fn parse_artifact_id(pom: &str) -> Option<String> {
        xml_tag(pom, "artifactId")
    }

    pub fn parse_modules(pom: &str) -> Vec<String> {
        let mut mods = Vec::new();
        let mut rest = pom;
        while let Some(start) = rest.find("<module>") {
            rest = &rest[start + 8..];
            if let Some(end) = rest.find("</module>") {
                let name = rest[..end].trim();
                if !name.is_empty() {
                    mods.push(name.to_string());
                }
                rest = &rest[end + 9..];
            } else {
                break;
            }
        }
        mods
    }
}

fn xml_tag(src: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = src.find(&open)?;
    let rest = &src[start + open.len()..];
    let end = rest.find(&close)?;
    let v = rest[..end].trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

fn package_from_pom(dir: &Path) -> Option<PackageEntry> {
    let pom = dir.join("pom.xml");
    let text = std::fs::read_to_string(&pom).ok()?;
    let id = MavenAdapter::parse_artifact_id(&text)
        .unwrap_or_else(|| dir.file_name().and_then(|s| s.to_str()).unwrap_or("maven").into());
    let src = dir.join("src/main/java");
    Some(PackageEntry::new(id, dir.to_path_buf()).with_source_root(src))
}

impl WorkspaceSource for MavenAdapter {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel> {
        let pom_path = root.join("pom.xml");
        if !pom_path.is_file() {
            return None;
        }
        let text = std::fs::read_to_string(&pom_path).ok()?;
        let mut model = WorkspaceModel::new("maven", root.to_path_buf());
        let modules = Self::parse_modules(&text);
        if modules.is_empty() {
            if let Some(pkg) = package_from_pom(root) {
                model.add_package(pkg);
            }
        } else {
            for m in modules {
                let child = root.join(&m);
                if let Some(pkg) = package_from_pom(&child) {
                    model.add_package(pkg);
                }
            }
        }
        if model.is_empty() {
            None
        } else {
            Some(model)
        }
    }
}

pub fn default_java_source(root: &Path) -> PathBuf {
    root.join("src/main/java")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modules_and_artifact() {
        let pom = r#"
            <project>
              <artifactId>app</artifactId>
              <modules>
                <module>lib</module>
                <module>app</module>
              </modules>
            </project>
        "#;
        assert_eq!(MavenAdapter::parse_artifact_id(pom).as_deref(), Some("app"));
        assert_eq!(MavenAdapter::parse_modules(pom), vec!["lib", "app"]);
        assert_eq!(MavenAdapter::parse_artifact_id("<project/>"), None);
        assert!(MavenAdapter::parse_modules("<module>").is_empty());
        assert!(MavenAdapter::parse_modules("<module>  </module>").is_empty());
    }

    #[test]
    fn detect_multi_module_without_jdk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pom.xml"),
            "<project><modules><module>lib</module><module>app</module></modules></project>\n",
        )
        .unwrap();
        for name in ["lib", "app"] {
            let src = dir.path().join(name).join("src/main/java");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                dir.path().join(name).join("pom.xml"),
                format!("<project><artifactId>{name}</artifactId></project>\n"),
            )
            .unwrap();
        }
        let model = MavenAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "maven");
        assert_eq!(model.packages.len(), 2);
        assert_eq!(model.packages[0].id.as_str(), "lib");
        assert_eq!(model.packages[1].id.as_str(), "app");
        assert!(model.packages[0].source_roots[0].ends_with("src/main/java"));
        assert!(default_java_source(dir.path()).ends_with("src/main/java"));
    }

    #[test]
    fn detect_none_without_pom() {
        let dir = tempfile::tempdir().unwrap();
        assert!(MavenAdapter.detect(dir.path()).is_none());
    }

    #[test]
    fn single_module_falls_back_to_directory_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project></project>\n").unwrap();
        std::fs::create_dir_all(dir.path().join("src/main/java")).unwrap();
        let model = MavenAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.packages.len(), 1);
        assert!(!model.packages[0].id.as_str().is_empty());
    }

    #[test]
    fn missing_child_modules_yield_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pom.xml"),
            "<project><modules><module>gone</module></modules></project>\n",
        )
        .unwrap();
        assert!(MavenAdapter.detect(dir.path()).is_none());
    }
}
