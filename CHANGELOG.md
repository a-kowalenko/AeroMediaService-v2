# Changelog

Nutzer-sichtbare Release Notes (Update-Dialog & GitHub Release).

Schreibregeln & Struktur: `docs/RELEASE.md` → Abschnitt **Release-Notes**.
Neue Einträge unter **`[Unreleased]`**; `npm run release` versioniert sie.
Patch ohne Unreleased-Text: Notes der Vorgängerversion werden übernommen.

## [Unreleased]

### Neu

- Einstellung **Betatester** unter Wartung → Software-Update: Vorabversionen anzeigen und per Auto-Update erhalten
- In den Einstellungen gezielt auf eine **andere Version wechseln** (Upgrade oder Downgrade mit Bestätigung)
- Bridge: **Primär-Share** und optionalen **Backup-Share** für Schnittplätze hinterlegen (`smb://…`)
- Bei Netzwerk-Monitorpfad: Vorschlag **Primär aus Monitor übernehmen**
- Kundenaufnahme: optional **Kunden-ID** und **Buchungs-ID** — Kontakt und gebuchte Medienarten werden aus der Buchung übernommen
- Bei abweichenden Daten: Vergleich anzeigen und wählen, ob Buchungsdaten oder Formular behalten werden
- Einstellungen: Reiter **Crew** — Tandemmaster, Videospringer und Aliases für die Ordnernamen-Vorhersage
- Zuweisung mit IDs: Medien in die richtigen Unterordner sortieren, Ordner umbenennen und Fertig-Übergabe setzen
- Bei unsicherer Crew-Erkennung: Dialog **Crew & Ordnername prüfen** mit Live-Vorschau (auch bei Stapel-Zuweisung)

### Verbessert

- Bridge Primär-/Backup-Share: **Auswahlliste** mit auf dem AMS-Rechner erkannten Freigaben (Netzlaufwerke, gemountete Shares, lokale Exporte)
- Button **Lokale Shares aktualisieren** lädt die Vorschlagsliste neu
- Backup-Share: Vorschlag **vom Primär-Share abgeleitet** (z. B. mit `-backup`)
- **Primär aus Monitor übernehmen** — auch wenn der Monitor ein lokaler Pfad ist (Modus **Pfad**, nicht nur Netzwerk)
- Upload **Abbrechen**: Status „Abbruch…“, Button „Wird abgebrochen…“ — Pause und erneutes Abbrechen sind während des Abbruchs gesperrt
- Update-Dialog: Hinweis bei **Vorabversionen (Beta)**; Patchnotes aufklappbar
- Versionsliste in den Einstellungen mit Kennzeichnung (Neueste, Beta, Installiert)
- Hinweis in der App und unter Bridge-Einstellungen, wenn der Primär-Share fehlt oder nicht zum Monitor-Pfad passt
- Einstellungen: eigener Reiter **Bridge** — Warnhinweis öffnet dort direkt
- Primär- und Backup-Share mit Protokollwahl (`smb://`, UNC `\\` oder lokaler **Pfad**)
- Crew-Reiter: Rollen TM/VS als **Schalter**; übersichtlichere Mitgliederliste
- Bridge-Client-Übersicht und Aktivitätsliste: Aktualisieren ohne leeres Flackern
- Ordnerauswahl bei der Zuweisung: Hintergrund-Aktualisierung ohne sichtbares Neu-Laden
- Monitoring an/aus bleibt nach Neustart erhalten

### Behoben

- Abgebrochener Upload: am Schnittplatz nicht mehr fälschlich **Fertig**, wenn die Statusdatei noch „erledigt“ meldet — **Abgebrochen** aus der Historie hat Vorrang
- Bridge-Jobstatus: fehlende Statusdatei wird aus der **Historie** geliefert (auch bei Abbruch)
- Share-Pfade: falsches `smb://`-Präfix vor lokalen Windows-Pfaden wird beim Eingeben bereinigt
- Windows-Release-Builds nutzen nur noch **NSIS** (kein MSI), damit Beta-Tags wie `0.1.14-beta.1` in CI durchlaufen
- **Auto-Update** wieder vollständig: macOS-Updater-Archive (`.app.tar.gz`) werden in CI zuverlässig hochgeladen; `latest.json`-Merge erst nach grünem Matrix-Build
- Übergabe vom Schnittplatz: Wartezeit startet erst mit aktivem Monitoring — Aufträge werden nicht abgelehnt, nur weil AMS kurz aus war
- Fertige Übergabe-Ordner werden auch nach Ablauf der Wartezeit noch übernommen, sobald sie bereit sind
- Timeout-Meldung bei sichtbarem Ordner nicht mehr fälschlich als „Ordner nicht sichtbar“
- Abgelehnte Übergabe verschwindet aus der Ansicht, sobald der Upload den Ordner übernommen hat
- Dialog **Medien nachreichen**: Abdunkelung liegt wieder korrekt hinter dem Fenster

