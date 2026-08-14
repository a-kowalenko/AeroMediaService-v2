//! Shared cloud clients used by upload worker and settings/connect commands.

use std::sync::Arc;

use crate::cloud::{CustomApiClient, DropboxClient};

#[derive(Clone)]
pub struct CloudState {
    pub dropbox: Arc<DropboxClient>,
    pub custom_api: Arc<CustomApiClient>,
}

impl CloudState {
    pub fn new() -> Self {
        Self {
            dropbox: Arc::new(DropboxClient::new()),
            custom_api: Arc::new(CustomApiClient::new()),
        }
    }
}

impl Default for CloudState {
    fn default() -> Self {
        Self::new()
    }
}
