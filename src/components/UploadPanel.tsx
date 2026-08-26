import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { CloudUpload, Loader2, Pause, Play, Radio, Timer, X } from "lucide-react";
import { Panel } from "./Panel";
import { ProgressBar } from "./ProgressBar";
import { UploadSlotList } from "./UploadSlotList";
import { Button } from "@/components/ui/button";
import {
  STABILITY_PENDING_CHANGED,
  UPLOAD_ACTIVITY,
  UPLOAD_CONTROL_CHANGED,
  UPLOAD_FAILED,
  UPLOAD_FINISHED,
  UPLOAD_JOB_ACTIVE,
  UPLOAD_PROGRESS_FILE,
  UPLOAD_PROGRESS_SLOTS,
  UPLOAD_PROGRESS_TOTAL,
  UPLOAD_QUEUE_CHANGED,
  UPLOAD_STATUS_UPDATE,
} from "@/lib/events";
import {
  cancelUpload,
  getStabilityPending,
  getUploadControlState,
  getUploadQueue,
  pauseUpload,
  resumeUpload,
  type ByteProgress,
  type QueueSnapshotItem,
  type StabilityPendingItem,
  type UploadActivity,
  type UploadActivityPhase,
  type UploadControlState,
  type UploadSlotsProgress,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { showAppToast } from "@/lib/toast";
import { useUiStore } from "@/store/uiStore";

const EMPTY_PROGRESS: ByteProgress = { percent: 0, current: 0, total: 0 };
const EMPTY_SLOTS: UploadSlotsProgress = {
  files_done: 0,
  files_total: 0,
  slots: [],
};
const IDLE_CONTROL: UploadControlState = {
  paused: false,
  holding: false,
  cancelled: false,
};

/** Pause request vs. worker actually blocked in wait_if_paused. */
type PausePhase = "running" | "pausing" | "paused";

function pausePhaseFrom(control: UploadControlState): PausePhase {
  if (!control.paused) return "running";
  return control.holding ? "paused" : "pausing";
}

function activityPhaseLabel(phase: UploadActivityPhase): string {
  switch (phase) {
    case "idle":
      return "Wartet";
    case "starting":
      return "Startet";
    case "uploading":
      return "Lädt hoch";
    case "finalizing":
      return "Finalisiert";
    case "registering":
      return "Registriert Order";
    case "linking":
      return "Verknüpft Dateien";
    case "paused":
      return "Pausiert";
    case "pausing":
      return "Wird pausiert…";
    case "appending":
      return "Nachreichen";
    case "success":
      return "Erfolgreich";
    case "failed":
      return "Fehler";
    case "cancelled":
      return "Abgebrochen";
    default:
      return "Upload";
  }
}

type PendingView = StabilityPendingItem & { receivedAt: number };

function formatBytes(value: number): string {
  if (value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let n = value;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i += 1;
  }
  return `${n.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

function formatSpeed(bps: number): string {
  if (!Number.isFinite(bps) || bps <= 0) return "";
  return `${formatBytes(bps)}/s`;
}

function stampPending(items: StabilityPendingItem[]): PendingView[] {
  const receivedAt = Date.now();
  return items.map((item) => ({ ...item, receivedAt }));
}

function remainingSeconds(item: PendingView, now: number): number {
  if (item.waiting_for_media) return 0;
  const elapsed = (now - item.receivedAt) / 1000;
  return Math.max(0, item.remaining_seconds - elapsed);
}

function isHandoffItem(item: StabilityPendingItem): boolean {
  return item.kind === "handoff";
}

function stabilityLabel(item: PendingView, now: number): string {
  if (item.waiting_for_media) return "Wartet auf Medien-Dateien";
  const left = remainingSeconds(item, now);
  if (left <= 0) return "Inhalt stabil — Upload wird vorbereitet";
  return `Warte auf Datei-Stabilität · noch ${Math.ceil(left)} s`;
}

function pendingItemLabel(item: PendingView, now: number): string {
  if (isHandoffItem(item)) {
    const phase = item.handoff_phase?.trim() ?? "";
    switch (phase) {
      case "waiting_folder":
        return "Warte auf Ordner auf dem Share…";
      case "waiting_fertig":
        return "Ordner da — warte auf _fertig.txt…";
      case "waiting_media":
        return "Warte auf Medien-Dateien…";
      case "rejected": {
        const msg = item.handoff_error_message?.trim();
        if (msg) return msg;
        const code = item.handoff_error_code?.trim();
        return code ? `Handoff abgelehnt (${code})` : "Handoff abgelehnt";
      }
      default:
        return "Vom Studio gemeldet — Upload wird vorbereitet";
    }
  }
  return stabilityLabel(item, now);
}

function handoffDetailLabel(item: PendingView): string {
  const phase = item.handoff_phase?.trim() ?? "";
  if (phase === "rejected") return "abgelehnt";
  if (phase === "waiting_folder") return "Share";
  if (phase === "waiting_fertig") return "Marker";
  if (phase === "waiting_media") return "Dateien";
  return "neu";
}

function stabilityProgress(item: PendingView, now: number): number {
  if (item.waiting_for_media) return 0;
  const required = item.required_seconds;
  if (required <= 0) return 100;
  const left = remainingSeconds(item, now);
  return Math.max(0, Math.min(100, ((required - left) / required) * 100));
}

type Props = {
  className?: string;
  compact?: boolean;
};

export function UploadPanel({ className, compact = false }: Props) {
  const [active, setActive] = useState(false);
  const [status, setStatus] = useState("Warte auf nächsten Auftrag…");
  const [activity, setActivity] = useState<UploadActivity | null>(null);
  const [file, setFile] = useState<ByteProgress>(EMPTY_PROGRESS);
  const [total, setTotal] = useState<ByteProgress>(EMPTY_PROGRESS);
  const [slots, setSlots] = useState<UploadSlotsProgress>(EMPTY_SLOTS);
  const [queue, setQueue] = useState<QueueSnapshotItem[]>([]);
  const [pending, setPending] = useState<PendingView[]>([]);
  const [now, setNow] = useState(() => Date.now());
  const [busy, setBusy] = useState(false);
  const [control, setControl] = useState<UploadControlState>(IDLE_CONTROL);
  const [speedBps, setSpeedBps] = useState(0);
  const speedSampleRef = useRef<{ t: number; bytes: number } | null>(null);
  const confirm = useUiStore((s) => s.confirm);
  const pausePhase = pausePhaseFrom(control);
  const cancelling = control.cancelled;
  const isPausedLike = pausePhase !== "running";

  useEffect(() => {
    getUploadQueue().then(setQueue).catch(() => {});
    getStabilityPending().then((items) => setPending(stampPending(items))).catch(() => {});
    getUploadControlState().then(setControl).catch(() => {});
  }, []);

  useEffect(() => {
    if (pending.length === 0) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [pending.length]);

  useEffect(() => {
    if (!active || isPausedLike) {
      if (!active) {
        speedSampleRef.current = null;
        setSpeedBps(0);
      }
      return;
    }
    const now = performance.now();
    const bytes = total.current;
    const prev = speedSampleRef.current;
    if (prev == null) {
      speedSampleRef.current = { t: now, bytes };
      return;
    }
    const dt = (now - prev.t) / 1000;
    if (dt < 0.35) return;
    const delta = bytes - prev.bytes;
    speedSampleRef.current = { t: now, bytes };
    if (delta < 0) {
      setSpeedBps(0);
      return;
    }
    const instant = delta / dt;
    setSpeedBps((s) => (s <= 0 ? instant : s * 0.65 + instant * 0.35));
  }, [total.current, active, isPausedLike]);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    const add = <T,>(name: string, handler: (payload: T) => void) => {
      listen<T>(name, (event) => handler(event.payload))
        .then((fn) => {
          if (cancelled) {
            fn();
            return;
          }
          unlisteners.push(fn);
        })
        .catch(() => {});
    };
    add<boolean>(UPLOAD_JOB_ACTIVE, (next) => {
      setActive(next);
      if (!next) {
        setControl(IDLE_CONTROL);
        setActivity(null);
        setSlots(EMPTY_SLOTS);
        setFile(EMPTY_PROGRESS);
        setTotal(EMPTY_PROGRESS);
        setSpeedBps(0);
        speedSampleRef.current = null;
      }
    });
    add<string>(UPLOAD_STATUS_UPDATE, setStatus);
    add<UploadActivity>(UPLOAD_ACTIVITY, setActivity);
    add<UploadControlState>(UPLOAD_CONTROL_CHANGED, setControl);
    add<ByteProgress>(UPLOAD_PROGRESS_FILE, setFile);
    add<ByteProgress>(UPLOAD_PROGRESS_TOTAL, setTotal);
    add<UploadSlotsProgress>(UPLOAD_PROGRESS_SLOTS, setSlots);
    add<QueueSnapshotItem[]>(UPLOAD_QUEUE_CHANGED, setQueue);
    add<StabilityPendingItem[]>(STABILITY_PENDING_CHANGED, (items) => {
      setPending(stampPending(items));
      setNow(Date.now());
    });
    add<string>(UPLOAD_FINISHED, (msg) => {
      setStatus(`Erfolgreich: ${msg}`);
      showAppToast(msg, {
        tone: "success",
        title: "Upload fertig",
        id: `upload-finished:${msg}`,
      });
    });
    add<string>(UPLOAD_FAILED, (msg) => {
      setStatus(`Fehler: ${msg}`);
      showAppToast(msg, {
        tone: "error",
        title: "Upload fehlgeschlagen",
        id: `upload-failed:${msg}`,
      });
    });
    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    try {
      await action();
    } finally {
      setBusy(false);
    }
  }

  async function onTogglePause() {
    if (pausePhase === "running") {
      setControl((prev) => ({ ...prev, paused: true, holding: false }));
      await run(pauseUpload);
      return;
    }
    setControl((prev) => ({
      ...prev,
      paused: false,
      holding: false,
    }));
    await run(resumeUpload);
  }

  async function onCancel() {
    const ok = await confirm("Laufenden Upload wirklich abbrechen?", {
      title: "Upload abbrechen",
      primaryLabel: "Ja, abbrechen",
      secondaryLabel: "Weiterlaufen lassen",
      destructive: true,
    });
    if (!ok) return;
    setControl((prev) => ({
      ...prev,
      cancelled: true,
      paused: false,
      holding: false,
    }));
    await run(cancelUpload);
  }

  const hasPending = pending.length > 0;
  const handoffPending = pending.filter((item) => isHandoffItem(item));
  const stabilityPending = pending.filter((item) => !isHandoffItem(item));
  const onlyHandoff = hasPending && stabilityPending.length === 0;
  const activeJob = queue.find((item) => item.state === "active");
  const waitingQueue = queue.filter((item) => item.state !== "active");
  const queueLabel =
    waitingQueue.length === 0
      ? "Keine Aufträge"
      : waitingQueue.length === 1
        ? "1 wartet"
        : `${waitingQueue.length} warten`;
  const chipLabel = active
    ? cancelling
      ? "Abbruch…"
      : pausePhase === "paused"
        ? "Pausiert"
        : pausePhase === "pausing"
          ? "Pause…"
          : "Aktiv"
    : onlyHandoff
      ? "Neu"
      : hasPending
        ? "Wartet"
        : "Idle";
  const activeDirName =
    activeJob?.dir_name?.trim() || activity?.dir_name?.trim() || "";
  const activeCustomerLabel = activeJob?.customer_label?.trim() || "";
  const hasActiveCustomerLabel =
    activeCustomerLabel !== "" && activeCustomerLabel !== "—";
  const description = active
    ? hasActiveCustomerLabel
      ? activeCustomerLabel
      : activeDirName || "Upload läuft"
    : onlyHandoff
      ? handoffPending.length === 1
        ? "Neuer Auftrag…"
        : `${handoffPending.length} neue Aufträge`
      : hasPending
        ? pending.length === 1
          ? "Stabilität prüfen…"
          : `${pending.length} Ordner warten`
        : waitingQueue.length > 0
          ? queueLabel
          : undefined;
  const subDescription =
    active && hasActiveCustomerLabel && activeDirName ? activeDirName : undefined;

  const activityLine =
    activity != null
      ? activity.phase === "failed" && activity.message?.trim()
        ? activity.message.trim()
        : activity.message?.trim() || activityPhaseLabel(activity.phase)
      : null;
  const relPath = activity?.rel_path?.trim() || "";
  const showPath =
    Boolean(relPath) &&
    activity != null &&
    activity.phase !== "paused" &&
    activity.phase !== "pausing" &&
    activity.phase !== "idle";
  const filesTotal =
    slots.files_total > 0
      ? slots.files_total
      : activity?.file_count != null
        ? activity.file_count
        : 0;
  const filesDone = slots.files_total > 0 ? slots.files_done : 0;
  const filesCounterLabel =
    filesTotal > 0 ? `${filesDone}/${filesTotal}` : null;
  const activeSlots = slots.slots;
  const bytesLabel = (current: number, totalBytes: number) =>
    `${formatBytes(current)} / ${formatBytes(totalBytes)}`;
  const speedLabel = !isPausedLike ? formatSpeed(speedBps) : "";
  const totalSizeParts: string[] = [];
  if (total.total > 0 || total.current > 0) {
    totalSizeParts.push(bytesLabel(total.current, total.total));
  }
  if (speedLabel) totalSizeParts.push(speedLabel);
  const totalPercentDetail =
    total.percent === 0 && total.current === 0
      ? undefined
      : `${Math.round(total.percent)}%`;
  // Only fall back to the single-file bar when the slot tracker never started
  // (older emits / non-batch paths without slots).
  const showLegacyFileBar =
    slots.files_total === 0 &&
    activeSlots.length === 0 &&
    (file.total > 0 || file.current > 0 || file.percent > 0);
  const legacyFileBadge =
    activity?.file_index != null && activity?.file_count != null
      ? `${activity.file_index}/${activity.file_count}`
      : undefined;
  const showSlotList = activeSlots.length > 0;

  const statusChip = (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-[11px] font-medium",
        active && isPausedLike
          ? "border-warning/35 bg-warning/10 text-warning"
          : active
            ? "border-primary/35 bg-primary/10 text-primary"
            : onlyHandoff
              ? "border-primary/35 bg-primary/10 text-primary"
              : hasPending
                ? "border-warning/35 bg-warning/10 text-warning"
                : "border-border bg-card-elevated/80 text-muted",
      )}
    >
      <span
        className={cn(
          "h-1.5 w-1.5 rounded-full",
          active && isPausedLike
            ? "bg-warning ams-chip-active"
            : active
              ? "bg-primary ams-chip-active"
              : onlyHandoff
                ? "bg-primary ams-chip-active"
                : hasPending
                  ? "bg-warning ams-chip-active"
                  : "bg-muted",
        )}
      />
      {chipLabel}
    </span>
  );

  return (
    <Panel
      className={className}
      compact={compact}
      title="Upload"
      titleAdornment={statusChip}
      description={description}
      subDescription={subDescription}
    >
      {!active && !hasPending && queue.length === 0 ? (
        <div className="mb-1 flex items-start gap-2 text-sm text-muted">
          <CloudUpload className="mt-0.5 h-4 w-4 shrink-0 text-primary/70" />
          <span className="min-w-0 leading-snug">
            Bereit — wartet auf den nächsten Auftrag.
          </span>
        </div>
      ) : null}

      {hasPending ? (
        <div className={cn(active ? "mb-4" : "mb-1")}>
          <p className="mb-2 text-[11px] font-semibold tracking-[0.08em] text-muted uppercase">
            {onlyHandoff
              ? "Neue Aufträge"
              : handoffPending.length > 0
                ? "Eingehend"
                : "Stabilität"}
          </p>
          <ul className="space-y-1.5">
            {pending.map((item) => {
              const handoff = isHandoffItem(item);
              const handoffRejected =
                handoff && item.handoff_phase?.trim() === "rejected";
              const left = remainingSeconds(item, now);
              const detail = handoff
                ? handoffDetailLabel(item)
                : item.waiting_for_media
                  ? "Dateien"
                  : left <= 0
                    ? "bereit"
                    : `${Math.ceil(left)}s`;
              return (
                <li
                  key={`${item.kind ?? "stability"}-${item.dir_name}`}
                  className={cn(
                    "rounded-lg border px-2.5 py-2 text-sm",
                    handoffRejected
                      ? "border-destructive/30 bg-destructive/5"
                      : handoff
                        ? "border-primary/25 bg-primary/5"
                        : "border-warning/25 bg-warning/5",
                  )}
                >
                  <div className="flex items-center justify-between gap-2">
                    <strong className="truncate text-foreground">
                      {item.dir_name}
                    </strong>
                    <span
                      className={cn(
                        "inline-flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 text-[10px] font-medium",
                        handoffRejected
                          ? "bg-destructive/15 text-destructive"
                          : handoff
                            ? "bg-primary/15 text-primary"
                            : "bg-warning/15 text-warning",
                      )}
                    >
                      {handoff ? (
                        <Radio className="h-3 w-3" />
                      ) : (
                        <Timer className="h-3 w-3" />
                      )}
                      {detail}
                    </span>
                  </div>
                  <p className="mt-0.5 truncate text-xs text-muted">
                    {pendingItemLabel(item, now)}
                  </p>
                  {!handoff && !item.waiting_for_media ? (
                    <div className="mt-2">
                      <ProgressBar
                        percent={stabilityProgress(item, now)}
                        label="Datei-Stabilität"
                        sizeDetail={
                          left <= 0
                            ? "bereit"
                            : `${Math.ceil(left)} / ${Math.round(item.required_seconds)} s`
                        }
                        detail={`${Math.round(stabilityProgress(item, now))}%`}
                      />
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        </div>
      ) : null}

      {active ? (
        <>
          <div className="mb-3 flex items-start gap-2 text-sm text-muted">
            <CloudUpload className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
            <div className="min-w-0 flex-1 space-y-0.5">
              {activityLine != null ? (
                <>
                  <p
                    className={cn(
                      "min-w-0 leading-snug text-foreground",
                      activity?.phase === "failed"
                        ? "break-words"
                        : "truncate",
                    )}
                    title={activityLine}
                  >
                    {activityLine}
                  </p>
                  {showPath ? (
                    <p
                      className="min-w-0 truncate text-xs text-muted [overflow-wrap:anywhere]"
                      title={relPath}
                    >
                      {relPath}
                    </p>
                  ) : null}
                </>
              ) : (
                <span className="min-w-0 break-words leading-snug">{status}</span>
              )}
            </div>
          </div>

          <div className={cn("space-y-2.5", isPausedLike && "opacity-70")}>
            <ProgressBar
              percent={total.percent}
              label="Gesamt"
              badge={filesCounterLabel ?? undefined}
              sizeDetail={
                totalSizeParts.length > 0
                  ? totalSizeParts.join(" · ")
                  : undefined
              }
              detail={totalPercentDetail}
              indeterminate={
                active &&
                total.percent === 0 &&
                total.current === 0 &&
                (activity?.phase === "uploading" || activity?.phase === "appending")
              }
            />
            {showSlotList ? (
              <UploadSlotList
                slots={activeSlots}
                formatSize={bytesLabel}
              />
            ) : showLegacyFileBar ? (
              <ProgressBar
                percent={file.percent}
                label="Datei"
                badge={legacyFileBadge}
                sizeDetail={
                  file.total > 0 || file.current > 0
                    ? bytesLabel(file.current, file.total)
                    : undefined
                }
                detail={
                  file.percent === 0 && file.current === 0
                    ? undefined
                    : `${Math.round(file.percent)}%`
                }
                indeterminate={file.percent === 0 && file.current === 0}
              />
            ) : null}
          </div>

          <div className="mt-3.5 flex flex-wrap gap-1.5">
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={busy || pausePhase === "pausing" || cancelling}
              onClick={() => void onTogglePause()}
            >
              {pausePhase === "running" ? (
                <>
                  <Pause className="h-3.5 w-3.5" />
                  Pause
                </>
              ) : pausePhase === "pausing" ? (
                <>
                  <Pause className="h-3.5 w-3.5" />
                  Wird pausiert…
                </>
              ) : (
                <>
                  <Play className="h-3.5 w-3.5" />
                  Fortsetzen
                </>
              )}
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive"
              disabled={busy || cancelling}
              onClick={() => void onCancel()}
            >
              {cancelling ? (
                <>
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  Wird abgebrochen…
                </>
              ) : (
                <>
                  <X className="h-3.5 w-3.5" />
                  Abbrechen
                </>
              )}
            </Button>
          </div>
        </>
      ) : null}

      {waitingQueue.length > 0 ? (
        <div className={cn("border-t border-border/70 pt-3", active || hasPending ? "mt-4" : "mt-3")}>
          <p className="mb-2 text-[11px] font-semibold tracking-[0.08em] text-muted uppercase">
            Warteschlange
          </p>
          <ul className="space-y-1.5">
            {waitingQueue.map((item) => (
              <li
                key={`${item.position}-${item.dir_name}`}
                className="rounded-lg border border-border/70 bg-card-elevated/60 px-2.5 py-2 text-sm"
              >
                <div className="flex items-center justify-between gap-2">
                  <strong className="truncate text-foreground">
                    {item.dir_name}
                  </strong>
                  <span className="shrink-0 rounded-md bg-background/70 px-1.5 py-0.5 text-[10px] font-medium text-muted">
                    {`wartet${
                      item.wait_seconds
                        ? ` · ${Math.round(item.wait_seconds)}s`
                        : ""
                    }`}
                  </span>
                </div>
                <p className="mt-0.5 truncate text-xs text-muted">
                  {item.customer_label}
                </p>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </Panel>
  );
}
