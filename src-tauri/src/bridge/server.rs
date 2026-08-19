//! Axum LAN server: health, lookup, job status, handoff/ready (Phase 13 / P3).

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::types::{
    HandoffReadyRequest, HandoffReadyResponse, HealthResponse, JobStatusResponse, LookupRequest,
    LookupResponse,
};
use crate::cloud::custom_api::fetch_customer_as_kunde;
use crate::commands::ConfigState;
use crate::model::handoff::{read_status_outbox, CODE_CUSTOMER_LOOKUP_FAILED};
use crate::model::marker::ApiMarkerQuery;
use crate::storage::ats_presence::AtsPresenceState;
use crate::storage::logging;
use super::presence::{record_bridge_event, BridgeEventKind};

/// Callback to interrupt the monitor wait loop (no upload enqueue).
/// Args: folder_name, correlation_id (either may be empty).
pub type MonitorWakeFn = Arc<dyn Fn(String, String) + Send + Sync>;

#[derive(Clone)]
struct AppState {
    token: Arc<String>,
    version: String,
    /// Live monitor_path from config on each health / jobs call.
    config: ConfigState,
    presence: AtsPresenceState,
    wake_monitor: MonitorWakeFn,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatus {
    pub running: bool,
    pub bind_addr: String,
    pub last_error: Option<String>,
}

pub struct BridgeRuntime {
    bind_addr: String,
    shutdown_tx: Option<oneshot::Sender<()>>,
    join: Option<tauri::async_runtime::JoinHandle<()>>,
    mdns: Option<super::mdns::MdnsAdvertiser>,
}

impl BridgeRuntime {
    pub fn status(&self) -> BridgeStatus {
        BridgeStatus {
            running: self.shutdown_tx.is_some(),
            bind_addr: self.bind_addr.clone(),
            last_error: None,
        }
    }

    pub async fn shutdown(mut self) {
        if let Some(mdns) = self.mdns.take() {
            mdns.stop();
        }
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.await;
        }
    }

    pub async fn start(
        bind: String,
        token: String,
        monitor_path_initial: String,
        version: String,
        config: ConfigState,
        presence: AtsPresenceState,
        wake_monitor: MonitorWakeFn,
    ) -> Result<Self, String> {
        let addr: SocketAddr = bind
            .parse()
            .map_err(|e| format!("Ungültige bridge_bind-Adresse '{bind}': {e}"))?;

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("Bridge-Bind fehlgeschlagen ({bind}): {e}"))?;
        let local = listener
            .local_addr()
            .map_err(|e| format!("Bridge local_addr: {e}"))?;

        let state = AppState {
            token: Arc::new(token),
            version: version.clone(),
            config,
            presence,
            wake_monitor,
        };

        let app = Router::new()
            .route("/v1/health", get(health))
            .route("/v1/customer/lookup", post(customer_lookup))
            .route("/v1/jobs/{correlation_id}", get(job_status))
            .route("/v1/handoff/ready", post(handoff_ready))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                require_bearer,
            ))
            .with_state(state);

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let join = tauri::async_runtime::spawn(async move {
            let server = axum::serve(listener, app).with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            });
            if let Err(e) = server.await {
                logging::log_warn(&format!("AMS-Bridge Server-Fehler: {e}"));
            }
        });

        let mdns = super::mdns::MdnsAdvertiser::start(local, &version, &monitor_path_initial);

        Ok(Self {
            bind_addr: local.to_string(),
            shutdown_tx: Some(shutdown_tx),
            join: Some(join),
            mdns,
        })
    }
}

async fn require_bearer(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t == state.token.as_str())
        .unwrap_or(false);

    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "ok": false,
                "error": { "code": "unauthorized", "message": "Bearer token required" }
            })),
        )
            .into_response();
    }
    next.run(request).await
}

async fn health(State(state): State<AppState>, headers: HeaderMap) -> Json<HealthResponse> {
    let monitor_path = state
        .config
        .get("monitor_path", Some(""))
        .unwrap_or_default();
    let response = Json(HealthResponse::p3(&state.version, monitor_path));
    record_bridge_event(
        &state.presence,
        &headers,
        BridgeEventKind::Health,
        "/v1/health",
        "GET",
        StatusCode::OK,
        None,
        None,
        None,
    );
    response
}

