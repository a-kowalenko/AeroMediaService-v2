# AMS — ATS Cloud-Lookup-Fallback (Phase 22)

> **Partner-Plan:** Master in ATS `docs/CLOUD_LOOKUP_FALLBACK_PLAN.md`  
> Bridge-Basis: [`HANDOFF.md`](./HANDOFF.md) §9  
> **Deploy:** nach Cloud-Slices C0–C4; vor ATS Phase 53.

**Status:** ✅ A0–A3  
**Ziel:** Nach erfolgreicher Bridge-Auth stellt AMS für die ATS-Instanz ein Cloud-Client-JWT aus (Cloud issue proxyen) und liefert die `cloud_base_url`. Customer-API-Keys bleiben in AMS/Cloud — ATS bekommt nur das JWT.

> Eine Session = **ein Slice A0–A3**.

---

## Leitentscheidungen

| # | Thema | Entscheidung |
|---|--------|----------------|
| 1 | Issuer | Cloud stellt JWT; AMS ist Bootstrap-Proxy |
| 2 | Auth zum Cloud-Issue | bestehender Cloud-API-Key (`custom_api_bearer_token`) + neue Permission `ats_client_token` |
| 3 | Bridge-Route | `POST /v1/client-token` (Bridge-Token-Auth + ATS-Identity-Header) |
| 4 | Health | optional Hint `cloud_lookup.base_url` + Capability `cloud-lookup-v1` wenn Issue konfiguriert |
| 5 | Lookup-Primärweg | unverändert `POST /v1/customer/lookup` → Customer-API |
| 6 | Cloud-URL | aus AMS-Config/Secret (meist Custom-API-Base); an ATS durchreichen |

---

## Config / Secrets (A0)

Kein neuer Setting-Key: **Reuse** der Skydive-Media-Secrets.

| Key (OS-Keyring) | Zweck |
|------------------|--------|
| `custom_api_url` | Cloud-Base (oft `https://…/api`); Origin = `api_origin()` → `cloud_base_url` für ATS |
| `custom_api_bearer_token` | Cloud-API-Key (`keyId.secret`); **muss** Permission `ats_client_token` haben |

| Konstante / Code | Wert |
|------------------|------|
| Issue-Pfad | `POST {origin}/api/ats/v1/client-token` |
| Fehlercode fehlende Base | `cloud_base_missing` |
| Fehlercode fehlender Key | `cloud_api_key_missing` |
| Modul | `src-tauri/src/cloud/custom_api/ats_client_token.rs` |
| IPC Status | `get_cloud_lookup_issue_status` |

**Betrieb:** Cloud-Admin → API Keys → Permission `ats_client_token` (Preset „ATS Client-Token (AMS)“ oder manuell). Fehlende Creds → Warn-Log + klarer Fehlercode (kein stiller Fail). Settings → Skydive Media zeigt Amber-Hinweis wenn URL/Token leer.

---

## Slices

### A0 — Cloud-Base für Token-Issue ✅

- Sicherstellen: AMS kennt Cloud-Base-URL für `POST /api/ats/v1/client-token` (reuse Custom-API base oder eigener Setting-Key)
- API-Key muss Permission `ats_client_token` haben (Cloud-Admin / Key-Update)
- Settings-UI: Hinweis wenn Base/Key fehlen (kein stiller Fail ohne Log)

**DoD:** Config/Secrets dokumentiert; fehlende Creds → klarer Fehlercode.

### A1 — `POST /v1/client-token` ✅

- Bridge-Auth wie andere `/v1/*`-Routen
- Liest `X-Ats-Instance-Id` (+ Hostname/Version/App)
- Ruft Cloud C1 auf; mapped Response an ATS:

```json
{
  "access_token": "…",
  "expires_at": "…",
  "expires_in": 172800,
  "cloud_base_url": "https://…",
  "scope": ["customer.lookup"]
}
```

- Fehler: Cloud unreachable / 401/403 → Bridge-Fehler mit verständlicher Message
- Unit-Tests: Request-Bau, Identity-Pflicht, Fehler-Mapping

**DoD:** ATS kann mit Bridge-Token ein JWT holen (gegen Staging/Mock).

### A2 — Health-Hint + Capability ✅

- Wenn Cloud-Issue konfiguriert: Capability `cloud-lookup-v1`
- Additives Feld z. B. `cloud_lookup: { "base_url": "https://…" }` (keine Tokens in Health)
- Tests: Capability nur wenn konfiguriert; Health bleibt schlank

**DoD:** ATS kann URL auch ohne sofortigen Token-Call sehen (optional UX).

### A3 — Docs + Abnahme ✅

- [`HANDOFF.md`](./HANDOFF.md) §9.4: Client-Token + Capability `cloud-lookup-v1` (Wire, Fehlercodes, ATS-Routing)
- Index `IMPLEMENTATION_PLAN.md`: Phase 22 Checkboxen + Abnahme-Checkliste
- Manuell: Issue mit gültigem Key; 403 ohne Permission; Bridge ohne Identity → 4xx

**DoD:** Partner-Vertrag für ATS T0+ grün.

---

## Non-Goals

- JWT selbst signieren in AMS (außer explizit später vereinbart — **nein**, Cloud signed)
- Customer-API-Keys an ATS weitergeben
- Cloud-Lookup-Proxy in AMS (ATS spricht Cloud direkt)
- Änderungen an Upload-/Append-Pipeline

---

## Dateien (erwartet)

| Bereich | Pfad |
|---------|------|
| Bridge Routes | `src-tauri/src/bridge/server.rs` |
| Types | `src-tauri/src/bridge/types.rs` |
| Cloud HTTP | `src-tauri/src/cloud/custom_api/` (neu `ats_client_token.rs` o. ä.) |
| Secrets | `src-tauri/src/storage/secrets.rs`, Settings-UI |
| Docs | `docs/HANDOFF.md`, dieser File, `IMPLEMENTATION_PLAN.md` |

---

## Schnell-Prompt

```
Implementiere Slice A3 aus docs/ATS_CLOUD_LOOKUP_FALLBACK.md
Bridge: docs/HANDOFF.md
Master: ATS docs/CLOUD_LOOKUP_FALLBACK_PLAN.md
Nur A3. Danach cargo test.
```

---

## Checkboxen

- [x] A0 Cloud-Base / Key-Permission
- [x] A1 `POST /v1/client-token`
- [x] A2 Health `cloud-lookup-v1`
- [x] A3 HANDOFF + Abnahme
- [ ] Cloud C0–C4 deployed
