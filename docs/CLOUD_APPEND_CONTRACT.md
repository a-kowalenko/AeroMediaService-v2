# Cloud-Partner: Append / Auto-Nachreichen (AMS Phase 14 / 15 / 21)

**Status:** Spec für Partner-Cloud-Repo (nicht AMS-Code)  
**Bezug:** [`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md) Phase 14, 15, 21 · [`HANDOFF.md`](./HANDOFF.md) §6.1 / §6.1b  
**Gilt für:** Custom-API-Pfad (`POST /api/orders/create`, Manifest v1.1).  
**Nicht betroffen:** Native-Dropbox-only Uploads (kein Cloud-Order-Create).

AMS erledigt Auto-Nachreichen bereits clientseitig (Parent-`remote_path`, `existing_order_id`, Remote-Rename `(1)/(2)`, Notify mit Parent-Link). Dieses Dokument beschreibt **sinnvolle Cloud-Anpassungen**, damit Portal und Dropbox nicht auseinanderlaufen.

Phase 14 markiert Cloud Order-Lookup/Merge bereits als erledigt — **zuerst auditieren**, dann nur Lücken schließen.

---

## 1. Zielvertrag

| Fall | Erwartetes Cloud-Verhalten |
|------|----------------------------|
| `existing_order_id` gesetzt | Dateien an **diese** Order mergen; **`final_url` unverändert** (Parent-Link) |
| `base_dir` / `root_folder.path` = Parent-Root | Dateien unter diesem Root; keine zweite Order „nebenbei“ |
| Neue `rel_path`s | anhängen/mergen; **kein erzwungenes Überschreiben** gleichnamiger Dateien |
| Gleiche customer+booking **ohne** `existing_order_id` | bestehende aktive Order wiederverwenden **oder** klarer Fehler — **kein** stilles „Portal = Alt, Dropbox = Neu“ |
| Antwort | `ok`, `order_id` (= bestehende bei Append), `final_url` (= unverändert), optional `status` (`processing` / fertig) |

### AMS sendet (Manifest v1.1, Auszug)

- `existing_order_id` und `meta.existing_order_id` (bei Append)
- `customer.customer_number`, `customer.booking_number`, `customer.type`, …
- `base_dir`, `root_folder.path`, optional `root_folder.share_link`
- `categories[].files[].rel_path` / Größe / Dropbox-IDs
- `meta.version` = `"1.1"`, `meta.link_mode` = `"paths_only"`

Referenz-Builder: AMS `src-tauri/src/cloud/manifest.rs` (`build_manifest_v11`).

### Bekanntes Fehlermuster (ohne korrekten Append)

AMS lädt historisch in `/{neuer_dir_name}` hoch; Cloud liefert oft dieselbe `final_url` zur Booking-ID → Link zeigt auf den **alten** Ordner, neue Dateien liegen woanders. Phase 21 verhindert das **AMS-seitig** durch Append auf Parent-Root; Cloud muss dabei **keine neue Portal-Order** erzeugen und `final_url` nicht „neu“ machen.

---

## 2. Slices (eine Session / PR)

### C0 — Audit (zuerst)

- [ ] Bestehenden `orders/create`-Pfad mit `existing_order_id` gegen Phase-14-Append prüfen
- [ ] Logs/Tests: gleiche `order_id` + gleiche `final_url` bei Append vs. Erst-Upload
- [ ] Entscheidung: C1 nur Lücken schließen **oder** Full-Härtung

**DoD:** kurzer Ist-Stand (was schon geht / was bricht) im Cloud-Repo oder Ticket.

---

### C1 — Must: Append mit `existing_order_id`

Nur nötig, wenn C0 Lücken zeigt; sonst Regressionstests reichen.

- [ ] Order per ID laden; unbekannt → klarer `error` / `error_code` (kein Silent-Create einer neuen Order)
- [ ] Manifest-Dateien an bestehende Order mergen (Kategorien / Status)
- [ ] **`final_url` nicht neu erzeugen**; Response enthält die bestehende URL
- [ ] Storage-/Dropbox-Root aus Order bzw. Manifest-`base_dir` (Parent), nicht als neue Order aus neuem Ordnernamen
- [ ] Tests: Erst-Upload → Append → `order_id` + `final_url` identisch; Dateien unter Parent-Root sichtbar

**Nicht in C1:** Lookup ohne Order-ID (→ C2), Diagnose-Endpoint (→ C4).

---

### C2 — Should: customer+booking ohne `existing_order_id`

Absicherung gegen Drift (History ohne `order_id`, Retry, ältere Clients):

- [ ] Lookup aktive Order zu `customer_number` + `booking_number` (+ optional `type`)
- [ ] Treffer → wie C1 (Reuse, gleiche `final_url`)
- [ ] Kein Treffer → normaler Create (Erst-Upload)
- [ ] Mehrdeutig / Konflikt → **Fehler**, kein stiller Zweit-Portal-Link

**DoD:** kein Szenario „Link zeigt alten Ordner, Dateien liegen neu“.

---

### C3 — Should: Datei-Merge ohne Server-Overwrite

- [ ] Neue `rel_path`s unter bestehendem Root akzeptieren
- [ ] Gleicher `rel_path` schon vorhanden → skip / ignore / conflict-code — **nicht** blind überschreiben  
  (AMS renamed Kollisionen clientseitig zu `name (1).ext`)
- [ ] Optional: Response-Hinweis zu übersprungenen Duplikaten

---

### C4 — Nice: Diagnose-Lookup (optional)

- [ ] Endpoint z. B. `GET …/orders/active?customer=&booking=` (aktive Order zu IDs) für Support / Debug
- [ ] Kein Blocker für AMS Phase 21

---

## 3. Priorität

| Prio | Slice | Wann |
|------|--------|------|
| P0 | **C0 Audit** | immer zuerst |
| P0 | **C1** | wenn Audit Lücken zeigt; sonst nur Regression |
| P1 | **C2** | sinnvoll gegen Portal/Dropbox-Mismatch |
| P2 | **C3** | wenn Server heute überschreibt oder Merge unklar |
| P3 | **C4** | optional |

**Minimal sinnvolles Paket:** C0 → (C1 falls nötig) → C2.

---

## 4. Abnahme (Cloud + AMS gemeinsam)

1. Erst-Upload mit Kunden- + Booking-ID → `order_id` + `final_url` notieren.
2. Append / Auto-ID-Append mit `existing_order_id` → Response: gleiche `order_id` + gleiche `final_url`.
3. Kundenportal unter dem **alten** Link zeigt die neuen Dateien.
4. (C2) Ohne `existing_order_id`, gleiche customer+booking → Reuse oder klarer Fehler.
5. Kollisionsdatei: AMS `(1)` landet; Server überschreibt das Original nicht.

---

## 5. Nicht-Ziele

- Neuer Kundenlink bei gleichen IDs
- Automatisches Löschen „falscher“ Dateien in Dropbox/Cloud
- AMS- oder ATS-Code in diesem Cloud-Repo
- Kunden-Notify (macht AMS; Auto-ID-Append notify’t, explizites Append nicht)

---

## 6. Agent-Prompt (Cloud-Repo)

```
Audit + Härtung orders/create für Append (AMS Phase 14/21).
Spec: @docs/CLOUD_APPEND_CONTRACT.md (AMS-Repo) — Slices C0→C1→C2.
existing_order_id → Dateien an bestehende Order; final_url unverändert.
Optional C2: customer+booking ohne existing_order_id → reuse oder klarer Fehler.
Keine neuen Portal-Links bei Nachreichen. Tests: Erst-Upload → Append.
```

---

## 7. AMS-Referenz (nur lesen)

| Bereich | Pfad |
|---------|------|
| Manifest-Builder | `src-tauri/src/cloud/manifest.rs` |
| `orders/create` Client | `src-tauri/src/cloud/custom_api/orders.rs` |
| Append / `existing_order_id` | `src-tauri/src/upload/append.rs` |
| Auto-ID-Route | `src-tauri/src/monitor/service.rs`, Phase 21 in `IMPLEMENTATION_PLAN.md` |
| Handoff Append | `docs/HANDOFF.md` §6.1 / §6.1b |
