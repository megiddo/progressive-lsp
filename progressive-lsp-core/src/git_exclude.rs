//! Worktree cache exclusion via `.git/info/exclude`. Never edits a committed `.gitignore`.

use std::path::{Path, PathBuf};

use crate::error::ConfigError;

/// Overlay directory name (no hyphen). Same as [`crate::PREFIX_DIR_NAME`].
pub const OVERLAY_DIR_NAME: &str = ".progressivelsp";

const BELT_GITIGNORE: &str = "cache/\nrun/\nlog/\n";

const GIT_EXCLUDE_LINES: &[&str] = &[
    ".progressivelsp/cache/",
    ".progressivelsp/run/",
    ".progressivelsp/log/",
];

pub fn belt_gitignore_body() -> &'static str {
    BELT_GITIGNORE
}

pub fn git_exclude_lines() -> &'static [&'static str] {
    GIT_EXCLUDE_LINES
}

/// Ensure overlay belt `.gitignore` and append worktree cache paths to `.git/info/exclude`.
///
/// Does **not** create or modify the project's committed `.gitignore`.
pub fn apply_worktree_excludes(workspace_root: &Path) -> Result<GitExcludeReport, ConfigError> {
    if workspace_root.as_os_str().is_empty() {
        return Err(ConfigError::Prefix("workspace path is empty".into()));
    }
    let overlay = workspace_root.join(OVERLAY_DIR_NAME);
    std::fs::create_dir_all(&overlay)
        .map_err(|e| ConfigError::Io(format!("mkdir {}: {e}", overlay.display())))?;

    let belt = overlay.join(".gitignore");
    if !belt.exists() {
        std::fs::write(&belt, BELT_GITIGNORE)
            .map_err(|e| ConfigError::Io(format!("write {}: {e}", belt.display())))?;
    }

    let project_gitignore = workspace_root.join(".gitignore");
    let gitignore_before = read_optional(&project_gitignore)?;

    let git_dir = workspace_root.join(".git");
    let exclude_path = if git_dir.is_dir() {
        Some(append_git_exclude(&git_dir, GIT_EXCLUDE_LINES)?)
    } else {
        None
    };

    let gitignore_after = read_optional(&project_gitignore)?;
    if gitignore_before != gitignore_after {
        return Err(ConfigError::Io(
            "refusing to modify the project's committed .gitignore".into(),
        ));
    }

    Ok(GitExcludeReport {
        overlay,
        belt_gitignore: belt,
        git_exclude: exclude_path,
        project_gitignore_unchanged: true,
        project_gitignore_bytes: gitignore_after,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitExcludeReport {
    pub overlay: PathBuf,
    pub belt_gitignore: PathBuf,
    pub git_exclude: Option<PathBuf>,
    pub project_gitignore_unchanged: bool,
    pub project_gitignore_bytes: Option<Vec<u8>>,
}

fn append_git_exclude(git_dir: &Path, lines: &[&str]) -> Result<PathBuf, ConfigError> {
    let info = git_dir.join("info");
    std::fs::create_dir_all(&info)
        .map_err(|e| ConfigError::Io(format!("mkdir {}: {e}", info.display())))?;
    let exclude = info.join("exclude");
    let mut existing = if exclude.exists() {
        std::fs::read_to_string(&exclude)
            .map_err(|e| ConfigError::Io(format!("read {}: {e}", exclude.display())))?
    } else {
        String::new()
    };
    for line in lines {
        if existing.lines().any(|l| l.trim() == *line) {
            continue;
        }
        if !existing.is_empty() && !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push_str(line);
        existing.push('\n');
    }
    std::fs::write(&exclude, existing)
        .map_err(|e| ConfigError::Io(format!("write {}: {e}", exclude.display())))?;
    Ok(exclude)
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, ConfigError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ConfigError::Io(format!("read {}: {e}", path.display()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git_init(root: &Path) {
        let status = Command::new("git")
            .args(["init"])
            .current_dir(root)
            .status()
            .expect("git init");
        assert!(status.success());
    }

    #[test]
    fn fixture_repo_excludes_via_info_exclude_not_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        git_init(dir.path());
        let marker = "# project committed gitignore — do not touch\n";
        std::fs::write(dir.path().join(".gitignore"), marker).unwrap();

        let report = apply_worktree_excludes(dir.path()).unwrap();
        assert!(report.project_gitignore_unchanged);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
            marker
        );
        assert_eq!(
            report.project_gitignore_bytes.as_deref(),
            Some(marker.as_bytes())
        );
        assert_eq!(
            std::fs::read_to_string(&report.belt_gitignore).unwrap(),
            BELT_GITIGNORE
        );
        let exclude = std::fs::read_to_string(report.git_exclude.as_ref().unwrap()).unwrap();
        for line in GIT_EXCLUDE_LINES {
            assert!(
                exclude.lines().any(|l| l.trim() == *line),
                "missing {line} in {exclude}"
            );
        }
        assert_eq!(belt_gitignore_body(), BELT_GITIGNORE);
        assert_eq!(git_exclude_lines(), GIT_EXCLUDE_LINES);
    }

    #[test]
    fn apply_is_idempotent_and_keeps_existing_exclude_lines() {
        let dir = tempfile::tempdir().unwrap();
        git_init(dir.path());
        let info = dir.path().join(".git/info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("exclude"), "keep-me\n.progressivelsp/cache/").unwrap();
        apply_worktree_excludes(dir.path()).unwrap();
        apply_worktree_excludes(dir.path()).unwrap();
        let exclude = std::fs::read_to_string(info.join("exclude")).unwrap();
        assert_eq!(exclude.matches("keep-me").count(), 1);
        assert_eq!(exclude.matches(".progressivelsp/cache/").count(), 1);
        assert_eq!(exclude.matches(".progressivelsp/run/").count(), 1);
        assert!(exclude.contains("keep-me\n"));
    }

    #[test]
    fn appends_newline_when_existing_exclude_has_no_trailing_nl() {
        let dir = tempfile::tempdir().unwrap();
        git_init(dir.path());
        let info = dir.path().join(".git/info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("exclude"), "keep-me").unwrap();
        apply_worktree_excludes(dir.path()).unwrap();
        let exclude = std::fs::read_to_string(info.join("exclude")).unwrap();
        assert!(exclude.starts_with("keep-me\n"));
    }

    #[test]
    fn no_git_dir_skips_exclude_and_still_writes_belt() {
        let dir = tempfile::tempdir().unwrap();
        let report = apply_worktree_excludes(dir.path()).unwrap();
        assert!(report.git_exclude.is_none());
        assert!(report.belt_gitignore.is_file());
        assert!(dir.path().join(".gitignore").exists() == false);
        assert_eq!(report.project_gitignore_bytes, None);
    }

    #[test]
    fn empty_workspace_is_error() {
        let err = apply_worktree_excludes(Path::new("")).unwrap_err();
        assert!(matches!(err, ConfigError::Prefix(_)));
    }

    #[test]
    fn does_not_overwrite_existing_belt_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let overlay = dir.path().join(OVERLAY_DIR_NAME);
        std::fs::create_dir_all(&overlay).unwrap();
        std::fs::write(overlay.join(".gitignore"), "custom\n").unwrap();
        apply_worktree_excludes(dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(overlay.join(".gitignore")).unwrap(),
            "custom\n"
        );
    }
}
