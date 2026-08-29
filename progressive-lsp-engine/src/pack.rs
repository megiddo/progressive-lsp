//! Production pack adapter: discover under `$PREFIX/engines/`. Stub bytes never exec.

use progressive_lsp_core::{EngineError, LanguageId, PrefixLayout};

use crate::adapter::{ChildHandle, EngineAdapter, EngineBinary, ReadyKind, SpawnCtx};
use crate::discovery::{
    discover_pack_opt, is_pack_stub, BIOME_PACK, CLANGD_PACK, GOPLS_PACK, PHPANTOM_PACK,
    PYTHON_PACK, RUST_PACK, SUPERHTML_PACK, TSGO_PACK, ZLS_PACK,
};

pub struct PackAdapter {
    pack_name: String,
    language: LanguageId,
}

impl PackAdapter {
    pub fn new(pack_name: impl Into<String>, language: impl Into<LanguageId>) -> Self {
        Self {
            pack_name: pack_name.into(),
            language: language.into(),
        }
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
                "stub pack; real engine musl ELF is Linux CI / Docker".into(),
            ));
        }
        Err(EngineError::Spawn(
            "host process spawn of engine packs is reserved for Linux CI / Docker".into(),
        ))
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
        assert!(PackAdapter::new("phpantom", LanguageId::new("php"))
            .extra_languages()
            .is_empty());
    }
}