async fn customer_lookup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LookupRequest>,
) -> impl IntoResponse {
    let mode = match body.lookup_mode() {
        Ok(m) => m,
        Err(msg) => {
            let failure = LookupResponse::failure("invalid_mode", msg);
            let response = (
                StatusCode::BAD_REQUEST,
                Json(failure.clone()),
            );
            record_bridge_event(
                &state.presence,
                &headers,
                BridgeEventKind::CustomerLookup,
                "/v1/customer/lookup",
                "POST",
                StatusCode::BAD_REQUEST,
                None,
                None,
                Some(lookup_event_payload(&body, StatusCode::BAD_REQUEST, &failure)),
            );
            return response;
        }
    };

    let customer_id = body.customer_id.trim().to_string();
    let booking_id = body.booking_id.trim().to_string();
    let marker_type = body.marker_type.trim().to_string();
    if customer_id.is_empty() || booking_id.is_empty() || marker_type.is_empty() {
        let failure = LookupResponse::failure(
            "invalid_request",
            "customer_id, booking_id und type sind Pflicht.",
        );
        let response = (StatusCode::BAD_REQUEST, Json(failure.clone()));
        record_bridge_event(
            &state.presence,
            &headers,
            BridgeEventKind::CustomerLookup,
            "/v1/customer/lookup",
            "POST",
            StatusCode::BAD_REQUEST,
            None,
            None,
            Some(lookup_event_payload(&body, StatusCode::BAD_REQUEST, &failure)),
        );
        return response;
    }

    let query = ApiMarkerQuery {
        customer_id,
        booking_id,
        marker_type,
    };

    let response = match fetch_customer_as_kunde(&query, mode).await {
        Ok(kunde) => (StatusCode::OK, Json(LookupResponse::success(kunde))),
        Err(msg) => (
            StatusCode::BAD_GATEWAY,
            Json(LookupResponse::failure(CODE_CUSTOMER_LOOKUP_FAILED, msg)),
        ),
    };
    record_bridge_event(
        &state.presence,
        &headers,
        BridgeEventKind::CustomerLookup,
        "/v1/customer/lookup",
        "POST",
        response.0,
        None,
        None,
        Some(lookup_event_payload(&body, response.0, &response.1.0)),
    );
    response
}

fn lookup_event_payload(
    body: &LookupRequest,
    status: StatusCode,
    response: &LookupResponse,
) -> Value {
    json!({
        "request": {
            "customer_id": body.customer_id.trim(),
            "booking_id": body.booking_id.trim(),
            "type": body.marker_type.trim(),
            "mode": body.mode.trim(),
        },
        "response": {
            "http_status": status.as_u16(),
            "ok": response.ok,
            "error": response.error,
            "customer": response.customer.as_ref().map(compact_kunde_payload),
        }
    })
}

fn compact_kunde_payload(kunde: &crate::model::kunde::Kunde) -> Value {
    json!({
        "customer_number": kunde.customer_number,
        "booking_number": kunde.booking_number,
        "first_name": kunde.first_name,
        "last_name": kunde.last_name,
        "email": kunde.email,
        "phone": kunde.phone,
        "type": kunde.customer_type,
        "handcam_foto": kunde.handcam_foto,
        "handcam_video": kunde.handcam_video,
        "outside_foto": kunde.outside_foto,
        "outside_video": kunde.outside_video,
    })
}

