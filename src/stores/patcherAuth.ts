import { create } from "zustand";

type PatcherAuthStatus = "unknown" | "requesting" | "authenticated" | "denied";

interface PatcherAuthStore {
  status: PatcherAuthStatus;
  setRequesting: () => void;
  setAuthenticated: () => void;
  setDenied: () => void;
}

export const usePatcherAuthStore = create<PatcherAuthStore>((set) => ({
  status: "unknown",
  setRequesting: () => set({ status: "requesting" }),
  setAuthenticated: () => set({ status: "authenticated" }),
  setDenied: () => set({ status: "denied" }),
}));
