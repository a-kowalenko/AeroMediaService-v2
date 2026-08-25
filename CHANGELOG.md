# Changelog

Nutzer-sichtbare Release Notes (Update-Dialog & GitHub Release).

Schreibregeln & Struktur: `docs/RELEASE.md` → Abschnitt **Release-Notes**.
Neue Einträge unter **`[Unreleased]`**; `npm run release` versioniert sie.
Patch ohne Unreleased-Text: Notes der Vorgängerversion werden übernommen.

## [Unreleased]

### Verbessert

- Download-Links, Dropbox-Autorisierung und Update-Installer öffnen zuverlässiger im Standard-Browser — besonders unter **Linux (AppImage)**
- Archivordner aus der Historie lassen sich stabiler im Dateimanager öffnen

### Behoben

- Auto-Update: Releases mit mehreren Plattformen (Windows, macOS, Linux) liefern wieder das korrekte Update-Paket für jedes Betriebssystem
- Parallele Uploads: Datei-Zähler und Slot-Anzeige blieben hängen, wenn direkt nach Abschluss einer Datei die nächste in derselben Reihe startete

## [0.1.10] - 2026-08-25

### Verbessert

- Parallele Uploads: jede laufende Datei behält ihre Zeile — wenn eine Datei fertig ist, startet die nächste in derselben Reihe, ohne dass die Liste springt
- Upload-Bereich zeigt nur noch tatsächlich laufende Dateien, keine leeren Platzhalter-Zeilen mehr

## [0.1.9] - 2026-08-25

### Verbessert

- Parallele Upload-Slots: sobald eine Datei fertig ist, startet die nächste — die vier Slots bleiben durchgehend sichtbar statt alle kurz zu verschwinden
- Gesamt-Fortschritt (Bytes) steigt nur noch nach oben und springt nicht mehr zurück, wenn einzelne Dateien abgeschlossen werden

## [0.1.8] - 2026-08-25

### Verbessert

- **Dropbox-Uploads** reagieren bei Rate-Limits automatisch — weniger parallele Dateien, danach schrittweise wieder hoch, statt Fehler oder Stillstand
- Upload-Bereich: Liste der parallelen Slots springt beim Ein- und Ausblenden weniger

### Behoben

- Großer Dropbox-Upload bricht bei Rate-Limits nicht mehr nahe am Ende ab und startet nicht von vorn — bereits hochgeladene Dateien werden übersprungen und der Upload setzt dort fort

## [0.1.7] - 2026-08-25

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
