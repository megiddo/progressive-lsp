//! Crash → backoff. Tests advance FakeClock; never sleep.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackoffPolicy {
    pub initial_ms: u64,
    pub max_ms: u64,
}

impl BackoffPolicy {
    pub const DEFAULT: Self = Self {
        initial_ms: 100,
        max_ms: 10_000,
    };

    pub fn delay_ms(self, failures: u32) -> u64 {
        if failures == 0 {
            return 0;
        }
        let shift = failures.saturating_sub(1).min(10);
        (self.initial_ms.saturating_mul(1u64 << shift)).min(self.max_ms)
    }

    pub fn next_attempt_ms(self, now_ms: u64, failures: u32) -> u64 {
        now_ms.saturating_add(self.delay_ms(failures))
    }
}

pub fn can_respawn(now_ms: u64, until_ms: u64) -> bool {
    now_ms >= until_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_doubles_then_caps() {
        let p = BackoffPolicy::DEFAULT;
        assert_eq!(p.delay_ms(0), 0);
        assert_eq!(p.delay_ms(1), 100);
        assert_eq!(p.delay_ms(2), 200);
        assert_eq!(p.delay_ms(3), 400);
        assert_eq!(p.delay_ms(8), 10_000);
        assert_eq!(p.delay_ms(20), 10_000);
        assert_eq!(p.next_attempt_ms(1_000, 1), 1_100);
        assert!(can_respawn(1_100, 1_100));
        assert!(!can_respawn(1_099, 1_100));
        assert!(can_respawn(2_000, 1_100));
        let tiny = BackoffPolicy {
            initial_ms: 50,
            max_ms: 80,
        };
        assert_eq!(tiny.delay_ms(1), 50);
        assert_eq!(tiny.delay_ms(2), 80);
    }
}
