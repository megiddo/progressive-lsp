//! Static tier metadata (ABS-3). Sort key: latency → quality.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LatencyClass {
    Low = 0,
    Medium = 1,
    High = 2,
}

impl LatencyClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QualityClass {
    Syntax = 0,
    HeuristicRefs = 1,
    EngineBacked = 2,
}

impl QualityClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::HeuristicRefs => "heuristic_refs",
            Self::EngineBacked => "engine_backed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierDescriptor {
    pub id: &'static str,
    pub latency: LatencyClass,
    pub quality: QualityClass,
    pub query_kinds: &'static [&'static str],
}

impl PartialOrd for TierDescriptor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TierDescriptor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.latency
            .cmp(&other.latency)
            .then_with(|| self.quality.cmp(&other.quality))
            .then_with(|| self.id.cmp(other.id))
    }
}

pub const DISCOVER_KINDS: &[&str] = &[
    "definition",
    "implementation",
    "references",
    "typeDefinition",
];

pub static TIER_REGISTRY: &[TierDescriptor] = &[
    TierDescriptor {
        id: "syntax",
        latency: LatencyClass::Low,
        quality: QualityClass::Syntax,
        query_kinds: DISCOVER_KINDS,
    },
    TierDescriptor {
        id: "graph",
        latency: LatencyClass::Medium,
        quality: QualityClass::HeuristicRefs,
        query_kinds: DISCOVER_KINDS,
    },
    TierDescriptor {
        id: "types",
        latency: LatencyClass::High,
        quality: QualityClass::EngineBacked,
        query_kinds: DISCOVER_KINDS,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_sorts_latency_then_quality() {
        let mut tiers: Vec<_> = TIER_REGISTRY.iter().copied().collect();
        tiers.sort();
        assert_eq!(tiers[0].id, "syntax");
        assert_eq!(tiers[1].id, "graph");
        assert_eq!(tiers[2].id, "types");
    }
}
