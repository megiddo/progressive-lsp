//! Composer adapter. Reads `composer.json` PSR-4. No host `php`.

use std::path::{Path, PathBuf};

use crate::model::{PackageEntry, WorkspaceModel, WorkspaceSource};

#[derive(Clone, Copy, Debug, Default)]
pub struct ComposerAdapter;

impl ComposerAdapter {
    pub fn parse_psr4(json: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let Some(autoload) = json.find("\"psr-4\"") else {
            return out;
        };
        let rest = &json[autoload..];
        let Some(brace) = rest.find('{') else {
            return out;
        };
        let body = &rest[brace + 1..];
        let end = body.find('}').unwrap_or(body.len());
        let map = &body[..end];
        for part in map.split(',') {
            let mut kv = part.splitn(2, ':');
            let key = kv
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .trim()
                .replace("\\\\", "\\");
            let val = kv
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(['"', ' ', ','])
                .replace("\\\\", "\\");
            if !key.is_empty() && !val.is_empty() {
                out.push((key, val));
            }
        }
        out
    }

    pub fn resolve_psr4(ns: &str, mappings: &[(String, String)]) -> Option<String> {
        let mut best: Option<(usize, String)> = None;
        for (prefix, dir) in mappings {
            if ns == prefix.trim_end_matches('\\') || ns.starts_with(prefix) {
                let rest = ns
                    .strip_prefix(prefix.trim_end_matches('\\'))
                    .unwrap_or("")
                    .trim_start_matches('\\')
                    .replace('\\', "/");
                let path = if rest.is_empty() {
                    dir.trim_end_matches('/').to_string()
                } else {
                    format!("{}/{}", dir.trim_end_matches('/'), rest)
                };
                if prefix.len() >= best.as_ref().map(|(n, _)| *n).unwrap_or(0) {
                    best = Some((prefix.len(), path));
                }
            }
        }
        best.map(|(_, p)| p)
    }
}

impl WorkspaceSource for ComposerAdapter {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel> {
        let composer = root.join("composer.json");
        if !composer.is_file() {
            return None;
        }
        let text = std::fs::read_to_string(&composer).ok()?;
        let mappings = Self::parse_psr4(&text);
        let mut model = WorkspaceModel::new("composer", root.to_path_buf());
        if mappings.is_empty() {
            let src = root.join("src");
            let mut pkg = PackageEntry::new(
                root.file_name().and_then(|s| s.to_str()).unwrap_or("php"),
                root.to_path_buf(),
            );
            pkg = pkg.with_source_root(src);
            if pkg.source_roots.is_empty() {
                pkg = pkg.with_source_root(root.to_path_buf());
            }
            model.add_package(pkg);
        } else {
            for (ns, dir) in mappings {
                let id = ns.trim_end_matches('\\').replace('\\', ".");
                let src = if Path::new(&dir).is_absolute() {
                    PathBuf::from(&dir)
                } else {
                    root.join(&dir)
                };
                let pkg = PackageEntry::new(id, src.clone()).with_source_root(src);
                model.add_package(pkg);
            }
        }
        if model.is_empty() {
            None
        } else {
            Some(model)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_psr4_and_resolve() {
        let json = r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/",
                    "Lib\\": "lib/"
                }
            }
        }"#;
        let maps = ComposerAdapter::parse_psr4(json);
        assert_eq!(maps.len(), 2);
        assert_eq!(
            ComposerAdapter::resolve_psr4("App\\Greeter", &maps).as_deref(),
            Some("src/Greeter")
        );
        assert_eq!(
            ComposerAdapter::resolve_psr4("Lib\\Hello", &maps).as_deref(),
            Some("lib/Hello")
        );
        assert!(ComposerAdapter::resolve_psr4("Nope\\X", &maps).is_none());
        assert!(ComposerAdapter::parse_psr4("{}").is_empty());
        assert!(ComposerAdapter::parse_psr4("\"psr-4\"").is_empty());
    }

    #[test]
    fn detect_composer_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let model = ComposerAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "composer");
        assert_eq!(model.packages[0].id.as_str(), "App");
        assert!(ComposerAdapter.detect(tempfile::tempdir().unwrap().path()).is_none());
    }

    #[test]
    fn detect_without_psr4_uses_src() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}\n").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let model = ComposerAdapter.detect(dir.path()).unwrap();
        assert!(!model.packages.is_empty());
    }
}
