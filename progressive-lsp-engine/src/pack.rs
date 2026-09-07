//! Production pack adapter: discover under `$PREFIX/engines/`.
//! Stub bytes never exec. Darwin / non-Linux refuse. Linux uses `Command`.

use std::sync::Arc;

use progressive_lsp_core::{EngineError, LanguageId, PrefixLayout};

use crate::adapter::{
    ChildHandle, CommandSpawnPort, EngineAdapter, EngineBinary, ReadyKind, SpawnCtx, SpawnPlan,
    SpawnPort,
};
use crate::discovery::{
    discover_pack_opt, is_pack_stub, BIOME_PACK, CLANGD_PACK, GOPLS_PACK, JAVA_PACK, PHPANTOM_PACK,
    PYTHON_PACK, RUST_PACK, SUPERHTML_PACK, TSGO_PACK, ZLS_PACK,
};

const STUB_REFUSE: &str = "stub pack; real engine musl ELF is Linux CI / Docker";

pub struct PackAdapter {
    pack_name: String,
    language: LanguageId,
    spawn_port: Option<Arc<dyn SpawnPort>>,
}

impl PackAdapter {
    pub fn new(pack_name: impl Into<String>, language: impl Into<LanguageId>) -> Self {
        Self {
            pack_name: pack_name.into(),
            language: language.into(),
            spawn_port: None,
        }
    }

    /// Tests inject a [`SpawnPort`] so Darwin can assert “would have spawned”.
    pub fn with_spawn_port(mut self, port: Arc<dyn SpawnPort>) -> Self {
        self.spawn_port = Some(port);
        self
    }

    /// Testable Linux spawn plan. Stub bytes still refuse before a plan exists.
    pub fn spawn_plan(&self, ctx: &SpawnCtx) -> Result<SpawnPlan, EngineError> {
        let bytes = std::fs::read(&ctx.binary.path)
            .map_err(|e| EngineError::Spawn(format!("read {}: {e}", ctx.binary.path.display())))?;
        if is_pack_stub(&bytes) {
            return Err(EngineError::Spawn(STUB_REFUSE.into()));
        }
        Ok(SpawnPlan::from_spawn_ctx(ctx))
    }

    pub fn python() -> Self {
        Self::new(PYTHON_PACK, LanguageId::new("python"))
    }

    pub fn rust() -> Self {
        Self::new(RUST_PACK, LanguageId::new("rust"))
    }

    pub fn clangd() -> Self {
        Self::new(CLANGD_PACK, LanguageId::new("c"))
    }

    pub fn tsgo() -> Self {
        Self::new(TSGO_PACK, LanguageId::new("typescript"))
    }

    pub fn phpantom() -> Self {
        Self::new(PHPANTOM_PACK, LanguageId::new("php"))
    }

    pub fn superhtml() -> Self {
        Self::new(SUPERHTML_PACK, LanguageId::new("html"))
    }

    pub fn biome() -> Self {
        Self::new(BIOME_PACK, LanguageId::new("css"))
    }

    pub fn gopls() -> Self {
        Self::new(GOPLS_PACK, LanguageId::new("go"))
    }

    pub fn zls() -> Self {
        Self::new(ZLS_PACK, LanguageId::new("zig"))
    }

    pub fn java() -> Self {
        Self::new(JAVA_PACK, LanguageId::new("java"))
    }
}

impl EngineAdapter for PackAdapter {
    fn pack_name(&self) -> &str {
        &self.pack_name
    }

    fn language_id(&self) -> LanguageId {
        self.language.clone()
    }

    fn discover(&self, prefix: &PrefixLayout) -> Option<EngineBinary> {
        discover_pack_opt(prefix, &self.pack_name)
    }

    fn spawn(&self, ctx: SpawnCtx) -> Result<ChildHandle, EngineError> {
        let plan = self.spawn_plan(&ctx)?;
        if let Some(port) = &self.spawn_port {
            return port.spawn_child(&plan, &self.pack_name);
        }
        CommandSpawnPort.spawn_child(&plan, &self.pack_name)
    }

    fn ready_signal(&self) -> ReadyKind {
        ReadyKind::Initialize
    }

