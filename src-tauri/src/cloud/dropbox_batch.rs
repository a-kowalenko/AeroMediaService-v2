//! Parallel / hybrid Dropbox multi-file upload (legacy `custom_api_client` hybrid).
//!
//! - Small files (≤ [`SMALL_FILE_BYTES`]): parallel `/files/upload`
//! - Large files (n>1): `start_batch` → parallel `append_v2` → `finish_batch_v2`
//! - Large n==1: serial [`DropboxClient::upload_large_file`]
//! - Groups of at most [`BATCH_MAX_FILES`]

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::task::JoinSet;

use super::dropbox::{
    percent, read_chunk, session_append_arg, DropboxClient, DropboxCursor, DropboxSessionResume,
    UploadFile, BATCH_MAX_FILES, BATCH_PARALLEL_WORKERS, CHUNK_SIZE, SMALL_FILE_BYTES,
};
use crate::cloud::traits::CloudError;
use crate::events;
use crate::upload::progress::BatchProgress;
use crate::storage::logging;
use crate::upload::control::UploadControl;

/// One completed upload within a hybrid group (stable file index into the full job list).
#[derive(Debug, Clone)]
pub struct HybridUploaded {
    pub file_index: usize,
    pub dropbox_id: Option<String>,
}

/// Partition indices into small (≤ 4 MiB) vs large.
pub(crate) fn partition_small_large(files: &[UploadFile]) -> (Vec<usize>, Vec<usize>) {
    let mut small = Vec::new();
    let mut large = Vec::new();
    for (i, f) in files.iter().enumerate() {
        if f.size <= SMALL_FILE_BYTES as u64 {
            small.push(i);
        } else {
            large.push(i);
        }
    }
    (small, large)
}

/// Inclusive-exclusive slices of length ≤ `max` covering `0..len`.
pub(crate) fn batch_slices(len: usize, max: usize) -> Vec<(usize, usize)> {
    if len == 0 || max == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < len {
        let end = (start + max).min(len);
        out.push((start, end));
        start = end;
    }
    out
}

/// Drive hybrid upload from `start_idx` through the end of `files`.
pub async fn upload_files_hybrid<F, G, H>(
    client: &DropboxClient,
    files: &[UploadFile],
    start_idx: usize,
    total_job_size: u64,
    mut bytes_uploaded: u64,
    control: &UploadControl,
    skip_paths: &HashSet<String>,
    mut resume: Option<DropboxSessionResume>,
    mut on_serial_progress: F,
    mut on_file_done: H,
    mut on_group_done: G,
) -> Result<(), CloudError>
where
    F: FnMut(usize, Option<DropboxCursor>, bool),
    G: FnMut(usize, u64, &[HybridUploaded]) -> Result<(), CloudError>,
    H: FnMut(usize, u64, &HybridUploaded) -> Result<(), CloudError>,
{
    let mut idx = start_idx.min(files.len());
    while idx < files.len() && skip_paths.contains(&files[idx].rel_norm) {
        idx += 1;
    }

    if let Some(r) = resume.take() {
        if idx < files.len() {
            let file = &files[idx];
            if r.rel_path == file.rel_norm && !skip_paths.contains(&file.rel_norm) {
                control.wait_if_paused().await?;
                emit_file_status(file, idx, files.len(), 0);
                let id = upload_one_serial(
                    client,
                    file,
                    idx,
                    bytes_uploaded,
                    total_job_size,
                    control,
                    Some(r),
                    &mut on_serial_progress,
                )
                .await?;
                bytes_uploaded += file.size;
                emit_total(bytes_uploaded, total_job_size);
                let uploaded = HybridUploaded {
                    file_index: idx,
                    dropbox_id: id,
                };
                client.write_limiter().on_success();
                on_file_done(idx + 1, bytes_uploaded, &uploaded)?;
                on_group_done(idx + 1, bytes_uploaded, std::slice::from_ref(&uploaded))?;
                idx += 1;
            }
        }
    }

    for (batch_start, batch_end) in batch_slices(files.len().saturating_sub(idx), BATCH_MAX_FILES)
        .into_iter()
        .map(|(s, e)| (idx + s, idx + e))
    {
        control.wait_if_paused().await?;
        let batch = &files[batch_start..batch_end];
        if batch.is_empty() {
            continue;
        }

        let pending: Vec<usize> = (0..batch.len())
            .filter(|&local_i| !skip_paths.contains(&batch[local_i].rel_norm))
            .collect();
        if pending.is_empty() {
            continue;
        }

        if pending.len() == 1 {
            let local_i = pending[0];
            let file = &batch[local_i];
            let global_idx = batch_start + local_i;
            emit_file_status(file, global_idx, files.len(), 0);
            let id = upload_one_serial(
                client,
                file,
                global_idx,
                bytes_uploaded,
                total_job_size,
                control,
                None,
                &mut on_serial_progress,
            )
            .await?;
            bytes_uploaded += file.size;
            emit_total(bytes_uploaded, total_job_size);
            let uploaded = HybridUploaded {
                file_index: global_idx,
                dropbox_id: id,
            };
            client.write_limiter().on_success();
            on_file_done(global_idx + 1, bytes_uploaded, &uploaded)?;
            on_group_done(global_idx + 1, bytes_uploaded, std::slice::from_ref(&uploaded))?;
            continue;
        }

        logging::log_info(&format!(
            "Dropbox Batch-Upload: {} Dateien (Index {}–{}).",
            pending.len(),
            batch_start,
            batch_end - 1
        ));
        let uploaded = upload_batch_group(
            client,
            batch,
            batch_start,
            files.len(),
            total_job_size,
            bytes_uploaded,
            control,
            skip_paths,
            &mut on_file_done,
        )
        .await?;
        for u in &uploaded {
            bytes_uploaded += files[u.file_index].size;
        }
        emit_total(bytes_uploaded, total_job_size);
        on_group_done(batch_end, bytes_uploaded, &uploaded)?;
    }

    Ok(())
}

