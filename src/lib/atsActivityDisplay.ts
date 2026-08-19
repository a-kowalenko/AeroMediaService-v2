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
