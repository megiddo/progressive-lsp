//! Directory adapter: source roots that already exist. No compiler.

use std::path::{Path, PathBuf};

use crate::model::{PackageEntry, WorkspaceModel, WorkspaceSource};

#[derive(Clone, Copy, Debug, Default)]
pub struct DirectoryAdapter;

impl DirectoryAdapter {
    pub fn collect_java_files(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        walk(root, &mut out, 0);
        out
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "node_modules" || name == ".git" || name == "target" || name == "build" {
            continue;
        }
        if path.is_dir() {
            walk(&path, out, depth + 1);
        } else if path.extension().and_then(|s| s.to_str()) == Some("java") {
            out.push(path);
        }
    }
}

fn infer_source_root(java_file: &Path) -> PathBuf {
    let mut cur = java_file.parent();
    while let Some(dir) = cur {
        if dir.file_name().and_then(|s| s.to_str()) == Some("java") {
            return dir.to_path_buf();
        }
        if dir.file_name().and_then(|s| s.to_str()) == Some("src") {
            return dir.to_path_buf();
        }
        cur = dir.parent();
    }
    java_file.parent().unwrap_or(java_file).to_path_buf()
}

impl WorkspaceSource for DirectoryAdapter {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel> {
        let files = Self::collect_java_files(root);
        if files.is_empty() {
            return None;
        }
        let mut model = WorkspaceModel::new("directory", root.to_path_buf());
        let mut pkg = PackageEntry::new(
            root.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("root"),
            root.to_path_buf(),
        );
        let mut roots = Vec::new();
        for f in &files {
            let sr = infer_source_root(f);
            if !roots.contains(&sr) {
                roots.push(sr);
            }
        }
        for sr in roots {
            pkg = pkg.with_source_root(sr);
        }
        model.add_package(pkg);
        Some(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_src_main_java_and_skips_vendor() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src/main/java/com/example");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("A.java"), "package com.example; class A {}").unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/x")).unwrap();
        std::fs::write(dir.path().join("node_modules/x/B.java"), "class B {}").unwrap();
        let model = DirectoryAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "directory");
        assert_eq!(model.packages.len(), 1);
        let roots = &model.packages[0].source_roots;
        assert!(roots.iter().any(|r| r.ends_with("src/main/java") || r.ends_with("java")));
        assert_eq!(DirectoryAdapter::collect_java_files(dir.path()).len(), 1);
    }

    #[test]
    fn none_without_java() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README"), "x").unwrap();
        assert!(DirectoryAdapter.detect(dir.path()).is_none());
    }

    #[test]
    fn skips_git_target_build_and_finds_src_root() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src/com/example");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("A.java"), "class A {}").unwrap();
        for skip in [".git", "target", "build"] {
            let nested = dir.path().join(skip);
            std::fs::create_dir_all(&nested).unwrap();
            std::fs::write(nested.join("Skip.java"), "class Skip {}").unwrap();
        }
        let files = DirectoryAdapter::collect_java_files(dir.path());
        assert_eq!(files.len(), 1);
        let model = DirectoryAdapter.detect(dir.path()).unwrap();
        assert!(model.packages[0].source_roots.iter().any(|r| r.ends_with("src")));
    }

    #[test]
    fn walk_includes_depth_eight_but_not_nine() {
        let dir = tempfile::tempdir().unwrap();
        let mut d8 = dir.path().to_path_buf();
        for i in 0..8 {
            d8.push(format!("d{i}"));
        }
        std::fs::create_dir_all(&d8).unwrap();
        std::fs::write(d8.join("Deep.java"), "class Deep {}").unwrap();
        let mut d9 = d8.clone();
        d9.push("d8");
        std::fs::create_dir_all(&d9).unwrap();
        std::fs::write(d9.join("TooDeep.java"), "class TooDeep {}").unwrap();
        let files = DirectoryAdapter::collect_java_files(dir.path());
        assert!(
            files.iter().any(|p| p.ends_with("Deep.java")),
            "depth 8 must still be walked: {files:?}"
        );
        assert!(
            !files.iter().any(|p| p.ends_with("TooDeep.java")),
            "depth 9 must be skipped: {files:?}"
        );
    }
}
