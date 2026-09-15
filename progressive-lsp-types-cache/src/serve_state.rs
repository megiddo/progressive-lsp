//! Discover WAL `cache_state` values (REQ-NFR-3.1).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheServeState {
    Hit,
    Miss,
    Stale,
    NotApplicable,
}

impl CacheServeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Stale => "stale",
            Self::NotApplicable => "n/a",
        }
    }
}
