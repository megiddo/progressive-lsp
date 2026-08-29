//! Optional T2 Strategy slot. Java TSG is not shipped (see language-matrix.md).

use crate::query::{ResolveOutcome, ResolveQuery};
use crate::Resolver;

/// Stack-graphs Strategy. Always [`ResolveOutcome::NotReady`] unless a language
/// binds a winning TSG backend. Heuristics remain the default.
pub struct StackGraphResolver {
    pub label: String,
}

impl StackGraphResolver {
    pub fn unused() -> Self {
        Self {
            label: "unused".into(),
        }
    }
}

impl Resolver for StackGraphResolver {
    fn resolve(&self, _q: &ResolveQuery) -> ResolveOutcome {
        ResolveOutcome::NotReady
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{Position, QueryKind};
    use crate::Resolver;
    use progressive_lsp_core::FileId;

    #[test]
    fn unused_slot_is_not_ready_and_does_not_replace_heuristics() {
        let r = StackGraphResolver::unused();
        assert_eq!(r.label, "unused");
        let q = ResolveQuery::new(FileId::new("A.java"), Position::default(), QueryKind::Definition);
        assert!(!r.resolve(&q).is_ready());
    }
}
