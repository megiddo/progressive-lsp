//! Production pack adapter: discover under `$PREFIX/engines/`. Stub bytes never exec.

use progressive_lsp_core::{EngineError, LanguageId, PrefixLayout};

use crate::adapter::{ChildHandle, EngineAdapter, EngineBinary, ReadyKind, SpawnCtx};
use crate::discovery::{discover_pack_opt, is_pack_stub, PYTHON_PACK, RUST_PACK};

pub struct PackAdapter {
    pack_name: String,
    language: LanguageId,
}

impl PackAdapter {
    pub fn python() -> Self {
        Self {
            pack_name: PYTHON_PACK.into(),
            language: LanguageId::new("python"),
        }
    }

    pub fn rust() -> Self {
        Self {
            pack_name: RUST_PACK.into(),
            language: LanguageId::new("rust"),
        }
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
        let bytes = std::fs::read(&ctx.binary.path)
            .map_err(|e| EngineError::Spawn(format!("read {}: {e}", ctx.binary.path.display())))?;
        if is_pack_stub(&bytes) {
            return Err(EngineError::Spawn(
                "stub pack; real ty/rust-analyzer musl ELF is Linux CI / Docker".into(),
            ));
        }
        Err(EngineError::Spawn(
            "host process spawn of engine packs is reserved for Linux CI / Docker".into(),
        ))
    }

    fn ready_signal(&self) -> ReadyKind {
        ReadyKind::Initialize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{hex_of, stub_pack_bytes, TY_BINARY};
    use progressive_lsp_install::{Manifest, ManifestArtifact};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

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
                binary: bin,
            })
            .unwrap_err();
        assert!(err.to_string().contains("stub pack"));
        assert_eq!(a.ready_signal(), ReadyKind::Initialize);
        let rust = PackAdapter::rust();
        assert_eq!(rust.pack_name(), "rust");
        assert!(rust.discover(&prefix).is_none());
    }
}
