import type {AtsHostSummary} from "@/lib/tauri";

export type AtsPresenceCategory = "connected" | "disconnected" | "inactive_long";

export type AtsHostGroups = {
  connected: AtsHostSummary[];
  disconnected: AtsHostSummary[];
  inactiveLong: AtsHostSummary[];
};

function sortByLastSeenDesc(hosts: AtsHostSummary[]): AtsHostSummary[] {
  return [...hosts].sort(
    (a, b) => new Date(b.last_seen_at).getTime() - new Date(a.last_seen_at).getTime(),
  );
}

export function groupAtsHostsByPresence(hosts: AtsHostSummary[]): AtsHostGroups {
  const connected: AtsHostSummary[] = [];
  const disconnected: AtsHostSummary[] = [];
  const inactiveLong: AtsHostSummary[] = [];
  for (const host of hosts) {
    switch (host.presence_category) {
      case "connected":
        connected.push(host);
        break;
      case "inactive_long":
        inactiveLong.push(host);
        break;
      default:
        disconnected.push(host);
        break;
    }
  }
  return {
    connected: sortByLastSeenDesc(connected),
    disconnected: sortByLastSeenDesc(disconnected),
    inactiveLong: sortByLastSeenDesc(inactiveLong),
  };
}

export function findAtsHost(hosts: AtsHostSummary[], instanceId: string): AtsHostSummary | null {
  return hosts.find((host) => host.instance_id === instanceId) ?? null;
}

export function defaultAtsHostSelection(hosts: AtsHostSummary[]): string {
  const groups = groupAtsHostsByPresence(hosts);
  return (
    groups.connected[0]?.instance_id ??
    groups.disconnected[0]?.instance_id ??
    groups.inactiveLong[0]?.instance_id ??
    ""
  );
}

export function atsPresenceChipLabel(host: AtsHostSummary): string {
  if (host.degraded_identity) return "Degradiert";
  switch (host.presence_category) {
    case "connected":
      return "Verbunden";
    case "inactive_long":
      return "Inaktiv (>30 Tage)";
    default:
      return host.is_active ? "Kürzlich aktiv" : "Getrennt";
  }
}

export function atsPresenceChipTone(
  host: AtsHostSummary,
): "active" | "inactive" | "degraded" {
  if (host.degraded_identity) return "degraded";
  if (host.presence_category === "connected") return "active";
  if (host.presence_category === "disconnected" && host.is_active) return "degraded";
  return "inactive";
}

/** Row container — avoids primary (brand green) for non-connected hosts. */
export function atsPresenceRowClass(host: AtsHostSummary, selected: boolean): string {
  const base =
    "w-full rounded-lg border px-3 py-3 text-left transition-colors";

  if (host.degraded_identity) {
    return selected
      ? `${base} border-warning/45 bg-warning/10`
      : `${base} border-warning/25 bg-background hover:bg-warning/5`;
  }

  switch (host.presence_category) {
    case "connected":
      return selected
        ? `${base} border-success/45 bg-success/10`
        : `${base} border-success/20 bg-background hover:bg-success/5`;
    case "disconnected":
      if (host.is_active) {
        return selected
          ? `${base} border-warning/45 bg-warning/10`
          : `${base} border-warning/25 bg-background hover:bg-warning/5`;
      }
      return selected
        ? `${base} border-border/70 bg-muted/30`
        : `${base} border-border/60 bg-background hover:bg-muted/20`;
    case "inactive_long":
      return selected
        ? `${base} border-border/60 bg-muted/20`
        : `${base} border-border/50 bg-muted/5 hover:bg-muted/15`;
    default:
      return selected
        ? `${base} border-border/70 bg-muted/30`
        : `${base} border-border/60 bg-background hover:bg-muted/20`;
  }
}
