//! `ArtifactTransport` strategies. `LocalFs` only — no SSH types, no network.

use std::path::Path;
use std::sync::{Arc, Mutex};

use progressive_lsp_core::InstallError;

use crate::hash::sha256_file;
use crate::probe::{BuildCensus, HostProbe};

pub trait ArtifactTransport {
    fn put(&self, dest: &Path, bytes: &[u8]) -> Result<(), InstallError>;
    fn chmod_exec(&self, path: &Path) -> Result<(), InstallError>;
    fn rename_atomic(&self, from: &Path, to: &Path) -> Result<(), InstallError>;
    fn read_hash(&self, path: &Path) -> Result<[u8; 32], InstallError>;
    fn probe(&self) -> Result<HostProbe, InstallError>;
}

/// In-tree transport. Filesystem only.
#[derive(Clone, Debug, Default)]
pub struct LocalFs;

impl ArtifactTransport for LocalFs {
    fn put(&self, dest: &Path, bytes: &[u8]) -> Result<(), InstallError> {
        if dest.as_os_str().is_empty() {
            return Err(InstallError::Transport("put destination is empty".into()));
        }
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| InstallError::Io(format!("mkdir {}: {e}", parent.display())))?;
            }
        }
        std::fs::write(dest, bytes)
            .map_err(|e| InstallError::Io(format!("write {}: {e}", dest.display())))
    }

    fn chmod_exec(&self, path: &Path) -> Result<(), InstallError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(path)
                .map_err(|e| InstallError::Io(format!("stat {}: {e}", path.display())))?;
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o111);
            std::fs::set_permissions(path, perms)
                .map_err(|e| InstallError::Io(format!("chmod {}: {e}", path.display())))?;
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
        Ok(())
    }

    fn rename_atomic(&self, from: &Path, to: &Path) -> Result<(), InstallError> {
        if from == to {
            return Err(InstallError::Transport(
                "rename source and destination are the same path".into(),
            ));
        }
        std::fs::rename(from, to).map_err(|e| {
            InstallError::Io(format!(
                "rename {} -> {}: {e}",
                from.display(),
                to.display()
            ))
        })
    }

    fn read_hash(&self, path: &Path) -> Result<[u8; 32], InstallError> {
        sha256_file(path)
    }

    fn probe(&self) -> Result<HostProbe, InstallError> {
        Ok(HostProbe::current(BuildCensus::default()))
    }
}

/// Test double. Same trait as production. Optional hash corruption.
#[derive(Clone, Debug, Default)]
pub struct FakeTransport {
    pub corrupt_hash: bool,
    pub fail_put: bool,
    /// `put` creates a directory so later `remove_file` fails (Context Object emit).
    pub put_as_dir: bool,
}

impl FakeTransport {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ArtifactTransport for FakeTransport {
    fn put(&self, dest: &Path, bytes: &[u8]) -> Result<(), InstallError> {
        if self.fail_put {
            return Err(InstallError::Transport("fake put failed".into()));
        }
        if dest.as_os_str().is_empty() {
            return Err(InstallError::Transport("empty dest".into()));
        }
        if self.put_as_dir {
            std::fs::create_dir_all(dest)
                .map_err(|e| InstallError::Io(format!("fake mkdir {}: {e}", dest.display())))?;
            return Ok(());
        }
        std::fs::write(dest, bytes)
            .map_err(|e| InstallError::Io(format!("fake write {}: {e}", dest.display())))
    }

    fn chmod_exec(&self, _path: &Path) -> Result<(), InstallError> {
        Ok(())
    }

    fn rename_atomic(&self, from: &Path, to: &Path) -> Result<(), InstallError> {
        std::fs::rename(from, to).map_err(|e| InstallError::Io(e.to_string()))
    }

    fn read_hash(&self, path: &Path) -> Result<[u8; 32], InstallError> {
        if self.corrupt_hash {
            return Ok([0u8; 32]);
        }
        sha256_file(path)
    }

    fn probe(&self) -> Result<HostProbe, InstallError> {
        Ok(HostProbe::current(BuildCensus::default()))
    }
}

/// Test double that looks like remote put/chmod/rename/hash.
/// Same [`ArtifactTransport`] trait. No SSH types, no network.
#[derive(Clone, Debug, Default)]
pub struct FakeRemoteTransport {
    pub corrupt_hash: bool,
    pub fail_put: bool,
    pub fail_chmod: bool,
    pub fail_rename: bool,
    ops: Arc<Mutex<Vec<String>>>,
}

impl FakeRemoteTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ops(&self) -> Vec<String> {
        self.ops.lock().expect("ops").clone()
    }

    fn log(&self, op: impl Into<String>) {
        self.ops.lock().expect("ops").push(op.into());
    }
}

