//! Conformance dashboard: per language per tier pass % from fixtures, not invented 100%s.

use std::path::{Path, PathBuf};

use progressive_lsp_index::{IndexService, LanguageIndexer};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn indexer_for(lang: &str) -> Option<Box<dyn LanguageIndexer>> {
    match lang {
        #[cfg(feature = "lang-c")]
        "c" => Some(Box::new(progressive_lsp_lang_c::CIndexer)),
        #[cfg(feature = "lang-cpp")]
        "cpp" => Some(Box::new(progressive_lsp_lang_cpp::CppIndexer)),
        #[cfg(feature = "lang-csharp")]
        "csharp" => Some(Box::new(progressive_lsp_lang_csharp::CSharpIndexer)),
        #[cfg(feature = "lang-css")]
        "css" => Some(Box::new(progressive_lsp_lang_css::CssIndexer)),
        #[cfg(feature = "lang-go")]
        "go" => Some(Box::new(progressive_lsp_lang_go::GoIndexer)),
        #[cfg(feature = "lang-html")]
        "html" => Some(Box::new(progressive_lsp_lang_html::HtmlIndexer)),
        #[cfg(feature = "lang-java")]
        "java" => Some(Box::new(progressive_lsp_lang_java::JavaIndexer)),
        #[cfg(feature = "lang-javascript")]
        "javascript" | "typescript" => {
            Some(Box::new(progressive_lsp_lang_javascript::JavaScriptIndexer))
        }
        #[cfg(feature = "lang-php")]
        "php" => Some(Box::new(progressive_lsp_lang_php::PhpIndexer)),
        #[cfg(feature = "lang-python")]
        "python" => Some(Box::new(progressive_lsp_lang_python::PythonIndexer)),
        #[cfg(feature = "lang-rust")]
        "rust" => Some(Box::new(progressive_lsp_lang_rust::RustIndexer)),
        #[cfg(feature = "lang-zig")]
        "zig" => Some(Box::new(progressive_lsp_lang_zig::ZigIndexer)),
        _ => None,
    }
}

fn is_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some(
            "c" | "h"
                | "cc"
                | "cpp"
                | "cxx"
                | "hpp"
                | "cs"
                | "css"
                | "go"
                | "html"
                | "htm"
                | "java"
                | "js"
                | "ts"
                | "php"
                | "py"
                | "rs"
                | "zig"
        )
    )
}

fn walk_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                stack.push(p);
            } else if is_source(&p) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[derive(Clone, Debug)]
struct Row {
    language: &'static str,
    t1: Cell,
    t2: Cell,
    t3: Cell,
}

#[derive(Clone, Debug)]
enum Cell {
    Pct { pass: usize, total: usize },
    Na(&'static str),
}

impl Cell {
    fn render(&self) -> String {
        match self {
            Cell::Pct { pass, total } => {
                let pct = if *total == 0 {
                    0.0
                } else {
                    100.0 * (*pass as f64) / (*total as f64)
                };
                format!("{pass}/{total} ({pct:.0}%)")
            }
            Cell::Na(why) => format!("N/A ({why})"),
        }
    }

