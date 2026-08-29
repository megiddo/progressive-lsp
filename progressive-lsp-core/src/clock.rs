//! `ClockPort` plus production and test-double clocks. Tests never `thread::sleep`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Injected clock. Production uses the wall clock; tests use [`FakeClock`].
pub trait ClockPort: Send + Sync {
    fn now(&self) -> Instant;
    fn unix_ms(&self) -> u64;
}

/// Wall clock. Not used in deterministic tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl ClockPort for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn unix_ms(&self) -> u64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => u64::try_from(d.as_millis()).unwrap_or(u64::MAX),
            Err(_) => 0,
        }
    }
}

/// Deterministic clock. Advance with [`FakeClock::advance_ms`] — never sleep.
pub struct FakeClock {
    origin: Instant,
    offset_ms: AtomicU64,
    unix_ms: AtomicU64,
}

impl FakeClock {
    pub fn at_unix_ms(unix_ms: u64) -> Self {
        Self {
            origin: Instant::now(),
            offset_ms: AtomicU64::new(0),
            unix_ms: AtomicU64::new(unix_ms),
        }
    }

    pub fn advance_ms(&self, ms: u64) {
        self.offset_ms.fetch_add(ms, Ordering::SeqCst);
        self.unix_ms.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn offset_ms(&self) -> u64 {
        self.offset_ms.load(Ordering::SeqCst)
    }
}

impl ClockPort for FakeClock {
    fn now(&self) -> Instant {
        self.origin + Duration::from_millis(self.offset_ms())
    }

    fn unix_ms(&self) -> u64 {
        self.unix_ms.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_starts_at_injected_unix_and_zero_offset() {
        let clock = FakeClock::at_unix_ms(1_700_000_000_000);
        assert_eq!(clock.unix_ms(), 1_700_000_000_000);
        assert_eq!(clock.offset_ms(), 0);
        let a = clock.now();
        let b = clock.now();
        assert_eq!(a, b);
    }

    #[test]
    fn fake_clock_advance_moves_both_now_and_unix() {
        let clock = FakeClock::at_unix_ms(1000);
        let before = clock.now();
        clock.advance_ms(250);
        assert_eq!(clock.unix_ms(), 1250);
        assert_eq!(clock.offset_ms(), 250);
        assert_eq!(clock.now().duration_since(before), Duration::from_millis(250));
        clock.advance_ms(50);
        assert_eq!(clock.unix_ms(), 1300);
        assert_eq!(clock.offset_ms(), 300);
    }

    #[test]
    fn fake_clock_advance_zero_is_noop() {
        let clock = FakeClock::at_unix_ms(42);
        clock.advance_ms(0);
        assert_eq!(clock.unix_ms(), 42);
        assert_eq!(clock.offset_ms(), 0);
    }

    #[test]
    fn system_clock_reports_sane_unix_ms() {
        let clock = SystemClock;
        let _ = clock.now();
        let ms = clock.unix_ms();
        // 2020-01-01 .. far future; not wall-clock polling, one sample.
        assert!(ms > 1_577_836_800_000, "unix_ms={ms}");
        assert!(ms < 10_000_000_000_000, "unix_ms={ms}");
        let _ = SystemClock;
    }
}
