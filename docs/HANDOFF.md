# ATS ↔ AMS Handoff

> Kanonisches Konzept für den Datei-Handoff zwischen **AeroTandemStudio-v2 (ATS)** und **Aero Media Service v2 (AMS)**.  
> Implementierung: [IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md) Phase 13 (Teilphasen P0–P3+).  
> AMS-Upload-Pipeline bleibt unverändert; nur ein Gate vor dem Claim und optionale Control-Plane.

---

## 1. Betriebsmodell

```
ATS ──write──►  SMB-Share „aktuell“ / <JobOrdner>/
                      │
                      │  Manifest + _fertig.txt
                      ▼
AMS Monitor ──claim──► Upload / Notify / Archiv (bestehend)
```

- **ATS** erstellt den Vorgangsordner und schreibt ihn auf den SMB-Share **`aktuell`** (Beispiel: `\\169.254.169.254\aktuell`).
- **AMS** monitored genau diesen Ordner (`monitor_path` = derselbe Share).
- Apps liegen **meist auf verschiedenen Rechnern** im (wechselnden) LAN; Data Plane = Share.
- Control Plane (LAN-Bridge) ist **optional**; der Datei-Handoff muss allein funktionieren.

Voraussetzung v1: gemeinsamer Share — keine separate Copy-Pipeline ATS→AMS.

---

## 2. Ziele & Nicht-Ziele

### Ziele

1. Garantieren, dass ein Job-Ordner auf `aktuell` **vollständig** ist, bevor AMS claimt.
2. Optional **Feedback** an ATS (übernommen / abgelehnt / fertig / fehler).
3. Optional **Customer-Preflight**: ATS fragt AMS; AMS ruft die Customer-API (Credentials bleiben bei AMS).
4. Erweiterbar (Schema-Version, Capabilities), ohne den Upload-Kern anzupassen.

### Nicht-Ziele

- AMS-Upload-Pipeline, Pause/Resume/Cancel, Cloud-Clients oder Notify umbauen.
- Medien per HTTP zwischen den Apps übertragen.
- Marker-Protokoll (`_fertig.txt` / `_in_verarbeitung.txt`) abschaffen.
- Zwang zu gleichem PC oder localhost-only.

---

## 3. Schichten

```
L4  UX (ATS: Handoff-Status am Vorgang)
L3  Bridge API LAN (optional) — health, lookup, job status, ready
L2  Handoff-Contract auf Share — Manifest + _fertig + Status-Outbox
L1  AMS Monitor → Claim → Upload … (bestehend; Gate vor Claim)
```

| Ebene | Verantwortung |
|--------|----------------|
| **Data Plane** | SMB `aktuell/<Job>/` — Wahrheit für Dateien |
| **Control Plane** | optional HTTP auf AMS-Host; Fallback = Dateien auf dem Share |

Regel: **Filesystem allein muss immer reichen.** Bridge ist Enhancement.

---

## 4. Dateivertrag

### 4.1 Im Job-Ordner (`aktuell/<JobName>/`)

| Datei | Autor | Rolle |
|--------|--------|--------|
| Medien (Unterordner wie heute) | ATS | Payload |
| `_ams_manifest.v1.json` | ATS | Vollständigkeitsvertrag |
| `_fertig.txt` | ATS | Ready-Signal (bestehend) |
| `_in_verarbeitung.txt` | AMS | Claim (bestehend) |

### 4.2 Seitlich (Monitor ignoriert)

| Pfad | Autor | Rolle |
|------|--------|--------|
| `aktuell/.ams-handoff/<correlation_id>.json` | AMS | Status-Outbox für ATS |

**Ignore-Liste** (Fingerprint / Scan / Manifest-Pflicht):  
`.ams-handoff`, `_fertig.txt`, `_in_verarbeitung.txt`, `_ams_manifest.v1.json`, Upload-Checkpoints, `Thumbs.db`, `.DS_Store`.

