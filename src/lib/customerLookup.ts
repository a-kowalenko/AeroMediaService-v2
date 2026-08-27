/** Kundenaufnahme ID-Lookup helpers (Phase 19b). */

import type { CustomerDraft, IntakeLookupHit } from "./tauri";

export const LOOKUP_MIN_ID_DIGITS = 4;
/** Wait after last ID keystroke before calling Customer-API. */
export const LOOKUP_DEBOUNCE_MS = 650;

export type ContactFieldKey = "vorname" | "nachname" | "email" | "telefon";

export type IntakeFieldDiff = {
  field: ContactFieldKey;
  label: string;
  formValue: string;
  apiValue: string;
};

export function sanitizeNumericIdInput(raw: string): string {
  return raw.replace(/\D+/g, "");
}

export function isLookupIdReady(id: string | null | undefined): boolean {
  const t = (id ?? "").trim();
  return t.length >= LOOKUP_MIN_ID_DIGITS && /^\d+$/.test(t);
}

export function isLookupIdPairReady(
  kundenId: string | null | undefined,
  bookingId: string | null | undefined,
): boolean {
  return isLookupIdReady(kundenId) && isLookupIdReady(bookingId);
}

export function lookupIdLengthHint(id: string | null | undefined): string | null {
  const t = (id ?? "").trim();
  if (t.length === 0 || isLookupIdReady(t)) return null;
  return `Mind. ${LOOKUP_MIN_ID_DIGITS} Ziffern`;
}

export function customerHasApiIds(
  customer: Pick<{ kunden_id?: string; booking_id?: string }, "kunden_id" | "booking_id">,
): boolean {
  return Boolean((customer.kunden_id ?? "").trim() && (customer.booking_id ?? "").trim());
}

/** Match by e-mail and/or kunden_id+booking_id pair. */
export function isSameCustomerIdentity(
  a: { email?: string | null; kunden_id?: string | null; booking_id?: string | null },
  b: { email?: string | null; kunden_id?: string | null; booking_id?: string | null },
): boolean {
  const emailA = (a.email ?? "").trim().toLowerCase();
  const emailB = (b.email ?? "").trim().toLowerCase();
  if (emailA && emailB && emailA === emailB) return true;
  const kidA = (a.kunden_id ?? "").trim();
  const bidA = (a.booking_id ?? "").trim();
  const kidB = (b.kunden_id ?? "").trim();
  const bidB = (b.booking_id ?? "").trim();
  return Boolean(kidA && bidA && kidA === kidB && bidA === bidB);
}

export function existingCustomerWarningLabel(
  existing: { vorname: string; nachname: string; processed?: boolean },
): string {
  const name = `${existing.vorname} ${existing.nachname}`.trim() || "Kunde";
  const where = existing.processed ? "als erledigt" : "in der Warteschlange";
  return `Bereits vorhanden: ${name} (${where})`;
}

/** API/ISO → Anzeige `TT.MM.YYYY`; bereits deutsches Format bleibt. */
export function toDisplayBookingDate(raw: string | null | undefined): string {
  const t = (raw ?? "").trim();
  if (!t) return "";
  const iso = /^(\d{4})-(\d{2})-(\d{2})/.exec(t);
  if (iso) return `${iso[3]}.${iso[2]}.${iso[1]}`;
  if (/^\d{1,2}\.\d{1,2}\.\d{4}$/.test(t)) {
    const [d, m, y] = t.split(".");
    return `${d.padStart(2, "0")}.${m.padStart(2, "0")}.${y}`;
  }
  return t;
}

/** Anzeige `TT.MM.YYYY` → Speicherung `YYYY-MM-DD` (ISO bleibt). */
export function toStorageBookingDate(raw: string | null | undefined): string {
  const t = (raw ?? "").trim();
  if (!t) return "";
  const de = /^(\d{1,2})\.(\d{1,2})\.(\d{4})$/.exec(t);
  if (de) {
    return `${de[3]}-${de[2].padStart(2, "0")}-${de[1].padStart(2, "0")}`;
  }
  const iso = /^(\d{4})-(\d{2})-(\d{2})/.exec(t);
  if (iso) return `${iso[1]}-${iso[2]}-${iso[3]}`;
  return t;
}