    fn extra_languages(&self) -> Vec<LanguageId> {
        match self.pack_name.as_str() {
            CLANGD_PACK => vec![LanguageId::new("cpp")],
            TSGO_PACK => vec![LanguageId::new("javascript")],
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{RecordingSpawnPort, SpawnCtx};
    use crate::discovery::{hex_of, is_pack_stub, stub_pack_bytes, TY_BINARY};
    use progressive_lsp_install::{Manifest, ManifestArtifact};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn pack_adapter_discovers_stub_and_refuses_exec() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(dir.path());
        prefix.ensure_dirs().unwrap();
        let bytes = stub_pack_bytes(PYTHON_PACK, TY_BINARY);
        let d = prefix.engines_dir().join("python");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(TY_BINARY), &bytes).unwrap();
        let m = Manifest {
            version: "1".into(),
            artifacts: vec![ManifestArtifact {
                name: TY_BINARY.into(),
                rel_path: TY_BINARY.into(),
                sha256: hex_of(&bytes),
                executable: true,
            }],
        };
        std::fs::write(d.join("manifest.json"), m.to_json().unwrap()).unwrap();
        let a = PackAdapter::python();
        assert_eq!(a.pack_name(), "python");
        assert_eq!(a.language_id().as_str(), "python");
        let bin = a.discover(&prefix).unwrap();
        let err = a
            .spawn(SpawnCtx {
                workspace: PathBuf::from("/w"),
                language: LanguageId::new("python"),
                package: progressive_lsp_core::PackageId::new("p"),
                argv: Vec::new(),
                cwd: PathBuf::from("/w"),
                env: BTreeMap::new(),
                binary: bin.clone(),
            })
            .unwrap_err();
        assert!(err.to_string().contains("stub pack"));
        assert_eq!(a.ready_signal(), ReadyKind::Initialize);
        let rust = PackAdapter::rust();
        assert_eq!(rust.pack_name(), "rust");
        assert!(rust.discover(&prefix).is_none());
        assert_eq!(PackAdapter::clangd().language_id().as_str(), "c");
        assert_eq!(
            PackAdapter::clangd().extra_languages(),
            vec![LanguageId::new("cpp")]
        );
        assert_eq!(PackAdapter::tsgo().pack_name(), "tsgo");
        assert_eq!(
            PackAdapter::tsgo().extra_languages(),
            vec![LanguageId::new("javascript")]
        );
        assert_eq!(PackAdapter::phpantom().language_id().as_str(), "php");
        assert_eq!(PackAdapter::superhtml().language_id().as_str(), "html");
        assert_eq!(PackAdapter::biome().language_id().as_str(), "css");
        assert_eq!(PackAdapter::gopls().language_id().as_str(), "go");
        assert_eq!(PackAdapter::zls().language_id().as_str(), "zig");
        assert_eq!(PackAdapter::java().language_id().as_str(), "java");
        assert_eq!(PackAdapter::java().pack_name(), JAVA_PACK);
        assert!(PackAdapter::new("phpantom", LanguageId::new("php"))
            .extra_languages()
            .is_empty());
        let io = crate::adapter::ChildIo::lsp_with_stderr_pipe();
        assert!(io.has_stderr_pipe());
        assert!(io.stdout_is_never_log_adapter());
        assert!(a
            .spawn_plan(&SpawnCtx {
                workspace: PathBuf::from("/w"),
                language: LanguageId::new("python"),
                package: progressive_lsp_core::PackageId::new("p"),
                argv: Vec::new(),
                cwd: PathBuf::from("/w"),
                env: BTreeMap::new(),
                binary: bin.clone(),
            })
            .unwrap_err()
            .to_string()
            .contains("stub pack"));
        let missing = a
            .spawn(SpawnCtx {
                workspace: PathBuf::from("/w"),
                language: LanguageId::new("python"),
                package: progressive_lsp_core::PackageId::new("p"),
                argv: Vec::new(),
                cwd: PathBuf::from("/w"),
                env: BTreeMap::new(),
                binary: crate::adapter::EngineBinary {
                    pack_name: "python".into(),
                    path: PathBuf::from("/missing-pack-bytes"),
                    sha256: [0; 32],
                },
            })
            .unwrap_err();
        assert!(missing.to_string().contains("read"), "{missing}");
    }

    fn write_fixture_pack(prefix: &PrefixLayout, bytes: &[u8]) -> crate::adapter::EngineBinary {
        let d = prefix.engines_dir().join("python");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(TY_BINARY), bytes).unwrap();
        let m = Manifest {
            version: "1".into(),
            artifacts: vec![ManifestArtifact {
                name: TY_BINARY.into(),
                rel_path: TY_BINARY.into(),
                sha256: hex_of(bytes),
                executable: true,
            }],
        };
        std::fs::write(d.join("manifest.json"), m.to_json().unwrap()).unwrap();
        PackAdapter::python().discover(prefix).unwrap()
    }

    fn fixture_ctx(bin: crate::adapter::EngineBinary) -> SpawnCtx {
        SpawnCtx {
            workspace: PathBuf::from("/w"),
            language: LanguageId::new("python"),
            package: progressive_lsp_core::PackageId::new("p"),
            argv: vec![bin.path.display().to_string(), "--stdio".into()],
            cwd: PathBuf::from("/w"),
            env: BTreeMap::from([("TY_LOG".into(), "info".into())]),
            binary: bin,
        }
    }

    #[test]
    fn spawn_plan_value_object_covers_linux_command_without_exec() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(dir.path());
        prefix.ensure_dirs().unwrap();
        let bytes = b"fixture-pack-bytes-not-a-stub";
        assert!(!is_pack_stub(bytes));
        let bin = write_fixture_pack(&prefix, bytes);
        let a = PackAdapter::python();
        let ctx = fixture_ctx(bin);
        let plan = a.spawn_plan(&ctx).expect("non-stub fixture yields a plan");
        assert_eq!(plan.program(), ctx.binary.path.as_path());
        assert_eq!(plan.argv(), ctx.argv.as_slice());
        assert_eq!(plan.command_args(), &["--stdio"]);
        assert_eq!(plan.cwd(), ctx.cwd.as_path());
        assert_eq!(plan.env(), &ctx.env);
        assert!(plan.io().has_stderr_pipe());
        assert!(plan.io().stdout_is_never_log_adapter());
        let err = a.spawn(ctx.clone()).unwrap_err();
        if cfg!(target_os = "linux") {
            assert!(
                err.to_string().contains("command") || err.to_string().contains("Spawn"),
                "Linux fixture bytes must fail closed, not look like a live musl child: {err}"
            );
        } else {
            assert!(
                err.to_string().contains("not this OS"),
                "Darwin must refuse exec of non-stub bytes: {err}"
            );
        }
        let port = Arc::new(RecordingSpawnPort::new());
        let injected =
            PackAdapter::python().with_spawn_port(Arc::clone(&port) as Arc<dyn SpawnPort>);
        let handle = injected
            .spawn(ctx)
            .expect("RecordingSpawnPort is would-have-spawned, not Command");
        assert_eq!(port.last_plan().as_ref(), Some(&plan));
        assert!(handle.io().has_stderr_pipe());
        assert!(handle.io().stdout_is_never_log_adapter());
        assert!(!handle.has_os_stderr());
    }
}
