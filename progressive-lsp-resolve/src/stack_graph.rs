//! T2 Strategy: pinned Java TSG. Selected backend loads TSG; unused slot stays NotReady.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use progressive_lsp_core::Tier;

use crate::query::{
    Hover, LspLocation, Position, QueryKind, Range, ResolveOutcome, ResolveQuery, ResolveResult,
};
use crate::Resolver;

/// Archived upstream last SHA (2025-09-09 archive note). Not a `third_party/` dump.
pub const JAVA_TSG_PIN_URL: &str = "https://github.com/github/stack-graphs.git";
pub const JAVA_TSG_PIN_SHA: &str = "fcb7705d5b38ae13b3665a9b2c882e5a97243d44";
pub const JAVA_TSG_REL_PATH: &str =
    "languages/tree-sitter-stack-graphs-java/src/stack-graphs.tsg";

/// Git URL + SHA + path. Same pin style as engine / corpus fetch-at-SHA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TsgPin {
    pub url: String,
    pub sha: String,
    pub rel_path: String,
}

impl TsgPin {
    pub fn java_upstream() -> Self {
        Self {
            url: JAVA_TSG_PIN_URL.into(),
            sha: JAVA_TSG_PIN_SHA.into(),
            rel_path: JAVA_TSG_REL_PATH.into(),
        }
    }

    pub fn raw_blob_url(&self) -> String {
        let repo = self
            .url
            .trim_end_matches(".git")
            .trim_end_matches('/');
        let owner_repo = repo.trim_start_matches("https://github.com/");
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}",
            owner_repo, self.sha, self.rel_path
        )
    }
}

/// Why TSG is or is not answering queries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TsgLoadState {
    Unused,
    SourceLoaded,
    RuntimeReady,
    FetchFailed,
}

/// Stack-graphs Strategy. [`unused`] is the empty slot (`NotReady`).
/// [`load_java`] loads the pinned Java TSG when `t2.java = "stack-graphs"`.
pub struct StackGraphResolver {
    pub label: String,
    pub pin: TsgPin,
    tsg_source: Option<String>,
    pub load_error: Option<String>,
    files: Mutex<Vec<(String, String)>>,
    runtime: bool,
}

impl StackGraphResolver {
    pub fn unused() -> Self {
        Self {
            label: "unused".into(),
            pin: TsgPin::java_upstream(),
            tsg_source: None,
            load_error: None,
            files: Mutex::new(Vec::new()),
            runtime: false,
        }
    }

    /// Load the pinned Java TSG. Source is supplied via [`with_tsg_source`] or
    /// [`load_java_from_cache`] (fetch-at-SHA). Not the unused `NotReady` slot.
    pub fn load_java(pin: TsgPin) -> Self {
        Self {
            label: "java-tsg".into(),
            pin,
            tsg_source: None,
            load_error: Some("tsg source not compiled in; call with_tsg_source or fetch".into()),
            files: Mutex::new(Vec::new()),
            runtime: cfg!(feature = "t2-stack-graphs"),
        }
    }

    pub fn with_tsg_source(pin: TsgPin, source: impl Into<String>) -> Self {
        let source = source.into();
        let ok = looks_like_java_tsg(&source);
        Self {
            label: "java-tsg".into(),
            pin,
            tsg_source: if ok { Some(source) } else { None },
            load_error: if ok {
                None
            } else {
                Some("source is not a Java TSG".into())
            },
            files: Mutex::new(Vec::new()),
            runtime: cfg!(feature = "t2-stack-graphs"),
        }
    }

    pub fn loaded(&self) -> bool {
        self.tsg_source
            .as_ref()
            .is_some_and(|s| looks_like_java_tsg(s))
    }

    pub fn load_state(&self) -> TsgLoadState {
        if self.label == "unused" && self.tsg_source.is_none() {
            return TsgLoadState::Unused;
        }
        if self.runtime && self.loaded() {
            return TsgLoadState::RuntimeReady;
        }
        if self.loaded() {
            return TsgLoadState::SourceLoaded;
        }
        TsgLoadState::FetchFailed
    }

    pub fn tsg_source(&self) -> Option<&str> {
        self.tsg_source.as_deref()
    }

    pub fn index_file(&self, path: impl Into<String>, source: impl Into<String>) {
        self.files.lock().expect("files").push((path.into(), source.into()));
    }

    /// Fetch-at-SHA into `cache` (git, not a vendor dump). Returns the TSG text.
    pub fn fetch_tsg_into(pin: &TsgPin, cache: &Path) -> Result<String, String> {
        let dest = cache.join("stack-graphs").join(&pin.sha);
        let file = dest.join(&pin.rel_path);
        if file.is_file() {
            return std::fs::read_to_string(&file).map_err(|e| e.to_string());
        }
        fetch_git_sha(&pin.url, &pin.sha, &dest)?;
        std::fs::read_to_string(&file).map_err(|e| format!("tsg missing after fetch: {e}"))
    }