async fn upload_one_serial<F>(
    client: &DropboxClient,
    file: &UploadFile,
    file_index: usize,
    bytes_uploaded: u64,
    total_job_size: u64,
    control: &UploadControl,
    resume: Option<DropboxSessionResume>,
    on_serial_progress: &mut F,
) -> Result<Option<String>, CloudError>
where
    F: FnMut(usize, Option<DropboxCursor>, bool),
{
    const WORKER: usize = 0;
    events::emit_progress_file(0, 0, file.size);
    if file.size <= SMALL_FILE_BYTES as u64 {
        let size = file.size;
        let on_send = std::sync::Arc::new(move |sent: u64| {
            let sent = sent.min(size);
            events::upload_slots_worker_progress(WORKER, sent, size);
            events::emit_progress_file(percent(sent, size), sent, size);
            emit_total(bytes_uploaded.saturating_add(sent), total_job_size);
        }) as std::sync::Arc<dyn Fn(u64) + Send + Sync>;
        let id = client
            .upload_small_file_with_progress(
                &file.local_path,
                &file.dropbox_path,
                file.size,
                control,
                Some(on_send),
            )
            .await?;
        on_serial_progress(file_index, None, true);
        events::upload_slots_worker_progress(WORKER, file.size, file.size);
        emit_file_done(WORKER);
        Ok(id)
    } else {
        let size = file.size;
        let on_bytes = std::sync::Arc::new(move |sent: u64| {
            events::upload_slots_worker_progress(WORKER, sent.min(size), size);
        }) as std::sync::Arc<dyn Fn(u64) + Send + Sync>;
        let result = client
            .upload_large_file(
                &file.local_path,
                &file.dropbox_path,
                file.size,
                bytes_uploaded,
                total_job_size,
                control,
                resume,
                Some(|cursor: Option<DropboxCursor>, force: bool| {
                    if let Some(ref c) = cursor {
                        events::upload_slots_worker_progress(WORKER, c.offset, file.size);
                    }
                    on_serial_progress(file_index, cursor, force);
                }),
                Some(on_bytes),
            )
            .await;
        if result.is_ok() {
            events::upload_slots_worker_progress(WORKER, file.size, file.size);
            emit_file_done(WORKER);
        }
        result
    }
}

