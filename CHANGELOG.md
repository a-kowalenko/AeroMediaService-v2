# Changelog

Nutzer-sichtbare Release Notes (Update-Dialog & GitHub Release).

Schreibregeln & Struktur: `docs/RELEASE.md` → Abschnitt **Release-Notes**.
Neue Einträge unter **`[Unreleased]`**; `npm run release` versioniert sie.
Patch ohne Unreleased-Text: Notes der Vorgängerversion werden übernommen.

## [Unreleased]

### Verbessert

- Dropbox-Autorisierung: Autorisierungs-Link per **Link kopieren** in die Zwischenablage, wenn sich kein Browser öffnet
- Bestätigungsdialoge mit optionaler dritter Aktion (z. B. neben „Browser öffnen“ und „Abbrechen“)

## [0.1.6] - 2026-08-25

### Neu

- **Mehrere Dropbox-Konten** — Native Dropbox und Dropbox über die Custom-API getrennt anlegen und verwalten
- Pro Konto: Bezeichnung, Verbinden/Trennen, als aktiv setzen, Zugangsdaten bearbeiten, Konto löschen
- **App-Ordnername** pro Dropbox-Konto einstellbar (wichtig bei mehreren Dropbox-Apps unter `/Apps/…`)
- **Parallele Uploads** — mehrere Dateien gleichzeitig, mit Übersicht der laufenden Slots im Upload-Bereich
- **Status-Chips** in Historie, Einstellungen und Seitenleiste (Cloud-Verbindung, Upload-Pipeline)
- Einrichtungsassistent schlägt **Standard-Pfade** vor und legt einen App-Stammordner fest
- Release Notes direkt im **Update-Dialog** lesbar

### Verbessert

- Upload-Fortschritt zeigt **Bytes und Dateien** realistisch — nicht nur eine grobe Schätzung
- Upload-Bereich: laufende Dateien, Gesamtfortschritt und Pause/Fortsetzen/Abbrechen übersichtlicher
- Einstellungen in klarere Bereiche aufgeteilt (Dropbox-Konten, Extras, …)
- Schnittplatz-Aktivität und verbundene Clients in den Einstellungen übersichtlicher
- Medien nachreichen: Dialog und Ablauf robuster

## [0.1.5] - 2026-08-25

### Verbessert

- App-Name in Fenster und Oberfläche vereinheitlicht (**Aero Media Service**)
- Einstellungen: kurze **Bestätigung beim Speichern** (Toast)

## [0.1.4] - 2026-08-24

### Verbessert

- Upload: Pause, Fortsetzen und Abbrechen zuverlässiger — Fortschritt bleibt nach Steuerungswechsel konsistent
- Historie: Statusanzeige und Pipeline-Schritte präziser

## [0.1.3] - 2026-08-23

### Neu

- **Schnittplatz-Übergabe im Netzwerk** — Rechner am gemeinsamen Ordner erkennen und verbinden
- Aktivität und Status verbundener Schnittplätze in den Einstellungen einsehbar

### Verbessert

- Einstellungen überarbeitet — bessere Struktur und Übersicht

## [0.1.2] - 2026-08-19

### Verbessert

- Kleinere Stabilitäts- und Oberflächenverbesserungen

## [0.1.1] - 2026-08-19

### Neu

- Erste öffentliche Version — Desktop-App für **Windows, macOS und Linux**
- **Ordner-Monitor** mit Stabilitätsprüfung und automatischem Upload zu **Dropbox** oder **Custom-API**
- **Historie** mit Status, Wiederholen, erneut senden und manuellen Aktionen
- **Benachrichtigungen** per E-Mail, SMS und WhatsApp
- **Einrichtungsassistent** beim ersten Start (Pfade, Cloud-Dienst)
- **Kundenaufnahme** und Marker-Zuweisung am Upload-PC
- **Medien nachreichen** in bestehende Cloud-Ordner (gleicher Link, keine erneute Benachrichtigung)
- **Auto-Update** mit signierten Releases