Status **nicht** in AMS-AppData (andere Maschine) und nicht nur im Job-Ordner (Archiv-Move).

---

## 5. ATS-Schreibreihenfolge (verbindlich)

1. Job-Ordner anlegen und alle Medien schreiben; Dateihandles schließen.
2. `_ams_manifest.v1.json` **atomar** schreiben (temp → rename).
3. `_fertig.txt` **atomar** schreiben (temp → rename) — Ready-Signal.
4. Optional (P3): Bridge `POST /v1/handoff/ready`.

Lokal-Modus ohne Marker (`skip_marker_file`): kein AMS-Handoff.

---

## 6. Manifest Schema v1

```json
{
  "schema": 1,
  "protocol": "ams-handoff",
  "correlation_id": "<uuid>",
  "producer": { "app": "AeroTandemStudio", "version": "x.y.z" },
  "producer_ref": { "vorgang_id": 123 },
  "created_at": "2026-08-15T00:00:00+02:00",
  "folder_name": "20260815_Max_Mustermann_TA_TM",
  "integrity": {
    "algo": "size",
    "files": [
      { "path": "Handcam_Video/….mp4", "size": 123456789 }
    ]
  },
  "marker_hint": {
    "format": "api_hash | api_id | pure_contact | none",
    "type": "Handcam | Outside"
  },
  "extensions": {}
}
```

| Feld / Thema | Regel |
|--------------|--------|
| Pfade | relativ zum Job-Root |
| Integrität v1 | nur `size` (SHA später über `algo` / `extensions`) |
| Marker-Inhalt | bleibt in `_fertig.txt`; Manifest nur Hint |
| `correlation_id` | UUID; ATS speichert zusätzlich `producer_ref.vorgang_id` |
| `extensions` | freies Objekt — Forward-Compat ohne Schema-Bump |
| Dateiname | `_ams_manifest.v1.json` — parallele Schema-Versionen möglich |

### 6.1 Append / Nachreichen (Phase 15)

Medien nachträglich in **dieselbe Cloud-Order / denselben Share-Link** legen, ohne neuen Kundenordner.

ATS schreibt einen **neuen Staging-Ordner** auf `aktuell` (nicht in das AMS-Archiv):

`{original}_nachreichung_01/`

Manifest bleibt Schema v1; Append-Metadaten liegen in `extensions` (kein Schema-Bump):

```json
{
  "extensions": {
    "kind": "append",
    "parent_correlation_id": "<uuid des Erst-Handoffs>"
  }
}
```

| Regel | Verhalten |
|--------|-----------|
| Ordnername | `{parent_folder_name}_nachreichung_{nn}` — eigener Claim, eigene `correlation_id` |
| Parent | AMS löst `parent_correlation_id` in der Historie auf (`remote_path`, `order_id`) |
| Parent-Status | nur `Erfolgreich` — sonst Gate `append_parent_not_ready`, kein Claim |
| Upload | bestehende Append-Pipeline (Phase 14): gleicher `remote_path` / `existing_order_id` |
| Link | unverändert; **keine** Kunden-Benachrichtigung |
| Archiv | Append-Ordner nach Erfolg nach `erfolg` (wie ein Job) |
| Bridge | Capability `append-v1`; `POST /v1/handoff/ready` unverändert (Wake) |

Filesystem allein reicht: Bridge down → Monitor scannt den Append-Ordner.

---

## 7. AMS-Gate (einzige Kernänderung an L1)

In `try_claim_and_enqueue`, **vor** `claim_fertig_marker`:

| Situation | Verhalten |
|-----------|-----------|
| Manifest vorhanden + gültig | Claim; Stability überspringen oder verkürzen |
| Manifest: Datei fehlt / Size falsch | **Kein Claim**, Ordner liegen lassen, Status `rejected`, Retry beim nächsten Scan |
| Manifest: JSON / Schema ungültig | wie oben `rejected` |
| Kein Manifest | **Legacy**: Stability + Claim wie heute |
| Marker ungültig / Customer-Lookup fail | wie heute → Archiv `fehler` |

