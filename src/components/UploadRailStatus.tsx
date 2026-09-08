import { useEffect, useState, type CSSProperties } from "react";
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

/** Inner ring geometry (button is 36×36; ring sits inset). */
const RING_SIZE = 30;
const RING_STROKE = 2;
const RING_RADIUS = (RING_SIZE - RING_STROKE) / 2;
const RING_CX = RING_SIZE / 2;
const RING_CY = RING_SIZE / 2;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;
/** Upload-active indeterminate (unchanged visual). */
const INDETERMINATE_DASH = RING_CIRCUMFERENCE * 0.22;
const INDETERMINATE_GAP = RING_CIRCUMFERENCE - INDETERMINATE_DASH;
/** Stability wait: shorter arc so it reads as busy, not ~25% progress. */
const WAITING_DASH = RING_CIRCUMFERENCE * 0.14;
const WAITING_GAP = RING_CIRCUMFERENCE - WAITING_DASH;

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
  const waiting = !uploadJobActive && pendingCount > 0;
  /** Upload without bytes — keep existing indeterminate dash animation. */
  const uploadSpinning = !paused && uploadJobActive && !hasMeasurableProgress;
  /** Stability pending — dedicated short rotating arc (not progress). */
  const waitingSpin = !paused && waiting;
  const showArc =
    hasMeasurableProgress || uploadSpinning || waitingSpin || paused;

  const filesLabel =
    slots.files_total > 0
      ? `${slots.files_done}/${slots.files_total}`
      : null;
  const statusLabel = paused
    ? control.holding || activity?.phase === "paused"
      ? "Pausiert"
      : "Wird pausiert…"
    : activity?.phase === "appending"
      ? "Nachreichen"
      : hasMeasurableProgress
        ? `${Math.round(percent)}%`
        : uploadJobActive
          ? "Upload aktiv"
          : waiting
            ? "Wartet auf Stabilität"
            : null;

  const title = [statusLabel, filesLabel, "Panel öffnen"]
    .filter(Boolean)
    .join(" · ");

  const ringOffset = hasMeasurableProgress
    ? RING_CIRCUMFERENCE * (1 - percent / 100)
    : paused
      ? RING_CIRCUMFERENCE * 0.75
      : RING_CIRCUMFERENCE;
  const tone = paused || waiting ? "warning" : uploadJobActive ? "primary" : "muted";

  return (
    <button
      type="button"
      className={cn(
        "relative mt-2 flex h-9 w-9 shrink-0 items-center justify-center rounded-full transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background",
        tone === "primary" &&
          "bg-primary/12 text-primary hover:bg-primary/18",
        tone === "warning" &&
          "bg-warning/12 text-warning hover:bg-warning/18",
        tone === "muted" &&
          "text-muted hover:bg-card/80 hover:text-foreground",
        className,
      )}
      onClick={onOpen}
      title={title}
      aria-label={title}
    >
      <svg
        className="pointer-events-none absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2"
        width={RING_SIZE}
        height={RING_SIZE}
        viewBox={`0 0 ${RING_SIZE} ${RING_SIZE}`}
        aria-hidden
      >
        <circle
          cx={RING_CX}
          cy={RING_CY}
          r={RING_RADIUS}
          fill="none"
          className={cn(
            tone === "muted" ? "stroke-border" : "stroke-current opacity-20",
          )}
          strokeWidth={RING_STROKE}
        />
        {showArc ? (
          waitingSpin ? (
            <g
              className="ams-rail-waiting-spin"
              style={
                {
                  transformOrigin: `${RING_CX}px ${RING_CY}px`,
                  transformBox: "view-box",
                } as CSSProperties
              }
            >
              <circle
                cx={RING_CX}
                cy={RING_CY}
                r={RING_RADIUS}
                fill="none"
                className="stroke-current"
                strokeWidth={RING_STROKE}
                strokeLinecap="round"
                strokeDasharray={`${WAITING_DASH} ${WAITING_GAP}`}
                transform={`rotate(-90 ${RING_CX} ${RING_CY})`}
              />
            </g>
          ) : (
            <circle
              cx={RING_CX}
              cy={RING_CY}
              r={RING_RADIUS}
              fill="none"
              className={cn(
                "stroke-current",
                uploadSpinning && "ams-rail-indeterminate-circle",
              )}
              strokeWidth={RING_STROKE}
              strokeLinecap="round"
              strokeDasharray={
                uploadSpinning
                  ? `${INDETERMINATE_DASH} ${INDETERMINATE_GAP}`
                  : RING_CIRCUMFERENCE
              }
              strokeDashoffset={uploadSpinning ? 0 : ringOffset}
              transform={`rotate(-90 ${RING_CX} ${RING_CY})`}
              style={
                uploadSpinning
                  ? ({
                      ["--ams-rail-circ" as string]: String(RING_CIRCUMFERENCE),
                    } as CSSProperties)
                  : hasMeasurableProgress
                    ? { transition: "stroke-dashoffset 200ms ease-out" }
                    : undefined
              }
            />
          )
        ) : null}
      </svg>

      <span className="relative flex h-4 w-4 items-center justify-center">
        {paused ? (
          <Pause className="h-3.5 w-3.5" strokeWidth={2.25} aria-hidden />
        ) : hasMeasurableProgress ? (
          <span className="text-[9px] font-semibold tabular-nums leading-none tracking-tight">
            {Math.round(percent)}
          </span>
        ) : (
          <CloudUpload className="h-3.5 w-3.5" strokeWidth={2.25} aria-hidden />
        )}
      </span>
    </button>
  );
}
