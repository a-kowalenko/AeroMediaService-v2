import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";
import type { HistoryAppendEvent } from "./tauri";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatHistoryDate(raw: string): string {
  if (!raw) return "";
  if (raw.includes("T")) {
    try {
      const [dPart, tPart = ""] = raw.split("T");
      const [y, m, d] = dPart.split("-");
      if (y && m && d) {
        return `${d}.${m}.${y} ${tPart.slice(0, 8)}`;
      }
    } catch {
      return raw.slice(0, 19).replace("T", " ");
    }
  }
  return raw.slice(0, 19).replace("T", " ");
}

export function historyDisplayName(item: {
  first_name?: string;
  last_name?: string;
  dir_name?: string;
}): string {
  const name = `${item.first_name ?? ""} ${item.last_name ?? ""}`.trim();
  return name || item.dir_name || "Unbekannt";
}

export type StatusChannel = "upload" | "email" | "sms" | "overall" | "generic";

export type OverallStatusTone =
  | "error"
  | "warning"
  | "active"
  | "success"
  | "skipped"
  | "muted";

function normalizeStatus(status: string): string {
  return (status || "").trim().toLowerCase();
}

function isErrorStatus(lower: string): boolean {
  if (!lower || lower === "—" || lower === "-") return false;
  return (
    lower.includes("problem") ||
    lower.includes("fehler") ||
    lower.includes("fehlgeschlagen") ||
    lower.includes("abgebrochen") ||
    lower.includes("abgelehnt") ||
    lower.includes("reject") ||
    lower.includes("notdelivered")
  );
}

function isActiveStatus(lower: string): boolean {
  return (
    lower.includes("in bearbeitung") ||
    lower.includes("gestartet") ||
    lower.includes("übertragen") ||
    lower.includes("gepuffert") ||
    lower.includes("akzeptiert") ||
    lower.includes("wartet") ||
    lower.includes("läuft")
  );
}

/**
 * Map history status labels to chip tones.
 * Channel-aware: e.g. E-Mail „Gesendet“ = success, SMS „Gesendet“ = active (noch nicht zugestellt).
 */
export function overallStatusTone(
  status: string,
  channel: StatusChannel = "generic",
): OverallStatusTone {
  const raw = (status || "").trim();
  if (!raw || raw === "—" || raw === "-") return "muted";
  const lower = normalizeStatus(raw);

  if (lower === "übersprungen" || lower === "uebersprungen") {
    return "skipped";
  }
  if (isErrorStatus(lower)) return "error";

  // Exact / known labels first
  switch (lower) {
    case "komplett":
    case "erfolgreich":
    case "zugestellt":
      return "success";
    case "versendet":
      // Overall: raus, SMS ggf. noch ohne DLR — positiv, nicht Warnung
      return "success";
    case "gesendet":
      // E-Mail: beste Stufe. SMS: gesendet, aber noch nicht zugestellt.
      if (channel === "sms") return "active";
      return "success";
    case "teilweise":
      return "warning";
    case "unbekannt":
      return "muted";
    default:
      break;
  }

  if (isActiveStatus(lower)) return "active";

  if (lower.includes("zugestellt") || lower.includes("erfolgreich")) {
    return "success";
  }
  if (lower.includes("komplett") || lower.includes("versendet")) {
    return "success";
  }
  // „Gesendet“ substring (e.g. longer messages) — same channel rule
  if (lower.includes("gesendet")) {
    if (channel === "sms") return "active";
    return "success";
  }
  if (lower.includes("teilweise")) return "warning";

  return "muted";
}

export function overallStatusColor(
  status: string,
  channel: StatusChannel = "generic",
): string {
  switch (overallStatusTone(status, channel)) {
    case "error":
      return "var(--ams-destructive)";
    case "active":
      return "var(--ams-primary)";
    case "success":
      return "var(--ams-success)";
    case "warning":
      return "var(--ams-warning)";
    case "skipped":
    default:
      return "var(--ams-muted)";
  }
}

export const RETRYABLE_STATUSES = new Set(["Fehler", "Abgebrochen"]);

export function canRetryUpload(status: string): boolean {
  return RETRYABLE_STATUSES.has((status || "").trim());
}

export function canResendNotifications(status: string): boolean {
  return (status || "").trim() === "Erfolgreich";
}

export function canAppendMedia(status: string): boolean {
  return canResendNotifications(status);
}

export function extraString(entry: { extra?: Record<string, unknown> }, key: string): string {
  const value = entry.extra?.[key];
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return "";
}

export function extraNumber(entry: { extra?: Record<string, unknown> }, key: string): number {
  const value = entry.extra?.[key];
  if (typeof value === "number") return value;
  if (typeof value === "string") return Number(value) || 0;
  return 0;
}

export function extraBool(entry: { extra?: Record<string, unknown> }, key: string): boolean {
  const value = entry.extra?.[key];
  if (typeof value === "boolean") return value;
  if (typeof value === "number") return value !== 0;
  if (typeof value === "string") {
    const lower = value.trim().toLowerCase();
    return lower === "1" || lower === "true" || lower === "yes" || lower === "ja";
  }
  return false;
}

export function historyAppendEvents(entry: {
  extra?: Record<string, unknown>;
}): HistoryAppendEvent[] {
  const raw = entry.extra?.append_events;
  if (!Array.isArray(raw)) return [];
  return raw.filter((item): item is HistoryAppendEvent => Boolean(item && typeof item === "object"));
}

