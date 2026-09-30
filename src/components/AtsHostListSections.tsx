import {ChevronDown} from "lucide-react";
import {useState, type ReactNode} from "react";
import {
  atsPresenceChipLabel,
  atsPresenceChipTone,
  atsPresenceRowClass,
  groupAtsHostsByPresence,
  type AtsHostGroups,
} from "@/lib/atsPresence";
import type {AtsHostSummary} from "@/lib/tauri";
import {Button} from "@/components/ui/button";

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

function eventTypeLabel(value: string): string {
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

function HostRow({
  host,
  selected,
  onSelect,
}: {
  host: AtsHostSummary;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      key={host.instance_id}
      type="button"
      className={atsPresenceRowClass(host, selected)}
      onClick={onSelect}
    >
      <div className="min-w-0 space-y-1.5">
        <div className="flex min-w-0 flex-wrap items-center gap-1.5">
          <span className="min-w-0 max-w-full truncate font-medium text-foreground">
            {host.hostname}
          </span>
          <PresenceChip
            label={atsPresenceChipLabel(host)}
            tone={atsPresenceChipTone(host)}
          />
        </div>
        <p className="truncate text-[11px] text-muted">
          {host.ats_app || "ATS"} {host.ats_version || ""}
        </p>
        <p
          className="truncate font-mono text-[10px] text-muted/80"
          title={host.instance_id}
        >
          {host.instance_id}
        </p>
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted">
          <span className="truncate">{eventTypeLabel(host.last_event_type)}</span>
          <span className="text-border" aria-hidden>
            ·
          </span>
          <span className="tabular-nums">{formatTimestamp(host.last_seen_at)}</span>
          <span className="rounded bg-muted/50 px-1.5 py-0.5">
            Events: {host.activity_count_ttl}
          </span>
          <span className="rounded bg-muted/50 px-1.5 py-0.5">
            Jobs: {host.jobs_count_ttl}
          </span>
        </div>
      </div>
    </button>
  );
}

function HostSection({
  title,
  hosts,
  selectedHostId,
  onSelectHost,
  collapsible = false,
  defaultCollapsed = false,
  headerAction,
}: {
  title: string;
  hosts: AtsHostSummary[];
  selectedHostId: string;
  onSelectHost: (instanceId: string) => void;
  collapsible?: boolean;
  defaultCollapsed?: boolean;
  headerAction?: ReactNode;
}) {
  const [collapsed, setCollapsed] = useState(defaultCollapsed);
  if (hosts.length === 0) return null;

  const body = hosts.map((host) => (
    <HostRow
      key={host.instance_id}
      host={host}
      selected={selectedHostId === host.instance_id}
      onSelect={() => onSelectHost(host.instance_id)}
    />
  ));

  const titleText = (
    <span>
      {title} ({hosts.length})
    </span>
  );

  if (!collapsible) {
    return (
      <div className="min-w-0 space-y-2">
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
          <p className="text-[11px] font-semibold uppercase tracking-wide text-muted">
            {titleText}
          </p>
          {headerAction}
        </div>
        <div className="min-w-0 space-y-2">{body}</div>
      </div>
    );
  }

  return (
    <div className="min-w-0 space-y-2">
      <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-2 text-left text-[11px] font-semibold uppercase tracking-wide text-muted"
          onClick={() => setCollapsed((prev) => !prev)}
        >
          <span className="min-w-0 truncate">{titleText}</span>
          <ChevronDown
            className={`h-4 w-4 shrink-0 transition-transform ${collapsed ? "-rotate-90" : ""}`}
          />
        </button>
        {headerAction ? <div className="shrink-0">{headerAction}</div> : null}
      </div>
      {!collapsed ? <div className="min-w-0 space-y-2">{body}</div> : null}
    </div>
  );
}

type Props = {
  hosts: AtsHostSummary[];
  selectedHostId: string;
  onSelectHost: (instanceId: string) => void;
  emptyMessage?: string;
  onPurgeInactiveLong?: () => void;
  purgeInactiveLongBusy?: boolean;
};

export function AtsHostListSections({
  hosts,
  selectedHostId,
  onSelectHost,
  emptyMessage = "Noch keine bekannten ATS-Clients.",
  onPurgeInactiveLong,
  purgeInactiveLongBusy = false,
}: Props) {
  const groups: AtsHostGroups = groupAtsHostsByPresence(hosts);
  const total =
    groups.connected.length + groups.disconnected.length + groups.inactiveLong.length;

  if (total === 0) {
    return (
      <div className="rounded-lg border border-dashed border-border/60 bg-muted/10 px-4 py-6 text-sm text-muted">
        {emptyMessage}
      </div>
    );
  }

  const purgeAction =
    onPurgeInactiveLong && groups.inactiveLong.length > 0 ? (
      <Button
        type="button"
        variant="secondary"
        size="sm"
        disabled={purgeInactiveLongBusy}
        onClick={(e) => {
          e.stopPropagation();
          onPurgeInactiveLong();
        }}
      >
        Aufräumen
      </Button>
    ) : null;

  return (
    <div className="min-w-0 space-y-3 sm:space-y-4">
      <HostSection
        title="Verbunden"
        hosts={groups.connected}
        selectedHostId={selectedHostId}
        onSelectHost={onSelectHost}
      />
      <HostSection
        title="Nicht verbunden"
        hosts={groups.disconnected}
        selectedHostId={selectedHostId}
        onSelectHost={onSelectHost}
      />
      <HostSection
        title="Länger inaktiv"
        hosts={groups.inactiveLong}
        selectedHostId={selectedHostId}
        onSelectHost={onSelectHost}
        collapsible
        defaultCollapsed
        headerAction={purgeAction}
      />
    </div>
  );
}

export function countConnectedAtsHosts(hosts: AtsHostSummary[]): number {
  return groupAtsHostsByPresence(hosts).connected.length;
}

export function countActiveAtsHosts(hosts: AtsHostSummary[]): number {
  return hosts.filter((host) => host.is_active).length;
}
