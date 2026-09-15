//! Static tier registry + control DTO mapping (ABS-3).

use progressive_lsp_control::{EngineStatusRow, IngestState, TierCapabilityRow};
use progressive_lsp_core::{PackageId, PrefixLayout, Tier, TIER_REGISTRY};
use progressive_lsp_engine::{discover_pack, EngineSupervisor};

use crate::WorkspaceSession;

pub fn index_engine_rows(
    sup: &EngineSupervisor,
    layout: &PrefixLayout,
    package: &PackageId,
) -> Vec<EngineStatusRow> {
    sup.registered_packs()
        .into_iter()
        .map(|(pack, language)| {
            let (state, detail) = if sup.is_ready(&language, package) {
                ("ready".into(), String::new())
            } else if let Some(err) = sup.last_error(&pack) {
                ("error".into(), err.to_string())
            } else {
                match discover_pack(layout, &pack) {
                    Ok(_) => ("pending".into(), "discovered; waiting for engine".into()),
                    Err(e) => ("missing".into(), e.to_string()),
                }
            };
            EngineStatusRow {
                language: language.as_str().to_string(),
                pack,
                state,
                detail,
            }
        })
        .collect()
}

pub fn tier_capability_rows(
    session: &WorkspaceSession,
    supervisor: Option<&EngineSupervisor>,
    layout: &PrefixLayout,
    package_id: &str,
) -> Vec<TierCapabilityRow> {
    let ingest = session.ingest_state();
    let observed = session.package_tier(package_id);
    let pkg = PackageId::new(package_id);
    let engines = supervisor
        .map(|sup| index_engine_rows(sup, layout, &pkg))
        .unwrap_or_default();
    let types_missing = types_tier_missing(&engines);

    let mut rows: Vec<TierCapabilityRow> = TIER_REGISTRY
        .iter()
        .copied()
        .map(|desc| TierCapabilityRow {
            tier_id: desc.id.to_string(),
            latency_class: desc.latency.as_str().to_string(),
            quality_class: desc.quality.as_str().to_string(),
            query_kinds: desc.query_kinds.iter().map(|s| (*s).to_string()).collect(),
            readiness: tier_readiness(desc.id, ingest, observed, types_missing, &engines)
                .as_str()
                .to_string(),
        })
        .collect();
    rows.sort_by(|a, b| {
        let da = TIER_REGISTRY
            .iter()
            .find(|d| d.id == a.tier_id.as_str())
            .copied()
            .unwrap_or(TIER_REGISTRY[0]);
        let db = TIER_REGISTRY
            .iter()
            .find(|d| d.id == b.tier_id.as_str())
            .copied()
            .unwrap_or(TIER_REGISTRY[0]);
        da.cmp(&db)
    });
    rows
}

fn types_tier_missing(engines: &[EngineStatusRow]) -> bool {
    !engines.is_empty() && engines.iter().all(|e| e.state == "missing")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TierReadiness {
    Ready,
    Pending,
    NotApplicable,
}

impl TierReadiness {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Pending => "pending",
            Self::NotApplicable => "not_applicable",
        }
    }
}

fn tier_readiness(
    tier_id: &str,
    ingest: IngestState,
    observed: Option<Tier>,
    types_missing: bool,
    engines: &[EngineStatusRow],
) -> TierReadiness {
    match tier_id {
        "syntax" => {
            if ingest.is_done() {
                TierReadiness::Ready
            } else {
                TierReadiness::Pending
            }
        }
        "graph" => {
            if matches!(observed, Some(Tier::Graph) | Some(Tier::Types)) && ingest.is_done() {
                TierReadiness::Ready
            } else {
                TierReadiness::Pending
            }
        }
        "types" => {
            if types_missing {
                return TierReadiness::NotApplicable;
            }
            if matches!(observed, Some(Tier::Types)) {
                TierReadiness::Ready
            } else if engines.iter().any(|e| e.state == "ready") {
                TierReadiness::Pending
            } else {
                TierReadiness::Pending
            }
        }
        _ => TierReadiness::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::PrefixLayout;

    #[test]
    fn registry_rows_sorted_syntax_graph_types() {
        let layout = PrefixLayout::from_path(tempfile::tempdir().unwrap().path());
        let session = WorkspaceSession::with_prefix_and_t2_log(
            &layout,
            progressive_lsp_core::T2Backend::Heuristic,
            std::sync::Arc::new(progressive_lsp_core::NullLog),
        );
        let rows = tier_capability_rows(&session, None, &layout, "pkg");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].tier_id, "syntax");
        assert_eq!(rows[1].tier_id, "graph");
        assert_eq!(rows[2].tier_id, "types");
    }
}
