//! Job/History Dropbox account binding (Phase 16b).
//!
//! Active account freezes at claim/enqueue; upload side-paths resolve the bound
//! profile — never silently fall back to whatever is currently active.

use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};

use crate::cloud::active_slot::ActiveDropboxSlot;
use crate::cloud::dropbox::{DropboxClient, DropboxPool};
use crate::cloud::state::CloudState;
use crate::storage::dropbox_accounts::{DropboxAccountRow, DropboxAccountStore};

/// Snapshot of the AMS Dropbox profile bound to a job / history row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DropboxAccountBinding {
    pub ams_id: String,
    pub pool: DropboxPool,
    pub dropbox_account_id: String,
    pub email: String,
}

impl DropboxAccountBinding {
    pub fn from_row(row: &DropboxAccountRow) -> Result<Self, String> {
        let pool = DropboxPool::parse(&row.pool).map_err(|e| e.to_string())?;
        Ok(Self {
            ams_id: row.id.clone(),
            pool,
            dropbox_account_id: row.dropbox_account_id.clone(),
            email: row.email.clone(),
        })
    }

    pub fn from_history_fields(
        ams_id: &str,
        pool_raw: &str,
        dropbox_account_id: &str,
        email: &str,
    ) -> Option<Self> {
        let ams_id = ams_id.trim();
        if ams_id.is_empty() {
            return None;
        }
        let pool = DropboxPool::parse(pool_raw).ok()?;
        Some(Self {
            ams_id: ams_id.to_string(),
            pool,
            dropbox_account_id: dropbox_account_id.trim().to_string(),
            email: email.trim().to_string(),
        })
    }
}

/// Pool that new jobs under `selected_cloud` (+ contact-marker flag) must bind to.
pub fn pool_for_new_job(selected_cloud: &str, _use_dropbox_client: bool) -> DropboxPool {
    match selected_cloud.trim() {
        "custom_api" => DropboxPool::CustomApi,
        _ => DropboxPool::Native,
    }
}

/// Freeze the active profile for a **new** job (claim / enqueue).
///
/// Returns `Ok(None)` when the pool has no profiles yet (pre-migration / tests).
pub fn freeze_active_binding(
    pool: DropboxPool,
    active_ams_id: &str,
    accounts: &DropboxAccountStore,
) -> Result<Option<DropboxAccountBinding>, String> {
    let rows = accounts.list(pool).map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Ok(None);
    }
    let active = active_ams_id.trim();
    if !active.is_empty() {
        let row = accounts
            .get(active)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                format!(
                    "Aktives Dropbox-Profil ({}) nicht gefunden. Bitte in den Einstellungen prüfen.",
                    pool.as_str()
                )
            })?;
        let binding = DropboxAccountBinding::from_row(&row)?;
        if binding.pool != pool {
            return Err(format!(
                "Aktives Profil gehört zu Pool „{}“, erwartet „{}“.",
                binding.pool.as_str(),
                pool.as_str()
            ));
        }
        return Ok(Some(binding));
    }
    if rows.len() == 1 {
        return Ok(Some(DropboxAccountBinding::from_row(&rows[0])?));
    }
    Err(format!(
        "Kein aktives Dropbox-Konto für Pool „{}“ gewählt ({} Profile vorhanden).",
        pool.as_str(),
        rows.len()
    ))
}

/// Resolve binding for Retry / Append / Resend from history (or sole-profile fallback).
///
/// Does **not** use the currently active account when a binding is present.
pub fn resolve_binding_for_history(
    entry: &Value,
    expected_pool: DropboxPool,
    accounts: &DropboxAccountStore,
) -> Result<DropboxAccountBinding, String> {
    let ams_id = json_str(entry, "dropbox_account_ams_id");
    if !ams_id.is_empty() {
        let pool_raw = json_str(entry, "dropbox_account_pool");
        let pool = if pool_raw.is_empty() {
            expected_pool
        } else {
            DropboxPool::parse(pool_raw).map_err(|e| e.to_string())?
        };
        if pool != expected_pool {
            return Err(format!(
                "Historieneintrag ist an Pool „{}“ gebunden, aktueller Pfad erwartet „{}“.",
                pool.as_str(),
                expected_pool.as_str()
            ));
        }
        match accounts.get(ams_id).map_err(|e| e.to_string())? {
            Some(row) => {
                let binding = DropboxAccountBinding::from_row(&row)?;
                if binding.pool != pool {
                    return Err(format!(
                        "Gebundenes Profil „{ams_id}“ liegt im falschen Pool."
                    ));
                }
                Ok(binding)
            }
            None => Err(format!(
                "Gebundenes Dropbox-Profil fehlt ({ams_id}). Konto wurde gelöscht oder getrennt."
            )),
        }
    } else {
        // Legacy history without binding: only safe when exactly one profile exists.
        let rows = accounts.list(expected_pool).map_err(|e| e.to_string())?;
        match rows.len() {
            1 => DropboxAccountBinding::from_row(&rows[0]),
            0 => Err(format!(
                "Kein Dropbox-Konto für Pool „{}“ konfiguriert.",
                expected_pool.as_str()
            )),
            n => Err(format!(
                "Historieneintrag ohne Konto-Bindung und {n} Profile im Pool „{}“. \
                 Bitte Konto in der Historie bestätigen oder Job erneut mit Binding anlegen.",
                expected_pool.as_str()
            )),
        }
    }
}

