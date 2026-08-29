//! Test doubles. Same `Resolver` trait as production.

use progressive_lsp_core::{EngineNotReady, LanguageId, PackageId, Tier};

use crate::query::{
    LspLocation, QueryKind, ResolveOutcome, ResolveQuery, ResolveResult,
};
use crate::Resolver;

/// T2 stand-in. Always `Ready` with a configured tier.
#[derive(Clone, Debug)]
pub struct FakeResolver {
    pub label: String,
    pub tier: Tier,
    pub locations: Vec<LspLocation>,
    pub handled: Option<QueryKind>,
}

impl FakeResolver {
    pub fn graph(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            tier: Tier::Graph,
            locations: Vec::new(),
            handled: None,
        }
    }

    pub fn syntax(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            tier: Tier::Syntax,
            locations: Vec::new(),
            handled: None,
        }
    }

    pub fn types(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            tier: Tier::Types,
            locations: Vec::new(),
            handled: None,
        }
    }

    pub fn with_location(mut self, loc: LspLocation) -> Self {
        self.locations.push(loc);
        self
    }

    pub fn only_kind(mut self, kind: QueryKind) -> Self {
        self.handled = Some(kind);
        self
    }
}

impl Resolver for FakeResolver {
    fn resolve(&self, q: &ResolveQuery) -> ResolveOutcome {
        if let Some(kind) = self.handled {
            if kind != q.kind {
                return ResolveOutcome::NotReady;
            }
        }
        ResolveOutcome::Ready(ResolveResult::locations(self.tier, self.locations.clone()))
    }
}

/// T3 stand-in that is never ready. Chain must continue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotReadyResolver {
    pub error: EngineNotReady,
}

impl NotReadyResolver {
    pub fn new(language: LanguageId, package: PackageId) -> Self {
        Self {
            error: EngineNotReady::new(language, package),
        }
    }
}

impl Resolver for NotReadyResolver {
    fn resolve(&self, _q: &ResolveQuery) -> ResolveOutcome {
        ResolveOutcome::NotReady
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{Position, Range};
    use progressive_lsp_core::FileId;

    fn q(kind: QueryKind) -> ResolveQuery {
        ResolveQuery::new(FileId::new("f"), Position::default(), kind)
    }

    #[test]
    fn fake_ready_uses_configured_tier_and_locations() {
        let r = FakeResolver::graph("g").with_location(LspLocation::new(
            "u",
            Range::default(),
            Tier::Graph,
        ));
        assert_eq!(r.label, "g");
        match r.resolve(&q(QueryKind::Definition)) {
            ResolveOutcome::Ready(res) => {
                assert_eq!(res.tier, Tier::Graph);
                assert_eq!(res.locations.len(), 1);
            }
            ResolveOutcome::NotReady => panic!("fake graph must be ready"),
        }
        let s = FakeResolver::syntax("s");
        match s.resolve(&q(QueryKind::Hover)) {
            ResolveOutcome::Ready(res) => assert_eq!(res.tier, Tier::Syntax),
            ResolveOutcome::NotReady => panic!("syntax fake"),
        }
        let t = FakeResolver::types("t");
        match t.resolve(&q(QueryKind::References)) {
            ResolveOutcome::Ready(res) => assert_eq!(res.tier, Tier::Types),
            ResolveOutcome::NotReady => panic!("types fake"),
        }
    }

    #[test]
    fn fake_only_kind_declines_other_queries() {
        let r = FakeResolver::graph("g").only_kind(QueryKind::Definition);
        assert!(r.resolve(&q(QueryKind::Definition)).is_ready());
        assert!(!r.resolve(&q(QueryKind::References)).is_ready());
    }

    #[test]
    fn not_ready_exposes_engine_error_and_skips() {
        let r = NotReadyResolver::new(LanguageId::new("java"), PackageId::new("p"));
        assert_eq!(r.error.language.as_str(), "java");
        assert_eq!(r.error.package.as_str(), "p");
        assert!(!r.resolve(&q(QueryKind::Definition)).is_ready());
        assert_eq!(
            r.error.to_string(),
            "engine not ready for java/p"
        );
    }
}
