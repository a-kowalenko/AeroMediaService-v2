//! Cooperative pause / resume / cancel for the upload worker.
//! Port of legacy `core/upload_control.py`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;

const PAUSE_POLL_MS: u64 = 150;

/// The operator cancelled the current upload job.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("Upload abgebrochen")]
pub struct UploadCancelled;

/// Shared between UI commands and the upload worker (legacy GUI-thread vs QThread).
#[derive(Clone, Default)]
pub struct UploadControl {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    pause: AtomicBool,
    cancel: AtomicBool,
}

impl UploadControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_for_new_job(&self) {
        self.inner.pause.store(false, Ordering::SeqCst);
        self.inner.cancel.store(false, Ordering::SeqCst);
    }

    pub fn request_pause(&self) {
        self.inner.pause.store(true, Ordering::SeqCst);
    }

    pub fn request_resume(&self) {
        self.inner.pause.store(false, Ordering::SeqCst);
    }

    pub fn request_cancel(&self) {
        self.inner.cancel.store(true, Ordering::SeqCst);
        self.inner.pause.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.inner.pause.load(Ordering::SeqCst)
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
        while self.inner.pause.load(Ordering::SeqCst) {
            if self.inner.cancel.load(Ordering::SeqCst) {
                return Err(UploadCancelled);
            }
            tokio::time::sleep(Duration::from_millis(PAUSE_POLL_MS)).await;
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
        assert!(!ctl.is_cancelled());
        assert!(ctl.check_cancelled().is_ok());
    }

    #[test]
    fn cancel_clears_pause() {
        let ctl = UploadControl::new();
        ctl.request_pause();
        ctl.request_cancel();
        assert!(!ctl.is_paused());
        assert!(ctl.is_cancelled());
        assert_eq!(ctl.check_cancelled(), Err(UploadCancelled));
    }

    #[test]
    fn resume_clears_pause_only() {
        let ctl = UploadControl::new();
        ctl.request_pause();
        ctl.request_resume();
        assert!(!ctl.is_paused());
        assert!(ctl.check_cancelled().is_ok());
    }

    #[tokio::test]
    async fn wait_if_paused_returns_immediately_when_not_paused() {
        let ctl = UploadControl::new();
        ctl.wait_if_paused().await.unwrap();
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
