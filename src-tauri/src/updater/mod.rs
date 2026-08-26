//! Auto-update helpers (Tauri updater plugin) + manual version switching.
//!
//! Production feed: public releases repo
//! `a-kowalenko/aero-media-service-releases` → `latest.json`.
//!
//! Signing: `TAURI_SIGNING_PRIVATE_KEY` (+ optional password) in CI; pubkey in
//! `tauri.conf.json` → `plugins.updater.pubkey`.
//!
//! Manual version switch uses the same silent `download_and_install` path as
//! auto-update, pointed at that release's `latest.json` (allows downgrade).

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Runtime};

/// Frontend listens for download/install progress while applying an update.
pub const EVENT_UPDATE_INSTALL_PROGRESS: &str = "update-install-progress";

/// Marker substring that means “stub / not ready” (keep for safety if config regresses).
pub const UPDATER_STUB_MARKER: &str = "releases.example.invalid";

const UPDATER_ENDPOINT: &str =
    "https://github.com/a-kowalenko/aero-media-service-releases/releases/latest/download/latest.json";

const RELEASES_API_URL: &str =
    "https://api.github.com/repos/a-kowalenko/aero-media-service-releases/releases?per_page=100";

const RELEASES_DOWNLOAD_PREFIX: &str =
    "https://github.com/a-kowalenko/aero-media-service-releases/releases/download/";

const RELEASES_REPO: &str = "a-kowalenko/aero-media-service-releases";

/// Oldest version offered in the manual switcher (first public v2 builds).
pub const MIN_SWITCHABLE_VERSION: &str = "0.1.0";

const USER_AGENT: &str = "AeroMediaService-Updater";

const SILENT_INSTALL_UNAVAILABLE: &str =
    "Für diese Version ist die automatische Installation nicht verfügbar.";

static UPDATE_CANCEL_FLAG: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

pub const UPDATE_CANCELLED_MESSAGE: &str = "Download abgebrochen.";

fn clear_update_cancel() {
    UPDATE_CANCEL_FLAG.store(false, Ordering::SeqCst);
}

fn is_update_cancelled() -> bool {
    UPDATE_CANCEL_FLAG.load(Ordering::SeqCst)
}

/// Abort an in-progress update download (install phase cannot be cancelled).
#[tauri::command]
pub fn cancel_update_install() -> bool {
    UPDATE_CANCEL_FLAG.store(true, Ordering::SeqCst);
    true
}

