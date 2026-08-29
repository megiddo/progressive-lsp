//! Engine spawn / tier-ready hooks. Scripts cannot replace the resolver.

use std::sync::Mutex;

use progressive_lsp_core::PrefixLayout;
use progressive_lsp_script::{
    filter_spawn_tweaks, ScriptHost, SpawnDecision, SpawnTweak,
};

use crate::adapter::SpawnCtx;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpawnHookResult {
    Proceed(SpawnTweak),
    Abort(String),
}

pub trait EngineHooks: Send + Sync {
    fn decide_spawn(&self, pack: &str, ctx: &SpawnCtx) -> SpawnHookResult;
    fn notify_tier_ready(&self, language: &str, package: &str);
}

pub struct NoopHooks;

impl EngineHooks for NoopHooks {
    fn decide_spawn(&self, _pack: &str, _ctx: &SpawnCtx) -> SpawnHookResult {
        SpawnHookResult::Proceed(SpawnTweak::default())
    }
    fn notify_tier_ready(&self, _language: &str, _package: &str) {}
}

pub struct AbortSpawnHooks {
    pub message: String,
}

impl EngineHooks for AbortSpawnHooks {
    fn decide_spawn(&self, _pack: &str, _ctx: &SpawnCtx) -> SpawnHookResult {
        SpawnHookResult::Abort(self.message.clone())
    }
    fn notify_tier_ready(&self, _language: &str, _package: &str) {}
}

pub struct ScriptHookBridge {
    host: Mutex<ScriptHost>,
}

impl ScriptHookBridge {
    pub fn new(host: ScriptHost) -> Self {
        Self {
            host: Mutex::new(host),
        }
    }
}

impl EngineHooks for ScriptHookBridge {
    fn decide_spawn(&self, pack: &str, ctx: &SpawnCtx) -> SpawnHookResult {
        let workspace = ctx.workspace.to_string_lossy().into_owned();
        let mut host = self.host.lock().expect("scripts");
        match host.on_engine_spawn(pack, &workspace) {
            Ok(SpawnDecision::Skip(msg)) => SpawnHookResult::Abort(msg),
            Ok(SpawnDecision::Proceed(tweak)) => {
                SpawnHookResult::Proceed(filter_spawn_tweaks(&tweak, &workspace))
            }
            Err(e) => SpawnHookResult::Abort(e.0),
        }
    }

    fn notify_tier_ready(&self, language: &str, package: &str) {
        let mut host = self.host.lock().expect("scripts");
        let _ = host.on_tier_ready(language, package);
    }
}

pub fn apply_tweaks(mut ctx: SpawnCtx, tweak: &SpawnTweak, prefix: &PrefixLayout) -> SpawnCtx {
    let workspace = ctx.workspace.to_string_lossy().into_owned();
    let filtered = filter_spawn_tweaks(tweak, &workspace);
    ctx.argv.extend(filtered.argv);
    if let Some(cwd) = filtered.cwd {
        let p = std::path::PathBuf::from(&cwd);
        if p.starts_with(&ctx.workspace) || p.starts_with(prefix.root()) {
            ctx.cwd = p;
        }
    }
    for (k, v) in filtered.env {
        ctx.env.insert(k, v);
    }
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::EngineBinary;
    use progressive_lsp_core::{FakeClock, LanguageId, PackageId};
    use progressive_lsp_script::{FakeEngineFactory, ScriptDecision, ScriptHost};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn ctx() -> SpawnCtx {
        SpawnCtx {
            workspace: std::path::PathBuf::from("/ws"),
            language: LanguageId::new("python"),
            package: PackageId::new("pkg"),
            argv: vec!["ty".into()],
            cwd: std::path::PathBuf::from("/ws"),
            env: BTreeMap::new(),
            binary: EngineBinary {
                pack_name: "python".into(),
                path: std::path::PathBuf::from("/p/engines/python/ty"),
                sha256: [0; 32],
            },
        }
    }

    #[test]
    fn noop_proceeds_abort_skips() {
        assert_eq!(
            NoopHooks.decide_spawn("python", &ctx()),
            SpawnHookResult::Proceed(SpawnTweak::default())
        );
        NoopHooks.notify_tier_ready("python", "pkg");
        let abort = AbortSpawnHooks {
            message: "skip-ty".into(),
        };
        assert_eq!(
            abort.decide_spawn("python", &ctx()),
            SpawnHookResult::Abort("skip-ty".into())
        );
    }

    #[test]
    fn script_bridge_abort_and_tweaks() {
        let mut host = ScriptHost::new(
            Box::new(FakeEngineFactory {
                decision: ScriptDecision::Abort("skip-ty".into()),
                fail_create: None,
            }),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        host.load("ok", "fake").unwrap();
        let bridge = ScriptHookBridge::new(host);
        assert_eq!(
            bridge.decide_spawn("python", &ctx()),
            SpawnHookResult::Abort("skip-ty".into())
        );
        bridge.notify_tier_ready("python", "pkg");

        let mut tweaks = ScriptHost::new(
            Box::new(FakeEngineFactory {
                decision: ScriptDecision::TweakSpawn(SpawnTweak {
                    argv: vec!["--stdio".into(), "--rce".into()],
                    cwd: Some("/ws".into()),
                    env: vec![
                        ("RUST_LOG".into(), "info".into()),
                        ("LD_PRELOAD".into(), "x".into()),
                    ],
                }),
                fail_create: None,
            }),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        tweaks.load("ok", "fake").unwrap();
        let bridge = ScriptHookBridge::new(tweaks);
        match bridge.decide_spawn("python", &ctx()) {
            SpawnHookResult::Proceed(t) => {
                assert_eq!(t.argv, vec!["--stdio".to_string()]);
                assert_eq!(t.env, vec![("RUST_LOG".into(), "info".into())]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn apply_tweaks_keeps_allowlisted_only() {
        let prefix = PrefixLayout::from_path("/pfx");
        let t = SpawnTweak {
            argv: vec!["--quiet".into()],
            cwd: Some("/ws/src".into()),
            env: vec![("TY_LOG".into(), "1".into())],
        };
        let out = apply_tweaks(ctx(), &t, &prefix);
        assert!(out.argv.contains(&"--quiet".to_string()));
        assert_eq!(out.cwd, std::path::PathBuf::from("/ws/src"));
        assert_eq!(out.env.get("TY_LOG").map(String::as_str), Some("1"));
        let outside = SpawnTweak {
            argv: Vec::new(),
            cwd: Some("/etc".into()),
            env: Vec::new(),
        };
        let out = apply_tweaks(ctx(), &outside, &prefix);
        assert_eq!(out.cwd, std::path::PathBuf::from("/ws"));
    }
}