### Hinweis

- Ohne Kunden-/Buchungs-ID bleibt die bisherige Kontakt-Zuweisung unverändert








## [0.2.0-beta.1] - 2026-08-28

### Neu

- Einstellung **Betatester** unter Wartung → Software-Update: Vorabversionen anzeigen und per Auto-Update erhalten
- In den Einstellungen gezielt auf eine **andere Version wechseln** (Upgrade oder Downgrade mit Bestätigung)
- Bridge: **Primär-Share** und optionalen **Backup-Share** für Schnittplätze hinterlegen (`smb://…`)
- Bei Netzwerk-Monitorpfad: Vorschlag **Primär aus Monitor übernehmen**
- Kundenaufnahme: optional **Kunden-ID** und **Buchungs-ID** — Kontakt und gebuchte Medienarten werden aus der Buchung übernommen
- Bei abweichenden Daten: Vergleich anzeigen und wählen, ob Buchungsdaten oder Formular behalten werden
- Einstellungen: Reiter **Crew** — Tandemmaster, Videospringer und Aliases für die Ordnernamen-Vorhersage
- Zuweisung mit IDs: Medien in die richtigen Unterordner sortieren, Ordner umbenennen und Fertig-Übergabe setzen
- Bei unsicherer Crew-Erkennung: Dialog **Crew & Ordnername prüfen** mit Live-Vorschau (auch bei Stapel-Zuweisung)

### Verbessert

- Bridge Primär-/Backup-Share: **Auswahlliste** mit auf dem AMS-Rechner erkannten Freigaben (Netzlaufwerke, gemountete Shares, lokale Exporte)
- Button **Lokale Shares aktualisieren** lädt die Vorschlagsliste neu
- Backup-Share: Vorschlag **vom Primär-Share abgeleitet** (z. B. mit `-backup`)
- **Primär aus Monitor übernehmen** — auch wenn der Monitor ein lokaler Pfad ist (Modus **Pfad**, nicht nur Netzwerk)
- Upload **Abbrechen**: Status „Abbruch…“, Button „Wird abgebrochen…“ — Pause und erneutes Abbrechen sind während des Abbruchs gesperrt
- Update-Dialog: Hinweis bei **Vorabversionen (Beta)**; Patchnotes aufklappbar
- Versionsliste in den Einstellungen mit Kennzeichnung (Neueste, Beta, Installiert)
- Hinweis in der App und unter Bridge-Einstellungen, wenn der Primär-Share fehlt oder nicht zum Monitor-Pfad passt
- Einstellungen: eigener Reiter **Bridge** — Warnhinweis öffnet dort direkt
- Primär- und Backup-Share mit Protokollwahl (`smb://`, UNC `\\` oder lokaler **Pfad**)
- Crew-Reiter: Rollen TM/VS als **Schalter**; übersichtlichere Mitgliederliste
- Bridge-Client-Übersicht und Aktivitätsliste: Aktualisieren ohne leeres Flackern
- Ordnerauswahl bei der Zuweisung: Hintergrund-Aktualisierung ohne sichtbares Neu-Laden
- Monitoring an/aus bleibt nach Neustart erhalten

### Behoben

