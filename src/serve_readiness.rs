//! Serve-side readiness FSM (ADR 002, ABS-1.1).

use progressive_lsp_control::IngestState;
use progressive_lsp_types_cache::CacheServeState;

/// Composition-root view of workspace + query-key readiness for control observers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServeReadiness {
    pub workspace_ingest: IngestState,
}

impl ServeReadiness {
    pub fn new(ingest: IngestState) -> Self {
        Self {
            workspace_ingest: ingest,
        }
    }

    /// T3′ is pending on the mux path when the types step stopped with miss/inflight.
    pub fn query_pending_at_types(cache_state: CacheServeState, types_tier_active: bool) -> bool {
        types_tier_active
            && matches!(
                cache_state,
                CacheServeState::Miss | CacheServeState::Inflight
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_control::IngestState;

    #[test]
    fn query_pending_when_types_miss_or_inflight() {
        assert!(ServeReadiness::query_pending_at_types(
            CacheServeState::Miss,
            true
        ));
        assert!(ServeReadiness::query_pending_at_types(
            CacheServeState::Inflight,
            true
        ));
        assert!(!ServeReadiness::query_pending_at_types(
            CacheServeState::Hit,
            true
        ));
        assert!(!ServeReadiness::query_pending_at_types(
            CacheServeState::Miss,
            false
        ));
        let snap = ServeReadiness::new(IngestState::Running);
        assert_eq!(snap.workspace_ingest, IngestState::Running);
    }
}
