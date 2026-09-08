//! Elevierter SMB-Helper (Phase 20d) — nur List / Safe-Close.
//!
//! Wird vom unelevierten AMS per `ShellExecuteEx` Verb `runas` gestartet.
//! Kein PowerShell, kein Pipeline-/Config-Zugriff außer CLI-Parameter.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let code = aero_media_service_lib::run_smb_helper(std::env::args().skip(1).collect());
    std::process::exit(code);
}
