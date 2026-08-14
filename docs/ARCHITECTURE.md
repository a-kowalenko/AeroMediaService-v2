# Aero Media Service v2 — Architektur

> Kurzübersicht. Details: [IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md)

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
│   │   ├── upload/             # Queue, Control, Checkpoint
│   │   ├── cloud/              # Dropbox, Custom API
│   │   ├── notify/             # E-Mail, SMS, WhatsApp
│   │   ├── storage/            # Config, Secrets, History, Logging
│   │   ├── model/              # Kunde, Marker, Status
│   │   ├── util/               # Archive, Shortener, Validation
│   │   └── commands/           # Tauri IPC
│   └── icons/
├── docs/
│   ├── IMPLEMENTATION_PLAN.md  # ← Hauptdokument
│   ├── ARCHITECTURE.md         # ← Dieses Dokument
│   └── MIGRATION.md
└── AGENTS.md
```

## Datenfluss

```
Monitor-Ordner
    │ Ordner stabil?
    ▼
Marker lesen (.fertig / Upload-Marker)
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
    │ Events
    ▼
React UI (Log, History, Status, Progress)
```

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