/// Binding stored on an append target (parent job), if any.
pub fn binding_from_append_fields(
    ams_id: Option<&str>,
    pool_raw: Option<&str>,
    dropbox_account_id: Option<&str>,
    email: Option<&str>,
) -> Option<DropboxAccountBinding> {
    DropboxAccountBinding::from_history_fields(
        ams_id.unwrap_or(""),
        pool_raw.unwrap_or(""),
        dropbox_account_id.unwrap_or(""),
        email.unwrap_or(""),
    )
}

pub fn merge_binding_into_history(history: &mut Value, binding: Option<&DropboxAccountBinding>) {
    let Some(binding) = binding else {
        return;
    };
    if let Some(obj) = history.as_object_mut() {
        obj.insert(
            "dropbox_account_ams_id".into(),
            json!(binding.ams_id.clone()),
        );
        obj.insert(
            "dropbox_account_pool".into(),
            json!(binding.pool.as_str()),
        );
        if !binding.dropbox_account_id.is_empty() {
            obj.insert(
                "dropbox_account_id".into(),
                json!(binding.dropbox_account_id.clone()),
            );
        }
        if !binding.email.is_empty() {
            obj.insert(
                "dropbox_account_email".into(),
                json!(binding.email.clone()),
            );
        }
    }
}

pub fn client_for_binding(
    cloud: &CloudState,
    binding: &DropboxAccountBinding,
) -> Arc<DropboxClient> {
    cloud.client_for(binding.pool, &binding.ams_id)
}

/// Temporarily point the custom-api Dropbox slot at a bound profile (Direct-Dropbox path).
/// Restores the previous client on drop. Does **not** change the Soft-Active account id.
/// Only use when the job/history already carries the same binding (16c: no silent override).
pub struct CustomDropboxPin {
    slot: ActiveDropboxSlot,
    previous: Arc<DropboxClient>,
}

impl CustomDropboxPin {
    pub fn pin(cloud: &CloudState, ams_id: &str) -> Self {
        let previous = cloud.custom_dropbox();
        let bound = cloud.client_for(DropboxPool::CustomApi, ams_id);
        cloud.pin_custom_dropbox_slot(bound);
        Self {
            slot: cloud.custom_dropbox_slot(),
            previous,
        }
    }
}

impl Drop for CustomDropboxPin {
    fn drop(&mut self) {
        self.slot.set(Arc::clone(&self.previous));
    }
}

