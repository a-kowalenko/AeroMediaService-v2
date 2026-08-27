import {ChevronDown} from "lucide-react";
import {useCallback, useEffect, useMemo, useRef, useState} from "react";
import {AtsActivityEventCard} from "@/components/AtsActivityEventCard";
import {Spinner} from "@/components/Spinner";
import {Button} from "@/components/ui/button";
import {
  ATS_ACTIVITY_PAGE_SIZE,
  groupConsecutiveHealthRuns,
  healthRunChipLabel,
  healthRunTimeRange,
} from "@/lib/atsActivityDisplay";
import {getAtsHostActivity, type AtsActivityEntry} from "@/lib/tauri";

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

function HealthRunCard({entries}: {entries: AtsActivityEntry[]}) {
  const [open, setOpen] = useState(false);
  if (entries.length === 1) {
    return <AtsActivityEventCard entry={entries[0]} />;
  }
  return (
    <div className="rounded-md border border-border/50 bg-background/80 text-xs">
      <button
        type="button"
        className="flex w-full items-center justify-between gap-2 px-3 py-3 text-left"
        onClick={() => setOpen((prev) => !prev)}
        aria-expanded={open}
      >
        <span className="flex min-w-0 flex-wrap items-center gap-2">
          <PresenceChip label={healthRunChipLabel(entries)} tone="neutral" />
          <span className="text-muted">{healthRunTimeRange(entries)}</span>
        </span>
        <ChevronDown
          className={`h-4 w-4 shrink-0 text-muted transition-transform ${open ? "" : "-rotate-90"}`}
        />
      </button>
      {open ? (
        <div className="space-y-2 border-t border-border/40 px-3 py-3">
          {entries.map((entry, index) => (
            <AtsActivityEventCard
              key={`${entry.occurred_at}-${entry.route}-${index}`}
              entry={entry}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

type Props = {
  instanceId: string;
  /** Bump to soft-reload without blanking the list. */
  refreshToken?: number;
};

export function AtsHostActivitySection({instanceId, refreshToken = 0}: Props) {
  const [items, setItems] = useState<AtsActivityEntry[]>([]);
  const [total, setTotal] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");
  const lastSoftTokenRef = useRef(0);

  const loadInitial = useCallback(async (id: string, soft: boolean) => {
    const trimmed = id.trim();
    if (!trimmed) {
      setItems([]);
      setTotal(0);
      setHasMore(false);
      return;
    }
    if (!soft) setLoading(true);
    setError("");
    try {
      const page = await getAtsHostActivity(trimmed, 0, ATS_ACTIVITY_PAGE_SIZE);
      setItems(page.items);
      setTotal(page.total);
      setHasMore(page.has_more);
    } catch (err) {
      if (!soft) {
        setItems([]);
        setTotal(0);
        setHasMore(false);
      }
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadInitial(instanceId, false);
  }, [instanceId, loadInitial]);

  useEffect(() => {
    if (refreshToken <= 0 || refreshToken === lastSoftTokenRef.current) return;
    lastSoftTokenRef.current = refreshToken;
    void loadInitial(instanceId, true);
  }, [refreshToken, instanceId, loadInitial]);

  const loadMore = useCallback(async () => {
    const trimmed = instanceId.trim();
    if (!trimmed || loadingMore || !hasMore) return;
    setLoadingMore(true);
    setError("");
    try {
      const page = await getAtsHostActivity(trimmed, items.length, ATS_ACTIVITY_PAGE_SIZE);
      setItems((prev) => [...prev, ...page.items]);
      setTotal(page.total);
      setHasMore(page.has_more);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingMore(false);
    }
  }, [instanceId, items.length, hasMore, loadingMore]);

  const groups = useMemo(() => groupConsecutiveHealthRuns(items), [items]);

  if (loading) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted">
        <Spinner size={14} className="border-[1.5px]" />
        Events werden geladen…
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-xs font-semibold uppercase tracking-wide text-muted">Letzte Events</p>
        {total > 0 ? (
          <p className="text-[11px] text-muted">
            {groups.length} Einträge · {items.length}/{total} Events · 7 Tage
          </p>
        ) : null}
      </div>
      {error ? <p className="text-xs text-destructive">{error}</p> : null}
      {items.length === 0 ? (
        <p className="text-sm text-muted">Keine Events im Aufbewahrungsfenster (7 Tage).</p>
      ) : (
        <div className="space-y-2">
          {groups.map((group, index) =>
            group.kind === "single" ? (
              <AtsActivityEventCard
                key={`${group.entry.occurred_at}-${group.entry.event_type}-${group.entry.correlation_id}-${index}`}
                entry={group.entry}
              />
            ) : (
              <HealthRunCard
                key={`health-run-${group.entries[0]?.occurred_at}-${index}`}
                entries={group.entries}
              />
            ),
          )}
        </div>
      )}
      {hasMore ? (
        <Button
          type="button"
          variant="secondary"
          size="sm"
          className="w-full"
          disabled={loadingMore}
          onClick={() => void loadMore()}
        >
          {loadingMore ? "Laden…" : `Weitere Events laden (+${ATS_ACTIVITY_PAGE_SIZE})`}
        </Button>
      ) : null}
    </div>
  );
}
