# AMS — ATS Client-Diagnostik (Bridge-Forward)

> **Partner-Plan:** Master in ATS `AeroTandemStudio-v2/docs/CLIENT_DIAGNOSTICS_PLAN.md`  
> Bridge-Basis: [`HANDOFF.md`](./HANDOFF.md) §9.5  
> **Deploy:** nach Cloud D0–D2; vor ATS Phase 57.

**Status:** ✅ Abgeschlossen (B0–B3)  
**Ziel:** ATS sendet Diagnostik über die Bridge; AMS leitet an Cloud weiter (API-Key). Kein zweites JWT in AMS — Cloud-Issue-Scope-Erweiterung ist Cloud D0.

> Eine Session = **ein Slice B0–B3**.

---

## Leitentscheidungen

| # | Thema | Entscheidung |
|---|--------|----------------|
| 1 | Bridge-Route | `POST /v1/diagnostics/events` (Bridge-Token + ATS-Identity) |
| 2 | Cloud-Call | `POST {origin}/api/ats/v1/diagnostics/events` mit `custom_api_bearer_token` |
| 3 | Permission | Cloud-API-Key: **`ats_diagnostics_ingest`** (neben `ats_client_token`) |
| 4 | Health | Capability **`diagnostics-v1`** wenn Base + Key konfiguriert |
| 5 | Body | 1:1 ATS JSON durchreichen; AMS setzt keine PII hinzu |
| 6 | Client-Token | Unverändert `POST /v1/client-token` — Cloud liefert erweitertes `scope` (D0) |

---

## Config / Secrets (B0) ✅

Reuse Skydive-Media / Custom-API wie Phase 22:

| Key | Zweck |
|-----|--------|
| `custom_api_url` | Cloud-Origin |
| `custom_api_bearer_token` | Muss **`ats_diagnostics_ingest`** haben (kann derselbe Key wie `ats_client_token` sein, wenn beide Permissions gesetzt) |

| Konstante | Wert |
|-----------|------|
| Cloud-Pfad | `POST {origin}/api/ats/v1/diagnostics/events` |
| Modul | `src-tauri/src/cloud/custom_api/ats_diagnostics.rs` |
| Fehlercodes | `cloud_base_missing`, `cloud_api_key_missing`, `cloud_forbidden`, `cloud_unreachable`, `cloud_ingest_failed`, … |

**DoD:** Fehlende Permission → verständlicher Bridge-Fehler; kein stilles Drop.

---

## Slices

### B0 — Cloud-Forward-Client ✅

- HTTP-Client: Batch-Body an Cloud D2
- Auth: Bearer API-Key
- Response-Mapping an ATS (`ok`, `accepted`, `duplicates`)
- Unit-Tests: URL-Bau, Auth-Header, Status-Mapping (Mock)

**DoD:** Rust-Modul testbar ohne laufende Cloud.

### B1 — `POST /v1/diagnostics/events` ✅

- Bridge-Auth wie `/v1/customer/lookup`
- Pflicht: `X-Ats-Instance-Id` (+ Hostname/Version/App wie üblich)
- Ruft B0 auf; Timeouts konservativ (Diagnostik nicht hot path)
- Fehler: Cloud 401/403/429 → Bridge-Envelope

**DoD:** ATS R4 kann Events über AMS senden (Staging).

### B2 — Health + Capability ✅

- Wenn Ingest konfiguriert: `diagnostics-v1` in `capabilities[]`
- Additives JSON `diagnostics: { "configured": true }` — **keine** Secrets
- Capability fehlt wenn Key/Base fehlen (Permission wird beim POST gegen Cloud geprüft)

**DoD:** ATS erkennt AMS-Primärweg für Diagnostik.

### B3 — Docs + Abnahme ✅

- [`HANDOFF.md`](./HANDOFF.md) §9.5: Route, Capability, Fehlercodes, Body-Skizze
- `IMPLEMENTATION_PLAN.md`: Phase 23 Eintrag (Diagnostik)
- Manuell: Bridge POST → Cloud Row mit `ingress=ams_bridge`

**DoD:** Partner-Vertrag für ATS R4 grün.

---

## Non-Goals

- Diagnostik in AMS-Datenbank persistieren (Cloud ist Sink)
- JWT in AMS signieren
- Lookup-/Upload-Pipeline ändern

---

## Manuelle Abnahme (B3)

Partner-Vertrag (Kurzfassung): [`HANDOFF.md`](./HANDOFF.md) §9.5 „Manuelle Abnahme“.

1. Cloud-Key mit `ats_diagnostics_ingest` (+ weiterhin `ats_client_token` für Lookup-Issue).
2. `GET /v1/health`: `diagnostics-v1` present.
3. `POST /v1/diagnostics/events` mit Bridge-Token + Identity → `200`.
4. Cloud Admin: Event sichtbar, `ingress=ams_bridge`.
5. Key ohne Permission → `502` / `cloud_forbidden`.
6. `POST /v1/client-token` unverändert funktionsfähig; `scope` enthält ggf. beide Scopes (Cloud D0).

---

## Agent-Prompt

```
Implementiere Slice B3 aus docs/ATS_CLIENT_DIAGNOSTICS.md
Regeln: @AGENTS.md
Spec: docs/ATS_CLIENT_DIAGNOSTICS.md · Bridge-Basis: docs/HANDOFF.md §9
Nur B3 (Docs + Abnahme). Danach cargo test.
```
