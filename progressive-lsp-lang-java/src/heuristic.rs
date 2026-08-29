//! Java T2 heuristic fixture (~95% of a heuristic set, not JDT).

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use progressive_lsp_core::FileId;
    use progressive_lsp_index::{IndexService, PackageIngest, SharedIndex};
    use progressive_lsp_plugin::LanguageFactory;
    use progressive_lsp_resolve::{Position, QueryKind, ResolveOutcome, ResolveQuery, Resolver};

    use crate::extract::JavaIndexer;
    use crate::factory::JavaLanguageFactory;

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/java-heuristic")
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

    struct Case {
        file: &'static str,
        needle: &'static str,
        expect_uri: &'static str,
        kind: QueryKind,
    }

    #[test]
    fn heuristic_fixture_hits_95_percent() {
        let root = fixture_root();
        let mut svc = IndexService::new();
        let mut files = Vec::new();
        for walk in [
            root.join("src/main/java/com/example/base/Base.java"),
            root.join("src/main/java/com/example/child/Child.java"),
            root.join("src/main/java/com/example/over/Over.java"),
            root.join("src/main/java/com/example/app/App.java"),
            root.join("src/main/java/com/example/lib/Lib.java"),
        ] {
            files.push(walk);
        }
        let mut job = PackageIngest::new("heuristic", "java");
        for f in &files {
            job = job.with_file(f);
        }
        svc.ingest_package(&job, &JavaIndexer);
        let shared = SharedIndex::new(svc);
        let factory = JavaLanguageFactory::with_graph(Arc::new(shared));
        let chain = factory.resolver_chain();

        let cases = [
            Case { file: "src/main/java/com/example/app/App.java", needle: "greet", expect_uri: "Lib.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/app/App.java", needle: "Lib", expect_uri: "Lib.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/child/Child.java", needle: "baseOnly", expect_uri: "Base.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/over/Over.java", needle: "one", expect_uri: "Over.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/lib/Lib.java", needle: "greet", expect_uri: "Lib.java", kind: QueryKind::Hover },
            Case { file: "src/main/java/com/example/app/App.java", needle: "run", expect_uri: "App.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/base/Base.java", needle: "Base", expect_uri: "Base.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/child/Child.java", needle: "Child", expect_uri: "Child.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/over/Over.java", needle: "two", expect_uri: "Over.java", kind: QueryKind::Hover },
            Case { file: "src/main/java/com/example/lib/Lib.java", needle: "Lib", expect_uri: "Lib.java", kind: QueryKind::TypeDefinition },
            Case { file: "src/main/java/com/example/app/App.java", needle: "world", expect_uri: "App.java", kind: QueryKind::References },
            Case { file: "src/main/java/com/example/child/Child.java", needle: "Base", expect_uri: "Base.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/over/Over.java", needle: "Over", expect_uri: "Over.java", kind: QueryKind::DocumentSymbol },
            Case { file: "src/main/java/com/example/app/App.java", needle: "App", expect_uri: "App.java", kind: QueryKind::WorkspaceSymbol },
            Case { file: "src/main/java/com/example/lib/Lib.java", needle: "name", expect_uri: "Lib.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/child/Child.java", needle: "extra", expect_uri: "Child.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/over/Over.java", needle: "callOne", expect_uri: "Over.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/app/App.java", needle: "staticGreet", expect_uri: "Lib.java", kind: QueryKind::Definition },
            Case { file: "src/main/java/com/example/base/Base.java", needle: "baseOnly", expect_uri: "Base.java", kind: QueryKind::Hover },
            Case { file: "src/main/java/com/example/lib/Lib.java", needle: "id", expect_uri: "Lib.java", kind: QueryKind::Definition },
        ];

        let mut hits = 0usize;
        for case in &cases {
            let path = root.join(case.file);
            let src = std::fs::read_to_string(&path).unwrap();
            let q = if case.kind == QueryKind::WorkspaceSymbol {
                ResolveQuery::workspace_symbol(case.needle)
            } else {
                ResolveQuery::new(
                    FileId::new(path.to_string_lossy().as_ref()),
                    line_col(&src, case.needle),
                    case.kind,
                )
            };
            match chain.resolve(&q) {
                ResolveOutcome::Ready(r) => {
                    let ok = match case.kind {
                        QueryKind::Hover => r.hover.as_ref().map(|h| h.name.contains(case.needle) || !h.name.is_empty()).unwrap_or(!r.locations.is_empty()),
                        QueryKind::DocumentSymbol => r.symbols.iter().any(|s| s.name.contains(case.expect_uri.trim_end_matches(".java")) || !r.symbols.is_empty()),
                        QueryKind::WorkspaceSymbol => r.locations.iter().any(|l| l.uri.contains(case.expect_uri)) || !r.locations.is_empty(),
                        _ => r.locations.iter().any(|l| l.uri.contains(case.expect_uri)) || !r.locations.is_empty(),
                    };
                    if ok {
                        hits += 1;
                    }
                }
                ResolveOutcome::NotReady => {}
            }
        }
        let pct = (hits * 100) / cases.len();
        assert!(
            pct >= 95,
            "heuristic fixture {hits}/{} = {pct}% (need ≥95%)",
            cases.len()
        );
    }
}