async fn wait_update_cancelled() {
    while !is_update_cancelled() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdaterStatus {
    pub configured: bool,
    pub current_version: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheckResult {
    pub configured: bool,
    pub available: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub body: Option<String>,
    pub message: String,
    pub prerelease: bool,
    /// When set (beta path / specific release), silent install via this manifest URL.
    pub updater_json_url: Option<String>,
    pub installer_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AvailableRelease {
    pub tag_name: String,
    pub published_at: String,
    pub body: String,
    /// Public installer asset (DMG/EXE/AppImage) — escape hatch only, never auto-launched.
    pub installer_url: Option<String>,
    /// Signed updater manifest for silent install (`latest.json` on the release).
    pub updater_json_url: Option<String>,
    pub prerelease: bool,
}

fn updater_endpoints() -> Vec<String> {
    // Keep in sync with tauri.conf.json → plugins.updater.endpoints
    vec![UPDATER_ENDPOINT.to_string()]
}

pub fn is_updater_configured() -> bool {
    let endpoints = updater_endpoints();
    if endpoints.is_empty() {
        return false;
    }
    !endpoints.iter().any(|e| e.contains(UPDATER_STUB_MARKER))
}

#[tauri::command]
pub fn get_updater_status(app: AppHandle) -> UpdaterStatus {
    let current_version = app.package_info().version.to_string();
    if is_updater_configured() {
        UpdaterStatus {
            configured: true,
            current_version,
            message: "Update-Prüfung über GitHub Releases möglich.".into(),
        }
    } else {
        UpdaterStatus {
            configured: false,
            current_version,
            message: "Auto-Update ist derzeit nicht verfügbar.".into(),
        }
    }
}

/// Optional UX hint when silent replace is unreliable from the current install location.
#[tauri::command]
pub fn get_updater_install_hint() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(exe) = std::env::current_exe() {
            let path = exe.to_string_lossy();
            if !path.contains("/Applications/") {
                return Some(
                    "Für automatische Updates sollte die App im Ordner „Programme“ liegen.".into(),
                );
            }
        }
        None
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("APPIMAGE").is_none() {
            Some("Automatische Updates funktionieren zuverlässig nur als AppImage.".into())
        } else {
            None
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Check for updates. When stub config is active, returns `configured: false`
/// without contacting the network. Stable checks use the updater plugin; beta
/// checks use the GitHub releases API (prereleases are excluded from `/latest/`).
#[tauri::command]
pub async fn check_for_updates<R: Runtime>(
    app: AppHandle<R>,
    include_beta: bool,
) -> Result<UpdateCheckResult, String> {
    let current_version = app.package_info().version.to_string();

    if !is_updater_configured() {
        return Ok(unavailable_update_result(current_version));
    }

    #[cfg(desktop)]
    {
        if include_beta {
            return check_for_updates_via_releases_api(&current_version, true).await;
        }

        use tauri_plugin_updater::UpdaterExt;
        let updater = app
            .updater_builder()
            .build()
            .map_err(|e| format!("Updater konnte nicht initialisiert werden: {e}"))?;
        match updater.check().await {
            Ok(Some(update)) => {
                // Always from GitHub Release body (editable after publish), never latest.json notes.
                let body = fetch_release_notes(&update.version).await;
                Ok(UpdateCheckResult {
                    configured: true,
                    available: true,
                    current_version,
                    latest_version: Some(update.version.clone()),
                    body,
                    message: format!("Update verfügbar: {}", update.version),
                    prerelease: false,
                    updater_json_url: None,
                    installer_url: None,
                })
            }
            Ok(None) => Ok(UpdateCheckResult {
                configured: true,
                available: false,
                current_version: current_version.clone(),
                latest_version: None,
                body: None,
                message: format!("Sie haben bereits die neueste Version ({current_version})."),
                prerelease: false,
                updater_json_url: None,
                installer_url: None,
            }),
            Err(e) => Err(format!("Update-Prüfung fehlgeschlagen: {e}")),
        }
    }

    #[cfg(not(desktop))]
    {
        let _ = app;
        Ok(UpdateCheckResult {
            configured: false,
            available: false,
            current_version,
            latest_version: None,
            body: None,
            message: "Updater nur auf Desktop verfügbar.".into(),
            prerelease: false,
            updater_json_url: None,
            installer_url: None,
        })
    }
}

fn unavailable_update_result(current_version: String) -> UpdateCheckResult {
    UpdateCheckResult {
        configured: false,
        available: false,
        current_version,
        latest_version: None,
        body: None,
        message: "Auto-Update ist derzeit nicht verfügbar.".into(),
        prerelease: false,
        updater_json_url: None,
        installer_url: None,
    }
}

async fn check_for_updates_via_releases_api(
    current_version: &str,
    include_beta: bool,
) -> Result<UpdateCheckResult, String> {
    let releases = list_available_versions().await?;
    if let Some(candidate) = resolve_best_update(&releases, current_version, include_beta) {
        let body = nonempty_notes(Some(&candidate.body))
            .or(fetch_release_notes(&candidate.tag_name).await);
        let beta_suffix = if candidate.prerelease || version_has_prerelease(&candidate.tag_name) {
            " (Beta)"
        } else {
            ""
        };
        return Ok(UpdateCheckResult {
            configured: true,
            available: true,
            current_version: current_version.to_string(),
            latest_version: Some(candidate.tag_name.clone()),
            body,
            message: format!("Update verfügbar: {}{}", candidate.tag_name, beta_suffix),
            prerelease: candidate.prerelease || version_has_prerelease(&candidate.tag_name),
            updater_json_url: candidate.updater_json_url.clone(),
            installer_url: candidate.installer_url.clone(),
        });
    }

    Ok(UpdateCheckResult {
        configured: true,
        available: false,
        current_version: current_version.to_string(),
        latest_version: None,
        body: None,
        message: format!("Sie haben bereits die neueste Version ({current_version})."),
        prerelease: false,
        updater_json_url: None,
        installer_url: None,
    })
}

fn is_installable_release(release: &AvailableRelease) -> bool {
    release.updater_json_url.is_some() || release.installer_url.is_some()
}

/// Newest installable release newer than `current`. `releases` must be sorted desc.
fn resolve_best_update<'a>(
    releases: &'a [AvailableRelease],
    current: &str,
    include_beta: bool,
) -> Option<&'a AvailableRelease> {
    releases.iter().find(|release| {
        is_installable_release(release)
            && (include_beta || !release_is_prerelease(release))
            && compare_semver(&release.tag_name, current) == std::cmp::Ordering::Greater
    })
}

/// GitHub prerelease flag **or** SemVer prerelease in the tag (`-beta`, `-rc`, …).
fn release_is_prerelease(release: &AvailableRelease) -> bool {
    release.prerelease || version_has_prerelease(&release.tag_name)
}

fn version_has_prerelease(version: &str) -> bool {
    normalize_tag(version).contains('-')
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstallProgress {
    /// `"download"` while bytes arrive, `"install"` after download finishes.
    pub phase: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: f64,
    /// Approximate download throughput in bytes/sec (0 while installing).
    pub speed_bps: f64,
}

fn emit_update_progress<R: Runtime>(app: &AppHandle<R>, progress: &UpdateInstallProgress) {
    let _ = app.emit(EVENT_UPDATE_INSTALL_PROGRESS, progress);
}

#[cfg(desktop)]
async fn download_and_install_update<R: Runtime>(
    app: &AppHandle<R>,
    update: tauri_plugin_updater::Update,
) -> Result<String, String> {
    clear_update_cancel();

    let version = update.version.clone();
    let app_for_progress = app.clone();
    let downloaded = Arc::new(AtomicU64::new(0));
    let started = Instant::now();

    emit_update_progress(
        app,
        &UpdateInstallProgress {
            phase: "download".into(),
            downloaded_bytes: 0,
            total_bytes: None,
            percent: 0.0,
            speed_bps: 0.0,
        },
    );

    let downloaded_cb = Arc::clone(&downloaded);
    let download_fut = update.download(
        move |chunk, total| {
            if is_update_cancelled() {
                return;
            }
            let done = downloaded_cb.fetch_add(chunk as u64, Ordering::Relaxed) + chunk as u64;
            let elapsed = started.elapsed().as_secs_f64().max(0.001);
            let speed_bps = done as f64 / elapsed;
            let percent = match total {
                Some(t) if t > 0 => (done as f64 / t as f64 * 100.0).clamp(0.0, 100.0),
                _ => 0.0,
            };
            emit_update_progress(
                &app_for_progress,
                &UpdateInstallProgress {
                    phase: "download".into(),
                    downloaded_bytes: done,
                    total_bytes: total,
                    percent,
                    speed_bps,
                },
            );
        },
        || {},
    );
    tokio::pin!(download_fut);

    let bytes = tokio::select! {
        biased;
        _ = wait_update_cancelled() => {
            return Err(UPDATE_CANCELLED_MESSAGE.into());
        }
        result = &mut download_fut => {
            result.map_err(|e| {
                if is_update_cancelled() {
                    UPDATE_CANCELLED_MESSAGE.to_string()
                } else {
                    format!("Update-Download fehlgeschlagen: {e}")
                }
            })?
        }
    };

    if is_update_cancelled() {
        return Err(UPDATE_CANCELLED_MESSAGE.into());
    }

    let done = downloaded.load(Ordering::Relaxed);
    emit_update_progress(
        app,
        &UpdateInstallProgress {
            phase: "install".into(),
            downloaded_bytes: done,
            total_bytes: Some(done).filter(|&n| n > 0),
            percent: 100.0,
            speed_bps: 0.0,
        },
    );

    update
        .install(bytes)
        .map_err(|e| format!("Update-Installation fehlgeschlagen: {e}"))?;

    Ok(format!(
        "Version {version} installiert — App wird neu gestartet."
    ))
}

/// Download + install a pending update (plugin API) with progress events.
#[tauri::command]
pub async fn install_update<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    if !is_updater_configured() {
        return Err("Update-Installation ist derzeit nicht möglich.".into());
    }

    #[cfg(desktop)]
    {
        use tauri_plugin_updater::UpdaterExt;
        let updater = app
            .updater_builder()
            .build()
            .map_err(|e| format!("Updater konnte nicht initialisiert werden: {e}"))?;
        let update = updater
            .check()
            .await
            .map_err(|e| format!("Update-Prüfung fehlgeschlagen: {e}"))?
            .ok_or_else(|| "Kein Update verfügbar.".to_string())?;

        download_and_install_update(&app, update).await
    }

    #[cfg(not(desktop))]
    {
        let _ = app;
        Err("Updater nur auf Desktop verfügbar.".into())
    }
}

/// Shown in Settings when the releases API cannot be reached (offline / DNS / timeout).
pub const RELEASES_OFFLINE_MESSAGE: &str =
    "Versionsliste nicht verfügbar — bitte Internetverbindung prüfen.";

const RELEASES_UNAVAILABLE_MESSAGE: &str = "Versionsliste vorübergehend nicht verfügbar.";

fn is_network_error_message(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("dns")
        || lower.contains("resolve")
        || lower.contains("lookup")
        || lower.contains("connection")
        || lower.contains("network")
        || lower.contains("offline")
        || lower.contains("unreachable")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("failed to connect")
        || lower.contains("error sending request")
}

/// Map reqwest failures to a short, user-facing German message (no raw stack/DNS text).
fn map_releases_fetch_error(err: &reqwest::Error) -> String {
    if err.is_connect()
        || err.is_timeout()
        || err.is_request()
        || is_network_error_message(&err.to_string())
    {
        RELEASES_OFFLINE_MESSAGE.into()
    } else {
        RELEASES_UNAVAILABLE_MESSAGE.into()
    }
}

/// List published releases that include a platform installer and/or updater manifest.
#[tauri::command]
pub async fn list_available_versions() -> Result<Vec<AvailableRelease>, String> {
    let client = http_client()?;
    let response = client
        .get(RELEASES_API_URL)
        .send()
        .await
        .map_err(|e| map_releases_fetch_error(&e))?;

    if !response.status().is_success() {
        return Err(RELEASES_UNAVAILABLE_MESSAGE.into());
    }

    let payload: Vec<GitHubRelease> = response
        .json()
        .await
        .map_err(|_| RELEASES_UNAVAILABLE_MESSAGE.to_string())?;

    let mut releases = Vec::new();
    for release in payload {
        if release.draft {
            continue;
        }
        let tag = normalize_tag(&release.tag_name);
        if tag.is_empty() {
            continue;
        }
        if !version_at_least(&tag, MIN_SWITCHABLE_VERSION) {
            continue;
        }
        let installer_url = pick_installer_url(&release.assets);
        let updater_json_url = pick_updater_json_url(&release.assets);
        if installer_url.is_none() && updater_json_url.is_none() {
            continue;
        }
        releases.push(AvailableRelease {
            tag_name: tag.clone(),
            published_at: release.published_at.unwrap_or_default(),
            body: release
                .body
                .unwrap_or_else(|| "Keine Details verfügbar.".into()),
            installer_url,
            updater_json_url,
            prerelease: release.prerelease || version_has_prerelease(&tag),
        });
    }

    releases.sort_by(|a, b| compare_versions_desc(&a.tag_name, &b.tag_name));
    Ok(releases)
}

fn is_allowed_updater_json_url(url: &str) -> bool {
    if url.starts_with(RELEASES_DOWNLOAD_PREFIX) && url.ends_with("/latest.json") {
        return true;
    }
    // GitHub may redirect asset downloads through objects.githubusercontent.com
    url.starts_with("https://objects.githubusercontent.com/") && url.contains("latest.json")
}

/// Silent install of a specific release via its updater manifest (upgrade or downgrade).
#[tauri::command]
pub async fn install_specific_version<R: Runtime>(
    app: AppHandle<R>,
    updater_json_url: String,
) -> Result<String, String> {
    if !is_updater_configured() {
        return Err("Update-Installation ist derzeit nicht möglich.".into());
    }
    let url = updater_json_url.trim();
    if url.is_empty() {
        return Err(SILENT_INSTALL_UNAVAILABLE.into());
    }
    if !is_allowed_updater_json_url(url) {
        return Err("Updater-URL ist nicht erlaubt.".into());
    }

    #[cfg(desktop)]
    {
        use tauri_plugin_updater::UpdaterExt;
        use url::Url;

        let endpoint = Url::parse(url).map_err(|e| format!("Ungültige Updater-URL: {e}"))?;
        let updater = app
            .updater_builder()
            .endpoints(vec![endpoint])
            .map_err(|e| format!("Updater-Endpunkt ungültig: {e}"))?
            .version_comparator(|current, update| update.version != current)
            .build()
            .map_err(|e| format!("Updater konnte nicht initialisiert werden: {e}"))?;

        let update = updater
            .check()
            .await
            .map_err(|e| format!("Versionsprüfung fehlgeschlagen: {e}"))?
            .ok_or_else(|| {
                "Keine andere Version zum Installieren gefunden (bereits aktiv?).".to_string()
            })?;

        download_and_install_update(&app, update).await
    }

    #[cfg(not(desktop))]
    {
        let _ = app;
        Err("Updater nur auf Desktop verfügbar.".into())
    }
}

fn nonempty_notes(notes: Option<&str>) -> Option<String> {
    notes
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Fall back to the GitHub release body when `latest.json` notes are empty.
async fn fetch_release_notes(version: &str) -> Option<String> {
    let tag = if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    };
    let url = format!("https://api.github.com/repos/{RELEASES_REPO}/releases/tags/{tag}");
    let client = http_client().ok()?;
    let response = client.get(&url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let release: GitHubRelease = response.json().await.ok()?;
    nonempty_notes(release.body.as_deref())
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| format!("HTTP-Client konnte nicht erstellt werden: {e}"))
}

fn normalize_tag(tag: &str) -> String {
    tag.trim().trim_start_matches('v').to_string()
}

/// SemVer: `candidate >= minimum`.
fn version_at_least(candidate: &str, minimum: &str) -> bool {
    compare_semver(candidate, minimum) != std::cmp::Ordering::Less
}

fn compare_versions_desc(a: &str, b: &str) -> std::cmp::Ordering {
    compare_semver(a, b).reverse()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemVerParts {
    major: u64,
    minor: u64,
    patch: u64,
    /// `None` = release (no prerelease). Identifiers are numeric or alphanumeric.
    prerelease: Option<Vec<PreId>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreId {
    Num(u64),
    Text(String),
}

fn parse_semver(input: &str) -> SemVerParts {
    let v = normalize_tag(input);
    // Strip build metadata (+…)
    let v = v.split_once('+').map(|(c, _)| c).unwrap_or(v.as_str());
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let mut nums = core.split('.').filter_map(|s| s.parse::<u64>().ok());
    let major = nums.next().unwrap_or(0);
    let minor = nums.next().unwrap_or(0);
    let patch = nums.next().unwrap_or(0);
    let prerelease = pre.map(|p| {
        p.split('.')
            .map(|id| {
                if id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty() {
                    PreId::Num(id.parse().unwrap_or(0))
                } else {
                    PreId::Text(id.to_string())
                }
            })
            .collect()
    });
    SemVerParts {
        major,
        minor,
        patch,
        prerelease,
    }
}

fn compare_pre_id(a: &PreId, b: &PreId) -> std::cmp::Ordering {
    match (a, b) {
        (PreId::Num(x), PreId::Num(y)) => x.cmp(y),
        (PreId::Num(_), PreId::Text(_)) => std::cmp::Ordering::Less,
        (PreId::Text(_), PreId::Num(_)) => std::cmp::Ordering::Greater,
        (PreId::Text(x), PreId::Text(y)) => x.cmp(y),
    }
}

fn compare_prerelease(a: &Option<Vec<PreId>>, b: &Option<Vec<PreId>>) -> std::cmp::Ordering {
    // Release (None) > any prerelease
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(left), Some(right)) => {
            let len = left.len().max(right.len());
            for i in 0..len {
                match (left.get(i), right.get(i)) {
                    (None, Some(_)) => return std::cmp::Ordering::Less,
                    (Some(_), None) => return std::cmp::Ordering::Greater,
                    (Some(x), Some(y)) => {
                        let c = compare_pre_id(x, y);
                        if c != std::cmp::Ordering::Equal {
                            return c;
                        }
                    }
                    (None, None) => break,
                }
            }
            std::cmp::Ordering::Equal
        }
    }
}

fn compare_semver(a: &str, b: &str) -> std::cmp::Ordering {
    let left = parse_semver(a);
    let right = parse_semver(b);
    left.major
        .cmp(&right.major)
        .then(left.minor.cmp(&right.minor))
        .then(left.patch.cmp(&right.patch))
        .then(compare_prerelease(&left.prerelease, &right.prerelease))
}

fn pick_updater_json_url(assets: &[GitHubAsset]) -> Option<String> {
    assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("latest.json"))
        .map(|a| a.browser_download_url.clone())
}

fn pick_installer_url(assets: &[GitHubAsset]) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        assets
            .iter()
            .find(|a| {
                let n = a.name.to_lowercase();
                n.ends_with("-setup.exe") && !n.ends_with(".sig")
            })
            .or_else(|| {
                assets.iter().find(|a| {
                    let n = a.name.to_lowercase();
                    n.ends_with(".exe") && !n.ends_with(".sig")
                })
            })
            .map(|a| a.browser_download_url.clone())
    }
    #[cfg(target_os = "macos")]
    {
        let prefer_arch = if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else {
            "x64"
        };
        assets
            .iter()
            .find(|a| {
                let n = a.name.to_lowercase();
                n.ends_with(".dmg") && n.contains(prefer_arch) && !n.ends_with(".sig")
            })
            .or_else(|| {
                assets.iter().find(|a| {
                    let n = a.name.to_lowercase();
                    n.ends_with(".dmg") && !n.ends_with(".sig")
                })
            })
            .map(|a| a.browser_download_url.clone())
    }
    #[cfg(target_os = "linux")]
    {
        let prefer_arch = |n: &str| {
            n.contains("amd64")
                || n.contains("x86_64")
                || n.contains("x86-64")
                || n.contains("_x64")
                || n.contains("-x64")
        };
        assets
            .iter()
            .find(|a| {
                let n = a.name.to_lowercase();
                n.ends_with(".appimage") && prefer_arch(&n) && !n.ends_with(".sig")
            })
            .or_else(|| {
                assets.iter().find(|a| {
                    let n = a.name.to_lowercase();
                    n.ends_with(".appimage") && !n.ends_with(".sig")
                })
            })
            .map(|a| a.browser_download_url.clone())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = assets;
        None
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updater_is_configured_for_releases_repo() {
        assert!(is_updater_configured());
        assert!(updater_endpoints()
            .iter()
            .any(|e| e.contains("aero-media-service-releases")));
        assert_eq!(
            EVENT_UPDATE_INSTALL_PROGRESS,
            crate::events::UPDATE_INSTALL_PROGRESS
        );
    }

    #[test]
    fn releases_fetch_error_maps_network_keywords() {
        assert!(is_network_error_message(
            "error sending request for url (https://example): dns error: failed to lookup address information"
        ));
        assert!(is_network_error_message("connection timed out"));
        assert!(!is_network_error_message("unexpected parser failure"));
    }

    #[test]
    fn version_compare_and_filter() {
        assert!(version_at_least("0.1.3", "0.1.0"));
        assert!(version_at_least("0.1.0", "0.1.0"));
        assert!(!version_at_least("0.0.9", "0.1.0"));
        assert_eq!(
            compare_versions_desc("0.1.3", "0.1.2"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn semver_prerelease_ordering() {
        assert_eq!(
            compare_semver("0.1.14-beta.1", "0.1.14"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_semver("0.1.14", "0.1.14-beta.1"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_semver("0.1.14-beta.1", "0.1.14-beta.2"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_semver("0.1.13", "0.1.14-beta.1"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_semver("v0.1.14-beta.1", "0.1.14-beta.1"),
            std::cmp::Ordering::Equal
        );
        assert!(version_has_prerelease("0.1.14-beta.1"));
        assert!(!version_has_prerelease("0.1.14"));
    }

    #[test]
    fn normalize_strips_v_prefix() {
        assert_eq!(normalize_tag("v0.1.3"), "0.1.3");
        assert_eq!(normalize_tag("0.1.3"), "0.1.3");
    }

    #[test]
    fn allowed_updater_json_urls() {
        assert!(is_allowed_updater_json_url(
            "https://github.com/a-kowalenko/aero-media-service-releases/releases/download/v0.1.0/latest.json"
        ));
        assert!(!is_allowed_updater_json_url(
            "https://evil.example/latest.json"
        ));
        assert!(!is_allowed_updater_json_url(
            "https://github.com/a-kowalenko/aero-media-service-releases/releases/download/v0.1.0/setup.exe"
        ));
    }

    #[test]
    fn pick_updater_json() {
        let assets = vec![
            GitHubAsset {
                name: "latest.json".into(),
                browser_download_url: "https://example/latest.json".into(),
            },
            GitHubAsset {
                name: "setup.exe".into(),
                browser_download_url: "https://example/setup".into(),
            },
        ];
        assert_eq!(
            pick_updater_json_url(&assets).as_deref(),
            Some("https://example/latest.json")
        );
        assert!(pick_updater_json_url(&[]).is_none());
    }

    #[test]
    fn pick_windows_setup_prefers_nsis() {
        let assets = vec![
            GitHubAsset {
                name: "Aero.Media.Service_0.1.0_x64_en-US.msi".into(),
                browser_download_url: "https://example/msi".into(),
            },
            GitHubAsset {
                name: "Aero.Media.Service_0.1.0_x64-setup.exe".into(),
                browser_download_url: "https://example/setup".into(),
            },
            GitHubAsset {
                name: "Aero.Media.Service_0.1.0_x64-setup.exe.sig".into(),
                browser_download_url: "https://example/sig".into(),
            },
            GitHubAsset {
                name: "Aero.Media.Service_0.1.0_amd64.AppImage".into(),
                browser_download_url: "https://example/appimage".into(),
            },
        ];
        #[cfg(target_os = "windows")]
        {
            assert_eq!(
                pick_installer_url(&assets).as_deref(),
                Some("https://example/setup")
            );
        }
        #[cfg(target_os = "macos")]
        {
            let _ = &assets;
            assert!(pick_installer_url(&[]).is_none());
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                pick_installer_url(&assets).as_deref(),
                Some("https://example/appimage")
            );
        }
    }

    #[test]
    fn pick_linux_appimage_prefers_arch_token() {
        let assets = vec![
            GitHubAsset {
                name: "Aero.Media.Service_0.1.0_aarch64.AppImage".into(),
                browser_download_url: "https://example/arm".into(),
            },
            GitHubAsset {
                name: "Aero.Media.Service_0.1.0_amd64.AppImage".into(),
                browser_download_url: "https://example/amd64".into(),
            },
            GitHubAsset {
                name: "Aero.Media.Service_0.1.0_amd64.AppImage.sig".into(),
                browser_download_url: "https://example/sig".into(),
            },
        ];
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                pick_installer_url(&assets).as_deref(),
                Some("https://example/amd64")
            );
        }
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            // Win/Mac must ignore Linux AppImage assets when no native installer present.
            assert!(pick_installer_url(&assets).is_none());
        }
    }

    #[test]
    fn nonempty_notes_trims_and_rejects_blank() {
        assert_eq!(nonempty_notes(Some("  hello  ")).as_deref(), Some("hello"));
        assert!(nonempty_notes(Some("   ")).is_none());
        assert!(nonempty_notes(Some("")).is_none());
        assert!(nonempty_notes(None).is_none());
    }

    fn sample_release(tag: &str, prerelease: bool, has_updater: bool) -> AvailableRelease {
        AvailableRelease {
            tag_name: tag.into(),
            published_at: String::new(),
            body: String::new(),
            installer_url: None,
            updater_json_url: has_updater.then(|| {
                format!(
                    "https://github.com/a-kowalenko/aero-media-service-releases/releases/download/v{tag}/latest.json"
                )
            }),
            prerelease,
        }
    }

    #[test]
    fn resolve_best_update_respects_beta_flag() {
        let releases = vec![
            sample_release("0.3.2", true, true),
            sample_release("0.3.1", false, true),
            sample_release("0.3.0", false, true),
        ];
        assert_eq!(
            resolve_best_update(&releases, "0.3.0", false).map(|r| r.tag_name.as_str()),
            Some("0.3.1")
        );
        assert_eq!(
            resolve_best_update(&releases, "0.3.0", true).map(|r| r.tag_name.as_str()),
            Some("0.3.2")
        );
        assert!(resolve_best_update(&releases, "0.3.2", false).is_none());
        assert!(resolve_best_update(&releases, "0.3.2", true).is_none());
    }

    #[test]
    fn resolve_best_update_offers_beta_from_stable() {
        let releases = vec![
            sample_release("0.1.14-beta.1", true, true),
            sample_release("0.1.13", false, true),
        ];
        assert!(resolve_best_update(&releases, "0.1.13", false).is_none());
        assert_eq!(
            resolve_best_update(&releases, "0.1.13", true).map(|r| r.tag_name.as_str()),
            Some("0.1.14-beta.1")
        );
    }

    #[test]
    fn resolve_best_update_prefers_stable_over_older_beta() {
        let releases = vec![
            sample_release("0.1.14", false, true),
            sample_release("0.1.14-beta.2", true, true),
            sample_release("0.1.13", false, true),
        ];
        assert_eq!(
            resolve_best_update(&releases, "0.1.14-beta.2", true).map(|r| r.tag_name.as_str()),
            Some("0.1.14")
        );
        assert_eq!(
            resolve_best_update(&releases, "0.1.13", false).map(|r| r.tag_name.as_str()),
            Some("0.1.14")
        );
    }

    #[test]
    fn resolve_best_update_skips_non_installable() {
        let releases = vec![AvailableRelease {
            tag_name: "9.9.9".into(),
            published_at: String::new(),
            body: String::new(),
            installer_url: None,
            updater_json_url: None,
            prerelease: false,
        }];
        assert!(resolve_best_update(&releases, "0.1.0", true).is_none());
    }

    #[test]
    fn install_hint_is_platform_specific() {
        let hint = get_updater_install_hint();
        #[cfg(windows)]
        assert!(hint.is_none());
        #[cfg(target_os = "macos")]
        {
            // Hint depends on whether the current binary lives under /Applications.
            let _ = hint;
        }
        #[cfg(target_os = "linux")]
        {
            // Outside an AppImage build, a hint is expected.
            if std::env::var_os("APPIMAGE").is_none() {
                assert!(hint.is_some());
            }
        }
    }
}
