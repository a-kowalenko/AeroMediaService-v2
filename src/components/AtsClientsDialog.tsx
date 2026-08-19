import {useCallback, useEffect, useMemo, useState} from "react";
import {Spinner} from "@/components/Spinner";
import {StatusChip} from "@/components/StatusChip";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {Button} from "@/components/ui/button";
import {
  getAtsHostDetails,
  getAtsHostsSummary,
  type AtsHostDetails,
  type AtsHostSummary,
} from "@/lib/tauri";

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

type Props = {
  open: boolean;
  onClose: () => void;
};

export function AtsClientsDialog({open, onClose}: Props) {
  const [hosts, setHosts] = useState<AtsHostSummary[]>([]);
  const [hostsLoading, setHostsLoading] = useState(false);
  const [hostsError, setHostsError] = useState("");
  const [selectedHostId, setSelectedHostId] = useState("");
  const [details, setDetails] = useState<AtsHostDetails | null>(null);
  const [detailsLoading, setDetailsLoading] = useState(false);

  const selectedHost = useMemo(
    () => hosts.find((host) => host.instance_id === selectedHostId) ?? null,
    [hosts, selectedHostId],
  );
  const activeHostsCount = useMemo(
    () => hosts.filter((host) => host.is_active).length,
    [hosts],
  );

  const loadHosts = useCallback(async () => {
    setHostsLoading(true);
    setHostsError("");
    try {
      const items = await getAtsHostsSummary(60);
      setHosts(items);
      setSelectedHostId((prev) => {
        if (prev && items.some((host) => host.instance_id === prev)) return prev;
        return items[0]?.instance_id ?? "";
      });
    } catch (err) {
      setHosts([]);
      setSelectedHostId("");
      setDetails(null);
      setHostsError(String(err));
    } finally {
      setHostsLoading(false);
    }
  }, []);

  const loadDetails = useCallback(async (instanceId: string) => {
    const id = instanceId.trim();
    if (!id) {
      setDetails(null);
      return;
    }
    setDetailsLoading(true);
    try {
      const next = await getAtsHostDetails(id, 60, 100);
      setDetails(next);
    } catch (err) {
      setDetails(null);
      setHostsError(String(err));
    } finally {
      setDetailsLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    void loadHosts();
    const id = window.setInterval(() => void loadHosts(), 30000);
    return () => window.clearInterval(id);
  }, [open, loadHosts]);

  useEffect(() => {
    if (!open || !selectedHostId) return;
    void loadDetails(selectedHostId);
  }, [open, selectedHostId, loadDetails]);

  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="flex h-[min(82vh,44rem)] max-w-5xl flex-col gap-4 overflow-hidden">
        <DialogHeader className="shrink-0">
          <DialogTitle>ATS-Clients</DialogTitle>
          <DialogDescription>
            Sichtbar sind nur ATS-Instanzen, die in den letzten 60 Minuten mindestens einen Bridge-Request an AMS gesendet haben.
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-3 sm:grid-cols-3">
          <div className="rounded-lg border border-border/60 bg-muted/15 p-3">
            <p className="text-[11px] font-semibold uppercase tracking-wide text-muted">
              Verbundene Clients
            </p>
            <p className="mt-1 text-2xl font-semibold text-foreground">
              {hostsLoading ? "..." : hosts.length}
            </p>
          </div>
          <div className="rounded-lg border border-border/60 bg-muted/15 p-3">
            <p className="text-[11px] font-semibold uppercase tracking-wide text-muted">
              Aktiv
            </p>
            <p className="mt-1 text-2xl font-semibold text-foreground">
              {hostsLoading ? "..." : activeHostsCount}
            </p>
          </div>
          <div className="flex items-center justify-between rounded-lg border border-border/60 bg-muted/15 p-3">
            <div>
              <p className="text-[11px] font-semibold uppercase tracking-wide text-muted">
                Aktualisierung
              </p>
              <p className="mt-1 text-sm text-muted">30s Auto-Refresh</p>
            </div>
            <Button type="button" variant="secondary" size="sm" disabled={hostsLoading} onClick={() => void loadHosts()}>
              Aktualisieren
            </Button>
          </div>
        </div>

        {hostsError ? <p className="text-xs text-destructive">{hostsError}</p> : null}

        <div className="grid min-h-0 flex-1 gap-3 xl:grid-cols-[minmax(0,0.92fr)_minmax(0,1.28fr)]">
          <div className="min-h-0 space-y-2 overflow-y-auto pr-1 [scrollbar-gutter:stable]">
            {hosts.length === 0 ? (
              <div className="rounded-lg border border-dashed border-border/60 bg-muted/10 px-4 py-6 text-sm text-muted">
                Noch keine ATS-Bridge-Aktivität in den letzten 60 Minuten.
              </div>
            ) : (
              hosts.map((host) => (
                <button
                  key={host.instance_id}
                  type="button"
                  className={`w-full rounded-lg border px-3 py-3 text-left transition-colors ${
                    selectedHostId === host.instance_id
                      ? "border-primary/45 bg-primary/5"
                      : "border-border/60 bg-background hover:bg-muted/20"
                  }`}
                  onClick={() => setSelectedHostId(host.instance_id)}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="truncate font-medium text-foreground">{host.hostname}</span>
                        <PresenceChip
                          label={host.degraded_identity ? "Degradiert" : host.is_active ? "Aktiv" : "Inaktiv"}
                          tone={host.degraded_identity ? "degraded" : host.is_active ? "active" : "inactive"}
                        />
                      </div>
                      <p className="mt-1 truncate text-[11px] text-muted">
                        {host.ats_app || "ATS"} {host.ats_version || ""}
                      </p>
                      <p className="mt-1 truncate text-[11px] text-muted">{host.instance_id}</p>
                    </div>
                    <div className="shrink-0 text-right text-[11px] text-muted">
                      <p>{eventTypeLabel(host.last_event_type)}</p>
                      <p>{formatTimestamp(host.last_seen_at)}</p>
                    </div>
                  </div>
                  <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-muted">
                    <span className="rounded bg-muted/50 px-2 py-1">Events: {host.activity_count_ttl}</span>
                    <span className="rounded bg-muted/50 px-2 py-1">Jobs: {host.jobs_count_ttl}</span>
                  </div>
                </button>
              ))
            )}
          </div>

          <div className="min-h-0 overflow-y-auto rounded-lg border border-border/60 bg-muted/10 p-4 [scrollbar-gutter:stable]">
            {!selectedHost ? (
              <div className="text-sm text-muted">Client auswählen, um letzte Events und Vorgänge zu sehen.</div>
            ) : detailsLoading ? (
              <div className="flex items-center gap-2 text-sm text-muted">
                <Spinner size={14} className="border-[1.5px]" />
                Details werden geladen...
              </div>
            ) : !details ? (
              <div className="text-sm text-muted">Für diesen Client sind derzeit keine Details verfügbar.</div>
            ) : (
              <div className="space-y-4">
                <div className="space-y-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-base font-semibold text-foreground">{details.host.hostname}</span>
                    <PresenceChip
                      label={details.host.degraded_identity ? "Degradiert" : details.host.is_active ? "Aktiv" : "Inaktiv"}
                      tone={details.host.degraded_identity ? "degraded" : details.host.is_active ? "active" : "inactive"}
                    />
                  </div>
                  <div className="grid gap-2 sm:grid-cols-2">
                    <div className="rounded-md border border-border/50 bg-background/80 p-3 text-xs text-muted">
                      <p className="font-medium text-foreground">Client</p>
                      <p className="mt-1">{details.host.ats_app || "-"} {details.host.ats_version || ""}</p>
                      <p className="mt-1 truncate">{details.host.instance_id}</p>
                    </div>
                    <div className="rounded-md border border-border/50 bg-background/80 p-3 text-xs text-muted">
                      <p className="font-medium text-foreground">Sichtbarkeit</p>
                      <p className="mt-1">First seen: {formatTimestamp(details.host.first_seen_at)}</p>
                      <p className="mt-1">Last seen: {formatTimestamp(details.host.last_seen_at)}</p>
                    </div>
                  </div>
                  <div className="rounded-md border border-border/50 bg-background/80 p-3 text-xs text-muted">
                    Letztes Event: {eventTypeLabel(details.host.last_event_type)} um {formatTimestamp(details.host.last_event_at)}
                  </div>
                </div>

                <div className="space-y-2">
                  <p className="text-xs font-semibold uppercase tracking-wide text-muted">Letzte Events</p>
                  {details.host.recent_events.length === 0 ? (
                    <p className="text-sm text-muted">Keine Events im Zeitfenster.</p>
                  ) : (
                    <div className="space-y-2">
                      {details.host.recent_events.slice(0, 8).map((entry) => (
                        <div
                          key={`${entry.occurred_at}-${entry.event_type}-${entry.correlation_id}`}
                          className="rounded-md border border-border/50 bg-background/80 p-3 text-xs"
                        >
                          <div className="flex flex-wrap items-center gap-2">
                            <PresenceChip label={eventTypeLabel(entry.event_type)} tone="neutral" />
                            <span className="text-muted">{formatTimestamp(entry.occurred_at)}</span>
                            <span className="text-muted">{entry.method} {entry.route}</span>
                          </div>
                          {entry.correlation_id || entry.folder_name ? (
                            <div className="mt-2 space-y-0.5 text-muted">
                              {entry.correlation_id ? <p>Correlation ID: {entry.correlation_id}</p> : null}
                              {entry.folder_name ? <p>Ordner: {entry.folder_name}</p> : null}
                            </div>
                          ) : null}
                        </div>
                      ))}
                    </div>
                  )}
                </div>

                <div className="space-y-2">
                  <p className="text-xs font-semibold uppercase tracking-wide text-muted">Letzte Vorgänge</p>
                  {details.recent_jobs.length === 0 ? (
                    <p className="text-sm text-muted">Keine korrelierten Vorgänge im Zeitraum.</p>
                  ) : (
                    <div className="space-y-2">
                      {details.recent_jobs.slice(0, 8).map((job) => (
                        <div
                          key={job.correlation_id}
                          className="rounded-md border border-border/50 bg-background/80 p-3 text-xs"
                        >
                          <div className="flex flex-wrap items-center justify-between gap-2">
                            <span className="truncate font-medium text-foreground">
                              {job.folder_name || "Ohne Ordnername"}
                            </span>
                            <StatusChip status={job.ams_status_label} channel="overall" compact />
                          </div>
                          <div className="mt-2 space-y-0.5 text-muted">
                            <p>Correlation ID: {job.correlation_id}</p>
                            <p>Quelle: {eventTypeLabel(job.source_event_type)}</p>
                            <p>Last seen: {formatTimestamp(job.last_seen_at)}</p>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
