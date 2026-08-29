//! ScriptHost: Interpreter + Sandbox. Scripts are not a Strategy for definition.

pub mod engine;
pub mod host;

pub use engine::{FakeEngine, FakeEngineFactory, RhaiEngineFactory, ScriptEngine, ScriptEngineFactory};
pub use host::{
    HookName, ScriptContext, ScriptDecision, ScriptHost, DEFAULT_OPS_LIMIT, DEFAULT_STRING_CAP,
};

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::FakeClock;
    use std::sync::Arc;

    #[test]
    fn public_reexports() {
        let _ = HookName::OnBootstrap;
        let _ = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        assert!(DEFAULT_OPS_LIMIT > 0);
        assert!(DEFAULT_STRING_CAP > 0);
    }
}
