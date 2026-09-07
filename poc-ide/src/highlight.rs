//! `Highlighter` Adapter over syntect plus a [`HighlightCache`].
//! Unknown syntax → empty spans, no panic. A second highlight of the same
//! path + generation does not re-tokenize.

use std::path::{Path, PathBuf};

use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// syntect light theme: dark token colors on the white egui editor.
const LIGHT_THEME: &str = "InspiredGitHub";

/// Unhighlighted / unknown-syntax text. Dark gray, not a dark-theme pastel.
pub const PLAIN_TEXT_RGB: (u8, u8, u8) = (36, 41, 46);

/// One highlighted run. Range is ordered `start <= end` in char offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HighlightSpan {
    start: usize,
    end: usize,
    r: u8,
    g: u8,
    b: u8,
}

impl HighlightSpan {
    pub fn new(start: usize, end: usize, r: u8, g: u8, b: u8) -> Self {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        Self {
            start,
            end,
            r,
            g,
            b,
        }
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }

    pub fn r(&self) -> u8 {
        self.r
    }

    pub fn g(&self) -> u8 {
        self.g
    }

    pub fn b(&self) -> u8 {
        self.b
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Identity for a cached highlight: path + rope generation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HighlightKey {
    path: PathBuf,
    generation: u64,
}

impl HighlightKey {
    pub fn new(path: impl AsRef<Path>, generation: u64) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            generation,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Cache of syntect spans. Hit when path + generation match.
#[derive(Clone, Debug, Default)]
pub struct HighlightCache {
    key: Option<HighlightKey>,
    spans: Vec<HighlightSpan>,
}

impl HighlightCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &HighlightKey) -> Option<&[HighlightSpan]> {
        match &self.key {
            Some(stored) if stored == key => Some(self.spans.as_slice()),
            _ => None,
        }
    }

    pub fn store(&mut self, key: HighlightKey, spans: Vec<HighlightSpan>) {
        self.key = Some(key);
        self.spans = spans;
    }

    pub fn clear(&mut self) {
        self.key = None;
        self.spans.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.key.is_none()
    }
}

/// syntect tokens for a path + text. Does not panic on unknown syntax.
/// Owns a [`HighlightCache`]; the layouter rebuilds only on generation bump.
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme: Theme,
    cache: HighlightCache,
    tokenize_count: u64,
}