    fn pct(&self) -> Option<f64> {
        match self {
            Cell::Pct { pass, total } if *total > 0 => Some(100.0 * (*pass as f64) / (*total as f64)),
            _ => None,
        }
    }
}

fn t1_for(lang: &str) -> Cell {
    let Some(indexer) = indexer_for(lang) else {
        return Cell::Pct { pass: 0, total: 0 };
    };
    let dir = fixtures().join("matrix").join(lang);
    let sources = walk_sources(&dir);
    let total = sources.len();
    let mut pass = 0usize;
    for path in sources {
        let src = std::fs::read_to_string(&path).unwrap();
        let mut svc = IndexService::new();
        svc.index_text(&path, &src, indexer.as_ref(), false);
        if let Some(rec) = svc.indexed(&path) {
            if !rec.has_error {
                pass += 1;
            }
        }
    }
    Cell::Pct { pass, total }
}

fn t2_for(lang: &str, t1: &Cell) -> Cell {
    match lang {
        "rust" => Cell::Na("no dedicated Rust T2"),
        "python" => Cell::Na("optional TSG unused"),
        "html" | "css" => Cell::Na("no T2"),
        "c" | "cpp" => Cell::Na("brief gap"),
        "java" | "php" | "javascript" | "typescript" | "go" | "zig" | "csharp" => {
            let Cell::Pct { pass: _, total } = t1 else {
                return Cell::Pct { pass: 0, total: 0 };
            };
            // T2 heuristics need extracted symbols. Count the same matrix files
            // that T1 indexed cleanly *and* produced at least one symbol.
            let Some(indexer) = indexer_for(lang) else {
                return Cell::Pct { pass: 0, total: *total };
            };
            let dir = fixtures().join("matrix").join(lang);
            let mut pass = 0usize;
            for path in walk_sources(&dir) {
                let src = std::fs::read_to_string(&path).unwrap();
                let mut svc = IndexService::new();
                svc.index_text(&path, &src, indexer.as_ref(), false);
                if svc.indexed(&path).map(|r| !r.has_error).unwrap_or(false)
                    && !svc.all_indexed_symbols().is_empty()
                {
                    pass += 1;
                }
            }
            Cell::Pct {
                pass,
                total: *total,
            }
        }
        _ => Cell::Na("unknown"),
    }
}

fn t3_for(lang: &str) -> Cell {
    match lang {
        "java" => Cell::Na("no T3 in v1"),
        "csharp" => Cell::Na("T1/T2 ceiling"),
        _ => {
            // Darwin host: xtask dist / install write pack stubs, not musl ELFs.
            // Supervisor is not ready; T3 typed queries are 0% here.
            // Linux CI with real packs is the place to raise these numbers.
            Cell::Pct { pass: 0, total: 1 }
        }
    }
}

fn rows() -> Vec<Row> {
    let langs = [
        "c",
        "cpp",
        "csharp",
        "css",
        "go",
        "html",
        "java",
        "javascript",
        "php",
        "python",
        "rust",
        "typescript",
        "zig",
    ];
    langs
        .into_iter()
        .map(|language| {
            let t1 = t1_for(language);
            let t2 = t2_for(language, &t1);
            let t3 = t3_for(language);
            Row {
                language,
                t1,
                t2,
                t3,
            }
        })
        .collect()
}

fn render(rows: &[Row]) -> String {
    let mut out = String::from(
        "# Conformance dashboard (v1)

Per language, per tier pass rates from `fixtures/matrix/` (LATEST+2) plus the
Darwin T3 reality: pack stubs are not musl ELFs, so T3 is **0%** unless a cell
is N/A. Numbers are computed by `tests/conformance.rs`. Do not invent 100%s.

C# is T1/T2 only. Java has no T3. Linux CI with real engine packs is the
place to re-score T3.

| Language | T1 (syntax) | T2 (heuristics) | T3 (types) |
|---|---|---|---|
",
    );
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            row.language,
            row.t1.render(),
            row.t2.render(),
            row.t3.render()
        ));
    }
    out.push_str(
        "\nGenerated by `cargo test --test conformance`. Linked from [docs/README.md](README.md).\n",
    );
    out
}

#[test]
fn conformance_dashboard_from_fixtures() {
    let rows = rows();
    let md = render(&rows);
    let dest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/conformance.md");
    std::fs::write(&dest, &md).unwrap();
    let java = rows.iter().find(|r| r.language == "java").unwrap();
    assert!(matches!(java.t3, Cell::Na(_)), "Java has no T3");
    let csharp = rows.iter().find(|r| r.language == "csharp").unwrap();
    assert!(matches!(csharp.t3, Cell::Na(_)), "C# T1/T2 ceiling");
    assert!(matches!(csharp.t2, Cell::Pct { .. }));
    for row in &rows {
        if let Some(pct) = row.t3.pct() {
            assert!(
                pct < 100.0,
                "{} T3 must not be an invented 100%, got {pct}",
                row.language
            );
        }
        if let Cell::Pct { total, .. } = row.t1 {
            assert!(total >= 3, "{} needs LATEST+2 fixtures", row.language);
        }
    }
    assert!(md.contains("C# is T1/T2 only"));
    assert!(md.contains("Java has no T3"));
    assert!(md.contains("0/1 (0%)"));
}
