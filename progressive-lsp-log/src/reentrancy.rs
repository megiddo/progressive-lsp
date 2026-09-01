//! [`ReentrancyGuard`] — Proxy / Guard. Thread-local `IN_EMIT`.

use std::cell::Cell;

thread_local! {
    static IN_EMIT: Cell<bool> = const { Cell::new(false) };
}

/// Proxy / Guard. If `emit` is already on the stack, enqueue without extra locks.
pub struct ReentrancyGuard {
    prev: bool,
}

impl ReentrancyGuard {
    /// Mark this thread as inside `emit`. Drop restores the previous flag.
    pub fn enter() -> Self {
        let prev = IN_EMIT.with(|c| c.replace(true));
        Self { prev }
    }

    pub fn already_in_emit(&self) -> bool {
        self.prev
    }

    pub fn in_emit() -> bool {
        IN_EMIT.with(Cell::get)
    }
}

impl Drop for ReentrancyGuard {
    fn drop(&mut self) {
        IN_EMIT.with(|c| c.set(self.prev));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reentrancy_guard_nests_and_restores_proxy() {
        assert!(!ReentrancyGuard::in_emit());
        let outer = ReentrancyGuard::enter();
        assert!(!outer.already_in_emit());
        assert!(ReentrancyGuard::in_emit());
        {
            let inner = ReentrancyGuard::enter();
            assert!(inner.already_in_emit());
            assert!(ReentrancyGuard::in_emit());
            drop(inner);
        }
        assert!(ReentrancyGuard::in_emit());
        drop(outer);
        assert!(!ReentrancyGuard::in_emit());
    }
}
