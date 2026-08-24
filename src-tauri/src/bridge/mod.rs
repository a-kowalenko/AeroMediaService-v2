//! Optional LAN Bridge API (Phase 13 / P4).
//! Spec: `docs/HANDOFF.md` §9 — health, lookup, jobs, ready + mDNS advertise.
//! File handoff (Manifest + Outbox) must work without this module.

mod identity;
mod mdns;
mod presence;
mod server;
mod types;

pub use identity::{ensure_instance_id, resolve_display_name};

pub use server::{BridgeRuntime, BridgeStatus, MonitorCancelFn, MonitorWakeFn};
pub use types::{DEFAULT_BRIDGE_BIND, P3_CAPABILITIES};

use std::sync::{Arc, Mutex};

use crate::commands::ConfigState;
use crate::storage::ats_presence::AtsPresenceState;
use crate::storage::logging;
use crate::storage::secrets;

/// Tauri-managed bridge handle (start/stop/restart from settings).
#[derive(Clone)]
pub struct BridgeState {
    inner: Arc<Mutex<Option<BridgeRuntime>>>,
    presence: AtsPresenceState,
    wake_monitor: MonitorWakeFn,
    cancel_monitor: MonitorCancelFn,
}

impl BridgeState {
    pub fn new(
        wake_monitor: MonitorWakeFn,
        cancel_monitor: MonitorCancelFn,
        presence: AtsPresenceState,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            presence,
            wake_monitor,
            cancel_monitor,
        }
    }

    pub fn status(&self) -> BridgeStatus {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(rt) => rt.status(),
            None => BridgeStatus {
                running: false,
                bind_addr: String::new(),
                display_name: String::new(),
                instance_id: String::new(),
                mdns_active: false,
                last_error: None,
            },
        }
    }

    pub async fn stop(&self) {
        let runtime = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        if let Some(rt) = runtime {
            rt.shutdown().await;
            logging::log_info("AMS-Bridge gestoppt.");
        }
    }

    /// Apply config: stop when disabled; (re)start when enabled + token present.
    pub async fn apply_from_config(&self, config: &ConfigState) -> Result<BridgeStatus, String> {
        let enabled = config
            .get("bridge_enabled", Some("false"))
            .unwrap_or_else(|_| "false".into())
            .trim()
            .eq_ignore_ascii_case("true");
        if !enabled {
            self.stop().await;
            return Ok(self.status());
        }

        let bind = config
            .get("bridge_bind", Some(DEFAULT_BRIDGE_BIND))
            .unwrap_or_else(|_| DEFAULT_BRIDGE_BIND.into());
        let bind = bind.trim();
        if bind.is_empty() {
            return Err("bridge_bind ist leer.".into());
        }

        let token = secrets::get_secret("bridge_token")
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let token = token.trim().to_string();
        if token.is_empty() {
            self.stop().await;
            return Err("Bridge aktiv, aber bridge_token fehlt (Token-Auth ist Pflicht).".into());
        }

        let monitor_path = config.get("monitor_path", Some("")).unwrap_or_default();
        let version = env!("CARGO_PKG_VERSION").to_string();

        self.stop().await;
        let runtime = BridgeRuntime::start(
            bind.to_string(),
            token,
            monitor_path,
            version,
            config.clone(),
            self.presence.clone(),
            Arc::clone(&self.wake_monitor),
            Arc::clone(&self.cancel_monitor),
        )
            .await?;
        let status = runtime.status();
        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(runtime);
        }
        logging::log_info(&format!(
            "AMS-Bridge gestartet auf {} (capabilities: {:?})",
            status.bind_addr, P3_CAPABILITIES
        ));
        Ok(status)
    }
}