- Abgebrochener Upload: am Schnittplatz nicht mehr fälschlich **Fertig**, wenn die Statusdatei noch „erledigt“ meldet — **Abgebrochen** aus der Historie hat Vorrang
- Bridge-Jobstatus: fehlende Statusdatei wird aus der **Historie** geliefert (auch bei Abbruch)
- Share-Pfade: falsches `smb://`-Präfix vor lokalen Windows-Pfaden wird beim Eingeben bereinigt
- Windows-Release-Builds nutzen nur noch **NSIS** (kein MSI), damit Beta-Tags wie `0.1.14-beta.1` in CI durchlaufen
- **Auto-Update** wieder vollständig: macOS-Updater-Archive (`.app.tar.gz`) werden in CI zuverlässig hochgeladen; `latest.json`-Merge erst nach grünem Matrix-Build
- Übergabe vom Schnittplatz: Wartezeit startet erst mit aktivem Monitoring — Aufträge werden nicht abgelehnt, nur weil AMS kurz aus war
- Fertige Übergabe-Ordner werden auch nach Ablauf der Wartezeit noch übernommen, sobald sie bereit sind
- Timeout-Meldung bei sichtbarem Ordner nicht mehr fälschlich als „Ordner nicht sichtbar“
- Abgelehnte Übergabe verschwindet aus der Ansicht, sobald der Upload den Ordner übernommen hat
- Dialog **Medien nachreichen**: Abdunkelung liegt wieder korrekt hinter dem Fenster

### Hinweis

- Ohne Kunden-/Buchungs-ID bleibt die bisherige Kontakt-Zuweisung unverändert

## [0.1.14-beta.6] - 2026-08-28

### Neu

- Einstellung **Betatester** unter Wartung → Software-Update: Vorabversionen anzeigen und per Auto-Update erhalten
- In den Einstellungen gezielt auf eine **andere Version wechseln** (Upgrade oder Downgrade mit Bestätigung)
- Bridge: **Primär-Share** und optionalen **Backup-Share** für Schnittplätze hinterlegen (`smb://…`)
- Bei Netzwerk-Monitorpfad: Vorschlag **Primär aus Monitor übernehmen**
- Kundenaufnahme: optional **Kunden-ID** und **Buchungs-ID** — Kontakt und gebuchte Medienarten werden aus der Buchung übernommen
- Bei abweichenden Daten: Vergleich anzeigen und wählen, ob Buchungsdaten oder Formular behalten werden
- Einstellungen: Reiter **Crew** — Tandemmaster, Videospringer und Aliases für die Ordnernamen-Vorhersage
- Zuweisung mit IDs: Medien in die richtigen Unterordner sortieren, Ordner umbenennen und Fertig-Übergabe setzen
- Bei unsicherer Crew-Erkennung: Dialog **Crew & Ordnername prüfen** mit Live-Vorschau (auch bei Stapel-Zuweisung)

### Verbessert

- Upload **Abbrechen**: Status „Abbruch…“, Button „Wird abgebrochen…“ — Pause und erneutes Abbrechen sind während des Abbruchs gesperrt
- Update-Dialog: Hinweis bei **Vorabversionen (Beta)**; Patchnotes aufklappbar
- Versionsliste in den Einstellungen mit Kennzeichnung (Neueste, Beta, Installiert)
- Hinweis in der App und unter Bridge-Einstellungen, wenn der Primär-Share fehlt oder nicht zum Monitor-Pfad passt
- Einstellungen: eigener Reiter **Bridge** — Warnhinweis öffnet dort direkt
- Primär- und Backup-Share mit Protokollwahl (`smb://` oder UNC `\\`)
- Bridge-Client-Übersicht und Aktivitätsliste: Aktualisieren ohne leeres Flackern
- Ordnerauswahl bei der Zuweisung: Hintergrund-Aktualisierung ohne sichtbares Neu-Laden
- Monitoring an/aus bleibt nach Neustart erhalten

### Behoben

- Windows-Release-Builds nutzen nur noch **NSIS** (kein MSI), damit Beta-Tags wie `0.1.14-beta.1` in CI durchlaufen
- **Auto-Update** wieder vollständig: macOS-Updater-Archive (`.app.tar.gz`) werden in CI zuverlässig hochgeladen; `latest.json`-Merge erst nach grünem Matrix-Build
- Übergabe vom Schnittplatz: Wartezeit startet erst mit aktivem Monitoring — Aufträge werden nicht abgelehnt, nur weil AMS kurz aus war
- Fertige Übergabe-Ordner werden auch nach Ablauf der Wartezeit noch übernommen, sobald sie bereit sind
- Timeout-Meldung bei sichtbarem Ordner nicht mehr fälschlich als „Ordner nicht sichtbar“
- Abgelehnte Übergabe verschwindet aus der Ansicht, sobald der Upload den Ordner übernommen hat
- Dialog **Medien nachreichen**: Abdunkelung liegt wieder korrekt hinter dem Fenster