async fn upload_batch_group<H>(
    client: &DropboxClient,
    batch: &[UploadFile],
    batch_start_index: usize,
    total_files_in_job: usize,
    total_job_size: u64,
    bytes_uploaded_base: u64,
    control: &UploadControl,
    skip_paths: &HashSet<String>,
    on_file_done: &mut H,
) -> Result<Vec<HybridUploaded>, CloudError>
where
    H: FnMut(usize, u64, &HybridUploaded) -> Result<(), CloudError>,
{
    let (small_local, large_local) = partition_small_large(batch);
    let small_local: Vec<usize> = small_local
        .into_iter()
        .filter(|&li| !skip_paths.contains(&batch[li].rel_norm))
        .collect();
    let large_local: Vec<usize> = large_local
        .into_iter()
        .filter(|&li| !skip_paths.contains(&batch[li].rel_norm))
        .collect();
    let mut rows_by_local: Vec<Option<HybridUploaded>> = vec![None; batch.len()];

    if !small_local.is_empty() {
        logging::log_info(&format!(
            "Dropbox: {} kleine Dateien parallel (files/upload).",
            small_local.len()
        ));
        let small_indexed: Vec<(usize, usize)> = small_local
            .iter()
            .map(|&li| (li, batch_start_index + li))
            .collect();
        let mut running_bytes = bytes_uploaded_base;
        let results = upload_small_parallel(
            client,
            batch,
            &small_indexed,
            total_files_in_job,
            total_job_size,
            bytes_uploaded_base,
            control,
            &mut running_bytes,
            on_file_done,
        )
        .await?;
        for (local_idx, id) in results {
            rows_by_local[local_idx] = Some(HybridUploaded {
                file_index: batch_start_index + local_idx,
                dropbox_id: id,
            });
        }
    }

    if !large_local.is_empty() {
        let large_files: Vec<&UploadFile> = large_local.iter().map(|&i| &batch[i]).collect();
        let large_global: Vec<usize> = large_local
            .iter()
            .map(|&i| batch_start_index + i)
            .collect();
        logging::log_info(&format!(
            "Dropbox Session-Batch: {} große Dateien.",
            large_files.len()
        ));
        let small_done: u64 = small_local.iter().map(|&i| batch[i].size).sum();
        let mut running_bytes = bytes_uploaded_base + small_done;
        let results = upload_large_batch(
            client,
            &large_files,
            &large_global,
            total_files_in_job,
            total_job_size,
            bytes_uploaded_base + small_done,
            control,
            &mut running_bytes,
            on_file_done,
        )
        .await?;
        for (pos, id) in results.into_iter().enumerate() {
            let local_idx = large_local[pos];
            rows_by_local[local_idx] = Some(HybridUploaded {
                file_index: batch_start_index + local_idx,
                dropbox_id: id,
            });
        }
    }

    let mut out = Vec::new();
    for (i, slot) in rows_by_local.into_iter().enumerate() {
        if skip_paths.contains(&batch[i].rel_norm) {
            continue;
        }
        out.push(slot.ok_or_else(|| {
            CloudError::Message(format!(
                "Batch-Upload unvollständig für {}",
                batch[i].rel_norm
            ))
        })?);
    }
    Ok(out)
}

/// Try to start queued work on idle worker lanes up to `cap` in-flight tasks.
fn try_spawn_worker_lanes<T, F, Fut>(
    set: &mut JoinSet<(usize, Result<T, CloudError>)>,
    lane_busy: &mut [bool; BATCH_PARALLEL_WORKERS],
    cap: usize,
    queue: &mut VecDeque<(usize, usize)>,
    mut build: F,
) where
    F: FnMut(usize, (usize, usize)) -> Fut,
    Fut: std::future::Future<Output = Result<T, CloudError>> + Send + 'static,
    T: Send + 'static,
{
    for worker_id in 0..BATCH_PARALLEL_WORKERS {
        if set.len() >= cap {
            break;
        }
        if lane_busy[worker_id] {
            continue;
        }
        let Some(item) = queue.pop_front() else {
            break;
        };
        lane_busy[worker_id] = true;
        let fut = build(worker_id, item);
        set.spawn(async move {
            let result = fut.await;
            (worker_id, result)
        });
    }
}

