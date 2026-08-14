//! Tauri IPC for persistent upload history.

use tauri::State;

use crate::storage::history::{HistoryEntry, HistoryPage, HistoryState};

#[tauri::command]
pub fn get_history(
    history: State<'_, HistoryState>,
    search: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<HistoryPage, String> {
    history.get_filtered(
        search.as_deref().unwrap_or(""),
        page.unwrap_or(0),
        page_size.unwrap_or(25),
    )
}

#[tauri::command]
pub fn get_history_entry(
    history: State<'_, HistoryState>,
    id: String,
) -> Result<Option<HistoryEntry>, String> {
    history.get_by_id(&id)
}

#[tauri::command]
pub fn delete_history_items(
    history: State<'_, HistoryState>,
    ids: Vec<String>,
) -> Result<usize, String> {
    history.delete_items(&ids)
}

#[tauri::command]
pub fn clear_history(history: State<'_, HistoryState>) -> Result<(), String> {
    history.clear_all()
}