Setting `manifest_required` default **`false`** (Legacy, AMS-Kunden-UI ohne Manifest).

Upload-Worker, Queue, Cloud, Notify: **unberührt**.

### Fehlercodes (stabil)

| Code | Bedeutung |
|------|-----------|
| `manifest_missing_legacy` | kein Manifest → Legacy-Pfad |
| `manifest_invalid_json` | Manifest nicht parsebar |
| `manifest_unsupported_schema` | unbekannte / zu neue Schema-Version |
| `file_missing` | deklarierte Datei fehlt |
| `size_mismatch` | Größe stimmt nicht |
| `marker_invalid` | wie bisher |
| `customer_lookup_failed` | wie bisher |
| `append_parent_missing` | `kind=append` ohne `parent_correlation_id` |
| `append_parent_not_ready` | Parent unbekannt oder nicht `Erfolgreich` |

---

## 8. Status-Outbox

Pfad: `aktuell/.ams-handoff/<correlation_id>.json` (atomar schreiben).

Beispiel:

```json
{
  "schema": 1,
  "correlation_id": "…",
  "updated_at": "ISO-8601",
  "state": "accepted | rejected | queued | uploading | completed | failed",
  "error": { "code": "size_mismatch", "message": "…" },
  "ams": { "history_id": "…", "archive": "erfolg|fehler|null" },
  "extensions": {}
}
```

- Finalen Status **vor** Archiv-Move des Job-Ordners setzen; Outbox-Datei bleibt liegen.
- ATS pollt über `correlation_id` (an Vorgang gebunden).

---

## 9. Bridge API (optional, ab P2)

- Bind: LAN (nicht nur `127.0.0.1`); **Token-Auth** Pflicht.
- ATS-Config: Base-URL + Token (wechselnde Netze → manuell / zuletzt erfolgreich speichern).
- ATS sendet bei **jedem** Bridge-Request zusätzlich Identitäts-Header:
  - `X-Ats-Instance-Id` = stabile ATS-Installations-UUID (Primary Key für Presence)
  - `X-Ats-Hostname` = PC-Name aus ATS-Settings / Hostname (für UI-Anzeige)
  - `X-Ats-Version` = ATS-Version
  - `X-Ats-App` = App-Name (z. B. `AeroTandemStudio`)
- Discovery (mDNS): **P4** — Service-Typ `_ams-bridge._tcp.local.`; Token bleibt manuell; Fallback = manuelle URL.
- Credentials der Customer-API bleiben bei AMS.

| Methode | Pfad | Zweck |
|---------|------|--------|
| `GET` | `/v1/health` | online, Version, `monitor_path`, `capabilities[]` |
| `POST` | `/v1/customer/lookup` | Preflight; AMS → bestehende Customer-API |
| `GET` | `/v1/jobs/{correlation_id}` | Status (Spiegel Outbox / History) |
| `POST` | `/v1/handoff/ready` | Monitor wake / Priorität — **kein** Upload-Bypass |

### 9.1 Presence / Host-Aktivität (P5+)

- AMS kann optional Bridge-only Presence anzeigen: **aktiv = mindestens ein Bridge-Event in den letzten 60 Minuten**.
- Presence basiert **nur** auf Bridge-Requests; ATS ohne Bridge bleibt bewusst unsichtbar.
- `X-Ats-Instance-Id` ist der technische Host-Schlüssel; `X-Ats-Hostname` bleibt die menschenlesbare Anzeige im AMS-UI.
- `GET /v1/jobs/{correlation_id}` und `POST /v1/handoff/ready` erlauben zusätzlich die Zuordnung `correlation_id -> ATS-Host`.

