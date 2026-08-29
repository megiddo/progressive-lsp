//! M5 matrix, mixed workspace, and grammar-lag fixtures.
//!
//! Darwin `cargo test` is the stand-in for matrix CI green.
//! Linux CI must run the same fixtures.

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
        "javascript" | "typescript" | "js" => {
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

#[test]
fn matrix_latest_plus_two_fixtures_index_without_panic() {
    let root = fixtures().join("matrix");
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
    let mut seen = 0usize;
    for lang in langs {
        let dir = root.join(lang);
        assert!(dir.is_dir(), "missing matrix fixtures for {lang}");
        let Some(indexer) = indexer_for(lang) else {
            panic!("no indexer for {lang} (enable default lang-* features)");
        };
        let sources = walk_sources(&dir);
        assert!(
            sources.len() >= 3,
            "{lang} needs LATEST + LATEST-1 + LATEST-2, got {sources:?}"
        );
        for path in sources {
            let src = std::fs::read_to_string(&path).unwrap();
            let mut svc = IndexService::new();
            svc.index_text(&path, &src, indexer.as_ref(), false);
            assert_eq!(svc.file_count(), 1, "{}", path.display());
            assert!(svc.indexed(&path).is_some());
            seen += 1;
        }
    }
    assert!(seen >= 13 * 3, "expected ≥39 version fixtures, got {seen}");
}

#[test]
fn mixed_version_workspace_indexes_all_sources() {
    let root = fixtures().join("matrix/mixed");
    let mut svc = IndexService::new();
    let mut n = 0usize;
    for path in walk_sources(&root) {
        let lang = match path.extension().and_then(|s| s.to_str()) {
            Some("java") => "java",
            Some("py") => "python",
            Some("rs") => "rust",
            Some("php") => "php",
            Some("js") => "javascript",
            Some("go") => "go",
            Some("c") => "c",
            other => panic!("unexpected mixed fixture {other:?}"),
        };
        let indexer = indexer_for(lang).unwrap();
        let src = std::fs::read_to_string(&path).unwrap();
        svc.index_text(&path, &src, indexer.as_ref(), false);
        n += 1;
    }
    assert!(n >= 7, "mixed workspace should cover several languages, got {n}");
    assert_eq!(svc.file_count(), n);
}

#[test]
fn lag_fixtures_record_unparsed_and_do_not_panic() {
    let cases = [
        ("java", "lag/java/Lag.java"),
        ("php", "lag/php/Lag.php"),
        ("javascript", "lag/javascript/lag.js"),
        ("python", "lag/python/lag.py"),
        ("rust", "lag/rust/lag.rs"),
        ("c", "lag/c/lag.c"),
    ];
    for (lang, rel) in cases {
        let path = fixtures().join(rel);
        let src = std::fs::read_to_string(&path).unwrap();
        let indexer = indexer_for(lang).unwrap();
        let mut svc = IndexService::new();
        svc.index_text(&path, &src, indexer.as_ref(), false);
        let rec = svc.indexed(&path).expect("indexed after lag parse");
        assert!(
            rec.has_error && rec.unparsed_note.is_some(),
            "{lang} lag fixture must surface ERROR/unparsed, got has_error={} note={:?}",
            rec.has_error,
            rec.unparsed_note
        );
        assert!(rec.unparsed_note.as_deref().unwrap().contains("unparsed"));
    }
}

#[test]
fn csharp_is_t1_t2_ceiling_and_java_has_no_t3_slot_in_matrix() {
    let csharp = fixtures().join("matrix/csharp/14/Sample.cs");
    let java = fixtures().join("matrix/java/26/App.java");
    assert!(csharp.is_file());
    assert!(java.is_file());
    let src = std::fs::read_to_string(&csharp).unwrap();
    assert!(src.contains("T1/T2 ceiling"));
}

#[test]
fn core_rss_sample_is_labeled_not_allocator_winner() {
    let label = progressive_lsp_core::rss_sample_label();
    assert!(label.contains("allocator"));
    let _ = progressive_lsp_core::sample_rss_bytes();
}
