//! Phase 16c — Soft-Active / Hard-Guard / OAuth identity / checkpoint binding invariants.
//!
//! Soft-Active: switching the active profile only changes the default for *new* jobs;
//! queued and in-flight jobs keep their frozen `DropboxAccountBinding`.
//! Hard-Guard: delete/disconnect refuse while the upload queue still holds jobs for that AMS id.

use serde_json::{json, Map, Value};

use crate::cloud::dropbox::{DropboxAccountInfo, DropboxPool, DropboxSecretKeys};
use crate::storage::dropbox_accounts::{DropboxAccountRow, DropboxAccountStore};
use crate::storage::logging;
use crate::storage::secrets;
use crate::upload::registry::UploadQueueRegistry;

/// Soft-Active: active switch must never rebind jobs already in the queue.
///
/// Callers only persist the new active id + refresh the shared client slot.
/// Upload resolution always prefers `job.dropbox_binding` / history binding.
pub fn soft_active_switch_is_safe() -> bool {
    true
}

/// Hard-Guard before deleting or disconnecting an AMS Dropbox profile.
pub fn assert_can_delete_or_disconnect(
    registry: &UploadQueueRegistry,
    ams_id: &str,
) -> Result<(), String> {
    registry.assert_can_remove_account(ams_id)
}

/// Outcome of applying Dropbox `account_id` after OAuth / connect (D5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OauthIdentityOutcome {
    /// Tokens stay on `ams_id`; metadata refreshed.
    Updated { ams_id: String },
    /// Same Dropbox account already owned by another AMS profile → tokens moved there.
    AppliedToExisting {
        requested_ams_id: String,
        existing: DropboxAccountRow,
    },
    /// Profile already bound to a different Dropbox `account_id`.
    RejectedMismatch {
        ams_id: String,
        expected_dropbox_id: String,
        got_dropbox_id: String,
    },
}

impl OauthIdentityOutcome {
    pub fn message(&self) -> String {
        match self {
            Self::Updated { .. } => "Dropbox-Konto aktualisiert.".into(),
            Self::AppliedToExisting { existing, .. } => format!(
                "Gleiche Dropbox-Konto-ID wie Profil „{}“ — Token dort aktualisiert (kein zweites Profil).",
                if existing.label.trim().is_empty() {
                    existing.email.as_str()
                } else {
                    existing.label.as_str()
                }
            ),
            Self::RejectedMismatch {
                expected_dropbox_id,
                got_dropbox_id,
                ..
            } => format!(
                "Dieses AMS-Profil ist bereits an Dropbox-Konto „{expected_dropbox_id}“ gebunden; \
                 OAuth lieferte „{got_dropbox_id}“. Bitte neues Profil anlegen oder das passende Profil verbinden."
            ),
        }
    }

    pub fn is_ok(&self) -> bool {
        !matches!(self, Self::RejectedMismatch { .. })
    }
}

