//! Tauri IPC for optional AMS LAN Bridge (Phase 13 / P2).

use tauri::State;

use crate::bridge::{BridgeState, BridgeStatus};
use crate::commands::ConfigState;

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