/// Mirror status outbox under `monitor_path/.ams-handoff/<correlation_id>.json`.
async fn job_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(correlation_id): AxumPath<String>,
) -> impl IntoResponse {
    let cid = correlation_id.trim();
    if cid.is_empty() {
        let response = (
            StatusCode::BAD_REQUEST,
            Json(JobStatusResponse::bad_request("correlation_id fehlt.")),
        );
        record_bridge_event(
            &state.presence,
            &headers,
            BridgeEventKind::JobStatus,
            "/v1/jobs/{correlation_id}",
            "GET",
            StatusCode::BAD_REQUEST,
            None,
            None,
            None,
        );
        return response;
    }

    let monitor_path = state
        .config
        .get("monitor_path", Some(""))
        .unwrap_or_default();
    let monitor_path = monitor_path.trim();
    if monitor_path.is_empty() {
        let response = (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(JobStatusResponse {
                ok: false,
                job: None,
                error: Some(super::types::LookupErrorBody {
                    code: "monitor_path_unset".into(),
                    message: "monitor_path ist nicht konfiguriert.".into(),
                }),
            }),
        );
        record_bridge_event(
            &state.presence,
            &headers,
            BridgeEventKind::JobStatus,
            "/v1/jobs/{correlation_id}",
            "GET",
            StatusCode::SERVICE_UNAVAILABLE,
            Some(cid),
            None,
            None,
        );
        return response;
    }

    let share_root = Path::new(monitor_path);
    let path = crate::model::handoff::outbox_path(share_root, cid);
    if !path.is_file() {
        let response = (
            StatusCode::NOT_FOUND,
            Json(JobStatusResponse::not_found(cid)),
        );
        record_bridge_event(
            &state.presence,
            &headers,
            BridgeEventKind::JobStatus,
            "/v1/jobs/{correlation_id}",
            "GET",
            StatusCode::NOT_FOUND,
            Some(cid),
            None,
            None,
        );
        return response;
    }
    let response = match read_status_outbox(share_root, cid) {
        Ok(doc) => (StatusCode::OK, Json(JobStatusResponse::found(doc))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(JobStatusResponse {
                ok: false,
                job: None,
                error: Some(super::types::LookupErrorBody {
                    code: "outbox_read_failed".into(),
                    message: e.to_string(),
                }),
            }),
        ),
    };
    record_bridge_event(
        &state.presence,
        &headers,
        BridgeEventKind::JobStatus,
        "/v1/jobs/{correlation_id}",
        "GET",
        response.0,
        Some(cid),
        None,
        None,
    );
    response
}