fn try_spawn_large_lanes<T, F, Fut>(
    set: &mut JoinSet<(usize, Result<T, CloudError>)>,
    lane_busy: &mut [bool; BATCH_PARALLEL_WORKERS],
    cap: usize,
    queue: &mut VecDeque<usize>,
    mut build: F,
) where
    F: FnMut(usize, usize) -> Fut,
    Fut: std::future::Future<Output = Result<T, CloudError>> + Send + 'static,
    T: Send + 'static,
{
    for worker_id in 0..BATCH_PARALLEL_WORKERS {
        if set.len() >= cap {
            break;
        }
        if lane_busy[worker_id] {
            continue;
        }
        let Some(item) = queue.pop_front() else {
            break;
        };
        lane_busy[worker_id] = true;
        let fut = build(worker_id, item);
        set.spawn(async move {
            let result = fut.await;
            (worker_id, result)
        });
    }
}

fn worker_capacity(in_flight: usize, pending: usize) -> usize {
    in_flight + pending
}

async fn upload_small_parallel<H>(
    client: &DropboxClient,
    batch: &[UploadFile],
    indexed: &[(usize, usize)],
    total_files_in_job: usize,
    total_job_size: u64,
    bytes_uploaded_base: u64,
    control: &UploadControl,
    running_bytes: &mut u64,
    on_file_done: &mut H,
) -> Result<Vec<(usize, Option<String>)>, CloudError>
where
    H: FnMut(usize, u64, &HybridUploaded) -> Result<(), CloudError>,
{
    if indexed.is_empty() {
        return Ok(Vec::new());
    }
    if indexed.len() == 1 {
        let (local_idx, global_idx) = indexed[0];
        const WORKER: usize = 0;
        let file = &batch[local_idx];
        control.wait_if_paused().await?;
        emit_file_status(file, global_idx, total_files_in_job, WORKER);
        let size = file.size;
        let on_send = std::sync::Arc::new(move |sent: u64| {
            let sent = sent.min(size);
            events::upload_slots_worker_progress(WORKER, sent, size);
            events::emit_progress_file(percent(sent, size), sent, size);
            emit_total(bytes_uploaded_base.saturating_add(sent), total_job_size);
        }) as std::sync::Arc<dyn Fn(u64) + Send + Sync>;
        let id = client
            .upload_small_file_with_progress(
                &file.local_path,
                &file.dropbox_path,
                file.size,
                control,
                Some(on_send),
            )
            .await?;
        events::upload_slots_worker_progress(WORKER, file.size, file.size);
        emit_file_done(WORKER);
        *running_bytes = running_bytes.saturating_add(file.size);
        emit_total(*running_bytes, total_job_size);
        let row = HybridUploaded {
            file_index: global_idx,
            dropbox_id: id.clone(),
        };
        client.write_limiter().on_success();
        on_file_done(global_idx + 1, *running_bytes, &row)?;
        return Ok(vec![(local_idx, id)]);
    }

    let limiter = client.write_limiter();
    let mut queue: VecDeque<(usize, usize)> = indexed.iter().copied().collect();
    let mut results = Vec::with_capacity(indexed.len());
    let batch_prog = BatchProgress::new(BATCH_PARALLEL_WORKERS, bytes_uploaded_base, total_job_size);
    let mut lane_busy = [false; BATCH_PARALLEL_WORKERS];
    let mut set: JoinSet<(usize, Result<(usize, usize, Option<String>), CloudError>)> =
        JoinSet::new();

    while !queue.is_empty() || !set.is_empty() {
        control.wait_if_paused().await?;
        let cap = limiter.current_workers(worker_capacity(set.len(), queue.len()));
        try_spawn_worker_lanes(&mut set, &mut lane_busy, cap, &mut queue, |worker_id, (local_idx, global_idx)| {
            let client = client.clone();
            let control = control.clone();
            let file = batch[local_idx].clone();
            let batch_prog = Arc::clone(&batch_prog);
            async move {
                control.wait_if_paused().await?;
                emit_file_status(&file, global_idx, total_files_in_job, worker_id);
                events::emit_progress_file(0, 0, file.size);
                let size = file.size;
                let bp = Arc::clone(&batch_prog);
                let on_send = std::sync::Arc::new(move |sent: u64| {
                    let sent = sent.min(size);
                    bp.report_inflight(worker_id, sent, false);
                    events::upload_slots_worker_progress(worker_id, sent, size);
                    events::emit_progress_file(percent(sent, size), sent, size);
                }) as std::sync::Arc<dyn Fn(u64) + Send + Sync>;
                let id = client
                    .upload_small_file_with_progress(
                        &file.local_path,
                        &file.dropbox_path,
                        file.size,
                        &control,
                        Some(on_send),
                    )
                    .await?;
                events::emit_progress_file(100, file.size, file.size);
                events::upload_slots_worker_progress(worker_id, file.size, file.size);
                batch_prog.complete_slot(worker_id, file.size);
                Ok((local_idx, global_idx, id))
            }
        });

        let Some(joined) = set.join_next().await else {
            continue;
        };
        match joined {
            Ok((worker_id, Ok((local_idx, global_idx, id)))) => {
                lane_busy[worker_id] = false;
                let file = &batch[local_idx];
                *running_bytes = running_bytes.saturating_add(file.size);
                let row = HybridUploaded {
                    file_index: global_idx,
                    dropbox_id: id.clone(),
                };
                limiter.on_success();
                on_file_done(global_idx + 1, *running_bytes, &row)?;
                results.push((local_idx, id));
                emit_file_done(worker_id);

                let cap = limiter.current_workers(worker_capacity(set.len(), queue.len()));
                try_spawn_worker_lanes(
                    &mut set,
                    &mut lane_busy,
                    cap,
                    &mut queue,
                    |worker_id, (local_idx, global_idx)| {
                        let client = client.clone();
                        let control = control.clone();
                        let file = batch[local_idx].clone();
                        let batch_prog = Arc::clone(&batch_prog);
                        async move {
                            control.wait_if_paused().await?;
                            emit_file_status(&file, global_idx, total_files_in_job, worker_id);
                            events::emit_progress_file(0, 0, file.size);
                            let size = file.size;
                            let bp = Arc::clone(&batch_prog);
                            let on_send = std::sync::Arc::new(move |sent: u64| {
                                let sent = sent.min(size);
                                bp.report_inflight(worker_id, sent, false);
                                events::upload_slots_worker_progress(worker_id, sent, size);
                                events::emit_progress_file(percent(sent, size), sent, size);
                            })
                                as std::sync::Arc<dyn Fn(u64) + Send + Sync>;
                            let id = client
                                .upload_small_file_with_progress(
                                    &file.local_path,
                                    &file.dropbox_path,
                                    file.size,
                                    &control,
                                    Some(on_send),
                                )
                                .await?;
                            events::emit_progress_file(100, file.size, file.size);
                            events::upload_slots_worker_progress(worker_id, file.size, file.size);
                            batch_prog.complete_slot(worker_id, file.size);
                            Ok((local_idx, global_idx, id))
                        }
                    },
                );
            }
            Ok((_, Err(e))) => {
                set.abort_all();
                return Err(e);
            }
            Err(e) => {
                set.abort_all();
                return Err(CloudError::Message(format!("Join-Fehler: {e}")));
            }
        }
    }

    emit_total(batch_prog.combined(), total_job_size);

    results.sort_by_key(|(i, _)| *i);
    Ok(results)
}

