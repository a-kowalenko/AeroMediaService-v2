//! Cloud client interface (port of legacy `services/base_client.py`).

use std::path::Path;

use async_trait::async_trait;
use thiserror::Error;

use crate::upload::control::{UploadCancelled, UploadControl};

#[derive(Debug, Error)]
pub enum CloudError {
    #[error("{0}")]
    Cancelled(#[from] UploadCancelled),
    #[error("nicht verbunden: {0}")]
    NotConnected(String),
    #[error("HTTP-Fehler: {0}")]
    Http(String),
    #[error("I/O-Fehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Message(String),
}

impl CloudError {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, CloudError::Cancelled(_))
    }
}

/// Files the uploader must not send (markers, checkpoints, OS junk).
/// Port of legacy `utils/upload_checkpoint.py::should_skip_upload_file`.
pub fn should_skip_upload_file(filename: &str) -> bool {
    if filename == crate::upload::checkpoint::CHECKPOINT_FILENAME {
        return true;
    }
    matches!(
        filename,
        crate::model::marker::MARKER_FERTIG
            | crate::model::marker::MARKER_PROCESSING
            | ".DS_Store"
            | ".apdisk"
            | "Thumbs.db"
            | "desktop.ini"
    ) || filename.starts_with("._")
}

#[async_trait]
pub trait CloudClient: Send + Sync {
    async fn connect(&self) -> Result<bool, CloudError>;
    #[allow(dead_code)]
    async fn disconnect(&self) -> Result<(), CloudError>;
    #[allow(dead_code)]
    fn connection_status(&self) -> String;

    /// Upload a directory. `Ok(true)` on success, `Ok(false)` on reported failure.
    /// Cancel is `Err(CloudError::Cancelled)`.
    async fn upload_directory(
        &self,
        local_dir_path: &Path,
        remote_base_path: &str,
        control: &UploadControl,
        kunde: &crate::model::kunde::Kunde,
    ) -> Result<bool, CloudError>;

    async fn get_shareable_link(&self, remote_path: &str) -> Result<Option<String>, CloudError>;

    /// Cloud order id after a Custom-API upload, if any.
    fn last_order_id(&self) -> Option<String> {
        let _ = self;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_markers_checkpoint_and_os_junk() {
        assert!(should_skip_upload_file("_fertig.txt"));
        assert!(should_skip_upload_file("_in_verarbeitung.txt"));
        assert!(should_skip_upload_file("_aero_upload_checkpoint.json"));
        assert!(should_skip_upload_file(".DS_Store"));
        assert!(should_skip_upload_file("._hidden"));
        assert!(should_skip_upload_file("Thumbs.db"));
        assert!(!should_skip_upload_file("photo.jpg"));
        assert!(!should_skip_upload_file("clip.mp4"));
    }
}
