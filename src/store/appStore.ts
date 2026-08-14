import { create } from "zustand";

type AppState = {
  monitoring: boolean;
  connectionStatus: string;
  uploadJobActive: boolean;
  setMonitoring: (value: boolean) => void;
  setConnectionStatus: (value: string) => void;
  setUploadJobActive: (value: boolean) => void;
};

export function isCloudConnected(status: string): boolean {
  return status.trim() === "Verbunden";
}

export const useAppStore = create<AppState>((set) => ({
  monitoring: false,
  connectionStatus: "",
  uploadJobActive: false,
  setMonitoring: (monitoring) => set({ monitoring }),
  setConnectionStatus: (connectionStatus) => set({ connectionStatus }),
  setUploadJobActive: (uploadJobActive) => set({ uploadJobActive }),
}));
