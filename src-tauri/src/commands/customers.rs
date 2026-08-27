//! Tauri IPC for customer intake and marker assignment.

use std::path::PathBuf;

use tauri::State;

use crate::cloud::custom_api::fetch_customer_as_kunde_with_extras;
use crate::commands::settings::ConfigState;
use crate::model::crew::load_crew_list;
use crate::model::customer_intake::{
    classify_typed_hits, hit_from_kunde, is_lookup_id_pair_ready, ClassifiedIntakeHits,
    IntakeLookupHit, IntakeLookupResult, INTAKE_LOOKUP_TYPES,
};
use crate::model::id_assign::{IdAssignOverride, IdAssignPreview};
use crate::model::marker::{ApiMarkerQuery, LookupMode};
use crate::storage::customers::{
    list_media_folders, propose_batch_assignments, rank_folders_for_customer, AssignResult,
    AssignmentHistoryEntry, BatchAssignItem, BatchAssignOutcome, BatchAssignmentProposal, Customer,
    CustomerDraft, CustomerState, MediaDirectoryListing,
};
use crate::util::archive::is_customer_lookup_failure;

#[tauri::command]
pub fn list_customers(
    customers: State<'_, CustomerState>,
    search: Option<String>,
    filter: Option<String>,
) -> Result<Vec<Customer>, String> {
    customers.list(
        search.as_deref().unwrap_or(""),
        filter.as_deref().unwrap_or("all"),
    )
}

#[tauri::command]
pub fn save_customer(
    customers: State<'_, CustomerState>,
    draft: CustomerDraft,
) -> Result<Customer, String> {
    customers.save(&draft)
}

#[tauri::command]
pub fn update_customer(
    customers: State<'_, CustomerState>,
    customer: Customer,
) -> Result<Customer, String> {
    customers.update(&customer)
}

#[tauri::command]
pub fn delete_customer(customers: State<'_, CustomerState>, id: String) -> Result<(), String> {
    customers.delete(&id)
}

#[tauri::command]
pub fn set_customer_processed(
    customers: State<'_, CustomerState>,
    id: String,
    processed: bool,
) -> Result<Customer, String> {
    customers.set_processed(&id, processed)
}

#[tauri::command]
pub fn list_media_folders_cmd(
    config: State<'_, ConfigState>,
    path: Option<String>,
    vorname: Option<String>,
    nachname: Option<String>,
) -> Result<MediaDirectoryListing, String> {
    let target = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p.trim()),
        _ => {
            let monitor = config.get("monitor_path", Some(""))?;
            if monitor.trim().is_empty() {
                return Err(
                    "Kein Überwachungsordner konfiguriert. Bitte in den Einstellungen setzen."
                        .into(),
                );
            }
            PathBuf::from(monitor.trim())
        }
    };
    let mut listing = list_media_folders(&target).map_err(|e| e.to_string())?;
    let vorname = vorname.unwrap_or_default();
    let nachname = nachname.unwrap_or_default();
    if !vorname.trim().is_empty() || !nachname.trim().is_empty() {
        rank_folders_for_customer(&mut listing.folders, &vorname, &nachname);
    }
    Ok(listing)
}

#[tauri::command]
pub fn assign_customer_to_folder(
    customers: State<'_, CustomerState>,
    config: State<'_, ConfigState>,
    id: String,
    target_path: String,
    id_override: Option<IdAssignOverride>,
) -> Result<AssignResult, String> {
    let path = PathBuf::from(target_path.trim());
    let crew_raw = config.get("crew_list", Some(""))?;
    let crew = load_crew_list(&crew_raw);
    customers.assign_to_folder(&id, &path, &crew, id_override.as_ref())
}

/// Non-mutating ID-assign preview (predictor + live folder name) for Review UI (19d).
#[tauri::command]
pub fn preview_id_assign(
    customers: State<'_, CustomerState>,
    config: State<'_, ConfigState>,
    id: String,
    target_path: String,
    id_override: Option<IdAssignOverride>,
) -> Result<IdAssignPreview, String> {
    let path = PathBuf::from(target_path.trim());
    let crew_raw = config.get("crew_list", Some(""))?;
    let crew = load_crew_list(&crew_raw);
    customers.preview_id_assign(&id, &path, &crew, id_override.as_ref())
}