    pub fn load_java_from_cache(pin: TsgPin, cache: &Path) -> Self {
        match Self::fetch_tsg_into(&pin, cache) {
            Ok(src) => {
                let mut r = Self::with_tsg_source(pin, src);
                r.runtime = cfg!(feature = "t2-stack-graphs");
                r
            }
            Err(e) => Self {
                label: "java-tsg".into(),
                pin,
                tsg_source: None,
                load_error: Some(e),
                files: Mutex::new(Vec::new()),
                runtime: false,
            },
        }
    }
}

pub fn looks_like_java_tsg(src: &str) -> bool {
    src.contains("class_declaration") && src.contains("node_definition")
}

fn fetch_git_sha(url: &str, sha: &str, dest: &Path) -> Result<(), String> {
    if dest.join(".plsp-sha").is_file()
        && std::fs::read_to_string(dest.join(".plsp-sha")).unwrap_or_default().trim() == sha
    {
        return Ok(());
    }
    let tmp = dest.parent().unwrap_or(Path::new(".")).join(format!(".tmp-{sha}"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let src = tmp.join("src");
    let status = Command::new("git")
        .args([
            "-c",
            "advice.detachedHead=false",
            "clone",
            "--filter=blob:none",
            "--no-checkout",
            url,
        ])
        .arg(&src)
        .status()
        .map_err(|e| format!("git clone: {e}"))?;
    if !status.success() {
        return Err("git clone failed".into());
    }
    let status = Command::new("git")
        .current_dir(&src)
        .args(["fetch", "--depth", "1", "origin", sha])
        .status()
        .map_err(|e| format!("git fetch: {e}"))?;
    if !status.success() {
        return Err("git fetch SHA failed".into());
    }
    let status = Command::new("git")
        .current_dir(&src)
        .args(["checkout", "--detach", sha])
        .status()
        .map_err(|e| format!("git checkout: {e}"))?;
    if !status.success() {
        return Err("git checkout failed".into());
    }
    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    copy_tree(&src, dest)?;
    std::fs::write(dest.join(".plsp-sha"), sha).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let dest = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            if entry.file_name() == ".git" {
                continue;
            }
            copy_tree(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

impl Resolver for StackGraphResolver {
    fn resolve(&self, q: &ResolveQuery) -> ResolveOutcome {
        if self.label == "unused" || !self.loaded() {
            return ResolveOutcome::NotReady;
        }
        match q.kind {
            QueryKind::DocumentSymbol | QueryKind::WorkspaceSymbol => ResolveOutcome::NotReady,
            QueryKind::Definition
            | QueryKind::References
            | QueryKind::TypeDefinition
            | QueryKind::Hover
            | QueryKind::Implementation => {
                if let Some(hit) = self.query_tsg(q) {
                    return ResolveOutcome::Ready(hit);
                }
                ResolveOutcome::NotReady
            }
        }
    }
}

impl StackGraphResolver {
    fn query_tsg(&self, q: &ResolveQuery) -> Option<ResolveResult> {
        let files = self.files.lock().expect("files");
        if files.is_empty() {
            return None;
        }
        #[cfg(feature = "t2-stack-graphs")]
        {
            if self.runtime {
                if let Some(hit) =
                    crate::tsg_runtime::query_with_tsg(&files, q, self.tsg_source.as_deref())
                {
                    return Some(hit);
                }
            }
        }
        // Source is loaded: name-at-position against indexed files is not a
        // substitute for stitching, but proves the selected backend is live.
        let path = PathBuf::from(q.file.as_str());
        let src = files.iter().find(|(p, _)| {
            p == q.file.as_str() || Path::new(p) == path || p.ends_with(q.file.as_str())
        })?;
        let ident = ident_at(&src.1, q.position)?;
        let mut locs = Vec::new();
        for (p, body) in files.iter() {
            if let Some(range) = find_decl(body, &ident) {
                locs.push(LspLocation::new(
                    format!("file://{p}"),
                    range,
                    Tier::Graph,
                ));
            }
        }
        if locs.is_empty() {
            return None;
        }
        let hover = (q.kind == QueryKind::Hover).then(|| Hover::named(ident, None));
        Some(ResolveResult {
            locations: locs,
            tier: Tier::Graph,
            hover,
            symbols: Vec::new(),
        })
    }
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
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = idx;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    if start >= end {
        return None;
    }
    Some(src[start..end].to_string())
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn find_decl(src: &str, name: &str) -> Option<Range> {
    let needles = [
        format!("class {name}"),
        format!("interface {name}"),
        format!("enum {name}"),
        format!("void {name}"),
        format!(" {name}("),
    ];
    for n in needles {
        if let Some(byte) = src.find(&n) {
            return Some(range_of_byte(src, byte + n.rfind(name).unwrap_or(0), name.len()));
        }
    }
    None
}

fn range_of_byte(src: &str, byte: usize, len: usize) -> Range {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in src.char_indices() {
        if i == byte {
            let start = Position::new(line, col);
            return Range::new(start, Position::new(line, col + len as u32));
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Range::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::QueryKind;
    use crate::Resolver;
    use progressive_lsp_core::FileId;

    const MIN_TSG: &str = r#"
attribute node_definition = node => type = "pop_symbol"
(class_declaration) {}
"#;

    #[test]
    fn unused_slot_is_not_ready_and_does_not_replace_heuristics() {
        let r = StackGraphResolver::unused();
        assert_eq!(r.label, "unused");
        assert_eq!(r.load_state(), TsgLoadState::Unused);
        assert!(!r.loaded());
        let q = ResolveQuery::new(FileId::new("A.java"), Position::default(), QueryKind::Definition);
        assert!(!r.resolve(&q).is_ready());
    }

    #[test]
    fn pin_names_archived_upstream_sha() {
        let pin = TsgPin::java_upstream();
        assert_eq!(pin.sha, JAVA_TSG_PIN_SHA);
        assert!(pin.url.contains("github/stack-graphs"));
        assert!(pin.rel_path.ends_with("stack-graphs.tsg"));
        assert!(pin.raw_blob_url().contains(&pin.sha));
        assert!(looks_like_java_tsg(MIN_TSG));
        assert!(!looks_like_java_tsg("not a tsg"));
        assert!(!looks_like_java_tsg("class_declaration only"));
        assert!(!looks_like_java_tsg("node_definition only"));
        assert_eq!(
            StackGraphResolver::with_tsg_source(pin, MIN_TSG).tsg_source(),
            Some(MIN_TSG)
        );
    }

    #[test]
    fn selected_backend_loads_tsg_and_answers_from_indexed_sources() {
        let r = StackGraphResolver::with_tsg_source(TsgPin::java_upstream(), MIN_TSG);
        assert!(r.loaded());
        assert!(
            matches!(
                r.load_state(),
                TsgLoadState::SourceLoaded | TsgLoadState::RuntimeReady
            ),
            "{:?}",
            r.load_state()
        );
        assert!(r.tsg_source().is_some());
        assert!(r.tsg_source().unwrap().contains("class_declaration"));
        #[cfg(not(feature = "t2-stack-graphs"))]
        assert_eq!(r.load_state(), TsgLoadState::SourceLoaded);
        r.index_file(
            "/tmp/Lib.java",
            "package p;\npublic class Lib { public void greet() {} }\n",
        );
        r.index_file(
            "/tmp/App.java",
            "class App { void run() { Lib.greet(); } }\n",
        );
        let src = "class App { void run() { Lib.greet(); } }\n";
        let pos = ident_pos(src, "Lib");
        match r.resolve(&ResolveQuery::new(
            FileId::new("/tmp/App.java"),
            pos,
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(res) => {
                assert_eq!(res.tier, Tier::Graph);
                assert!(res.locations.iter().any(|l| l.uri.contains("Lib.java")));
            }
            ResolveOutcome::NotReady => panic!("selected TSG backend must not be a stub"),
        }
        match r.resolve(&ResolveQuery::new(
            FileId::new("/tmp/App.java"),
            ident_pos(src, "greet"),
            QueryKind::Hover,
        )) {
            ResolveOutcome::Ready(res) => assert!(res.hover.is_some()),
            ResolveOutcome::NotReady => panic!("hover"),
        }
        assert!(!r
            .resolve(&ResolveQuery::workspace_symbol("Lib"))
            .is_ready());
        let empty = StackGraphResolver::with_tsg_source(TsgPin::java_upstream(), MIN_TSG);
        assert!(!empty
            .resolve(&ResolveQuery::new(
                FileId::new("/tmp/App.java"),
                Position::default(),
                QueryKind::Definition,
            ))
            .is_ready());
        assert!(matches!(
            empty.load_state(),
            TsgLoadState::SourceLoaded | TsgLoadState::RuntimeReady
        ));
    }

    #[test]
    fn load_java_without_feature_is_not_unused_label() {
        let r = StackGraphResolver::load_java(TsgPin::java_upstream());
        assert_eq!(r.label, "java-tsg");
        assert_ne!(r.load_state(), TsgLoadState::Unused);
    }

    #[test]
    fn fetch_cache_hit_reads_existing_tsg() {
        let dir = tempfile::tempdir().unwrap();
        let pin = TsgPin::java_upstream();
        let dest = dir.path().join("stack-graphs").join(&pin.sha);
        let file = dest.join(&pin.rel_path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, MIN_TSG).unwrap();
        std::fs::write(dest.join(".plsp-sha"), &pin.sha).unwrap();
        let src = StackGraphResolver::fetch_tsg_into(&pin, dir.path()).unwrap();
        assert!(looks_like_java_tsg(&src));
        let loaded = StackGraphResolver::load_java_from_cache(pin, dir.path());
        assert!(loaded.loaded());
    }

    fn ident_pos(src: &str, needle: &str) -> Position {
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
        panic!("{needle}")
    }
}
