//! Customer lookup and orders/create (manifest submit).

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{
    extract_customer_url, CustomApiClient, MANIFEST_STATUS_POLL_INTERVAL_SECS,
    MANIFEST_STATUS_POLL_MAX_SECS, ORDERS_CREATE_TIMEOUT_SECS,
};
use crate::cloud::traits::CloudError;
use crate::events;
use crate::model::kunde::Kunde;
use crate::model::marker::{build_kunde_from_customer, ApiMarkerQuery, LookupMode};
use crate::storage::logging;
use crate::storage::secrets;
use crate::upload::control::UploadControl;

pub fn customer_lookup_url(base_url: &str, mode: LookupMode) -> String {
    let base = base_url.trim().trim_end_matches('/');
    match mode {
        LookupMode::Id => format!("{base}/aero-media-customer-fallback"),
        LookupMode::Hash => format!("{base}/aero-media-customer"),
    }
}

pub fn customer_lookup_params(query: &ApiMarkerQuery, mode: LookupMode) -> Vec<(String, String)> {
    let mut params = vec![
        ("customer_id".into(), query.customer_id.clone()),
        ("booking_id".into(), query.booking_id.clone()),
        ("type".into(), query.marker_type.clone()),
    ];
    if mode == LookupMode::Id {
        params.push(("Fallback".into(), "true".into()));
    }
    params
}

/// Loads customer data from the Aero customer API and maps it onto `Kunde`.
pub async fn fetch_customer_as_kunde(
    query: &ApiMarkerQuery,
    mode: LookupMode,
) -> Result<Kunde, String> {
    let payload = fetch_customer_payload(query, mode).await?;
    let customer = payload
        .get("customer")
        .cloned()
        .ok_or_else(|| "Customer-Lookup lieferte kein 'customer'-Objekt.".to_string())?;
    build_kunde_from_customer(&customer).map_err(|e| e.to_string())
}

pub async fn fetch_customer_payload(
    query: &ApiMarkerQuery,
    mode: LookupMode,
) -> Result<Value, String> {
    let api_base_url = secrets::get_secret("aero_customer_base_url")
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty());
    let api_token = secrets::get_secret("aero_customer_api_token")
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty());
    let (Some(api_base_url), Some(api_token)) = (api_base_url, api_token) else {
        return Err(
            "API-Credentials fehlen (aero_customer_base_url/aero_customer_api_token).".into(),
        );
    };

    let url = customer_lookup_url(&api_base_url, mode);
    let params = customer_lookup_params(query, mode);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_token}"))
        .header("Content-Type", "application/json")
        .query(&params)
        .send()
        .await
        .map_err(|e| format!("Customer-Lookup fehlgeschlagen: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(300).collect();
        return Err(format!(
            "Customer-Lookup fehlgeschlagen: HTTP {status} - {snippet}"
        ));
    }

    response
        .json::<Value>()
        .await
        .map_err(|e| format!("Customer-Lookup fehlgeschlagen: {e}"))
}

/// Share-link lookup via `aero-media-customer` (legacy `lookup_customer_url`).
pub async fn lookup_customer_url(
    customer_number: &str,
    booking_number: &str,
    customer_type: &str,
) -> Option<String> {
    let customer_id = customer_number.trim();
    let booking_id = booking_number.trim();
    let marker_type = customer_type.trim();
    if customer_id.is_empty() || booking_id.is_empty() || marker_type.is_empty() {
        return None;
    }

    let api_base_url = secrets::get_secret("aero_customer_base_url")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())?;
    let api_token = secrets::get_secret("aero_customer_api_token")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())?;

    let endpoint = format!(
        "{}/aero-media-customer",
        api_base_url.trim().trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;

    for fallback in [false, true] {
        let mut params = vec![
            ("customer_id", customer_id.to_string()),
            ("booking_id", booking_id.to_string()),
            ("type", marker_type.to_string()),
        ];
        if fallback {
            params.push(("Fallback", "true".into()));
        }
        let response = match client
            .get(&endpoint)
            .header("Authorization", format!("Bearer {api_token}"))
            .header("Content-Type", "application/json")
            .query(&params)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                logging::log_warn(&format!("aero-media-customer Lookup Fehler: {e}"));
                continue;
            }
        };
        if !response.status().is_success() {
            logging::log_warn(&format!(
                "aero-media-customer Lookup fehlgeschlagen (HTTP {}) mit fallback={fallback}",
                response.status()
            ));
            continue;
        }
        let payload = match response.json::<Value>().await {
            Ok(v) => v,
            Err(e) => {
                logging::log_warn(&format!("aero-media-customer Lookup Fehler: {e}"));
                continue;
            }
        };
        if let Some(link) = super::extract_link_from_customer_payload(&payload) {
            logging::log_info(&format!("Link über aero-media-customer gefunden: {link}"));
            return Some(link);
        }
    }
    None
}

