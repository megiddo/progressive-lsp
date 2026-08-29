//! Package-stream ingest. Completion emits workDoneProgress and marks Graph.

use progressive_lsp_core::{PackageId, Tier};

/// Standard LSP workDoneProgress payload (Facade emits `$/progress`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkDoneProgress {
    pub token: String,
    pub kind: ProgressKind,
    pub title: Option<String>,
    pub message: Option<String>,
    pub percentage: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressKind {
    Begin,
    Report,
    End,
}

impl ProgressKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Report => "report",
            Self::End => "end",
        }
    }
}

impl WorkDoneProgress {
    pub fn begin(token: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            kind: ProgressKind::Begin,
            title: Some(title.into()),
            message: None,
            percentage: Some(0),
        }
    }

    pub fn report(token: impl Into<String>, message: impl Into<String>, percentage: u32) -> Self {
        Self {
            token: token.into(),
            kind: ProgressKind::Report,
            title: None,
            message: Some(message.into()),
            percentage: Some(percentage.min(100)),
        }
    }

    pub fn end(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            kind: ProgressKind::End,
            title: None,
            message: None,
            percentage: Some(100),
        }
    }
}

/// One package in the ingest stream. Command, not a god indexer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageIngest {
    pub package: PackageId,
    pub language: String,
    pub files: Vec<std::path::PathBuf>,
}

impl PackageIngest {
    pub fn new(package: impl AsRef<str>, language: impl Into<String>) -> Self {
        Self {
            package: PackageId::new(package.as_ref()),
            language: language.into(),
            files: Vec::new(),
        }
    }

    pub fn with_file(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.files.push(path.into());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// Result of finishing one package (T2 becomes available).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IngestReport {
    pub package: PackageId,
    pub tier: Tier,
    pub files: usize,
    pub progress: Vec<WorkDoneProgress>,
}

impl IngestReport {
    pub fn graph(package: PackageId, files: usize, token: &str) -> Self {
        let title = format!("ingest {}", package.as_str());
        Self {
            package,
            tier: Tier::Graph,
            files,
            progress: vec![
                WorkDoneProgress::begin(token, title),
                WorkDoneProgress::report(token, "graph", 100),
                WorkDoneProgress::end(token),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_kinds_and_package_ingest() {
        assert_eq!(ProgressKind::Begin.as_str(), "begin");
        assert_eq!(ProgressKind::Report.as_str(), "report");
        assert_eq!(ProgressKind::End.as_str(), "end");
        let b = WorkDoneProgress::begin("t", "ingest lib");
        assert_eq!(b.kind, ProgressKind::Begin);
        assert_eq!(b.percentage, Some(0));
        let r = WorkDoneProgress::report("t", "lib", 150);
        assert_eq!(r.percentage, Some(100));
        assert_eq!(r.message.as_deref(), Some("lib"));
        let e = WorkDoneProgress::end("t");
        assert_eq!(e.kind, ProgressKind::End);
        let mut pkg = PackageIngest::new("lib", "java");
        assert!(pkg.is_empty());
        pkg = pkg.with_file("A.java");
        assert!(!pkg.is_empty());
        assert_eq!(pkg.package.as_str(), "lib");
        let report = IngestReport::graph(PackageId::new("lib"), 2, "ingest-lib");
        assert_eq!(report.tier, Tier::Graph);
        assert_eq!(report.progress.len(), 3);
        assert_eq!(report.files, 2);
    }
}
