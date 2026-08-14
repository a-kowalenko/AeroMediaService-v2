import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { CloudUpload, Pause, Play, X } from "lucide-react";
import { Panel } from "./Panel";
import { ProgressBar } from "./ProgressBar";
import { Button } from "@/components/ui/button";
import {
  UPLOAD_FAILED,
  UPLOAD_FINISHED,
  UPLOAD_JOB_ACTIVE,
  UPLOAD_PROGRESS_FILE,
  UPLOAD_PROGRESS_TOTAL,
  UPLOAD_QUEUE_CHANGED,
  UPLOAD_STATUS_UPDATE,
} from "@/lib/events";
import {
  cancelUpload,
  getUploadQueue,
  pauseUpload,
  resumeUpload,
  type ByteProgress,
  type QueueSnapshotItem,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";

const EMPTY_PROGRESS: ByteProgress = { percent: 0, current: 0, total: 0 };

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

type Props = {
  className?: string;
  compact?: boolean;
};

export function UploadPanel({ className, compact = false }: Props) {
  const [active, setActive] = useState(false);
  const [status, setStatus] = useState("Warte auf nächsten Auftrag…");
  const [file, setFile] = useState<ByteProgress>(EMPTY_PROGRESS);
  const [total, setTotal] = useState<ByteProgress>(EMPTY_PROGRESS);
  const [queue, setQueue] = useState<QueueSnapshotItem[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getUploadQueue().then(setQueue).catch(() => {});
  }, []);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    const add = <T,>(name: string, handler: (payload: T) => void) => {
      listen<T>(name, (event) => handler(event.payload))
        .then((fn) => unlisteners.push(fn))
        .catch(() => {});
    };
    add<boolean>(UPLOAD_JOB_ACTIVE, setActive);
    add<string>(UPLOAD_STATUS_UPDATE, setStatus);
    add<ByteProgress>(UPLOAD_PROGRESS_FILE, setFile);
    add<ByteProgress>(UPLOAD_PROGRESS_TOTAL, setTotal);
    add<QueueSnapshotItem[]>(UPLOAD_QUEUE_CHANGED, setQueue);
    add<string>(UPLOAD_FINISHED, (msg) => setStatus(`Erfolgreich: ${msg}`));
    add<string>(UPLOAD_FAILED, (msg) => setStatus(`Fehler: ${msg}`));
    return () => {
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

  return (
    <Panel
      className={className}
      compact={compact}
      title="Upload"
      description={active ? "Job aktiv" : "Bereit"}
      actions={
        <span
          className={cn(
            "inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-[11px] font-medium",
            active
              ? "border-primary/35 bg-primary/10 text-primary"
              : "border-border bg-card-elevated/80 text-muted",
          )}
        >
          <span
            className={cn(
              "h-1.5 w-1.5 rounded-full",
              active ? "bg-primary" : "bg-muted",
            )}
          />
          {active ? "Aktiv" : "Idle"}
        </span>
      }
    >
      <div className="mb-3 flex items-start gap-2 text-sm text-muted">
        <CloudUpload className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
        <span className="min-w-0 leading-snug">{status}</span>
      </div>

      <div className="space-y-3">
        <ProgressBar
          percent={file.percent}
          label={`Datei · ${formatBytes(file.current)} / ${formatBytes(file.total)}`}
          detail={`${Math.round(file.percent)}%`}
        />
        <ProgressBar
          percent={total.percent}
          label={`Gesamt · ${formatBytes(total.current)} / ${formatBytes(total.total)}`}
          detail={`${Math.round(total.percent)}%`}
        />
      </div>

      <div className="mt-3.5 flex flex-wrap gap-1.5">
        <Button
          type="button"
          variant="secondary"
          size="sm"
          disabled={busy || !active}
          onClick={() => run(pauseUpload)}
        >
          <Pause className="h-3.5 w-3.5" />
          Pause
        </Button>
        <Button
          type="button"
          variant="secondary"
          size="sm"
          disabled={busy || !active}
          onClick={() => run(resumeUpload)}
        >
          <Play className="h-3.5 w-3.5" />
          Fortsetzen
        </Button>
        <Button
          type="button"
          variant="secondary"
          size="sm"
          className="border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive"
          disabled={busy || !active}
          onClick={() => run(cancelUpload)}
        >
          <X className="h-3.5 w-3.5" />
          Abbrechen
        </Button>
      </div>

      <div className="mt-4 border-t border-border/70 pt-3">
        <p className="mb-2 text-[11px] font-semibold tracking-[0.08em] text-muted uppercase">
          Warteschlange
        </p>
        {queue.length === 0 ? (
          <p className="text-sm text-muted">Keine Aufträge.</p>
        ) : (
          <ul className="space-y-1.5">
            {queue.map((item) => (
              <li
                key={`${item.position}-${item.dir_name}`}
                className="rounded-lg border border-border/70 bg-card-elevated/60 px-2.5 py-2 text-sm"
              >
                <div className="flex items-center justify-between gap-2">
                  <strong className="truncate text-foreground">
                    {item.dir_name}
                  </strong>
                  <span
                    className={cn(
                      "shrink-0 rounded-md px-1.5 py-0.5 text-[10px] font-medium",
                      item.state === "active"
                        ? "bg-primary/15 text-primary"
                        : "bg-background/70 text-muted",
                    )}
                  >
                    {item.state === "active"
                      ? "läuft"
                      : `wartet${
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
        )}
      </div>
    </Panel>
  );
}
