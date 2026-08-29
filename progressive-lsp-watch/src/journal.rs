//! FilesSince journal. Bounded list + `truncated` after overflow or generation gap.

use progressive_lsp_control::{files_since_request, FilesSinceRequest, FilesSinceResponse, Status};
use progressive_lsp_core::WatchOverflow;

/// How the client names the catch-up cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesSinceQuery {
    SinceGeneration(u64),
    SinceUnixMs(u64),
}

impl FilesSinceQuery {
    pub fn from_request(req: &FilesSinceRequest) -> Option<Self> {
        match req.since {
            Some(files_since_request::Since::SinceGeneration(g)) => Some(Self::SinceGeneration(g)),
            Some(files_since_request::Since::SinceUnixMs(ms)) => Some(Self::SinceUnixMs(ms)),
            None => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilesSinceAnswer {
    pub paths: Vec<String>,
    pub truncated: bool,
    pub generation: u64,
    pub overflow: Option<WatchOverflow>,
}

impl FilesSinceAnswer {
    pub fn to_proto(&self) -> FilesSinceResponse {
        FilesSinceResponse {
            status: Some(if let Some(ov) = &self.overflow {
                Status::error(1, ov.to_string())
            } else {
                Status::ok()
            }),
            paths: self.paths.clone(),
            truncated: self.truncated,
            generation: self.generation,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JournalEntry {
    path: String,
    generation: u64,
    unix_ms: u64,
}

/// Retained change list. After overflow the retained window may skip generations.
#[derive(Clone, Debug)]
pub struct FilesSinceJournal {
    entries: Vec<JournalEntry>,
    pub current_generation: u64,
    pub limit: usize,
    pub overflowed: bool,
    /// Oldest generation still fully retained. Below this is a gap.
    pub retained_from_generation: u64,
}

impl FilesSinceJournal {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: Vec::new(),
            current_generation: 0,
            limit: limit.max(1),
            overflowed: false,
            retained_from_generation: 0,
        }
    }

    pub fn record(&mut self, path: impl Into<String>, generation: u64, unix_ms: u64) {
        self.entries.push(JournalEntry {
            path: path.into(),
            generation,
            unix_ms,
        });
        if generation > self.current_generation {
            self.current_generation = generation;
        }
        self.trim();
    }

    pub fn mark_overflow(&mut self, generation: u64) {
        self.overflowed = true;
        self.current_generation = generation;
        self.retained_from_generation = generation;
        self.entries.clear();
    }

    pub fn query(&self, q: Option<FilesSinceQuery>) -> FilesSinceAnswer {
        let matched: Vec<&JournalEntry> = match q {
            None => self.entries.iter().collect(),
            Some(FilesSinceQuery::SinceGeneration(g)) => {
                self.entries.iter().filter(|e| e.generation > g).collect()
            }
            Some(FilesSinceQuery::SinceUnixMs(ms)) => {
                self.entries.iter().filter(|e| e.unix_ms > ms).collect()
            }
        };

        let gap = match q {
            Some(FilesSinceQuery::SinceGeneration(g)) => {
                g < self.retained_from_generation || (self.overflowed && g < self.current_generation)
            }
            _ => self.overflowed && q.is_some(),
        };

        let mut paths: Vec<String> = Vec::new();
        for e in matched {
            if !paths.iter().any(|p| p == &e.path) {
                paths.push(e.path.clone());
            }
        }

        let mut truncated = gap || paths.len() > self.limit;
        if paths.len() > self.limit {
            paths.truncate(self.limit);
            truncated = true;
        }

        let overflow = if gap {
            Some(WatchOverflow {
                generation: self.current_generation,
            })
        } else {
            None
        };

        FilesSinceAnswer {
            paths,
            truncated,
            generation: self.current_generation,
            overflow,
        }
    }

    fn trim(&mut self) {
        if self.entries.len() <= self.limit * 4 {
            return;
        }
        let drop = self.entries.len() - self.limit * 2;
        if let Some(first_kept) = self.entries.get(drop) {
            self.retained_from_generation = first_kept.generation;
        }
        self.entries.drain(0..drop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_from_request_oneof() {
        assert_eq!(
            FilesSinceQuery::from_request(&FilesSinceRequest {
                since: Some(files_since_request::Since::SinceGeneration(3)),
            }),
            Some(FilesSinceQuery::SinceGeneration(3))
        );
        assert_eq!(
            FilesSinceQuery::from_request(&FilesSinceRequest {
                since: Some(files_since_request::Since::SinceUnixMs(99)),
            }),
            Some(FilesSinceQuery::SinceUnixMs(99))
        );
        assert_eq!(
            FilesSinceQuery::from_request(&FilesSinceRequest { since: None }),
            None
        );
    }

    #[test]
    fn since_generation_and_unix_ms_and_dedup() {
        let mut j = FilesSinceJournal::new(8);
        j.record("a.java", 1, 10);
        j.record("b.java", 2, 20);
        j.record("a.java", 2, 21);
        let by_gen = j.query(Some(FilesSinceQuery::SinceGeneration(1)));
        assert_eq!(by_gen.paths, vec!["b.java", "a.java"]);
        assert!(!by_gen.truncated);
        assert_eq!(by_gen.generation, 2);
        let by_ms = j.query(Some(FilesSinceQuery::SinceUnixMs(10)));
        assert_eq!(by_ms.paths, vec!["b.java", "a.java"]);
        let all = j.query(None);
        assert_eq!(all.paths, vec!["a.java", "b.java"]);
        assert!(all.overflow.is_none());
    }

    #[test]
    fn overflow_sets_truncated_and_generation_gap() {
        let mut j = FilesSinceJournal::new(2);
        j.record("old.java", 1, 1);
        j.mark_overflow(4);
        let ans = j.query(Some(FilesSinceQuery::SinceGeneration(1)));
        assert!(ans.truncated);
        assert!(ans.paths.is_empty());
        assert_eq!(ans.generation, 4);
        assert_eq!(ans.overflow.as_ref().unwrap().generation, 4);
        let proto = ans.to_proto();
        assert!(proto.truncated);
        assert_eq!(proto.generation, 4);
        assert!(!proto.status.unwrap().is_ok());
        let ok = FilesSinceAnswer {
            paths: vec!["x".into()],
            truncated: false,
            generation: 1,
            overflow: None,
        }
        .to_proto();
        assert!(ok.status.unwrap().is_ok());
        assert!(!ok.truncated);
    }

    #[test]
    fn bounded_list_truncates_without_losing_flag() {
        let mut j = FilesSinceJournal::new(2);
        j.record("a", 1, 1);
        j.record("b", 1, 2);
        j.record("c", 1, 3);
        let ans = j.query(None);
        assert!(ans.truncated);
        assert_eq!(ans.paths.len(), 2);
        assert_eq!(ans.paths[0], "a");
        assert_eq!(ans.paths[1], "b");
    }

    #[test]
    fn trim_advances_retained_from_generation() {
        let mut j = FilesSinceJournal::new(1);
        for i in 0..10 {
            j.record(format!("f{i}"), i as u64 + 1, i as u64);
        }
        assert!(j.entries.len() <= 4);
        assert!(j.entries.len() >= 2);
        assert!(j.retained_from_generation >= 1);
        assert!(!j.overflowed);
        let gap = j.query(Some(FilesSinceQuery::SinceGeneration(0)));
        assert!(gap.truncated);
        assert!(gap.overflow.is_some());
    }

    #[test]
    fn caught_up_after_overflow_is_not_a_generation_gap() {
        let mut j = FilesSinceJournal::new(2);
        j.mark_overflow(4);
        j.record("new.java", 4, 10);
        let caught_up = j.query(Some(FilesSinceQuery::SinceGeneration(4)));
        assert!(!caught_up.truncated);
        assert!(caught_up.overflow.is_none());
        assert!(caught_up.paths.is_empty());
        let remaining = j.query(None);
        assert!(!remaining.truncated);
        assert_eq!(remaining.paths, vec!["new.java"]);
        let by_ms = j.query(Some(FilesSinceQuery::SinceUnixMs(0)));
        assert!(by_ms.truncated);
        assert!(by_ms.overflow.is_some());
    }

    #[test]
    fn exact_limit_is_not_truncated() {
        let mut j = FilesSinceJournal::new(2);
        j.record("a", 1, 1);
        j.record("b", 1, 2);
        let ans = j.query(None);
        assert!(!ans.truncated);
        assert_eq!(ans.paths.len(), 2);
    }

    #[test]
    fn limit_is_at_least_one() {
        let j = FilesSinceJournal::new(0);
        assert_eq!(j.limit, 1);
    }
}
