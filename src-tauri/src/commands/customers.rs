//! Tauri IPC for customer intake and marker assignment.

use std::path::PathBuf;

use tauri::State;

use crate::commands::settings::ConfigState;
use crate::storage::customers::{
    list_media_folders, propose_batch_assignments, rank_folders_for_customer, AssignResult,
    AssignmentHistoryEntry, BatchAssignItem, BatchAssignOutcome, BatchAssignmentProposal, Customer,
    CustomerState, MediaDirectoryListing,
};

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
    vorname: String,
    nachname: String,
    email: String,
    telefon: Option<String>,
) -> Result<Customer, String> {
    customers.save(
        &vorname,
        &nachname,
        &email,
        telefon.as_deref().unwrap_or(""),
    )
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
    id: String,
    target_path: String,
) -> Result<AssignResult, String> {
    let path = PathBuf::from(target_path.trim());
    customers.assign_to_folder(&id, &path)
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
    items: Vec<BatchAssignItem>,
) -> Result<BatchAssignOutcome, String> {
    customers.assign_many(&items)
}
