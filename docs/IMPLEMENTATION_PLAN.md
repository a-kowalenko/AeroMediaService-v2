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
| ATS↔AMS Handoff (Docs P0) | 🔄 Phase 13 — Spec: [`HANDOFF.md`](./HANDOFF.md) · P0 ✅ · P1 ✅ · P1b ✅ · P2 ✅ · P3 ✅ · P4 ✅ · L4 UX ✅ · P5+ offen |
| Medien nachreichen (bestehende Order) | ✅ Phase 14 |
| ATS-Nachreichen (Append-Handoff) | ✅ Phase 15 |

**Nächste Phase:** 13 P5+ (optional) — Spec: [`HANDOFF.md`](./HANDOFF.md)

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
- [ ] **P5+** — optional: SHA-256, strict extras, …

#### AMS-Gate (P1) — Kurz

Vor `claim_fertig_marker`: Manifest gültig → Claim (Stability verkürzen/überspringen); incomplete → liegen lassen + `rejected`; kein Manifest → Legacy inkl. Stability. `manifest_required` default `false`.

#### Partner-Repo

```
C:\Users\Kowalenko\PycharmProjects\AeroTandemStudio-v2
```

P1/P1b/P2/P3 erfordern Änderungen in **beiden** Repos; pro Session nur **eine** Teilphase (P1, P1b, …).

#### Erfolgskriterien (gesamt Phase 13)

- [x] Manifest-Handoff auf SMB-Share verhindert Claim unvollständiger Jobs
- [x] Legacy-Ordner ohne Manifest und AMS-Kunden-UI (Pure-Contact-Marker) weiter funktionsfähig
- [x] Outbox-Status für ATS ohne gleichen PC lesbar
- [x] Bridge optional; Datei-Pfad allein ausreichend
- [x] `cargo test` grün (AMS; ATS analog in ATS-Repo)

#### Agent-Prompt (nächste Session = P5+)

```
Implementiere Phase 13 Teilphase P5+ aus @docs/IMPLEMENTATION_PLAN.md
Spec: @docs/HANDOFF.md
Regeln: @AGENTS.md
Scope vorher klären (SHA-256 / strict extras / …).
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
| `selected_cloud_service` | `dropbox` \| `custom_api` | `dropbox` |
| `smtp_*` / Sandbox-Flags | E-Mail | — |

### Secrets (Keyring)

| Key | Beschreibung |
|-----|--------------|
| `db_app_key` / `db_app_secret` / `db_refresh_token` | Dropbox |
| `custom_db_app_key` / `custom_db_app_secret` / `custom_db_refresh_token` | Custom-API Dropbox |
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

## 10. Teststrategie

- Rust Unit-Tests für Marker, Status, Payload-Builder, Checkpoint-Logik
- Ab Phase 13: Manifest-Validierung, Gate (Legacy vs. Handoff), Ignore `.ams-handoff`
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
| 13 | ATS↔AMS Handoff | 🔄 P0 ✅ · P1 ✅ · P1b ✅ · P2 ✅ · P3 ✅ · P4 ✅ · L4 UX ✅ · P5+ offen |
| 14 | Medien nachreichen | ✅ |
| 15 | ATS-Nachreichen (Append) | ✅ |
