pub mod append;
pub mod booking_flags;
pub mod brochure;
pub mod checkpoint;
pub mod control;
pub mod preview_watermark;
pub mod progress;
pub mod registry;
pub mod retry;
pub mod worker;

use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use crate::cloud::CloudState;
use crate::upload::control::UploadControl;
use crate::upload::registry::{UploadJob, UploadQueueRegistry};

pub struct UploadState {
    pub control: UploadControl,
    pub registry: Arc<UploadQueueRegistry>,
    pub jobs: UnboundedSender<UploadJob>,
    receiver: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<UploadJob>>>,
    worker: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl UploadState {
    pub fn new() -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            control: UploadControl::new(),
            registry: Arc::new(UploadQueueRegistry::new()),
            jobs: tx,
            receiver: Mutex::new(Some(rx)),
            worker: Mutex::new(None),
        }
    }

    pub fn spawn_worker<F>(&self, cloud: &CloudState, get_setting: F)
    where
        F: Fn(&str) -> String + Send + Sync + 'static,
    {
        let rx = match self.receiver.lock() {
            Ok(mut guard) => guard.take(),
            Err(e) => e.into_inner().take(),
        };
        let Some(rx) = rx else {
            return;
        };
        let cloud = cloud.clone();
        let control = self.control.clone();
        let registry = Arc::clone(&self.registry);
        let handle = tauri::async_runtime::spawn(async move {
            worker::run_loop(rx, cloud, control, registry, get_setting).await;
        });
        if let Ok(mut guard) = self.worker.lock() {
            *guard = Some(handle);
        }
    }
}

impl Default for UploadState {
    fn default() -> Self {
        Self::new()
    }
}
