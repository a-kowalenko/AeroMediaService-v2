//! Tauri IPC for optional AMS LAN Bridge (Phase 13 / P2).

use tauri::State;

use crate::bridge::{BridgeState, BridgeStatus};
use crate::commands::ConfigState;
use crate::storage::ats_presence::{
    AtsHostDetails, AtsHostSummary, AtsJobOriginRecord, AtsPresenceState,
};
use crate::storage::history::HistoryState;
use serde::Serialize;

#[tauri::command]
pub fn get_bridge_status(bridge: State<'_, BridgeState>) -> BridgeStatus {
    bridge.status()
}

/// Start, restart, or stop the bridge according to current settings + token.
#[tauri::command]
pub async fn apply_bridge_config(
    config: State<'_, ConfigState>,
    bridge: State<'_, BridgeState>,
) -> Result<BridgeStatus, String> {
    bridge.apply_from_config(config.inner()).await
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AtsJobOriginView {
    pub correlation_id: String,
    pub folder_name: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub source_event_type: String,
    pub ams_status_label: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AtsHostDetailsView {
    pub host: AtsHostDetails,
    pub recent_jobs: Vec<AtsJobOriginView>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AtsJobsPageView {
    pub items: Vec<AtsJobOriginView>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

#[tauri::command]
pub fn get_ats_hosts_summary(
    presence: State<'_, AtsPresenceState>,
    ttl_minutes: Option<u32>,
) -> Result<Vec<AtsHostSummary>, String> {
    presence.get_hosts_summary(ttl_minutes.unwrap_or(60))
}

#[tauri::command]
pub fn get_ats_host_details(
    presence: State<'_, AtsPresenceState>,
    history: State<'_, HistoryState>,
    instance_id: String,
    ttl_minutes: Option<u32>,
    limit: Option<u32>,
) -> Result<Option<AtsHostDetailsView>, String> {
    let Some(details) = presence.get_host_details(
        &instance_id,
        ttl_minutes.unwrap_or(60),
        limit.unwrap_or(100),
    )? else {
        return Ok(None);
    };
    let recent_jobs = enrich_jobs(&history, details.recent_jobs.clone())?;
    Ok(Some(AtsHostDetailsView {
        host: AtsHostDetails {
            recent_jobs: Vec::new(),
            ..details
        },
        recent_jobs,
    }))
}

#[tauri::command]
pub fn get_ats_jobs_by_host(
    presence: State<'_, AtsPresenceState>,
    history: State<'_, HistoryState>,
    instance_id: String,
    ttl_minutes: Option<u32>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<AtsJobsPageView, String> {
    let page_data = presence.get_jobs_by_host(
        &instance_id,
        ttl_minutes.unwrap_or(60),
        page.unwrap_or(0),
        page_size.unwrap_or(50),
    )?;
    Ok(AtsJobsPageView {
        items: enrich_jobs(&history, page_data.items)?,
        total: page_data.total,
        page: page_data.page,
        page_size: page_data.page_size,
    })
}

fn enrich_jobs(
    history: &HistoryState,
    jobs: Vec<AtsJobOriginRecord>,
) -> Result<Vec<AtsJobOriginView>, String> {
    jobs.into_iter()
        .map(|job| {
            let label = history
                .find_by_correlation_id(&job.correlation_id)?
                .map(|entry| {
                    let overall = entry.overall_status.trim();
                    if overall.is_empty() {
                        entry.status
                    } else {
                        overall.to_string()
                    }
                })
                .unwrap_or_else(|| "Unbekannt".into());
            Ok(AtsJobOriginView {
                correlation_id: job.correlation_id,
                folder_name: job.folder_name,
                first_seen_at: job.first_seen_at,
                last_seen_at: job.last_seen_at,
                source_event_type: job.source_event_type,
                ams_status_label: label,
            })
        })
        .collect()
}
