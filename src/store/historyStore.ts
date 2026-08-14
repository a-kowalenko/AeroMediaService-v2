import { create } from "zustand";
import {
  clearHistory,
  deleteHistoryItems,
  getHistory,
  type HistoryEntry,
  type HistoryPage,
} from "../lib/tauri";

const PAGE_SIZE = 25;

type HistoryState = {
  items: HistoryEntry[];
  total: number;
  page: number;
  pageSize: number;
  search: string;
  selectedId: string | null;
  loading: boolean;
  error: string;
  setSearch: (search: string) => void;
  setPage: (page: number) => void;
  select: (id: string | null) => void;
  load: (opts?: { maintainPage?: boolean }) => Promise<void>;
  removeSelected: () => Promise<void>;
  removeAll: () => Promise<void>;
};

export const useHistoryStore = create<HistoryState>((set, get) => ({
  items: [],
  total: 0,
  page: 0,
  pageSize: PAGE_SIZE,
  search: "",
  selectedId: null,
  loading: false,
  error: "",
  setSearch: (search) => {
    set({ search, page: 0 });
    void get().load();
  },
  setPage: (page) => {
    set({ page });
    void get().load({ maintainPage: true });
  },
  select: (selectedId) => set({ selectedId }),
  load: async (opts) => {
    const { search, page, pageSize, selectedId } = get();
    set({ loading: true, error: "" });
    try {
      const result: HistoryPage = await getHistory(search, opts?.maintainPage ? page : 0, pageSize);
      const stillSelected = result.items.some((item) => item.id === selectedId);
      set({
        items: result.items,
        total: result.total,
        page: result.page,
        pageSize: result.page_size,
        selectedId: stillSelected ? selectedId : null,
        loading: false,
      });
    } catch (err) {
      set({ loading: false, error: String(err) });
    }
  },
  removeSelected: async () => {
    const { selectedId } = get();
    if (!selectedId) return;
    await deleteHistoryItems([selectedId]);
    set({ selectedId: null });
    await get().load({ maintainPage: true });
  },
  removeAll: async () => {
    await clearHistory();
    set({ selectedId: null, page: 0 });
    await get().load();
  },
}));
