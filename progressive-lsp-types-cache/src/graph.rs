//! Relation graph slice (Layer B). Language-agnostic edge schema.

use std::collections::HashMap;
use std::sync::Mutex;

use progressive_lsp_core::FileId;
use progressive_lsp_resolve::{LspLocation, QueryKind};

use crate::key::CacheGeneration;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GraphEdge {
    pub kind: QueryKind,
    pub from_file: FileId,
    pub to_uri: String,
}

#[derive(Default)]
pub struct RelationGraph {
    edges: Mutex<HashMap<FileId, Vec<GraphEdge>>>,
}

impl RelationGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_from_locations(
        &self,
        kind: QueryKind,
        from_file: &FileId,
        locations: &[LspLocation],
    ) {
        if locations.is_empty() {
            return;
        }
        let mut map = self.edges.lock().expect("graph");
        let slot = map.entry(from_file.clone()).or_default();
        for loc in locations {
            let edge = GraphEdge {
                kind,
                from_file: from_file.clone(),
                to_uri: loc.uri.clone(),
            };
            if !slot.contains(&edge) {
                slot.push(edge);
            }
        }
    }

    pub fn edges_for(&self, file: &FileId) -> Vec<GraphEdge> {
        self.edges
            .lock()
            .expect("graph")
            .get(file)
            .cloned()
            .unwrap_or_default()
    }

    pub fn invalidate_file(&self, file: &FileId, _generation: CacheGeneration) {
        self.edges.lock().expect("graph").remove(file);
    }

    pub fn len(&self) -> usize {
        self.edges
            .lock()
            .expect("graph")
            .values()
            .map(|v| v.len())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::Tier;
    use progressive_lsp_resolve::Range;

    #[test]
    fn def_edges_stored_and_cleared_on_invalidate() {
        let g = RelationGraph::new();
        let file = FileId::new("A.java");
        g.add_from_locations(
            QueryKind::Definition,
            &file,
            &[LspLocation::new("file:///B.java", Range::default(), Tier::Types)],
        );
        assert_eq!(g.len(), 1);
        g.invalidate_file(&file, CacheGeneration::new(1));
        assert_eq!(g.len(), 0);
    }
}