/// After OAuth/connect tokens land on `ams_id`'s keyring keys, reconcile Dropbox `account_id`.
///
/// - Same id (or empty on profile) → update metadata (token-update).
/// - Same Dropbox id already on another profile in the pool → move tokens there.
/// - Different id than profile already stores → hard reject (clear new refresh token).
pub fn apply_oauth_account_identity(
    accounts: &DropboxAccountStore,
    pool: DropboxPool,
    ams_id: &str,
    info: &DropboxAccountInfo,
) -> Result<OauthIdentityOutcome, String> {
    let ams_id = ams_id.trim();
    if ams_id.is_empty() {
        return Err("account_id (AMS) fehlt.".into());
    }
    let got = info.account_id.trim();
    if got.is_empty() {
        return Err("Dropbox lieferte keine account_id.".into());
    }

    let row = accounts
        .get(ams_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Dropbox-Profil nicht gefunden: {ams_id}"))?;
    if row.pool != pool.as_str() {
        return Err(format!(
            "Profil {ams_id} gehört zu Pool „{}“, nicht „{}“.",
            row.pool,
            pool.as_str()
        ));
    }

    let existing_dbxid = row.dropbox_account_id.trim();
    if !existing_dbxid.is_empty() && existing_dbxid != got {
        // Hard warn: do not overwrite identity; drop the freshly written refresh token.
        let keys = DropboxSecretKeys::for_account(pool, ams_id);
        let _ = secrets::delete_secret(&keys.refresh_token);
        logging::log_warn(&format!(
            "OAuth-Account-Mismatch für {ams_id}: erwartet {existing_dbxid}, got {got}"
        ));
        return Ok(OauthIdentityOutcome::RejectedMismatch {
            ams_id: ams_id.to_string(),
            expected_dropbox_id: existing_dbxid.to_string(),
            got_dropbox_id: got.to_string(),
        });
    }

    if let Some(other) = accounts
        .find_by_dropbox_account_id(pool, got)
        .map_err(|e| e.to_string())?
    {
        if other.id != ams_id {
            // Token-Update on the existing profile; clear shell profile refresh token.
            copy_refresh_token(pool, ams_id, &other.id)?;
            let keys_from = DropboxSecretKeys::for_account(pool, ams_id);
            let _ = secrets::delete_secret(&keys_from.refresh_token);
            let updated = accounts
                .update_identity(
                    &other.id,
                    got,
                    &info.email,
                    &info.display_name,
                    &info.app_key_hint,
                )
                .map_err(|e| e.to_string())?;
            let _ = accounts.maybe_set_app_folder_name_from_discovery(&other.id, &info.app_name);
            logging::log_info(&format!(
                "OAuth: Dropbox-Konto {got} → bestehendes Profil {} (statt {ams_id})",
                other.id
            ));
            return Ok(OauthIdentityOutcome::AppliedToExisting {
                requested_ams_id: ams_id.to_string(),
                existing: updated,
            });
        }
    }

    accounts
        .update_identity(
            ams_id,
            got,
            &info.email,
            &info.display_name,
            &info.app_key_hint,
        )
        .map_err(|e| e.to_string())?;
    let _ = accounts.maybe_set_app_folder_name_from_discovery(ams_id, &info.app_name);
    Ok(OauthIdentityOutcome::Updated {
        ams_id: ams_id.to_string(),
    })
}

fn copy_refresh_token(pool: DropboxPool, from_ams: &str, to_ams: &str) -> Result<(), String> {
    let from = DropboxSecretKeys::for_account(pool, from_ams);
    let to = DropboxSecretKeys::for_account(pool, to_ams);
    // Prefer app key/secret on the existing profile; only move refresh token (+ seed keys if empty).
    if let Some(token) = secrets::get_secret(&from.refresh_token)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.is_empty())
    {
        secrets::save_secret(&to.refresh_token, &token).map_err(|e| e.to_string())?;
    }
    for (src, dst) in [
        (from.app_key.as_str(), to.app_key.as_str()),
        (from.app_secret.as_str(), to.app_secret.as_str()),
    ] {
        let dst_empty = secrets::get_secret(dst)
            .ok()
            .flatten()
            .filter(|s| !s.is_empty())
            .is_none();
        if dst_empty {
            if let Some(v) = secrets::get_secret(src)
                .ok()
                .flatten()
                .filter(|s| !s.is_empty())
            {
                let _ = secrets::save_secret(dst, &v);
            }
        }
    }
    Ok(())
}

/// Multi-account pools must not upload without a frozen binding (Settings-Verify exempt).
pub fn assert_upload_has_binding_when_required(
    pool: DropboxPool,
    binding_ams_id: Option<&str>,
    accounts: &DropboxAccountStore,
) -> Result<(), String> {
    if binding_ams_id.map(str::trim).filter(|s| !s.is_empty()).is_some() {
        return Ok(());
    }
    let n = accounts.list(pool).map_err(|e| e.to_string())?.len();
    if n > 1 {
        return Err(format!(
            "Upload ohne Konto-Bindung bei {n} Profilen im Pool „{}“. Job erneut anlegen.",
            pool.as_str()
        ));
    }
    Ok(())
}

/// Write AMS binding into a checkpoint object (native / direct-dropbox kinds).
pub fn merge_checkpoint_binding(
    payload: &mut Map<String, Value>,
    ams_id: Option<&str>,
    pool: Option<DropboxPool>,
) {
    if let Some(id) = ams_id.map(str::trim).filter(|s| !s.is_empty()) {
        payload.insert("dropbox_account_ams_id".into(), json!(id));
    }
    if let Some(pool) = pool {
        payload.insert("dropbox_account_pool".into(), json!(pool.as_str()));
    }
}

