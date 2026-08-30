//! `Highlighter` Adapter over syntect. Unknown syntax → empty spans, no panic.

use std::path::Path;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

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

/// syntect tokens for a path + text. Does not panic on unknown syntax.
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme: Theme,
}

impl Highlighter {
    pub fn new() -> Self {
        let theme = ThemeSet::load_defaults().themes["base16-ocean.dark"].clone();
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme,
        }
    }

    pub fn highlight(&self, path: &Path, text: &str) -> Vec<HighlightSpan> {
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
        let highlighter = Highlighter::new();
        let text = "fn main() {\n    let x = 1;\n}\n";
        let spans = highlighter.highlight(Path::new("/ws/src/lib.rs"), text);
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

    #[test]
    fn highlighter_adapter_unknown_syntax_empty_spans_no_panic() {
        let highlighter = Highlighter::default();
        let text = "fn main() { let x = 1; }";
        assert!(highlighter
            .highlight(Path::new("/ws/file.unknown"), text)
            .is_empty());
        assert!(highlighter
            .highlight(Path::new("/ws/noext"), text)
            .is_empty());
        assert!(highlighter.highlight(Path::new(""), text).is_empty());
        assert!(highlighter
            .highlight(Path::new("/ws/.hidden"), text)
            .is_empty());
        assert!(highlighter
            .highlight(Path::new("Makefile"), text)
            .is_empty());
        let debug = format!("{:?}", highlighter);
        assert!(debug.contains("Highlighter"));
        assert!(!debug.is_empty());
    }

    #[test]
    fn highlighter_adapter_empty_rs_and_uppercase_ext() {
        let highlighter = Highlighter::new();
        let empty = highlighter.highlight(Path::new("empty.rs"), "");
        assert!(empty.is_empty() || empty.iter().all(|s| s.is_empty()));
        let upper = highlighter.highlight(Path::new("/ws/MAIN.RS"), "fn x() {}");
        assert!(!upper.is_empty(), "extension matching is case-insensitive");
        let py = highlighter.highlight(Path::new("a.py"), "def f():\n    return 1\n");
        assert!(!py.is_empty());
    }
}