fn json_str<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::CONFIG_DB_FILE;
    use tempfile::tempdir;

    fn open_accounts() -> (tempfile::TempDir, DropboxAccountStore) {
        let dir = tempdir().unwrap();
        let db = dir.path().join(CONFIG_DB_FILE);
        let accounts = DropboxAccountStore::open_at(db).unwrap();
        (dir, accounts)
    }

    #[test]
    fn freeze_uses_active_not_other_profile() {
        let (_dir, accounts) = open_accounts();
        let a = accounts.create(DropboxPool::Native, "A").unwrap();
        let _b = accounts.create(DropboxPool::Native, "B").unwrap();
        let binding = freeze_active_binding(DropboxPool::Native, &a.id, &accounts)
            .unwrap()
            .unwrap();
        assert_eq!(binding.ams_id, a.id);
        assert_eq!(binding.pool, DropboxPool::Native);
    }

    #[test]
    fn freeze_errors_when_multiple_and_no_active() {
        let (_dir, accounts) = open_accounts();
        accounts.create(DropboxPool::Native, "A").unwrap();
        accounts.create(DropboxPool::Native, "B").unwrap();
        let err = freeze_active_binding(DropboxPool::Native, "", &accounts).unwrap_err();
        assert!(err.contains("Kein aktives"), "{err}");
    }

    #[test]
    fn freeze_sole_profile_without_active() {
        let (_dir, accounts) = open_accounts();
        let a = accounts.create(DropboxPool::CustomApi, "Only").unwrap();
        let binding = freeze_active_binding(DropboxPool::CustomApi, "", &accounts)
            .unwrap()
            .unwrap();
        assert_eq!(binding.ams_id, a.id);
    }

    #[test]
    fn freeze_empty_pool_returns_none() {
        let (_dir, accounts) = open_accounts();
        assert!(freeze_active_binding(DropboxPool::Native, "", &accounts)
            .unwrap()
            .is_none());
    }

    #[test]
    fn resolve_history_uses_parent_not_active() {
        let (_dir, accounts) = open_accounts();
        let parent = accounts.create(DropboxPool::Native, "Parent").unwrap();
        let _active = accounts.create(DropboxPool::Native, "Active").unwrap();
        let entry = json!({
            "dropbox_account_ams_id": parent.id,
            "dropbox_account_pool": "native",
            "dropbox_account_email": "p@x.de",
        });
        let binding =
            resolve_binding_for_history(&entry, DropboxPool::Native, &accounts).unwrap();
        assert_eq!(binding.ams_id, parent.id);
        assert_ne!(binding.ams_id, _active.id);
    }

    #[test]
    fn resolve_history_missing_profile_errors() {
        let (_dir, accounts) = open_accounts();
        let entry = json!({
            "dropbox_account_ams_id": "missing-uuid",
            "dropbox_account_pool": "native",
        });
        let err =
            resolve_binding_for_history(&entry, DropboxPool::Native, &accounts).unwrap_err();
        assert!(err.contains("fehlt"), "{err}");
    }

    #[test]
    fn resolve_legacy_requires_sole_profile() {
        let (_dir, accounts) = open_accounts();
        accounts.create(DropboxPool::Native, "A").unwrap();
        accounts.create(DropboxPool::Native, "B").unwrap();
        let err =
            resolve_binding_for_history(&json!({}), DropboxPool::Native, &accounts).unwrap_err();
        assert!(err.contains("ohne Konto-Bindung"), "{err}");
    }

    #[test]
    fn resolve_legacy_sole_profile_ok() {
        let (_dir, accounts) = open_accounts();
        let a = accounts.create(DropboxPool::CustomApi, "Only").unwrap();
        let binding =
            resolve_binding_for_history(&json!({}), DropboxPool::CustomApi, &accounts).unwrap();
        assert_eq!(binding.ams_id, a.id);
    }

    #[test]
    fn client_for_binding_ignores_active_switch() {
        let state = CloudState::from_active_ids(Some("n1"), Some("c1"));
        let binding = DropboxAccountBinding {
            ams_id: "n1".into(),
            pool: DropboxPool::Native,
            dropbox_account_id: String::new(),
            email: String::new(),
        };
        let bound = client_for_binding(&state, &binding);
        state.set_active_account(DropboxPool::Native, Some("n2"));
        let still = client_for_binding(&state, &binding);
        assert!(Arc::ptr_eq(&bound, &still));
        assert!(!Arc::ptr_eq(&still, &state.dropbox()));
    }

    #[test]
    fn pool_for_new_job_routes_clouds() {
        assert_eq!(pool_for_new_job("dropbox", false), DropboxPool::Native);
        assert_eq!(pool_for_new_job("custom_api", true), DropboxPool::CustomApi);
        assert_eq!(pool_for_new_job("custom_api", false), DropboxPool::CustomApi);
    }

    #[test]
    fn merge_binding_writes_history_keys() {
        let mut history = json!({"dir_name": "x"});
        let binding = DropboxAccountBinding {
            ams_id: "ams-1".into(),
            pool: DropboxPool::Native,
            dropbox_account_id: "dbid:1".into(),
            email: "a@b.de".into(),
        };
        merge_binding_into_history(&mut history, Some(&binding));
        assert_eq!(history["dropbox_account_ams_id"], "ams-1");
        assert_eq!(history["dropbox_account_pool"], "native");
        assert_eq!(history["dropbox_account_email"], "a@b.de");
    }
}
