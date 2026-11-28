import { create } from "zustand";
import type {
  PanelFilter,
  PointerCuePayload,
  ThemeMode,
  TrayTab,
} from "./types";

const THEME_STORAGE_KEY = "tc-theme";

function detectInitialTheme(): ThemeMode {
  if (typeof window === "undefined") {
    return "dark";
  }
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (stored === "light" || stored === "dark") {
      return stored;
    }
    if (
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-color-scheme: light)").matches
    ) {
      return "light";
    }
  } catch {
    return "dark";
  }
  return "dark";
}

function persistTheme(theme: ThemeMode): void {
  if (typeof window === "undefined") {
    return;
  }
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    return;
  }
}

interface TaskUiState {
  activeTab: TrayTab;
  theme: ThemeMode;
  draftGoal: string;
  activeCue: PointerCuePayload | null;
  panelCollapsed: boolean;
  panelFilter: PanelFilter;
  toastError: string | null;
  setActiveTab: (tab: TrayTab) => void;
  setTheme: (theme: ThemeMode) => void;
  toggleTheme: () => void;
  setDraftGoal: (goal: string) => void;
  setActiveCue: (cue: PointerCuePayload | null) => void;
  setPanelCollapsed: (collapsed: boolean) => void;
  setPanelFilter: (filter: PanelFilter) => void;
  setToastError: (message: string | null) => void;
  clearToastError: () => void;
}

export const useTaskUiStore = create<TaskUiState>((set) => ({
  activeTab: "task",
  theme: detectInitialTheme(),
  draftGoal: "",
  activeCue: null,
  panelCollapsed: false,
  panelFilter: "all",
  toastError: null,
  setActiveTab: (activeTab) => set({ activeTab }),
  setTheme: (theme) => {
    persistTheme(theme);
    set({ theme });
  },
  toggleTheme: () =>
    set((state) => {
      const next: ThemeMode = state.theme === "dark" ? "light" : "dark";
      persistTheme(next);
      return { theme: next };
    }),
  setDraftGoal: (draftGoal) => set({ draftGoal }),
  setActiveCue: (activeCue) => set({ activeCue }),
  setPanelCollapsed: (panelCollapsed) => set({ panelCollapsed }),
  setPanelFilter: (panelFilter) => set({ panelFilter }),
  setToastError: (toastError) => set({ toastError }),
  clearToastError: () => set({ toastError: null }),
}));
