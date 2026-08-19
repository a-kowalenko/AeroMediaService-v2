# Aero Media Service v2 — Architektur

> Kurzübersicht. Details: [IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md) · Handoff ATS↔AMS: [HANDOFF.md](./HANDOFF.md)

## Stack

| Schicht | Technologie |
|---------|-------------|
| Shell | Tauri 2 |
| Backend | Rust (`src-tauri/`) |
| Frontend | React 19 + TypeScript (`src/`) |
| HTTP | `reqwest` + `tokio` |
| Secrets | OS-Keyring (`keyring`) |
| Storage | SQLite (`rusqlite`, ab Phase 1/7) |
| E-Mail | `lettre` |
| Auto-Update | Tauri Updater Plugin |
| Plattformen | Windows 10+, macOS, Linux |

## Window Chrome

Custom titlebar (Phase 11):

- **Windows / Linux:** `decorations: false` at create time (`tauri.conf.json`) + Min/Max/Close in `AppChrome`. Startup clamps the window to the monitor work area (`src-tauri/src/util/window_fit.rs`) so the bottom edge cannot sit below the taskbar.
- **macOS:** `tauri.macos.conf.json` sets `decorations` + `titleBarStyle: Overlay` + `hiddenTitle` at create time (no false→true toggle) + left inset; no custom close buttons
- Rollback: `localStorage.setItem('ams-custom-titlebar', '0')` then reload

## Projektstruktur

```
AeroMediaService-v2/
├── src/                        # React Frontend
│   ├── App.tsx
│   ├── components/
│   ├── hooks/
│   ├── store/
│   └── lib/
├── src-tauri/
│   ├── src/
│   │   ├── monitor/            # Ordnerüberwachung, Stability
│   │   ├── bridge/             # Optional LAN Bridge (health, lookup, jobs, ready, mDNS)
│   │   ├── upload/             # Queue, Control, Checkpoint
│   │   ├── cloud/              # Dropbox, Custom API
│   │   ├── notify/             # E-Mail, SMS, WhatsApp
│   │   ├── storage/            # Config, Secrets, History, Customers, Logging
│   │   ├── model/              # Kunde, Marker, Status, Handoff
│   │   ├── util/               # Archive, Shortener, Window-Fit
│   │   └── commands/           # Tauri IPC
│   └── icons/
├── docs/
│   ├── IMPLEMENTATION_PLAN.md  # ← Hauptdokument
│   ├── ARCHITECTURE.md         # ← Dieses Dokument
│   ├── HANDOFF.md              # ATS↔AMS Share-Handoff (Phase 13)
│   └── MIGRATION.md
└── AGENTS.md
```

## Datenfluss

```
ATS (anderer PC) ──SMB──► Share „aktuell“ / <Job>/
    │ schreibt Medien → Manifest → _fertig.txt
    │
Kunden-UI AMS (Aufnahme / Warteschlange)
    │ schreibt Pure-Contact-_fertig.txt (ohne Manifest = Legacy)
    ▼
Monitor-Ordner (monitor_path = aktueller Share)
    │ Manifest-Gate? (Phase 13) sonst Ordner stabil?
    ▼
Marker lesen (_fertig.txt / API-Marker)
    │ Kunde aus Marker ODER Custom-API-Lookup
    ▼
Upload-Queue (tokio)
    │ DropboxClient ODER CustomApiClient
    ▼
Share-Link (+ optional Shortener)
    │
    ▼
E-Mail / SMS / WhatsApp
    │
    ▼
History aktualisieren → Archiv (erfolg / fehler / abgebrochen)
    │ Events (+ optional Outbox .ams-handoff / Bridge)
    ▼
React UI (Log, History, Kunden, Status, Progress)
```

Handoff-Details (Manifest, Outbox, optionale LAN-Bridge): [HANDOFF.md](./HANDOFF.md).

## Legacy-Referenz

```
C:\Users\Kowalenko\PycharmProjects\AeroMediaService
```

Nur lesen — niemals editieren. Siehe `AGENTS.md` und `MIGRATION.md`.

## Config-Speicherort (Ziel)

- Windows: `%LOCALAPPDATA%\AeroMediaService\`
- macOS: `~/Library/Application Support/AeroMediaService/`
- Linux: `~/.local/share/AeroMediaService/`

Secrets: OS-Keyring, Service-Name z. B. `AeroMediaService-v2` (Legacy: `DropboxUploaderApp`).
