import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { z } from "zod";
import {
  AppConfigSchema,
  EngineSnapshotSchema,
  PointerCuePayloadSchema,
  TaskHistoryRecordSchema,
  type AppConfig,
  type EngineSnapshot,
  type PointerCuePayload,
  type TaskHistoryRecord,
  type ThemeMode,
  type UpdateAppConfigPayload,
} from "./types";
import { useTaskUiStore } from "./store";

const IS_DEV =
  typeof window !== "undefined" &&
  window.location.hostname === "localhost" &&
  window.location.port === "1420";

export const QUERY_KEYS = {
  snapshot: ["engine-snapshot"] as const,
  config: ["app-config"] as const,
  history: ["task-history"] as const,
};

function reportMutationError(error: unknown): void {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : "Unexpected action error";
  useTaskUiStore.getState().setToastError(message);
}

async function fetchEngineSnapshot(): Promise<EngineSnapshot> {
  const raw = await invoke<unknown>("get_engine_snapshot");
  return EngineSnapshotSchema.parse(raw);
}

async function fetchAppConfig(): Promise<AppConfig> {
  const raw = await invoke<unknown>("get_app_config");
  return AppConfigSchema.parse(raw);
}

async function fetchTaskHistory(): Promise<TaskHistoryRecord[]> {
  const raw = await invoke<unknown>("get_task_history");
  return z.array(TaskHistoryRecordSchema).parse(raw);
}

export function useEngineSnapshotQuery() {
  return useQuery({
    queryKey: QUERY_KEYS.snapshot,
    queryFn: fetchEngineSnapshot,
    refetchInterval: 10000,
  });
}

export function useAppConfigQuery() {
  return useQuery({
    queryKey: QUERY_KEYS.config,
    queryFn: fetchAppConfig,
    staleTime: 30000,
  });
}

export function useTaskHistoryQuery() {
  return useQuery({
    queryKey: QUERY_KEYS.history,
    queryFn: fetchTaskHistory,
  });
}

export function useFocusPermissionRefresh() {
  const queryClient = useQueryClient();

  useEffect(() => {
    function onFocus() {
      void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.snapshot });
    }
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [queryClient]);
}

export function useEngineEventSync() {
  const queryClient = useQueryClient();

  useEffect(() => {
    let active = true;
    let unlistenFn: (() => void) | null = null;

    listen<unknown>("engine-snapshot", (event) => {
      if (!active) {
        return;
      }
      const parsed = EngineSnapshotSchema.safeParse(event.payload);
      if (parsed.success) {
        queryClient.setQueryData(QUERY_KEYS.snapshot, parsed.data);
        if (
          parsed.data.phase === "completed" ||
          parsed.data.phase === "failed" ||
          parsed.data.phase === "stopped"
        ) {
          void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.history });
        }
      } else if (IS_DEV) {
        console.warn("Invalid engine-snapshot payload", parsed.error);
      }
    })
      .then((unlisten) => {
        if (active) {
          unlistenFn = unlisten;
        } else {
          unlisten();
        }
      })
      .catch((err: unknown) => {
        if (IS_DEV) {
          console.warn("Failed to listen for engine-snapshot", err);
        }
      });

    return () => {
      active = false;
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [queryClient]);
}

export function useThemeEventSync() {
  useEffect(() => {
    let active = true;
    let unlistenFn: (() => void) | null = null;

    listen<unknown>("theme-changed", (event) => {
      if (!active) {
        return;
      }
      if (event.payload === "light" || event.payload === "dark") {
        useTaskUiStore.getState().setTheme(event.payload);
      }
    })
      .then((unlisten) => {
        if (active) {
          unlistenFn = unlisten;
        } else {
          unlisten();
        }
      })
      .catch((err: unknown) => {
        if (IS_DEV) {
          console.warn("Failed to listen for theme-changed", err);
        }
      });

    return () => {
      active = false;
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, []);
}

export function useTaskMutations() {
  const queryClient = useQueryClient();

  const startTask = useMutation({
    mutationFn: async (params: { goal: string }) => {
      const raw = await invoke<unknown>("start_task", params);
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      useTaskUiStore.getState().clearToastError();
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
    },
    onError: reportMutationError,
  });

  const pauseTask = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("pause_task");
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
    },
    onError: reportMutationError,
  });

  const resumeTask = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("resume_task");
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
    },
    onError: reportMutationError,
  });

  const stopTask = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("stop_task");
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
      void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.history });
    },
    onError: reportMutationError,
  });

  const resolveConfirmation = useMutation({
    mutationFn: async (params: {
      confirmationId: string;
      approved: boolean;
    }) => {
      const raw = await invoke<unknown>("resolve_confirmation", params);
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
    },
    onError: reportMutationError,
  });

  const saveConfig = useMutation({
    mutationFn: async (config: UpdateAppConfigPayload) => {
      const raw = await invoke<unknown>("update_app_config", { config });
      return AppConfigSchema.parse(raw);
    },
    onSuccess: (saved) => {
      useTaskUiStore.getState().clearToastError();
      queryClient.setQueryData(QUERY_KEYS.config, saved);
      void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.snapshot });
    },
    onError: reportMutationError,
  });

  const testOllamaConnection = useMutation({
    mutationFn: async (baseUrl?: string) => {
      const raw = await invoke<unknown>("test_ollama_connection", {
        baseUrl: baseUrl ?? null,
      });
      return z.array(z.string()).parse(raw);
    },
    onError: reportMutationError,
  });

  const requestScreenPermission = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("request_screen_permission");
      return z.boolean().parse(raw);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.snapshot });
    },
    onError: reportMutationError,
  });

  const requestAccessibilityPermission = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("request_accessibility_permission");
      return z.boolean().parse(raw);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.snapshot });
    },
    onError: reportMutationError,
  });

  const testPointer = useMutation({
    mutationFn: async (): Promise<PointerCuePayload> => {
      const raw = await invoke<unknown>("fire_test_pointer");
      return PointerCuePayloadSchema.parse(raw);
    },
    onError: reportMutationError,
  });

  const hidePopover = useMutation({
    mutationFn: async (): Promise<void> => {
      await invoke<unknown>("hide_tray_window");
    },
    onError: reportMutationError,
  });

  const showPanel = useMutation({
    mutationFn: async (): Promise<void> => {
      await invoke<unknown>("show_orchestration_panel_window");
    },
    onError: reportMutationError,
  });

  const hidePanel = useMutation({
    mutationFn: async (): Promise<void> => {
      await invoke<unknown>("hide_orchestration_panel_window");
    },
    onError: reportMutationError,
  });

  const togglePanel = useMutation({
    mutationFn: async (): Promise<void> => {
      await invoke<unknown>("toggle_orchestration_panel_window");
    },
    onError: reportMutationError,
  });

  const setPanelCollapsed = useMutation({
    mutationFn: async (collapsed: boolean): Promise<void> => {
      await invoke<unknown>("set_orchestration_panel_collapsed_state", {
        collapsed,
      });
    },
    onError: reportMutationError,
  });

  const broadcastTheme = useMutation({
    mutationFn: async (theme: ThemeMode): Promise<void> => {
      await invoke<unknown>("broadcast_theme_mode", { theme });
    },
    onError: reportMutationError,
  });

  return {
    startTask,
    pauseTask,
    resumeTask,
    stopTask,
    resolveConfirmation,
    saveConfig,
    testOllamaConnection,
    requestScreenPermission,
    requestAccessibilityPermission,
    testPointer,
    hidePopover,
    showPanel,
    hidePanel,
    togglePanel,
    setPanelCollapsed,
    broadcastTheme,
  };
}