async fn upload_large_batch<H>(
    client: &DropboxClient,
    large_files: &[&UploadFile],
    global_indices: &[usize],
    total_files_in_job: usize,
    total_job_size: u64,
    bytes_uploaded_base: u64,
    control: &UploadControl,
    running_bytes: &mut u64,
    on_file_done: &mut H,
) -> Result<Vec<Option<String>>, CloudError>
where
    H: FnMut(usize, u64, &HybridUploaded) -> Result<(), CloudError>,
{
    let n = large_files.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    if n == 1 {
        const WORKER: usize = 0;
        let file = large_files[0];
        let global_idx = global_indices[0];
        let file_size = file.size;
        control.wait_if_paused().await?;
        emit_file_status(file, global_idx, total_files_in_job, WORKER);
        let on_bytes = std::sync::Arc::new(move |sent: u64| {
            events::upload_slots_worker_progress(WORKER, sent.min(file_size), file_size);
        }) as std::sync::Arc<dyn Fn(u64) + Send + Sync>;
        let id = client
            .upload_large_file(
                &file.local_path,
                &file.dropbox_path,
                file.size,
                bytes_uploaded_base,
                total_job_size,
                control,
                None,
                Some(|cursor: Option<DropboxCursor>, _force: bool| {
                    if let Some(c) = cursor {
                        events::upload_slots_worker_progress(WORKER, c.offset, file_size);
                    }
                }),
                Some(on_bytes),
            )
            .await?;
        events::upload_slots_worker_progress(WORKER, file.size, file.size);
        emit_file_done(WORKER);
        *running_bytes = running_bytes.saturating_add(file.size);
        let row = HybridUploaded {
            file_index: global_idx,
            dropbox_id: id.clone(),
        };
        client.write_limiter().on_success();
        on_file_done(global_idx + 1, *running_bytes, &row)?;
        return Ok(vec![id]);
    }

    control.wait_if_paused().await?;
    let start = client
        .rpc(
            "/files/upload_session/start_batch",
            json!({ "num_sessions": n }),
        )
        .await?;
    let session_ids: Vec<String> = start
        .get("session_ids")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if session_ids.len() != n {
        return Err(CloudError::Message(format!(
            "start_batch: erwartet {n} session_ids, erhalten {}",
            session_ids.len()
        )));
    }

    let limiter = client.write_limiter();
    let mut queue: VecDeque<usize> = (0..n).collect();
    let mut cursors: Vec<Option<DropboxCursor>> = vec![None; n];
    let batch_prog = BatchProgress::new(BATCH_PARALLEL_WORKERS, bytes_uploaded_base, total_job_size);
    let mut lane_busy = [false; BATCH_PARALLEL_WORKERS];
    let mut set: JoinSet<(usize, Result<(usize, usize, DropboxCursor), CloudError>)> = JoinSet::new();

    let spawn_large = |worker_id: usize, batch_idx: usize| {
        let client = client.clone();
        let control = control.clone();
        let file = (*large_files[batch_idx]).clone();
        let session_id = session_ids[batch_idx].clone();
        let global_idx = global_indices[batch_idx];
        let batch_prog = Arc::clone(&batch_prog);
        async move {
            control.wait_if_paused().await?;
            emit_file_status(&file, global_idx, total_files_in_job, worker_id);
            let size = file.size;
            let bp = Arc::clone(&batch_prog);
            let emit = std::sync::Arc::new(move |sent: u64| {
                let sent = sent.min(size);
                bp.report_inflight(worker_id, sent, false);
                events::upload_slots_worker_progress(worker_id, sent, size);
                events::emit_progress_file(percent(sent, size), sent, size);
            }) as std::sync::Arc<dyn Fn(u64) + Send + Sync>;
            let cursor = append_file_to_session(&client, &file, &session_id, &control, emit).await?;
            events::upload_slots_worker_progress(worker_id, file.size, file.size);
            batch_prog.complete_slot(worker_id, file.size);
            Ok((batch_idx, global_idx, cursor))
        }
    };

    while !queue.is_empty() || !set.is_empty() {
        control.wait_if_paused().await?;
        let cap = limiter.current_workers(worker_capacity(set.len(), queue.len()));
        try_spawn_large_lanes(&mut set, &mut lane_busy, cap, &mut queue, |worker_id, batch_idx| {
            spawn_large(worker_id, batch_idx)
        });

        let Some(joined) = set.join_next().await else {
            continue;
        };
        match joined {
            Ok((worker_id, Ok((batch_idx, global_idx, cursor)))) => {
                lane_busy[worker_id] = false;
                cursors[batch_idx] = Some(cursor);
                let file = large_files[batch_idx];
                *running_bytes = running_bytes.saturating_add(file.size);
                let row = HybridUploaded {
                    file_index: global_idx,
                    dropbox_id: None,
                };
                limiter.on_success();
                on_file_done(global_idx + 1, *running_bytes, &row)?;
                emit_file_done(worker_id);

                let cap = limiter.current_workers(worker_capacity(set.len(), queue.len()));
                try_spawn_large_lanes(&mut set, &mut lane_busy, cap, &mut queue, |worker_id, batch_idx| {
                    spawn_large(worker_id, batch_idx)
                });
            }
            Ok((_, Err(e))) => {
                set.abort_all();
                return Err(e);
            }
            Err(e) => {
                set.abort_all();
                return Err(CloudError::Message(format!("Join-Fehler: {e}")));
            }
        }
    }

    emit_total(batch_prog.combined(), total_job_size);

    let mut entries = Vec::with_capacity(n);
    for (i, file) in large_files.iter().enumerate() {
        let cursor = cursors[i].as_ref().ok_or_else(|| {
            CloudError::Message(format!(
                "Batch-Upload unvollständig für {}",
                file.rel_norm
            ))
        })?;
        entries.push(json!({
            "cursor": {
                "session_id": cursor.session_id,
                "offset": cursor.offset,
            },
            "commit": {
                "path": file.dropbox_path,
                "mode": { ".tag": "overwrite" },
            }
        }));
    }

    events::emit_status(format!("Committe {n} Dateien in Dropbox..."));
    let finish = finish_batch_v2(client, entries).await?;
    let entry_results = finish
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entry_results.len() != n {
        return Err(CloudError::Message(format!(
            "finish_batch_v2: erwartet {n} entries, erhalten {}",
            entry_results.len()
        )));
    }

    let mut ids = Vec::with_capacity(n);
    for (i, entry) in entry_results.iter().enumerate() {
        let tag = entry.get(".tag").and_then(Value::as_str).unwrap_or("");
        if tag == "success" || entry.get("id").is_some() {
            let meta = if tag == "success" {
                entry.get("success").unwrap_or(entry)
            } else {
                entry
            };
            ids.push(meta.get("id").and_then(Value::as_str).map(str::to_string));
        } else {
            let failure = entry.get("failure").cloned().unwrap_or_else(|| entry.clone());
            return Err(CloudError::Message(format!(
                "finish_batch_v2 fehlgeschlagen für {}: {failure}",
                large_files[i].rel_norm
            )));
        }
    }

    if let Some(last) = large_files.last() {
        events::emit_progress_file(100, last.size, last.size.max(1));
    }
    logging::log_info(&format!(
        "Dropbox Batch abgeschlossen: {} Dateien.",
        ids.len()
    ));
    Ok(ids)
}

