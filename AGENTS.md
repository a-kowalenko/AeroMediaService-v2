# Aero Media Service v2 — Agent Rules

## Hauptdokument

**Implementierungsplan:** `@docs/IMPLEMENTATION_PLAN.md`  
**Architektur:** `@docs/ARCHITECTURE.md`  
**Migration-Mapping:** `@docs/MIGRATION.md`

In jedem neuen Kontextfenster `@docs/IMPLEMENTATION_PLAN.md` referenzieren und **nur eine Phase** implementieren.

---

## Stack

Tauri 2 + Rust + React 19 + TypeScript

Tailwind + shadcn/ui, Zustand, SQLite — ab Phase 1/7 schrittweise.  
HTTP: `reqwest` + `tokio`. Secrets: `keyring`. E-Mail: `lettre`.

Kein Python, kein PySide6, kein MoviePy/FFmpeg (nicht benötigt).

---

## Regeln

- **NIEMALS** Dateien im Legacy-Projekt ändern (nur lesen)
- Qt-Signals → Tauri Events; QThread → `tokio` Tasks
- Secrets **nie** in SQLite/Klartext — immer OS-Keyring
- Upload Pause / Resume / Cancel muss erhalten bleiben
- Unit-Tests für Marker-Parsing, Status-Logik, API-Payloads
- Nach Änderungen: `cargo test` und `npm run tauri dev`
- **Eine Phase pro Session** — Scope nicht erweitern
- Verhalten aus Legacy portieren, nicht 1:1 copy-pasten
- Plattformen: **Windows + macOS + Linux**

---

## Projektpfade

| | Pfad |
|---|------|
| v2 (editieren) | `C:\Users\Kowalenko\PycharmProjects\AeroMediaService-v2` |
| Legacy (NUR LESEN) | `C:\Users\Kowalenko\PycharmProjects\AeroMediaService` |
| ATS-v2 (Vorbild Scaffold/CI) | `C:\Users\Kowalenko\PycharmProjects\AeroTandemStudio-v2` |

---

## Legacy-Referenz (NUR LESEN)

Basis: `C:\Users\Kowalenko\PycharmProjects\AeroMediaService`

### Kern-Dateien

| Legacy | v2 Modul | Phase |
|--------|----------|-------|
| `core/config.py` | `src-tauri/src/storage/config.rs` + `secrets.rs` | 1 |
| `core/upload_markers.py` | `src-tauri/src/model/marker.rs` | 2 |
| `models/kunde.py` | `src-tauri/src/model/kunde.rs` | 2 |
| `core/monitor.py` | `src-tauri/src/monitor/service.rs` | 3 |
| `core/uploader.py` | `src-tauri/src/upload/worker.rs` | 4 |
| `services/dropbox_client.py` | `src-tauri/src/cloud/dropbox.rs` | 4 |
| `services/custom_api_client.py` | `src-tauri/src/cloud/custom_api.rs` | 5–6 |
| `app.py` | `src/App.tsx` + Komponenten | 7–9 |
| `settings.py` | `src/components/SettingsDialog.tsx` | 1, 9 |
| QSettings / Keyring Legacy | `storage/legacy_migrate.rs` | 11 |

Vollständiges Mapping: `@docs/MIGRATION.md`

---

## Aktueller Stand

- ✅ Tauri 2 Scaffold (React + TypeScript)
- ✅ Phase 0: Scaffold, Docs, Minimal-UI
- ✅ Phase 1: Config, Secrets, Logging, Events
- ✅ Phase 2: Marker & Kundenmodell
- ✅ Phase 3: Ordner-Monitor + Stability
- ✅ Phase 4: Upload-Pipeline + Dropbox
- ✅ Phase 5: Checkpoints + Custom API (Upload-Kern)
- ✅ Phase 6: Notifications (E-Mail / SMS / WhatsApp)
- ✅ Phase 7: History-UI + Statusmodell
- ✅ Phase 8: Retry, Resend, Manual Status
- ✅ Phase 9: Settings vollständig + App-Shell
- ✅ Phase 10: Updater, Build, CI, Plattformen
- ✅ Phase 11: Polish (Wizard, Titlebar/Theme, History-Virtualisierung, Legacy-Migration)

**Nächster Schritt:** — (Phasen 0–11 erledigt)

---

## Schnell-Prompt für Agent

```
Implementiere Phase X aus @docs/IMPLEMENTATION_PLAN.md
Regeln: @AGENTS.md
Legacy: [Pfade aus Phase X im Plan]
Nur Phase X. Danach cargo test && npm run tauri dev.
```
