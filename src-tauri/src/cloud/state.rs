//! Shared cloud clients used by upload worker and settings/connect commands.

use std::sync::Arc;

use crate::cloud::{CustomApiClient, DropboxClient};

#[derive(Clone)]
pub struct CloudState {
    /// Native Dropbox (`db_*` secrets) — primary when Dropbox cloud is selected.
    pub dropbox: Arc<DropboxClient>,
    /// Custom-API Dropbox upload account (`custom_db_*` secrets).
    pub custom_dropbox: Arc<DropboxClient>,
    pub custom_api: Arc<CustomApiClient>,
}

impl CloudState {
    pub fn new() -> Self {
        let custom_dropbox = Arc::new(DropboxClient::for_custom_api());
        Self {
            dropbox: Arc::new(DropboxClient::new()),
            custom_dropbox: Arc::clone(&custom_dropbox),
            custom_api: Arc::new(CustomApiClient::with_dropbox(custom_dropbox)),
        }
    }
}

impl Default for CloudState {
    fn default() -> Self {
        Self::new()
    }
}
