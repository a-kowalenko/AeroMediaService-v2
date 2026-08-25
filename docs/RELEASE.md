# Releases & Auto-Update

Source: privates Repo (dieses Projekt)  
Binaries: öffentliches Repo [`a-kowalenko/aero-media-service-releases`](https://github.com/a-kowalenko/aero-media-service-releases)

Updater-Endpoint:

```text
https://github.com/a-kowalenko/aero-media-service-releases/releases/latest/download/latest.json
```

Pubkey: `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`  
Private Key: lokal `src-tauri/keys/updater.key` (gitignored) — Inhalt als GitHub Secret hinterlegen.

Die App zeigt beim Update die **GitHub Release Body** (nicht die Notes in `latest.json`).
Quelle der Body: Abschnitt in `CHANGELOG.md` zur Version.

## Secrets (privates Repo)

| Secret | Pflicht |
|--------|---------|
| `RELEASES_GITHUB_TOKEN` | ja — PAT, Contents R/W auf Releases-Repo |
| `TAURI_SIGNING_PRIVATE_KEY` | ja — Inhalt von `src-tauri/keys/updater.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | optional (leer, wenn Key ohne Passwort) |
| Apple / Windows Authenticode | nein (optional) |

## Release-Notes (`CHANGELOG.md`)

Zielgruppe: **Operator am Upload-/Dropzone-PC** — derselbe Text landet im Update-Dialog und als GitHub-Release-Body.

### Ablauf

1. Nutzer-sichtbare Punkte unter **`## [Unreleased]`** eintragen (Bullet-Listen) — empfohlen bei jeder sichtbaren Änderung.
2. `npm run release` zeigt die Notes, legt `## [x.y.z] - Datum` an und committed `CHANGELOG.md` mit.
   - **`[Unreleased]` befüllt** → diese Notes werden für die neue Version verwendet.
   - **`patch` und `[Unreleased]` leer** → Notes der **aktuellen** Version (z. B. 0.1.5 → 0.1.6) werden übernommen.
   - **`minor` / `major` und leer** → Abbruch; eigene Notes unter Unreleased nötig.
3. Workflow `release.yml` liest den Abschnitt zur Tag-Version und setzt ihn als Body im öffentlichen Releases-Repo (auch bei erneutem Lauf: Body wird aktualisiert). Fehlt der Abschnitt am Tag, lädt CI `CHANGELOG.md` vom Default-Branch; `extract` kann bei Patch-Lücken auf die vorherige Same-Minor-Version zurückfallen.

Hilfsskript:

```bash
node scripts/changelog.mjs extract 0.1.5   # Preview der Notes für CI/Updater
```

Bereits veröffentlichtes Release nachträglich aktualisieren: Workflow **release** → *Run workflow* mit Tag (z. B. `v0.1.5`), nachdem `CHANGELOG.md` auf `master` liegt — oder Body im öffentlichen Releases-Repo manuell setzen.

### Struktur

```markdown
## [Unreleased]

### Neu
- …

### Verbessert
- …

### Behoben
- …

### Hinweis
- …   # optional, z. B. bekannte Einschränkungen
```

- Überschriften nur bei Bedarf; leere Abschnitte weglassen.
- Pro Bullet **ein** Nutzen in Alltagssprache (nicht Phasennummern, nicht Ticket-IDs).
- Länge: eher 5–12 Bullets pro Release; Details gehören in `docs/`, nicht in den Update-Dialog.

### Schreibstil (Pflicht)

| Ja | Nein |
|----|------|
| Begriffe wie in der UI (Historie, Nachreichen, Dropbox, Marker, Upload, Einstellungen) | Interne Kürzel: Phase 16, P3, OPT-*, … |
| Was der Nutzer merkt („Medien in denselben Cloud-Ordner nachreichen“) | Wie es technisch läuft („Checkpoint-Merge“, „Bridge-Outbox“) |
| Plattform nur wenn nötig („Windows & Mac“) | Entwickler-Stack („Tauri“, „reqwest“, „Zustand“) |

Buchungs-/Kundensuche und Übergabe aus dem Schnitt-Workflow **alltagssprachlich** beschreiben — Produktkürzel anderer Systeme in den Notes vermeiden.

## Neuen Release erstellen (empfohlen)

Voraussetzung: **sauberer** Working Tree auf `master`/`main`, synchron mit `origin`.  
Bei **minor/major**: befülltes `[Unreleased]`. Bei **patch** optional (sonst Notes der Vorgängerversion).

### IDE (Play)

Run Configuration **Release** (`.run/Release.run.xml`) → Play.  
Im Terminal: `patch` / `minor` / `major` wählen, mit `y` bestätigen.

Das Skript setzt die Version, promoted den Changelog, committed `release: x.y.z`, taggt `vx.y.z` und pusht Branch + Tag.

### Terminal

```powershell
npm run release
```

Danach: Actions → Workflow **release** (Win + zwei Mac-Jobs + Ubuntu AppImage + Merge `latest.json`); öffentliches Repo → [Releases](https://github.com/a-kowalenko/aero-media-service-releases/releases).

`latest.json` wird **nicht** mehr von jedem Matrix-Job hochgeladen (Race/Overwrite), sondern am Ende per `scripts/merge-updater-manifest.mjs` aus allen Release-Assets zusammengeführt.

Kaputtes Manifest auf bestehendem Tag reparieren (ohne Neu-Build): Actions → **repair-updater-manifest** → Tag z. B. `v0.1.10`.

Neue Releases bekommen **kein** „Latest“-Label. Erst nach manueller Promotion greifen Installer-Links und Auto-Update (`/releases/latest/`).

### Lokaler Windows-Build ohne Release

```powershell
npm run build:win
```

Das baut lokal nur das Windows-NSIS-Setup. Ohne `TAURI_SIGNING_PRIVATE_KEY` werden die Updater-Artefakte automatisch über `src-tauri/tauri.conf.ci.json` deaktiviert.

Normale Commits auf `master` starten **keinen** App-Build. Volle Bundles nur bei Version-Tags (`release.yml`). PRs: leichter Check in `test.yml`.

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
