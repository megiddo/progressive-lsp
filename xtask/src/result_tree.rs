//! Compact PASS/FAIL/SKIP tree for `./build test` and `./build integ`.

use std::io::IsTerminal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail,
    Skip,
}

#[derive(Clone, Copy)]
struct Ansi {
    blue: &'static str,
    label: &'static str,
    pass: &'static str,
    fail: &'static str,
    skip: &'static str,
    reset: &'static str,
}

impl Ansi {
    fn detect() -> Self {
        if std::env::var("NO_COLOR").is_ok() || !std::io::stderr().is_terminal() {
            return Self::plain();
        }
        Self {
            blue: "\x1b[38;5;19m",
            label: "\x1b[30m",
            pass: "\x1b[38;5;28m",
            fail: "\x1b[31m",
            skip: "\x1b[38;5;130m",
            reset: "\x1b[0m",
        }
    }

    fn plain() -> Self {
        Self {
            blue: "",
            label: "",
            pass: "",
            fail: "",
            skip: "",
            reset: "",
        }
    }

    fn outcome(self, o: Outcome) -> &'static str {
        match o {
            Outcome::Pass => self.pass,
            Outcome::Fail => self.fail,
            Outcome::Skip => self.skip,
        }
    }
}

impl Outcome {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
        }
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        if self == Self::Fail || other == Self::Fail {
            Self::Fail
        } else if self == Self::Skip || other == Self::Skip {
            Self::Skip
        } else {
            Self::Pass
        }
    }
}

struct Line {
    depth: usize,
    name: String,
    outcome: Outcome,
    passed: u32,
    failed: u32,
    detail: Option<String>,
}

pub struct ResultTree {
    root_name: String,
    lines: Vec<Line>,
}

impl ResultTree {
    pub fn new(root_name: impl Into<String>) -> Self {
        Self {
            root_name: root_name.into(),
            lines: Vec::new(),
        }
    }

    pub fn push(&mut self, depth: usize, name: impl Into<String>, outcome: Outcome) {
        self.push_counts(depth, name, outcome, 0, 0, None);
    }

    pub fn push_detail(
        &mut self,
        depth: usize,
        name: impl Into<String>,
        outcome: Outcome,
        detail: Option<String>,
    ) {
        self.push_counts(depth, name, outcome, 0, 0, detail);
    }

    pub fn push_counts(
        &mut self,
        depth: usize,
        name: impl Into<String>,
        outcome: Outcome,
        passed: u32,
        failed: u32,
        detail: Option<String>,
    ) {
        self.lines.push(Line {
            depth,
            name: name.into(),
            outcome,
            passed,
            failed,
            detail,
        });
    }