### Hinweis

- Ohne Kunden-/Buchungs-ID bleibt die bisherige Kontakt-Zuweisung unverändert

## [0.1.14-beta.5] - 2026-08-27

### Neu

- Einstellung **Betatester** unter Wartung → Software-Update: Vorabversionen anzeigen und per Auto-Update erhalten
- In den Einstellungen gezielt auf eine **andere Version wechseln** (Upgrade oder Downgrade mit Bestätigung)
- Bridge: **Primär-Share** und optionalen **Backup-Share** für Schnittplätze hinterlegen (`smb://…`)
- Bei Netzwerk-Monitorpfad: Vorschlag **Primär aus Monitor übernehmen**
- Kundenaufnahme: optional **Kunden-ID** und **Buchungs-ID** — Kontakt und gebuchte Medienarten werden aus der Buchung übernommen
- Bei abweichenden Daten: Vergleich anzeigen und wählen, ob Buchungsdaten oder Formular behalten werden
- Einstellungen: Reiter **Crew** — Tandemmaster, Videospringer und Aliases für die Ordnernamen-Vorhersage
- Zuweisung mit IDs: Medien in die richtigen Unterordner sortieren, Ordner umbenennen und Fertig-Übergabe setzen
- Bei unsicherer Crew-Erkennung: Dialog **Crew & Ordnername prüfen** mit Live-Vorschau (auch bei Stapel-Zuweisung)

### Verbessert

- Upload **Abbrechen**: Status „Abbruch…“, Button „Wird abgebrochen…“ — Pause und erneutes Abbrechen sind während des Abbruchs gesperrt
- Update-Dialog: Hinweis bei **Vorabversionen (Beta)**; Patchnotes aufklappbar
- Versionsliste in den Einstellungen mit Kennzeichnung (Neueste, Beta, Installiert)
- Hinweis in der App und unter Bridge-Einstellungen, wenn der Primär-Share fehlt oder nicht zum Monitor-Pfad passt
- Einstellungen: eigener Reiter **Bridge** — Warnhinweis öffnet dort direkt
- Primär- und Backup-Share mit Protokollwahl (`smb://` oder UNC `\\`)
- Bridge-Client-Übersicht und Aktivitätsliste: Aktualisieren ohne leeres Flackern
- Ordnerauswahl bei der Zuweisung: Hintergrund-Aktualisierung ohne sichtbares Neu-Laden
- Monitoring an/aus bleibt nach Neustart erhalten

### Behoben

- Windows-Release-Builds nutzen nur noch **NSIS** (kein MSI), damit Beta-Tags wie `0.1.14-beta.1` in CI durchlaufen
- **Auto-Update** wieder vollständig: macOS-Updater-Archive (`.app.tar.gz`) werden in CI zuverlässig hochgeladen; `latest.json`-Merge erst nach grünem Matrix-Build

### Hinweis

- Ohne Kunden-/Buchungs-ID bleibt die bisherige Kontakt-Zuweisung unverändert

## [0.1.14-beta.4] - 2026-08-27

### Neu

- Einstellung **Betatester** unter Wartung → Software-Update: Vorabversionen anzeigen und per Auto-Update erhalten
- In den Einstellungen gezielt auf eine **andere Version wechseln** (Upgrade oder Downgrade mit Bestätigung)
- Bridge: **Primär-Share** und optionalen **Backup-Share** für Schnittplätze hinterlegen (`smb://…`)
- Bei Netzwerk-Monitorpfad: Vorschlag **Primär aus Monitor übernehmen**

### Verbessert

- Upload **Abbrechen**: Status „Abbruch…“, Button „Wird abgebrochen…“ — Pause und erneutes Abbrechen sind während des Abbruchs gesperrt
- Update-Dialog: Hinweis bei **Vorabversionen (Beta)**; Patchnotes aufklappbar
- Versionsliste in den Einstellungen mit Kennzeichnung (Neueste, Beta, Installiert)
- Hinweis in der App und unter Bridge-Einstellungen, wenn der Primär-Share fehlt oder nicht zum Monitor-Pfad passt

### Behoben

