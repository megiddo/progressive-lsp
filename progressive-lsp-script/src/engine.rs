//! Abstract Factory for script engines. Tests inject [`FakeEngine`]; production is Rhai.

use std::sync::{Arc, Mutex};

use progressive_lsp_core::{ClockPort, ScriptSandbox};
use rhai::{Dynamic, Engine, EvalAltResult, Position, Scope};

use crate::host::{HookName, ScriptContext, ScriptDecision, SpawnTweak};

/// One loaded script engine (Interpreter).
pub trait ScriptEngine: Send {
    fn eval_hook(&mut self, hook: HookName, ctx: &ScriptContext) -> Result<ScriptDecision, ScriptSandbox>;
}

/// Abstract Factory. Watch tests must not hard-code Rhai.
pub trait ScriptEngineFactory: Send + Sync {
    fn create(
        &self,
        source: &str,
        name: &str,
        clock: Arc<dyn ClockPort>,
        ops_limit: u64,
        string_cap: usize,
        allow_shell: bool,
    ) -> Result<Box<dyn ScriptEngine>, ScriptSandbox>;
}

pub struct RhaiEngineFactory;

struct HookState {
    denied: Vec<String>,
    skip: bool,
    tweak: SpawnTweak,
}

struct RhaiEngine {
    engine: Engine,
    ast: rhai::AST,
    name: String,
    state: Arc<Mutex<HookState>>,
}

impl ScriptEngineFactory for RhaiEngineFactory {
    fn create(
        &self,
        source: &str,
        name: &str,
        clock: Arc<dyn ClockPort>,
        ops_limit: u64,
        string_cap: usize,
        allow_shell: bool,
    ) -> Result<Box<dyn ScriptEngine>, ScriptSandbox> {
        if source.contains("register_method") && source.contains("textDocument/definition") {
            return Err(ScriptSandbox(
                "scripts cannot register textDocument/definition".into(),
            ));
        }
        if !allow_shell
            && (source.contains("shell(")
                || source.contains("exec(")
                || source.contains("std::fs")
                || source.contains("open(") && source.contains("file"))
        {
            return Err(ScriptSandbox("I/O is not allowed (allow_shell is false)".into()));
        }
        let mut engine = Engine::new();
        engine.set_max_operations(ops_limit);
        engine.set_max_string_size(string_cap);
        let clock_now = clock.clone();
        engine.register_fn("now", move || clock_now.unix_ms() as i64);
        engine.register_fn("abort", |msg: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            Err(Box::new(EvalAltResult::ErrorRuntime(
                Dynamic::from(format!("abort:{msg}")),
                Position::NONE,
            )))
        });
        let state = Arc::new(Mutex::new(HookState {
            denied: Vec::new(),
            skip: false,
            tweak: SpawnTweak::default(),
        }));
        let deny = state.clone();
        engine.register_fn("deny_path", move |p: &str| {
            deny.lock().expect("hook").denied.push(p.to_string());
        });
        let skip = state.clone();
        engine.register_fn("skip_package", move || {
            skip.lock().expect("hook").skip = true;
        });
        let argv = state.clone();
        engine.register_fn("tweak_argv", move |a: &str| {
            argv.lock().expect("hook").tweak.argv.push(a.to_string());
        });
        let cwd = state.clone();
        engine.register_fn("tweak_cwd", move |c: &str| {
            cwd.lock().expect("hook").tweak.cwd = Some(c.to_string());
        });
        let env = state.clone();
        engine.register_fn("tweak_env", move |k: &str, v: &str| {
            env.lock()
                .expect("hook")
                .tweak
                .env
                .push((k.to_string(), v.to_string()));
        });
        let ast = engine
            .compile(source)
            .map_err(|e| ScriptSandbox(e.to_string()))?;
        Ok(Box::new(RhaiEngine {
            engine,
            ast,
            name: name.to_string(),
            state,
        }))
    }
}

fn decision_from_err(err: &EvalAltResult) -> Option<ScriptDecision> {
    let s = err.to_string();
    if let Some(rest) = s.strip_prefix("Runtime error: abort:") {
        let msg = rest.split(" (").next().unwrap_or(rest).trim().to_string();
        return Some(ScriptDecision::Abort(msg));
    }
    if s.contains("abort:") {
        let msg = s
            .split("abort:")
            .nth(1)
            .unwrap_or("aborted")
            .split('(')
            .next()
            .unwrap_or("aborted")
            .trim()
            .to_string();
        return Some(ScriptDecision::Abort(msg));
    }
    None
}

