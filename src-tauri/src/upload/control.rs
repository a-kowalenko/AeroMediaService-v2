//! Cooperative pause / resume / cancel for the upload worker.
//! Port of legacy `core/upload_control.py`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use thiserror::Error;

use crate::events;

const PAUSE_POLL_MS: u64 = 150;

/// The operator cancelled the current upload job.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("Upload abgebrochen")]
pub struct UploadCancelled;

/// Snapshot for UI / IPC (`paused` = request, `holding` = worker blocked).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct UploadControlState {
    pub paused: bool,
    pub holding: bool,
    pub cancelled: bool,
}

/// Shared between UI commands and the upload worker (legacy GUI-thread vs QThread).
#[derive(Clone, Default)]
pub struct UploadControl {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    pause: AtomicBool,
    holding: AtomicBool,
    cancel: AtomicBool,
}

impl UploadControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> UploadControlState {
        UploadControlState {
            paused: self.is_paused(),
            holding: self.is_holding(),
            cancelled: self.is_cancelled(),
        }
    }

    fn emit_state(&self) {
        events::emit_upload_control(self.snapshot());
    }

    pub fn reset_for_new_job(&self) {
        self.inner.pause.store(false, Ordering::SeqCst);
        self.inner.holding.store(false, Ordering::SeqCst);
        self.inner.cancel.store(false, Ordering::SeqCst);
        self.emit_state();
    }

    pub fn request_pause(&self) {
        self.inner.pause.store(true, Ordering::SeqCst);
        events::emit_status("Wird pausiert…");
        self.emit_state();
    }

    pub fn request_resume(&self) {
        self.inner.pause.store(false, Ordering::SeqCst);
        self.inner.holding.store(false, Ordering::SeqCst);
        events::emit_status("Upload wird fortgesetzt…");
        self.emit_state();
    }

    pub fn request_cancel(&self) {
        self.inner.cancel.store(true, Ordering::SeqCst);
        self.inner.pause.store(false, Ordering::SeqCst);
        self.inner.holding.store(false, Ordering::SeqCst);
        self.emit_state();
    }

    pub fn is_paused(&self) -> bool {
        self.inner.pause.load(Ordering::SeqCst)
    }

    pub fn is_holding(&self) -> bool {
        self.inner.holding.load(Ordering::SeqCst)
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancel.load(Ordering::SeqCst)
    }

    pub fn check_cancelled(&self) -> Result<(), UploadCancelled> {
        if self.inner.cancel.load(Ordering::SeqCst) {
            Err(UploadCancelled)
        } else {
            Ok(())
        }
    }

    /// Block while paused; cancel still wins (legacy `wait_if_paused`).
    pub async fn wait_if_paused(&self) -> Result<(), UploadCancelled> {
        let mut announced = false;
        while self.inner.pause.load(Ordering::SeqCst) {
            if self.inner.cancel.load(Ordering::SeqCst) {
                if announced {
                    self.inner.holding.store(false, Ordering::SeqCst);
                    self.emit_state();
                }
                return Err(UploadCancelled);
            }
            if !announced {
                self.inner.holding.store(true, Ordering::SeqCst);
                events::emit_status("Pausiert");
                self.emit_state();
                announced = true;
            }
            tokio::time::sleep(Duration::from_millis(PAUSE_POLL_MS)).await;
        }
        if announced {
            self.inner.holding.store(false, Ordering::SeqCst);
            self.emit_state();
        }
        self.check_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_clears_pause_and_cancel() {
        let ctl = UploadControl::new();
        ctl.request_pause();
        ctl.request_cancel();
        ctl.reset_for_new_job();
        assert!(!ctl.is_paused());
        assert!(!ctl.is_holding());
        assert!(!ctl.is_cancelled());
        assert!(ctl.check_cancelled().is_ok());
    }

    #[test]
    fn cancel_clears_pause() {
        let ctl = UploadControl::new();
        ctl.request_pause();
        ctl.request_cancel();
        assert!(!ctl.is_paused());
        assert!(!ctl.is_holding());
        assert!(ctl.is_cancelled());
        assert_eq!(ctl.check_cancelled(), Err(UploadCancelled));
    }

    #[test]
    fn resume_clears_pause_only() {
        let ctl = UploadControl::new();
        ctl.request_pause();
        ctl.request_resume();
        assert!(!ctl.is_paused());
        assert!(!ctl.is_holding());
        assert!(ctl.check_cancelled().is_ok());
    }

    #[test]
    fn snapshot_reflects_flags() {
        let ctl = UploadControl::new();
        assert_eq!(
            ctl.snapshot(),
            UploadControlState {
                paused: false,
                holding: false,
                cancelled: false,
            }
        );
        ctl.request_pause();
        assert!(ctl.snapshot().paused);
        assert!(!ctl.snapshot().holding);
    }

    #[tokio::test]
    async fn wait_if_paused_returns_immediately_when_not_paused() {
        let ctl = UploadControl::new();
        ctl.wait_if_paused().await.unwrap();
        assert!(!ctl.is_holding());
    }

    #[tokio::test]
    async fn wait_if_paused_sets_holding() {
        let ctl = UploadControl::new();
        ctl.request_pause();
        let waiter = ctl.clone();
        let handle = tokio::spawn(async move { waiter.wait_if_paused().await });
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(ctl.is_holding());
        ctl.request_resume();
        handle.await.unwrap().unwrap();
        assert!(!ctl.is_holding());
    }

    #[tokio::test]
    async fn cancel_while_paused_raises() {
        let ctl = UploadControl::new();
        ctl.request_pause();
        let waiter = ctl.clone();
        let handle = tokio::spawn(async move { waiter.wait_if_paused().await });
        tokio::time::sleep(Duration::from_millis(40)).await;
        ctl.request_cancel();
        assert_eq!(handle.await.unwrap(), Err(UploadCancelled));
        assert!(!ctl.is_holding());
    }

    #[tokio::test]
    async fn resume_unblocks_wait() {
        let ctl = UploadControl::new();
        ctl.request_pause();
        let waiter = ctl.clone();
        let handle = tokio::spawn(async move { waiter.wait_if_paused().await });
        tokio::time::sleep(Duration::from_millis(40)).await;
        ctl.request_resume();
        handle.await.unwrap().unwrap();
    }
}