- Windows-Release-Builds nutzen nur noch **NSIS** (kein MSI), damit Beta-Tags wie `0.1.14-beta.1` in CI durchlaufen
- **Auto-Update** wieder vollständig: macOS-Updater-Archive (`.app.tar.gz`) werden in CI zuverlässig hochgeladen; `latest.json`-Merge erst nach grünem Matrix-Build

## [0.1.14-beta.3] - 2026-08-26

### Neu

- Einstellung **Betatester** unter Wartung → Software-Update: Vorabversionen anzeigen und per Auto-Update erhalten
- In den Einstellungen gezielt auf eine **andere Version wechseln** (Upgrade oder Downgrade mit Bestätigung)

### Verbessert

- Upload **Abbrechen**: Status „Abbruch…“, Button „Wird abgebrochen…“ — Pause und erneutes Abbrechen sind während des Abbruchs gesperrt
- Update-Dialog: Hinweis bei **Vorabversionen (Beta)**; Patchnotes aufklappbar
- Versionsliste in den Einstellungen mit Kennzeichnung (Neueste, Beta, Installiert)

### Behoben

- Windows-Release-Builds nutzen nur noch **NSIS** (kein MSI), damit Beta-Tags wie `0.1.14-beta.1` in CI durchlaufen
- **Auto-Update** wieder vollständig: macOS-Updater-Archive (`.app.tar.gz`) werden in CI zuverlässig hochgeladen; `latest.json`-Merge erst nach grünem Matrix-Build

## [0.1.14-beta.2] - 2026-08-26

### Neu

- Einstellung **Betatester** unter Wartung → Software-Update: Vorabversionen anzeigen und per Auto-Update erhalten
- In den Einstellungen gezielt auf eine **andere Version wechseln** (Upgrade oder Downgrade mit Bestätigung)

### Verbessert

- Upload **Abbrechen**: Status „Abbruch…“, Button „Wird abgebrochen…“ — Pause und erneutes Abbrechen sind während des Abbruchs gesperrt
- Update-Dialog: Hinweis bei **Vorabversionen (Beta)**; Patchnotes aufklappbar
- Versionsliste in den Einstellungen mit Kennzeichnung (Neueste, Beta, Installiert)

### Behoben

- Windows-Release-Builds nutzen nur noch **NSIS** (kein MSI), damit Beta-Tags wie `0.1.14-beta.1` in CI durchlaufen

## [0.1.14-beta.1] - 2026-08-26

### Neu

- Einstellung **Betatester** unter Wartung → Software-Update: Vorabversionen anzeigen und per Auto-Update erhalten
- In den Einstellungen gezielt auf eine **andere Version wechseln** (Upgrade oder Downgrade mit Bestätigung)

### Verbessert

- Upload **Abbrechen**: Status „Abbruch…“, Button „Wird abgebrochen…“ — Pause und erneutes Abbrechen sind während des Abbruchs gesperrt
- Update-Dialog: Hinweis bei **Vorabversionen (Beta)**; Patchnotes aufklappbar
- Versionsliste in den Einstellungen mit Kennzeichnung (Neueste, Beta, Installiert)

## [0.1.13] - 2026-08-26

### Neu

- **Infobroschüre** in den Einstellungen hinterlegen (PDF per Drag & Drop, max. 5 MB)
- Beim **Erst-Upload** wird die Broschüre automatisch mit in denselben Cloud-Ordner gelegt
- Dateiname und optionaler Unterordner einstellbar (Standard: `Infobroschuere.pdf` im Ordner-Root)

### Hinweis

- Nur beim Erst-Upload — beim **Nachreichen** kommt keine zweite Broschüre; E-Mail/SMS/WhatsApp bleiben unverändert

## [0.1.12] - 2026-08-26

### Verbessert

- Cloud-Dienst in Einstellungen, Einrichtungsassistent und Status heißt jetzt **Skydive Media** (statt „Custom API“) — inkl. zugehöriger Dropbox-Konten und Meldungen
- Historie sortiert nach **Erstelldatum** — Einträge rutschen nicht mehr nach oben, wenn nur Kontaktdaten, Buchungsinfos oder Archivpfad nachgezogen werden
- Detailansicht zeigt **Erstellt** und **Zuletzt aktualisiert** getrennt
- „Zuletzt aktualisiert“ ändert sich nur noch bei echten Upload-/Status-Schritten, nicht bei reinen Metadaten-Updates

## [0.1.11] - 2026-08-26

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
