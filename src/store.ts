import { create } from "zustand";
import type { PointerCuePayload, ThemeMode, TrayTab } from "./types";

interface TaskUiState {
  activeTab: TrayTab;
  theme: ThemeMode;
  draftGoal: string;
  activeCue: PointerCuePayload | null;
  setActiveTab: (tab: TrayTab) => void;
  toggleTheme: () => void;
  setDraftGoal: (goal: string) => void;
  setActiveCue: (cue: PointerCuePayload | null) => void;
}

export const useTaskUiStore = create<TaskUiState>((set) => ({
  activeTab: "task",
  theme: "dark",
  draftGoal: "",
  activeCue: null,
  setActiveTab: (activeTab) => set({ activeTab }),
  toggleTheme: () =>
    set((state) => ({
      theme: state.theme === "dark" ? "light" : "dark",
    })),
  setDraftGoal: (draftGoal) => set({ draftGoal }),
  setActiveCue: (activeCue) => set({ activeCue }),
}));
