import { create } from "zustand";
import type { NudgeEventPayload, ThemeMode, TrayTab } from "./types";

interface FocusUiState {
  activeTab: TrayTab;
  theme: ThemeMode;
  draftTask: string;
  draftDurationMinutes: number;
  activeNudge: NudgeEventPayload | null;
  setActiveTab: (tab: TrayTab) => void;
  toggleTheme: () => void;
  setDraftTask: (task: string) => void;
  setDraftDurationMinutes: (minutes: number) => void;
  setActiveNudge: (nudge: NudgeEventPayload | null) => void;
}

export const useFocusUiStore = create<FocusUiState>((set) => ({
  activeTab: "focus",
  theme: "dark",
  draftTask: "Building a Rust focus app",
  draftDurationMinutes: 25,
  activeNudge: null,
  setActiveTab: (activeTab) => set({ activeTab }),
  toggleTheme: () =>
    set((state) => ({
      theme: state.theme === "dark" ? "light" : "dark",
    })),
  setDraftTask: (draftTask) => set({ draftTask }),
  setDraftDurationMinutes: (minutes) =>
    set({
      draftDurationMinutes: Math.min(240, Math.max(1, Math.round(minutes))),
    }),
  setActiveNudge: (activeNudge) => set({ activeNudge }),
}));
