//! ScriptHost: Interpreter + Sandbox (Proxy). ClockPort for now(). No I/O by default.

use std::path::PathBuf;
use std::sync::Arc;

use progressive_lsp_core::{ClockPort, InitializeFailed, ScriptAbort, ScriptSandbox};

use crate::engine::{ScriptEngine, ScriptEngineFactory};

pub const DEFAULT_OPS_LIMIT: u64 = 10_000;
pub const DEFAULT_STRING_CAP: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookName {
    OnBootstrap,
    OnWorkspaceDiscover,
    OnPreIndex,
    OnPostIndex,
    OnWatch,
}

impl HookName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnBootstrap => "on_bootstrap",
            Self::OnWorkspaceDiscover => "on_workspace_discover",
            Self::OnPreIndex => "on_pre_index",
            Self::OnPostIndex => "on_post_index",
            Self::OnWatch => "on_watch",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptContext {
    pub path: String,
    pub package: String,
    pub root: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptDecision {
    Continue,
    Abort(String),
    DenyPaths(Vec<String>),
    SkipPackage,
}

impl Default for ScriptDecision {
    fn default() -> Self {
        Self::Continue
    }
}

/// Interpreter + Sandbox. Composition root wires one host; tests inject a factory.
pub struct ScriptHost {
    factory: Box<dyn ScriptEngineFactory>,
    clock: Arc<dyn ClockPort>,
    engines: Vec<Box<dyn ScriptEngine>>,
    pub allow_shell: bool,
    pub ops_limit: u64,
    pub string_cap: usize,
}

impl ScriptHost {
    pub fn new(factory: Box<dyn ScriptEngineFactory>, clock: Arc<dyn ClockPort>) -> Self {
        Self {
            factory,
            clock,
            engines: Vec::new(),
            allow_shell: false,
            ops_limit: DEFAULT_OPS_LIMIT,
            string_cap: DEFAULT_STRING_CAP,
        }
    }

    pub fn load(&mut self, source: &str, name: &str) -> Result<(), ScriptSandbox> {
        let engine = self.factory.create(
            source,
            name,
            self.clock.clone(),
            self.ops_limit,
            self.string_cap,
            self.allow_shell,
        )?;
        self.engines.push(engine);
        Ok(())
    }

    pub fn load_path(&mut self, path: &std::path::Path) -> Result<(), ScriptSandbox> {
        let src = std::fs::read_to_string(path).map_err(|e| ScriptSandbox(e.to_string()))?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("script");
        self.load(&src, name)
    }

    pub fn run(&mut self, hook: HookName, ctx: &ScriptContext) -> Result<ScriptDecision, ScriptSandbox> {
        let mut denied = Vec::new();
        for engine in &mut self.engines {
            match engine.eval_hook(hook, ctx)? {
                ScriptDecision::Continue => {}
                ScriptDecision::Abort(msg) => return Ok(ScriptDecision::Abort(msg)),
                ScriptDecision::DenyPaths(paths) => denied.extend(paths),
                ScriptDecision::SkipPackage => return Ok(ScriptDecision::SkipPackage),
            }
        }
        if denied.is_empty() {
            Ok(ScriptDecision::Continue)
        } else {
            Ok(ScriptDecision::DenyPaths(denied))
        }
    }

    pub fn on_bootstrap(&mut self, ctx: &ScriptContext) -> Result<(), InitializeFailed> {
        match self.run(HookName::OnBootstrap, ctx) {
            Ok(ScriptDecision::Abort(msg)) => Err(InitializeFailed(msg)),
            Ok(_) => Ok(()),
            Err(e) => Err(InitializeFailed(e.0)),
        }
    }

    pub fn on_workspace_discover(&mut self, roots: &[PathBuf]) -> Result<Vec<PathBuf>, ScriptAbort> {
        let mut keep = Vec::new();
        for root in roots {
            let ctx = ScriptContext {
                root: root.to_string_lossy().into_owned(),
                path: root.to_string_lossy().into_owned(),
                package: String::new(),
            };
            match self.run(HookName::OnWorkspaceDiscover, &ctx) {
                Ok(ScriptDecision::Abort(_)) | Ok(ScriptDecision::SkipPackage) => {}
                Ok(ScriptDecision::DenyPaths(paths)) => {
                    if paths.iter().any(|p| root.to_string_lossy().contains(p) || p == root.to_string_lossy().as_ref())
                    {
                        // skip this root
                    } else {
                        keep.push(root.clone());
                    }
                }
                Ok(ScriptDecision::Continue) => keep.push(root.clone()),
                Err(e) => return Err(ScriptAbort(e.0)),
            }
        }
        Ok(keep)
    }

    pub fn on_pre_index(&mut self, package: &str) -> Result<bool, ScriptSandbox> {
        let ctx = ScriptContext {
            package: package.into(),
            ..ScriptContext::default()
        };
        match self.run(HookName::OnPreIndex, &ctx)? {
            ScriptDecision::Abort(_) | ScriptDecision::SkipPackage => Ok(false),
            _ => Ok(true),
        }
    }

    pub fn on_post_index(&mut self, package: &str) -> Result<(), ScriptSandbox> {
        let ctx = ScriptContext {
            package: package.into(),
            ..ScriptContext::default()
        };
        let _ = self.run(HookName::OnPostIndex, &ctx)?;
        Ok(())
    }

    pub fn on_watch(&mut self, paths: &[String]) -> Result<Vec<String>, ScriptSandbox> {
        let mut keep = Vec::new();
        for path in paths {
            let ctx = ScriptContext {
                path: path.clone(),
                ..ScriptContext::default()
            };
            match self.run(HookName::OnWatch, &ctx)? {
                ScriptDecision::Abort(_) | ScriptDecision::SkipPackage => {}
                ScriptDecision::DenyPaths(denied) => {
                    if !denied.iter().any(|d| path.contains(d) || d == path) {
                        keep.push(path.clone());
                    }
                }
                ScriptDecision::Continue => keep.push(path.clone()),
            }
        }
        Ok(keep)
    }

    pub fn is_empty(&self) -> bool {
        self.engines.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{FakeEngineFactory, RhaiEngineFactory};
    use progressive_lsp_core::FakeClock;

    fn host_with(decision: ScriptDecision) -> ScriptHost {
        let mut h = ScriptHost::new(
            Box::new(FakeEngineFactory {
                decision,
                fail_create: None,
            }),
            Arc::new(FakeClock::at_unix_ms(9)),
        );
        h.load("ok", "fake").unwrap();
        h
    }

    #[test]
    fn hook_names() {
        assert_eq!(HookName::OnBootstrap.as_str(), "on_bootstrap");
        assert_eq!(HookName::OnWorkspaceDiscover.as_str(), "on_workspace_discover");
        assert_eq!(HookName::OnPreIndex.as_str(), "on_pre_index");
        assert_eq!(HookName::OnPostIndex.as_str(), "on_post_index");
        assert_eq!(HookName::OnWatch.as_str(), "on_watch");
        assert_eq!(ScriptDecision::default(), ScriptDecision::Continue);
    }

    #[test]
    fn abort_bootstrap_is_initialize_failed() {
        let mut h = host_with(ScriptDecision::Abort("denied".into()));
        assert!(!h.is_empty());
        let err = h.on_bootstrap(&ScriptContext::default()).unwrap_err();
        assert!(err.0.contains("denied"));
        let mut ok = host_with(ScriptDecision::Continue);
        ok.on_bootstrap(&ScriptContext::default()).unwrap();
        let mut fail = ScriptHost::new(
            Box::new(FakeEngineFactory {
                decision: ScriptDecision::Continue,
                fail_create: Some("sandbox-boom".into()),
            }),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        assert!(fail.load("x", "t").is_err());
        let empty = ScriptHost::new(
            Box::new(FakeEngineFactory {
                decision: ScriptDecision::Continue,
                fail_create: None,
            }),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        assert!(empty.is_empty());
        let mut no_engines = empty;
        no_engines.on_bootstrap(&ScriptContext::default()).unwrap();
    }

    #[test]
    fn discover_and_watch_and_index_skips() {
        let mut h = host_with(ScriptDecision::SkipPackage);
        assert!(!h.on_pre_index("pkg").unwrap());
        h.on_post_index("pkg").unwrap();
        let roots = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        assert!(h.on_workspace_discover(&roots).unwrap().is_empty());
        assert!(h.on_watch(&["x.rs".into()]).unwrap().is_empty());

        let mut deny = host_with(ScriptDecision::DenyPaths(vec!["secret".into()]));
        let kept = deny
            .on_watch(&["src/a.rs".into(), "secret/x.rs".into()])
            .unwrap();
        assert_eq!(kept, vec!["src/a.rs"]);
        let roots = vec![PathBuf::from("/keep"), PathBuf::from("/secret")];
        let kept_roots = deny.on_workspace_discover(&roots).unwrap();
        assert_eq!(kept_roots, vec![PathBuf::from("/keep")]);
        assert!(deny.on_pre_index("p").unwrap());

        let mut abort_pre = host_with(ScriptDecision::Abort("no-pkg".into()));
        assert!(!abort_pre.on_pre_index("pkg").unwrap());
        abort_pre.on_post_index("pkg").unwrap();
        assert!(abort_pre.on_watch(&["keep.rs".into()]).unwrap().is_empty());

        let mut cont = host_with(ScriptDecision::Continue);
        assert_eq!(cont.on_watch(&["a.rs".into(), "b.rs".into()]).unwrap(), vec!["a.rs", "b.rs"]);
        assert_eq!(
            cont.on_workspace_discover(&[PathBuf::from("/keep")]).unwrap(),
            vec![PathBuf::from("/keep")]
        );
        assert!(cont.on_pre_index("pkg").unwrap());
    }

    #[test]
    fn rhai_fixture_denies_path_and_aborts_initialize() {
        let clock = Arc::new(FakeClock::at_unix_ms(42));
        let mut host = ScriptHost::new(Box::new(RhaiEngineFactory), clock);
        assert!(host.is_empty());
        assert!(!host.allow_shell);
        host.load(
            r#"
            fn on_bootstrap() {
                abort("denied-path");
            }
            "#,
            "deny.rhai",
        )
        .unwrap();
        let err = host.on_bootstrap(&ScriptContext::default()).unwrap_err();
        assert!(err.0.contains("denied-path"), "{err}");
    }

    #[test]
    fn rhai_now_uses_clock_and_cannot_register_definition() {
        let clock = Arc::new(FakeClock::at_unix_ms(77));
        let mut host = ScriptHost::new(Box::new(RhaiEngineFactory), clock);
        host.load(
            r#"
            fn on_post_index() {
                let t = now();
            }
            "#,
            "now.rhai",
        )
        .unwrap();
        host.on_post_index("p").unwrap();
        let mut bad = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        assert!(bad
            .load(
                "register_method(\"textDocument/definition\"); fn on_bootstrap() {}",
                "bad.rhai"
            )
            .is_err());
    }

    #[test]
    fn rhai_ops_and_string_cap_are_sandbox_errors() {
        let mut host = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        host.ops_limit = 5;
        host.load(
            r#"
            fn on_watch() {
                let s = 0;
                while s < 100000 { s += 1; }
            }
            "#,
            "ops.rhai",
        )
        .unwrap();
        let err = host.on_watch(&["a".into()]).unwrap_err();
        assert!(err.to_string().contains("sandbox") || err.0.contains("operation") || err.0.contains("too"));

        let mut s = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        s.string_cap = 8;
        let loaded = s.load(
            r#"
            fn on_watch() {
                let t = "abcdefghijklmnop";
            }
            "#,
            "str.rhai",
        );
        assert!(
            loaded.is_err() || s.on_watch(&["a".into()]).is_err(),
            "string cap must error on compile or eval"
        );
    }

    #[test]
    fn rhai_deny_path_and_skip_package() {
        let mut host = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        host.load(
            r#"
            fn on_watch() {
                deny_path("drop.me");
            }
            fn on_pre_index() {
                skip_package();
            }
            "#,
            "f.rhai",
        )
        .unwrap();
        let kept = host.on_watch(&["keep.rs".into(), "drop.me".into()]).unwrap();
        assert_eq!(kept, vec!["keep.rs"]);
        assert!(!host.on_pre_index("pkg").unwrap());

        let mut gated = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        gated
            .load(
                r#"
            fn on_pre_index() {
                if pkg != "pkg" { abort("bad-package"); }
            }
            fn on_watch() {
                if path == "drop.me" { deny_path("drop.me"); }
            }
            "#,
                "ctx.rhai",
            )
            .unwrap();
        assert!(gated.on_pre_index("pkg").unwrap());
        assert!(!gated.on_pre_index("other").unwrap());
        assert_eq!(
            gated.on_watch(&["keep.rs".into(), "drop.me".into()]).unwrap(),
            vec!["keep.rs"]
        );
        gated.ops_limit = 5;
        // already compiled with default ops; reload with tiny cap
        let mut tiny = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        tiny.ops_limit = 5;
        tiny.load(
            r#"
            fn on_post_index() {
                let s = 0;
                while s < 100000 { s += 1; }
            }
            "#,
            "post.rhai",
        )
        .unwrap();
        assert!(tiny.on_post_index("p").is_err());
    }

    #[test]
    fn load_path_and_allow_shell_default() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ok.rhai");
        std::fs::write(&p, "fn on_bootstrap() {}\n").unwrap();
        let mut host = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        host.load_path(&p).unwrap();
        host.on_bootstrap(&ScriptContext::default()).unwrap();
        assert!(!host.allow_shell);
        let io = host.load("shell(\"ls\"); fn on_bootstrap() {}", "io.rhai");
        assert!(io.is_err());
        assert!(host.load_path(PathBuf::from("/no/such/script.rhai").as_path()).is_err());
    }

    #[test]
    fn missing_hook_is_continue() {
        let mut host = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        host.load("fn other() {}", "n.rhai").unwrap();
        assert_eq!(
            host.run(HookName::OnPostIndex, &ScriptContext::default()).unwrap(),
            ScriptDecision::Continue
        );
    }
}
