//! Liveness budget checks for chain steps (REQ-NFR-1).

use std::time::{Duration, Instant};

pub const CHAIN_STEP_BUDGET_MS: u64 = 20;

pub fn assert_within_budget(elapsed: Duration, label: &str) {
    assert!(
        elapsed.as_millis() <= CHAIN_STEP_BUDGET_MS as u128,
        "{label} took {} ms (budget {CHAIN_STEP_BUDGET_MS} ms)",
        elapsed.as_millis()
    );
}

pub fn elapsed(start: Instant) -> Duration {
    start.elapsed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_constant_matches_req_nfr() {
        assert_eq!(CHAIN_STEP_BUDGET_MS, 20);
    }

    #[test]
    fn instant_elapsed_is_within_budget_on_empty_work() {
        let start = Instant::now();
        assert_within_budget(elapsed(start), "empty");
    }
}
