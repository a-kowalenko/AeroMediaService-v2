import { create } from "zustand";
import {
  assignCustomerToFolder,
  deleteCustomer,
  getAssignmentHistory,
  listCustomers,
  saveCustomer,
  setCustomerProcessed,
  updateCustomer,
  type AssignmentHistoryEntry,
  type Customer,
} from "../lib/tauri";

export type CustomerFilter = "all" | "unprocessed" | "processed";

type CustomerState = {
  items: Customer[];
  history: AssignmentHistoryEntry[];
  search: string;
  filter: CustomerFilter;
  loading: boolean;
  error: string;
  message: string;
  setSearch: (search: string) => void;
  setFilter: (filter: CustomerFilter) => void;
  load: () => Promise<void>;
  loadHistory: () => Promise<void>;
  add: (
    vorname: string,
    nachname: string,
    email: string,
    telefon: string,
  ) => Promise<void>;
  update: (customer: Customer) => Promise<void>;
  remove: (id: string) => Promise<void>;
  setProcessed: (id: string, processed: boolean) => Promise<void>;
  assign: (id: string, targetPath: string) => Promise<string>;
};

export const useCustomerStore = create<CustomerState>((set, get) => ({
  items: [],
  history: [],
  search: "",
  filter: "unprocessed",
  loading: false,
  error: "",
  message: "",
  setSearch: (search) => {
    set({ search });
    void get().load();
  },
  setFilter: (filter) => {
    set({ filter });
    void get().load();
  },
  load: async () => {
    const { search, filter } = get();
    set({ loading: true, error: "" });
    try {
      const items = await listCustomers(search, filter);
      set({ items, loading: false });
    } catch (err) {
      set({ loading: false, error: String(err) });
    }
  },
  loadHistory: async () => {
    try {
      const history = await getAssignmentHistory();
      set({ history });
    } catch (err) {
      set({ error: String(err) });
    }
  },
  add: async (vorname, nachname, email, telefon) => {
    set({ error: "", message: "" });
    try {
      await saveCustomer(vorname, nachname, email, telefon);
      set({ message: "Kunde zur Warteschlange hinzugefügt." });
      await get().load();
    } catch (err) {
      set({ error: String(err) });
      throw err;
    }
  },
  update: async (customer) => {
    set({ error: "", message: "" });
    try {
      await updateCustomer(customer);
      set({ message: "Kunde aktualisiert." });
      await get().load();
    } catch (err) {
      set({ error: String(err) });
      throw err;
    }
  },
  remove: async (id) => {
    set({ error: "", message: "" });
    try {
      await deleteCustomer(id);
      set({ message: "Kunde gelöscht." });
      await get().load();
    } catch (err) {
      set({ error: String(err) });
      throw err;
    }
  },
  setProcessed: async (id, processed) => {
    set({ error: "", message: "" });
    try {
      await setCustomerProcessed(id, processed);
      set({
        message: processed
          ? "Kunde als „Bearbeitet“ markiert."
          : "Kunde als „Zu bearbeiten“ markiert.",
      });
      await get().load();
    } catch (err) {
      set({ error: String(err) });
      throw err;
    }
  },
  assign: async (id, targetPath) => {
    set({ error: "", message: "" });
    try {
      const result = await assignCustomerToFolder(id, targetPath);
      set({ message: `Marker geschrieben: ${result.file_path}` });
      await Promise.all([get().load(), get().loadHistory()]);
      return result.file_path;
    } catch (err) {
      set({ error: String(err) });
      throw err;
    }
  },
}));
