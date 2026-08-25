//! Adaptive parallelism for Dropbox write operations (429 / too_many_write_operations).

use std::sync::{Arc, Mutex};

use crate::storage::logging;

/// Successful uploads before increasing parallelism by one (up to [`max_workers`]).
pub const SUCCESSES_TO_INCREASE: u32 = 30;

struct LimiterState {
    max_workers: usize,
    current: usize,
    successes_since_change: u32,
}

/// Shared limiter cloned across parallel upload tasks (AIMD-style).
#[derive(Clone)]
pub struct ParallelWriteLimiter {
    inner: Arc<Mutex<LimiterState>>,
}

impl ParallelWriteLimiter {
    pub fn new(max_workers: usize) -> Self {
        let max_workers = max_workers.max(1);
        Self {
            inner: Arc::new(Mutex::new(LimiterState {
                max_workers,
                current: max_workers,
                successes_since_change: 0,
            })),
        }
    }

    pub fn reset(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.current = g.max_workers;
            g.successes_since_change = 0;
        }
    }

    pub fn current_workers(&self, pending: usize) -> usize {
        let current = self
            .inner
            .lock()
            .map(|g| g.current)
            .unwrap_or(1);
        current.max(1).min(pending.max(1))
    }

    pub fn on_success(&self) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        g.successes_since_change = g.successes_since_change.saturating_add(1);
        if g.successes_since_change >= SUCCESSES_TO_INCREASE && g.current < g.max_workers {
            g.current += 1;
            g.successes_since_change = 0;
            logging::log_info(&format!(
                "Dropbox-Parallelität erhöht auf {} Worker.",
                g.current
            ));
        }
    }

    pub fn on_rate_limit(&self) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        let prev = g.current;
        g.current = (g.current / 2).max(1);
        g.successes_since_change = 0;
        if g.current < prev {
            logging::log_warn(&format!(
                "Dropbox-Parallelität reduziert auf {} Worker (Rate-Limit).",
                g.current
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_halves_on_rate_limit_and_recovers() {
        let lim = ParallelWriteLimiter::new(4);
        assert_eq!(lim.current_workers(100), 4);
        lim.on_rate_limit();
        assert_eq!(lim.current_workers(100), 2);
        lim.on_rate_limit();
        assert_eq!(lim.current_workers(100), 1);
        for _ in 0..SUCCESSES_TO_INCREASE {
            lim.on_success();
        }
        assert_eq!(lim.current_workers(100), 2);
    }

    #[test]
    fn reset_restores_max() {
        let lim = ParallelWriteLimiter::new(4);
        lim.on_rate_limit();
        lim.on_rate_limit();
        assert_eq!(lim.current_workers(10), 1);
        lim.reset();
        assert_eq!(lim.current_workers(10), 4);
    }
}
