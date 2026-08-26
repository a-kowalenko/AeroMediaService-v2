//! Proxied session upload and sequential direct-Dropbox + manifest submit.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::{extract_customer_url, guess_mime, parse_next_offset, CustomApiClient, CHUNK_BYTES};
use crate::cloud::dropbox::{self, DropboxSessionResume};
use crate::cloud::guards::{assert_checkpoint_binding_matches, merge_checkpoint_binding};
use crate::cloud::manifest::build_manifest_v11;
use crate::cloud::traits::{should_skip_upload_file, CloudError};
use crate::events;
use crate::model::kunde::Kunde;
use crate::storage::config::runtime_setting;
use crate::storage::logging;
use crate::upload::checkpoint::{
    clear_checkpoint, load_checkpoint, manifest_fingerprint, save_checkpoint,
};
use crate::upload::control::UploadControl;
use crate::util::link_shortener;

struct ProxiedFile {
    name: String,
    size: u64,
    mime: String,
    local_path: std::path::PathBuf,
}

impl CustomApiClient {
    pub(super) async fn upload_directory_inner(
        &self,
        local_dir_path: &Path,
        remote_base_path: &str,
        control: &UploadControl,
        kunde: &Kunde,
    ) -> Result<bool, CloudError> {
        if !self.is_connected() && !self.connect_api().await.unwrap_or(false) {
            logging::log_error("Upload fehlgeschlagen: Nicht verbunden.");
            return Ok(false);
        }

        logging::log_info(&format!(
            "Beginne Session-Upload von '{}'",
            local_dir_path.display()
        ));
        self.set_last_kunde(Some(kunde.clone()));
        let upload_mode = runtime_setting("custom_api_upload_mode");
        if crate::constants::is_direct_dropbox_upload_mode(&upload_mode) {
            logging::log_info("Custom API Upload-Modus: Dropbox + Manifest v1.1 (paths_only)");
            return self
                .upload_direct_dropbox(local_dir_path, remote_base_path, control, kunde)
                .await;
        }
        logging::log_info("Custom API Upload-Modus: proxied_session");
        self.upload_proxied_session(local_dir_path, control, kunde)
            .await
    }

    async fn upload_proxied_session(
        &self,
        local_dir_path: &Path,
        control: &UploadControl,
        kunde: &Kunde,
    ) -> Result<bool, CloudError> {
        let mut files = collect_proxied_files(local_dir_path);
        if files.is_empty() {
            logging::log_error("Keine Dateien zum Hochladen gefunden.");
            return Ok(false);
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));
        let manifest: Vec<Value> = files
            .iter()
            .map(|f| json!({"name": f.name, "size": f.size, "type": f.mime}))
            .collect();
        let manifest_fp = manifest_fingerprint(&manifest);
        let folder_name = local_dir_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let total_size: u64 = files.iter().map(|f| f.size).sum();

        let raw_ck = load_checkpoint(local_dir_path);
        let mut resume_ck = None;
        if let Some(raw) = raw_ck {
            if raw.get("kind").and_then(Value::as_str) == Some("custom_api_proxied")
                && raw.get("manifest_fp").and_then(Value::as_str) == Some(manifest_fp.as_str())
            {
                resume_ck = Some(raw);
            } else {
                logging::log_warn("Upload-Checkpoint verworfen (Manifest/Typ passt nicht).");
                clear_checkpoint(local_dir_path);
            }
        }

        if let Some(ck) = resume_ck.as_ref() {
            if ck.get("phase").and_then(Value::as_str) == Some("finalizing") {
                if let Some(sid) = ck.get("api_session_id").and_then(Value::as_str) {
                    logging::log_info(
                        "Checkpoint: Finalisierung der Upload-Session wird fortgesetzt.",
                    );
                    self.set_last_session_id(Some(sid.to_string()));
                    if let Some(oid) = ck.get("order_id").and_then(value_as_string) {
                        self.set_last_order_id(Some(oid));
                    }
                    events::emit_status("Finalisiere Upload...");
                    control.wait_if_paused().await?;
                    return match self.finalize_or_poll(sid, control).await {
                        Ok(_) => {
                            clear_checkpoint(local_dir_path);
                            events::emit_status("Upload abgeschlossen.");
                            Ok(true)
                        }
                        Err(e) if e.is_cancelled() => Err(e),
                        Err(e) => {
                            logging::log_error(&format!("Finalize (Recovery) fehlgeschlagen: {e}"));
                            events::emit_status(format!("Fehler: {e}"));
                            Ok(false)
                        }
                    };
                }
            }
        }

        let mut session_id = resume_ck
            .as_ref()
            .and_then(|ck| ck.get("api_session_id").and_then(Value::as_str))
            .map(str::to_string);
        let mut order_id = resume_ck
            .as_ref()
            .and_then(|ck| ck.get("order_id").and_then(value_as_string));