    pub fn total_passed(&self) -> u32 {
        let mut total = 0u32;
        let mut i = 0usize;
        while i < self.lines.len() {
            if self.lines[i].depth != 1 {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            let mut child_pass = 0u32;
            while j < self.lines.len() && self.lines[j].depth > 1 {
                if self.lines[j].depth == 2 {
                    child_pass += self.lines[j].passed;
                }
                j += 1;
            }
            total += if child_pass > 0 {
                child_pass
            } else {
                self.lines[i].passed
            };
            i = j;
        }
        total
    }

    pub fn root_outcome(&self) -> Outcome {
        self.lines
            .iter()
            .filter(|l| l.depth == 1)
            .map(|l| l.outcome)
            .fold(Outcome::Pass, Outcome::merge)
    }

    pub fn print(&self) {
        let ansi = Ansi::detect();
        let root = self.root_outcome();
        let total = self.total_passed();
        let root_label = format!(
            "{}{}{}{}: {}{} {}",
            ansi.label,
            self.root_name,
            ansi.reset,
            ansi.outcome(root),
            root.label(),
            ansi.reset,
            if total > 0 {
                format!("({total} tests)")
            } else {
                String::new()
            }
        );
        eprintln!("▸ {root_label}");
        for (idx, line) in self.lines.iter().enumerate() {
            let prefix = branch_prefix(&self.lines, idx);
            let colored_prefix = format!("{}{}{}", ansi.blue, prefix, ansi.reset);
            eprintln!(
                "{colored_prefix}{}{}{}: {}{} {}",
                ansi.label,
                line.display_name(),
                ansi.reset,
                ansi.outcome(line.outcome),
                line.outcome.label(),
                ansi.reset
            );
            if line.outcome == Outcome::Fail {
                if let Some(ref detail) = line.detail {
                    let detail_prefix = detail_prefix(&self.lines, idx);
                    let colored_dp = format!("{}{}{}", ansi.blue, detail_prefix, ansi.reset);
                    for part in detail.lines().take(12) {
                        eprintln!("{colored_dp}↳ {part}");
                    }
                }
            }
        }
    }

    pub fn failed(&self) -> bool {
        self.root_outcome() == Outcome::Fail
    }
}

impl Line {
    fn display_name(&self) -> String {
        match self.outcome {
            Outcome::Pass if self.passed > 0 => {
                format!("{} ({} tests)", self.name, self.passed)
            }
            Outcome::Fail if self.passed == 0 && self.failed == 0 => {
                format!("{} (compile failed)", self.name)
            }
            Outcome::Fail => format!(
                "{} ({} passed, {} failed)",
                self.name, self.passed, self.failed
            ),
            _ => self.name.clone(),
        }
    }
}

/// Sum `test result:` lines from `cargo test` (lib + integration + doc bins).
/// Per-package totals from one `cargo test` log (`Running …` + `test result:` pairs).
pub fn parse_cargo_test_by_package(text: &str) -> std::collections::BTreeMap<String, (u32, u32)> {
    use std::collections::BTreeMap;
    let mut runnings: Vec<String> = Vec::new();
    let mut results: Vec<(u32, u32)> = Vec::new();
    for line in text.lines() {
        let trimmed = strip_ansi(line.trim());
        if trimmed.starts_with("Running ") && trimmed.contains("(target/") {
            if let Some(stem) = deps_stem_from_running_line(&trimmed) {
                runnings.push(package_from_deps_stem(&stem));
            }
            continue;
        }
        if trimmed.starts_with("test result:") {
            results.push(parse_single_test_result_line(&trimmed));
        }
    }

    let mut out = BTreeMap::new();
    if runnings.len() == results.len() && !runnings.is_empty() {
        merge_package_totals(&mut out, runnings.into_iter().zip(results));
        return out;
    }

    // TTY order: `Running` then `test result:` for the same binary.
    let mut current_pkg: Option<String> = None;
    for line in text.lines() {
        let trimmed = strip_ansi(line.trim());
        if trimmed.starts_with("Running ") && trimmed.contains("(target/") {
            if let Some(stem) = deps_stem_from_running_line(&trimmed) {
                current_pkg = Some(package_from_deps_stem(&stem));
            }
            continue;
        }
        if !trimmed.starts_with("test result:") {
            continue;
        }
        let (p, f) = parse_single_test_result_line(&trimmed);
        let pkg = current_pkg.clone().unwrap_or_else(|| "unknown".into());
        let entry = out.entry(pkg).or_insert((0, 0));
        entry.0 += p;
        entry.1 += f;
    }
    out
}

fn merge_package_totals(
    out: &mut std::collections::BTreeMap<String, (u32, u32)>,
    pairs: impl Iterator<Item = (String, (u32, u32))>,
) {
    for (pkg, (p, f)) in pairs {
        let entry = out.entry(pkg).or_insert((0, 0));
        entry.0 += p;
        entry.1 += f;
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            while let Some(next) = chars.next() {
                if next == 'm' {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn deps_stem_from_running_line(line: &str) -> Option<String> {
    let open = line.rfind('(')?;
    let close = line.rfind(')')?;
    if close <= open {
        return None;
    }
    let path = &line[open + 1..close];
    let file = path.rsplit('/').next()?.rsplit('\\').next()?;
    Some(file.to_string())
}

fn strip_deps_hash(stem: &str) -> &str {
    if let Some(i) = stem.rfind('-') {
        let suffix = &stem[i + 1..];
        if suffix.len() == 16 && suffix.chars().all(|c| c.is_ascii_hexdigit()) {
            return &stem[..i];
        }
    }
    stem
}

fn package_from_deps_stem(stem: &str) -> String {
    let base = strip_deps_hash(stem);
    if base == "seams_extended" {
        return "progressive-lsp".into();
    }
    if let Some(rest) = base.strip_prefix("progressive_lsp_") {
        return format!("progressive-lsp-{rest}").replace('_', "-");
    }
    if base == "progressive_lsp" || base.starts_with("progressive_lsp-") {
        return "progressive-lsp".into();
    }
    if base == "poc_ide" || base.starts_with("poc_ide-") {
        return "poc-ide".into();
    }
    if base == "plsp_it1" || base.starts_with("plsp_it1-") {
        return "plsp-it1".into();
    }
    base.replace('_', "-")
}

fn parse_single_test_result_line(line: &str) -> (u32, u32) {
    let mut passed = 0u32;
    let mut failed = 0u32;
    for segment in line.split(';') {
        let seg = segment.trim();
        if let Some(idx) = seg.find(" passed") {
            if let Some(n) = seg[..idx].trim().split_whitespace().last() {
                passed += n.parse().unwrap_or(0);
            }
        }
        if let Some(idx) = seg.find(" failed") {
            if let Some(n) = seg[..idx].trim().split_whitespace().last() {
                failed += n.parse().unwrap_or(0);
            }
        }
    }
    (passed, failed)
}

pub fn parse_cargo_test_totals(text: &str) -> (u32, u32) {
    let mut passed = 0u32;
    let mut failed = 0u32;
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("test result:") {
            continue;
        }
        for segment in line.split(';') {
            let seg = segment.trim();
            if let Some(idx) = seg.find(" passed") {
                if let Some(n) = seg[..idx].trim().split_whitespace().last() {
                    passed += n.parse().unwrap_or(0);
                }
            }
            if let Some(idx) = seg.find(" failed") {
                if let Some(n) = seg[..idx].trim().split_whitespace().last() {
                    failed += n.parse().unwrap_or(0);
                }
            }
        }
    }
    (passed, failed)
}

pub fn cargo_failure_snippet(stdout: &[u8], stderr: &[u8], max_lines: usize) -> String {
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );
    let mut picked: Vec<&str> = combined
        .lines()
        .filter(|line| {
            let t = line.trim();
            t.starts_with("error")
                || t.contains(" error[E")
                || t.starts_with("failures:")
                || t.contains(" test ... FAILED")
                || (t.starts_with("test result:") && t.contains("FAILED"))
        })
        .collect();
    if picked.is_empty() {
        return tail_output(stdout, stderr, max_lines);
    }
    if picked.len() > max_lines {
        picked = picked[picked.len() - max_lines..].to_vec();
    }
    picked.join("\n").trim().to_string()
}

pub fn failed_crates_from_cargo_output(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(i) = line.find("could not compile `") {
            let rest = &line[i + "could not compile `".len()..];
            if let Some(j) = rest.find('`') {
                out.push(rest[..j].to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `│  ├─` / `│  └─` / `   └─` style prefixes from flat depth-tagged lines.
fn sibling_is_last(lines: &[Line], idx: usize, depth: usize) -> bool {
    !lines[idx + 1..].iter().any(|l| l.depth == depth)
}

fn ancestor_continues(lines: &[Line], idx: usize, ancestor_depth: usize) -> bool {
    lines[idx + 1..]
        .iter()
        .any(|l| l.depth <= ancestor_depth)
}

fn branch_prefix(lines: &[Line], idx: usize) -> String {
    let depth = lines[idx].depth;
    let mut s = String::new();
    for d in 1..depth {
        if ancestor_continues(lines, idx, d) {
            s.push_str("│  ");
        } else {
            s.push_str("   ");
        }
    }
    if sibling_is_last(lines, idx, depth) {
        s.push('└');
    } else {
        s.push('├');
    }
    s.push_str("─ ");
    s
}

fn detail_prefix(lines: &[Line], idx: usize) -> String {
    let depth = lines[idx].depth;
    let mut s = String::new();
    for d in 1..depth {
        if ancestor_continues(lines, idx, d) {
            s.push_str("│  ");
        } else {
            s.push_str("   ");
        }
    }
    if sibling_is_last(lines, idx, depth) {
        s.push_str("   ");
    } else {
        s.push_str("│  ");
    }
    s
}

pub fn tail_output(stdout: &[u8], stderr: &[u8], max_lines: usize) -> String {
    let mut text = String::from_utf8_lossy(stderr).into_owned();
    if text.trim().is_empty() {
        text = String::from_utf8_lossy(stdout).into_owned();
    }
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.trim().to_string();
    }
    lines[lines.len() - max_lines..]
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_fails_if_any_child_fails() {
        let mut t = ResultTree::new("test");
        t.push(1, "ok", Outcome::Pass);
        t.push(1, "bad", Outcome::Fail);
        assert_eq!(t.root_outcome(), Outcome::Fail);
        assert!(t.failed());
    }

    #[test]
    fn skip_does_not_fail_root() {
        let mut t = ResultTree::new("integ");
        t.push(1, "skipped", Outcome::Skip);
        t.push(1, "ok", Outcome::Pass);
        assert_eq!(t.root_outcome(), Outcome::Skip);
        assert!(!t.failed());
    }

    #[test]
    fn parse_cargo_test_totals_sums_bins() {
        let sample = "test result: ok. 45 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n\
                      test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
        assert_eq!(parse_cargo_test_totals(sample), (45, 0));
    }

    #[test]
    fn parse_cargo_test_by_package_tty_running_before_result() {
        let sample = "\
     Running unittests src/lib.rs (target/debug/deps/progressive_lsp_core-0123456789abcdef)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running unittests src/lib.rs (target/debug/deps/progressive_lsp_resolve-fedcba9876543210)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
";
        let map = parse_cargo_test_by_package(sample);
        assert_eq!(map.get("progressive-lsp-core"), Some(&(10, 0)));
        assert_eq!(map.get("progressive-lsp-resolve"), Some(&(5, 0)));
    }

    #[test]
    fn parse_cargo_test_by_package_captured_result_before_running() {
        let sample = "\
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running unittests src/lib.rs (target/debug/deps/progressive_lsp_core-0123456789abcdef)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running unittests src/lib.rs (target/debug/deps/progressive_lsp_resolve-fedcba9876543210)
";
        let map = parse_cargo_test_by_package(sample);
        assert_eq!(map.get("progressive-lsp-core"), Some(&(10, 0)));
        assert_eq!(map.get("progressive-lsp-resolve"), Some(&(5, 0)));
    }

    #[test]
    fn branch_prefix_last_sibling_uses_corner() {
        let mut t = ResultTree::new("test");
        t.push(1, "a", Outcome::Pass);
        t.push(2, "a1", Outcome::Pass);
        t.push(2, "a2", Outcome::Pass);
        t.push(1, "b", Outcome::Pass);
        assert_eq!(branch_prefix(&t.lines, 0), "├─ ");
        assert_eq!(branch_prefix(&t.lines, 1), "│  ├─ ");
        assert_eq!(branch_prefix(&t.lines, 2), "│  └─ ");
        assert_eq!(branch_prefix(&t.lines, 3), "└─ ");
    }

    #[test]
    fn failed_crates_detects_compile_errors() {
        let log = "error: could not compile `poc-ide` (lib test) due to 2 previous errors";
        assert_eq!(failed_crates_from_cargo_output(log), vec!["poc-ide".to_string()]);
    }
}
