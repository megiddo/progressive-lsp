//! Eclipse adapter. Reads `.project` / `.classpath`. No JDT.

use std::path::Path;

use crate::model::{PackageEntry, WorkspaceModel, WorkspaceSource};

#[derive(Clone, Copy, Debug, Default)]
pub struct EclipseAdapter;

impl EclipseAdapter {
    pub fn parse_project_name(xml: &str) -> Option<String> {
        let start = xml.find("<name>")?;
        let rest = &xml[start + 6..];
        let end = rest.find("</name>")?;
        let name = rest[..end].trim();
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    }

    pub fn parse_classpath_src(xml: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = xml;
        while let Some(idx) = rest.find("<classpathentry") {
            rest = &rest[idx..];
            let end = rest.find('>').unwrap_or(rest.len());
            let tag = &rest[..end];
            rest = &rest[end.min(rest.len())..];
            if !tag.contains("kind=\"src\"") && !tag.contains("kind='src'") {
                continue;
            }
            if let Some(p) = attr(tag, "path") {
                out.push(p);
            }
        }
        out
    }
}

fn attr(tag: &str, key: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let pat = format!("{key}={quote}");
        if let Some(start) = tag.find(&pat) {
            let rest = &tag[start + pat.len()..];
            if let Some(end) = rest.find(quote) {
                let v = &rest[..end];
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

impl WorkspaceSource for EclipseAdapter {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel> {
        let project = root.join(".project");
        let classpath = root.join(".classpath");
        if !project.is_file() && !classpath.is_file() {
            return None;
        }
        let name = std::fs::read_to_string(&project)
            .ok()
            .and_then(|s| Self::parse_project_name(&s))
            .unwrap_or_else(|| {
                root.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("eclipse")
                    .to_string()
            });
        let mut pkg = PackageEntry::new(name, root.to_path_buf());
        if let Ok(cp) = std::fs::read_to_string(&classpath) {
            for rel in Self::parse_classpath_src(&cp) {
                pkg = pkg.with_source_root(root.join(rel));
            }
        }
        if pkg.source_roots.is_empty() {
            pkg = pkg.with_source_root(root.join("src"));
        }
        let mut model = WorkspaceModel::new("eclipse", root.to_path_buf());
        model.add_package(pkg);
        Some(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_project_and_classpath() {
        assert_eq!(
            EclipseAdapter::parse_project_name("<projectDescription><name>demo</name></projectDescription>")
                .as_deref(),
            Some("demo")
        );
        assert_eq!(EclipseAdapter::parse_project_name("<name></name>"), None);
        assert_eq!(EclipseAdapter::parse_project_name("nope"), None);
        let cp = r#"
            <classpath>
              <classpathentry kind="src" path="src/main/java"/>
              <classpathentry kind="lib" path="lib/foo.jar"/>
              <classpathentry kind='src' path='src/test/java'/>
            </classpath>
        "#;
        assert_eq!(
            EclipseAdapter::parse_classpath_src(cp),
            vec!["src/main/java", "src/test/java"]
        );
        assert!(EclipseAdapter::parse_classpath_src("<classpath/>").is_empty());
    }

    #[test]
    fn detect_eclipse_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".project"),
            "<projectDescription><name>demo</name></projectDescription>\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src/main/java")).unwrap();
        std::fs::write(
            dir.path().join(".classpath"),
            r#"<classpath><classpathentry kind="src" path="src/main/java"/></classpath>"#,
        )
        .unwrap();
        let model = EclipseAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "eclipse");
        assert_eq!(model.packages[0].id.as_str(), "demo");
        assert!(model.packages[0].source_roots[0].ends_with("src/main/java"));
    }

    #[test]
    fn detect_none_without_dot_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(EclipseAdapter.detect(dir.path()).is_none());
    }

    #[test]
    fn detect_classpath_only_uses_directory_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join(".classpath"),
            r#"<classpath><classpathentry kind="src" path="src"/></classpath>"#,
        )
        .unwrap();
        let model = EclipseAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "eclipse");
        assert!(!model.packages[0].id.as_str().is_empty());
        assert!(model.packages[0].source_roots[0].ends_with("src"));
    }
}
