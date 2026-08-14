//! Tauri IPC for customer intake and marker assignment.

use std::path::PathBuf;

use tauri::State;

use crate::commands::settings::ConfigState;
use crate::storage::customers::{
    list_media_folders, AssignResult, AssignmentHistoryEntry, Customer, CustomerState,
    MediaDirectoryListing,
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
pub fn delete_customer(
    customers: State<'_, CustomerState>,
    id: String,
) -> Result<(), String> {
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
    list_media_folders(&target).map_err(|e| e.to_string())
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
