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
    OnEngineSpawn,
    OnTierReady,
    OnInstallVerify,
}

impl HookName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnBootstrap => "on_bootstrap",
            Self::OnWorkspaceDiscover => "on_workspace_discover",
            Self::OnPreIndex => "on_pre_index",
            Self::OnPostIndex => "on_post_index",
            Self::OnWatch => "on_watch",
            Self::OnEngineSpawn => "on_engine_spawn",
            Self::OnTierReady => "on_tier_ready",
            Self::OnInstallVerify => "on_install_verify",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptContext {
    pub path: String,
    pub package: String,
    pub root: String,
    pub pack: String,
    pub argv: Vec<String>,
    pub cwd: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnTweak {
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
}

impl SpawnTweak {
    pub fn is_empty(&self) -> bool {
        self.argv.is_empty() && self.cwd.is_none() && self.env.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptDecision {
    Continue,
    Abort(String),
    DenyPaths(Vec<String>),
    SkipPackage,
    TweakSpawn(SpawnTweak),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpawnDecision {
    Proceed(SpawnTweak),
    Skip(String),
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

    pub fn run(
        &mut self,
        hook: HookName,
        ctx: &ScriptContext,
    ) -> Result<ScriptDecision, ScriptSandbox> {
        let mut denied = Vec::new();
        let mut tweak = SpawnTweak::default();
        for engine in &mut self.engines {
            match engine.eval_hook(hook, ctx)? {
                ScriptDecision::Continue => {}
                ScriptDecision::Abort(msg) => return Ok(ScriptDecision::Abort(msg)),
                ScriptDecision::DenyPaths(paths) => denied.extend(paths),
                ScriptDecision::SkipPackage => return Ok(ScriptDecision::SkipPackage),
                ScriptDecision::TweakSpawn(t) => {
                    tweak.argv.extend(t.argv);
                    if t.cwd.is_some() {
                        tweak.cwd = t.cwd;
                    }
                    tweak.env.extend(t.env);
                }
            }
        }
        if !denied.is_empty() {
            Ok(ScriptDecision::DenyPaths(denied))
        } else if !tweak.is_empty() {
            Ok(ScriptDecision::TweakSpawn(tweak))
        } else {
            Ok(ScriptDecision::Continue)
        }
    }

    pub fn on_bootstrap(&mut self, ctx: &ScriptContext) -> Result<(), InitializeFailed> {
        match self.run(HookName::OnBootstrap, ctx) {
            Ok(ScriptDecision::Abort(msg)) => Err(InitializeFailed(msg)),
            Ok(_) => Ok(()),
            Err(e) => Err(InitializeFailed(e.0)),
        }
    }

    pub fn on_workspace_discover(
        &mut self,
        roots: &[PathBuf],
    ) -> Result<Vec<PathBuf>, ScriptAbort> {
        let mut keep = Vec::new();
        for root in roots {
            let ctx = ScriptContext {
                root: root.to_string_lossy().into_owned(),
                path: root.to_string_lossy().into_owned(),
                package: String::new(),
                ..ScriptContext::default()
            };
            match self.run(HookName::OnWorkspaceDiscover, &ctx) {
                Ok(ScriptDecision::Abort(_)) | Ok(ScriptDecision::SkipPackage) => {}
                Ok(ScriptDecision::DenyPaths(paths)) => {
                    if paths.iter().any(|p| {
                        root.to_string_lossy().contains(p) || p == root.to_string_lossy().as_ref()
                    }) {
                        // skip this root
                    } else {
                        keep.push(root.clone());
                    }
                }
                Ok(ScriptDecision::Continue) | Ok(ScriptDecision::TweakSpawn(_)) => {
                    keep.push(root.clone())
                }
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

    pub fn on_engine_spawn(
        &mut self,
        pack: &str,
        workspace: &str,
    ) -> Result<SpawnDecision, ScriptSandbox> {
        let ctx = ScriptContext {
            pack: pack.into(),
            root: workspace.into(),
            cwd: workspace.into(),
            ..ScriptContext::default()
        };
        match self.run(HookName::OnEngineSpawn, &ctx)? {
            ScriptDecision::Abort(msg) => Ok(SpawnDecision::Skip(msg)),
            ScriptDecision::SkipPackage => Ok(SpawnDecision::Skip("skip_package".into())),
            ScriptDecision::TweakSpawn(tweak) => Ok(SpawnDecision::Proceed(filter_spawn_tweaks(
                &tweak, workspace,
            ))),
            _ => Ok(SpawnDecision::Proceed(SpawnTweak::default())),
        }
    }

    /// After hash check, before first exec / rename of a new binary. Abort refuses it.
    pub fn on_install_verify(&mut self, path: &str, pack: &str) -> Result<(), ScriptAbort> {
        let ctx = ScriptContext {
            path: path.into(),
            pack: pack.into(),
            ..ScriptContext::default()
        };
        match self.run(HookName::OnInstallVerify, &ctx) {
            Ok(ScriptDecision::Abort(msg)) => Err(ScriptAbort(msg)),
            Ok(_) => Ok(()),
            Err(e) => Err(ScriptAbort(e.0)),
        }
    }

    /// Logging only. Abort cannot unwind a tier that is already ready.
    pub fn on_tier_ready(&mut self, language: &str, package: &str) -> Result<(), ScriptSandbox> {
        let ctx = ScriptContext {
            package: package.into(),
            path: language.into(),
            ..ScriptContext::default()
        };
        match self.run(HookName::OnTierReady, &ctx)? {
            ScriptDecision::Abort(_) => Ok(()),
            _ => Ok(()),
        }
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
                ScriptDecision::Continue | ScriptDecision::TweakSpawn(_) => keep.push(path.clone()),
            }
        }
        Ok(keep)
    }

    pub fn is_empty(&self) -> bool {
        self.engines.is_empty()
    }
}

/// Env keys scripts may set on engine spawn. Others are dropped.
pub const SPAWN_ENV_ALLOWLIST: &[&str] = &["RUST_LOG", "RA_LOG", "TY_LOG", "TMPDIR"];
/// Argv tokens scripts may append. Others are dropped.
pub const SPAWN_ARGV_ALLOWLIST: &[&str] = &[
    "--stdio",
    "--quiet",
    "--log-level",
    "--log-file",
    "--log",
    "-logfile",
];

pub fn filter_spawn_tweaks(tweak: &SpawnTweak, workspace: &str) -> SpawnTweak {
    let argv = tweak
        .argv
        .iter()
        .filter(|a| argv_allowed(a))
        .cloned()
        .collect();
    let cwd = tweak.cwd.as_deref().and_then(|c| {
        if cwd_allowed(c, workspace) {
            Some(c.to_string())
        } else {
            None
        }
    });
    let env = tweak
        .env
        .iter()
        .filter(|(k, _)| SPAWN_ENV_ALLOWLIST.iter().any(|ok| *ok == k.as_str()))
        .cloned()
        .collect();
    SpawnTweak { argv, cwd, env }
}

fn argv_allowed(arg: &str) -> bool {
    SPAWN_ARGV_ALLOWLIST
        .iter()
        .any(|k| *arg == **k || arg.starts_with(&format!("{k}=")))
}

fn cwd_allowed(cwd: &str, workspace: &str) -> bool {
    if workspace.is_empty() {
        return false;
    }
    cwd == workspace
        || cwd.starts_with(&format!("{workspace}/"))
        || cwd.starts_with(&format!("{workspace}\\"))
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
        assert_eq!(
            HookName::OnWorkspaceDiscover.as_str(),
            "on_workspace_discover"
        );
        assert_eq!(HookName::OnPreIndex.as_str(), "on_pre_index");
        assert_eq!(HookName::OnPostIndex.as_str(), "on_post_index");
        assert_eq!(HookName::OnWatch.as_str(), "on_watch");
        assert_eq!(HookName::OnEngineSpawn.as_str(), "on_engine_spawn");
        assert_eq!(HookName::OnTierReady.as_str(), "on_tier_ready");
        assert_eq!(HookName::OnInstallVerify.as_str(), "on_install_verify");
        assert_eq!(ScriptDecision::default(), ScriptDecision::Continue);
        assert!(SpawnTweak::default().is_empty());
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
        assert_eq!(
            cont.on_watch(&["a.rs".into(), "b.rs".into()]).unwrap(),
            vec!["a.rs", "b.rs"]
        );
        assert_eq!(
            cont.on_workspace_discover(&[PathBuf::from("/keep")])
                .unwrap(),
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
        assert!(
            err.to_string().contains("sandbox")
                || err.0.contains("operation")
                || err.0.contains("too")
        );

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
        let kept = host
            .on_watch(&["keep.rs".into(), "drop.me".into()])
            .unwrap();
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
            gated
                .on_watch(&["keep.rs".into(), "drop.me".into()])
                .unwrap(),
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
        assert!(host
            .load_path(PathBuf::from("/no/such/script.rhai").as_path())
            .is_err());
    }

    #[test]
    fn on_engine_spawn_abort_skips_and_tweaks_are_allowlisted() {
        let mut abort = host_with(ScriptDecision::Abort("skip-ty".into()));
        match abort.on_engine_spawn("python", "/ws").unwrap() {
            SpawnDecision::Skip(msg) => assert!(msg.contains("skip-ty")),
            other => panic!("{other:?}"),
        }
        let mut skip = host_with(ScriptDecision::SkipPackage);
        match skip.on_engine_spawn("python", "/ws").unwrap() {
            SpawnDecision::Skip(_) => {}
            other => panic!("{other:?}"),
        }
        let mut tweaks = host_with(ScriptDecision::TweakSpawn(SpawnTweak {
            argv: vec!["--stdio".into(), "--evil".into()],
            cwd: Some("/ws/src".into()),
            env: vec![
                ("RUST_LOG".into(), "info".into()),
                ("LD_PRELOAD".into(), "x".into()),
            ],
        }));
        match tweaks.on_engine_spawn("python", "/ws").unwrap() {
            SpawnDecision::Proceed(t) => {
                assert_eq!(t.argv, vec!["--stdio".to_string()]);
                assert_eq!(t.cwd.as_deref(), Some("/ws/src"));
                assert_eq!(t.env, vec![("RUST_LOG".into(), "info".into())]);
            }
            other => panic!("{other:?}"),
        }
        let mut bad_cwd = host_with(ScriptDecision::TweakSpawn(SpawnTweak {
            argv: vec!["--log-level=error".into()],
            cwd: Some("/etc".into()),
            env: vec![("TY_LOG".into(), "1".into())],
        }));
        match bad_cwd.on_engine_spawn("python", "/ws").unwrap() {
            SpawnDecision::Proceed(t) => {
                assert_eq!(t.argv, vec!["--log-level=error".to_string()]);
                assert!(t.cwd.is_none());
                assert_eq!(t.env, vec![("TY_LOG".into(), "1".into())]);
            }
            other => panic!("{other:?}"),
        }
        let mut cont = host_with(ScriptDecision::Continue);
        match cont.on_engine_spawn("rust", "/ws").unwrap() {
            SpawnDecision::Proceed(t) => assert!(t.is_empty()),
            other => panic!("{other:?}"),
        }
        assert!(filter_spawn_tweaks(&SpawnTweak::default(), "").is_empty());
        assert!(!cwd_allowed("/x", ""));
        assert!(cwd_allowed(r"C:\ws\a", r"C:\ws"));
        assert!(argv_allowed("--quiet"));
        assert!(!argv_allowed("--eval"));
        assert!(argv_allowed("--log=verbose"));
        assert!(argv_allowed("-logfile=/tmp/gopls.log"));
        assert!(!argv_allowed("-rpc.trace"));
        assert!(!argv_allowed("-rpc.trace=true"));
        assert!(!SPAWN_ENV_ALLOWLIST.contains(&"RA_LOG_FILE"));
        assert!(!SPAWN_ENV_ALLOWLIST.contains(&"TY_LOG_PROFILE"));
        let clangd = filter_spawn_tweaks(
            &SpawnTweak {
                argv: vec!["--log=verbose".into(), "-rpc.trace".into()],
                cwd: None,
                env: vec![("RA_LOG_FILE".into(), "x".into())],
            },
            "/ws",
        );
        assert_eq!(clangd.argv, vec!["--log=verbose".to_string()]);
        assert!(clangd.env.is_empty());
        let gopls = filter_spawn_tweaks(
            &SpawnTweak {
                argv: vec!["-logfile=/tmp/gopls.log".into()],
                cwd: None,
                env: vec![("TY_LOG_PROFILE".into(), "1".into())],
            },
            "/ws",
        );
        assert_eq!(gopls.argv, vec!["-logfile=/tmp/gopls.log".to_string()]);
        assert!(gopls.env.is_empty());
    }

    #[test]
    fn on_tier_ready_abort_cannot_unwind_intelligence() {
        let mut abort = host_with(ScriptDecision::Abort("no".into()));
        abort.on_tier_ready("python", "pkg").unwrap();
        let mut cont = host_with(ScriptDecision::Continue);
        cont.on_tier_ready("rust", "crate").unwrap();
    }

    #[test]
    fn on_install_verify_abort_refuses_new_binary() {
        let mut abort = host_with(ScriptDecision::Abort("bad-elf".into()));
        let err = abort
            .on_install_verify("/prefix/engines/python/ty", "python")
            .unwrap_err();
        assert!(err.0.contains("bad-elf"));
        let mut ok = host_with(ScriptDecision::Continue);
        ok.on_install_verify("/prefix/bin/x", "python").unwrap();
        let mut skip = host_with(ScriptDecision::SkipPackage);
        skip.on_install_verify("/p", "rust").unwrap();
        let mut tiny = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        tiny.ops_limit = 5;
        tiny.load(
            r#"
            fn on_install_verify() {
                let s = 0;
                while s < 100000 { s += 1; }
            }
            "#,
            "verify.rhai",
        )
        .unwrap();
        assert!(tiny.on_install_verify("/p", "python").is_err());
        let mut rhai = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        rhai.load(
            r#"
            fn on_install_verify() {
                if pack != "python" { abort("bad-pack"); }
                if path == "" { abort("empty"); }
            }
            "#,
            "ok.rhai",
        )
        .unwrap();
        rhai.on_install_verify("/prefix/engines/python/ty", "python")
            .unwrap();
        assert!(rhai.on_install_verify("/x", "rust").is_err());
    }

    #[test]
    fn rhai_spawn_uses_pack_root_cwd_and_tier_ready_sandbox() {
        let mut host = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        host.load(
            r#"
            fn on_engine_spawn() {
                if pack != "python" { abort("bad-pack"); }
                if root != "/ws" { abort("bad-root"); }
                tweak_argv("--stdio");
                tweak_cwd(cwd);
                tweak_env("TY_LOG", "1");
            }
            "#,
            "spawn.rhai",
        )
        .unwrap();
        match host.on_engine_spawn("python", "/ws").unwrap() {
            SpawnDecision::Proceed(t) => {
                assert_eq!(t.argv, vec!["--stdio".to_string()]);
                assert_eq!(t.cwd.as_deref(), Some("/ws"));
                assert_eq!(t.env, vec![("TY_LOG".into(), "1".into())]);
            }
            other => panic!("{other:?}"),
        }
        match host.on_engine_spawn("rust", "/ws").unwrap() {
            SpawnDecision::Skip(msg) => assert!(msg.contains("bad-pack")),
            other => panic!("{other:?}"),
        }
        let mut tiny = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        tiny.ops_limit = 5;
        tiny.load(
            r#"
            fn on_tier_ready() {
                let s = 0;
                while s < 100000 { s += 1; }
            }
            "#,
            "tier.rhai",
        )
        .unwrap();
        assert!(tiny.on_tier_ready("python", "pkg").is_err());
    }

    #[test]
    fn missing_hook_is_continue() {
        let mut host = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        host.load("fn other() {}", "n.rhai").unwrap();
        assert_eq!(
            host.run(HookName::OnPostIndex, &ScriptContext::default())
                .unwrap(),
            ScriptDecision::Continue
        );
    }
}
