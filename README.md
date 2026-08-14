# Aero Media Service v2

Neuentwicklung von **Aero Media Service** mit **Tauri 2 + Rust + React 19 + TypeScript**.

Legacy (nur lesen): `C:\Users\Kowalenko\PycharmProjects\AeroMediaService`

## Docs

- [Implementierungsplan](docs/IMPLEMENTATION_PLAN.md) — Hauptdokument
- [Architektur](docs/ARCHITECTURE.md)
- [Migration-Mapping](docs/MIGRATION.md)
- [Agent-Regeln](AGENTS.md)

## Entwicklung

```powershell
cd C:\Users\Kowalenko\PycharmProjects\AeroMediaService-v2
npm install
npm run tauri dev
```

Rust-Tests:

```powershell
npm run test:rust
```

## Aktueller Stand

Phase 0 (Scaffold & Docs) erledigt. Nächste Phase: Config, Secrets, Logging, Events.
