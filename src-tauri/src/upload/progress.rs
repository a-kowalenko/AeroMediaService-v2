//! Honest upload progress: send-byte tracking + parallel inflight aggregation.
//! Port of legacy `_BatchUploadProgress` / stream-read progress (no time-based faking).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::stream;
use reqwest::Body;

use crate::cloud::dropbox::percent;
use crate::events;

/// UI emit throttle (legacy `DROPBOX_UI_PROGRESS_INTERVAL_S`).
pub const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(50);
/// Body slice size when streaming send progress (legacy `_STREAM_READALL_SLICE`).
pub const SEND_PROGRESS_SLICE: usize = 256 * 1024;

/// Thread-safe total = completed + Σ(inflight). Monotonic within a job when used correctly.
pub struct BatchProgress {
    completed: AtomicU64,
    total_job: u64,
    inflight: Mutex<Vec<u64>>,
    last_emit: Mutex<Instant>,
}

impl BatchProgress {
    pub fn new(slot_count: usize, completed_base: u64, total_job: u64) -> Arc<Self> {
        Arc::new(Self {
            completed: AtomicU64::new(completed_base),
            total_job,
            inflight: Mutex::new(vec![0u64; slot_count]),
            last_emit: Mutex::new(Instant::now() - PROGRESS_EMIT_INTERVAL),
        })
    }

    pub fn combined(&self) -> u64 {
        let base = self.completed.load(Ordering::Relaxed);
        let extra: u64 = self
            .inflight
            .lock()
            .map(|g| g.iter().copied().sum())
            .unwrap_or(0);
        base.saturating_add(extra)
    }

    /// Update bytes sent for an in-flight slot (0-based). `sent` is clamped to non-decreasing.
    pub fn report_inflight(&self, slot: usize, sent: u64, force: bool) {
        if let Ok(mut guard) = self.inflight.lock() {
            if let Some(slot_val) = guard.get_mut(slot) {
                *slot_val = (*slot_val).max(sent);
            }
        }
        self.maybe_emit_total(force);
    }

    /// File finished: clear inflight and add full size to completed.
    pub fn complete_slot(&self, slot: usize, file_size: u64) {
        if let Ok(mut guard) = self.inflight.lock() {
            if let Some(slot_val) = guard.get_mut(slot) {
                *slot_val = 0;
            }
        }
        self.completed.fetch_add(file_size, Ordering::Relaxed);
        self.maybe_emit_total(true);
    }

    fn maybe_emit_total(&self, force: bool) {
        let now = Instant::now();
        if let Ok(mut last) = self.last_emit.lock() {
            if !force && now.duration_since(*last) < PROGRESS_EMIT_INTERVAL {
                return;
            }
            *last = now;
        }
        let current = self.combined();
        events::emit_progress_total(percent(current, self.total_job), current, self.total_job);
    }
}

/// Throttled callback wrapper for per-file send progress.
pub struct ThrottledSendNotify {
    last: Mutex<Instant>,
    inner: Arc<dyn Fn(u64) + Send + Sync>,
}

impl ThrottledSendNotify {
    pub fn new(inner: Arc<dyn Fn(u64) + Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            last: Mutex::new(Instant::now() - PROGRESS_EMIT_INTERVAL),
            inner,
        })
    }

    pub fn notify(&self, sent: u64, total: u64, force: bool) {
        let at_end = total > 0 && sent >= total;
        let now = Instant::now();
        if let Ok(mut last) = self.last.lock() {
            if !force && !at_end && now.duration_since(*last) < PROGRESS_EMIT_INTERVAL {
                return;
            }
            *last = now;
        }
        (self.inner)(sent);
    }
}

/// Build a reqwest body that reports bytes as slices are handed to the HTTP stack.
pub fn body_with_send_progress(
    data: Bytes,
    on_send: Arc<dyn Fn(u64) + Send + Sync>,
) -> Body {
    let total = data.len() as u64;
    let notify = ThrottledSendNotify::new(on_send);
    notify.notify(0, total, true);

    let stream = stream::unfold(
        (data, 0usize, notify, total),
        |(data, offset, notify, total)| async move {
            if offset >= data.len() {
                return None;
            }
            let end = (offset + SEND_PROGRESS_SLICE).min(data.len());
            let chunk = data.slice(offset..end);
            let new_off = end as u64;
            notify.notify(new_off, total, new_off >= total);
            Some((
                Ok::<Bytes, std::io::Error>(chunk),
                (data, end, notify, total),
            ))
        },
    );
    Body::wrap_stream(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn batch_progress_combined_is_completed_plus_inflight() {
        let bp = BatchProgress::new(2, 100, 1000);
        bp.report_inflight(0, 50, true);
        bp.report_inflight(1, 25, true);
        assert_eq!(bp.combined(), 175);
        bp.complete_slot(0, 200);
        assert_eq!(bp.combined(), 325); // 100+200 + 25
        bp.complete_slot(1, 75);
        assert_eq!(bp.combined(), 375);
    }

    #[test]
    fn batch_progress_inflight_is_monotonic_per_slot() {
        let bp = BatchProgress::new(1, 0, 100);
        bp.report_inflight(0, 40, true);
        bp.report_inflight(0, 30, true); // regress ignored
        assert_eq!(bp.combined(), 40);
        bp.report_inflight(0, 60, true);
        assert_eq!(bp.combined(), 60);
    }

    #[test]
    fn throttled_notify_always_fires_at_end() {
        let count = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::clone(&count);
        let n = ThrottledSendNotify::new(Arc::new(move |_| {
            c2.fetch_add(1, Ordering::SeqCst);
        }));
        n.notify(1, 100, false);
        n.notify(2, 100, false); // likely throttled
        n.notify(100, 100, false); // end → always
        assert!(count.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn progress_body_builds_without_panic() {
        let data = Bytes::from(vec![1u8, 2, 3, 4, 5]);
        let last = Arc::new(AtomicU64::new(0));
        let last2 = Arc::clone(&last);
        let _body = body_with_send_progress(
            data,
            Arc::new(move |sent| {
                last2.store(sent, Ordering::SeqCst);
            }),
        );
        // Constructor notifies 0.
        assert_eq!(last.load(Ordering::SeqCst), 0);
    }
}
