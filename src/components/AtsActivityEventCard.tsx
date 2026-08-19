import {
  eventTypeLabel,
  formatActivityPayload,
} from "@/lib/atsActivityDisplay";
import type {AtsActivityEntry} from "@/lib/tauri";

function PresenceChip({
  label,
  tone,
}: {
  label: string;
  tone: "active" | "inactive" | "degraded" | "neutral";
}) {
  const toneClass =
    tone === "active"
      ? "border-success/40 bg-success/10 text-success"
      : tone === "inactive"
        ? "border-border/70 bg-muted/30 text-muted"
        : tone === "degraded"
          ? "border-warning/45 bg-warning/10 text-warning"
          : "border-primary/40 bg-primary/10 text-primary";
  return (
    <span
      className={`inline-flex items-center rounded border px-1.5 py-0.5 text-[10px] font-medium leading-none ${toneClass}`}
    >
      {label}
    </span>
  );
}

function formatTimestamp(value: string): string {
  if (!value.trim()) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("de-DE", {
    dateStyle: "short",
    timeStyle: "short",
  }).format(date);
}

type Props = {
  entry: AtsActivityEntry;
};

export function AtsActivityEventCard({entry}: Props) {
  const payload = formatActivityPayload(entry.payload_json);
  const showMeta =
    entry.correlation_id.trim() ||
    entry.folder_name.trim() ||
    entry.status_code_class.trim();

  return (
    <div className="rounded-md border border-border/50 bg-background/80 p-3 text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <PresenceChip label={eventTypeLabel(entry.event_type)} tone="neutral" />
        <span className="text-muted">{formatTimestamp(entry.occurred_at)}</span>
        <span className="text-muted">
          {entry.method} {entry.route}
        </span>
        {entry.status_code_class ? (
          <span className="text-muted">HTTP {entry.status_code_class}</span>
        ) : null}
      </div>
      {showMeta ? (
        <div className="mt-2 space-y-0.5 text-muted">
          {entry.correlation_id ? <p>Correlation ID: {entry.correlation_id}</p> : null}
          {entry.folder_name ? <p>Ordner: {entry.folder_name}</p> : null}
        </div>
      ) : null}
      {payload ? (
        <details className="mt-2">
          <summary className="cursor-pointer text-[11px] font-medium text-foreground">
            Payload
          </summary>
          <pre className="mt-2 max-h-48 overflow-auto rounded border border-border/40 bg-muted/20 p-2 text-[10px] leading-relaxed text-foreground">
            {payload}
          </pre>
        </details>
      ) : null}
    </div>
  );
}