async fn append_file_to_session(
    client: &DropboxClient,
    file: &UploadFile,
    session_id: &str,
    control: &UploadControl,
    emit_progress: std::sync::Arc<dyn Fn(u64) + Send + Sync>,
) -> Result<DropboxCursor, CloudError> {
    use tokio::io::AsyncSeekExt;

    let file_size = file.size;
    let mut fh = tokio::fs::File::open(&file.local_path).await?;
    let mut offset = 0u64;
    emit_progress(0);

    let upload_chunk = |offset: u64, close: bool, data: bytes::Bytes| {
        let emit = Arc::clone(&emit_progress);
        let chunk_len = data.len() as u64;
        let on_send = std::sync::Arc::new(move |sent: u64| {
            let abs = offset.saturating_add(sent.min(chunk_len)).min(file_size);
            emit(abs);
        }) as std::sync::Arc<dyn Fn(u64) + Send + Sync>;
        let arg = session_append_arg(session_id, offset, close);
        async move {
            client
                .content_upload_with_progress(
                    "/files/upload_session/append_v2",
                    &arg,
                    data,
                    control,
                    Some(on_send),
                )
                .await
        }
    };

    if file_size == 0 {
        control.wait_if_paused().await?;
        let arg = session_append_arg(session_id, 0, true);
        client
            .content_upload_with_progress(
                "/files/upload_session/append_v2",
                &arg,
                bytes::Bytes::new(),
                control,
                None,
            )
            .await?;
        return Ok(DropboxCursor {
            session_id: session_id.to_string(),
            offset: 0,
        });
    }

    let mut buf = vec![0u8; CHUNK_SIZE];
    if file_size <= CHUNK_SIZE as u64 {
        control.wait_if_paused().await?;
        let n = read_chunk(&mut fh, &mut buf).await?;
        upload_chunk(0, true, bytes::Bytes::copy_from_slice(&buf[..n])).await?;
        emit_progress(file_size);
        return Ok(DropboxCursor {
            session_id: session_id.to_string(),
            offset: file_size,
        });
    }

    control.wait_if_paused().await?;
    let n = read_chunk(&mut fh, &mut buf).await?;
    upload_chunk(offset, false, bytes::Bytes::copy_from_slice(&buf[..n])).await?;
    offset += n as u64;
    emit_progress(offset);

    while file_size.saturating_sub(offset) > CHUNK_SIZE as u64 {
        control.wait_if_paused().await?;
        let n = read_chunk(&mut fh, &mut buf).await?;
        if n == 0 {
            break;
        }
        upload_chunk(offset, false, bytes::Bytes::copy_from_slice(&buf[..n])).await?;
        offset += n as u64;
        emit_progress(offset);
    }

    control.wait_if_paused().await?;
    let _ = fh.seek(std::io::SeekFrom::Start(offset)).await;
    let n = read_chunk(&mut fh, &mut buf).await?;
    upload_chunk(offset, true, bytes::Bytes::copy_from_slice(&buf[..n])).await?;
    offset = file_size;
    emit_progress(offset);
    Ok(DropboxCursor {
        session_id: session_id.to_string(),
        offset,
    })
}

