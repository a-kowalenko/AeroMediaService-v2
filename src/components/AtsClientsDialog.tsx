import {useCallback, useEffect, useMemo, useState} from "react";
import {AtsActivityEventCard} from "@/components/AtsActivityEventCard";
import {Spinner} from "@/components/Spinner";
import {StatusChip} from "@/components/StatusChip";
import {
  AtsHostListSections,
  countActiveAtsHosts,
  countConnectedAtsHosts,
} from "@/components/AtsHostListSections";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {Button} from "@/components/ui/button";
import {
  atsPresenceChipLabel,
  atsPresenceChipTone,
  defaultAtsHostSelection,
  findAtsHost,
} from "@/lib/atsPresence";
import {
  getAtsHostDetails,
  getAtsHostsSummary,
  type AtsHostDetails,
} from "@/lib/tauri";
import {eventTypeLabel} from "@/lib/atsActivityDisplay";

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
  open: boolean;
  onClose: () => void;
};

export function AtsClientsDialog({open, onClose}: Props) {
  const [hosts, setHosts] = useState<Awaited<ReturnType<typeof getAtsHostsSummary>>>([]);
  const [hostsLoading, setHostsLoading] = useState(false);
  const [hostsError, setHostsError] = useState("");
  const [selectedHostId, setSelectedHostId] = useState("");
  const [details, setDetails] = useState<AtsHostDetails | null>(null);
  const [detailsLoading, setDetailsLoading] = useState(false);

  const connectedHostsCount = useMemo(() => countConnectedAtsHosts(hosts), [hosts]);
  const activeHostsCount = useMemo(() => countActiveAtsHosts(hosts), [hosts]);

  const selectedHost = useMemo(
    () => findAtsHost(hosts, selectedHostId),
    [hosts, selectedHostId],
  );

  const loadHosts = useCallback(async () => {
    setHostsLoading(true);
    setHostsError("");
    try {
      const items = await getAtsHostsSummary(60);
      setHosts(items);
      setSelectedHostId((prev) => {
        if (prev && items.some((host) => host.instance_id === prev)) return prev;
        return defaultAtsHostSelection(items);
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

  const detailsChipHost = selectedHost;

  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="flex h-[min(82vh,44rem)] max-w-5xl flex-col gap-4 overflow-hidden">
        <DialogHeader className="shrink-0">
          <DialogTitle>ATS-Clients</DialogTitle>
          <DialogDescription>
            Verbundene Clients (~2 Min.), nicht verbunden (letzte 30 Tage), länger inaktiv (&gt;30 Tage). Sortierung nach letztem Kontakt.
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-3 sm:grid-cols-3">
          <div className="rounded-lg border border-border/60 bg-muted/15 p-3">
            <p className="text-[11px] font-semibold uppercase tracking-wide text-muted">
              Verbundene Clients
            </p>
            <p className="mt-1 text-2xl font-semibold text-foreground">
              {hostsLoading ? "..." : connectedHostsCount}
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
          <div className="min-h-0 overflow-y-auto pr-1 [scrollbar-gutter:stable]">
            <AtsHostListSections
              hosts={hosts}
              selectedHostId={selectedHostId}
              onSelectHost={setSelectedHostId}
            />
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
                    {detailsChipHost ? (
                      <PresenceChip
                        label={atsPresenceChipLabel(detailsChipHost)}
                        tone={atsPresenceChipTone(detailsChipHost)}
                      />
                    ) : null}
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
                    <p className="text-sm text-muted">Keine Events im 60-Minuten-Fenster.</p>
                  ) : (
                    <div className="space-y-2">
                      {details.host.recent_events.slice(0, 8).map((entry) => (
                        <AtsActivityEventCard
                          key={`${entry.occurred_at}-${entry.event_type}-${entry.correlation_id}-${entry.payload_json.slice(0, 24)}`}
                          entry={entry}
                        />
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
