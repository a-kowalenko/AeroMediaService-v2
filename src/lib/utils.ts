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
    return lower === "1" || lower === "true" || lower === "yes";
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