Capabilities statt harter Versionskopplung, z. B.  
`["manifest-v1","status-outbox","lookup","ready","append-v1"]`.

Breaking Changes → `/v2`; additive Felder in `/v1` erlaubt.

---

## 10. Kompatibilitätsmodi

| Modus | Verhalten |
|--------|-----------|
| Legacy ohne Manifest | wie heute (Stability + Marker) |
| Handoff v1 | Manifest + Gate |
| + Outbox | Feedback ohne Bridge |
| + Bridge | Lookup + Live-Status / Wake |
| Bridge down | Datei-Handoff + Outbox weiter nutzbar |

---

## 11. Konfiguration

### ATS

- `speicherort` = UNC des `aktuell`-Shares
- Manifest schreiben (Feature an, sobald P1)
- optional Bridge-URL + Token

### AMS

- `monitor_path` = derselbe Share
- Ignore `.ams-handoff`
- `manifest_required` = `false` (Default)
- optional Bridge enable + Token

### Betrieb

Gleiche UNC in beiden Apps. Health/Preflight kann Abweichung nur warnen — das ist ein Betriebsfehler, kein Protokollfehler.

Windows-Config: UNC (`\\host\aktuell`), nicht nur `smb://`.

---

## 12. Teilphasen (Phase 13)

| Teil | Scope | Repos |
|------|--------|--------|
| **P0** | Diese Spec + Plan/AGENTS/Architektur | AMS (Docs) |
| **P1** ✅ | ATS: Manifest schreiben; AMS: Parse + Gate; Ignore-Liste; Tests | AMS + ATS |
| **P1b** ✅ | AMS: Outbox-Status; ATS: `correlation_id` + Status lesen/anzeigen | AMS + ATS |
| **P2** ✅ | Bridge: health + customer lookup | AMS Server, ATS Client |
| **P3** ✅ | Bridge: job status + handoff/ready | beide |
| **P4** ✅ | Bridge mDNS Discovery only (`_ams-bridge._tcp.local.`) | beide |
| **L4 UX** ✅ | ATS Historie: Status-Chips/Stepper, Last-Known in SQLite, Poll stoppt bei Terminal | ATS |
| **P5+** | optional / nach Bedarf: SHA-256, strict extras, Bridge-Presence/Host-Aktivität | nach Bedarf |
| **Phase 15** ✅ | Append/Nachreichen: `kind=append` + Parent-Gate + Worker-Route | AMS + ATS |

**Eine Teilphase pro Agent-Session.** Upload-Worker nicht anfassen außer Status-Spiegel für Outbox, wo nötig.

---

## 13. Tests (ab P1)

- Manifest build / parse / validate (beide Seiten)
- Gate: `file_missing`, `size_mismatch`, Legacy ohne Manifest
- Ignore: `.ams-handoff` wird nicht als Job gescannt
- Regression: bestehende Marker- / Monitor- / Upload-Tests grün

---

## 14. Defaults für offene Betriebsfragen

| Punkt | Entscheidung |
|--------|----------------|
| Reject bei incomplete Manifest | liegen lassen + `rejected` |
| Reject bei kaputtem Marker / Lookup | Archiv `fehler` (bestehend) |
| Integrität v1 | nur `size` |
| Status-Ort | `aktuell/.ams-handoff/` |
| Bridge | erst P2; blockiert P1 nicht |
| `correlation_id` | UUID + `producer_ref.vorgang_id` |

---

## 15. Partner-Repo

ATS-Implementierung: `C:\Users\Kowalenko\PycharmProjects\AeroTandemStudio-v2`  
(Export-Job / Marker-Schreiben; Vorgang-History für `correlation_id` / Status-UI.)

Dieses Dokument ist die gemeinsame Spec; bei Drift gilt die neuere abgestimmte Version in AMS `docs/HANDOFF.md` als Referenz, bis ATS eine Kopie/Verlinkung führt.