/// Wake monitor scan loop. Does **not** claim folders or bypass the upload queue.
async fn handoff_ready(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<HandoffReadyRequest>,
) -> impl IntoResponse {
    let cid = body.correlation_id.trim();
    let folder = body.folder_name.trim();
    logging::log_info(&format!(
        "AMS-Bridge handoff/ready: correlation_id={}, folder_name={} — Monitor wake (kein Upload-Bypass).",
        if cid.is_empty() { "-" } else { cid },
        if folder.is_empty() { "-" } else { folder },
    ));
    (state.wake_monitor)(folder.to_string(), cid.to_string());
    let response = (StatusCode::OK, Json(HandoffReadyResponse::woken()));
    record_bridge_event(
        &state.presence,
        &headers,
        BridgeEventKind::HandoffReady,
        "/v1/handoff/ready",
        "POST",
        StatusCode::OK,
        Some(cid),
        Some(folder),
        Some(json!({
            "correlation_id": cid,
            "folder_name": folder,
        })),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::CONFIG_DB_FILE;
    use crate::model::handoff::{write_status_outbox, OutboxAmsMeta, OutboxState, SCHEMA_V1};
    use crate::storage::config::ConfigStore;
    use crate::constants::ATS_PRESENCE_DB_FILE;
    use crate::storage::ats_presence::{AtsPresenceState, AtsPresenceStore};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    fn test_config_with_monitor(monitor: &str) -> ConfigState {
        let dir = tempdir().unwrap();
        let db = dir.path().join(CONFIG_DB_FILE);
        let mut store = ConfigStore::open_at(db).unwrap();
        store.save("monitor_path", monitor).unwrap();
        std::mem::forget(dir);
        ConfigState::from_store(store)
    }

    fn noop_wake() -> MonitorWakeFn {
        Arc::new(|_, _| {})
    }

    fn test_presence() -> AtsPresenceState {
        let dir = tempdir().unwrap();
        let store = AtsPresenceStore::open_at(dir.path().join(ATS_PRESENCE_DB_FILE)).unwrap();
        std::mem::forget(dir);
        AtsPresenceState::from_store(store)
    }

    #[tokio::test]
    async fn health_requires_token_and_returns_ready_capability() {
        let config = test_config_with_monitor(r"\\test\aktuell");
        let runtime = BridgeRuntime::start(
            "127.0.0.1:0".into(),
            "test-token-xyz".into(),
            String::new(),
            "0.1.0-test".into(),
            config,
            test_presence(),
            noop_wake(),
        )
            .await
            .expect("bind");
        let base = format!("http://{}", runtime.bind_addr);

        let client = reqwest::Client::new();
        let unauthorized = client
            .get(format!("{base}/v1/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let ok = client
            .get(format!("{base}/v1/health"))
            .header(header::AUTHORIZATION, "Bearer test-token-xyz")
            .send()
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        let body: HealthResponse = ok.json().await.unwrap();
        assert!(body.online);
        assert_eq!(body.version, "0.1.0-test");
        assert_eq!(body.monitor_path, r"\\test\aktuell");
        assert!(body.capabilities.contains(&"lookup".into()));
        assert!(body.capabilities.contains(&"manifest-v1".into()));
        assert!(body.capabilities.contains(&"status-outbox".into()));
        assert!(body.capabilities.contains(&"ready".into()));

        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn lookup_rejects_bad_mode() {
        let config = test_config_with_monitor(r"\\test\aktuell");
        let runtime = BridgeRuntime::start(
            "127.0.0.1:0".into(),
            "tok".into(),
            String::new(),
            "0.1.0".into(),
            config,
            test_presence(),
            noop_wake(),
        )
            .await
            .unwrap();
        let base = format!("http://{}", runtime.bind_addr);
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{base}/v1/customer/lookup"))
            .header(header::AUTHORIZATION, "Bearer tok")
            .json(&json!({
                "customer_id": "a",
                "booking_id": "b",
                "type": "Handcam",
                "mode": "weird"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body: LookupResponse = resp.json().await.unwrap();
        assert!(!body.ok);
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn jobs_returns_outbox_mirror_and_404() {
        let share = tempdir().unwrap();
        let cid = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
        write_status_outbox(
            share.path(),
            cid,
            OutboxState::Queued,
            None,
            OutboxAmsMeta::default(),
        )
            .unwrap();

        let config = test_config_with_monitor(share.path().to_str().unwrap());
        let runtime = BridgeRuntime::start(
            "127.0.0.1:0".into(),
            "tok".into(),
            String::new(),
            "0.1.0".into(),
            config,
            test_presence(),
            noop_wake(),
        )
            .await
            .unwrap();
        let base = format!("http://{}", runtime.bind_addr);
        let client = reqwest::Client::new();

        let ok = client
            .get(format!("{base}/v1/jobs/{cid}"))
            .header(header::AUTHORIZATION, "Bearer tok")
            .send()
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        let body: JobStatusResponse = ok.json().await.unwrap();
        assert!(body.ok);
        let job = body.job.unwrap();
        assert_eq!(job.schema, SCHEMA_V1);
        assert_eq!(job.correlation_id, cid);
        assert_eq!(job.state, OutboxState::Queued);

        let missing = client
            .get(format!("{base}/v1/jobs/no-such-id"))
            .header(header::AUTHORIZATION, "Bearer tok")
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn handoff_ready_wakes_monitor_without_auth_bypass() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let wakes_c = Arc::clone(&wakes);
        let wake: MonitorWakeFn = Arc::new(move |_, _| {
            wakes_c.fetch_add(1, Ordering::SeqCst);
        });

        let config = test_config_with_monitor(r"\\test\aktuell");
        let runtime = BridgeRuntime::start(
            "127.0.0.1:0".into(),
            "tok".into(),
            String::new(),
            "0.1.0".into(),
            config,
            test_presence(),
            wake,
        )
            .await
            .unwrap();
        let base = format!("http://{}", runtime.bind_addr);
        let client = reqwest::Client::new();

        let unauth = client
            .post(format!("{base}/v1/handoff/ready"))
            .json(&json!({ "correlation_id": "x", "folder_name": "f" }))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(wakes.load(Ordering::SeqCst), 0);

        let ok = client
            .post(format!("{base}/v1/handoff/ready"))
            .header(header::AUTHORIZATION, "Bearer tok")
            .json(&json!({
                "correlation_id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                "folder_name": "20260815_Test"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        let body: HandoffReadyResponse = ok.json().await.unwrap();
        assert!(body.ok && body.woken);
        assert_eq!(wakes.load(Ordering::SeqCst), 1);

        runtime.shutdown().await;
    }
}
