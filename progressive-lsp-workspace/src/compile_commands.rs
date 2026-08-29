//! `compile_commands.json` WorkspaceSource. One-shot cmake argv only if CMakeLists.txt exists.

use std::path::{Path, PathBuf};

use crate::model::{PackageEntry, WorkspaceModel, WorkspaceSource};

#[derive(Clone, Copy, Debug, Default)]
pub struct CompileCommandsAdapter;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileCommand {
    pub directory: PathBuf,
    pub file: PathBuf,
}

impl CompileCommandsAdapter {
    /// True only when the project already uses CMake and has no compile_commands.json yet.
    /// Never invents CMakeLists.txt.
    pub fn should_one_shot_cmake(root: &Path) -> bool {
        root.join("CMakeLists.txt").is_file() && !root.join("compile_commands.json").is_file()
    }

    pub fn cmake_export_argv(root: &Path) -> Option<Vec<String>> {
        if !Self::should_one_shot_cmake(root) {
            return None;
        }
        Some(vec![
            "cmake".into(),
            "-DCMAKE_EXPORT_COMPILE_COMMANDS=ON".into(),
            "-S".into(),
            root.display().to_string(),
            "-B".into(),
            root.join("build").display().to_string(),
        ])
    }

    pub fn parse(json: &str, root: &Path) -> Vec<CompileCommand> {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(json) else {
            return Vec::new();
        };
        let Some(arr) = val.as_array() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for item in arr {
            let file = item
                .get("file")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if file.is_empty() {
                continue;
            }
            let dir = item
                .get("directory")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .unwrap_or_else(|| root.to_path_buf());
            let path = if Path::new(file).is_absolute() {
                PathBuf::from(file)
            } else {
                dir.join(file)
            };
            out.push(CompileCommand {
                directory: dir,
                file: path,
            });
        }
        out
    }
}

impl WorkspaceSource for CompileCommandsAdapter {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel> {
        let cc = root.join("compile_commands.json");
        if !cc.is_file() {
            return None;
        }
        let text = std::fs::read_to_string(&cc).ok()?;
        let cmds = Self::parse(&text, root);
        let mut model = WorkspaceModel::new("compile_commands", root.to_path_buf());
        let id = root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("cc")
            .to_string();
        let mut pkg = PackageEntry::new(id, root.to_path_buf());
        let mut dirs: Vec<PathBuf> = cmds
            .iter()
            .filter_map(|c| c.file.parent().map(Path::to_path_buf))
            .collect();
        dirs.sort();
        dirs.dedup();
        for d in dirs {
            pkg = pkg.with_source_root(d);
        }
        if pkg.source_roots.is_empty() {
            pkg = pkg.with_source_root(root.to_path_buf());
        }
        model.add_package(pkg);
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
    fn parse_entries_and_detect() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("greet.c"), "int greet(void) { return 1; }\n").unwrap();
        let json = format!(
            r#"[{{"directory":"{}","file":"src/greet.c","command":"cc -c src/greet.c"}}]"#,
            dir.path().display()
        );
        let cmds = CompileCommandsAdapter::parse(&json, dir.path());
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].file.ends_with("greet.c"));
        assert!(CompileCommandsAdapter::parse("{}", dir.path()).is_empty());
        assert!(CompileCommandsAdapter::parse("not-json", dir.path()).is_empty());
        assert!(CompileCommandsAdapter::parse(r#"[{"file":""}]"#, dir.path()).is_empty());
        std::fs::write(dir.path().join("compile_commands.json"), &json).unwrap();
        let model = CompileCommandsAdapter.detect(dir.path()).unwrap();
        assert_eq!(model.kind, "compile_commands");
        assert!(!model.packages[0].source_roots.is_empty());
        assert!(CompileCommandsAdapter
            .detect(tempfile::tempdir().unwrap().path())
            .is_none());
        let abs = format!(
            r#"[{{"directory":"{}","file":"{}"}}]"#,
            dir.path().display(),
            src.join("greet.c").display()
        );
        let abs_cmds = CompileCommandsAdapter::parse(&abs, dir.path());
        assert_eq!(abs_cmds[0].file, src.join("greet.c"));
    }

    #[test]
    fn cmake_one_shot_only_when_project_already_uses_cmake() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!CompileCommandsAdapter::should_one_shot_cmake(dir.path()));
        assert!(CompileCommandsAdapter::cmake_export_argv(dir.path()).is_none());
        std::fs::write(dir.path().join("CMakeLists.txt"), "project(demo)\n").unwrap();
        assert!(CompileCommandsAdapter::should_one_shot_cmake(dir.path()));
        let argv = CompileCommandsAdapter::cmake_export_argv(dir.path()).unwrap();
        assert!(argv.contains(&"-DCMAKE_EXPORT_COMPILE_COMMANDS=ON".into()));
        assert!(!dir.path().join("compile_commands.json").is_file());
        std::fs::write(dir.path().join("compile_commands.json"), "[]\n").unwrap();
        assert!(!CompileCommandsAdapter::should_one_shot_cmake(dir.path()));
        assert!(CompileCommandsAdapter::cmake_export_argv(dir.path()).is_none());
    }
}