export function formToLookupHit(form: {
  vorname: string;
  nachname: string;
  email: string;
  telefon: string;
  kunden_id?: string;
  booking_id?: string;
  booking_date?: string;
  typ?: string;
  handcam_foto?: boolean;
  handcam_video?: boolean;
  outside_foto?: boolean;
  outside_video?: boolean;
  ist_bezahlt_handcam_foto?: boolean;
  ist_bezahlt_handcam_video?: boolean;
  ist_bezahlt_outside_foto?: boolean;
  ist_bezahlt_outside_video?: boolean;
  media_option?: string;
}): IntakeLookupHit {
  return {
    vorname: form.vorname,
    nachname: form.nachname,
    email: form.email,
    telefon: form.telefon,
    kunden_id: form.kunden_id ?? "",
    booking_id: form.booking_id ?? "",
    booking_date: form.booking_date ?? "",
    typ: form.typ ?? "",
    handcam_foto: Boolean(form.handcam_foto),
    handcam_video: Boolean(form.handcam_video),
    outside_foto: Boolean(form.outside_foto),
    outside_video: Boolean(form.outside_video),
    ist_bezahlt_handcam_foto: Boolean(form.ist_bezahlt_handcam_foto),
    ist_bezahlt_handcam_video: Boolean(form.ist_bezahlt_handcam_video),
    ist_bezahlt_outside_foto: Boolean(form.ist_bezahlt_outside_foto),
    ist_bezahlt_outside_video: Boolean(form.ist_bezahlt_outside_video),
    media_option: form.media_option ?? "",
  };
}

export function contactFieldDiffs(
  form: IntakeLookupHit,
  api: IntakeLookupHit,
): IntakeFieldDiff[] {
  const pairs: Array<[ContactFieldKey, string, string, string]> = [
    ["vorname", "Vorname", form.vorname, api.vorname],
    ["nachname", "Nachname", form.nachname, api.nachname],
    ["email", "E-Mail", form.email, api.email],
    ["telefon", "Telefon", form.telefon, api.telefon],
  ];
  const out: IntakeFieldDiff[] = [];
  for (const [field, label, formValue, apiValue] of pairs) {
    const f = (formValue ?? "").trim();
    const a = (apiValue ?? "").trim();
    if (!f || !a) continue;
    if (f.toLowerCase() === a.toLowerCase()) continue;
    out.push({ field, label, formValue: f, apiValue: a });
  }
  return out;
}

/** Empty contact from API; media/IDs/typ/booking_date always from API when present. */
export function mergeLookupIntoForm(
  form: IntakeLookupHit,
  api: IntakeLookupHit,
): IntakeLookupHit {
  const apiDate = toDisplayBookingDate(api.booking_date);
  return {
    vorname: (form.vorname ?? "").trim() ? form.vorname : (api.vorname ?? ""),
    nachname: (form.nachname ?? "").trim() ? form.nachname : (api.nachname ?? ""),
    email: (form.email ?? "").trim() ? form.email : (api.email ?? ""),
    telefon: (form.telefon ?? "").trim() ? form.telefon : (api.telefon ?? ""),
    kunden_id: api.kunden_id ?? form.kunden_id ?? "",
    booking_id: api.booking_id ?? form.booking_id ?? "",
    booking_date: apiDate || toDisplayBookingDate(form.booking_date),
    typ: (api.typ ?? "").trim() ? api.typ : (form.typ ?? ""),
    handcam_foto: Boolean(api.handcam_foto),
    handcam_video: Boolean(api.handcam_video),
    outside_foto: Boolean(api.outside_foto),
    outside_video: Boolean(api.outside_video),
    ist_bezahlt_handcam_foto: Boolean(api.ist_bezahlt_handcam_foto ?? api.handcam_foto),
    ist_bezahlt_handcam_video: Boolean(api.ist_bezahlt_handcam_video ?? api.handcam_video),
    ist_bezahlt_outside_foto: Boolean(api.ist_bezahlt_outside_foto ?? api.outside_foto),
    ist_bezahlt_outside_video: Boolean(api.ist_bezahlt_outside_video ?? api.outside_video),
    media_option: api.media_option ?? form.media_option ?? "",
  };
}

