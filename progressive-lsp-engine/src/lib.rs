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
    ChildHandle, EngineAdapter, EngineBinary, EngineMessage, ReadyKind, SpawnCtx,
};
pub use backoff::{can_respawn, BackoffPolicy};
pub use capabilities::EngineCapabilities;
pub use discovery::{
    discover_pack, discover_pack_opt, hex_of, is_pack_stub, pack_dir, stub_pack_bytes, PYTHON_PACK,
    RA_BINARY, RUST_PACK, TY_BINARY,
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
        let _ = TY_BINARY;
        let _ = RA_BINARY;
        let _ = PackAdapter::python();
        let _ = FakeEngineAdapter::ty();
        let _ = NoopHooks;
        assert!(can_respawn(1, 0));
    }
}
