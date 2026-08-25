//! Shared cloud clients used by upload worker and settings/connect commands.
//!
//! Phase 16a: Dropbox clients are resolved per AMS profile within two pools
//! (`native` / `custom_api`). `dropbox` / `custom_dropbox` expose the **active**
//! client for each pool (legacy call sites + Settings).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::cloud::active_slot::ActiveDropboxSlot;
use crate::cloud::dropbox::{DropboxClient, DropboxPool, DropboxSecretKeys};
use crate::cloud::CustomApiClient;
use crate::storage::config::ConfigStore;

#[derive(Default)]
struct PoolRegistry {
    clients: HashMap<String, Arc<DropboxClient>>,
    active_id: Option<String>,
    /// Used when no active profile exists yet (pre-migration / empty pool).
    legacy: Arc<DropboxClient>,
}

impl PoolRegistry {
    fn new(pool: DropboxPool) -> Self {
        Self {
            clients: HashMap::new(),
            active_id: None,
            legacy: Arc::new(DropboxClient::with_keys(pool.legacy_keys())),
        }
    }

    fn ensure(&mut self, pool: DropboxPool, ams_id: &str) -> Arc<DropboxClient> {
        let id = ams_id.trim();
        if id.is_empty() {
            return Arc::clone(&self.legacy);
        }
        self.clients
            .entry(id.to_string())
            .or_insert_with(|| {
                Arc::new(DropboxClient::with_keys(DropboxSecretKeys::for_account(
                    pool, id,
                )))
            })
            .clone()
    }

    fn active(&self) -> Arc<DropboxClient> {
        if let Some(id) = self.active_id.as_deref() {
            if let Some(client) = self.clients.get(id) {
                return Arc::clone(client);
            }
        }
        Arc::clone(&self.legacy)
    }

    fn set_active(&mut self, pool: DropboxPool, ams_id: Option<&str>) -> Arc<DropboxClient> {
        match ams_id.map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => {
                let client = self.ensure(pool, id);
                self.active_id = Some(id.to_string());
                client
            }
            None => {
                self.active_id = None;
                Arc::clone(&self.legacy)
            }
        }
    }

    fn remove(&mut self, ams_id: &str) {
        self.clients.remove(ams_id.trim());
        if self.active_id.as_deref() == Some(ams_id.trim()) {
            self.active_id = None;
        }
    }
}

struct RegistryInner {
    native: PoolRegistry,
    custom: PoolRegistry,
}

#[derive(Clone)]
pub struct CloudState {
    registry: Arc<Mutex<RegistryInner>>,
    /// Active native Dropbox client (updated when active profile changes).
    native_slot: ActiveDropboxSlot,
    /// Active custom-api Dropbox client (shared with `CustomApiClient`).
    custom_slot: ActiveDropboxSlot,
    pub custom_api: Arc<CustomApiClient>,
}

impl CloudState {
    pub fn new() -> Self {
        Self::from_active_ids(None, None)
    }

    pub fn from_config(config: &ConfigStore) -> Self {
        let native = config.get("active_dropbox_account_id", Some(""));
        let custom = config.get("active_custom_dropbox_account_id", Some(""));
        Self::from_active_ids(
            Some(native.trim()).filter(|s| !s.is_empty()),
            Some(custom.trim()).filter(|s| !s.is_empty()),
        )
    }

    pub fn from_active_ids(native_id: Option<&str>, custom_id: Option<&str>) -> Self {
        let mut inner = RegistryInner {
            native: PoolRegistry::new(DropboxPool::Native),
            custom: PoolRegistry::new(DropboxPool::CustomApi),
        };
        let native_client = inner.native.set_active(DropboxPool::Native, native_id);
        let custom_client = inner.custom.set_active(DropboxPool::CustomApi, custom_id);
        let custom_slot = ActiveDropboxSlot::new(custom_client);
        let native_slot = ActiveDropboxSlot::new(native_client);
        let custom_api = Arc::new(CustomApiClient::with_dropbox_slot(custom_slot.clone()));
        Self {
            registry: Arc::new(Mutex::new(inner)),
            native_slot,
            custom_slot,
            custom_api,
        }
    }

    /// Active native Dropbox client (`selected_cloud_service=dropbox`).
    pub fn dropbox(&self) -> Arc<DropboxClient> {
        self.native_slot.get()
    }

    /// Active Custom-API Dropbox client (direct upload / contact markers).
    pub fn custom_dropbox(&self) -> Arc<DropboxClient> {
        self.custom_slot.get()
    }

    /// Lazy client for a specific AMS profile (creates registry entry if needed).
    pub fn client_for(&self, pool: DropboxPool, ams_id: &str) -> Arc<DropboxClient> {
        let mut guard = match self.registry.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        match pool {
            DropboxPool::Native => guard.native.ensure(pool, ams_id),
            DropboxPool::CustomApi => guard.custom.ensure(pool, ams_id),
        }
    }

    pub fn active_account_id(&self, pool: DropboxPool) -> Option<String> {
        let guard = match self.registry.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        match pool {
            DropboxPool::Native => guard.native.active_id.clone(),
            DropboxPool::CustomApi => guard.custom.active_id.clone(),
        }
    }

    /// Switch active profile for a pool and refresh the shared client slot.
    pub fn set_active_account(&self, pool: DropboxPool, ams_id: Option<&str>) -> Arc<DropboxClient> {
        let mut guard = match self.registry.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        let client = match pool {
            DropboxPool::Native => {
                let c = guard.native.set_active(pool, ams_id);
                self.native_slot.set(Arc::clone(&c));
                c
            }
            DropboxPool::CustomApi => {
                let c = guard.custom.set_active(pool, ams_id);
                self.custom_slot.set(Arc::clone(&c));
                c
            }
        };
        client
    }

    pub fn forget_account(&self, pool: DropboxPool, ams_id: &str) {
        let mut guard = match self.registry.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        match pool {
            DropboxPool::Native => {
                guard.native.remove(ams_id);
                self.native_slot.set(guard.native.active());
            }
            DropboxPool::CustomApi => {
                guard.custom.remove(ams_id);
                self.custom_slot.set(guard.custom.active());
            }
        }
    }

    /// Clone of the custom-api Dropbox slot (for temporary job pins).
    pub fn custom_dropbox_slot(&self) -> ActiveDropboxSlot {
        self.custom_slot.clone()
    }

    /// Point the shared custom Dropbox slot at `client` without changing active id.
    pub fn pin_custom_dropbox_slot(&self, client: Arc<DropboxClient>) {
        self.custom_slot.set(client);
    }
}

impl Default for CloudState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_isolates_pools_and_profiles() {
        let state = CloudState::from_active_ids(Some("n1"), Some("c1"));
        let n1 = state.client_for(DropboxPool::Native, "n1");
        let n2 = state.client_for(DropboxPool::Native, "n2");
        let c1 = state.client_for(DropboxPool::CustomApi, "c1");
        assert!(!Arc::ptr_eq(&n1, &n2));
        assert!(!Arc::ptr_eq(&n1, &c1));
        assert!(Arc::ptr_eq(&state.dropbox(), &n1));
        assert!(Arc::ptr_eq(&state.custom_dropbox(), &c1));

        state.set_active_account(DropboxPool::Native, Some("n2"));
        assert!(Arc::ptr_eq(&state.dropbox(), &n2));
        assert_eq!(state.active_account_id(DropboxPool::Native).as_deref(), Some("n2"));
        // Custom pool unchanged
        assert!(Arc::ptr_eq(&state.custom_dropbox(), &c1));
    }
}
