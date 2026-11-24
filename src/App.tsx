import { useEffect, useMemo } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { PointerOverlay } from "./components/PointerOverlay";
import { TrayControl } from "./components/TrayControl";
import { useFocusUiStore } from "./store";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

function detectOverlayView(): boolean {
  if (typeof window !== "undefined") {
    const params = new URLSearchParams(window.location.search);
    if (params.get("view") === "overlay") {
      return true;
    }
    if (params.get("view") === "tray") {
      return false;
    }
  }
  try {
    return getCurrentWindow().label === "overlay";
  } catch {
    return false;
  }
}

export function App() {
  const theme = useFocusUiStore((s) => s.theme);
  const isOverlay = useMemo(() => detectOverlayView(), []);

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
      {isOverlay ? <PointerOverlay /> : <TrayControl />}
    </QueryClientProvider>
  );
}