async fn finish_batch_v2(client: &DropboxClient, entries: Vec<Value>) -> Result<Value, CloudError> {
    let result = client
        .rpc(
            "/files/upload_session/finish_batch_v2",
            json!({ "entries": entries }),
        )
        .await?;

    if let Some(job_id) = result.get("async_job_id").and_then(Value::as_str) {
        let job_id = job_id.to_string();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let check = client
                .rpc(
                    "/files/upload_session/finish_batch/check",
                    json!({ "async_job_id": job_id }),
                )
                .await?;
            match check.get(".tag").and_then(Value::as_str) {
                Some("complete") => {
                    return Ok(check.get("complete").cloned().unwrap_or(check));
                }
                Some("in_progress") => continue,
                _ => {
                    if check.get("entries").is_some() {
                        return Ok(check);
                    }
                    return Err(CloudError::Message(format!(
                        "finish_batch/check unerwartet: {check}"
                    )));
                }
            }
        }
    }

    Ok(result)
}

fn emit_file_status(file: &UploadFile, global_idx: usize, total: usize, worker_id: usize) {
    let name = file
        .local_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file.rel_norm.as_str());
    events::emit_status(format!("Lade hoch: {name}"));
    events::emit_progress_message(format!(
        "Datei {}/{total}: {}",
        global_idx + 1,
        file.rel_norm
    ));
    events::upload_slots_worker_start(worker_id, global_idx, name, file.size);
}