export function applyDiffResolutions(
  form: IntakeLookupHit,
  api: IntakeLookupHit,
  resolutions: Partial<Record<ContactFieldKey, "api" | "form">>,
): IntakeLookupHit {
  const base = mergeLookupIntoForm(form, api);
  const pick = (key: ContactFieldKey): string => {
    const choice = resolutions[key] ?? "form";
    return choice === "api" ? api[key] : form[key].trim() ? form[key] : api[key];
  };
  return {
    ...base,
    vorname: pick("vorname"),
    nachname: pick("nachname"),
    email: pick("email"),
    telefon: pick("telefon"),
  };
}

export function applyAllApi(form: IntakeLookupHit, api: IntakeLookupHit): IntakeLookupHit {
  return applyDiffResolutions(form, api, {
    vorname: "api",
    nachname: "api",
    email: "api",
    telefon: "api",
  });
}

export function mediaFlagsSummary(hit: Pick<
  IntakeLookupHit,
  | "typ"
  | "handcam_foto"
  | "handcam_video"
  | "outside_foto"
  | "outside_video"
>): string {
  const typ = (hit.typ || "").trim();
  const parts: string[] = [];
  if (hit.outside_video || hit.outside_foto) {
    const kinds = [
      hit.outside_video ? "Video" : "",
      hit.outside_foto ? "Foto" : "",
    ].filter(Boolean);
    parts.push(kinds.length ? `Outside ${kinds.join("/")}` : "Outside");
  }
  if (hit.handcam_video || hit.handcam_foto) {
    const kinds = [
      hit.handcam_video ? "Video" : "",
      hit.handcam_foto ? "Foto" : "",
    ].filter(Boolean);
    parts.push(kinds.length ? `Handcam ${kinds.join("/")}` : "Handcam");
  }
  if (parts.length) return parts.join(" · ");
  return typ || "";
}

export function draftFromForm(form: {
  vorname: string;
  nachname: string;
  email: string;
  telefon: string;
  kunden_id: string;
  booking_id: string;
  booking_date: string;
  typ: string;
  handcam_foto: boolean;
  handcam_video: boolean;
  outside_foto: boolean;
  outside_video: boolean;
  ist_bezahlt_handcam_foto: boolean;
  ist_bezahlt_handcam_video: boolean;
  ist_bezahlt_outside_foto: boolean;
  ist_bezahlt_outside_video: boolean;
  media_option: string;
}): CustomerDraft {
  return {
    vorname: form.vorname.trim(),
    nachname: form.nachname.trim(),
    email: form.email.trim(),
    telefon: form.telefon.trim(),
    kunden_id: form.kunden_id.trim(),
    booking_id: form.booking_id.trim(),
    booking_date: toStorageBookingDate(form.booking_date),
    typ: form.typ.trim(),
    handcam_foto: form.handcam_foto,
    handcam_video: form.handcam_video,
    outside_foto: form.outside_foto,
    outside_video: form.outside_video,
    // Gebucht = paid; ungebucht = nicht paid.
    ist_bezahlt_handcam_foto: form.handcam_foto,
    ist_bezahlt_handcam_video: form.handcam_video,
    ist_bezahlt_outside_foto: form.outside_foto,
    ist_bezahlt_outside_video: form.outside_video,
    media_option: form.media_option.trim(),
  };
}
