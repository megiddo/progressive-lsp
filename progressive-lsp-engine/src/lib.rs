//! EngineAdapter, EngineSupervisor, pack discovery. Core boots without engines.

pub mod adapter;
pub mod backoff;
pub mod capabilities;
pub mod discovery;
pub mod fake;
pub mod hooks;
pub mod pack;
pub mod resolve;
pub mod supervisor;

pub use adapter::{
    ChildHandle, ChildIo, EngineAdapter, EngineBinary, EngineMessage, ReadyKind, SpawnCtx,
};
pub use backoff::{can_respawn, BackoffPolicy};
pub use capabilities::EngineCapabilities;
pub use discovery::{
    binary_name_for_pack, discover_pack, discover_pack_opt, full_pack_names, hex_of, is_heavy_pack,
    is_pack_stub, pack_dir, slim_pack_names, stub_pack_bytes, BIOME_BINARY, BIOME_PACK,
    CLANGD_BINARY, CLANGD_PACK, GOPLS_BINARY, GOPLS_PACK, PHPANTOM_BINARY, PHPANTOM_PACK,
    PYTHON_PACK, RA_BINARY, RUST_PACK, SUPERHTML_BINARY, SUPERHTML_PACK, TSGO_BINARY, TSGO_PACK,
    TY_BINARY, ZLS_BINARY, ZLS_PACK,
};
pub use fake::{FakeAnswers, FakeEngineAdapter};
pub use hooks::{
    apply_tweaks, AbortSpawnHooks, EngineHooks, NoopHooks, ScriptHookBridge, SpawnHookResult,
};
pub use pack::PackAdapter;
pub use resolve::EngineResolver;
pub use supervisor::EngineSupervisor;

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::{FakeClock, PrefixLayout};
    use std::sync::Arc;

    #[test]
    fn public_reexports_and_core_boots_without_engines() {
        let prefix = PrefixLayout::from_path("/tmp/plsp-no-engines");
        let sup = EngineSupervisor::new(Arc::new(FakeClock::at_unix_ms(1)), prefix);
        assert!(sup.adapter_names().is_empty());
        assert!(!sup.is_ready(
            &progressive_lsp_core::LanguageId::new("python"),
            &progressive_lsp_core::PackageId::new("pkg")
        ));
        let _ = BackoffPolicy::DEFAULT;
        let _ = PYTHON_PACK;
        let _ = RUST_PACK;
        let _ = CLANGD_PACK;
        let _ = TSGO_PACK;
        let _ = PHPANTOM_PACK;
        let _ = SUPERHTML_PACK;
        let _ = BIOME_PACK;
        let _ = GOPLS_PACK;
        let _ = ZLS_PACK;
        let _ = TY_BINARY;
        let _ = RA_BINARY;
        let _ = CLANGD_BINARY;
        let _ = TSGO_BINARY;
        let _ = PHPANTOM_BINARY;
        let _ = SUPERHTML_BINARY;
        let _ = BIOME_BINARY;
        let _ = GOPLS_BINARY;
        let _ = ZLS_BINARY;
        let _ = PackAdapter::python();
        let _ = PackAdapter::clangd();
        let _ = ChildIo::lsp_with_stderr_pipe();
        let _ = FakeEngineAdapter::ty();
        let _ = FakeEngineAdapter::clangd();
        assert!(slim_pack_names().len() < full_pack_names().len());
        assert!(is_heavy_pack(CLANGD_PACK));
        assert_eq!(binary_name_for_pack(TSGO_PACK), Some(TSGO_BINARY));
        let _ = NoopHooks;
        assert!(can_respawn(1, 0));
    }
}