        if let Some(sid) = session_id.as_ref() {
            self.set_last_session_id(Some(sid.clone()));
            if let Some(oid) = order_id.as_ref() {
                self.set_last_order_id(Some(oid.clone()));
            }
            logging::log_info(&format!(
                "Proxied-Session wird fortgesetzt (session_id={sid}, next_file_index={:?}).",
                resume_ck.as_ref().and_then(|ck| ck.get("next_file_index"))
            ));
        }

        if session_id.is_none() {
            match self
                .initialize_direct_session(&files, &folder_name, kunde, control)
                .await
            {
                Ok(data) => {
                    let sid = data
                        .get("session_id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            CloudError::Message(
                                "Session-Initialisierung fehlgeschlagen (keine session_id).".into(),
                            )
                        })?
                        .to_string();
                    if data.get("ok").and_then(Value::as_bool) == Some(false) {
                        return Err(CloudError::Message(format!(
                            "Session-Initialisierung fehlgeschlagen: {data}"
                        )));
                    }
                    order_id = data.get("order_id").and_then(value_as_string);
                    self.set_last_session_id(Some(sid.clone()));
                    if let Some(oid) = order_id.as_ref() {
                        self.set_last_order_id(Some(oid.clone()));
                    }
                    logging::log_info(&format!(
                        "Upload-Session initialisiert: session_id={sid}{}",
                        order_id
                            .as_ref()
                            .map(|o| format!(", order_id={o}"))
                            .unwrap_or_default()
                    ));
                    let _ = save_checkpoint(
                        local_dir_path,
                        &json!({
                            "kind": "custom_api_proxied",
                            "manifest_fp": manifest_fp,
                            "folder_name": folder_name,
                            "api_session_id": sid,
                            "order_id": order_id,
                            "total_size": total_size,
                            "completed_bytes": 0,
                            "next_file_index": 0,
                            "custom_active": Value::Null,
                            "phase": "uploading",
                        }),
                    );
                    session_id = Some(sid);
                }
                Err(e) if e.is_cancelled() => return Err(e),
                Err(e) => {
                    logging::log_error(&format!("Session-Initialisierung fehlgeschlagen: {e}"));
                    events::emit_status(format!("Fehler: {e}"));
                    return Ok(false);
                }
            }
        }

        let session_id = session_id.unwrap();
        let mut start_idx = 0usize;
        let mut resume_server_offset = None;
        if let Some(ck) = resume_ck.as_ref() {
            start_idx = ck
                .get("next_file_index")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            start_idx = start_idx.min(files.len());
            let ca = ck.get("custom_active").cloned().unwrap_or(Value::Null);
            if start_idx < files.len()
                && ca.get("file_name").and_then(Value::as_str)
                    == Some(files[start_idx].name.as_str())
            {
                let ro = ca.get("server_offset").and_then(Value::as_u64).unwrap_or(0);
                if ro > 0 {
                    resume_server_offset = Some(ro);
                }
            }
        }

        let mut uploaded_bytes = resume_ck
            .as_ref()
            .and_then(|ck| ck.get("completed_bytes").and_then(Value::as_u64))
            .unwrap_or(0);

        let save_ck = |extra: Value| {
            let mut payload = json!({
                "kind": "custom_api_proxied",
                "manifest_fp": manifest_fp,
                "folder_name": folder_name,
                "api_session_id": session_id,
                "order_id": order_id,
                "total_size": total_size,
                "phase": "uploading",
            });
            if let (Some(obj), Some(extra_obj)) = (payload.as_object_mut(), extra.as_object()) {
                for (k, v) in extra_obj {
                    obj.insert(k.clone(), v.clone());
                }
            }
            save_checkpoint(local_dir_path, &payload).map_err(CloudError::from)
        };

        events::emit_started_at(files.len() as i32, start_idx as u32);
        for i in start_idx..files.len() {
            control.wait_if_paused().await?;
            let roff = if i == start_idx {
                resume_server_offset.take()
            } else {
                None
            };
            match self
                .upload_file_via_session(
                    &session_id,
                    &files[i],
                    total_size,
                    uploaded_bytes,
                    i,
                    roff,
                    control,
                    |server_off| {
                        let _ = save_ck(json!({
                            "completed_bytes": files.iter().take(i).map(|f| f.size).sum::<u64>(),
                            "next_file_index": i,
                            "custom_active": {
                                "file_name": files[i].name,
                                "server_offset": server_off,
                            },
                        }));
                    },
                )
                .await
            {
                Ok(()) => {
                    uploaded_bytes += files[i].size;
                    let _ = save_ck(json!({
                        "completed_bytes": uploaded_bytes,
                        "next_file_index": i + 1,
                        "custom_active": Value::Null,
                    }));
                }
                Err(e) if e.is_cancelled() => return Err(e),
                Err(e) => {
                    logging::log_error(&format!("Upload fehlgeschlagen: {e}"));
                    events::emit_status(format!("Fehler: {e}"));
                    return Ok(false);
                }
            }
        }

        logging::log_info("Alle Dateien hochgeladen, finalisiere Session...");
        control.wait_if_paused().await?;
        let _ = save_ck(json!({
            "completed_bytes": total_size,
            "next_file_index": files.len(),
            "custom_active": Value::Null,
            "phase": "finalizing",
        }));
        events::emit_status("Finalisiere Upload...");
        match self.finalize_or_poll(&session_id, control).await {
            Ok(_) => {
                clear_checkpoint(local_dir_path);
                events::emit_status("Upload abgeschlossen.");
                Ok(true)
            }
            Err(e) if e.is_cancelled() => Err(e),
            Err(e) => {
                logging::log_error(&format!("Upload fehlgeschlagen: {e}"));
                events::emit_status(format!("Fehler: {e}"));
                Ok(false)
            }
        }
    }

    async fn initialize_direct_session(
        &self,
        files: &[ProxiedFile],
        folder_name: &str,
        kunde: &Kunde,
        control: &UploadControl,
    ) -> Result<Value, CloudError> {
        let file_rows: Vec<(String, u64, String)> = files
            .iter()
            .map(|f| (f.name.clone(), f.size, f.mime.clone()))
            .collect();
        let payload = direct_init_payload(&file_rows, folder_name, kunde);
        logging::log_info(&format!("direct-init metadata: {}", payload["metadata"]));
        let response = self
            .post_json_upload(
                "/direct-init",
                &payload,
                Duration::from_secs(60),
                "direct-init",
                &[],
                control,
            )
            .await?;
        response
            .json::<Value>()
            .await
            .map_err(|e| CloudError::Http(e.to_string()))
    }

    async fn upload_file_via_session(
        &self,
        session_id: &str,
        file: &ProxiedFile,
        total_job_size: u64,
        base_uploaded: u64,
        file_index: usize,
        resume_server_offset: Option<u64>,
        control: &UploadControl,
        mut on_chunk: impl FnMut(u64),
    ) -> Result<(), CloudError> {
        const WORKER: usize = 0;
        let file_size = file.size;
        let emit = |sent: u64| {
            let pct = percent(sent, file_size);
            let combined = base_uploaded + sent;
            events::emit_progress_file(pct, sent, file_size.max(1));
            events::upload_slots_worker_progress(WORKER, sent, file_size.max(1));
            events::emit_progress_total(
                percent(combined, total_job_size),
                combined,
                total_job_size,
            );
        };

        logging::log_info(&format!(
            "[session={session_id:?} file={:?}] Session-Upload: {} bytes{}",
            file.name,
            file_size,
            resume_server_offset
                .map(|o| format!(", resume_off={o}"))
                .unwrap_or_default()
        ));
        events::emit_status(format!("Lade hoch: {}", file.name));
        events::upload_slots_worker_start(WORKER, file_index, &file.name, file_size);
        emit(0);
        events::emit_progress_file(0, 0, file_size.max(1));

        control.wait_if_paused().await?;
        let mut fh = tokio::fs::File::open(&file.local_path).await?;
        let mut off;
        if let Some(resume) = resume_server_offset.filter(|o| *o > 0) {
            if resume > file_size {
                return Err(CloudError::Message(format!(
                    "{}: Checkpoint-Offset {resume} > Dateigröße {file_size}",
                    file.name
                )));
            }
            fh.seek(std::io::SeekFrom::Start(resume)).await?;
            off = resume;
            logging::log_info(&format!(
                "[session={session_id:?} file={:?}] Setze Upload bei Byte {off} fort.",
                file.name
            ));
        } else {
            control.wait_if_paused().await?;
            // Server requires first chunk == CHUNK_BYTES when file_size > CHUNK_BYTES.
            // tokio::fs::File::read may return a short read; fill the buffer fully.
            let first_len = (CHUNK_BYTES as u64).min(file_size) as usize;
            let first = read_exact_chunk(&mut fh, first_len, &file.name, "start").await?;
            let n = first.len();
            let extra = vec![("expected_size", file_size.to_string())];
            let response = self
                .post_session_multipart(
                    "/session/start",
                    session_id,
                    &file.name,
                    &extra,
                    first,
                    control,
                )
                .await?;
            let payload = response.json::<Value>().await.ok();
            off = parse_next_offset(payload.as_ref(), n as u64);
            emit(off);
            on_chunk(off);
        }

        let mut buf = vec![0u8; CHUNK_BYTES];
        while file_size.saturating_sub(off) > CHUNK_BYTES as u64 {
            control.wait_if_paused().await?;
            let n = read_exact_into(&mut fh, &mut buf, &file.name, "append").await?;
            if off + n as u64 >= file_size {
                return Err(CloudError::Message(format!(
                    "{}: append wuerde letzten Block senden — stattdessen finish",
                    file.name
                )));
            }
            let extra = vec![("offset", off.to_string())];
            let response = self
                .post_session_multipart(
                    "/session/append",
                    session_id,
                    &file.name,
                    &extra,
                    buf[..n].to_vec(),
                    control,
                )
                .await?;
            let payload = response.json::<Value>().await.ok();
            off = parse_next_offset(payload.as_ref(), off + n as u64);
            emit(off);
            on_chunk(off);
        }

        control.wait_if_paused().await?;
        let mut last = Vec::new();
        fh.read_to_end(&mut last).await?;
        if off + last.len() as u64 != file_size {
            return Err(CloudError::Message(format!(
                "{}: finish-Invariante verletzt off={off} len(last)={} expected_size={file_size}",
                file.name,
                last.len()
            )));
        }
        let extra = vec![
            ("offset", off.to_string()),
            ("mime_type", file.mime.clone()),
        ];
        self.post_session_multipart(
            "/session/finish",
            session_id,
            &file.name,
            &extra,
            last,
            control,
        )
        .await?;

        emit(file_size);
        events::emit_progress_file(100, file_size, file_size.max(1));
        events::upload_slots_worker_finish(WORKER);
        let current = base_uploaded + file_size;
        events::emit_progress_total(percent(current, total_job_size), current, total_job_size);
        logging::log_info(&format!(
            "[session={session_id:?} file={:?}] Fertig",
            file.name
        ));
        Ok(())
    }

    async fn finalize_or_poll(
        &self,
        session_id: &str,
        control: &UploadControl,
    ) -> Result<(), CloudError> {
        let customer_url = match self.finalize_session(session_id, control).await? {
            Some(url) => Some(url),
            None => self.wait_for_completion_legacy(session_id, control).await?,
        };
        self.set_last_customer_url(customer_url.clone());
        if let Some(url) = customer_url {
            logging::log_info(&format!("Upload erfolgreich: {url}"));
        } else {
            logging::log_warn(
                "Upload der Dateien erfolgreich, aber customer_url noch nicht verfuegbar.",
            );
        }
        Ok(())
    }

    async fn finalize_session(
        &self,
        session_id: &str,
        control: &UploadControl,
    ) -> Result<Option<String>, CloudError> {
        let response = self
            .post_json_upload(
                "/finalize",
                &json!({ "session_id": session_id }),
                Duration::from_secs(120),
                "finalize",
                &[404, 405, 501],
                control,
            )
            .await?;
        let status = response.status().as_u16();
        if matches!(status, 404 | 405 | 501) {
            logging::log_warn(&format!(
                "finalize nicht unterstuetzt (HTTP {status}), nutze Status-Poll-Fallback [session={session_id:?}]"
            ));
            return Ok(None);
        }
        let data = match response.json::<Value>().await {
            Ok(v) => v,
            Err(_) => {
                logging::log_warn(&format!(
                    "finalize: keine JSON-Antwort [session={session_id:?}]"
                ));
                return Ok(None);
            }
        };
        if let Some(oid) = data.get("order_id").and_then(value_as_string) {
            self.set_last_order_id(Some(oid));
        }
        Ok(extract_customer_url(&data))
    }

    async fn wait_for_completion_legacy(
        &self,
        session_id: &str,
        control: &UploadControl,
    ) -> Result<Option<String>, CloudError> {
        let max_wait = Duration::from_secs(if cfg!(test) { 0 } else { 30 });
        let poll = Duration::from_secs(if cfg!(test) { 0 } else { 2 });
        let started = std::time::Instant::now();
        let urls = [
            format!("{}/status/{session_id}", self.upload_root()?),
            format!("{}/upload/status/{session_id}", self.origin()?),
        ];
        logging::log_info("Warte auf Server-Finalisierung (Status-Poll)...");
        while started.elapsed() < max_wait || max_wait.is_zero() {
            for status_url in &urls {
                match self
                    .get_json(status_url, Duration::from_secs(15), control)
                    .await
                {
                    Ok(response) if response.status().is_success() => {
                        if let Ok(result) = response.json::<Value>().await {
                            if let Some(url) = extract_customer_url(&result) {
                                return Ok(Some(url));
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) if e.is_cancelled() => return Err(e),
                    Err(e) => logging::log_debug(&format!("Status-Poll {status_url}: {e}")),
                }
            }
            if max_wait.is_zero() {
                break;
            }
            tokio::time::sleep(poll).await;
            control.wait_if_paused().await?;
        }
        Ok(None)
    }

    async fn upload_direct_dropbox(
        &self,
        local_dir_path: &Path,
        remote_base_path: &str,
        control: &UploadControl,
        kunde: &Kunde,
    ) -> Result<bool, CloudError> {
        let files = dropbox::collect_upload_files(local_dir_path, remote_base_path);
        if files.is_empty() {
            logging::log_error("Keine Dateien zum Hochladen gefunden.");
            return Ok(false);
        }
        let folder_name = dropbox::remote_dir_name(remote_base_path);
        let folder_name = if folder_name.is_empty() {
            local_dir_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string()
        } else {
            folder_name
        };
        let total_size: u64 = files.iter().map(|f| f.size).sum();
        let manifest_items: Vec<Value> = files
            .iter()
            .map(|f| {
                json!({
                    "name": f.rel_norm,
                    "size": f.size,
                    "type": guess_mime(&f.local_path),
                })
            })
            .collect();
        let manifest_fp = manifest_fingerprint(&manifest_items);

        let raw_ck = load_checkpoint(local_dir_path);
        let mut resume_ck = None;
        if let Some(raw) = raw_ck {
            if raw.get("kind").and_then(Value::as_str) == Some("custom_api_direct_dropbox")
                && raw.get("manifest_fp").and_then(Value::as_str) == Some(manifest_fp.as_str())
            {
                let db = self.dropbox_client();
                if let Err(msg) = assert_checkpoint_binding_matches(
                    &raw,
                    db.profile_ams_id().as_deref(),
                    db.profile_pool(),
                ) {
                    logging::log_warn(&msg);
                    return Err(CloudError::Message(msg));
                }
                resume_ck = Some(raw);
            } else {
                logging::log_warn("Direct-Dropbox-Checkpoint verworfen.");
                clear_checkpoint(local_dir_path);
            }
        }

        let uploaded_files: Vec<Value> = resume_ck
            .as_ref()
            .and_then(|ck| ck.get("uploaded_files").and_then(Value::as_array).cloned())
            .unwrap_or_default();

        if resume_ck
            .as_ref()
            .and_then(|ck| ck.get("phase").and_then(Value::as_str))
            == Some("manifest_pending")
        {
            let root_share_link = resume_ck
                .as_ref()
                .and_then(|ck| ck.get("root_share_link").and_then(Value::as_str))
                .map(str::to_string);
            let resume_order_id = resume_ck
                .as_ref()
                .and_then(|ck| ck.get("order_id").and_then(value_as_string));
            let resume_final_url = resume_ck
                .as_ref()
                .and_then(|ck| ck.get("final_url").and_then(value_as_string));
            logging::log_info(&format!(
                "Checkpoint: Manifest-POST wird fortgesetzt ({} Dateien).",
                uploaded_files.len()
            ));
            return self
                .finish_direct_manifest(
                    local_dir_path,
                    &folder_name,
                    kunde,
                    &uploaded_files,
                    root_share_link.as_deref(),
                    resume_order_id.as_deref(),
                    resume_final_url.as_deref(),
                    &manifest_fp,
                    control,
                )
                .await;
        }

        if resume_ck.is_none() {
            let db = self.dropbox_client();
            let mut payload = json!({
                "kind": "custom_api_direct_dropbox",
                "manifest_fp": manifest_fp,
                "uploaded_files": [],
                "next_file_index": 0,
                "dd_active": Value::Null,
                "phase": "uploading",
            });
            if let Some(obj) = payload.as_object_mut() {
                merge_checkpoint_binding(
                    obj,
                    db.profile_ams_id().as_deref(),
                    Some(db.profile_pool()),
                );
            }
            let _ = save_checkpoint(local_dir_path, &payload);
        }

        // Use connect_session(false): CloudClient::connect emits global status and would
        // overwrite Custom-API "Verbunden" when Dropbox credentials are checked mid-upload.
        if let Err(e) = self.dropbox_client().connect_session(false).await {
            logging::log_error(&format!(
                "Direct-Dropbox: Verbindung fehlgeschlagen ({e}). Bitte Skydive-Media-Dropbox-Konto (App Key/Secret + OAuth) in den Einstellungen verbinden."
            ));
            events::emit_status(format!(
                "Fehler: Dropbox-Upload-Konto nicht verbunden ({e})"
            ));
            return Ok(false);
        }

        self.dropbox_client().reset_write_limiter();

        let ck_next_idx = resume_ck
            .as_ref()
            .and_then(|ck| ck.get("next_file_index").and_then(Value::as_u64))
            .unwrap_or(0) as usize;
        let done: std::collections::HashSet<String> = uploaded_files
            .iter()
            .filter_map(|row| {
                row.get("rel_path")
                    .and_then(Value::as_str)
                    .or_else(|| row.get("file_name").and_then(Value::as_str))
                    .map(str::to_string)
            })
            .collect();
        let start_idx =
            dropbox::effective_resume_index(&files, ck_next_idx, &done);
        let bytes_uploaded = dropbox::bytes_for_done_files(&files, &done);
        let files_done = dropbox::files_done_count(&files, &done);

        let mut resume_dd = None;
        if let Some(ck) = resume_ck.as_ref() {
            if ck.get("phase").and_then(Value::as_str) == Some("uploading") {
                let dd = ck.get("dd_active").cloned().unwrap_or(Value::Null);
                if start_idx < files.len()
                    && dd.get("file_name").and_then(Value::as_str)
                        == Some(files[start_idx].rel_norm.as_str())
                {
                    let offset = dd.get("offset").and_then(Value::as_u64).unwrap_or(0);
                    if offset > 0 {
                        if let Some(sid) = dd.get("session_id").and_then(Value::as_str) {
                            resume_dd = Some(DropboxSessionResume {
                                session_id: sid.to_string(),
                                offset,
                                rel_path: files[start_idx].rel_norm.clone(),
                            });
                        }
                    }
                }
            }
        }

        if start_idx > 0 || !done.is_empty() {
            logging::log_info(&format!(
                "Direct-Dropbox-Upload fortsetzen (Index {start_idx}, {} Dateien bereits checkpointiert).",
                done.len()
            ));
        }

        events::emit_started_at(files.len() as i32, files_done);

        let dir = local_dir_path.to_path_buf();
        let fp = manifest_fp.clone();
        let db = self.dropbox_client();
        let ams = db.profile_ams_id();
        let pool = db.profile_pool();
        let files_for_ck = files.clone();
        let uploaded_files_cell = std::sync::Mutex::new(uploaded_files);
        let ck_saver = std::sync::Mutex::new(
            crate::upload::checkpoint::ThrottledCheckpointSaver::new(
                dropbox::CK_MIN_INTERVAL_SECS,
                dropbox::CHUNK_SIZE as u64,
            ),
        );

        let result = crate::cloud::dropbox_batch::upload_files_hybrid(
            self.dropbox_client().as_ref(),
            &files,
            start_idx,
            total_size,
            bytes_uploaded,
            control,
            &done,
            resume_dd,
            |file_idx, cursor, force| {
                let i = file_idx.min(files_for_ck.len().saturating_sub(1));
                let file = &files_for_ck[i];
                let snapshot = uploaded_files_cell
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let active = cursor.as_ref().map(|c| {
                    json!({
                        "file_name": file.rel_norm,
                        "session_id": c.session_id,
                        "offset": c.offset,
                        "dropbox_path": file.dropbox_path,
                    })
                });
                let mut payload = json!({
                    "kind": "custom_api_direct_dropbox",
                    "manifest_fp": fp,
                    "uploaded_files": snapshot,
                    "next_file_index": i,
                    "dd_active": active,
                    "phase": "uploading",
                });
                if let Some(obj) = payload.as_object_mut() {
                    merge_checkpoint_binding(obj, ams.as_deref(), Some(pool));
                }
                let offset = cursor.as_ref().map(|c| c.offset).unwrap_or(0);
                if let Ok(mut saver) = ck_saver.lock() {
                    let _ = saver.update(&dir, &payload, offset, force);
                    if cursor.is_none() {
                        let _ = saver.flush();
                    }
                }
            },
            |next_idx, _bytes, u| {
                if let Ok(mut list) = uploaded_files_cell.lock() {
                    let file = &files_for_ck[u.file_index];
                    let already = list.iter().any(|row| {
                        row.get("rel_path").and_then(Value::as_str) == Some(file.rel_norm.as_str())
                            || row.get("file_name").and_then(Value::as_str)
                                == file
                                    .local_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                    });
                    if !already {
                        let mut row = json!({
                            "name": file.local_path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                            "rel_path": file.rel_norm,
                            "size": file.size,
                            "mime": guess_mime(&file.local_path),
                        });
                        if let Some(id) = &u.dropbox_id {
                            row["dropbox_id"] = json!(id);
                        }
                        list.push(row);
                    }
                }
                let snapshot = uploaded_files_cell
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let mut payload = json!({
                    "kind": "custom_api_direct_dropbox",
                    "manifest_fp": fp,
                    "uploaded_files": snapshot,
                    "next_file_index": next_idx,
                    "dd_active": Value::Null,
                    "phase": "uploading",
                });
                if let Some(obj) = payload.as_object_mut() {
                    merge_checkpoint_binding(obj, ams.as_deref(), Some(pool));
                }
                if let Ok(mut saver) = ck_saver.lock() {
                    let _ = saver.update(&dir, &payload, next_idx as u64, false);
                    if next_idx % 25 == 0 {
                        let _ = saver.flush();
                    }
                }
                Ok(())
            },
            |next_idx, _bytes, uploaded| {
                if let Ok(mut saver) = ck_saver.lock() {
                    let _ = saver.flush();
                }
                if let Ok(mut list) = uploaded_files_cell.lock() {
                    for u in uploaded {
                        let file = &files_for_ck[u.file_index];
                        let already = list.iter().any(|row| {
                            row.get("rel_path").and_then(Value::as_str)
                                == Some(file.rel_norm.as_str())
                        });
                        if already {
                            continue;
                        }
                        let mut row = json!({
                            "name": file.local_path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                            "rel_path": file.rel_norm,
                            "size": file.size,
                            "mime": guess_mime(&file.local_path),
                        });
                        if let Some(id) = &u.dropbox_id {
                            row["dropbox_id"] = json!(id);
                        }
                        list.push(row);
                    }
                }
                let snapshot = uploaded_files_cell
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let mut payload = json!({
                    "kind": "custom_api_direct_dropbox",
                    "manifest_fp": fp,
                    "uploaded_files": snapshot,
                    "next_file_index": next_idx,
                    "dd_active": Value::Null,
                    "phase": "uploading",
                });
                if let Some(obj) = payload.as_object_mut() {
                    merge_checkpoint_binding(obj, ams.as_deref(), Some(pool));
                }
                let _ = save_checkpoint(&dir, &payload);
                Ok(())
            },
        )
        .await;

        let uploaded_files = match result {
            Ok(()) => uploaded_files_cell.into_inner().unwrap_or_default(),
            Err(e) if e.is_cancelled() => return Err(e),
            Err(e) => {
                if let Ok(mut saver) = ck_saver.lock() {
                    let _ = saver.flush();
                }
                let snapshot = uploaded_files_cell
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                if !snapshot.is_empty() {
                    let done_now: std::collections::HashSet<String> = snapshot
                        .iter()
                        .filter_map(|row| {
                            row.get("rel_path")
                                .and_then(Value::as_str)
                                .or_else(|| row.get("file_name").and_then(Value::as_str))
                                .map(str::to_string)
                        })
                        .collect();
                    let next_idx =
                        dropbox::effective_resume_index(&files, start_idx, &done_now);
                    let mut payload = json!({
                        "kind": "custom_api_direct_dropbox",
                        "manifest_fp": fp,
                        "uploaded_files": snapshot,
                        "next_file_index": next_idx,
                        "dd_active": Value::Null,
                        "phase": "uploading",
                    });
                    if let Some(obj) = payload.as_object_mut() {
                        merge_checkpoint_binding(obj, ams.as_deref(), Some(pool));
                    }
                    let _ = save_checkpoint(&dir, &payload);
                }
                logging::log_error(&format!("Fehler beim Direct-Dropbox-Upload: {e}"));
                events::emit_status(format!("Fehler: {e}"));
                return Ok(false);
            }
        };

        let root_share = self
            .dropbox_client()
            .get_shareable_link_raw(remote_base_path)
            .await
            .ok()
            .flatten();
        self.finish_direct_manifest(
            local_dir_path,
            &folder_name,
            kunde,
            &uploaded_files,
            root_share.as_deref(),
            None,
            None,
            &manifest_fp,
            control,
        )
        .await
    }

    async fn finish_direct_manifest(
        &self,
        local_dir_path: &Path,
        folder_name: &str,
        kunde: &Kunde,
        uploaded_files: &[Value],
        root_share_link: Option<&str>,
        known_order_id: Option<&str>,
        known_final_url: Option<&str>,
        manifest_fp: &str,
        control: &UploadControl,
    ) -> Result<bool, CloudError> {
        let version = env!("CARGO_PKG_VERSION");
        let manifest = build_manifest_v11(
            folder_name,
            Some(kunde),
            uploaded_files,
            root_share_link,
            version,
            self.append_order_id().as_deref(),
        );
        let uploaded_files = uploaded_files.to_vec();
        let root = root_share_link.map(str::to_string);
        let fp = manifest_fp.to_string();
        let dir = local_dir_path.to_path_buf();
        let db = self.dropbox_client();
        let ck_ams = db.profile_ams_id();
        let ck_pool = db.profile_pool();
        match self
            .submit_manifest_v11(
                &manifest,
                known_order_id,
                known_final_url,
                control,
                |extra| {
                    let mut payload = json!({
                        "kind": "custom_api_direct_dropbox",
                        "manifest_fp": fp,
                        "uploaded_files": uploaded_files,
                        "root_share_link": root,
                        "phase": "manifest_pending",
                    });
                    if let Some(obj) = payload.as_object_mut() {
                        merge_checkpoint_binding(obj, ck_ams.as_deref(), Some(ck_pool));
                    }
                    if let (Some(obj), Some(extra_obj)) =
                        (payload.as_object_mut(), extra.as_object())
                    {
                        for (k, v) in extra_obj {
                            obj.insert(k.clone(), v.clone());
                        }
                    }
                    save_checkpoint(&dir, &payload).map_err(CloudError::from)
                },
            )
            .await
        {
            Ok(()) => {
                clear_checkpoint(local_dir_path);
                events::emit_status("Upload abgeschlossen.");
                Ok(true)
            }
            Err(e) if e.is_cancelled() => Err(e),
            Err(e) => {
                logging::log_error(&format!("Manifest-Submit fehlgeschlagen: {e}"));
                events::emit_status(format!("Fehler: {e}"));
                Ok(false)
            }
        }
    }

    pub(super) async fn shareable_link(&self) -> Result<Option<String>, CloudError> {
        if self.last_customer_url().is_none() {
            if let (Some(session_id), Ok(upload_root), Ok(origin)) =
                (self.last_session_id(), self.upload_root(), self.origin())
            {
                let urls = [
                    format!("{upload_root}/status/{session_id}"),
                    format!("{origin}/upload/status/{session_id}"),
                ];
                for status_url in urls {
                    if let Ok(response) = self
                        .http
                        .get(&status_url)
                        .header(
                            reqwest::header::AUTHORIZATION,
                            format!("Bearer {}", self.api_key().unwrap_or_default()),
                        )
                        .timeout(Duration::from_secs(15))
                        .send()
                        .await
                    {
                        if response.status().is_success() {
                            if let Ok(payload) = response.json::<Value>().await {
                                if let Some(url) = extract_customer_url(&payload) {
                                    self.set_last_customer_url(Some(url.clone()));
                                    logging::log_info(&format!(
                                        "customer_url per Fallback-Check erhalten: {url}"
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        let Some(url) = self.last_customer_url() else {
            logging::log_warn("Keine customer_url verfügbar. Upload noch nicht abgeschlossen?");
            return Ok(None);
        };
        logging::log_info(&format!("Gebe customer_url zurück: {url}"));
        Ok(Some(link_shortener::shorten(&url).await))
    }
}

fn collect_proxied_files(local_dir_path: &Path) -> Vec<ProxiedFile> {
    let mut files = Vec::new();
    walk_proxied(local_dir_path, local_dir_path, &mut files);
    files
}

fn walk_proxied(root: &Path, current: &Path, out: &mut Vec<ProxiedFile>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_proxied(root, &path, out);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if should_skip_upload_file(name) {
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let rel_norm = relative.to_string_lossy().replace('\\', "/");
        match fs::metadata(&path) {
            Ok(meta) => out.push(ProxiedFile {
                name: rel_norm,
                size: meta.len(),
                mime: guess_mime(&path),
                local_path: path,
            }),
            Err(_) => logging::log_warn(&format!("Datei nicht gefunden: {}", path.display())),
        }
    }
}

fn value_as_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|n| n.to_string()))
        .or_else(|| value.as_u64().map(|n| n.to_string()))
        .filter(|s| !s.is_empty())
}

/// Fill `buf` completely (loop past short `read`s). Errors if EOF comes early.
async fn read_exact_into(
    file: &mut tokio::fs::File,
    buf: &mut [u8],
    file_name: &str,
    phase: &str,
) -> Result<usize, CloudError> {
    let need = buf.len();
    if need == 0 {
        return Ok(0);
    }
    let mut filled = 0usize;
    while filled < need {
        let n = file.read(&mut buf[filled..]).await?;
        if n == 0 {
            return Err(CloudError::Message(format!(
                "{file_name}: {phase} erwartete {need} Bytes, erhalten {filled} (EOF)"
            )));
        }
        filled += n;
    }
    Ok(filled)
}

async fn read_exact_chunk(
    file: &mut tokio::fs::File,
    len: usize,
    file_name: &str,
    phase: &str,
) -> Result<Vec<u8>, CloudError> {
    let mut buf = vec![0u8; len];
    read_exact_into(file, &mut buf, file_name, phase).await?;
    Ok(buf)
}

fn percent(current: u64, total: u64) -> i32 {
    if total == 0 {
        0
    } else {
        ((current as f64 / total as f64) * 100.0) as i32
    }
}

pub fn direct_init_payload(
    files: &[(String, u64, String)],
    folder_name: &str,
    kunde: &Kunde,
) -> Value {
    let mut metadata = serde_json::to_value(kunde).unwrap_or(json!({}));
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert("base_folder_name".into(), json!(folder_name));
    }
    json!({
        "files": files.iter().map(|(name, size, mime)| json!({
            "name": name,
            "size": size,
            "type": mime,
        })).collect::<Vec<_>>(),
        "metadata": metadata,
        "base_folder_name": folder_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[test]
    fn direct_init_payload_includes_kunde_type_and_files() {
        let kunde = Kunde {
            first_name: Some("Anna".into()),
            customer_type: Some("Outside".into()),
            outside_foto: true,
            ..Kunde::default()
        };
        let payload = direct_init_payload(
            &[("Outside_Foto/a.jpg".into(), 10, "image/jpeg".into())],
            "Job-1",
            &kunde,
        );
        assert_eq!(payload["base_folder_name"], "Job-1");
        assert_eq!(payload["files"][0]["name"], "Outside_Foto/a.jpg");
        assert_eq!(payload["files"][0]["size"], 10);
        assert_eq!(payload["metadata"]["type"], "Outside");
        assert_eq!(payload["metadata"]["first_name"], "Anna");
        assert_eq!(payload["metadata"]["base_folder_name"], "Job-1");
    }

    #[tokio::test]
    async fn read_exact_chunk_fills_full_buffer_despite_partial_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chunk.bin");
        let payload = vec![0xABu8; CHUNK_BYTES];
        {
            let mut f = tokio::fs::File::create(&path).await.unwrap();
            f.write_all(&payload).await.unwrap();
            f.flush().await.unwrap();
        }
        let mut f = tokio::fs::File::open(&path).await.unwrap();
        let got = read_exact_chunk(&mut f, CHUNK_BYTES, "chunk.bin", "start")
            .await
            .unwrap();
        assert_eq!(got.len(), CHUNK_BYTES);
        assert_eq!(got, payload);
    }

    #[tokio::test]
    async fn read_exact_into_errors_on_short_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("short.bin");
        {
            let mut f = tokio::fs::File::create(&path).await.unwrap();
            f.write_all(&[1, 2, 3]).await.unwrap();
        }
        let mut f = tokio::fs::File::open(&path).await.unwrap();
        let mut buf = [0u8; 8];
        let err = read_exact_into(&mut f, &mut buf, "short.bin", "start")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("erwartete 8 Bytes"));
    }
}