impl ScriptEngine for RhaiEngine {
    fn eval_hook(&mut self, hook: HookName, ctx: &ScriptContext) -> Result<ScriptDecision, ScriptSandbox> {
        let mut scope = Scope::new();
        scope.push("path", ctx.path.clone());
        // Rhai reserves `package`; scripts read the package id as `pkg`.
        scope.push("pkg", ctx.package.clone());
        scope.push("root", ctx.root.clone());
        scope.push("pack", ctx.pack.clone());
        scope.push("cwd", ctx.cwd.clone());
        {
            let mut st = self.state.lock().expect("hook");
            st.denied.clear();
            st.skip = false;
            st.tweak = SpawnTweak::default();
        }
        let _ = &self.name;
        match self
            .engine
            .call_fn::<()>(&mut scope, &self.ast, hook.as_str(), ())
        {
            Ok(()) => {
                let st = self.state.lock().expect("hook");
                if st.skip {
                    return Ok(ScriptDecision::SkipPackage);
                }
                if !st.denied.is_empty() {
                    return Ok(ScriptDecision::DenyPaths(st.denied.clone()));
                }
                if !st.tweak.is_empty() {
                    return Ok(ScriptDecision::TweakSpawn(st.tweak.clone()));
                }
                Ok(ScriptDecision::Continue)
            }
            Err(e) => {
                if let Some(dec) = decision_from_err(e.as_ref()) {
                    return Ok(dec);
                }
                let msg = e.to_string();
                if msg.contains("Function not found") {
                    return Ok(ScriptDecision::Continue);
                }
                if msg.contains("too many operations")
                    || msg.contains("string") && msg.contains("too")
                    || msg.contains("exceed")
                {
                    return Err(ScriptSandbox(msg));
                }
                Err(ScriptSandbox(msg))
            }
        }
    }
}

/// Test double. Same factory trait as Rhai.
#[derive(Clone, Debug)]
pub struct FakeEngine {
    pub decision: ScriptDecision,
}

impl ScriptEngine for FakeEngine {
    fn eval_hook(&mut self, _hook: HookName, _ctx: &ScriptContext) -> Result<ScriptDecision, ScriptSandbox> {
        Ok(self.decision.clone())
    }
}

#[derive(Clone, Debug, Default)]
pub struct FakeEngineFactory {
    pub decision: ScriptDecision,
    pub fail_create: Option<String>,
}

