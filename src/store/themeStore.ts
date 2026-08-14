import { create } from "zustand";

export type ThemeMode = "light" | "dark";

const STORAGE_KEY = "ams-theme";

function readStored(): ThemeMode | null {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === "light" || v === "dark") return v;
  } catch {
    /* ignore */
  }
  return null;
}

function applyDom(mode: ThemeMode) {
  document.documentElement.classList.toggle("dark", mode === "dark");
  document.documentElement.dataset.theme = mode;
}

export function initTheme(): ThemeMode {
  const stored = readStored();
  // Product default is dark (existing AMS look); light is opt-in.
  const mode = stored ?? "dark";
  applyDom(mode);
  return mode;
}

type ThemeState = {
  mode: ThemeMode;
  setMode: (mode: ThemeMode) => void;
  toggle: () => void;
};

export const useThemeStore = create<ThemeState>((set, get) => ({
  mode: "dark",
  setMode: (mode) => {
    applyDom(mode);
    try {
      localStorage.setItem(STORAGE_KEY, mode);
    } catch {
      /* ignore */
    }
    set({ mode });
  },
  toggle: () => {
    const next: ThemeMode = get().mode === "light" ? "dark" : "light";
    get().setMode(next);
  },
}));