#[tauri::command]
pub fn get_assignment_history(
    customers: State<'_, CustomerState>,
) -> Result<Vec<AssignmentHistoryEntry>, String> {
    customers.assignment_history()
}

#[tauri::command]
pub fn propose_customer_assignments(
    customers: State<'_, CustomerState>,
    config: State<'_, ConfigState>,
) -> Result<BatchAssignmentProposal, String> {
    let monitor = config.get("monitor_path", Some(""))?;
    if monitor.trim().is_empty() {
        return Err(
            "Kein Überwachungsordner konfiguriert. Bitte in den Einstellungen setzen.".into(),
        );
    }
    let listing = list_media_folders(&PathBuf::from(monitor.trim())).map_err(|e| e.to_string())?;
    let open = customers.list("", "unprocessed")?;
    let rows = propose_batch_assignments(&open, &listing.folders);
    Ok(BatchAssignmentProposal {
        rows,
        folders: listing.folders,
    })
}

#[tauri::command]
pub fn assign_customers_batch(
    customers: State<'_, CustomerState>,
    config: State<'_, ConfigState>,
    items: Vec<BatchAssignItem>,
) -> Result<BatchAssignOutcome, String> {
    let crew_raw = config.get("crew_list", Some(""))?;
    let crew = load_crew_list(&crew_raw);
    customers.assign_many(&items, &crew)
}

/// Dual Handcam/Outside Customer-API lookup for intake (Phase 19b).
#[tauri::command]
pub async fn lookup_customer_intake(
    kunden_id: String,
    booking_id: String,
) -> Result<IntakeLookupResult, String> {
    let kunden_id = kunden_id.trim().to_string();
    let booking_id = booking_id.trim().to_string();
    if !is_lookup_id_pair_ready(&kunden_id, &booking_id) {
        return Ok(IntakeLookupResult::Error {
            message: "Kunden-ID und Buchungs-ID müssen jeweils mindestens 4 Ziffern haben.".into(),
        });
    }

    let mut handcam: Option<IntakeLookupHit> = None;
    let mut outside: Option<IntakeLookupHit> = None;
    let mut last_error: Option<String> = None;
    let mut saw_not_found = false;

    for marker_type in INTAKE_LOOKUP_TYPES {
        let query = ApiMarkerQuery {
            customer_id: kunden_id.clone(),
            booking_id: booking_id.clone(),
            marker_type: (*marker_type).to_string(),
        };
        match fetch_customer_as_kunde_with_extras(&query, LookupMode::Id).await {
            Ok((kunde, extras)) => {
                let hit = hit_from_kunde(
                    &kunde,
                    &kunden_id,
                    &booking_id,
                    &extras.booking_date,
                    &extras.media_option,
                );
                // Prefer API typ; if empty, stamp requested family.
                let mut hit = hit;
                if hit.typ.trim().is_empty() {
                    hit.typ = (*marker_type).to_string();
                }
                match *marker_type {
                    "Handcam" => handcam = Some(hit),
                    "Outside" => outside = Some(hit),
                    _ => {}
                }
            }
            Err(msg) => {
                let lower = msg.to_ascii_lowercase();
                if is_customer_lookup_failure(&msg)
                    || lower.contains("nicht gefunden")
                    || lower.contains("not found")
                    || lower.contains("http 404")
                {
                    saw_not_found = true;
                } else {
                    last_error = Some(msg);
                }
            }
        }
    }

    match classify_typed_hits(handcam.as_ref(), outside.as_ref()) {
        ClassifiedIntakeHits::One { customer } => Ok(IntakeLookupResult::Hit { customer }),
        ClassifiedIntakeHits::Choice { handcam, outside } => {
            Ok(IntakeLookupResult::Choice { handcam, outside })
        }
        ClassifiedIntakeHits::None => {
            if let Some(message) = last_error {
                Ok(IntakeLookupResult::Error { message })
            } else if saw_not_found || handcam.is_none() && outside.is_none() {
                Ok(IntakeLookupResult::NotFound)
            } else {
                Ok(IntakeLookupResult::NotFound)
            }
        }
    }
}
