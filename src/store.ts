import { create } from "zustand";
import type {
  PanelFilter,
  PointerCuePayload,
  ThemeMode,
  TrayTab,
} from "./types";

interface TaskUiState {
  activeTab: TrayTab;
  theme: ThemeMode;
  draftGoal: string;
  activeCue: PointerCuePayload | null;
  panelCollapsed: boolean;
  panelFilter: PanelFilter;
  setActiveTab: (tab: TrayTab) => void;
  setTheme: (theme: ThemeMode) => void;
  toggleTheme: () => void;
  setDraftGoal: (goal: string) => void;
  setActiveCue: (cue: PointerCuePayload | null) => void;
  setPanelCollapsed: (collapsed: boolean) => void;
  setPanelFilter: (filter: PanelFilter) => void;
}

export const useTaskUiStore = create<TaskUiState>((set) => ({
  activeTab: "task",
  theme: "dark",
  draftGoal: "",
  activeCue: null,
  panelCollapsed: false,
  panelFilter: "all",
  setActiveTab: (activeTab) => set({ activeTab }),
  setTheme: (theme) => set({ theme }),
  toggleTheme: () =>
    set((state) => ({
      theme: state.theme === "dark" ? "light" : "dark",
    })),
  setDraftGoal: (draftGoal) => set({ draftGoal }),
  setActiveCue: (activeCue) => set({ activeCue }),
  setPanelCollapsed: (panelCollapsed) => set({ panelCollapsed }),
  setPanelFilter: (panelFilter) => set({ panelFilter }),
}));