impl Highlighter {
    pub fn new() -> Self {
        let theme = ThemeSet::load_defaults().themes[LIGHT_THEME].clone();
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme,
            cache: HighlightCache::new(),
            tokenize_count: 0,
        }
    }

    pub fn tokenize_count(&self) -> u64 {
        self.tokenize_count
    }

    pub fn cache(&self) -> &HighlightCache {
        &self.cache
    }

    /// Cached highlight. Same path + generation does not re-tokenize.
    pub fn highlight(&mut self, path: &Path, text: &str, generation: u64) -> Vec<HighlightSpan> {
        let key = HighlightKey::new(path, generation);
        if let Some(spans) = self.cache.get(&key) {
            return spans.to_vec();
        }
        let spans = self.tokenize(path, text);
        self.cache.store(key, spans.clone());
        spans
    }

    fn tokenize(&mut self, path: &Path, text: &str) -> Vec<HighlightSpan> {
        self.tokenize_count = self.tokenize_count.saturating_add(1);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        let Some(syntax) = self.syntax_set.find_syntax_by_extension(&ext) else {
            return Vec::new();
        };
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut spans = Vec::new();
        let mut offset = 0usize;
        for line in LinesWithEndings::from(text) {
            let ranges = highlighter
                .highlight_line(line, &self.syntax_set)
                .unwrap_or_default();
            for (style, piece) in ranges {
                let len = piece.chars().count();
                if len > 0 {
                    spans.push(HighlightSpan::new(
                        offset,
                        offset + len,
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                    ));
                }
                offset += len;
            }
        }
        spans
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Highlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Highlighter").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_span_value_object_start_le_end() {
        let span = HighlightSpan::new(2, 8, 10, 20, 30);
        assert_eq!(span.start(), 2);
        assert_eq!(span.end(), 8);
        assert_eq!(span.r(), 10);
        assert_eq!(span.g(), 20);
        assert_eq!(span.b(), 30);
        assert!(!span.is_empty());
        let swapped = HighlightSpan::new(8, 2, 9, 8, 7);
        assert_eq!(swapped.start(), 2);
        assert_eq!(swapped.end(), 8);
        assert!(HighlightSpan::new(4, 4, 0, 0, 0).is_empty());
    }

    #[test]
    fn highlighter_adapter_rs_fixture_non_empty_spans() {
        let mut highlighter = Highlighter::new();
        let text = "fn main() {\n    let x = 1;\n}\n";
        let spans = highlighter.highlight(Path::new("/ws/src/lib.rs"), text, 0);
        assert!(
            !spans.is_empty(),
            "syntect must emit spans for a .rs fixture"
        );
        assert!(spans.iter().all(|s| s.start() <= s.end()));
        assert!(spans.iter().all(|s| s.end() > s.start()));
        assert_eq!(spans.first().map(|s| s.start()), Some(0));
        let covered = spans.last().map(|s| s.end()).unwrap_or(0);
        assert_eq!(covered, text.chars().count());
        for window in spans.windows(2) {
            assert_eq!(window[0].end(), window[1].start());
        }
        let colors: std::collections::BTreeSet<_> =
            spans.iter().map(|s| (s.r(), s.g(), s.b())).collect();
        assert!(
            colors.len() > 1,
            "a Rust fixture must use more than one token color, got {colors:?}"
        );
    }

    fn luminance(r: u8, g: u8, b: u8) -> u16 {
        ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u16
    }

    #[test]
    fn highlighter_uses_dark_token_colors_on_light_theme() {
        assert!(
            ThemeSet::load_defaults().themes.contains_key(LIGHT_THEME),
            "missing syntect theme {LIGHT_THEME}"
        );
        assert!(luminance(PLAIN_TEXT_RGB.0, PLAIN_TEXT_RGB.1, PLAIN_TEXT_RGB.2) < 80);
        let mut highlighter = Highlighter::new();
        let rust = highlighter.highlight(
            Path::new("/ws/src/lib.rs"),
            "fn main() {\n    let x = \"hi\";\n}\n",
            0,
        );
        let java = highlighter.highlight(
            Path::new("/ws/A.java"),
            "public class A { void greet() {} }\n",
            1,
        );
        for (lang, spans) in [("rs", rust.as_slice()), ("java", java.as_slice())] {
            let dark = spans
                .iter()
                .filter(|s| luminance(s.r(), s.g(), s.b()) < 160)
                .count();
            assert!(
                dark * 2 >= spans.len(),
                "{lang} tokens must be dark on white, dark={dark}/{} colors={:?}",
                spans.len(),
                spans
                    .iter()
                    .map(|s| (s.r(), s.g(), s.b(), luminance(s.r(), s.g(), s.b())))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn highlighter_adapter_unknown_syntax_empty_spans_no_panic() {
        let mut highlighter = Highlighter::default();
        let text = "fn main() { let x = 1; }";
        assert!(highlighter
            .highlight(Path::new("/ws/file.unknown"), text, 0)
            .is_empty());
        assert!(highlighter
            .highlight(Path::new("/ws/noext"), text, 1)
            .is_empty());
        assert!(highlighter.highlight(Path::new(""), text, 2).is_empty());
        assert!(highlighter
            .highlight(Path::new("/ws/.hidden"), text, 3)
            .is_empty());
        assert!(highlighter
            .highlight(Path::new("Makefile"), text, 4)
            .is_empty());
        let debug = format!("{:?}", highlighter);
        assert!(debug.contains("Highlighter"));
        assert!(!debug.is_empty());
    }

    #[test]
    fn highlighter_adapter_empty_rs_and_uppercase_ext() {
        let mut highlighter = Highlighter::new();
        let empty = highlighter.highlight(Path::new("empty.rs"), "", 0);
        assert!(empty.is_empty() || empty.iter().all(|s| s.is_empty()));
        let upper = highlighter.highlight(Path::new("/ws/MAIN.RS"), "fn x() {}", 1);
        assert!(!upper.is_empty(), "extension matching is case-insensitive");
        let py = highlighter.highlight(Path::new("a.py"), "def f():\n    return 1\n", 2);
        assert!(!py.is_empty());
    }

    #[test]
    fn highlight_cache_same_path_generation_does_not_re_tokenize() {
        let mut highlighter = Highlighter::new();
        let path = Path::new("/ws/src/lib.rs");
        let text = "fn main() {\n    let x = 1;\n}\n";
        assert!(highlighter.cache().is_empty());
        let first = highlighter.highlight(path, text, 7);
        assert_eq!(highlighter.tokenize_count(), 1);
        assert!(!first.is_empty());
        let key = HighlightKey::new(path, 7);
        assert_eq!(key.path(), path);
        assert_eq!(key.generation(), 7);
        assert!(highlighter.cache().get(&key).is_some());
        let second = highlighter.highlight(path, text, 7);
        assert_eq!(highlighter.tokenize_count(), 1);
        assert_eq!(first, second);
        let bumped = highlighter.highlight(path, text, 8);
        assert_eq!(highlighter.tokenize_count(), 2);
        assert_eq!(bumped, first);
        let other = highlighter.highlight(Path::new("/ws/other.rs"), text, 8);
        assert_eq!(highlighter.tokenize_count(), 3);
        assert_eq!(other, first);
        highlighter.cache().get(&HighlightKey::new(path, 7));
        let mut cache = HighlightCache::new();
        assert!(cache.is_empty());
        cache.store(HighlightKey::new("/ws/a.rs", 1), first.clone());
        assert!(!cache.is_empty());
        assert_eq!(
            cache.get(&HighlightKey::new("/ws/a.rs", 1)),
            Some(first.as_slice())
        );
        assert!(cache.get(&HighlightKey::new("/ws/a.rs", 2)).is_none());
        cache.clear();
        assert!(cache.is_empty());
        assert!(cache.get(&HighlightKey::new("/ws/a.rs", 1)).is_none());
        assert_eq!(HighlightKey::new(path, 1), HighlightKey::new(path, 1));
        assert_ne!(HighlightKey::new(path, 1), HighlightKey::new(path, 2));
        assert_ne!(
            HighlightKey::new(path, 1),
            HighlightKey::new(Path::new("/other.rs"), 1)
        );
    }
}
