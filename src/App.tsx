import { useEffect, useMemo } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { FloatingOrchestrationHud } from "./components/OrchestrationPanel";
import { PointerOverlay } from "./components/PointerOverlay";
import { TrayControl } from "./components/TrayControl";
import { useTaskUiStore } from "./store";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

type AppView = "tray" | "overlay" | "panel";

function detectAppView(): AppView {
  if (typeof window !== "undefined") {
    const params = new URLSearchParams(window.location.search);
    const paramView = params.get("view");
    if (paramView === "overlay" || paramView === "panel" || paramView === "tray") {
      return paramView;
    }
  }
  try {
    const label = getCurrentWindow().label;
    if (label === "overlay" || label === "panel") {
      return label;
    }
    return "tray";
  } catch {
    return "tray";
  }
}

export function App() {
  const theme = useTaskUiStore((s) => s.theme);
  const view = useMemo(() => detectAppView(), []);

  useEffect(() => {
    const root = document.documentElement;
    if (theme === "dark") {
      root.classList.add("dark");
    } else {
      root.classList.remove("dark");
    }
  }, [theme]);

  return (
    <QueryClientProvider client={queryClient}>
      {view === "overlay" ? (
        <PointerOverlay />
      ) : view === "panel" ? (
        <FloatingOrchestrationHud />
      ) : (
        <TrayControl />
      )}
    </QueryClientProvider>
  );
}
