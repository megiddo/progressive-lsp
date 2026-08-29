//! ScriptHost: Interpreter + Sandbox. Scripts are not a Strategy for definition.

pub mod engine;
pub mod host;

pub use engine::{FakeEngine, FakeEngineFactory, RhaiEngineFactory, ScriptEngine, ScriptEngineFactory};
pub use host::{
    filter_spawn_tweaks, HookName, ScriptContext, ScriptDecision, ScriptHost, SpawnDecision,
    SpawnTweak, DEFAULT_OPS_LIMIT, DEFAULT_STRING_CAP, SPAWN_ARGV_ALLOWLIST, SPAWN_ENV_ALLOWLIST,
};

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::FakeClock;
    use std::sync::Arc;

    #[test]
    fn public_reexports() {
        let _ = HookName::OnBootstrap;
        let _ = HookName::OnEngineSpawn;
        let _ = HookName::OnTierReady;
        let _ = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        assert!(DEFAULT_OPS_LIMIT > 0);
        assert!(DEFAULT_STRING_CAP > 0);
    }
}
