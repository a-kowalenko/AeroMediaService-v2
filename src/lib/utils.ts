import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

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

export function overallStatusColor(status: string): string {
  const lower = status.toLowerCase();
  if (lower.includes("problem") || lower.includes("fehler") || lower.includes("fehlgeschlagen")) {
    return "var(--ams-destructive)";
  }
  if (lower.includes("in bearbeitung") || lower.includes("gestartet")) {
    return "var(--ams-primary)";
  }
  if (lower.includes("komplett")) {
    return "var(--ams-success)";
  }
  if (lower.includes("versendet")) {
    return "color-mix(in srgb, var(--ams-success) 70%, white)";
  }
  if (lower.includes("erfolgreich") || lower.includes("zugestellt")) {
    return "var(--ams-success)";
  }
  if (lower.includes("gesendet") || lower.includes("teilweise")) {
    return "var(--ams-warning)";
  }
  return "var(--ams-muted)";
}

export const RETRYABLE_STATUSES = new Set(["Fehler", "Abgebrochen"]);

export function canRetryUpload(status: string): boolean {
  return RETRYABLE_STATUSES.has((status || "").trim());
}

export function canResendNotifications(status: string): boolean {
  return (status || "").trim() === "Erfolgreich";
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
