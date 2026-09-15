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
        self.push_detail(depth, name, outcome, None);
    }

    pub fn push_detail(
        &mut self,
        depth: usize,
        name: impl Into<String>,
        outcome: Outcome,
        detail: Option<String>,
    ) {
        self.lines.push(Line {
            depth,
            name: name.into(),
            outcome,
            detail,
        });
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
        eprintln!("- {}: {}", self.root_name, root.label());
        for line in &self.lines {
            let indent = "  ".repeat(line.depth);
            eprintln!("{indent}|- {}: {}", line.name, line.outcome.label());
            if line.outcome == Outcome::Fail {
                if let Some(ref detail) = line.detail {
                    for part in detail.lines().take(12) {
                        eprintln!("{indent}    {part}");
                    }
                }
            }
        }
    }

    pub fn failed(&self) -> bool {
        self.root_outcome() == Outcome::Fail
    }
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
}