impl ArtifactTransport for FakeRemoteTransport {
    fn put(&self, dest: &Path, bytes: &[u8]) -> Result<(), InstallError> {
        self.log(format!("put {}", dest.display()));
        if self.fail_put {
            return Err(InstallError::Transport("remote put failed".into()));
        }
        if dest.as_os_str().is_empty() {
            return Err(InstallError::Transport("empty dest".into()));
        }
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    InstallError::Io(format!("remote mkdir {}: {e}", parent.display()))
                })?;
            }
        }
        std::fs::write(dest, bytes)
            .map_err(|e| InstallError::Io(format!("remote write {}: {e}", dest.display())))
    }

    fn chmod_exec(&self, path: &Path) -> Result<(), InstallError> {
        self.log(format!("chmod {}", path.display()));
        if self.fail_chmod {
            return Err(InstallError::Transport("remote chmod failed".into()));
        }
        Ok(())
    }

    fn rename_atomic(&self, from: &Path, to: &Path) -> Result<(), InstallError> {
        self.log(format!("rename {} -> {}", from.display(), to.display()));
        if self.fail_rename {
            return Err(InstallError::Transport("remote rename failed".into()));
        }
        if let Some(parent) = to.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    InstallError::Io(format!("remote mkdir {}: {e}", parent.display()))
                })?;
            }
        }
        std::fs::rename(from, to).map_err(|e| InstallError::Io(e.to_string()))
    }

    fn read_hash(&self, path: &Path) -> Result<[u8; 32], InstallError> {
        self.log(format!("hash {}", path.display()));
        if self.corrupt_hash {
            return Ok([0u8; 32]);
        }
        sha256_file(path)
    }

    fn probe(&self) -> Result<HostProbe, InstallError> {
        Ok(HostProbe::current(BuildCensus::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::sha256;

    #[test]
    fn local_fs_put_hash_rename_probe() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("nested/file");
        let fs = LocalFs;
        fs.put(&dest, b"abc").unwrap();
        fs.chmod_exec(&dest).unwrap();
        assert_eq!(fs.read_hash(&dest).unwrap(), sha256(b"abc"));
        let dest2 = dir.path().join("nested/file2");
        fs.rename_atomic(&dest, &dest2).unwrap();
        assert!(!dest.exists());
        assert_eq!(std::fs::read(&dest2).unwrap(), b"abc");
        let probe = fs.probe().unwrap();
        assert_eq!(probe.os, std::env::consts::OS);
        assert!(fs.put(Path::new(""), b"x").is_err());
        assert!(fs.rename_atomic(&dest2, &dest2).is_err());
    }

    #[test]
    fn fake_transport_can_fail_put() {
        let mut t = FakeTransport::new();
        t.fail_put = true;
        assert!(matches!(
            t.put(Path::new("x"), b"y"),
            Err(InstallError::Transport(_))
        ));
        t.fail_put = false;
        assert!(t.put(Path::new(""), b"y").is_err());
        let probe = t.probe().unwrap();
        assert_eq!(probe.os, std::env::consts::OS);
        assert!(!probe.arch.is_empty());
        t.chmod_exec(Path::new("x")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::write(&a, b"abc").unwrap();
        t.rename_atomic(&a, &b).unwrap();
        assert!(!a.exists());
        assert_eq!(std::fs::read(&b).unwrap(), b"abc");
        assert_eq!(t.read_hash(&b).unwrap(), sha256(b"abc"));
        t.corrupt_hash = true;
        assert_eq!(t.read_hash(&b).unwrap(), [0u8; 32]);
    }

    #[test]
    fn fake_remote_logs_put_chmod_rename_hash() {
        let t = FakeRemoteTransport::new();
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let dest = dir.path().join("nested/b");
        t.put(&a, b"xyz").unwrap();
        t.chmod_exec(&a).unwrap();
        assert_eq!(t.read_hash(&a).unwrap(), sha256(b"xyz"));
        t.rename_atomic(&a, &dest).unwrap();
        assert!(!a.exists());
        assert_eq!(std::fs::read(&dest).unwrap(), b"xyz");
        let ops = t.ops();
        assert!(ops.iter().any(|o| o.starts_with("put ")));
        assert!(ops.iter().any(|o| o.starts_with("chmod ")));
        assert!(ops.iter().any(|o| o.starts_with("rename ")));
        assert!(ops.iter().any(|o| o.starts_with("hash ")));
        assert_eq!(t.probe().unwrap().os, std::env::consts::OS);
        let mut fail = FakeRemoteTransport::new();
        fail.fail_put = true;
        assert!(fail.put(Path::new("x"), b"y").is_err());
        fail.fail_put = false;
        assert!(fail.put(Path::new(""), b"y").is_err());
        fail.fail_chmod = true;
        assert!(fail.chmod_exec(&dest).is_err());
        fail.fail_chmod = false;
        fail.fail_rename = true;
        assert!(fail.rename_atomic(&dest, &a).is_err());
        fail.corrupt_hash = true;
        assert_eq!(fail.read_hash(&dest).unwrap(), [0u8; 32]);
    }
}
