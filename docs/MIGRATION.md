# Legacy → v2 Migration Mapping

> Kurzreferenz. Vollständiger Plan: [IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md)

## Legacy-Basis-Pfad

```
C:\Users\Kowalenko\PycharmProjects\AeroMediaService
```

## Status-Legende

- ⬜ Offen
- 🔄 In Arbeit
- ✅ Erledigt
- ⏭️ Nicht portieren

---

## Core / Upload

| Status | Legacy | v2 (Rust) |
|--------|--------|-----------|
| ✅ | `core/monitor.py` | `src-tauri/src/monitor/service.rs` |
| ✅ | `core/folder_stability.py` | `src-tauri/src/monitor/stability.rs` |
| ✅ | `core/upload_markers.py` | `src-tauri/src/model/marker.rs` |
| ✅ | `core/uploader.py` | `src-tauri/src/upload/worker.rs` |
| ✅ | `core/upload_control.py` | `src-tauri/src/upload/control.rs` |
| ✅ | `core/upload_queue_registry.py` | `src-tauri/src/upload/registry.rs` |
| ✅ | `utils/upload_checkpoint.py` | `src-tauri/src/upload/checkpoint.rs` |
| ✅ | `core/archive.py` | `src-tauri/src/util/archive.rs` |
| ✅ | `core/retry_upload.py` | `src-tauri/src/upload/retry.rs` |
| ✅ | `core/history_status.py` | `src-tauri/src/model/history_status.rs` |
| ✅ | `core/manual_status.py` | `src-tauri/src/model/manual_status.rs` |
| ✅ | `core/resend_notifications.py` | `src-tauri/src/notify/resend.rs` |
| ✅ | `core/sms_history_sync.py` | `src-tauri/src/notify/sms_sync.rs` |
| ✅ | `core/signals.py` | Tauri Events |
| ✅ | `core/config.py` | `src-tauri/src/storage/config.rs` + `secrets.rs` |
| ✅ | QSettings `AKSoftware`/`AeroMediaService` + Keyring `DropboxUploaderApp` | `src-tauri/src/storage/legacy_migrate.rs` (Phase 11) |
| ✅ | `core/logger.py` | `src-tauri/src/storage/logging.rs` |

## Cloud & Notify

| Status | Legacy | v2 (Rust) |
|--------|--------|-----------|
| ✅ | `services/base_client.py` | `src-tauri/src/cloud/traits.rs` |
| ✅ | `services/dropbox_client.py` | `src-tauri/src/cloud/dropbox.rs` + `oauth.rs` (OAuth/PKCE Phase 9) |
| ✅ | `services/custom_api_client.py` | `src-tauri/src/cloud/custom_api.rs` (+ Untermodule) |
| ✅ | `utils/dropbox_manifest.py` | `src-tauri/src/cloud/manifest.rs` |
| ✅ | `services/email_client.py` | `src-tauri/src/notify/email.rs` |
| ✅ | `services/sms_client.py` | `src-tauri/src/notify/sms.rs` |
| ✅ | `services/whatsapp_client.py` | `src-tauri/src/notify/whatsapp.rs` |
| ✅ | `services/message_client.py` | `src-tauri/src/notify/message.rs` |
| ✅ | `utils/link_shortener.py` | `src-tauri/src/util/link_shortener.rs` |

## Domain

| Status | Legacy | v2 (Rust) |
|--------|--------|-----------|
| ✅ | `models/kunde.py` | `src-tauri/src/model/kunde.rs` |
| ✅ | `utils/validation.py` | `src-tauri/src/model/validation.rs` |
| ✅ | `utils/history_manager.py` | `src-tauri/src/storage/history.rs` (SQLite) |
| ⬜ | `utils/constants.py` | `src-tauri/src/constants.rs` |
| ⬜ | `utils/path_helper.py` | Tauri `path` API / util |

## GUI → React

| Status | Legacy | v2 (React) |
|--------|--------|------------|
| ✅ | `main.py` | Tauri entry + `src/main.tsx` |
| ✅ | `app.py` (MainWindow) | `src/App.tsx` + Layout / Shell (Phase 9) + AppChrome (Phase 11) |
| ✅ | — (First-Run) | `src/components/SetupWizard.tsx` (Phase 11) |
| ✅ | `app.py` StatusLight | `src/components/StatusLight.tsx` |
| ✅ | `app.py` Monitor-Log | `src/components/MonitorLog.tsx` |
| ✅ | `app.py` History | `src/components/HistoryTable.tsx` (+ VirtualList Phase 11) |
| ✅ | `app.py` Resend-Dialog | `src/components/ResendNotificationsDialog.tsx` |
| ✅ | `settings.py` | `src/components/SettingsDialog.tsx` |
| ✅ | `utils/loading_overlay.py` | `src/components/LoadingOverlay.tsx` |
| ✅ | `utils/updater.py` | Tauri Updater + `UpdateDialog.tsx` + Settings Extras |
| ✅ | Fertig App (Companion) | `CustomersPanel` + `storage/customers.rs` (Phase 12) |

## Nicht portieren

| Legacy | Grund |
|--------|-------|
| `_test_*.py` | Als Spec lesen → Rust Unit-Tests neu |
| `build.py` / PyInstaller / NSIS | Tauri Bundler + CI |
| Qt-Worker-Klassen in `app.py` | React + Rust Tasks |
| `__pycache__`, `venv`, `build`, `dist` | — |
