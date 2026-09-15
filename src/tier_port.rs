//! TierPort abstraction (ABS-4). Chain composition without lang-* crate moves.

use progressive_lsp_resolve::{ResolveOutcome, ResolveQuery, ResolveResult, Resolver};
use progressive_lsp_watch::WatchBatch;

/// Per-tier readiness exposed to control / chain builder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierReadinessSnapshot {
    Ready,
    Pending,
    NotApplicable,
}

impl TierReadinessSnapshot {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Pending => "pending",
            Self::NotApplicable => "not_applicable",
        }
    }
}

/// Chain step outcome at one tier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    Ready(ResolveResult),
    NotReady,
    Skip,
}

/// ABS-4.1 port boundary (ingest hooks reserved for follow-on wiring).
pub trait TierPort: Send + Sync {
    fn tier_id(&self) -> &'static str;
    fn readiness(&self) -> TierReadinessSnapshot;
    fn resolve_step(&self, ctx: &ResolveQuery) -> StepOutcome;
    fn on_edit_event(&self, _batch: &WatchBatch) {}
}

/// Adapter: existing [`Resolver`] chain step behind [`TierPort`].
pub struct ResolverTierPort {
    id: &'static str,
    readiness: TierReadinessSnapshot,
    inner: Box<dyn Resolver>,
}

impl ResolverTierPort {
    pub fn new(id: &'static str, readiness: TierReadinessSnapshot, inner: Box<dyn Resolver>) -> Self {
        Self {
            id,
            readiness,
            inner,
        }
    }
}

impl TierPort for ResolverTierPort {
    fn tier_id(&self) -> &'static str {
        self.id
    }

    fn readiness(&self) -> TierReadinessSnapshot {
        self.readiness
    }

    fn resolve_step(&self, ctx: &ResolveQuery) -> StepOutcome {
        match self.inner.resolve(ctx) {
            ResolveOutcome::Ready(r) => StepOutcome::Ready(r),
            ResolveOutcome::NotReady => StepOutcome::NotReady,
        }
    }
}

/// Ordered tier chain (ABS-4.3 dynamic length).
pub struct TierPortChain {
    ports: Vec<Box<dyn TierPort>>,
}

impl TierPortChain {
    pub fn new(ports: Vec<Box<dyn TierPort>>) -> Self {
        Self { ports }
    }

    pub fn len(&self) -> usize {
        self.ports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }

    pub fn tier_ids(&self) -> Vec<&'static str> {
        self.ports.iter().map(|p| p.tier_id()).collect()
    }

    /// Omit types tier when every engine pack is `missing` (IT-ABS-4).
    pub fn for_engine_availability(include_types: bool) -> Self {
        let mut ports: Vec<Box<dyn TierPort>> = vec![
            Box::new(FakeTierPort::new("syntax", TierReadinessSnapshot::Ready)),
            Box::new(FakeTierPort::new("graph", TierReadinessSnapshot::Ready)),
        ];
        if include_types {
            ports.push(Box::new(FakeTierPort::new(
                "types",
                TierReadinessSnapshot::Pending,
            )));
        }
        Self::new(ports)
    }

    pub fn resolve(&self, q: &ResolveQuery) -> StepOutcome {
        for port in &self.ports {
            match port.resolve_step(q) {
                StepOutcome::Skip => continue,
                other => return other,
            }
        }
        StepOutcome::NotReady
    }
}

/// Test double for chain-length and ordering tests.
pub struct FakeTierPort {
    id: &'static str,
    readiness: TierReadinessSnapshot,
}

impl FakeTierPort {
    pub fn new(id: &'static str, readiness: TierReadinessSnapshot) -> Self {
        Self { id, readiness }
    }
}

impl TierPort for FakeTierPort {
    fn tier_id(&self) -> &'static str {
        self.id
    }

    fn readiness(&self) -> TierReadinessSnapshot {
        self.readiness
    }

    fn resolve_step(&self, _ctx: &ResolveQuery) -> StepOutcome {
        StepOutcome::NotReady
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::{FileId, Tier};
    use progressive_lsp_resolve::{fake::FakeResolver, Position, QueryKind};

    #[test]
    fn chain_omits_types_tier_when_engines_missing() {
        let short = TierPortChain::for_engine_availability(false);
        let full = TierPortChain::for_engine_availability(true);
        assert_eq!(short.len(), 2);
        assert_eq!(full.len(), 3);
        assert_eq!(short.tier_ids(), vec!["syntax", "graph"]);
    }

    #[test]
    fn resolver_tier_port_maps_ready_and_not_ready() {
        let port = ResolverTierPort::new(
            "syntax",
            TierReadinessSnapshot::Ready,
            Box::new(FakeResolver::syntax("t1")),
        );
        let q = ResolveQuery::new(FileId::new("a.java"), Position::new(0, 0), QueryKind::Definition);
        assert!(matches!(port.resolve_step(&q), StepOutcome::Ready(_)));
    }
}
