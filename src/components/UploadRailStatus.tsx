import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { CloudUpload, Pause } from "lucide-react";
import {
  STABILITY_PENDING_CHANGED,
  UPLOAD_ACTIVITY,
  UPLOAD_CONTROL_CHANGED,
  UPLOAD_JOB_ACTIVE,
  UPLOAD_PROGRESS_SLOTS,
  UPLOAD_PROGRESS_TOTAL,
} from "@/lib/events";
import {
  getStabilityPending,
  getUploadControlState,
  type ByteProgress,
  type StabilityPendingItem,
  type UploadActivity,
  type UploadControlState,
  type UploadSlotsProgress,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/store/appStore";

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

const RING_SIZE = 36;
const RING_STROKE = 2.5;
const RING_RADIUS = (RING_SIZE - RING_STROKE) / 2;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

type Props = {
  onOpen: () => void;
  className?: string;
};

/** Compact circular progress for the collapsed upload rail. */
export function UploadRailStatus({ onOpen, className }: Props) {
  const uploadJobActive = useAppStore((s) => s.uploadJobActive);
  const [total, setTotal] = useState<ByteProgress>(EMPTY_PROGRESS);
  const [slots, setSlots] = useState<UploadSlotsProgress>(EMPTY_SLOTS);
  const [activity, setActivity] = useState<UploadActivity | null>(null);
  const [control, setControl] = useState<UploadControlState>(IDLE_CONTROL);
  const [pendingCount, setPendingCount] = useState(0);

  useEffect(() => {
    getUploadControlState().then(setControl).catch(() => {});
    getStabilityPending()
      .then((items: StabilityPendingItem[]) => setPendingCount(items.length))
      .catch(() => {});
  }, []);

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

    add<boolean>(UPLOAD_JOB_ACTIVE, (active) => {
      if (!active) {
        setTotal(EMPTY_PROGRESS);
        setSlots(EMPTY_SLOTS);
        setActivity(null);
        setControl(IDLE_CONTROL);
      }
    });
    add<ByteProgress>(UPLOAD_PROGRESS_TOTAL, setTotal);
    add<UploadSlotsProgress>(UPLOAD_PROGRESS_SLOTS, setSlots);
    add<UploadActivity>(UPLOAD_ACTIVITY, setActivity);
    add<UploadControlState>(UPLOAD_CONTROL_CHANGED, setControl);
    add<StabilityPendingItem[]>(STABILITY_PENDING_CHANGED, (items) => {
      setPendingCount(items.length);
    });

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  const paused =
    control.paused ||
    activity?.phase === "paused" ||
    activity?.phase === "pausing";
  const percent = Math.max(0, Math.min(100, total.percent));
  const hasMeasurableProgress =
    uploadJobActive && (percent > 0 || total.current > 0 || total.total > 0);
  const indeterminate =
    uploadJobActive &&
    !paused &&
    !hasMeasurableProgress &&
    (activity?.phase === "uploading" ||
      activity?.phase === "appending" ||
      activity?.phase === "starting" ||
      activity == null);
  const waiting = !uploadJobActive && pendingCount > 0;
  const activeVisual = uploadJobActive || waiting;

  const filesLabel =
    slots.files_total > 0
      ? `${slots.files_done}/${slots.files_total} Dateien`
      : null;
  const phaseLabel = paused
    ? control.holding || activity?.phase === "paused"
      ? "Pausiert"
      : "Wird pausiert…"
    : activity?.phase === "appending"
      ? "Nachreichen"
      : activity?.message?.trim() ||
        (uploadJobActive ? "Upload aktiv" : waiting ? "Wartet" : "Upload-Status");

  const titleParts = [
    phaseLabel,
    hasMeasurableProgress ? `${Math.round(percent)}%` : null,
    filesLabel,
    "Panel öffnen",
  ].filter(Boolean);
  const title = titleParts.join(" · ");

  const ringOffset = hasMeasurableProgress
    ? RING_CIRCUMFERENCE * (1 - percent / 100)
    : RING_CIRCUMFERENCE;
  const tone = paused
    ? "warning"
    : uploadJobActive
      ? "primary"
      : waiting
        ? "warning"
        : "muted";

  return (
    <button
      type="button"
      className={cn(
        "relative mt-2 flex h-9 w-9 items-center justify-center rounded-md transition-colors",
        tone === "primary" &&
          "bg-primary/10 text-primary hover:bg-primary/15",
        tone === "warning" &&
          "bg-warning/10 text-warning hover:bg-warning/15",
        tone === "muted" &&
          "border border-border bg-card/70 text-muted hover:bg-card hover:text-foreground",
        className,
      )}
      onClick={onOpen}
      title={title}
      aria-label={title}
    >
      <svg
        className="pointer-events-none absolute inset-0"
        width={RING_SIZE}
        height={RING_SIZE}
        viewBox={`0 0 ${RING_SIZE} ${RING_SIZE}`}
        aria-hidden
      >
        <circle
          cx={RING_SIZE / 2}
          cy={RING_SIZE / 2}
          r={RING_RADIUS}
          fill="none"
          className={cn(
            tone === "muted" ? "stroke-border/70" : "stroke-current opacity-25",
          )}
          strokeWidth={RING_STROKE}
        />
        {hasMeasurableProgress || indeterminate || paused ? (
          <g
            className={indeterminate ? "ams-rail-indeterminate" : undefined}
            style={{ transformOrigin: `${RING_SIZE / 2}px ${RING_SIZE / 2}px` }}
          >
            <circle
              cx={RING_SIZE / 2}
              cy={RING_SIZE / 2}
              r={RING_RADIUS}
              fill="none"
              className="stroke-current"
              strokeWidth={RING_STROKE}
              strokeLinecap="round"
              strokeDasharray={
                indeterminate
                  ? `${RING_CIRCUMFERENCE * 0.28} ${RING_CIRCUMFERENCE * 0.72}`
                  : RING_CIRCUMFERENCE
              }
              strokeDashoffset={indeterminate ? 0 : ringOffset}
              transform={`rotate(-90 ${RING_SIZE / 2} ${RING_SIZE / 2})`}
              style={
                hasMeasurableProgress
                  ? { transition: "stroke-dashoffset 200ms ease-out" }
                  : undefined
              }
            />
          </g>
        ) : null}
      </svg>

      {paused ? (
        <Pause className="relative h-3.5 w-3.5" aria-hidden />
      ) : hasMeasurableProgress ? (
        <span className="relative text-[9px] font-semibold tabular-nums leading-none">
          {Math.round(percent)}
        </span>
      ) : (
        <CloudUpload
          className={cn(
            "relative h-4 w-4",
            activeVisual && "ams-chip-active",
          )}
          aria-hidden
        />
      )}
    </button>
  );
}
