//! `progressiveLsp` JSON on initialize and intelligence requests (SEAMS).

use progressive_lsp_core::Tier;
use progressive_lsp_resolve::ChainPolicy;
use serde_json::Value;

/// Session defaults from `initializationOptions.progressiveLsp`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressiveLspSessionOptions {
    pub chain_defaults: ChainPolicy,
    pub emit_result_meta: bool,
    pub emit_timing: bool,
}

/// Per-request slice of progressive options (chain + emit overrides).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressiveLspRequestOptions {
    pub chain: ChainPolicy,
    pub emit_result_meta: Option<bool>,
    pub emit_timing: Option<bool>,
}

pub fn chain_policy_from_value(v: &Value) -> ChainPolicy {
    let max_chain_iter = v
        .get("maxChainIter")
        .and_then(|n| n.as_u64())
        .and_then(|n| u8::try_from(n).ok());
    let max_tier = v
        .get("maxTier")
        .and_then(|t| t.as_str())
        .and_then(Tier::parse);
    ChainPolicy {
        max_chain_iter,
        max_tier,
    }
}

pub fn request_options_from_params(params: &Value) -> ProgressiveLspRequestOptions {
    let Some(v) = params.get("progressiveLsp") else {
        return ProgressiveLspRequestOptions::default();
    };
    ProgressiveLspRequestOptions {
        chain: chain_policy_from_value(v),
        emit_result_meta: v.get("emitResultMeta").and_then(|b| b.as_bool()),
        emit_timing: v.get("emitTiming").and_then(|b| b.as_bool()),
    }
}

pub fn session_options_from_initialize(params: &Value) -> ProgressiveLspSessionOptions {
    let Some(v) = params
        .get("initializationOptions")
        .and_then(|o| o.get("progressiveLsp"))
    else {
        return ProgressiveLspSessionOptions::default();
    };
    ProgressiveLspSessionOptions {
        chain_defaults: chain_policy_from_value(v),
        emit_result_meta: v
            .get("emitResultMeta")
            .and_then(|b| b.as_bool())
            .unwrap_or(false),
        emit_timing: v.get("emitTiming").and_then(|b| b.as_bool()).unwrap_or(false),
    }
}

pub fn test_chain_policy_from_env() -> ChainPolicy {
    let max_chain_iter = std::env::var("PROGRESSIVE_LSP_TEST_MAX_CHAIN_ITER")
        .ok()
        .and_then(|s| s.parse().ok());
    let max_tier = std::env::var("PROGRESSIVE_LSP_TEST_MAX_TIER")
        .ok()
        .and_then(|s| Tier::parse(&s));
    ChainPolicy {
        max_chain_iter,
        max_tier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_chain_and_emit_fields() {
        let params = json!({
            "progressiveLsp": {
                "maxChainIter": 2,
                "maxTier": "graph",
                "emitResultMeta": true
            }
        });
        let req = request_options_from_params(&params);
        assert_eq!(req.chain.max_chain_iter, Some(2));
        assert_eq!(req.chain.max_tier, Some(Tier::Graph));
        assert_eq!(req.emit_result_meta, Some(true));

        let init = json!({
            "initializationOptions": {
                "progressiveLsp": {
                    "emitResultMeta": true,
                    "emitTiming": true,
                    "maxChainIter": 1
                }
            }
        });
        let sess = session_options_from_initialize(&init);
        assert!(sess.emit_result_meta);
        assert!(sess.emit_timing);
        assert_eq!(sess.chain_defaults.max_chain_iter, Some(1));
    }
}
