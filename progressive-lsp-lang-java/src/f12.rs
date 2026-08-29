//! F12 across packages on T1. No host JDK.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use progressive_lsp_core::FileId;
    use progressive_lsp_index::{IndexService, SharedIndex};
    use progressive_lsp_plugin::LanguageFactory;
    use progressive_lsp_resolve::{
        Position, QueryKind, ResolveOutcome, ResolveQuery, Resolver, ResolverChain, SymbolKind,
    };
    use progressive_lsp_resolve::fake::{FakeResolver, NotReadyResolver};
    use progressive_lsp_workspace::{detect_workspace, MavenAdapter, WorkspaceSource};

    use crate::extract::JavaIndexer;
    use crate::factory::JavaLanguageFactory;
    use crate::tokens::{encode_lsp_data, tokens_from_tree};

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/java-multi")
    }

    fn index_fixture() -> (SharedIndex, PathBuf, PathBuf) {
        let root = fixture_root();
        let model = MavenAdapter
            .detect(&root)
            .or_else(|| detect_workspace(&root))
            .expect("java-multi fixture");
        assert!(model.packages.len() >= 2, "multi-package fixture");
        let mut svc = IndexService::new();
        let app = root.join("app/src/main/java/com/example/app/App.java");
        let lib = root.join("lib/src/main/java/com/example/lib/Lib.java");
        svc.open_buffer(&app);
        svc.index_text(&app, &std::fs::read_to_string(&app).unwrap(), &JavaIndexer, false);
        svc.index_text(&lib, &std::fs::read_to_string(&lib).unwrap(), &JavaIndexer, false);
        (SharedIndex::new(svc), app, lib)
    }

    fn line_col(src: &str, needle: &str) -> Position {
        let byte = src.find(needle).expect(needle);
        let mut line = 0u32;
        let mut col = 0u32;
        for (i, ch) in src.char_indices() {
            if i == byte {
                return Position::new(line, col);
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        panic!("{needle}");
    }

    #[test]
    fn f12_definition_across_packages() {
        let (index, app, lib) = index_fixture();
        let app_src = std::fs::read_to_string(&app).unwrap();
        let pos = line_col(&app_src, "greet");
        let factory = JavaLanguageFactory::with_index(Arc::new(index));
        let chain = factory.resolver_chain();
        let q = ResolveQuery::new(
            FileId::new(app.to_string_lossy().as_ref()),
            pos,
            QueryKind::Definition,
        );
        match chain.resolve(&q) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier.as_str(), "syntax");
                assert!(
                    r.locations.iter().any(|l| l.uri.contains("Lib.java")),
                    "F12 should reach Lib.java, got {:?}",
                    r.locations
                );
                let _ = lib;
            }
            ResolveOutcome::NotReady => panic!("T1 ready"),
        }
    }

    #[test]
    fn hover_signature_name_and_arity() {
        let (index, _app, lib) = index_fixture();
        let src = std::fs::read_to_string(&lib).unwrap();
        let pos = line_col(&src, "greet");
        let factory = JavaLanguageFactory::with_index(Arc::new(index));
        let q = ResolveQuery::new(
            FileId::new(lib.to_string_lossy().as_ref()),
            pos,
            QueryKind::Hover,
        );
        match factory.resolver_chain().resolve(&q) {
            ResolveOutcome::Ready(r) => {
                let h = r.hover.expect("hover");
                assert_eq!(h.name, "greet");
                assert_eq!(h.arity, Some(1));
                assert_eq!(h.signature(), "greet(1)");
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
    }

    #[test]
    fn document_workspace_symbol_and_tokens() {
        let (index, app, lib) = index_fixture();
        let factory = JavaLanguageFactory::with_index(Arc::new(index.clone()));
        let doc = ResolveQuery::new(
            FileId::new(app.to_string_lossy().as_ref()),
            Position::default(),
            QueryKind::DocumentSymbol,
        );
        match factory.resolver_chain().resolve(&doc) {
            ResolveOutcome::Ready(r) => {
                assert!(r.symbols.iter().any(|s| s.name == "App"));
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::workspace_symbol("Lib")) {
            ResolveOutcome::Ready(r) => {
                assert!(r.locations.iter().any(|l| l.uri.contains("Lib.java")));
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
        let src = std::fs::read_to_string(&lib).unwrap();
        let mut p = tree_sitter::Parser::new();
        p.set_language(&crate::tree_sitter_language()).unwrap();
        let tree = p.parse(&src, None).unwrap();
        let data = encode_lsp_data(&tokens_from_tree(&src, &tree));
        assert!(!data.is_empty());
        assert_eq!(data.len() % 5, 0);
        let _ = SymbolKind::Class;
    }

    #[test]
    fn t3_not_ready_does_not_drop_t2_before_java_t1() {
        let (index, app, _) = index_fixture();
        let t2 = FakeResolver::graph("t2").with_location(progressive_lsp_resolve::LspLocation::new(
            "file:///t2",
            progressive_lsp_resolve::Range::default(),
            progressive_lsp_core::Tier::Graph,
        ));
        let chain = ResolverChain::new(vec![
            Box::new(NotReadyResolver::new(
                progressive_lsp_core::LanguageId::new("java"),
                progressive_lsp_core::PackageId::new("app"),
            )),
            Box::new(t2),
            Box::new(progressive_lsp_resolve::TreeSitterResolver::new(Arc::new(index))),
        ]);
        let q = ResolveQuery::new(
            FileId::new(app.to_string_lossy().as_ref()),
            Position::new(0, 0),
            QueryKind::Definition,
        );
        match chain.resolve(&q) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, progressive_lsp_core::Tier::Graph);
                assert_eq!(r.locations[0].uri, "file:///t2");
            }
            ResolveOutcome::NotReady => panic!("T2 must win"),
        }
    }
}
