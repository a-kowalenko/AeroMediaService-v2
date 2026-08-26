# Releases & Auto-Update

Source: privates Repo (dieses Projekt)  
Binaries: öffentliches Repo [`a-kowalenko/aero-media-service-releases`](https://github.com/a-kowalenko/aero-media-service-releases)

Updater-Endpoint (Stable / Latest):

```text
https://github.com/a-kowalenko/aero-media-service-releases/releases/latest/download/latest.json
```

Beta-Updates (nur mit Einstellung **Betatester**): GitHub-Releases-API inkl. Prereleases — auch wenn die App aktuell auf einer Stable-Version läuft.

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

### Alltag

Nutzer-sichtbare Punkte unter **`## [Unreleased]`** eintragen (Bullet-Listen) — bei jeder sichtbaren Änderung bzw. vom Agenten dort anlegen.

### Beta vs. Stable

| Aktion | `[Unreleased]` | Neuer Abschnitt |
|--------|----------------|-----------------|
| **Beta** (`0.1.14-beta.1`) | bleibt erhalten | Snapshot-Kopie → `## [0.1.14-beta.1]` |
| **Stable** (`0.1.14`) | wird geleert (promote) | `## [0.1.14]` |

Bei `beta.2` wird erneut der **gesamte** aktuelle Unreleased-Stand kopiert (inkl. dem, was schon in `beta.1` stand) — Absicht, damit Beta-Tester den Gesamtstand Richtung Stable sehen.

`npm run release`:

- **`[Unreleased]` befüllt** → Notes für Stable-Promote bzw. Beta-Snapshot
- **Beta und Unreleased leer** → Stub „Vorabversion zum Testen“
- **Stable patch und Unreleased leer** → Notes der Vorgängerversion
- **Stable minor/major und leer** → Abbruch

CI liest `## [versionsnummer]` am Tag (`node scripts/changelog.mjs extract …`). Bei fehlendem Beta-Abschnitt kein Walkback auf alte Stable-Notes.

```bash
node scripts/changelog.mjs extract 0.1.14-beta.1
node scripts/changelog.mjs extract 0.1.14
```

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
- …   # optional
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

### IDE (Play)

Run Configuration **Release** (`.run/Release.run.xml`) → Play.

### Terminal

```powershell
npm run release
```

### Menü

**Aktuelle Version ist Stable** (z. B. `0.1.13`):

1. Ziel-Bump: `patch` / `minor` / `major`
2. Kanal: `stable` / `beta`

| Wahl | Ergebnis |
|------|----------|
| patch + beta | `0.1.14-beta.1` |
| minor + beta | `0.2.0-beta.1` |
| major + beta | `1.0.0-beta.1` |
| patch + stable | `0.1.14` |

**Aktuelle Version ist schon Beta** (z. B. `0.1.14-beta.1`):

- `beta` → `0.1.14-beta.2`
- `stable` → `0.1.14` (Suffix weg, Unreleased → finale Notes)

Das Skript setzt die Version in `package.json`, Locks, `tauri.conf.json`, `Cargo.toml`, committed `release: …`, taggt `v…` und pusht Branch + Tag.

### CI-Verhalten

```text
prepare → release (Win + 2× Mac + Linux) → merge-updater-manifest → promote-latest (nur Stable)
```

| Tag | GitHub | Latest |
|-----|--------|--------|
| `v0.1.14-beta.1` | Prerelease | nein |
| `v0.1.14` | Release | **ja**, automatisch nach Merge von `latest.json` (`promote-latest`) |

`latest.json` wird **nicht** von jedem Matrix-Job hochgeladen (Race/Overwrite), sondern am Ende per `scripts/merge-updater-manifest.mjs` aus allen Release-Assets zusammengeführt — auch bei Beta-Tags (eigene Manifest-URL am Tag).

`promote-latest` startet von selbst — kein manueller Trigger. Voraussetzung: Asset `latest.json` vorhanden; ältere Stable-Tags demote kein neueres Latest.

Beispiel-Timeline:

```text
0.1.13 (Latest)
  → v0.1.14-beta.1 (prerelease)
  → v0.1.14-beta.2 (prerelease)
  → v0.1.14 (stable) → CI setzt Latest
```

Kaputtes Manifest auf bestehendem Tag reparieren (ohne Neu-Build): Actions → **repair-updater-manifest** → Tag z. B. `v0.1.10` oder `v0.1.14-beta.1`.

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
- SemVer: `0.1.14-beta.1` &lt; `0.1.14` — Beta-Nutzer erhalten die finale Stable als Update.
- PR-CI bleibt bewusst leichtgewichtig; volle Bundle-Builds laufen nur in `release.yml`.
- App-Datenpfade bleiben stabil: `%LOCALAPPDATA%\AeroMediaService\` / `~/Library/Application Support/AeroMediaService/` / `~/.local/share/AeroMediaService/`
- Keyring-Service: `AeroMediaService-v2`

## Neuen Signing-Key erzeugen (nur wenn nötig)

```powershell
npx tauri signer generate -w src-tauri/keys/updater.key --ci
```

Pubkey in `tauri.conf.json` übernehmen; Private Key **nie** committen.