impl CustomApiClient {
    pub(super) fn apply_order_create_response(&self, data: &Value) {
        if let Some(url) = data
            .get("final_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| extract_customer_url(data))
        {
            self.set_last_customer_url(Some(url));
        }
        if let Some(oid) = data.get("order_id") {
            let as_str = oid
                .as_str()
                .map(str::to_string)
                .or_else(|| oid.as_i64().map(|n| n.to_string()))
                .or_else(|| oid.as_u64().map(|n| n.to_string()));
            if let Some(oid) = as_str {
                self.set_last_order_id(Some(oid));
            }
        }
        logging::log_info(&format!(
            "Manifest v1.1: order_id={:?} final_url={:?} status={:?}",
            self.last_order_id(),
            self.last_customer_url(),
            data.get("status")
        ));
    }

    pub(super) async fn wait_for_manifest_status(
        &self,
        order_id: &str,
        control: &UploadControl,
    ) -> Result<(), CloudError> {
        if order_id.is_empty() {
            return Ok(());
        }
        let url = format!("{}/api/orders/{order_id}/manifest-status", self.origin()?);
        let started = Instant::now();
        logging::log_info(&format!(
            "Warte auf Manifest-Verknüpfung (order_id={order_id})..."
        ));
        let max = Duration::from_secs(if cfg!(test) {
            0
        } else {
            MANIFEST_STATUS_POLL_MAX_SECS
        });
        let interval = Duration::from_secs(if cfg!(test) {
            0
        } else {
            MANIFEST_STATUS_POLL_INTERVAL_SECS
        });

        while started.elapsed() < max || max.is_zero() {
            control.wait_if_paused().await?;
            match self.get_json(&url, Duration::from_secs(15), control).await {
                Ok(response) if response.status().is_success() => {
                    if let Ok(data) = response.json::<Value>().await {
                        let status = data.get("status").and_then(Value::as_str).unwrap_or("");
                        if status == "completed" {
                            logging::log_info(&format!(
                                "Manifest-Verknüpfung abgeschlossen (order_id={order_id})."
                            ));
                            self.apply_order_create_response(&data);
                            return Ok(());
                        }
                        if status == "failed" {
                            let error_msg = data
                                .get("error")
                                .and_then(Value::as_str)
                                .or_else(|| data.get("message").and_then(Value::as_str))
                                .unwrap_or("failed");
                            return Err(CloudError::Message(format!(
                                "manifest-status: failed — {error_msg}"
                            )));
                        }
                        events::emit_status(format!(
                            "Verknüpfe Dateien in Cloud... ({})",
                            if status.is_empty() {
                                "processing"
                            } else {
                                status
                            }
                        ));
                    }
                }
                Ok(response) => {
                    logging::log_debug(&format!(
                        "manifest-status: HTTP {} (order_id={order_id})",
                        response.status()
                    ));
                }
                Err(e) if e.is_cancelled() => return Err(e),
                Err(e) => logging::log_debug(&format!("manifest-status Poll: {e}")),
            }
            if max.is_zero() {
                break;
            }
            tokio::time::sleep(interval).await;
        }
        logging::log_warn(&format!(
            "manifest-status: Timeout nach {}s (order_id={order_id}) — final_url bleibt gültig.",
            MANIFEST_STATUS_POLL_MAX_SECS
        ));
        Ok(())
    }

    pub(super) async fn submit_manifest_v11(
        &self,
        manifest: &Value,
        known_order_id: Option<&str>,
        known_final_url: Option<&str>,
        control: &UploadControl,
        mut on_checkpoint: impl FnMut(Value) -> Result<(), CloudError>,
    ) -> Result<(), CloudError> {
        let totals = manifest.get("totals").cloned().unwrap_or(json!({}));
        let files_count = totals
            .get("files_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if files_count == 0 {
            return Err(CloudError::Message(
                "Manifest v1.1: keine Dateien in Standard-Kategorien.".into(),
            ));
        }
        let meta = manifest.get("meta").cloned().unwrap_or(json!({}));
        logging::log_info(&format!(
            "Sende Manifest v1.1 ({}, {}): {} Dateien, {} Bytes",
            meta.get("version").and_then(Value::as_str).unwrap_or(""),
            meta.get("link_mode").and_then(Value::as_str).unwrap_or(""),
            files_count,
            totals
                .get("bytes_total")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        ));
        on_checkpoint(json!({ "phase": "manifest_pending" }))?;
        control.wait_if_paused().await?;

        if let Some(order_id) = known_order_id.filter(|s| !s.is_empty()) {
            self.set_last_order_id(Some(order_id.to_string()));
            if let Some(url) = known_final_url.filter(|s| !s.is_empty()) {
                self.set_last_customer_url(Some(url.to_string()));
            }
            logging::log_info(&format!(
                "Checkpoint: Order {order_id} bereits angelegt — kein POST-Retry, warte auf Manifest."
            ));
            events::emit_status("Verknüpfe Dateien in Cloud...");
            self.wait_for_manifest_status(order_id, control).await?;
            return Ok(());
        }

        events::emit_status("Registriere Order bei Cloud...");
        let url = format!("{}/api/orders/create", self.origin()?);
        let response = self
            .post_json_url(
                &url,
                manifest,
                Duration::from_secs(ORDERS_CREATE_TIMEOUT_SECS),
                "orders/create",
                &[],
                control,
                false,
            )
            .await?;
        let http_status = response.status().as_u16();
        let data = response.json::<Value>().await.unwrap_or(json!({}));
        if data.get("ok").and_then(Value::as_bool) == Some(false) {
            let error_msg = data
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Unbekannter Fehler");
            let error_code = data.get("error_code").and_then(Value::as_str).unwrap_or("");
            return Err(CloudError::Message(if error_code.is_empty() {
                format!("orders/create: {error_msg}")
            } else {
                format!("orders/create: {error_msg} ({error_code})")
            }));
        }
        self.apply_order_create_response(&data);
        if self.last_customer_url().is_none() {
            logging::log_warn("orders/create: keine final_url in Antwort.");
        }
        on_checkpoint(json!({
            "phase": "manifest_pending",
            "order_id": self.last_order_id(),
            "final_url": self.last_customer_url(),
        }))?;

        let status = data.get("status").and_then(Value::as_str);
        if http_status == 202 || status == Some("processing") {
            logging::log_info(&format!(
                "orders/create: HTTP {http_status}, status={status:?} — Datei-Verknüpfung läuft im Hintergrund."
            ));
            if let Some(order_id) = self.last_order_id() {
                self.wait_for_manifest_status(&order_id, control).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customer_lookup_url_and_params_match_legacy() {
        let query = ApiMarkerQuery {
            customer_id: "c1".into(),
            booking_id: "b2".into(),
            marker_type: "Outside".into(),
        };
        assert_eq!(
            customer_lookup_url("https://api.example", LookupMode::Hash),
            "https://api.example/aero-media-customer"
        );
        assert_eq!(
            customer_lookup_url("https://api.example/", LookupMode::Id),
            "https://api.example/aero-media-customer-fallback"
        );
        let hash = customer_lookup_params(&query, LookupMode::Hash);
        assert_eq!(hash.len(), 3);
        let id = customer_lookup_params(&query, LookupMode::Id);
        assert!(id.iter().any(|(k, v)| k == "Fallback" && v == "true"));
    }

    #[tokio::test]
    async fn fetch_without_credentials_errors() {
        let _ = crate::storage::secrets::save_secret("aero_customer_base_url", "");
        let _ = crate::storage::secrets::save_secret("aero_customer_api_token", "");
        let query = ApiMarkerQuery {
            customer_id: "c1".into(),
            booking_id: "b2".into(),
            marker_type: "Outside".into(),
        };
        let err = fetch_customer_payload(&query, LookupMode::Hash)
            .await
            .unwrap_err();
        assert!(err.contains("API-Credentials fehlen"));
    }
}
