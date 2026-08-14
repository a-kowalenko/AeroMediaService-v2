import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  ArrowDownToLine,
  Copy,
  Eraser,
  Terminal,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { LOG_MESSAGE } from "@/lib/events";
import { getRecentLogs, type LogMessage } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import {
  filterLogEntries,
  useLogStore,
  type LogLevelFilter,
} from "@/store/logStore";

const MIN_HEIGHT = 160;
const MAX_HEIGHT = 560;
const DEFAULT_HEIGHT = 280;
const MAX_LINES = 500;

function levelClass(name: string): string {
  switch (name.toLowerCase()) {
    case "error":
      return "text-destructive";
    case "warn":
    case "warning":
      return "text-warning";
    case "debug":
      return "text-muted";
    default:
      return "text-foreground";
  }
}

type Props = {
  className?: string;
};

export function LogConsole({ className }: Props) {
  const open = useLogStore((s) => s.open);
  const setOpen = useLogStore((s) => s.setOpen);
  const entries = useLogStore((s) => s.entries);
  const search = useLogStore((s) => s.search);
  const setSearch = useLogStore((s) => s.setSearch);
  const levelFilter = useLogStore((s) => s.levelFilter);
  const setLevelFilter = useLogStore((s) => s.setLevelFilter);
  const autoScroll = useLogStore((s) => s.autoScroll);
  const setAutoScroll = useLogStore((s) => s.setAutoScroll);
  const clearEntries = useLogStore((s) => s.clearEntries);
  const replaceEntries = useLogStore((s) => s.replaceEntries);
  const appendEntry = useLogStore((s) => s.appendEntry);

  const [height, setHeight] = useState(DEFAULT_HEIGHT);
  const [copyFlash, setCopyFlash] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ startY: number; startH: number } | null>(null);
  const bootstrapped = useRef(false);

  const filtered = useMemo(
    () => filterLogEntries(entries, search, levelFilter),
    [entries, search, levelFilter],
  );

  useEffect(() => {
    if (bootstrapped.current) return;
    bootstrapped.current = true;
    getRecentLogs(MAX_LINES)
      .then(replaceEntries)
      .catch(() => {});
  }, [replaceEntries]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<LogMessage>(LOG_MESSAGE, (event) => {
      appendEntry(event.payload);
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
    };
  }, [appendEntry]);

  useEffect(() => {
    if (!open || !autoScroll) return;
    const el = listRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [filtered, open, autoScroll]);

  useEffect(() => {
    function onMove(e: MouseEvent) {
      if (!dragRef.current) return;
      const delta = dragRef.current.startY - e.clientY;
      const next = Math.min(
        MAX_HEIGHT,
        Math.max(MIN_HEIGHT, dragRef.current.startH + delta),
      );
      setHeight(next);
    }
    function onUp() {
      dragRef.current = null;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    }
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  useEffect(() => {
    if (!open) return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        setOpen(false);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, setOpen]);

  if (!open) return null;

  function onListScroll() {
    const el = listRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    if (atBottom !== autoScroll) setAutoScroll(atBottom);
  }

  async function handleCopy() {
    const text = filtered
      .map((e) => `[${e.ts}] [${e.level_name}] ${e.message}`)
      .join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setCopyFlash(true);
      window.setTimeout(() => setCopyFlash(false), 1200);
    } catch {
      // ignore
    }
  }

  return (
    <div
      className={cn(
        "flex shrink-0 flex-col border-t border-border bg-card/95 shadow-[0_-8px_24px_rgba(0,0,0,0.08)] backdrop-blur-md",
        className,
      )}
      style={{ height }}
    >
      <div
        role="separator"
        aria-orientation="horizontal"
        aria-label="Konsolenhöhe"
        className="group flex h-2 cursor-ns-resize items-center justify-center"
        onMouseDown={(e) => {
          dragRef.current = { startY: e.clientY, startH: height };
          document.body.style.cursor = "ns-resize";
          document.body.style.userSelect = "none";
        }}
      >
        <span className="h-0.5 w-10 rounded-full bg-border group-hover:bg-muted" />
      </div>

      <div className="flex flex-wrap items-center gap-2 border-b border-border/80 px-3 pb-2">
        <div className="flex items-center gap-1.5 text-xs font-medium text-foreground">
          <Terminal className="h-3.5 w-3.5 text-primary" />
          Konsole
          <span className="tabular-nums text-muted">
            ({filtered.length}/{entries.length})
          </span>
        </div>

        <Input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Suchen…"
          className="h-8 max-w-xs flex-1 text-xs"
          aria-label="Konsole durchsuchen"
        />

        <Select
          value={levelFilter}
          onValueChange={(v) => setLevelFilter(v as LogLevelFilter)}
        >
          <SelectTrigger className="h-8 w-[7.5rem] text-xs" aria-label="Level-Filter">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">Alle</SelectItem>
            <SelectItem value="info">Info+</SelectItem>
            <SelectItem value="warn">Warn+</SelectItem>
            <SelectItem value="error">Error</SelectItem>
          </SelectContent>
        </Select>

        <Button
          type="button"
          variant={autoScroll ? "default" : "secondary"}
          size="sm"
          className="h-8 gap-1.5 text-xs"
          onClick={() => setAutoScroll(!autoScroll)}
          title="Automatisch nach unten scrollen"
        >
          <ArrowDownToLine className="h-3.5 w-3.5" />
          Auto
        </Button>

        <Button
          type="button"
          variant="secondary"
          size="sm"
          className="h-8 gap-1.5 text-xs"
          onClick={() => void handleCopy()}
          title="Sichtbare Zeilen kopieren"
        >
          <Copy className="h-3.5 w-3.5" />
          {copyFlash ? "Kopiert" : "Kopieren"}
        </Button>

        <Button
          type="button"
          variant="secondary"
          size="sm"
          className="h-8 gap-1.5 text-xs"
          onClick={() => clearEntries()}
          title="Ansicht leeren"
        >
          <Eraser className="h-3.5 w-3.5" />
          Leeren
        </Button>

        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="ml-auto h-8 w-8"
          onClick={() => setOpen(false)}
          aria-label="Konsole schließen"
          title="Schließen (Esc)"
        >
          <X className="h-4 w-4" />
        </Button>
      </div>

      <div
        ref={listRef}
        onScroll={onListScroll}
        className="min-h-0 flex-1 overflow-auto px-3 py-2 font-mono text-[11px] leading-5"
      >
        {filtered.length === 0 ? (
          <p className="text-xs text-muted">Keine Log-Einträge.</p>
        ) : (
          filtered.map((e) => (
            <div
              key={e.id}
              className={cn(
                "flex gap-2 whitespace-pre-wrap break-all",
                levelClass(e.level_name),
              )}
            >
              <span className="shrink-0 tabular-nums text-muted">{e.ts}</span>
              <span
                className={cn(
                  "w-14 shrink-0 font-semibold uppercase",
                  levelClass(e.level_name),
                )}
              >
                {e.level_name}
              </span>
              <span className="min-w-0 flex-1">{e.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

export function LogConsoleToggleButton({
  className,
  disabled,
}: {
  className?: string;
  disabled?: boolean;
}) {
  const open = useLogStore((s) => s.open);
  const toggleOpen = useLogStore((s) => s.toggleOpen);
  const unreadErrors = useLogStore((s) => s.unreadErrors);

  return (
    <Button
      type="button"
      variant={open ? "default" : "secondary"}
      size="icon"
      className={cn("relative", className)}
      onClick={toggleOpen}
      disabled={disabled}
      aria-label="Konsole"
      aria-pressed={open}
      title="Konsole"
    >
      <Terminal className="h-4 w-4" />
      {unreadErrors > 0 && !open ? (
        <span
          className="absolute -right-0.5 -top-0.5 h-2.5 w-2.5 rounded-full bg-destructive ring-2 ring-card"
          aria-hidden
        />
      ) : null}
    </Button>
  );
}
