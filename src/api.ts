import axios, { type AxiosInstance } from "axios";
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
} from "./types";

export function createOllamaAxiosInstance(
  baseURL: string,
  apiKey: string
): AxiosInstance {
  const client = axios.create({
    baseURL,
    timeout: 10000,
    headers: {
      "Content-Type": "application/json",
    },
  });

  client.interceptors.request.use((config) => {
    const trimmed = apiKey.trim();
    if (trimmed.length > 0) {
      config.headers.Authorization = `Bearer ${trimmed}`;
    }
    return config;
  });

  client.interceptors.response.use(
    (response) => response,
    (error: unknown) => {
      if (axios.isAxiosError(error)) {
        const message =
          error.response?.status !== undefined
            ? `Ollama HTTP ${error.response.status}`
            : error.message;
        return Promise.reject(new Error(message));
      }
      return Promise.reject(new Error("Unexpected network failure"));
    }
  );

  return client;
}

export const QUERY_KEYS = {
  snapshot: ["engine-snapshot"] as const,
  config: ["app-config"] as const,
  history: ["task-history"] as const,
};

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
    refetchInterval: 1500,
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
      }
    })
      .then((unlisten) => {
        if (active) {
          unlistenFn = unlisten;
        } else {
          unlisten();
        }
      })
      .catch(() => undefined);

    return () => {
      active = false;
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [queryClient]);
}

export function useTaskMutations() {
  const queryClient = useQueryClient();

  const startTask = useMutation({
    mutationFn: async (params: { goal: string }) => {
      const raw = await invoke<unknown>("start_task", params);
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
    },
  });

  const pauseTask = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("pause_task");
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
    },
  });

  const resumeTask = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("resume_task");
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
    },
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
  });

  const saveConfig = useMutation({
    mutationFn: async (config: AppConfig) => {
      const raw = await invoke<unknown>("update_app_config", { config });
      return AppConfigSchema.parse(raw);
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(QUERY_KEYS.config, saved);
      void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.snapshot });
    },
  });

  const requestScreenPermission = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("request_screen_permission");
      return z.boolean().parse(raw);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.snapshot });
    },
  });

  const requestAccessibilityPermission = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("request_accessibility_permission");
      return z.boolean().parse(raw);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.snapshot });
    },
  });

  const testPointer = useMutation({
    mutationFn: async (): Promise<PointerCuePayload> => {
      const raw = await invoke<unknown>("fire_test_pointer");
      return PointerCuePayloadSchema.parse(raw);
    },
  });

  const hidePopover = useMutation({
    mutationFn: async (): Promise<void> => {
      await invoke<unknown>("hide_tray_window");
    },
  });

  return {
    startTask,
    pauseTask,
    resumeTask,
    stopTask,
    resolveConfirmation,
    saveConfig,
    requestScreenPermission,
    requestAccessibilityPermission,
    testPointer,
    hidePopover,
  };
}
