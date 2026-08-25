import { useEffect, useRef, useState } from "react";
import { ProgressBar } from "./ProgressBar";
import { cn } from "@/lib/utils";
import type { UploadActiveSlot } from "@/lib/tauri";

/** Matches backend `BATCH_PARALLEL_WORKERS` — keep list height stable. */
export const PARALLEL_SLOT_CAP = 4;

const SLOT_ROW_PX = 52;
const SLOT_GAP_PX = 6;
/** Snappy but smooth — quick start, soft landing. */
const COLLAPSE_MS = 210;
const EASE = "cubic-bezier(0.22, 1, 0.36, 1)";

type Phase = "enter" | "idle" | "exit";

type DisplayRow = {
  key: string;
  slot: UploadActiveSlot;
  phase: Phase;
};

function slotKey(slot: UploadActiveSlot): string {
  return `${slot.file_index}\0${slot.name}`;
}

function listMinHeightPx(rows: number): number | undefined {
  if (rows <= 0) return undefined;
  return rows * SLOT_ROW_PX + Math.max(0, rows - 1) * SLOT_GAP_PX;
}

function prefersReducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

type Props = {
  slots: UploadActiveSlot[];
  reservedRows: number;
  formatSize: (current: number, total: number) => string;
};

export function UploadSlotList({ slots, reservedRows, formatSize }: Props) {
  const [rows, setRows] = useState<DisplayRow[]>([]);
  const exitTimers = useRef<Map<string, number>>(new Map());

  useEffect(() => {
    setRows((prev) => {
      const next: DisplayRow[] = [];
      const seen = new Set<string>();

      for (const slot of slots) {
        const key = slotKey(slot);
        seen.add(key);
        const existing = prev.find((r) => r.key === key);
        if (existing?.phase === "exit") {
          const timer = exitTimers.current.get(key);
          if (timer != null) {
            window.clearTimeout(timer);
            exitTimers.current.delete(key);
          }
          next.push({ key, slot, phase: "idle" });
        } else if (!existing) {
          next.push({ key, slot, phase: "enter" });
        } else {
          next.push({
            key,
            slot,
            phase: existing.phase === "enter" ? "enter" : "idle",
          });
        }
      }

      for (const row of prev) {
        if (seen.has(row.key)) continue;
        if (row.phase === "exit") {
          next.push(row);
          continue;
        }
        next.push({ ...row, phase: "exit" });
      }

      return next;
    });
  }, [slots]);

  useEffect(() => {
    if (!rows.some((r) => r.phase === "enter")) return;
    let raf2 = 0;
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        setRows((prev) =>
          prev.map((r) => (r.phase === "enter" ? { ...r, phase: "idle" } : r)),
        );
      });
    });
    return () => {
      cancelAnimationFrame(raf1);
      cancelAnimationFrame(raf2);
    };
  }, [rows]);

  const removeRow = (key: string) => {
    exitTimers.current.delete(key);
    setRows((cur) => cur.filter((r) => r.key !== key));
  };

  const scheduleExitRemoval = (key: string) => {
    if (exitTimers.current.has(key)) return;
    if (prefersReducedMotion()) {
      removeRow(key);
      return;
    }
    const id = window.setTimeout(() => removeRow(key), COLLAPSE_MS + 80);
    exitTimers.current.set(key, id);
  };

  const finishExit = (key: string) => {
    const timer = exitTimers.current.get(key);
    if (timer != null) {
      window.clearTimeout(timer);
      exitTimers.current.delete(key);
    }
    removeRow(key);
  };

  useEffect(() => {
    for (const row of rows) {
      if (row.phase === "exit") scheduleExitRemoval(row.key);
    }
  }, [rows]);

  useEffect(() => {
    return () => {
      for (const id of exitTimers.current.values()) window.clearTimeout(id);
      exitTimers.current.clear();
    };
  }, []);

  const listHeight = listMinHeightPx(reservedRows);
  if (reservedRows <= 0 && rows.length === 0) return null;

  return (
    <ul
      className="flex flex-col overflow-hidden"
      style={
        listHeight != null
          ? {
              height: listHeight,
              transition: `height ${COLLAPSE_MS}ms ${EASE}`,
            }
          : undefined
      }
      aria-label="Parallele Uploads"
    >
      {rows.map((row, index) => {
        const waiting =
          row.phase === "idle" &&
          row.slot.percent === 0 &&
          row.slot.current === 0;
        const collapsed = row.phase === "enter" || row.phase === "exit";
        const isLast = index === rows.length - 1;

        return (
          <li
            key={row.key}
            data-phase={row.phase}
            className={cn(
              "ams-slot-row overflow-hidden",
              row.phase === "exit" && "pointer-events-none",
            )}
            style={{
              marginBottom:
                isLast || row.phase === "exit" ? 0 : SLOT_GAP_PX,
              transition: `margin-bottom ${COLLAPSE_MS}ms ${EASE}`,
            }}
          >
            <div
              className="ams-slot-collapse"
              data-collapsed={collapsed ? "true" : "false"}
              style={{
                transition: `grid-template-rows ${COLLAPSE_MS}ms ${EASE}`,
              }}
              onTransitionEnd={(ev) => {
                if (row.phase !== "exit") return;
                if (ev.propertyName !== "grid-template-rows") return;
                finishExit(row.key);
              }}
            >
              <div className="ams-slot-collapse-inner min-h-0">
                <div
                  data-phase={row.phase}
                  className="ams-upload-slot min-h-[3.25rem]"
                >
                  <ProgressBar
                    size="sm"
                    percent={row.slot.percent}
                    label={row.slot.name}
                    sizeDetail={
                      waiting
                        ? undefined
                        : formatSize(row.slot.current, row.slot.total)
                    }
                    detail={
                      waiting ? undefined : `${Math.round(row.slot.percent)}%`
                    }
                    indeterminate={waiting}
                  />
                </div>
              </div>
            </div>
          </li>
        );
      })}
    </ul>
  );
}