export function formatResendHistorySummary(entry: { extra?: Record<string, unknown> }): string {
  const emailCount = extraNumber(entry, "email_resend_count");
  const smsCount = extraNumber(entry, "sms_resend_count");
  const parts: string[] = [];
  if (emailCount) parts.push(`E-Mail ${emailCount}× erneut`);
  if (smsCount) parts.push(`SMS ${smsCount}× erneut`);
  return parts.length ? parts.join(" | ") : "Keine Wiederversände";
}

export function formatManualStatusSummary(entry: {
  extra?: Record<string, unknown>;
}): string {
  if (!extraBool(entry, "manual_status_override")) return "—";
  const action = extraString(entry, "manual_status_action").trim() || "Manuell";
  const atRaw = extraString(entry, "manual_status_at").trim();
  const atDisplay = atRaw ? atRaw.replace("T", " ").slice(0, 16) : "—";
  const note = extraString(entry, "manual_status_note").trim();
  return note ? `${action} (${atDisplay}) — ${note}` : `${action} (${atDisplay})`;
}

export type HistoryBookingFlags = {
  handcam_foto: boolean;
  handcam_video: boolean;
  outside_foto: boolean;
  outside_video: boolean;
  ist_bezahlt_handcam_foto: boolean;
  ist_bezahlt_handcam_video: boolean;
  ist_bezahlt_outside_foto: boolean;
  ist_bezahlt_outside_video: boolean;
};

export type ProductBadge = { key: string; label: string; paid: boolean };

const BOOKING_FLAG_KEYS: (keyof HistoryBookingFlags)[] = [
  "handcam_foto",
  "handcam_video",
  "outside_foto",
  "outside_video",
  "ist_bezahlt_handcam_foto",
  "ist_bezahlt_handcam_video",
  "ist_bezahlt_outside_foto",
  "ist_bezahlt_outside_video",
];

function truthyFlag(v: unknown): boolean {
  if (v === true || v === 1) return true;
  if (typeof v === "number") return v !== 0;
  if (typeof v !== "string") return false;
  const s = v.trim().toLowerCase();
  return s === "true" || s === "1" || s === "yes" || s === "ja";
}

function markerObject(markerRaw: string | undefined): Record<string, unknown> {
  try {
    const parsed = JSON.parse(markerRaw || "{}") as unknown;
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>;
    }
  } catch {
    /* ignore */
  }
  return {};
}

export function historyBookingFlags(entry: {
  extra?: Record<string, unknown>;
  marker_raw?: string;
}): HistoryBookingFlags {
  const marker = markerObject(entry.marker_raw);
  const extra = entry.extra ?? {};
  const read = (key: keyof HistoryBookingFlags): boolean => {
    if (key in extra) return truthyFlag(extra[key]);
    return truthyFlag(marker[key]);
  };
  return {
    handcam_foto: read("handcam_foto"),
    handcam_video: read("handcam_video"),
    outside_foto: read("outside_foto"),
    outside_video: read("outside_video"),
    ist_bezahlt_handcam_foto: read("ist_bezahlt_handcam_foto"),
    ist_bezahlt_handcam_video: read("ist_bezahlt_handcam_video"),
    ist_bezahlt_outside_foto: read("ist_bezahlt_outside_foto"),
    ist_bezahlt_outside_video: read("ist_bezahlt_outside_video"),
  };
}

export function overlayBookingFlags(
  base: HistoryBookingFlags,
  patch?: Partial<HistoryBookingFlags> | Record<string, unknown>,
): HistoryBookingFlags {
  if (!patch) return base;
  const next = { ...base };
  for (const key of BOOKING_FLAG_KEYS) {
    if (key in patch) {
      next[key] = truthyFlag(patch[key]);
    }
  }
  return next;
}

export function historyProductBadges(flags: HistoryBookingFlags): ProductBadge[] {
  const badges: ProductBadge[] = [];
  if (flags.handcam_video) {
    badges.push({ key: "hv", label: "HV", paid: flags.ist_bezahlt_handcam_video });
  }
  if (flags.handcam_foto) {
    badges.push({ key: "hf", label: "HF", paid: flags.ist_bezahlt_handcam_foto });
  }
  if (flags.outside_video) {
    badges.push({ key: "ov", label: "OV", paid: flags.ist_bezahlt_outside_video });
  }
  if (flags.outside_foto) {
    badges.push({ key: "of", label: "OF", paid: flags.ist_bezahlt_outside_foto });
  }
  return badges;
}

export function historyCanRefreshBookingFlags(entry: {
  extra?: Record<string, unknown>;
  marker_raw?: string;
  customer_number?: string;
  booking_number?: string;
  type?: string;
}): boolean {
  const marker = markerObject(entry.marker_raw);
  const hasHash =
    typeof marker.kunden_id_hash === "string" &&
    marker.kunden_id_hash.trim() !== "" &&
    typeof marker.booking_id_hash === "string" &&
    marker.booking_id_hash.trim() !== "";
  const hasId =
    typeof marker.kunden_id === "string" &&
    marker.kunden_id.trim() !== "" &&
    typeof marker.booking_id === "string" &&
    marker.booking_id.trim() !== "";
  if (hasHash || hasId) return true;
  const customer = (entry.customer_number ?? "").trim();
  const booking = (entry.booking_number ?? "").trim();
  const type = (entry.type ?? "").trim();
  return Boolean(customer && booking && type);
}
