//! Compact PASS/FAIL/SKIP tree for `./build test` and `./build integ`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail,
    Skip,
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
        let has_depth_2 = self.lines.iter().any(|l| l.depth == 2);
        if has_depth_2 {
            return self
                .lines
                .iter()
                .filter(|l| l.depth == 2)
                .map(|l| l.passed)
                .sum();
        }
        self.lines.iter().map(|l| l.passed).sum()
    }

    pub fn root_outcome(&self) -> Outcome {
        self.lines
            .iter()
            .filter(|l| l.depth == 1)
            .map(|l| l.outcome)
            .fold(Outcome::Pass, Outcome::merge)
    }

    pub fn print(&self) {
        let root = self.root_outcome();
        let total = self.total_passed();
        if total > 0 {
            eprintln!(
                "▸ {}: {} ({} tests)",
                self.root_name,
                root.label(),
                total
            );
        } else {
            eprintln!("▸ {}: {}", self.root_name, root.label());
        }
        for (idx, line) in self.lines.iter().enumerate() {
            let prefix = branch_prefix(&self.lines, idx);
            eprintln!(
                "{prefix}{}: {}",
                line.display_name(),
                line.outcome.label()
            );
            if line.outcome == Outcome::Fail {
                if let Some(ref detail) = line.detail {
                    let detail_prefix = detail_prefix(&self.lines, idx);
                    for part in detail.lines().take(12) {
                        eprintln!("{detail_prefix}↳ {part}");
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
            Outcome::Fail => format!(
                "{} ({} passed, {} failed)",
                self.name, self.passed, self.failed
            ),
            _ => self.name.clone(),
        }
    }
}

/// Sum `test result:` lines from `cargo test` (lib + integration + doc bins).
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
}
