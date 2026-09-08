# Aero Media Service v2 — Implementierungsplan

> **Zweck:** Zentrale Leitfaden für die Neuentwicklung.
> In jedem neuen Cursor-/Agent-Kontextfenster mit `@docs/IMPLEMENTATION_PLAN.md` referenzieren.
> Pro Session **nur eine Phase** implementieren.

---

## Inhaltsverzeichnis

1. [Projektübersicht](#1-projektübersicht)
2. [Aktueller Stand](#2-aktueller-stand)
3. [Tech-Stack](#3-tech-stack)
4. [Entwicklungsumgebung](#4-entwicklungsumgebung)
5. [Legacy-Referenz](#5-legacy-referenz)
6. [Vollständiges Datei-Mapping](#6-vollständiges-datei-mapping)
7. [Architektur](#7-architektur)
8. [Phasenplan](#8-phasenplan)
9. [Config-Schema](#9-config-schema)
10. [Teststrategie](#10-teststrategie)
11. [Build & Deployment](#11-build--deployment)
12. [Fortschritts-Tracker](#12-fortschritts-tracker)

---

## 1. Projektübersicht

**Aero Media Service** überwacht Medienordner, lädt Inhalte in die Cloud hoch und benachrichtigt Kunden:

- Ordner-Monitor + Stability-Check
- Marker-Dateien (Kundendaten / Booking-Hashes)
- Upload zu Dropbox oder Custom API
- Share-Links (+ optional Shortener)
- E-Mail (SMTP), SMS (seven.io), WhatsApp (Twilio)
- History, Retry, Resend, manuelle Status-Aktionen
- ATS↔AMS Handoff auf SMB-Share `aktuell` (Phase 13, Spec: [`HANDOFF.md`](./HANDOFF.md))
- Windows + macOS + Linux

### Projektpfade

| | Pfad |
|---|------|
| **Neues Projekt (v2)** | `C:\Users\Kowalenko\PycharmProjects\AeroMediaService-v2` |
| **Legacy (NUR LESEN)** | `C:\Users\Kowalenko\PycharmProjects\AeroMediaService` |
| **ATS-v2 (Vorbild)** | `C:\Users\Kowalenko\PycharmProjects\AeroTandemStudio-v2` |

---

## 2. Aktueller Stand

| Item | Status |
|------|--------|
| Tauri 2 Scaffold (React + TypeScript) | ✅ Erledigt |
| Docs (`AGENTS.md`, Plan, Architektur, Migration) | ✅ Erledigt |
| Minimal-UI (Name + Version) | ✅ Erledigt |
| `npm run tauri dev` | ✅ Erledigt |
| Config / Secrets / Logging / Events | ✅ Phase 1 |
| Marker / Kunde / Validierung | ✅ Phase 2 |
| Monitor / Stability | ✅ Phase 3 |
| Upload / Dropbox | ✅ Phase 4 |
| Checkpoints / Custom API | ✅ Phase 5 |
| Notifications (E-Mail / SMS / WhatsApp) | ✅ Phase 6 |
| History-UI / Statusmodell | ✅ Phase 7 |
| Retry / Resend / Manual Status | ✅ Phase 8 |
| Settings vollständig + App-Shell | ✅ Phase 9 |
| Updater / CI / Plattformen | ✅ Phase 10 |
| Polish (Wizard, Titlebar, History-Virtualisierung, Legacy-Migration) | ✅ Phase 11 |
| Kundenaufnahme & Marker-Zuweisung | ✅ Phase 12 |
| ATS↔AMS Handoff (Docs P0) | 🔄 Phase 13 — Spec: [`HANDOFF.md`](./HANDOFF.md) · P0–P4 ✅ · L4 UX ✅ · P5+ Presence ✅ · **P6 Path Hints** (Docs ✅ · **P6a** ✅ · P6b–d offen) |
| Medien nachreichen (bestehende Order) | ✅ Phase 14 |
| ATS-Nachreichen (Append-Handoff) | ✅ Phase 15 |
| Multi-Dropbox-Konten (Native + Custom-API) | ✅ Phase 16 — 16a ✅ · 16b ✅ · 16c ✅ · 16d ✅ |
| Infobroschüre PDF (Erst-Upload) | ✅ Phase 17 |
| Release-Kanäle (Beta / Stable / Auto-Latest) | ✅ Phase 18 |
| Kundenaufnahme ID-Flow + Ordner-Normalisierung | ✅ Phase 19 — Spec unten · **19a** ✅ · **19b** ✅ · **19c** ✅ · **19d** ✅ · **19e** ✅ |
| SMB-Session-Diagnose & Idle-Cleanup (Windows) | ✅ Phase 20 — Spec unten · **20a–20d** ✅ |
| Auto-Nachreichen bei gleicher Kunden-/Booking-ID | ✅ Phase 21 — Spec unten · **21a–21d** ✅ |

**Nächste Phase (AMS):** offen (Phase 21 abgeschlossen)  
**Parallel (ATS):** Phase 13 / **P6b** — Bridge Path Hints; Spec: [`HANDOFF.md`](./HANDOFF.md) §9.3 · ATS = Phase 35; optional Phase-21-Hinweis dokumentiert in 21d

---

## 3. Tech-Stack

| Schicht | Technologie | Hinweis |
|---------|-------------|---------|
| Desktop-Shell | Tauri 2 | Win + Mac + Linux |
| Backend | Rust | `src-tauri/src/` |
| Frontend | React 19 + TypeScript | `src/` |
| Styling (ab Phase 7/9) | Tailwind CSS + shadcn/ui | Schrittweise |
| State (ab Phase 7) | Zustand | Globaler App-State |
| HTTP | `reqwest` + `tokio` | Dropbox, Custom API, SMS, Twilio |
| Secrets | `keyring` | Nie Klartext |
| Storage | SQLite (`rusqlite`) | Config + History |
| E-Mail | `lettre` | SMTP (+ IMAP optional) |
| Auto-Update | Tauri Updater | Phase 10 |

### Agent-Regeln (immer gültig)

- **NIEMALS** Dateien im Legacy-Projekt ändern
- Secrets nur im OS-Keyring
- Qt-Signals → Tauri Events; Threads → tokio
- Nach jeder Phase: `cargo test` + `npm run tauri dev`
- **Eine Phase pro Agent-Session**
- Große Legacy-Dateien (`custom_api_client.py`, `app.py`, `settings.py`) immer feature-weise schneiden

---

## 4. Entwicklungsumgebung

### Befehle

```powershell
cd C:\Users\Kowalenko\PycharmProjects\AeroMediaService-v2

npm install
npm run tauri dev
cargo test --manifest-path src-tauri/Cargo.toml
npm run check          # nach Einführung von tsc --noEmit
npm run tauri build
```

### IDE

- Hauptprojekt: `AeroMediaService-v2`
- Legacy **nicht** attachieren — Pfade aus Abschnitt 5 im Prompt verwenden

---

## 5. Legacy-Referenz

### Basis-Pfad

```
C:\Users\Kowalenko\PycharmProjects\AeroMediaService
```

### Wichtigste Dateien (Copy-Paste für Agent-Prompts)

```
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\config.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\logger.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\signals.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\monitor.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\folder_stability.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\uploader.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\upload_markers.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\services\dropbox_client.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\services\custom_api_client.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\services\email_client.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\services\sms_client.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\models\kunde.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\app.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\settings.py
```

### Was NICHT aus Legacy kopieren

- `build/`, `dist/`, `venv/`, `__pycache__/`
- PyInstaller-/NSIS-Artefakte
- `_test_*.py` nur als Spezifikation lesen

---

## 6. Vollständiges Datei-Mapping

Siehe [MIGRATION.md](./MIGRATION.md).

---

## 7. Architektur

Siehe [ARCHITECTURE.md](./ARCHITECTURE.md).

### Geplante Rust-Modulstruktur

```
src-tauri/src/
  lib.rs
  constants.rs
  commands/
  monitor/          # service, stability
  upload/           # worker, control, registry, checkpoint, retry
  cloud/            # traits, dropbox, custom_api, manifest
  notify/           # email, sms, whatsapp, message, resend, sms_sync
  storage/          # config, secrets, history, customers, logging
  model/            # kunde, marker, validation, history_status, manual_status
  util/             # archive, link_shortener
```

### Geplante React-Struktur

```
src/
  App.tsx
  main.tsx
  components/
    StatusLight.tsx
    MonitorLog.tsx
    HistoryTable.tsx
    CustomersPanel.tsx
    FolderSelectionModal.tsx
    SettingsDialog.tsx
    SetupWizard.tsx
    ResendNotificationsDialog.tsx
    LoadingOverlay.tsx
    UpdateDialog.tsx
    chrome/                 # AppChrome, Titlebar (Phase 11)
  store/
    appStore.ts
    historyStore.ts
    customerStore.ts
    themeStore.ts
  lib/
    tauri.ts
    platform.ts
```

---

## 8. Phasenplan

> Kopiere den Prompt der jeweiligen Phase in ein neues Agent-Fenster.
> Hänge `@docs/IMPLEMENTATION_PLAN.md` und die genannten Legacy-Dateien an.

---

### Phase 0 — Scaffold & Docs

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Keine  
**Ziel:** Lauffähiges Tauri-Gerüst + Dokumentationsbasis

#### Aufgaben

- [x] Tauri 2 + React + TypeScript Scaffold
- [x] `AGENTS.md`, `docs/IMPLEMENTATION_PLAN.md`, `ARCHITECTURE.md`, `MIGRATION.md`
- [x] Branding: productName „Aero Media Service“, Identifier `com.aksoftware.aero-media-service`
- [x] Minimal-UI: App-Name + Version (Rust `get_app_version`)
- [x] `npm run tauri dev` verifizieren

#### Agent-Prompt

```
Phase 0 ist erledigt. Weiter mit Phase 1.
```

---

### Phase 1 — Config, Secrets, Logging, Events

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Phase 0  
**Ziel:** Persistente Einstellungen, Keyring, Log-Events

#### Aufgaben

**Rust:**
- [x] `storage/config.rs` — nicht-sensible Settings (Pfade, Intervalle, Flags)
- [x] `storage/secrets.rs` — Keyring (Dropbox Keys, Tokens, SMTP/SMS/Twilio)
- [x] `storage/logging.rs` — Datei-Logging + Event `log-message`
- [x] Event-Namen analog `core/signals.py` (connection, upload-progress, monitoring, …)
- [x] Tauri-Commands: `get_setting`, `save_setting`, `get_secret`, `save_secret`, `get_app_version`

**React:**
- [x] Settings-Skeleton: `monitor_path`, `archive_path`, `log_file_path`, `scan_interval`
- [x] Log-Panel stub (lauscht auf `log-message`)

#### Legacy

```
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\config.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\logger.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\signals.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\settings.py
```

#### Erfolgskriterien

- [x] Settings speichern/laden überlebt App-Neustart
- [x] Secrets landen im OS-Keyring, nicht in Config-Dateien
- [x] Log-Zeilen erscheinen in der UI
- [x] `cargo test` grün, `npm run tauri dev` startet

#### Agent-Prompt

```
Implementiere Phase 1 aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Legacy:
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\config.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\logger.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\signals.py
Nur Phase 1 — kein Monitor, kein Upload.
Danach: cargo test && npm run tauri dev.
```

---

### Phase 2 — Marker & Kundenmodell

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Phase 1  
**Ziel:** Marker-Parsing und `Kunde` inkl. Validierung

#### Aufgaben

- [x] `model/marker.rs` — lesen/schreiben/löschen, Typ-Normalisierung (Handcam→Handycam)
- [x] Pure-Contact-Marker-Erkennung
- [x] `model/kunde.rs` + `normalize_phone`
- [x] `model/validation.rs` (E-Mail, Share-Link)
- [x] Unit-Tests (Spec: Legacy `_test_marker_*.py`)

#### Legacy

```
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\upload_markers.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\models\kunde.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\utils\validation.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\monitor.py
```

#### Agent-Prompt

```
Implementiere Phase 2 aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Legacy: Marker + Kunde + Validation (Pfade im Plan).
Nur Phase 2. Unit-Tests Pflicht.
```

---

### Phase 3 — Ordner-Monitor + Stability

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Phase 2  
**Ziel:** Überwachung des Monitor-Ordners, Queue-Einträge (Dry-Run)

#### Aufgaben

- [x] `monitor/stability.rs` — Port von `folder_stability.py`
- [x] `monitor/service.rs` — Scan-Intervall, Marker → Job
- [x] Start/Stop Monitoring Commands + Events
- [x] Noch kein echter Cloud-Upload (Stub/Log)

#### Legacy

```
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\monitor.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\folder_stability.py
```

#### Agent-Prompt

```
Implementiere Phase 3 aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Legacy: monitor.py + folder_stability.py
Nur Monitor + Stability, Upload stubben.
```

---

### Phase 4 — Upload-Pipeline + Dropbox

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Phase 3  
**Ziel:** Warteschlange, Pause/Resume/Cancel, Dropbox-Upload, Archiv

#### Aufgaben

- [x] `upload/control.rs`, `registry.rs`, `worker.rs`
- [x] `cloud/traits.rs` + `cloud/dropbox.rs` (Chunk-Upload, Share-Link)
- [x] `util/archive.rs`
- [x] Progress-Events

#### Legacy

```
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\uploader.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\upload_control.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\upload_queue_registry.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\archive.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\services\base_client.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\services\dropbox_client.py
```

#### Agent-Prompt

```
Implementiere Phase 4 aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur Dropbox-Upload-Pipeline. Kein Custom API, keine Notifications.
```

---

### Phase 5 — Checkpoints + Custom API (Upload-Kern)

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Phase 4  
**Ziel:** Resume, Manifest, Custom-API-Upload, Shortener

#### Aufgaben

- [x] `upload/checkpoint.rs`
- [x] `cloud/manifest.rs`
- [x] `cloud/custom_api.rs` in Untermodule splitten (auth, orders, upload)
- [x] `util/link_shortener.rs`
- [x] Pure-Contact-Marker → Dropbox trotz `custom_api`

#### Legacy

```
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\utils\upload_checkpoint.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\utils\dropbox_manifest.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\utils\link_shortener.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\services\custom_api_client.py
```

**Hinweis:** `custom_api_client.py` ist sehr groß — nur Upload/Auth/Manifest in dieser Phase.

---

### Phase 6 — Notifications

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Phase 4 (Dropbox) / 5 (Links)  
**Ziel:** E-Mail, SMS, WhatsApp nach Upload

#### Aufgaben

- [x] `notify/email.rs` (SMTP + Sandbox-Fallback, optional IMAP Sent)
- [x] `notify/sms.rs` (seven.io + Balance)
- [x] `notify/whatsapp.rs` (Twilio)
- [x] `notify/message.rs` (gemeinsame Nachrichtentexte)
- [x] Orchestrierung analog Uploader

#### Legacy

```
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\services\email_client.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\services\sms_client.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\services\whatsapp_client.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\services\message_client.py
```

---

### Phase 7 — History-UI + Statusmodell

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Phase 4+  
**Ziel:** Persistente History + Haupt-UI

#### Aufgaben

- [x] `storage/history.rs` (SQLite; optional Import `upload_history.json`)
- [x] `model/history_status.rs` — `build_overall_status`
- [x] React: History-Tabelle, Monitor-Log, StatusLight
- [x] Tailwind/Zustand einführen falls noch nicht

#### Legacy

```
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\utils\history_manager.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\history_status.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\app.py
```

---

### Phase 8 — Retry, Resend, Manual Status

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Phase 6–7  
**Ziel:** Operator-Werkzeuge aus Legacy-History

#### Aufgaben

- [x] Retry Upload
- [x] Resend Notifications + Share-Link-Lookup
- [x] Manual Status Actions
- [x] SMS History Sync
- [x] Tests aus Legacy `_test_*.py` als Spec

#### Legacy

```
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\retry_upload.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\resend_notifications.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\manual_status.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\core\sms_history_sync.py
```

---

### Phase 9 — Settings vollständig + App-Shell

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Phase 1–8  
**Ziel:** Alle Settings-Tabs + OAuth-Connect-Flows + Deferred Startup

#### Aufgaben

- [x] SettingsDialog vollständig (Cloud, Shortener, SMTP, SMS, WhatsApp)
- [x] OAuth Browser-Flow
- [x] LoadingOverlay, Fehlerdialoge
- [x] App-Shell / Layout finalisieren

#### Legacy

```
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\settings.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\app.py
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\main.py
```

---

### Phase 10 — Updater, Build, CI, Plattformen

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Phase 9  
**Ziel:** Releases wie ATS-v2

#### Aufgaben

- [x] Tauri Updater Plugin
- [x] Windows / macOS / Linux Bundles
- [x] GitHub Actions (Vorbild ATS-v2)
- [x] Keyring-/Pfad-Tests pro Plattform
- [x] UpdateDialog + Settings-Tab „Extras“ (Software-Update / Version wechseln)

#### Legacy / Vorbild

```
@C:\Users\Kowalenko\PycharmProjects\AeroMediaService\utils\updater.py
@C:\Users\Kowalenko\PycharmProjects\AeroTandemStudio-v2\docs\IMPLEMENTATION_PLAN.md
```

Siehe auch `docs/RELEASE.md`.

---

### Phase 11 — Polish (optional)

**Status:** ✅ Erledigt  
**Abhängigkeiten:** Phase 9–10  
**Ziel:** First-Run, Titlebar/Theme, History-Performance, Legacy-Migration

#### Aufgaben

- [x] First-Run Wizard (`SetupWizard.tsx`) — Pfade, Cloud-Hinweis, Theme; `setup_completed`; Skip; Settings kann Wizard erneut öffnen (+ optional Factory-Reset Pfade)
- [x] Titlebar / Theme — AppChrome (Win/Linux Custom-Controls, macOS Overlay); Hell/Dunkel (Default dunkel, teal/slate — kein Purple)
- [x] History-Virtualisierung — Windowing in `HistoryTable` / `VirtualList` (Pagination bleibt)
- [x] Migration QSettings (`AKSoftware`/`AeroMediaService`) + Keyring `DropboxUploaderApp` → v2 Keyring + SQLite; Flag `legacy_migration_done`; Secrets nie in SQLite

#### Erfolgskriterien

- [x] Wizard beim ersten Start; Skip setzt Flag
- [x] Custom Titlebar stabil bzw. per Flag abschaltbar
- [x] Große History-Seiten scrollen ohne alle DOM-Zeilen
- [x] Legacy-Import idempotent
- [x] `cargo test` grün, `npm run tauri dev` startet

#### Agent-Prompt

```
Phase 11 ist erledigt. Optional: weitere Polish-Feinschliffe nach Bedarf.
```

---

### Phase 12 — Kundenaufnahme & Marker-Zuweisung (Fertig-App)

**Ziel:** Kunden erfassen, Warteschlange führen und `_fertig.txt` in Medienordner schreiben — ohne externe Fertig App.

**Scope:**
- [x] SQLite-Kundenwarteschlange (`customers.db`)
- [x] Tauri-Commands: CRUD, Status, Ordnerliste, Zuweisung
- [x] UI: Formular (+ Clipboard-JSON), Kundenliste, Ordner-Browser, Zuweisungs-Verlauf
- [x] Marker-Schreiben über `write_fertig_marker` (Pure Contact)
- [x] Belegt-Check: `_fertig.txt` / `_in_verarbeitung.txt`

**Referenz (nur lesen):**
```
@C:\Users\Kowalenko\WebstormProjects\fertig-app\electron\main.js
@C:\Users\Kowalenko\WebstormProjects\fertig-app\src\pages\FormPage.jsx
@C:\Users\Kowalenko\WebstormProjects\fertig-app\src\pages\PersonsPage.jsx
```

**Nicht in dieser Phase:** Fertig-App-Datenmigration, API-Marker-Erzeugung.

**Prompt:**
```
Implementiere Phase 12 aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur Phase 12. Danach cargo test && npm run tauri dev.
```

---

### Phase 13 — ATS ↔ AMS Handoff

**Ziel:** Zuverlässiger Datei-Handoff vom ATS-Export auf den SMB-Share `aktuell` (AMS-`monitor_path`), ohne die Upload-Pipeline umzubauen. Optional Feedback (Outbox) und LAN-Bridge (Customer-Lookup, Status, Wake).

**Kanonische Spec:** [`docs/HANDOFF.md`](./HANDOFF.md)

**Betriebsmodell:** ATS schreibt auf Share `aktuell` → AMS monitored denselben Share → Claim/Upload/Notify/Archiv wie bisher. Apps oft auf verschiedenen PCs im LAN.

**Nicht-Ziele:** Upload-Worker/Cloud/Notify umbauen; Medien per HTTP; Marker-Protokoll abschaffen; localhost-only.

#### Teilphasen

- [x] **P0** — Docs: `HANDOFF.md`, Phase-13-Eintrag, `AGENTS.md`, `ARCHITECTURE.md`
- [x] **P1** — ATS: `_ams_manifest.v1.json` schreiben; AMS: Manifest-Parse + Gate vor Claim; Ignore `.ams-handoff`; Legacy ohne Manifest; Unit-Tests
- [x] **P1b** — AMS: Status-Outbox `aktuell/.ams-handoff/<correlation_id>.json`; ATS: `correlation_id` / `producer_ref` + Status lesen/anzeigen
- [x] **P2** — Bridge LAN: `GET /v1/health`, `POST /v1/customer/lookup` (Token-Auth; Customer-API nur in AMS)
- [x] **P3** — Bridge: `GET /v1/jobs/{correlation_id}`, `POST /v1/handoff/ready` (Monitor wake, kein Upload-Bypass)
- [x] **P4** — Bridge mDNS Discovery only (`_ams-bridge._tcp.local.`; Token manuell; keine SHA/strict)
- [x] **L4 UX** — ATS Historie: AMS-Status-Chips + Phasen-Stepper, Last-Known in SQLite, Poll bei Terminal stoppen
- [x] **P5+** — Bridge-Presence/Host-Aktivität (ATS `X-Ats-*` Header, AMS Presence-Store + UI); weitere optionale Themen bleiben z. B. SHA-256 / strict extras
- [ ] **P6** — Bridge Path Hints (`ats_paths` + Capability `paths-v1`); Spec [`HANDOFF.md`](./HANDOFF.md) §9.3; ATS = Phase 35
  - [x] **P6 / Docs** — Spec + Plan-Einträge (AMS + ATS)
  - [x] **P6a** — AMS: Settings `ats_primary_smb_url` / `ats_backup_smb_url`; Health `ats_paths`; Capability `paths-v1`; optional mDNS `paths=1`
  - [ ] **P6b** — ATS: Health-DTO + Diff-Helpers (`amsPathHints`); Store; kein Auto-Apply
  - [ ] **P6c** — ATS: Suggest-UI + Profil `ams-backup` + Credentials-Flow nach Übernahme
  - [x] **P6d** — optional: Drift-Warnung Header/Settings (AMS)

#### AMS-Gate (P1) — Kurz

Vor `claim_fertig_marker`: Manifest gültig → Claim (Stability verkürzen/überspringen); incomplete → liegen lassen + `rejected`; kein Manifest → Legacy inkl. Stability. `manifest_required` default `false`.

#### Partner-Repo

```
C:\Users\Kowalenko\PycharmProjects\AeroTandemStudio-v2
```

P1/P1b/P2/P3/P6 erfordern Änderungen in **beiden** Repos; pro Session nur **eine** Teilphase (P6a, P6b, …).

#### Erfolgskriterien (gesamt Phase 13)

- [x] Manifest-Handoff auf SMB-Share verhindert Claim unvollständiger Jobs
- [x] Legacy-Ordner ohne Manifest und AMS-Kunden-UI (Pure-Contact-Marker) weiter funktionsfähig
- [x] Outbox-Status für ATS ohne gleichen PC lesbar
- [x] Bridge optional; Datei-Pfad allein ausreichend
- [x] `cargo test` grün (AMS; ATS analog in ATS-Repo)
- [ ] P6: AMS publiziert client-taugliche `ats_paths`; ATS Suggest ohne stilles Overwrite / ohne Failover

#### Agent-Prompt (nächste Session = P6b)

```
Implementiere Phase 13 Teilphase P6b aus @docs/IMPLEMENTATION_PLAN.md
Spec: @docs/HANDOFF.md §9.3
Regeln: @AGENTS.md (ATS-Repo)
Nur P6b (ATS Health-DTO + Diff-Helpers, Store; kein Auto-Apply). Kein AMS.
Danach cargo test.
```
---

## 9. Config-Schema

### Nicht-sensibel (Auswahl)

| Key | Beschreibung | Default |
|-----|--------------|---------|
| `monitor_path` | Überwachter Ordner | `""` |
| `archive_path` | Archiv-Basis | `""` |
| `log_file_path` | Log-Datei | `""` |
| `scan_interval` | Sekunden | `10` |
| `folder_stability_enabled` | Stability an | `"true"` |
| `folder_stability_seconds` | Wartezeit | `15` |
| `manifest_required` | Handoff: Manifest vor Claim erzwingen (Phase 13) | `"false"` |
| `bridge_enabled` | LAN-Bridge-Server (Phase 13 / P2) | `"false"` |
| `bridge_bind` | Bind-Adresse (LAN, z. B. `0.0.0.0:8787`) | `"0.0.0.0:8787"` |
| `ats_primary_smb_url` | Client-Hint Primär-Share für ATS (P6; bevorzugt `smb://`) | `""` |
| `ats_backup_smb_url` | Client-Hint Backup-Share für ATS (P6; nur Profil, kein Failover) | `""` |
| `smb_session_warn_threshold` | Warnung ab N SMB-Server-Sessions (Phase 20; Windows) | `"8"` |
| `smb_session_idle_min_seconds` | Min. Idle für Safe-Close (Phase 20) | `"600"` |
| `smb_session_auto_close_enabled` | Auto-Close Idle-Sessions (Phase 20c; default aus) | `"false"` |
| `smb_session_poll_seconds` | Poll-Intervall Session-Diagnose (Phase 20) | `"30"` |
| `selected_cloud_service` | `dropbox` \| `custom_api` | `dropbox` |
| `active_dropbox_account_id` | Aktives Native-Dropbox-Profil (Phase 16) | `""` (nach Migration gesetzt) |
| `active_custom_dropbox_account_id` | Aktives Custom-API-Dropbox-Profil (Phase 16) | `""` (nach Migration gesetzt) |
| `smtp_*` / Sandbox-Flags | E-Mail | — |

### Secrets (Keyring)

| Key | Beschreibung |
|-----|--------------|
| `db_app_key` / `db_app_secret` / `db_refresh_token` | Dropbox Native (Legacy-Alias Phase 16; Lesen bis Migration) |
| `custom_db_app_key` / `custom_db_app_secret` / `custom_db_refresh_token` | Custom-API Dropbox (Legacy-Alias Phase 16) |
| `db_app_key_<ams_id>` / `db_app_secret_<ams_id>` / `db_refresh_token_<ams_id>` | Native-Dropbox pro Profil (Phase 16) |
| `custom_db_app_key_<ams_id>` / `custom_db_app_secret_<ams_id>` / `custom_db_refresh_token_<ams_id>` | Custom-API-Dropbox pro Profil (Phase 16) |
| SMTP-Passwort, `sms_api_key`, Sandbox-Key | Notify |
| `twilio_account_sid` / `twilio_auth_token` | WhatsApp |
| `bridge_token` | LAN-Bridge Bearer-Token (Phase 13 / P2) |

Organisation Legacy: `AKSoftware` / `AeroMediaService`.  
Keyring-Service v2: `AeroMediaService-v2` (Legacy `DropboxUploaderApp` wird einmalig importiert, Flag `legacy_migration_done`).  
Setup: `setup_completed`, Theme: `ui_theme` (`dark` \| `light`).

---

### Phase 14 — Medien nachreichen (bestehende Order / Dropbox-Ordner)

**Status:** ✅ Erledigt  
**Ziel:** Vergessene Dateien in denselben Dropbox-Ordner und dieselbe Cloud-Order legen, ohne neuen Monitor-Ordner oder neuen Kunden-Link.

- [x] Historie-Aktion „Nachreichen…“ (Status Erfolgreich)
- [x] Dialog: Option (HV/HF/OV/OF + Preview) zuerst, dann Dateien wählen
- [x] Upload in gespeicherten `remote_path`; bestehender Share-Link; keine Kunden-Benachrichtigung
- [x] Custom API: `existing_order_id` + Root-Pfad aus `remote_path`
- [x] Cloud: Order-Lookup (id / Pfad / customer+booking), Status- und Manifest-Merge
- [x] Buchungsoptionen: Customer-API ist Source of Truth; Detail/Refresh/Nachreichen laden Flags neu; Historie speichert Last-Known

---

### Phase 15 — ATS-Nachreichen (Append-Handoff)

**Status:** ✅ Erledigt  
**Ziel:** ATS reicht Medien an exportierte Vorgänge nach; AMS hängt sie an die bestehende Order (gleicher Link, keine Notify). Spec: [`HANDOFF.md`](./HANDOFF.md) §6.1.

- [x] Manifest `extensions.kind=append` + `parent_correlation_id`
- [x] Gate: Parent muss Historie `Erfolgreich` sein
- [x] Worker-Route: Phase-14-Append (`remote_path` / `existing_order_id`), kein neuer Link, keine Mail
- [x] Bridge-Capability `append-v1`
- [x] ATS: Historie-Dialog Dateien + Kategorie + Preview/Voll

---

### Phase 16 — Multi-Dropbox-Konten (Native + Custom-API)

**Status:** ✅ 16a–16d erledigt  
**Ziel:** Mehrere Dropbox-Konten pro Cloud-Slot (Native und Custom-API-Dropbox), jeweils genau eines aktiv für **neue** Jobs; Upload/Append/Retry/Resume immer über das am Job/History **gebundene** Konto.

**Leitprinzipien**

1. Aktives Konto steuert nur *neue* Uploads (pro Pool).
2. Job/History speichern, welches AMS-Profil den Upload gemacht hat.
3. Upload/Append/Retry/Resume lesen die gebundene Account-ID — kein stiller Fallback auf „gerade aktiv“.
4. Secrets nur Keyring; SQLite nur Metadaten.
5. Zwei getrennte Pools: `native` (`db_*`) und `custom_api` (`custom_db_*`) — kein Mischen der Token zwischen Pools.

**Zwei Pools (parallel)**

| Pool | Setting (aktiv) | Legacy-Keys | Namespaced Keys | Verwendung |
|------|-----------------|-------------|-----------------|------------|
| Native | `active_dropbox_account_id` | `db_app_key` / `db_app_secret` / `db_refresh_token` | `db_*_<ams_id>` | `selected_cloud_service=dropbox` |
| Custom-API Dropbox | `active_custom_dropbox_account_id` | `custom_db_*` | `custom_db_*_<ams_id>` | Direct-Dropbox + Manifest unter Custom API; reine Kontakt-Marker (`use_dropbox_client`) |

`selected_cloud_service` bleibt unverändert (`dropbox` \| `custom_api`). Multi-Account ersetzt nicht die Cloud-Wahl — es multipliziert die Dropbox-Logins **innerhalb** jedes Slots.

#### Datenmodell (SQLite)

```text
dropbox_accounts
  id                    TEXT PK          -- AMS-UUID
  pool                  TEXT NOT NULL    -- 'native' | 'custom_api'
  label                 TEXT
  dropbox_account_id    TEXT             -- von users/get_current_account (pro pool unique)
  email                 TEXT
  display_name          TEXT
  app_key_hint          TEXT
  created_at / updated_at
  UNIQUE(pool, dropbox_account_id) WHERE dropbox_account_id <> ''
```

#### History / Job-Binding

| Feld | Bedeutung |
|------|-----------|
| `dropbox_account_ams_id` | AMS-Profil-UUID |
| `dropbox_account_pool` | `native` \| `custom_api` |
| `dropbox_account_id` | Dropbox-Account-ID (Diagnose) |
| `dropbox_account_email` | Snapshot für Anzeige nach Profil-Löschung |

`AppendTarget` und Checkpoints (`kind: dropbox_native` / Custom-Äquivalent) tragen dieselbe `dropbox_account_ams_id` (+ Pool).

#### Client-Auflösung

```text
resolve_dropbox_client(job|history):
  1. ams_id + pool aus Job/History
  2. Legacy ohne Feld: einziges Profil im erwarteten Pool, sonst Fehler/Confirm
  3. Client aus Registry; Token fehlt → klarer Fehler, kein Cross-Account-Fallback
```

Betrifft: `upload/worker`, `upload/append`, `upload/retry`, `notify/resend`, Operator-Append, `CloudState` / `CustomApiClient`-Dropbox-Pfad.

#### Teilphasen (eine Session = eine Slice)

##### 16a — Account-Profile + Migration + Registry (D1)

- [x] Tabelle `dropbox_accounts` + Settings `active_dropbox_account_id` / `active_custom_dropbox_account_id`
- [x] Keyring-Namespace pro Profil; Legacy-Keys weiter lesen
- [x] Einmal-Migration: vorhandene `db_*` → ein Native-Profil; `custom_db_*` → ein Custom-Profil; Active setzen
- [x] `DropboxSecretKeys::for_account(pool, ams_id)`; `DropboxAccountInfo.account_id` parsen
- [x] `CloudState`: Registry pro Pool (`HashMap` / lazy Clients) statt einzelner `Arc<DropboxClient>`
- [x] Unit-Tests: Migration, Key-Ableitung, Pool-Isolation (Native-Keys ≠ Custom-Keys)

**DoD:** Mehrere Profile speicherbar (beide Pools); OAuth connect/disconnect pro Profil; je Pool ein Active.

##### 16b — Job an Account binden (D2)

- [x] `UploadJob` + History-Felder (ams_id, pool, email, dropbox_account_id)
- [x] Claim/Enqueue friert aktives Profil des **zuständigen** Pools ein (Native vs. Custom-Dropbox-Pfad inkl. Kontakt-Marker)
- [x] Worker/Append/Retry/Resend/Link-Lookup über Binding
- [x] `AppendTarget.dropbox_account_ams_id` (+ pool); Altbestand ohne Feld: nur bei genau einem Profil im Pool oder UI-Confirm
- [x] Unit-Tests: Retry/Append nutzen Parent-Account, nicht Active; fehlendes Profil → Fehler

**DoD:** Neue Jobs schreiben Binding; alle Upload-Nebenpfade respektieren Binding (Native **und** Custom-API-Dropbox).

##### 16c — Switch-Guards + Invarianten (D3 + D5)

- [x] Soft-Active: Wechsel des Active ändert nur Default für *neue* Jobs; laufende behalten Binding
- [x] Hard-Guard: Löschen/Trennen blockieren wenn Queue/Active-Jobs an Profil gebunden (oder Jobs bewusst failen)
- [x] OAuth: gleiche Dropbox-`account_id` → Token-Update; andere ID → neues Profil oder harte Warnung
- [x] Kein Upload-Pfad ohne Binding (außer Settings-Verify); kein Temporary-Override ohne History-Schreiben
- [x] Checkpoints um `dropbox_account_ams_id` (+ pool) erweitern; Resume bei Mismatch verweigern
- [x] Audit-Checkliste: Monitor-Claim, Upload, Pause/Resume, Retry, Operator-Append, ATS-Append, Resend, Pure-Contact-Marker unter `custom_api`

**DoD:** Dokumentierte Switch-Semantik; keine Session-Resume mit falschem Token.

**Audit (16c — Binding-Pfade):** Soft-Active gilt für Claim/Enqueue (`freeze_active_binding`); Recovery liest History-Binding; Worker/`client_for_binding` + `CustomDropboxPin` nur mit Job-Binding; Retry/Append/Resend via `resolve_binding_for_history`; Native- und Direct-Dropbox-Checkpoints tragen `dropbox_account_ams_id`+`pool`; Pause/Resume nutzen denselben Worker-Client (kein Rebind).

##### 16d — Settings-UI + History-Hinweise (D4)

- [x] Settings Dropbox: Kontoliste Native (Label, E-Mail, Quota, Token, Badge Aktiv)
- [x] Settings Custom-API Dropbox: gleiche Liste für Pool `custom_api`
- [x] Aktionen: Verbinden (OAuth), Trennen, Als aktiv, Umbenennen, Entfernen
- [x] Hinweis bei Switch wenn Queue nicht leer („nur neue Jobs“)
- [x] Append-Banner wenn Parent-Konto ≠ Active (Nachreichen über Parent-Konto)
- [x] History-Detail: Dropbox-Konto (E-Mail / Label / Pool)
- [x] Duplikat-Schutz: gleiche `(pool, dropbox_account_id)` aktualisiert Token, kein zweites Profil

**DoD:** Bedienung ohne Keyring-Handarbeit; Status/Quota multiplikativ wie heutiges `DropboxAccountPanel`.

#### Explizit out of scope

- Automatische Job-Routung Kunde/Marker → Konto (spätere Phase)
- Dropbox Business Admin / Team-Namespaces als eigenes Modell
- Änderung Pause/Resume/Cancel-Semantik
- ATS-Handoff-Pflichtfelder (optional später Status-Outbox-Hinweis)

#### Risiken

| Risiko | Mitigation |
|--------|------------|
| Alt-History ohne Binding | Fallback nur bei 1 Profil im Pool; sonst Confirm |
| Cross-Pool-Verwechslung | `pool` am Job; Registry getrennt; UI-Tabs getrennt |
| Profil gelöscht | Snapshot-E-Mail; klarer Fehler statt Rate |
| Custom API Order + falsches Dropbox | Binding am Direct-Dropbox-/Kontakt-Pfad; Order-API unverändert |
| Keyring-Ballast | Delete räumt namespaced Keys auf |

#### Manuelle Abnahme

1. Legacy-Migration: je Pool ≤1 Konto, Upload wie bisher (Dropbox + Custom-API Direct-Dropbox).
2. Zweites Native-Konto aktiv → neuer Job dort; History zeigt Binding.
3. Zweites Custom-Dropbox-Konto; Kontakt-Marker / Direct-Dropbox nutzen Binding.
4. Active = B; Append an Job von A → Upload über A; Share-Link gültig.
5. Queue mit Job A; Active → B; Job A resumed weiter mit A.
6. Trennen von A bei offenem Job A → blockiert oder bewusster Fail.

**Abhängigkeiten:** Phase 4 (Dropbox), 5 (Custom API), 9 (Settings), 14/15 (Append).  
**Nicht anfassen außer Binding:** Custom-API Order/Manifest-Kern, Notify-Templates, Bridge.

**Agent-Prompt (Slice):**

```
Implementiere Phase 16 Teilphase 16d aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 16d. Settings-UI und History-Hinweise für Multi-Dropbox.
Danach cargo test && npm run tauri dev.
```

---

### Phase 17 — Infobroschüre PDF (Erst-Upload)

**Status:** ✅ Fertig  
**Ziel:** Optional eine Infobroschüre-PDF beim **Erst-Upload** in denselben Cloud-Job-Ordner legen (neuer Share). Keine Änderung an E-Mail/SMS/WhatsApp. Keine Injektion bei Append.

**Leitprinzipien**

1. Quelle nur App-verwaltete Kopie (Settings Drag-Drop), kein freier Externpfad als Wahrheitsquelle.
2. Injektion nur in die **Upload-Dateiliste** — nie in den lokalen Monitor-/Job-Ordner schreiben (kein Fingerprint-/Manifest-/Archiv-Drift).
3. Nur Erst-Upload; Append nie; Retry/Resume nur idempotent (Remote-Zieldatei fehlt noch).
4. Notify-Templates und Historie-UI unverändert; Transparenz nur über Logs.

**Settings**

| Key / UI | Default | Regel |
|----------|---------|--------|
| `brochure_enabled` | `false` | an / aus |
| Quell-PDF | — | Drag-Drop → sofort nach App-Data committen (z. B. `brochure/source.pdf`); Meldung „Broschüre gesetzt“; einsehen / ersetzen / entfernen |
| `brochure_export_name` | `Infobroschuere.pdf` | Remote-Dateiname; nur Basename, auf `.pdf` normalisieren |
| `brochure_subdir` | `""` (leer) | optional; leer = Job-Root → `/<Job>/Infobroschuere.pdf` |
| Validierung | — | nur `.pdf`, max. **5 MB**; sonst Import ablehnen |

**Upload-Verhalten**

- [x] Erst-Upload: wenn enabled und Quelldatei ok → Extra-Eintrag in Upload-Liste unter `{remote}/{subdir?}/{export_name}`
- [x] Append: **nie** injizieren
- [x] Retry / Resume: nur injizieren, wenn Remote-Zieldatei noch nicht existiert (Idempotenz); sonst skip
- [x] enabled, aber keine Broschüre hinterlegt: Job **nicht** failen; einmal warnen/loggen, ohne PDF weiter
- [x] Namenskollision (gleicher Relativpfad schon in Medien oder remote): **AMS überschreibt**, Warnung im Log
- [x] Custom-API Dropbox / Manifest v1.1 `paths_only`: PDF mit aufnehmen, sobald sie remote landet; kein neuer `STANDARD_CATEGORIES`-Ordner
- [x] Binding: über bestehendes Job-Dropbox-Binding (Phase 16), kein Extra-Konto-Pfad
- [x] Keine Notify-Text-Änderungen; Historie: **nur Log** („Infobroschüre hochgeladen“ / Warnungen), kein History-Flag

**Settings-UI**

- [x] Abschnitt Infobroschüre (z. B. unter Allgemein oder eigener klarer Block): Toggle, Drop-Zone, Export-Name, optional Unterordner, Vorschau (Name/Größe), Öffnen, Entfernen
- [x] Drop sofort speichern (nicht an Dialog-Save koppeln); Ersetzen überschreibt App-Data-Kopie

**Tests / DoD**

- [x] Unit: enabled/disabled; Append skip; Retry idempotent; fehlende Quelle warnt; Export-Name-Härtung; Subdir leer vs. gesetzt; 5 MB-Limit; Kollision überschreibt + Log
- [ ] Manuell: Settings Drop → Erst-Upload zeigt PDF im Cloud-Root; Append ohne zweite PDF; Notify unverändert
- [x] `cargo test` + `npm run tauri dev`

**Abhängigkeiten:** Phase 4 (Upload/Dropbox), 5 (Custom API / Manifest), 9 (Settings), 14/15 (Append-Guards), 16 (Binding).  
**Nicht-Ziele:** Notify ändern; ATS-Handoff/Manifest erweitern; PDF in lokalen Job kopieren; Append-Support; History-Flag.

**Agent-Prompt:**

```
Implementiere Phase 17 aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur Phase 17 (Infobroschüre PDF). Notify und Append-Pipeline-Verhalten nicht erweitern.
Danach cargo test && npm run tauri dev.
```

---

### Phase 18 — Release-Kanäle (Beta / Stable / Auto-Latest)

**Ziel:** Identischer Release-Flow wie ATS-v2: SemVer-Betas (`0.1.14-beta.1`), GitHub-Prereleases, Auto-Latest nur für Stable, Einstellung **Betatester** für Auto-Update inkl. Vorabversionen. AMS behält `merge-updater-manifest` (kein Race durch parallele `latest.json`-Uploads).

**Vorbild:** `AeroTandemStudio-v2` — `docs/RELEASE.md`, `scripts/{semver,changelog,release}.mjs`, `.github/workflows/release.yml` (`promote-latest`), `src-tauri/src/updater/mod.rs` (`include_beta`).

**Slices**

#### 18a — Tooling

- [x] `scripts/semver.mjs` (parse/compare/bump/nextBeta/toStable)
- [x] `scripts/changelog.mjs`: Beta-Snapshot (Unreleased bleibt), Stub, kein Walkback für Prereleases
- [x] `scripts/release.mjs`: Bump × Kanal; von Beta → `beta.N+1` oder Stable-Promote
- [x] `docs/RELEASE.md` an ATS-Flow angleichen

#### 18b — CI

- [x] `prepare`: `is_prerelease` aus Tag (`-` im Versionsstring)
- [x] Release create/edit mit `--prerelease` / `--prerelease=false`, immer `--latest=false`
- [x] Matrix `prerelease` aus Output; **`merge-updater-manifest` behalten** (auch Beta-Tags)
- [x] Job `promote-latest` nur Stable, nach Merge, SemVer-Guard gegen neueres Latest

#### 18c — Backend Updater

- [x] `check_for_updates(include_beta)`; Stable → Plugin/`/latest/`; Beta → Releases-API
- [x] Echtes SemVer inkl. Prerelease-Ordnung in Rust
- [x] Setting `beta_updates_enabled` (Default `false`)
- [x] Unit-Tests: Ordering, `resolve_best_update`

#### 18d — Frontend

- [x] `versionCompare.ts` → SemVer (wie ATS)
- [x] Extras: persistentes **Betatester** statt flüchtigem `showPrereleases`
- [x] Update-Check / Dialog: `includeBeta`, `isBeta`, Beta über `installSpecificVersion` wenn `updater_json_url`

**Nicht-Ziele:** Upload-Pipeline, Handoff, i18n-Port, Wechsel auf ATS `uploadUpdaterJson: true`.

**DoD**

- [x] `npm run release` erzeugt Beta- oder Stable-Tags inkl. Changelog-Regeln
- [x] CI: Beta = Prerelease ohne Latest; Stable = Merge + Auto-Latest
- [x] App ohne Betatester nur Stable; mit Betatester Beta sichtbar; finale Stable > Beta
- [x] `cargo test` + `npm run test:scripts` + `npm run check`

**Agent-Prompt:**

```
Implementiere Phase 18 aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Vorbild: ATS-v2 Release-Flow (Beta). merge-updater-manifest behalten.
Nur Phase 18. Danach cargo test && npm run test:scripts.
```

---

### Phase 19 — Kundenaufnahme ID-Flow + Job-Ordner-Normalisierung

**Ziel:** Optional `kunden_id` + `booking_id` in der Kundenaufnahme; API-Lookup füllt Kontakt/Medienflags; bei Zuweisung solcher Kunden den **ATS-ID-Flow** lokal nachstellen (Medienlayout → Rename → Manifest → `_fertig.txt`), inkl. robustem TM/VS-Predictor und Review-Dialog bei Unsicherheit.

**Zwei Pfade (verbindlich, kein Hybrid-Marker):**

| Kundendaten | Zuweisung |
|-------------|-----------|
| Ohne beide IDs | Unverändert Phase 12: Pure-Contact-`_fertig.txt`, kein Umbau, kein Manifest |
| Mit beiden IDs | ID-Flow: Busy → Medien umsortieren → Rename → Manifest → API-ID-`_fertig.txt` |

**Vorbild / Referenz (nur lesen):**

```
@C:\Users\Kowalenko\PycharmProjects\AeroTandemStudio-v2\src-tauri\src\video\marker.rs
@C:\Users\Kowalenko\PycharmProjects\AeroTandemStudio-v2\src-tauri\src\video\export_paths.rs
@C:\Users\Kowalenko\PycharmProjects\AeroTandemStudio-v2\src-tauri\src\video\handoff_manifest.rs
@C:\Users\Kowalenko\PycharmProjects\AeroTandemStudio-v2\src\lib\tauri.ts   (DEFAULT_CREW_LIST)
AMS: src-tauri/src/model/marker.rs (build_kunde_from_customer, media_option)
AMS: src-tauri/src/storage/customers.rs (assign_to_folder, Pure Contact)
AMS: docs/HANDOFF.md §5–6 (Schreibreihenfolge, Manifest)
```

**Produktregeln (abgestimmt):**

1. **Medienziel** aus gebuchtem Typ/Flags: Videos → `Outside_Video` oder `Handcam_Video`; Fotos → `Outside_Foto` oder `Handcam_Foto`. Nur Ordner für gebuchte Medienarten anlegen; bereits vorhandene Zielordner weiter befüllen.
2. **Rename** nach ATS: `{YYYYMMDD}_{Gast}_TA_{TM}[_V_{VS}][_Dropzone]`. Gast aus Kundendaten (sanitized). `_V_{VS}` nur bei Outside-Video.
3. **Datum:** aus Customer-API-Lookup; fehlt → heute. **Nicht** aus Ordnerpräfix (Tippfehler).
4. **TM/VS:** aus Original-Ordnernamen + Crew-Liste (Rollen + Aliases) mit Confidence; bei Unsicherheit oder Outside ohne VS → Review-Dialog.
5. **Marker:** wie ATS ID-Mode (`kunden_id`, `booking_id`, `type`, acht Media-/Paid-Flags; kein PII).
6. **Manifest:** Schema wie ATS; `producer.app = "AeroMediaService"` (AMS als Quelle); `marker_hint.format = "api_id"`.
7. **Lookup:** beim Ausfüllen beider IDs in der Aufnahme (Diff bei Konflikten: anzeigen, API übernehmen oder Formular behalten). Nach Zuweisung erneuter Lookup durch normalen Monitor-ID-Flow.
8. **Kollisionen:** `(1)`, `(2)`, …; Nicht-Medien bleiben (inkl. ihrer Subordner); leere Ordner nach Medien-Move löschen, aber nicht wenn Nicht-Medien darin liegen.
9. **Busy:** während Umbau sperren; Reihenfolge strikt: Umbau → Rename → Manifest → `_fertig.txt`.
10. **Batch:** gleicher ID-Umbau-Flow, sequentiell pro Ordner.

**Slices (eine pro Session):** 19a–19e ✅

#### 19a — Crew-Roster + Ordnername-Predictor

- [x] Config: `crew_list` analog ATS (`name`, `tandemmaster`, `videospringer`, **`aliases: string[]`**)
- [x] Defaults aus ATS-`DEFAULT_CREW_LIST` + sinnvolle Start-Aliases (z. B. Cornelius→`Corni`)
- [x] Settings-UI: Crew pflegen (Rollen + Aliases hinzufügen/entfernen)
- [x] Modul `folder_rename` / Predictor: Tokenisieren (Whitespace/`_`/`-`, CamelCase, `TACorni`), Noise droppen (Load/L#, Gera/G, Media-Codes), Struktur-Marker (`TA`/`TD`→TA), Crew-Match inkl. Aliases
- [x] Confidence + `needs_review`-Regeln (TM fehlt/unsicher; Outside-Video ohne VS; Mehrfachkandidaten)
- [x] Dropzone-Suffix aus Ordner ableiten wenn klar (`G`/`Gera`→`_G`), sonst optional/leer
- [x] Unit-Tests: Gold-Set aus den abgestimmten Beispielordnern (Roman→Stefan+Robin, Niels→Cornelius, Christin→Futti, Emilia `TD`→Ralph, Sabine `F`+Ralph, …)

**Nicht in 19a:** Intake-UI, Assign-Pipeline, Manifest-Schreiben.

#### 19b — Kundenaufnahme: IDs, Lookup, Persistenz

- [x] `customers.db` erweitern: `kunden_id`, `booking_id`, Buchungsdatum (optional), `typ`/Media-Flags (acht Booleans + Paid), Roh-`media_option` optional
- [x] Formular ganz oben: optionale `kunden_id` + `booking_id`
- [x] Wenn beide sicher gefüllt: Customer-API-Lookup (bestehender Client); leere Kontaktfelder füllen; Medienflags setzen
- [x] Bei Unterschieden: Diff-UI (Feldliste API vs. Formular) → pro Konflikt / global: API übernehmen oder Formular behalten
- [x] Speichern nur mit konsistentem Zustand; Kundenliste zeigt ID-Badge wenn IDs gesetzt
- [x] Edit-Dialog: IDs/Flags sichtbar/editierbar wo sinnvoll
- [x] Ohne IDs: Verhalten Phase 12 unverändert

**Nicht in 19b:** Ordner-Umbau bei Assign (weiter Pure Contact wenn ohne IDs; mit IDs Assign erst ab 19c/19d).

#### 19c — Assign-Backend: Layout, Rename, Manifest, ID-Marker

- [x] Verzweigung in `assign_to_folder` / Batch: mit IDs → ID-Pipeline; ohne → Pure Contact
- [x] Busy-Sperre (FolderState Busy / Belegt-Check inkl. laufendem Umbau)
- [x] Rekursiv Medien finden; nach Flags in Ziel-Subdirs verschieben; Kollision `(1)`…; Nicht-Medien unangetastet; leere Ordner aufräumen
- [x] Zielname bauen (Datum Lookup/heute, Gast, TM/VS aus Predictor-Ergebnis oder Caller-Override, Dropzone)
- [x] Ordner umbenennen (Konfliktbehandlung wenn Zielname existiert)
- [x] `_ams_manifest.v1.json` atomar (Integrity size; producer AMS; `api_id` hint; alle Upload-relevanten Dateien inkl. Nicht-Medien-Pfade)
- [x] `_fertig.txt` atomar wie ATS ID-Marker
- [x] Reihenfolge verbindlich; bei Fehler kein Fertig-Marker; klarer Fehlerstatus / teilweiser Rollback soweit machbar
- [x] Unit-Tests: Layout-Move, Kollision, leere Ordner, Marker-JSON, Manifest-Gate ready

**Nicht in 19c:** Review-Dialog-UI (Predictor-Override kommt als Parameter); Settings Crew nur nutzen, nicht bauen.

#### 19d — Assign-UI: Review-Dialog, Batch, Integration

- [x] Vor ID-Assign: Predictor aufrufen; wenn `needs_review` oder Outside ohne VS → Dialog
- [x] Dialog: Live-Vorschau Zielordnername; Felder TM/VS (Crew-Combobox, Rollenfilter); vorausgefüllt; **fehlende Pflichtfelder** markierter Border + Focus
- [x] Bestätigen erst wenn Pflichtfelder gesetzt; dann 19c-Pipeline
- [x] Batch: sequentiell; pro unsicherer Zeile Dialog (oder Zeilen-Review in Batch-UI); sichere Zeilen still
- [x] Fortschritt/Fehler pro Assign sichtbar; Busy während Lauf
- [x] Manuelle Abnahme: ID-Kunde → unstrukturierter Ordner → fertiger ATS-artiger Job → Monitor Claim/Upload

**DoD (Phase 19 gesamt)**

- [x] Zwei Pfade klar getrennt (kein Hybrid-Marker)
- [x] Gold-Set Predictor-Tests grün; Review erzwingt VS bei Outside ohne Treffer
- [x] Datum nur Lookup/heute
- [x] Crew-Aliases in Settings pflegbar
- [x] Manifest + ID-`_fertig` → Gate Ready / Legacy nicht für ID-Pfad
- [x] `cargo test` + `npm run tauri dev`
- [x] **19e:** Gast-Vorname kollidiert nicht mehr mit Crew-Alias (Post-`TA`-Zone + Kundennamen-Suppress)

**Abhängigkeiten:** Phase 12 (Kunden-UI), 2/5 (Marker + Customer API), 13 (Manifest-Schema/Gate).  
**Nicht-Ziele:** Pure-Contact abschaffen; Ordnerdatum als Datumsquelle; ATS-Crew-Sync über Bridge (optional später); Hash-Marker (`kunden_id_hash`) in der Aufnahme; Alias-Auto-Purge (Kollisionen → 19e Gast-Exclude).

**Agent-Prompts:**

```
Implementiere Phase 19 Teilphase 19a aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 19a (Crew + Predictor + Tests). Kein Assign-Umbau.
Danach cargo test && npm run tauri dev.
```

```
Implementiere Phase 19 Teilphase 19b aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 19b (IDs, Lookup, Diff, Persistenz). Kein Ordner-Umbau.
Danach cargo test && npm run tauri dev.
```

```
Implementiere Phase 19 Teilphase 19c aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 19c (Assign-Backend ID-Pipeline). Review-UI = 19d.
Danach cargo test && npm run tauri dev.
```

```
Implementiere Phase 19 Teilphase 19d aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 19d (Review-Dialog, Batch, Integration).
Danach cargo test && npm run tauri dev.
```

#### 19e — Predictor: Gast-Exclude + Härtung

**Anlass:** Gast-Vorname im Ordner (z. B. `Andreas`) matcht Crew-Alias (`Andy`→`Andreas`) → falscher TM/VS  
(`20260827_Andreas_Kowalenko_TA_Futti_V_Henni_C` → fälschlich `…_TA_Andy_V_Futti_C` statt `…_TA_Futti_V_Henrik_C`).

**Produktregeln:**

1. **Struktur-Zone (Primär):** Wenn Token `TA`/`TD` vorkommt → Crew-Hits **nur aus Tokens nach** diesem Marker. Gast-Zone = alles davor (nach Datums-Noise). Dropzone/`V`-Marker weiter auswertbar (global bzw. in Crew-Zone).
2. **Gast-Suppress (Sekundär):** `PredictOptions` erhält `guest_vorname` / `guest_nachname` (aus Assign-Kunde). Tokens, die klar der **Gast-Zone** zugeordnet sind und Vor-/Nachname matchen (case-insensitive, Umlaut-Fold analog `folder_match::fold_key`), werden nicht als Crew gewertet.
3. **Kein globales Namens-Kill:** Suppress **nicht** auf die Crew-Zone nach `TA` anwenden (Gast und VS können denselben Vornamen teilen, z. B. Robin).
4. **Aliases unverändert:** `Andreas`→Andy bleibt; korrekt, wenn Andy als Crew nach `TA` so geschrieben steht.
5. **Reuse:** Bei `_TA_`/`_TD_` `guest_segment` aus `folder_match` (oder gemeinsame Hilfsfunktion) nutzen — Gast-Slice strippen, Rest an Crew-Match; wenig Duplikat-Logik.
6. **Confidence:** Treffer strikt aus Post-`TA`-Zone → höhere Confidence / seltener unnötiger Review, sofern TM klar und (bei Outside) VS gesetzt.
7. **Review-Transparenz (optional, klein):** Wenn Tokens wegen Gast übersprungen wurden → intern/`review_reasons` oder Preview-Feld `skipped_guest_tokens` (kein Pflicht-Dialog allein deshalb).
8. **UI (leicht):** Im ID-Assign-Review kurz „Gast erkannt: …“ / Hinweis auf Crew-Quelle (z. B. „Crew aus Ordner nach TA“) — hilft Mispredicts zu erklären; kein neuer Wizard.
9. **Alias-Hygiene (Doku/Settings-Hinweis, kein Auto-Purge):** Kurze/eindeutige Aliase bevorzugen; Gast-Kollisionen sind durch Regeln 1–2 abgefangen, nicht durch Alias-Löschen.

**Scope:**

- [x] `PredictOptions`: `guest_vorname`, `guest_nachname` (optional/`Option`)
- [x] Predictor: Post-`TA`/`TD`-Crew-Zone + Gast-Zone-Suppress
- [x] `resolve_crew_fields` / Preview / Pipeline: Kundenvor-/nachname durchreichen
- [x] Shared/Reuse: Gast-Segment analog `folder_match::guest_segment`
- [x] Confidence-Anpassung bei strukturierter Zone
- [x] Unit-Tests (Gold + Regression, siehe unten)
- [x] Review-Dialog: kurze Gast-/Crew-Quelle-Zeile (wenn Preview-Felder vorhanden)
- [x] Plan/AGENTS: Slice 19e referenzieren

**Nicht in 19e:** Alias-Liste umbauen; Pure-Contact-Pfad; neues Settings-Feature außer ggf. einzeiligem Hinweistext; Batch-UI-Umbau.

**Tests (verbindlich):**

| Fall | Erwartung |
|------|-----------|
| Bug-Repro `…_Andreas_Kowalenko_TA_Futti_V_Henni_C` + Gast Andreas/Kowalenko | TM Futti, VS Henrik, `_C` |
| Unstrukturiert Gast Andreas, Ordner `Andreas_Futti` | TM Futti (nicht Andy) |
| Echter Crew-Andreas/Andy nach TA: `…_TA_Andreas_V_Robin` / `…_TA_Andy_…` | TM Andy |
| Gast Robin + `…_TA_Stefan_V_Robin` | VS bleibt Robin (kein Kill nach TA) |
| Bestehendes Gold-Set (Roman/Stefan/Robin, Corni, Christin/Futti, …) | unverändert grün |

**DoD:** Bug-Repro grün; Gold-Set grün; Alias Andreas bleibt; `cargo test`.

**Agent-Prompt:**

```
Implementiere Phase 19 Teilphase 19e aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 19e (Gast-Exclude + Predictor-Härtung). Kein Alias-Purge.
Danach cargo test && npm run tauri dev.
```

---

### Phase 20 — SMB-Session-Diagnose & Idle-Cleanup (Windows)

**Status:** ✅ 20a–20d  
**Betriebsannahme:** AMS läuft immer auf dem Rechner, der den Share (`aktuell` / `monitor_path`) exportiert → AMS = SMB-**Server**-Host.  
**Ziel:** Zu viele / hängende SMB-Server-Sessions erkennen, im Header warnen und gezielt nur sichere Idle-Sessions trennen — ohne Handoff-/Upload-Pipeline zu blockieren.  
**20d:** Uneleviertes AMS + on-demand `ams-smb-helper` (UAC/`runas`) für List/Close.

**Plattform:** Windows-first (Win32 `NetSessionEnum` / Felder analog CIM `MSFT_SmbSession`). macOS/Linux: Stub „nicht unterstützt“ (kein Samba-/smbd-Kill in dieser Phase).

**Produktregeln (verbindlich):**

1. **Diagnose vor Aktion:** Sessions auflisten und Schwellen-Warnung; kein blindes Kill aller Sessions.
2. **Clients-Chip = Warnungsfläche, nicht SMB-Zähler:** Header-Badge-Zahl bleibt **ATS-Bridge-Clients**. Bei SMB-Überlast (`ok` + Count ≥ Schwelle): Warning-Ton am Chip + Tooltip (z. B. „12 SMB-Sessions (Schwelle 8)“). `permission_denied` / Abfragefehler: **kein** Chip-Warn (nur Dialog-Text + Elevate-CTA). Klick öffnet weiterhin `AtsClientsDialog`, erweitert um SMB-Sektion.
3. **Safe-Close nur:** `SecondsIdle ≥ smb_session_idle_min_seconds` **und** `NumOpens == 0`. Nie Sessions mit offenen Handles hart schließen.
4. **Kein Pipeline-Gate:** Monitor/Claim/Upload hängen nicht von SMB-Cleanup ab. Feature = Betriebs-Hilfe.
5. **Elevation:** Lesen/Schließen braucht oft Admin. Klare UI („Admin nötig“); **20d:** optional elevierter Helper nur für List/Close — AMS nicht dauerhaft und nicht erneut als Admin starten.
6. **Technik:** Win32 NetAPI (`NetSessionEnum` / Close via `NetSessionDel`) aus Rust — **kein** `powershell.exe`-Spawn (Console-Flash, Fragilität). Felder spiegeln CIM-Diagnose (`Client`, `User`, `Idle`, `Exists`, `NumOpens`).
7. **Audit:** Jeden Close loggen (SessionId, Client, Idle, NumOpens, manuell/auto, Ergebnis).
8. **Defaults konservativ:** Warn-Schwelle ~8, Idle-Min ~10 min, Auto-Close **aus**, Poll ~30 s.
9. **Soft-Policy:** Kurz dokumentieren / Settings-Hinweis: OS-Idle-Timeout und Server-Limits sind nachhaltiger als App-Kill (App = Notnagel).
10. **Share-Fokus (20c):** Priorität Sessions/OpenFiles am Monitor-/`aktuell`-Share — nicht blind alle Freigaben des Rechners, soweit API es hergibt.
11. **Bridge-Korrelation (20c):** Hinweis, wenn Client-IP zu bekanntem ATS-Host passt (Diagnose); kein Hard-Gate / kein Auto-Kill nur wegen Presence.

**Referenz (bestehend, nur lesen/erweitern):**

```
src/App.tsx                          (Clients-Chip im Header)
src/components/AtsClientsDialog.tsx  (Dialog-Einstieg)
src/components/SmbSessionsSection.tsx
src/lib/smbSessions.ts
src-tauri/src/util/smb_sessions.rs   (Diagnose / Snapshot)
src-tauri/src/util/smb_export.rs     (lokale Share-Exports)
src-tauri/src/util/local_shares.rs   (Share-Kandidaten)
src-tauri/src/util/process.rs        (kein Console-Flash)
docs/HANDOFF.md                      (SMB Data Plane aktuell)
```

**Slices (eine pro Session):** 20a → 20b → 20c → **20d**

#### 20a — Diagnose + Clients-Chip-Warning

- [x] Modul `smb_sessions` (Windows): Sessions listen via NetAPI (`session_id` composite, ClientComputerName/User, `SecondsIdle`, `SecondsExists`, `NumOpens`); Elevation-/Permission-Fehler als Status
- [x] Nicht-Windows: leere Liste + `unsupported` / klare Meldung
- [x] Config: `smb_session_warn_threshold`, `smb_session_poll_seconds` (Defaults s. Schema)
- [x] Tauri-Commands: Status/Snapshot abfragen (Polling vom UI oder leichtgewichtiger Backend-Tick)
- [x] Header-Clients-Chip: Warning-State wenn `session_count ≥ threshold` **oder** Permission-Fehler relevant; Badge-**Zahl** = ATS-Clients unverändert; Tooltip mit SMB-Info
- [x] `AtsClientsDialog`: Sektion „SMB-Sessions“ (Windows) — Tabelle read-only, Schwellen-Hinweis, Admin-Hinweis
- [x] Unit-Tests: Schwellen-Logik / Snapshot-Normalisierung (Mocks oder Fixture-Structs); UI-Töne analog bestehender Warning-Chips

**Nicht in 20a:** Close/Kill; Auto-Close; Share-Filter; Bridge-IP-Match.

**DoD 20a:** Chip warnt bei Überlast; Dialog zeigt Sessions; Pipeline unberührt; `cargo test` + `npm run tauri dev` (Windows). ✅

**Agent-Prompt:**

```
Implementiere Phase 20 Teilphase 20a aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 20a (Diagnose + Clients-Chip-Warning + Dialog read-only).
Kein Close/Kill. Windows-first; andere OS = unsupported Stub.
Danach cargo test && npm run tauri dev.
```

#### 20b — Manueller Safe-Close + Audit + Elevation

- [x] Config: `smb_session_idle_min_seconds`
- [x] Command: einzelne oder „alle sicheren Idle“ schließen — Filter strikt Idle ≥ Min **und** `NumOpens == 0`
- [x] Confirm-Dialog im UI (Anzahl, Kriterien kurz erklärt); Abbruch ohne Side-Effect
- [x] Elevation: bei fehlenden Rechten verständliche Meldung; Close nicht still fehlschlagen
- [x] Audit-Log über bestehendes Logging (jeder Close-Versuch + Ergebnis)
- [x] Dialog: Aktionen „Idle schließen…“; keine Auto-Schleife
- [x] Tests: Filter-Prädikat (Idle/NumOpens); Ablehnung unsicherer Sessions

**Nicht in 20b:** Auto-Close; Share-scoped Filter; Soft-Policy-Doku außer kurzer UI-Hinweiszeile.

**DoD 20b:** Operator kann sichere Idle-Sessions schließen; aktive/`NumOpens>0` bleiben; Log vorhanden. ✅

**Agent-Prompt:**

```
Implementiere Phase 20 Teilphase 20b aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 20b (manueller Safe-Close + Audit + Elevation-UX).
Kein Auto-Close. Kein Share-Filter (→ 20c).
Danach cargo test && npm run tauri dev.
```

#### 20c — Härtung: Share-Filter, Bridge-Hinweis, optional Auto-Close, Soft-Policy

- [x] Share-Fokus: wo möglich OpenFiles/Sessions am `monitor_path` / Share `aktuell` priorisieren oder kennzeichnen; andere Shares nicht pauschal killen
- [x] Bridge-Korrelation: Client-IP ↔ bekannte ATS-Hosts (Presence) als Hinweis in der Liste (Hostname/„ATS?“); **kein** Kill nur wegen Presence
- [x] Config: `smb_session_auto_close_enabled` (default `false`); wenn an: periodisch nur Safe-Close-Kandidaten; weiter loggen
- [x] Settings: Schwellen, Idle-Min, Auto-Close-Toggle, Poll; kurzer Hinweis Soft-Policy (OS-Idle-Timeout / Server-Limits)
- [x] Kurz-Doku in Plan/Release-Notiz oder Settings-Hilfetext: App-Kill = Notnagel
- [x] Tests: Auto-Close nur Safe-Kandidaten; Toggle-default-aus; Share-Kennzeichnung soweit testbar

**Nicht in 20c:** macOS/Linux-Kill; NAS-Remote-Admin; Pipeline-Blocking; ATS-Repo-Änderungen.

**DoD (Phase 20 gesamt)**

- [x] Windows: Diagnose + Chip-Warning + manueller Safe-Close
- [x] Badge-Zahl = ATS; Warning-State = SMB-Druck
- [x] Safe-Close-Regeln eingehalten; Audit vorhanden
- [x] Auto-Close optional, default aus
- [x] Kein Monitor/Upload-Gate
- [x] Elevierter Helper (20d): List/Close on-demand via UAC; AMS uneleviert; kein Auto-UAC
- [x] `cargo test` + manuelle Abnahme Windows (Sessions sehen → Idle schließen → ATS-Write ungestört)

**Abhängigkeiten:** Phase 13 P5+ (Clients-Dialog/Presence), bestehende Share-Utils.  
**Nicht-Ziele:** Cross-Platform-Kill; aggressives Auto-Kill; Share auf anderem Host verwalten; PowerShell-UI-Automation.

**Agent-Prompt:**

```
Implementiere Phase 20 Teilphase 20c aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 20c (Share-Filter, Bridge-Hinweis, optional Auto-Close default aus, Soft-Policy).
Danach cargo test && npm run tauri dev.
```

#### 20d — Elevierter SMB-Helper (AMS bleibt uneleviert)

**Status:** ✅  
**Technik (gewählt):** Option **A** — eigenes Binary `ams-smb-helper` im Bundle (`externalBin`); Launch via `ShellExecuteEx` Verb `runas`; JSON-IPC unter `%TEMP%` (kein stdout bei Elevation).  
**Problem:** `NetSessionEnum` / `NetSessionDel` brauchen auf dem SMB-Server typischerweise Admin. AMS soll **dauerhaft normal** laufen; AMS schließen und „Als Administrator“ neu starten ist **keine** Betriebsoption.  
**Ziel:** Bei Bedarf nur den SMB-List/Close-Pfad elevieren (UAC einmalig), Ergebnis zurück an den laufenden AMS-Hauptprozess; Safe-Close-/Share-Fokus-/Audit-Regeln aus 20a–20c bleiben verbindlich.

**Produktregeln (verbindlich):**

1. **AMS-Hauptprozess bleibt uneleviert** — kein dauerhaftes Run-as-Admin, kein Neustart der App nur für SMB.
2. **On-Demand Elevation:** List und/oder Close starten einen kurzen elevierten Helper; UAC-Consent durch den Operator.
3. **Helper-Scope eng:** Nur SMB-Session-Diagnose + Safe-Close (+ Share-Fokus wie 20c). Kein allgemeiner Admin-Shell, kein PowerShell-Spawn, kein Pipeline-/Config-Zugriff außer übergebene Parameter.
4. **Gleiche Semantik wie In-Process:** Snapshot-/Close-JSON kompatibel zu bestehenden Types; Safe-Close nur Idle ≥ Min und `NumOpens == 0`; Bulk/Auto nur Fokus-Share wenn bekannt; ATS-Hinweis bleibt Diagnose (kein Kill-Gate).
5. **Auto-Close uneleviert:** Wenn Permission Denied und Auto-Close an → **nicht** still UAC spammen. Auto-Close nur in-process; Default: überspringen + Chip/Status „Admin nötig“.
6. **UAC-Abbruch:** Cancel ohne Side-Effect; UI klar („Abgebrochen“ vs. „Zugriff verweigert“).
7. **Kein Console-Flash:** Helper als `windows_subsystem` Binary; Spawn mit `SW_HIDE`; UAC-Dialog selbst ist erlaubt/nötig.
8. **Audit:** Close weiterhin über AMS-Logging (Mode `manual` / `auto` / `elevated-helper`); Helper-Exit und Fehlercode mitloggen.
9. **Plattform:** Windows-only. macOS/Linux: unverändert Stub / Button ausgeblendet oder disabled mit Hinweis.
10. **Security:** Helper nur neben AMS-Exe / Bundle-`externalBin`; Argumente strikt whitelisten (`list` | `close-id` | `close-safe-idle`); keine Shell-Metazeichen; `--out` nur unter `%TEMP%`.

**Umsetzung:**

- [x] Binary `ams-smb-helper` (`src-tauri/src/bin/ams_smb_helper.rs`) + CLI `util/smb_helper.rs`
- [x] Parent-Spawn `util/smb_elevate.rs` (`runas`, Timeout 60s, JSON temp file)
- [x] Commands: `get_smb_session_snapshot_elevated`, `close_smb_session_elevated`, `close_safe_idle_smb_sessions_elevated`
- [x] UI: CTA „Mit Admin-Rechten laden/schließen…“; Dialog-lokaler Elevated-Snapshot (kein Auto-Re-Elevate alle 30s)
- [x] Bundle: `externalBin` + `scripts/prepare-smb-helper.mjs`; Placeholder in `build.rs` für Dev/Test
- [x] Unit-Tests: Arg-Whitelist, Quote/Params, UAC-Cancel-Meldung (kein echter UAC in CI)

**DoD 20d**

- [x] AMS läuft uneleviert; Operator kann ohne App-Neustart Sessions listen (nach UAC)
- [x] Safe-Close (einzeln + Idle-Bulk) über denselben Helper; unsichere Sessions bleiben
- [x] UAC-Cancel und Helper-Fehler verständlich; kein stiller Fehlschlag
- [x] Auto-Close spammt keine UAC-Prompts
- [x] Audit vorhanden; Pipeline unberührt
- [x] `cargo test` + manuelle Abnahme Windows (uneleviertes AMS → UAC → Liste → Idle schließen)

**Nicht in 20d:** Dauerhaft elevierter Hintergrunddienst; Scheduled Task als Admin ohne Consent; macOS/Linux-Kill; NAS-Remote-Admin; PowerShell-UI-Automation; ATS-Repo.

**Abhängigkeiten:** 20a–20c (Snapshot/Close/Share-Fokus/Settings).  
**Risiken:** Code-Signing/SmartScreen für Helper; Tauri-Bundle-Pfad in Dev vs. installiert; Antivirus auf `runas`.

**Agent-Prompt:**

```
Implementiere Phase 20 Teilphase 20d aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 20d (elevierter SMB-Helper: List/Close on-demand, AMS bleibt uneleviert).
Kein Dauer-Admin, kein Auto-UAC-Spam, kein PowerShell-Spawn.
Danach cargo test && npm run tauri dev (Windows).
```

---

### Phase 21 — Auto-Nachreichen bei gleicher Kunden-/Booking-ID

**Status:** ✅ **21a** ✅ · **21b** ✅ · **21c** ✅ · **21d** ✅  
**Ziel:** Ein erneuter Upload (neuer lokaler/ATS-Ordner) mit **gleicher** `customer_number`/`kunden_id` + `booking_number`/`booking_id` gilt als **Nachreichen** an den ersten erfolgreichen Cloud-Vorgang: gleicher Dropbox-Root, gleiche Cloud-Order, gleicher Kundenlink — auch wenn der Ordnername anders heißt. Falsche Dateien löscht der Operator manuell in Dropbox/Cloud.

**Betriebsannahme (abgestimmt):**

1. Gleiche IDs = **derselbe fachliche Vorgang** (nicht neue Auslieferung).
2. Zweiter Upload → Inhalt landet im **Parent-`remote_path`** des ersten Erfolgs.
3. Neue Dateien **daneben**; Namenskollision → umbenennen (`(1)`, `(2)`, …), **nicht überschreiben**.
4. Kundenlink (`final_url` / History-`share_link`) bleibt der des Parents; nach Erfolg **Notify mit demselben Link** (im Gegensatz zu Phase-15-Append ohne Mail).
5. Explizites Manifest `kind=append` (Phase 15) bleibt unverändert und hat Vorrang vor ID-Match.

**Problem heute:** AMS lädt in `/{neuer_dir_name}` hoch; Cloud liefert oft dieselbe `final_url` zur Booking-ID → Link zeigt auf den **alten** Ordner, neue Dateien sind dort nicht sichtbar.

**Referenz (nur lesen / erweitern):**

```
docs/HANDOFF.md §6.1                          (explizites Append)
src-tauri/src/upload/append.rs                (AppendTarget, Parent-Resolve, unique_filename lokal)
src-tauri/src/monitor/service.rs              (Claim → resolve_claimed_append_target)
src-tauri/src/upload/worker.rs                (process_append_job vs. run_single_job)
src-tauri/src/storage/history.rs              (customer_number, booking_number, remote_path, order_id)
src-tauri/src/cloud/custom_api/upload.rs      (manifest, existing_order_id, root_share_link)
src-tauri/src/cloud/dropbox.rs                (Upload: autorename derzeit false)
src-tauri/src/cloud/manifest.rs               (build_manifest_v11)
ATS: video/append_job.rs, handoff_manifest.rs (explizites Nachreichen — optional parallel)
```

**Produktregeln (verbindlich):**

| # | Regel |
|---|--------|
| 1 | Parent nur wenn Historie **Erfolgreich** und `remote_path` nicht leer; bevorzugt mit `order_id` / `share_link`. |
| 2 | Match-Schlüssel: trim `customer_number` + `booking_number`; zusätzlich `customer_type` wenn beide Seiten gesetzt (sonst nur IDs). Beide IDs müssen nicht-leer sein. |
| 3 | Mehrere erfolgreiche Treffer → **neuesten** erfolgreichen Parent (nach `finished_at` / id); Log-Warnung. |
| 4 | Bereits `kind=append` / `_nachreichung_`-Ordner → bestehende Phase-15-Pipeline (kein zweites ID-Match). |
| 5 | Ohne Parent-Treffer → normaler Erst-Upload (wie heute). |
| 6 | Dropbox-Binding vom Parent (Phase 16), nicht vom Soft-Active — wie Append. |
| 7 | Remote-Kollision: vor/während Upload prüfen; Zielname `name (1).ext` (bzw. `(2)`…); lokal gestagte Append-Namen analog. |
| 8 | Custom API: Manifest mit `existing_order_id` + Root = Parent-`remote_path`; `final_url` = Parent-Link. |
| 9 | **Notify an** nach Auto-ID-Append (gleicher Link). Phase-15-explizit: Notify weiter **aus**. |
| 10 | History: Ereignis am Parent (`append_events` / append_count); Quell-Ordner archivieren wie Append-Job. |
| 11 | Infobroschüre: wie Append **nie** injizieren. |
| 12 | Pure-Contact ohne beide IDs: kein Auto-Match. |

**Slices (eine pro Session):** 21a → 21b → 21c → 21d

#### 21a — History-Lookup + AppendTarget aus ID-Match

- [x] `HistoryStore`: `find_successful_by_customer_booking(customer, booking, type?)` → neuester Erfolg mit `remote_path`
- [x] Hilfsfunktion `append_target_from_id_match(kunde) -> Option<AppendTarget>` (oder über History-Entry + bestehendes `append_target_from_parent_entry`)
- [x] Klare Fehler/None-Fälle: IDs leer, kein Treffer, Parent ohne `remote_path`
- [x] Unit-Tests: Match, Type-Filter, Newest-Wins, leere IDs, fehlender remote_path

**Nicht in 21a:** Monitor-Claim-Umbau; Dropbox-Rename; Notify; UI; Cloud/ATS-Repos.

**DoD 21a:** Lookup + AppendTarget rein testbar; Pipeline unverändert. ✅  
**Agent-Prompt:**

```
Implementiere Phase 21 Teilphase 21a aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 21a (History-Lookup + AppendTarget aus Kunden-/Booking-ID).
Kein Claim-Routing, kein Remote-Rename, kein Notify.
Danach cargo test && npm run tauri dev.
```

#### 21b — Claim/Enqueue: Auto-Route in Append-Pipeline

- [x] In `try_claim_and_enqueue` (nach Marker/Kunde, vor normalem Enqueue): wenn kein explizites Append → ID-Match → Job mit `append: Some(AppendTarget)`
- [x] Status/Log: „Auto-Nachreichen an \<parent_dir\> (gleiche Kunden-/Booking-ID)“
- [x] Outbox/History-Updates wie Append (Parent-Events); Binding vom Parent
- [x] Gate: Parent nicht bereit → **nicht** Claim als Erst-Upload missbrauchen; klarer Status/Code (z. B. `id_append_parent_not_ready`) oder Fallback nur wenn spezifiziert — **Default: kein Claim**, Ordner liegen lassen / Fehler sichtbar (kein stiller Zweit-Ordner)
  - *Abweichung nur wenn bewusst dokumentiert:* fehlender Parent = Erst-Upload. **Entscheidung:** fehlender Erfolg → normaler Erst-Upload; Parent existiert aber nicht `Erfolgreich` → kein Claim / reject (wie Append-Gate).
- [x] Unit-/Integration-nahe Tests: Claim baut AppendTarget; explizites `kind=append` hat Vorrang; ohne IDs kein Match

**Nicht in 21b:** Remote-`(1)`-Rename (→ 21c); Notify-Unterschied (→ 21c); ATS-Repo.

**DoD 21b:** Zweiter Job mit gleichen IDs enqueued als Append auf Parent-`remote_path`. ✅  
**Agent-Prompt:**

```
Implementiere Phase 21 Teilphase 21b aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 21b (Claim/Enqueue Auto-Route auf Append-Pipeline).
Kein Remote-Rename, kein Notify-Umbau.
Danach cargo test && npm run tauri dev.
```

#### 21c — Remote-Namenskollision + Notify für Auto-ID-Append

- [x] Dropbox/Custom-Direct: vor Upload Dateiname remote prüfen; bei Konflikt `stem (n).ext` wählen (n=1…); Logging
- [x] Session-/Pfad-Upload: gleiche Semantik (kein `autorename: true` als alleinige Lösung, wenn Manifest-`rel_path` mitziehen muss — Manifest-Pfade nach Rename aktualisieren)
- [x] `process_append_job` / Worker: Flag oder Erkennung **Auto-ID-Append** vs. Phase-15-Append
  - Auto-ID: nach Erfolg `notify_after_upload` mit Parent-`share_link` (wie Erst-Upload)
  - Phase 15 / manuelles History-Nachreichen: Notify weiter aus
- [x] Optional History-Feld/Extra: `append_reason: "id_match" | "manifest" | "operator"` für UI/Debug
- [x] Tests: Kollisionsnamen; Notify-Zweig nur bei id_match; Manifest-rel_path nach Rename

**Nicht in 21c:** Cloud-Server-Code; ATS-Export-Hinweis (→ 21d).

**DoD 21c:** Keine stillen Dropbox-Overwrite; Auto-ID sendet gleichen Link erneut. ✅  
**Agent-Prompt:**

```
Implementiere Phase 21 Teilphase 21c aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 21c (Remote-Kollision (1)/(2) + Notify nur für Auto-ID-Append).
Kein ATS-Repo, kein Cloud-Deploy.
Danach cargo test && npm run tauri dev.
```

#### 21d — UX/Docs + Cloud-Vertrag + optionales ATS

**AMS UI/Docs**

- [x] History: am Parent sichtbar „Auto-Nachgereicht …“; Quellordner-Name in `append_events`
- [x] Kurzer Status in App-Shell bei Auto-Route (`Auto-Nachreichen: …` / `Auto-Nachgereicht: …`)
- [x] [`HANDOFF.md`](./HANDOFF.md): §6.1b (ID-Match-Auto-Append; Verweis Phase 21; Notify vs. explizitem Append)
- [x] Plan/AGENTS: Slice-Referenzen; manuelle Abnahme-Checkliste

**Cloud (Partner-Repo — Checklist, nicht AMS-Code):**

Dokumentiert für Cloud-Team / Partner-Repo (kein Deploy in diesem Repo):

- [ ] `orders/create` mit `existing_order_id`: Dateien an bestehende Order; **`final_url` unverändert**
- [ ] Gleiche customer+booking ohne `existing_order_id`: bestehende Order wiederverwenden **oder** klarer Fehler (kein stilles „Portal = Alt, Dropbox = Neu“)
- [ ] Neue `rel_path`s unter bestehendem Root akzeptieren; kein erzwungenes Überschreiben gleichnamiger Dateien serverseitig
- [ ] Optional: Lookup-Endpoint „aktive Order zu customer+booking“ für Diagnose

**ATS (optional, ATS-Repo / eigene Session — kein Pflicht-Feature in 21d):**

Dokumentiert; AMS 21a–c funktioniert ohne ATS-Änderung:

- [ ] Beim Export mit beiden IDs: Hinweis oder Soft-Warnung „AMS hängt an bestehenden Vorgang an, wenn dort schon Erfolg existiert“
- [ ] Optional: wenn Parent-`correlation_id` bekannt → weiter explizites `kind=append` (Phase 15) statt nur ID-Match — robuster, Outbox klarer
- [x] Kein Zwang: AMS-21a–c muss auch ohne ATS-Änderung funktionieren (Legacy-Marker / neuer Vorgang-Ordner)

**Manuelle Abnahme (Phase 21)**

1. Erst-Upload mit Kunden- + Booking-ID → Erfolg, Share-Link notieren.
2. Zweiten Ordner (anderer Name) mit **gleichen** IDs ablegen → Monitor claimt als Auto-Nachreichen.
3. App-Shell: Status „Auto-Nachreichen: Parent …“ / danach „Auto-Nachgereicht: …“.
4. History am Parent: Detail „Nachgereicht … · Auto-Nachgereicht“; Timeline „Auto-Nachgereicht: Quellordner“.
5. Dropbox/Cloud: Dateien unter dem **ersten** Root; bei Namenskollision `(1)` / `(2)`.
6. Kunde erhält Notify erneut mit **gleichem** Link (Auto-ID); bei explizitem `kind=append` **keine** Notify.
7. Binding/Broschüre/Pause-Cancel verhalten sich wie Append.

**DoD (Phase 21 gesamt)**

- [x] Zweiter Upload gleiche IDs → Parent-Ordner + gleicher Link; `(1)` bei Namenskollision
- [x] Explizites Append unverändert (keine Notify)
- [x] Auto-ID-Append: Notify mit Parent-Link
- [x] Binding/Broschüre/Pause-Cancel wie Append
- [x] HANDOFF + Plan aktualisiert; Cloud-Checklist dokumentiert
- [x] `cargo test` + manuelle Abnahme-Checkliste (siehe oben)

**Abhängigkeiten:** Phase 14/15 (Append), 5 (Custom API Manifest), 16 (Binding), 8 (Notify/Resend).  
**Nicht-Ziele:** Automatisches Löschen falscher Dateien; neuer Kundenlink bei gleichen IDs; Überschreiben gleichnamiger Remote-Dateien; Erzwingen von ATS-`kind=append`; Cloud-Code in diesem Repo deployen.

**Agent-Prompt:**

```
Implementiere Phase 21 Teilphase 21d aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Nur 21d (History/UX-Hinweise, HANDOFF §6.1b, Cloud-Checklist im Plan).
Optional ATS nur dokumentieren oder Minimal-Hinweis — kein Pflicht-ATS-Feature in dieser Session.
Danach cargo test.
```

---

## 10. Teststrategie

- Rust Unit-Tests für Marker, Status, Payload-Builder, Checkpoint-Logik
- Ab Phase 13: Manifest-Validierung, Gate (Legacy vs. Handoff), Ignore `.ams-handoff`
- Ab Phase 17: Broschüre-Injektion (Erst-Upload only, Append skip, Idempotenz, 5 MB-Limit)
- Ab Phase 18: SemVer/Prerelease-Ordering, Changelog Beta-Snapshot, `resolve_best_update`
- Ab Phase 19: Crew/Alias-Match, Ordnername-Predictor (Gold-Set), ID-Marker-JSON, Medien-Layout-Move, Manifest nach AMS-Assign; ab **19e** Gast-Exclude (Post-`TA`-Zone + Kundennamen)
- Ab Phase 20: SMB-Session-Snapshot/Schwelle, Safe-Close-Filter (`Idle` + `NumOpens==0`); Close nur hinter Feature-Flag/Manual in Tests mocken; ab **20d** elevierter Helper (Spawn/`runas`, JSON-IPC) mocken — kein echter UAC in CI
- Ab Phase 21: ID-Match Parent-Lookup; Claim→Append-Route; Remote-Kollision `(1)`; Notify nur Auto-ID (nicht Phase-15-Append)
- Legacy `_test_*.py` als Spezifikation, nicht ausführen
- Manuelle Abnahme: Monitor → Upload → Notify → Archiv
- Ab Phase 10: CI auf Win/Mac/Linux

---

## 11. Build & Deployment

```powershell
npm run tauri build
```

Updater-Endpoint und Signing: siehe [`docs/RELEASE.md`](./RELEASE.md) (analog AeroTandemStudio-v2).

---
## 12. Fortschritts-Tracker

| Phase | Thema | Status |
|-------|--------|--------|
| 0 | Scaffold & Docs | ✅ |
| 1 | Config, Secrets, Logging, Events | ✅ |
| 2 | Marker & Kunde | ✅ |
| 3 | Monitor + Stability | ✅ |
| 4 | Upload + Dropbox | ✅ |
| 5 | Checkpoint + Custom API | ✅ |
| 6 | Notifications | ✅ |
| 7 | History-UI | ✅ |
| 8 | Retry / Resend / Manual | ✅ |
| 9 | Settings + Shell | ✅ |
| 10 | Updater / CI / Plattformen | ✅ |
| 11 | Polish | ✅ |
| 12 | Kundenaufnahme & Marker-Zuweisung | ✅ |
| 13 | ATS↔AMS Handoff | 🔄 P0–P4 ✅ · L4 UX ✅ · P5+ Presence ✅ · P6 Docs ✅ · P6a ✅ · P6b–d offen |
| 14 | Medien nachreichen | ✅ |
| 15 | ATS-Nachreichen (Append) | ✅ |
| 16 | Multi-Dropbox-Konten (Native + Custom-API) | ✅ 16a ✅ · 16b ✅ · 16c ✅ · 16d ✅ |
| 17 | Infobroschüre PDF (Erst-Upload) | ✅ |
| 18 | Release-Kanäle (Beta / Stable / Auto-Latest) | ✅ |
| 19 | Kundenaufnahme ID-Flow + Job-Ordner-Normalisierung | ✅ 19a–19e |
| 20 | SMB-Session-Diagnose & Idle-Cleanup (Windows) | ✅ 20a–20d |
| 21 | Auto-Nachreichen bei gleicher Kunden-/Booking-ID | ✅ 21a–21d |
