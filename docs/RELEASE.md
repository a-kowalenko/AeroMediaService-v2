# Releases & Auto-Update

Source: privates Repo (dieses Projekt)  
Binaries: öffentliches Repo [`a-kowalenko/aero-media-service-releases`](https://github.com/a-kowalenko/aero-media-service-releases)

Updater-Endpoint:

```text
https://github.com/a-kowalenko/aero-media-service-releases/releases/latest/download/latest.json
```

Pubkey: `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`  
Private Key: lokal `src-tauri/keys/updater.key` (gitignored) — Inhalt als GitHub Secret hinterlegen.

## Secrets (privates Repo)

| Secret | Pflicht |
|--------|---------|
| `RELEASES_GITHUB_TOKEN` | ja — PAT, Contents R/W auf Releases-Repo |
| `TAURI_SIGNING_PRIVATE_KEY` | ja — Inhalt von `src-tauri/keys/updater.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | optional (leer, wenn Key ohne Passwort) |
| Apple / Windows Authenticode | nein (optional) |

## Neuen Release erstellen (empfohlen)

Voraussetzung: **sauberer** Working Tree auf `master`/`main`, synchron mit `origin`.

### IDE (Play)

Run Configuration **Release** (`.run/Release.run.xml`) -> Play.  
Im Terminal: `patch` / `minor` / `major` wählen, mit `y` bestätigen.

Das Skript setzt die Version in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` und `src-tauri/tauri.conf.json`, committed `release: x.y.z`, taggt `vx.y.z` und pusht Branch + Tag.

### Terminal

```powershell
npm run release
```

Danach: Actions -> Workflow **release** (Win + zwei Mac-Jobs + Ubuntu AppImage); öffentliches Repo -> Releases prüfen.

Neue Releases bekommen **kein** „Latest“-Label. Erst nach manueller Promotion greifen Installer-Links und Auto-Update (`/releases/latest/`).

### Lokaler Windows-Build ohne Release

```powershell
npm run build:win
```

Das baut lokal nur das Windows-NSIS-Setup. Ohne `TAURI_SIGNING_PRIVATE_KEY` werden die Updater-Artefakte automatisch über `src-tauri/tauri.conf.ci.json` deaktiviert.

## Plattform-Artefakte

| OS | Bundle |
|----|--------|
| Windows | NSIS `-setup.exe` (+ Updater-Signatur) |
| macOS | `.dmg` (aarch64 + x64) |
| Linux | `.AppImage` (amd64) |

## Hinweise

- Das öffentliche Releases-Repo braucht **mindestens einen Commit** auf dem Default-Branch (z. B. README).
- macOS ohne Apple Developer Account: nicht notarisiert (Gatekeeper-Warnung möglich).
- Windows ohne Authenticode: ggf. SmartScreen-Warnung.
- Auto-Update nutzt die Tauri-Updater-Signatur (Pubkey), unabhängig von OS-Code-Signing.
- PR-CI bleibt bewusst leichtgewichtig; volle Bundle-Builds laufen nur in `release.yml`.
- App-Datenpfade bleiben stabil: `%LOCALAPPDATA%\AeroMediaService\` / `~/Library/Application Support/AeroMediaService/` / `~/.local/share/AeroMediaService/`
- Keyring-Service: `AeroMediaService-v2`

## Neuen Signing-Key erzeugen (nur wenn nötig)

```powershell
npx tauri signer generate -w src-tauri/keys/updater.key --ci
```

Pubkey in `tauri.conf.json` übernehmen; Private Key **nie** committen.
