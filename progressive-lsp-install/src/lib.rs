//! Install crate: `ArtifactTransport`, `LocalFs`, `Installer`. No SSH types. No network.

pub mod hash;
pub mod manifest;
pub mod probe;
pub mod selector;
pub mod transport;

use std::path::{Path, PathBuf};

use progressive_lsp_core::InstallError;

pub use hash::{hex_encode, sha256, sha256_file, verify_hash};
pub use manifest::{Manifest, ManifestArtifact};
pub use probe::{BuildCensus, HostProbe};
pub use selector::{CensusSelector, ExplicitPacks, PackId, PackSelector};
pub use transport::{ArtifactTransport, FakeTransport, LocalFs};

/// Plan + apply. Hash fail → no rename to the final path.
#[derive(Clone, Debug)]
pub struct Installer<T> {
    transport: T,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallPlan {
    pub dest: PathBuf,
    pub tmp: PathBuf,
    pub expected_sha256: [u8; 32],
    pub bytes: Vec<u8>,
    pub executable: bool,
}

impl<T: ArtifactTransport> Installer<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn plan(
        &self,
        dest: impl Into<PathBuf>,
        bytes: Vec<u8>,
        expected_sha256: [u8; 32],
        executable: bool,
    ) -> Result<InstallPlan, InstallError> {
        let dest = dest.into();
        if dest.as_os_str().is_empty() {
            return Err(InstallError::Transport("destination path is empty".into()));
        }
        let file_name = dest
            .file_name()
            .ok_or_else(|| InstallError::Transport("destination has no file name".into()))?;
        let parent = dest.parent().unwrap_or_else(|| Path::new("."));
        let tmp = parent.join(format!(
            ".tmp-{}-{}",
            file_name.to_string_lossy(),
            hex_encode(&expected_sha256[..8])
        ));
        Ok(InstallPlan {
            dest,
            tmp,
            expected_sha256,
            bytes,
            executable,
        })
    }

    pub fn apply(&self, plan: &InstallPlan) -> Result<(), InstallError> {
        if let Some(parent) = plan.dest.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| InstallError::Io(format!("mkdir {}: {e}", parent.display())))?;
            }
        }
        self.transport.put(&plan.tmp, &plan.bytes)?;
        if plan.executable {
            self.transport.chmod_exec(&plan.tmp)?;
        }
        let actual = self.transport.read_hash(&plan.tmp)?;
        if actual != plan.expected_sha256 {
            let _ = std::fs::remove_file(&plan.tmp);
            return Err(InstallError::Hash {
                expected: hex_encode(&plan.expected_sha256),
                actual: hex_encode(&actual),
            });
        }
        self.transport.rename_atomic(&plan.tmp, &plan.dest)?;
        let after = self.transport.read_hash(&plan.dest)?;
        if after != plan.expected_sha256 {
            let _ = std::fs::remove_file(&plan.dest);
            return Err(InstallError::Hash {
                expected: hex_encode(&plan.expected_sha256),
                actual: hex_encode(&after),
            });
        }
        Ok(())
    }

    pub fn apply_manifest(
        &self,
        prefix: &Path,
        manifest: &Manifest,
        blobs: &[(String, Vec<u8>)],
    ) -> Result<(), InstallError> {
        for artifact in &manifest.artifacts {
            let bytes = blobs
                .iter()
                .find(|(name, _)| name == &artifact.name)
                .map(|(_, b)| b.clone())
                .ok_or_else(|| {
                    InstallError::Manifest(format!("missing blob for {}", artifact.name))
                })?;
            let expected = artifact.sha256_bytes()?;
            let dest = prefix.join(&artifact.rel_path);
            let plan = self.plan(dest, bytes, expected, artifact.executable)?;
            self.apply(&plan)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_mismatch_does_not_rename_to_final_path() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bin/progressive-lsp");
        let bytes = b"payload".to_vec();
        let wrong = sha256(b"other");
        let installer = Installer::new(LocalFs);
        let plan = installer.plan(&dest, bytes, wrong, true).unwrap();
        let err = installer.apply(&plan).unwrap_err();
        assert!(matches!(err, InstallError::Hash { .. }));
        assert!(!dest.exists(), "final path must not exist after hash fail");
        assert!(!plan.tmp.exists(), "tmp must be removed");
    }

    #[test]
    fn hash_mismatch_leaves_existing_final_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("artifact");
        std::fs::write(&dest, b"old").unwrap();
        let installer = Installer::new(LocalFs);
        let plan = installer
            .plan(&dest, b"new".to_vec(), sha256(b"nope"), false)
            .unwrap();
        let err = installer.apply(&plan).unwrap_err();
        assert!(matches!(err, InstallError::Hash { .. }));
        assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    }

    #[test]
    fn good_hash_renames_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bin/x");
        let bytes = b"hello".to_vec();
        let expected = sha256(&bytes);
        let installer = Installer::new(LocalFs);
        let plan = installer.plan(&dest, bytes.clone(), expected, true).unwrap();
        installer.apply(&plan).unwrap();
        assert!(dest.is_file(), "apply must place the final path");
        assert_eq!(std::fs::read(&dest).unwrap(), bytes);
        assert!(!plan.tmp.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dest).unwrap().permissions().mode();
            assert_ne!(mode & 0o111, 0, "executable bit");
        }
    }

    #[test]
    fn plan_rejects_empty_dest() {
        let installer = Installer::new(LocalFs);
        let err = installer
            .plan("", b"x".to_vec(), sha256(b"x"), false)
            .unwrap_err();
        assert!(matches!(err, InstallError::Transport(_)));
    }

    #[test]
    fn apply_manifest_places_named_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"core".to_vec();
        let manifest = Manifest {
            version: "1".into(),
            artifacts: vec![ManifestArtifact {
                name: "progressive-lsp".into(),
                rel_path: "bin/progressive-lsp".into(),
                sha256: hex_encode(&sha256(&bytes)),
                executable: true,
            }],
        };
        let installer = Installer::new(LocalFs);
        installer
            .apply_manifest(dir.path(), &manifest, &[("progressive-lsp".into(), bytes.clone())])
            .unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("bin/progressive-lsp")).unwrap(),
            bytes
        );
    }

    #[test]
    fn apply_manifest_missing_blob() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = Manifest {
            version: "1".into(),
            artifacts: vec![ManifestArtifact {
                name: "missing".into(),
                rel_path: "bin/x".into(),
                sha256: hex_encode(&sha256(b"z")),
                executable: false,
            }],
        };
        let err = Installer::new(LocalFs)
            .apply_manifest(dir.path(), &manifest, &[])
            .unwrap_err();
        assert!(matches!(err, InstallError::Manifest(_)));
    }

    #[test]
    fn fake_transport_hash_fail_skips_rename() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("final");
        let mut fake = FakeTransport::new();
        fake.corrupt_hash = true;
        let installer = Installer::new(fake);
        let bytes = b"abc".to_vec();
        let plan = installer
            .plan(&dest, bytes, sha256(b"abc"), false)
            .unwrap();
        let err = installer.apply(&plan).unwrap_err();
        assert!(matches!(err, InstallError::Hash { .. }));
        assert!(!dest.exists());
    }

    #[test]
    fn installer_exposes_transport() {
        let installer = Installer::new(LocalFs);
        let _ = installer.transport().probe().unwrap();
    }
}