/// Resume only when checkpoint binding matches the uploading client (or checkpoint is legacy).
pub fn assert_checkpoint_binding_matches(
    checkpoint: &Value,
    client_ams_id: Option<&str>,
    client_pool: DropboxPool,
) -> Result<(), String> {
    let ck_ams = checkpoint
        .get("dropbox_account_ams_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if ck_ams.is_empty() {
        // Legacy checkpoint without binding — allowed (pre-16c).
        return Ok(());
    }
    let client = client_ams_id.map(str::trim).filter(|s| !s.is_empty());
    let Some(client) = client else {
        return Err(
            "Checkpoint ist an ein Dropbox-Profil gebunden, aktueller Client hat keines. Resume verweigert."
                .into(),
        );
    };
    if ck_ams != client {
        return Err(format!(
            "Checkpoint-Konto ({ck_ams}) stimmt nicht mit Upload-Konto ({client}) überein. Resume verweigert."
        ));
    }
    let ck_pool = checkpoint
        .get("dropbox_account_pool")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !ck_pool.is_empty() {
        let parsed = DropboxPool::parse(ck_pool).map_err(|e| e.to_string())?;
        if parsed != client_pool {
            return Err(format!(
                "Checkpoint-Pool („{}“) stimmt nicht mit Upload-Pool („{}“) überein. Resume verweigert.",
                parsed.as_str(),
                client_pool.as_str()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::binding::DropboxAccountBinding;
    use crate::constants::CONFIG_DB_FILE;
    use crate::model::kunde::Kunde;
    use crate::upload::registry::UploadJob;
    use tempfile::tempdir;
    use tokio::sync::mpsc::unbounded_channel;

    fn open_accounts() -> (tempfile::TempDir, DropboxAccountStore) {
        let dir = tempdir().unwrap();
        let db = dir.path().join(CONFIG_DB_FILE);
        let accounts = DropboxAccountStore::open_at(db).unwrap();
        (dir, accounts)
    }

    #[test]
    fn soft_active_flag_documents_invariant() {
        assert!(soft_active_switch_is_safe());
    }

    #[test]
    fn hard_guard_blocks_when_queue_bound() {
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        let dir = tempdir().unwrap();
        let job_dir = dir.path().join("j1");
        std::fs::create_dir(&job_dir).unwrap();
        let binding = DropboxAccountBinding {
            ams_id: "ams-a".into(),
            pool: DropboxPool::Native,
            dropbox_account_id: "dbid:1".into(),
            email: "a@b.de".into(),
        };
        let job = UploadJob {
            dir_path: job_dir,
            kunde: Kunde::default(),
            use_dropbox_client: false,
            correlation_id: None,
            append: None,
            dropbox_binding: Some(binding),
        };
        assert!(registry.enqueue(&tx, job, false));
        assert_eq!(registry.bound_job_count("ams-a"), 1);
        let err = assert_can_delete_or_disconnect(&registry, "ams-a").unwrap_err();
        assert!(err.contains("gebunden"), "{err}");
        assert!(assert_can_delete_or_disconnect(&registry, "ams-other").is_ok());
    }

    #[test]
    fn checkpoint_mismatch_refuses_resume() {
        let ck = json!({
            "dropbox_account_ams_id": "ams-a",
            "dropbox_account_pool": "native",
        });
        let err = assert_checkpoint_binding_matches(&ck, Some("ams-b"), DropboxPool::Native)
            .unwrap_err();
        assert!(err.contains("Resume verweigert"), "{err}");
        assert!(
            assert_checkpoint_binding_matches(&ck, Some("ams-a"), DropboxPool::Native).is_ok()
        );
        assert!(assert_checkpoint_binding_matches(&json!({}), Some("ams-a"), DropboxPool::Native).is_ok());
    }

    #[test]
    fn checkpoint_pool_mismatch_refuses() {
        let ck = json!({
            "dropbox_account_ams_id": "ams-a",
            "dropbox_account_pool": "custom_api",
        });
        let err = assert_checkpoint_binding_matches(&ck, Some("ams-a"), DropboxPool::Native)
            .unwrap_err();
        assert!(err.contains("Pool"), "{err}");
    }

    #[test]
    fn unbound_upload_blocked_when_multiple_profiles() {
        let (_dir, accounts) = open_accounts();
        accounts.create(DropboxPool::Native, "A").unwrap();
        accounts.create(DropboxPool::Native, "B").unwrap();
        let err =
            assert_upload_has_binding_when_required(DropboxPool::Native, None, &accounts).unwrap_err();
        assert!(err.contains("ohne Konto-Bindung"), "{err}");
        assert!(assert_upload_has_binding_when_required(
            DropboxPool::Native,
            Some("ams-x"),
            &accounts
        )
        .is_ok());
    }

    #[test]
    fn oauth_rejects_identity_mismatch() {
        let (_dir, accounts) = open_accounts();
        let row = accounts.create(DropboxPool::Native, "A").unwrap();
        accounts
            .update_identity(&row.id, "dbid:old", "old@x.de", "Old", "hint")
            .unwrap();
        let info = DropboxAccountInfo {
            account_id: "dbid:new".into(),
            display_name: "New".into(),
            email: "new@x.de".into(),
            profile_photo_url: String::new(),
            app_name: String::new(),
            app_key_hint: "hint".into(),
            token_valid: true,
            used_bytes: 0,
            allocated_bytes: None,
        };
        let outcome =
            apply_oauth_account_identity(&accounts, DropboxPool::Native, &row.id, &info).unwrap();
        assert!(matches!(outcome, OauthIdentityOutcome::RejectedMismatch { .. }));
        assert!(!outcome.is_ok());
    }

    #[test]
    fn oauth_same_id_updates() {
        let (_dir, accounts) = open_accounts();
        let row = accounts.create(DropboxPool::CustomApi, "A").unwrap();
        let info = DropboxAccountInfo {
            account_id: "dbid:1".into(),
            display_name: "N".into(),
            email: "e@x.de".into(),
            profile_photo_url: String::new(),
            app_name: String::new(),
            app_key_hint: "h".into(),
            token_valid: true,
            used_bytes: 0,
            allocated_bytes: None,
        };
        let outcome =
            apply_oauth_account_identity(&accounts, DropboxPool::CustomApi, &row.id, &info).unwrap();
        assert_eq!(
            outcome,
            OauthIdentityOutcome::Updated {
                ams_id: row.id.clone()
            }
        );
        let stored = accounts.get(&row.id).unwrap().unwrap();
        assert_eq!(stored.dropbox_account_id, "dbid:1");
        assert_eq!(stored.email, "e@x.de");
    }

    #[test]
    fn oauth_duplicate_dbxid_redirects_to_existing() {
        let (_dir, accounts) = open_accounts();
        let existing = accounts.create(DropboxPool::Native, "Existing").unwrap();
        accounts
            .update_identity(&existing.id, "dbid:same", "e@x.de", "E", "h")
            .unwrap();
        let shell = accounts.create(DropboxPool::Native, "Shell").unwrap();
        let info = DropboxAccountInfo {
            account_id: "dbid:same".into(),
            display_name: "E2".into(),
            email: "e2@x.de".into(),
            profile_photo_url: String::new(),
            app_name: String::new(),
            app_key_hint: "h2".into(),
            token_valid: true,
            used_bytes: 0,
            allocated_bytes: None,
        };
        let outcome =
            apply_oauth_account_identity(&accounts, DropboxPool::Native, &shell.id, &info).unwrap();
        match outcome {
            OauthIdentityOutcome::AppliedToExisting {
                requested_ams_id,
                existing: row,
            } => {
                assert_eq!(requested_ams_id, shell.id);
                assert_eq!(row.id, existing.id);
                assert_eq!(row.email, "e2@x.de");
            }
            other => panic!("expected AppliedToExisting, got {other:?}"),
        }
    }

    #[test]
    fn merge_checkpoint_writes_fields() {
        let mut map = Map::new();
        merge_checkpoint_binding(&mut map, Some("ams-1"), Some(DropboxPool::CustomApi));
        assert_eq!(map["dropbox_account_ams_id"], "ams-1");
        assert_eq!(map["dropbox_account_pool"], "custom_api");
    }
}
