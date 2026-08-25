import type {AtsActivityEntry} from "@/lib/tauri";

export function eventTypeLabel(value: string): string {
  switch (value) {
    case "handoff_ready":
      return "Ready";
    case "customer_lookup":
      return "Lookup";
    case "job_status":
      return "Job-Status";
    case "health":
      return "Health";
    default:
      return value || "-";
  }
}

export function hasActivityPayload(entry: AtsActivityEntry): boolean {
  return entry.payload_json.trim().length > 0;
}

export function formatActivityPayload(payloadJson: string): string | null {
  const raw = payloadJson.trim();
  if (!raw) return null;
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

export type AtsActivityGroup =
  | {kind: "single"; entry: AtsActivityEntry}
  | {kind: "health_run"; entries: AtsActivityEntry[]};

/** Groups consecutive `health` events (list order is newest-first). */
export function groupConsecutiveHealthRuns(entries: AtsActivityEntry[]): AtsActivityGroup[] {
  const groups: AtsActivityGroup[] = [];
  for (const entry of entries) {
    const isHealth = entry.event_type === "health";
    const last = groups[groups.length - 1];
    if (isHealth && last?.kind === "health_run") {
      last.entries.push(entry);
      continue;
    }
    if (isHealth) {
      groups.push({kind: "health_run", entries: [entry]});
      continue;
    }
    groups.push({kind: "single", entry});
  }
  return groups;
}

export function formatActivityTimestamp(value: string): string {
  if (!value.trim()) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("de-DE", {
    dateStyle: "short",
    timeStyle: "short",
  }).format(date);
}

export function formatActivityTimeOnly(value: string): string {
  if (!value.trim()) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("de-DE", {
    timeStyle: "short",
  }).format(date);
}

/** Newest-first run: entries[0] = newest, entries[last] = oldest. */
export function healthRunChipLabel(entries: AtsActivityEntry[]): string {
  const count = entries.length;
  const base = eventTypeLabel("health");
  if (count <= 1) return base;
  return `${base} · ${count}×`;
}

/** Time span only (count lives in the chip). */
export function healthRunTimeRange(entries: AtsActivityEntry[]): string {
  const count = entries.length;
  if (count === 0) return "";
  const newest = entries[0]?.occurred_at ?? "";
  const oldest = entries[count - 1]?.occurred_at ?? "";
  if (count === 1) return formatActivityTimestamp(newest);
  const newestDate = new Date(newest);
  const oldestDate = new Date(oldest);
  const sameDay =
    !Number.isNaN(newestDate.getTime()) &&
    !Number.isNaN(oldestDate.getTime()) &&
    newestDate.toDateString() === oldestDate.toDateString();
  const from = sameDay ? formatActivityTimeOnly(oldest) : formatActivityTimestamp(oldest);
  const to = sameDay ? formatActivityTimeOnly(newest) : formatActivityTimestamp(newest);
  return `${from}–${to}`;
}

export const ATS_ACTIVITY_PAGE_SIZE = 10;
