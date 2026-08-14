import { create } from "zustand";
import type { LogMessage } from "@/lib/tauri";

const MAX_ENTRIES = 500;

export type LogLevelFilter = "all" | "info" | "warn" | "error";

export type UiLogEntry = LogMessage & {
  id: number;
  ts: string;
};

type LogState = {
  open: boolean;
  entries: UiLogEntry[];
  search: string;
  levelFilter: LogLevelFilter;
  autoScroll: boolean;
  unreadErrors: number;
  nextId: number;
  setOpen: (open: boolean) => void;
  toggleOpen: () => void;
  setSearch: (search: string) => void;
  setLevelFilter: (filter: LogLevelFilter) => void;
  setAutoScroll: (autoScroll: boolean) => void;
  replaceEntries: (entries: LogMessage[]) => void;
  appendEntry: (entry: LogMessage) => void;
  clearEntries: () => void;
  markSeen: () => void;
};

function stamp(): string {
  return new Date().toLocaleTimeString("de-DE", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function toUi(entry: LogMessage, id: number, ts = stamp()): UiLogEntry {
  return { ...entry, id, ts };
}

export const useLogStore = create<LogState>((set, get) => ({
  open: false,
  entries: [],
  search: "",
  levelFilter: "all",
  autoScroll: true,
  unreadErrors: 0,
  nextId: 1,

  setOpen: (open) => {
    set({ open });
    if (open) get().markSeen();
  },
  toggleOpen: () => {
    const next = !get().open;
    set({ open: next });
    if (next) get().markSeen();
  },
  setSearch: (search) => set({ search }),
  setLevelFilter: (levelFilter) => set({ levelFilter }),
  setAutoScroll: (autoScroll) => set({ autoScroll }),

  replaceEntries: (entries) => {
    let id = 1;
    const mapped = entries.map((e) => toUi(e, id++, "—"));
    const trimmed =
      mapped.length > MAX_ENTRIES
        ? mapped.slice(mapped.length - MAX_ENTRIES)
        : mapped;
    set({ entries: trimmed, nextId: id, unreadErrors: 0 });
  },

  appendEntry: (entry) => {
    const { entries, open, nextId } = get();
    const ui = toUi(entry, nextId);
    const next = [...entries, ui];
    const trimmed =
      next.length > MAX_ENTRIES ? next.slice(next.length - MAX_ENTRIES) : next;
    const isError = entry.level_name.toLowerCase() === "error";
    set({
      entries: trimmed,
      nextId: nextId + 1,
      unreadErrors: !open && isError ? get().unreadErrors + 1 : get().unreadErrors,
    });
  },

  clearEntries: () => set({ entries: [], unreadErrors: 0 }),

  markSeen: () => set({ unreadErrors: 0 }),
}));

const LEVEL_RANK: Record<string, number> = {
  debug: 10,
  info: 20,
  warn: 30,
  warning: 30,
  error: 40,
};

function filterMinRank(filter: LogLevelFilter): number {
  switch (filter) {
    case "info":
      return 20;
    case "warn":
      return 30;
    case "error":
      return 40;
    default:
      return 0;
  }
}

export function filterLogEntries(
  entries: UiLogEntry[],
  search: string,
  levelFilter: LogLevelFilter,
): UiLogEntry[] {
  const q = search.trim().toLowerCase();
  const minRank = filterMinRank(levelFilter);
  return entries.filter((e) => {
    const rank = LEVEL_RANK[e.level_name.toLowerCase()] ?? 20;
    if (rank < minRank) return false;
    if (!q) return true;
    return (
      e.message.toLowerCase().includes(q) ||
      e.level_name.toLowerCase().includes(q)
    );
  });
}
