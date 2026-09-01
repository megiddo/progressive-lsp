//! Install crate: `ArtifactTransport`, `LocalFs`, `Installer`. No SSH types. No network.

pub mod dist_manifest;
pub mod hash;
pub mod manifest;
pub mod probe;
pub mod selector;
pub mod transport;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use progressive_lsp_core::{InstallError, LogComponent, LogPort, LogScope, NullLog};

pub use dist_manifest::{
    DistArtifact, DistManifest, DIST_PAYLOAD_STUB, DIST_PROTO, DIST_TRIPLES, MUSL_TRIPLES,
};
pub use hash::{hex_encode, sha256, sha256_file, verify_hash};
pub use manifest::{Manifest, ManifestArtifact};
pub use probe::{BuildCensus, HostProbe};
pub use selector::{CensusSelector, ExplicitPacks, PackId, PackSelector};
pub use transport::{ArtifactTransport, FakeRemoteTransport, FakeTransport, LocalFs};

/// Plan + apply. Hash fail → no rename to the final path.
#[derive(Clone)]
pub struct Installer<T> {
    transport: T,
    log: Arc<dyn LogPort>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for Installer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Installer")
            .field("transport", &self.transport)
            .finish()
    }
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
        Self {
            transport,
            log: Arc::new(NullLog),
        }
    }

    pub fn with_log(mut self, log: Arc<dyn LogPort>) -> Self {
        self.log = log;
        self
    }

    fn remove_or_emit(&self, path: &Path) {
        if let Err(e) = std::fs::remove_file(path) {
            let _g = LogScope::enter(
                LogScope::new()
                    .path(path.to_string_lossy().into_owned())
                    .operation("install")
                    .component(LogComponent::install()),
            );
            self.log
                .warn(&format!("remove_file {}: {e}", path.display()));
        }
    }

    fn emit_hash_mismatch(&self, expected: &str, actual: &str) {
        let _g = LogScope::enter(
            LogScope::new()
                .operation("install")
                .component(LogComponent::install()),
        );
        self.log.warn(&format!(
            "hash mismatch expected={expected} actual={actual}"
        ));
    }

    fn emit_verify_refused(&self, reason: &str) {
        let _g = LogScope::enter(
            LogScope::new()
                .operation("install")
                .component(LogComponent::install()),
        );
        self.log.warn(reason);
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
        self.apply_with_verify(plan, |_| Ok(()))
    }

    /// Hash tmp, run `verify` (e.g. `on_install_verify`), then rename.
    /// Hash mismatch or verify Err → no rename, tmp removed, no exec.
    pub fn apply_with_verify<F>(&self, plan: &InstallPlan, verify: F) -> Result<(), InstallError>
    where
        F: FnOnce(&InstallPlan) -> Result<(), InstallError>,
    {
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
            let expected = hex_encode(&plan.expected_sha256);
            let actual_hex = hex_encode(&actual);
            self.emit_hash_mismatch(&expected, &actual_hex);
            self.remove_or_emit(&plan.tmp);
            return Err(InstallError::Hash {
                expected,
                actual: actual_hex,
            });
        }
        if let Err(e) = verify(plan) {
            self.emit_verify_refused(&e.to_string());
            self.remove_or_emit(&plan.tmp);
            return Err(e);
        }
        self.transport.rename_atomic(&plan.tmp, &plan.dest)?;
        let after = self.transport.read_hash(&plan.dest)?;
        if after != plan.expected_sha256 {
            let expected = hex_encode(&plan.expected_sha256);
            let actual_hex = hex_encode(&after);
            self.emit_hash_mismatch(&expected, &actual_hex);
            self.remove_or_emit(&plan.dest);
            return Err(InstallError::Hash {
                expected,
                actual: actual_hex,
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
        let plan = installer
            .plan(&dest, bytes.clone(), expected, true)
            .unwrap();
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
            .apply_manifest(
                dir.path(),
                &manifest,
                &[("progressive-lsp".into(), bytes.clone())],
            )
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
        let plan = installer.plan(&dest, bytes, sha256(b"abc"), false).unwrap();
        let err = installer.apply(&plan).unwrap_err();
        assert!(matches!(err, InstallError::Hash { .. }));
        assert!(!dest.exists());
    }

    #[test]
    fn installer_exposes_transport() {
        let installer = Installer::new(LocalFs);
        let _ = installer.transport().probe().unwrap();
    }

    #[test]
    fn hash_mismatch_tmp_dir_remove_emits_log_scope_context_object() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("artifact");
        let log = progressive_lsp_core::FakeLog::new();
        let installer = Installer::new(FakeTransport {
            corrupt_hash: true,
            fail_put: false,
            put_as_dir: true,
            ..FakeTransport::default()
        })
        .with_log(std::sync::Arc::new(log.clone()));
        let plan = installer
            .plan(&dest, b"new".to_vec(), sha256(b"new"), false)
            .unwrap();
        let err = installer.apply(&plan).unwrap_err();
        assert!(matches!(err, InstallError::Hash { .. }));
        assert!(
            log.records()
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.operation.as_deref() == Some("install")
                    && r.message.contains("remove_file")),
            "{:?}",
            log.records()
        );
    }

    #[test]
    fn verify_abort_does_not_rename() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bin/new");
        let bytes = b"payload".to_vec();
        let expected = sha256(&bytes);
        let installer = Installer::new(LocalFs);
        let plan = installer.plan(&dest, bytes, expected, true).unwrap();
        let err = installer
            .apply_with_verify(&plan, |_| Err(InstallError::Refused("hook".into())))
            .unwrap_err();
        assert!(matches!(err, InstallError::Refused(_)));
        assert!(!dest.exists());
        assert!(!plan.tmp.exists());
    }

    #[test]
    fn fake_remote_hash_mismatch_skips_rename_and_atomic_replace() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("remote/final");
        let mut remote = FakeRemoteTransport::new();
        remote.corrupt_hash = true;
        let installer = Installer::new(remote);
        let bytes = b"abc".to_vec();
        let plan = installer.plan(&dest, bytes, sha256(b"abc"), true).unwrap();
        let err = installer.apply(&plan).unwrap_err();
        assert!(matches!(err, InstallError::Hash { .. }));
        assert!(!dest.exists());
        let ops = installer.transport().ops();
        assert!(ops.iter().any(|o| o.starts_with("put ")));
        assert!(ops.iter().any(|o| o.starts_with("hash ")));
        assert!(!ops.iter().any(|o| o.starts_with("rename ")));

        let dest2 = dir.path().join("remote/ok");
        std::fs::create_dir_all(dest2.parent().unwrap()).unwrap();
        std::fs::write(&dest2, b"old").unwrap();
        let remote2 = FakeRemoteTransport::new();
        let installer2 = Installer::new(remote2);
        let fresh = b"fresh".to_vec();
        let plan2 = installer2
            .plan(&dest2, fresh.clone(), sha256(&fresh), true)
            .unwrap();
        installer2.apply(&plan2).unwrap();
        assert_eq!(std::fs::read(&dest2).unwrap(), fresh);
        assert!(!plan2.tmp.exists());
        let ops2 = installer2.transport().ops();
        assert!(ops2.iter().any(|o| o.starts_with("put ")));
        assert!(ops2.iter().any(|o| o.starts_with("chmod ")));
        assert!(ops2.iter().any(|o| o.starts_with("rename ")));
        assert!(ops2.iter().any(|o| o.starts_with("hash ")));
    }

    fn install_warns(log: &progressive_lsp_core::FakeLog) -> Vec<progressive_lsp_core::LogRecord> {
        log.records()
            .into_iter()
            .filter(|r| r.operation.as_deref() == Some("install"))
            .collect()
    }

    #[test]
    fn hash_mismatch_emits_expected_actual_hex_before_remove() {
        const LEAK: &str = "LEAK_BLOB_BYTES";
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bin/progressive-lsp");
        let bytes = LEAK.as_bytes().to_vec();
        let expected = sha256(b"other");
        let actual = sha256(&bytes);
        let expected_hex = hex_encode(&expected);
        let actual_hex = hex_encode(&actual);
        let log = progressive_lsp_core::FakeLog::new();
        let installer = Installer::new(LocalFs).with_log(std::sync::Arc::new(log.clone()));
        let plan = installer.plan(&dest, bytes, expected, true).unwrap();
        let err = installer.apply(&plan).unwrap_err();
        match err {
            InstallError::Hash {
                expected: e,
                actual: a,
            } => {
                assert_eq!(e, expected_hex);
                assert_eq!(a, actual_hex);
            }
            other => panic!("{other:?}"),
        }
        let recs = install_warns(&log);
        let hash_row = recs
            .iter()
            .find(|r| {
                r.level == progressive_lsp_core::LogLevel::Warn
                    && r.message.contains("hash mismatch")
            })
            .expect(&format!("{recs:?}"));
        assert_eq!(
            hash_row.component.as_ref().map(|c| c.as_str()),
            Some("install")
        );
        let expected_at = hash_row.message.find(&expected_hex).expect("expected hex");
        let actual_at = hash_row.message.find(&actual_hex).expect("actual hex");
        assert!(
            expected_at < actual_at,
            "expected hex must precede actual: {}",
            hash_row.message
        );
        assert!(!hash_row.message.contains(LEAK));
        if let Some(ex) = &hash_row.extras {
            for v in ex.values() {
                assert!(!v.contains(LEAK), "{v}");
            }
        }
        assert!(!dest.exists());
        assert!(!plan.tmp.exists());
    }

    #[test]
    fn dest_hash_mismatch_emits_before_remove() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("artifact");
        let bytes = b"LEAK_BLOB_BYTES".to_vec();
        let expected = sha256(&bytes);
        let log = progressive_lsp_core::FakeLog::new();
        let installer = Installer::new(FakeTransport {
            corrupt_second_hash: true,
            ..FakeTransport::default()
        })
        .with_log(std::sync::Arc::new(log.clone()));
        let plan = installer.plan(&dest, bytes, expected, false).unwrap();
        let err = installer.apply(&plan).unwrap_err();
        let InstallError::Hash {
            expected: expected_hex,
            actual: actual_hex,
        } = err
        else {
            panic!("{err:?}");
        };
        assert_eq!(expected_hex, hex_encode(&expected));
        assert_eq!(actual_hex, hex_encode(&[0u8; 32]));
        let recs = install_warns(&log);
        assert!(
            recs.iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.message.contains("hash mismatch")
                    && r.message.contains(&expected_hex)
                    && r.message.contains(&actual_hex)
                    && r.message.find(&expected_hex).unwrap()
                        < r.message.find(&actual_hex).unwrap()),
            "{recs:?}"
        );
        assert!(!dest.exists());
        for r in &recs {
            assert!(!r.message.contains("LEAK_BLOB_BYTES"), "{}", r.message);
        }
    }

    #[test]
    fn verify_refuse_emits_before_remove() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bin/new");
        let bytes = b"LEAK_BLOB_BYTES".to_vec();
        let expected = sha256(&bytes);
        let log = progressive_lsp_core::FakeLog::new();
        let installer = Installer::new(LocalFs).with_log(std::sync::Arc::new(log.clone()));
        let plan = installer.plan(&dest, bytes, expected, true).unwrap();
        let err = installer
            .apply_with_verify(&plan, |_| Err(InstallError::Refused("hook".into())))
            .unwrap_err();
        assert!(matches!(err, InstallError::Refused(m) if m == "hook"));
        let recs = install_warns(&log);
        assert!(
            recs.iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.component.as_ref().map(|c| c.as_str()) == Some("install")
                    && r.message.contains("install verify refused")
                    && r.message.contains("hook")
                    && !r.message.contains("LEAK_BLOB_BYTES")),
            "{recs:?}"
        );
        assert!(!dest.exists());
        assert!(!plan.tmp.exists());
    }
}