fn emit_file_done(worker_id: usize) {
    events::upload_slots_worker_finish(worker_id);
}

fn emit_total(current: u64, total: u64) {
    events::emit_progress_total(percent(current, total), current, total);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn uf(name: &str, size: u64) -> UploadFile {
        UploadFile {
            local_path: PathBuf::from(name),
            dropbox_path: format!("/{name}"),
            size,
            rel_norm: name.to_string(),
        }
    }

    #[test]
    fn partition_small_large() {
        let files = vec![
            uf("a.jpg", 100),
            uf("b.bin", SMALL_FILE_BYTES as u64),
            uf("c.bin", SMALL_FILE_BYTES as u64 + 1),
            uf("d.bin", CHUNK_SIZE as u64),
        ];
        let (small, large) = super::partition_small_large(&files);
        assert_eq!(small, vec![0, 1]);
        assert_eq!(large, vec![2, 3]);
    }

    #[test]
    fn batch_slices() {
        assert!(super::batch_slices(0, 1000).is_empty());
        assert_eq!(super::batch_slices(3, 1000), vec![(0, 3)]);
        assert_eq!(super::batch_slices(5, 2), vec![(0, 2), (2, 4), (4, 5)]);
        assert_eq!(super::batch_slices(1000, 1000), vec![(0, 1000)]);
        assert_eq!(
            super::batch_slices(1001, 1000),
            vec![(0, 1000), (1000, 1001)]
        );
    }
}