impl ScriptEngineFactory for FakeEngineFactory {
    fn create(
        &self,
        source: &str,
        _name: &str,
        _clock: Arc<dyn ClockPort>,
        _ops_limit: u64,
        _string_cap: usize,
        _allow_shell: bool,
    ) -> Result<Box<dyn ScriptEngine>, ScriptSandbox> {
        if source.contains("register_method") && source.contains("textDocument/definition") {
            return Err(ScriptSandbox(
                "scripts cannot register textDocument/definition".into(),
            ));
        }
        if let Some(msg) = &self.fail_create {
            return Err(ScriptSandbox(msg.clone()));
        }
        Ok(Box::new(FakeEngine {
            decision: self.decision.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::FakeClock;

    #[test]
    fn fake_factory_is_not_rhai() {
        let f = FakeEngineFactory {
            decision: ScriptDecision::Abort("no".into()),
            fail_create: None,
        };
        let mut e = f
            .create("x", "t", Arc::new(FakeClock::at_unix_ms(1)), 10, 10, false)
            .unwrap();
        let d = e
            .eval_hook(HookName::OnBootstrap, &ScriptContext::default())
            .unwrap();
        assert_eq!(d, ScriptDecision::Abort("no".into()));
        let fail = FakeEngineFactory {
            decision: ScriptDecision::Continue,
            fail_create: Some("boom".into()),
        };
        assert!(fail
            .create("x", "t", Arc::new(FakeClock::at_unix_ms(1)), 10, 10, false)
            .is_err());
        assert!(f
            .create(
                "register_method(\"textDocument/definition\")",
                "t",
                Arc::new(FakeClock::at_unix_ms(1)),
                10,
                10,
                false
            )
            .is_err());
        assert!(
            f.create("register_method(\"hover\")", "t", Arc::new(FakeClock::at_unix_ms(1)), 10, 10, false)
                .is_ok(),
            "register_method alone is not definition"
        );
        assert!(
            f.create("let x = \"textDocument/definition\";", "t", Arc::new(FakeClock::at_unix_ms(1)), 10, 10, false)
                .is_ok(),
            "definition string alone is not a registration"
        );
    }

    #[test]
    fn rhai_sandbox_requires_both_definition_tokens_and_blocks_io() {
        let f = RhaiEngineFactory;
        let clock = || -> Arc<dyn progressive_lsp_core::ClockPort> { Arc::new(FakeClock::at_unix_ms(1)) };
        assert!(f
            .create(
                "register_method(\"textDocument/definition\"); fn on_bootstrap() {}",
                "t",
                clock(),
                100,
                100,
                false
            )
            .is_err());
        assert!(
            f.create("fn on_bootstrap() { register_method(\"hover\"); }", "t", clock(), 100, 4096, false)
                .is_ok()
        );
        assert!(
            f.create("fn on_bootstrap() { let x = \"textDocument/definition\"; }", "t", clock(), 100, 4096, false)
                .is_ok()
        );
        assert!(f
            .create("shell(\"ls\"); fn on_bootstrap() {}", "t", clock(), 100, 100, false)
            .is_err());
        assert!(f
            .create("exec(\"ls\"); fn on_bootstrap() {}", "t", clock(), 100, 100, false)
            .is_err());
        assert!(f
            .create("std::fs; fn on_bootstrap() {}", "t", clock(), 100, 100, false)
            .is_err());
        assert!(f
            .create("open(\"file\"); fn on_bootstrap() {}", "t", clock(), 100, 100, false)
            .is_err());
        assert!(
            f.create("fn on_bootstrap() { let x = \"file\"; }", "t", clock(), 100, 4096, false)
                .is_ok(),
            "file without open is allowed"
        );
        assert!(
            f.create("fn on_bootstrap() { open(\"x\"); }", "t", clock(), 100, 4096, false)
                .is_ok(),
            "open without file token is allowed when allow_shell is false"
        );
        assert!(
            f.create("fn on_bootstrap() { let hint = \"shell(\"; }", "t", clock(), 100, 4096, true)
                .is_ok(),
            "allow_shell true permits shell("
        );
    }

    #[test]
    fn rhai_abort_is_decision_not_sandbox() {
        let mut e = RhaiEngineFactory
            .create(
                r#"fn on_bootstrap() { abort("denied-path"); }"#,
                "t",
                Arc::new(FakeClock::at_unix_ms(1)),
                1000,
                4096,
                false,
            )
            .unwrap();
        let d = e
            .eval_hook(HookName::OnBootstrap, &ScriptContext::default())
            .expect("abort is a decision");
        assert_eq!(d, ScriptDecision::Abort("denied-path".into()));
        let mut skip = RhaiEngineFactory
            .create(
                "fn on_pre_index() { skip_package(); }",
                "t",
                Arc::new(FakeClock::at_unix_ms(1)),
                1000,
                4096,
                false,
            )
            .unwrap();
        assert_eq!(
            skip.eval_hook(HookName::OnPreIndex, &ScriptContext { package: "pkg".into(), ..Default::default() })
                .unwrap(),
            ScriptDecision::SkipPackage
        );
        let mut deny = RhaiEngineFactory
            .create(
                "fn on_watch() { deny_path(\"drop.me\"); }",
                "t",
                Arc::new(FakeClock::at_unix_ms(1)),
                1000,
                4096,
                false,
            )
            .unwrap();
        match deny
            .eval_hook(HookName::OnWatch, &ScriptContext { path: "drop.me".into(), ..Default::default() })
            .unwrap()
        {
            ScriptDecision::DenyPaths(p) => assert_eq!(p, vec!["drop.me".to_string()]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rhai_engine_spawn_abort_and_tweaks() {
        let mut skip = RhaiEngineFactory
            .create(
                r#"fn on_engine_spawn() { abort("skip-ty"); }"#,
                "t",
                Arc::new(FakeClock::at_unix_ms(1)),
                1000,
                4096,
                false,
            )
            .unwrap();
        assert_eq!(
            skip.eval_hook(HookName::OnEngineSpawn, &ScriptContext { pack: "python".into(), ..Default::default() })
                .unwrap(),
            ScriptDecision::Abort("skip-ty".into())
        );
        let mut tweak = RhaiEngineFactory
            .create(
                r#"fn on_engine_spawn() { tweak_argv("--stdio"); tweak_cwd("/ws"); tweak_env("RUST_LOG", "info"); }"#,
                "t",
                Arc::new(FakeClock::at_unix_ms(1)),
                1000,
                4096,
                false,
            )
            .unwrap();
        match tweak
            .eval_hook(
                HookName::OnEngineSpawn,
                &ScriptContext {
                    pack: "python".into(),
                    root: "/ws".into(),
                    ..Default::default()
                },
            )
            .unwrap()
        {
            ScriptDecision::TweakSpawn(t) => {
                assert_eq!(t.argv, vec!["--stdio".to_string()]);
                assert_eq!(t.cwd.as_deref(), Some("/ws"));
                assert_eq!(t.env, vec![("RUST_LOG".into(), "info".into())]);
            }
            other => panic!("{other:?}"),
        }
        let mut ready = RhaiEngineFactory
            .create(
                r#"fn on_tier_ready() { abort("cannot-drop"); }"#,
                "t",
                Arc::new(FakeClock::at_unix_ms(1)),
                1000,
                4096,
                false,
            )
            .unwrap();
        assert!(matches!(
            ready
                .eval_hook(HookName::OnTierReady, &ScriptContext::default())
                .unwrap(),
            ScriptDecision::Abort(_)
        ));
    }
}
