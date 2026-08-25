//! Shared swappable handle for the active Dropbox client in a pool.

use std::sync::{Arc, Mutex};

use crate::cloud::dropbox::DropboxClient;

/// Shared handle to an active Dropbox client (swappable on active-account change).
#[derive(Clone)]
pub struct ActiveDropboxSlot {
    slot: Arc<Mutex<Arc<DropboxClient>>>,
}

impl ActiveDropboxSlot {
    pub fn new(client: Arc<DropboxClient>) -> Self {
        Self {
            slot: Arc::new(Mutex::new(client)),
        }
    }

    pub fn get(&self) -> Arc<DropboxClient> {
        match self.slot.lock() {
            Ok(g) => Arc::clone(&g),
            Err(e) => Arc::clone(&e.into_inner()),
        }
    }

    pub fn set(&self, client: Arc<DropboxClient>) {
        match self.slot.lock() {
            Ok(mut g) => *g = client,
            Err(e) => *e.into_inner() = client,
        }
    }
}
