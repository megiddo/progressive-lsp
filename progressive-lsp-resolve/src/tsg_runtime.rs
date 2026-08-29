//! Optional stack-graphs runtime. Compiled only with `--features t2-stack-graphs`.

use std::panic::{catch_unwind, AssertUnwindSafe};

use progressive_lsp_core::Tier;
use tree_sitter_stack_graphs::{NoCancellation, StackGraphLanguage, Variables, FILE_PATH_VAR};

use crate::query::{LspLocation, Position, QueryKind, Range, ResolveQuery, ResolveResult};

pub fn query(files: &[(String, String)], q: &ResolveQuery) -> Option<ResolveResult> {
    query_with_tsg(files, q, None)
}

pub fn query_with_tsg(
    files: &[(String, String)],
    q: &ResolveQuery,
    tsg_source: Option<&str>,
) -> Option<ResolveResult> {
    let tsg = tsg_source?;
    catch_unwind(AssertUnwindSafe(|| query_inner(files, q, tsg))).ok().flatten()
}

fn query_inner(
    files: &[(String, String)],
    q: &ResolveQuery,
    tsg: &str,
) -> Option<ResolveResult> {
    let language = tree_sitter_java::LANGUAGE.into();
    let sgl = StackGraphLanguage::from_source(language, "stack-graphs.tsg".into(), tsg).ok()?;
    let mut graph = stack_graphs::graph::StackGraph::new();
    for (path, source) in files {
        if path.ends_with(".tsg") {
            continue;
        }
        let mut globals = Variables::new();
        let _ = globals.add("PROJECT_NAME".into(), "bakeoff".into());
        let _ = globals.add(FILE_PATH_VAR.into(), path.as_str().into());
        let file = graph.get_or_create_file(path);
        let _ = catch_unwind(AssertUnwindSafe(|| {
            sgl.build_stack_graph_into(&mut graph, file, source, &globals, &NoCancellation)
        }));
    }
    let ident = ident_at_files(files, q)?;
    let mut locs = Vec::new();
    for handle in graph.iter_nodes() {
        let node = &graph[handle];
        if !node.is_definition() {
            continue;
        }
        let Some(sym) = node.symbol() else {
            continue;
        };
        if graph[sym] != ident {
            continue;
        }
        let Some(file) = node.file() else {
            continue;
        };
        let path = graph[file].name();
        let range = graph
            .source_info(handle)
            .map(|info| {
                Range::new(
                    Position::new(
                        info.span.start.line as u32,
                        info.span.start.column.grapheme_offset as u32,
                    ),
                    Position::new(
                        info.span.end.line as u32,
                        info.span.end.column.grapheme_offset as u32,
                    ),
                )
            })
            .unwrap_or_default();
        locs.push(LspLocation::new(format!("file://{path}"), range, Tier::Graph));
    }
    if locs.is_empty() {
        return None;
    }
    let hover = (q.kind == QueryKind::Hover).then(|| crate::query::Hover::named(ident, None));
    Some(ResolveResult {
        locations: locs,
        tier: Tier::Graph,
        hover,
        symbols: Vec::new(),
    })
}

fn ident_at_files(files: &[(String, String)], q: &ResolveQuery) -> Option<String> {
    let src = files
        .iter()
        .find(|(p, _)| p == q.file.as_str() || p.ends_with(q.file.as_str()))?;
    ident_at(&src.1, q.position)
}

fn ident_at(src: &str, pos: Position) -> Option<String> {
    let mut line = 0u32;
    let mut col = 0u32;
    let mut idx = 0usize;
    for (i, ch) in src.char_indices() {
        if line == pos.line && col == pos.character {
            idx = i;
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
            idx = i + ch.len_utf8();
        }
    }
    let bytes = src.as_bytes();
    if idx >= bytes.len() {
        return None;
    }
    let mut start = idx;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    let mut end = idx;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    if start >= end {
        return None;
    }
    Some(src[start..end].to_string())
}
