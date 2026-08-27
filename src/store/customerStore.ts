import { create } from "zustand";
import {
  assignCustomerToFolder,
  assignCustomersBatch,
  deleteCustomer,
  getAssignmentHistory,
  listCustomers,
  saveCustomer,
  setCustomerProcessed,
  updateCustomer,
  type AssignmentHistoryEntry,
  type BatchAssignItem,
  type BatchAssignOutcome,
  type Customer,
  type CustomerDraft,
  type IdAssignOverride,
} from "../lib/tauri";
import { showAppToast } from "../lib/toast";

export type CustomerFilter = "all" | "unprocessed" | "processed";
export type CustomerView = "queue" | "history";

type CustomerState = {
  items: Customer[];
  history: AssignmentHistoryEntry[];
  search: string;
  filter: CustomerFilter;
  view: CustomerView;
  openCount: number;
  highlightId: string;
  loading: boolean;
  error: string;
  setSearch: (search: string) => void;
  setFilter: (filter: CustomerFilter) => void;
  setView: (view: CustomerView) => void;
  load: () => Promise<void>;
  loadHistory: () => Promise<void>;
  refreshCounts: () => Promise<void>;
  add: (draft: CustomerDraft) => Promise<Customer>;
  update: (customer: Customer) => Promise<void>;
  remove: (id: string) => Promise<void>;
  setProcessed: (id: string, processed: boolean) => Promise<void>;
  assign: (id: string, targetPath: string, idOverride?: IdAssignOverride | null) => Promise<string>;
  assignBatch: (items: BatchAssignItem[]) => Promise<BatchAssignOutcome>;
};

let searchTimer: ReturnType<typeof setTimeout> | undefined;
let highlightTimer: ReturnType<typeof setTimeout> | undefined;

function toastError(err: unknown, title: string) {
  const message = String(err);
  showAppToast(message, { tone: "error", title });
  return message;
}

export const useCustomerStore = create<CustomerState>((set, get) => ({
  items: [],
  history: [],
  search: "",
  filter: "all",
  view: "queue",
  openCount: 0,
  highlightId: "",
  loading: false,
  error: "",
  setSearch: (search) => {
    set({ search });
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => {
      void get().load();
    }, 250);
  },
  setFilter: (filter) => {
    set({ filter, view: "queue" });
    void get().load();
  },
  setView: (view) => {
    set({ view });
    if (view === "history") void get().loadHistory();
  },
  load: async () => {
    const { search, filter } = get();
    set({ loading: true, error: "" });
    try {
      const items = await listCustomers(search, filter);
      set({ items, loading: false });
      void get().refreshCounts();
    } catch (err) {
      set({ loading: false, error: toastError(err, "Kunden") });
    }
  },
  loadHistory: async () => {
    try {
      const history = await getAssignmentHistory();
      set({ history, error: "" });
    } catch (err) {
      set({ error: toastError(err, "Zuweisungen") });
    }
  },
  refreshCounts: async () => {
    try {
      const open = await listCustomers("", "unprocessed");
      set({ openCount: open.length });
    } catch {
      /* badge is best-effort */
    }
  },
  add: async (draft) => {
    set({ error: "" });
    try {
      const customer = await saveCustomer(draft);
      window.clearTimeout(highlightTimer);
      set({
        highlightId: customer.id,
        filter: get().filter === "processed" ? "all" : get().filter,
        view: "queue",
      });
      highlightTimer = window.setTimeout(() => {
        set((state) =>
          state.highlightId === customer.id ? { highlightId: "" } : state,
        );
      }, 5000);
      showAppToast(`${customer.vorname} ${customer.nachname} in der Warteschlange.`, {
        tone: "success",
        title: "Kunde aufgenommen",
      });
      await Promise.all([get().load(), get().loadHistory()]);
      return customer;
    } catch (err) {
      set({ error: toastError(err, "Kunde anlegen") });
      throw err;
    }
  },
  update: async (customer) => {
    set({ error: "" });
    try {
      await updateCustomer(customer);
      showAppToast("Kundendaten gespeichert.", { tone: "success" });
      await get().load();
    } catch (err) {
      set({ error: toastError(err, "Kunde") });
      throw err;
    }
  },
  remove: async (id) => {
    set({ error: "" });
    try {
      await deleteCustomer(id);
      showAppToast("Kunde gelöscht.", { tone: "success" });
      await get().load();
    } catch (err) {
      set({ error: toastError(err, "Kunde") });
      throw err;
    }
  },
  setProcessed: async (id, processed) => {
    set({ error: "" });
    try {
      await setCustomerProcessed(id, processed);
      showAppToast(
        processed ? "Als erledigt markiert." : "Wieder als offen markiert.",
        { tone: "success" },
      );
      await get().load();
    } catch (err) {
      set({ error: toastError(err, "Status") });
      throw err;
    }
  },
  assign: async (id, targetPath, idOverride) => {
    set({ error: "" });
    try {
      const result = await assignCustomerToFolder(id, targetPath, idOverride);
      showAppToast(`Marker geschrieben:\n${result.file_path}`, {
        tone: "success",
        title: "Zugewiesen",
      });
      await Promise.all([get().load(), get().loadHistory()]);
      return result.file_path;
    } catch (err) {
      set({ error: toastError(err, "Zuweisung") });
      throw err;
    }
  },
  assignBatch: async (items) => {
    set({ error: "" });
    try {
      const result = await assignCustomersBatch(items);
      await Promise.all([get().load(), get().loadHistory()]);
      if (result.errors.length === 0) {
        const n = result.assigned.length;
        showAppToast(
          n === 1 ? "1 Kunde zugewiesen." : `${n} Kunden zugewiesen.`,
          { tone: "success", title: "Sammelzuweisung" },
        );
      } else {
        const details = result.errors.map((e) => e.message).join("\n");
        showAppToast(
          `${result.assigned.length} ok, ${result.errors.length} Fehler.\n${details}`,
          { tone: "error", title: "Sammelzuweisung" },
        );
        set({ error: details });
      }
      return result;
    } catch (err) {
      set({ error: toastError(err, "Sammelzuweisung") });
      throw err;
    }
  },
}));
