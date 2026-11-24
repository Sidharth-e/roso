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
  NudgeEventPayloadSchema,
  SessionHistoryRecordSchema,
  type AppConfig,
  type EngineSnapshot,
  type NudgeEventPayload,
  type SessionHistoryRecord,
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
  history: ["focus-history"] as const,
};

async function fetchEngineSnapshot(): Promise<EngineSnapshot> {
  const raw = await invoke<unknown>("get_engine_snapshot");
  return EngineSnapshotSchema.parse(raw);
}

async function fetchAppConfig(): Promise<AppConfig> {
  const raw = await invoke<unknown>("get_app_config");
  return AppConfigSchema.parse(raw);
}

async function fetchFocusHistory(): Promise<SessionHistoryRecord[]> {
  const raw = await invoke<unknown>("get_focus_history");
  return z.array(SessionHistoryRecordSchema).parse(raw);
}

export function useEngineSnapshotQuery() {
  return useQuery({
    queryKey: QUERY_KEYS.snapshot,
    queryFn: fetchEngineSnapshot,
    refetchInterval: 2000,
  });
}

export function useAppConfigQuery() {
  return useQuery({
    queryKey: QUERY_KEYS.config,
    queryFn: fetchAppConfig,
    staleTime: 30000,
  });
}

export function useFocusHistoryQuery() {
  return useQuery({
    queryKey: QUERY_KEYS.history,
    queryFn: fetchFocusHistory,
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

export function useFocusMutations() {
  const queryClient = useQueryClient();

  const startSession = useMutation({
    mutationFn: async (params: {
      declaredTask: string;
      durationMinutes: number;
    }) => {
      const raw = await invoke<unknown>("start_focus_session", params);
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
    },
  });

  const pauseSession = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("pause_focus_session");
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
    },
  });

  const resumeSession = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("resume_focus_session");
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
    },
  });

  const stopSession = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("stop_focus_session");
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
      void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.history });
    },
  });

  const toggleDeveloperMode = useMutation({
    mutationFn: async (enabled: boolean) => {
      const raw = await invoke<unknown>("toggle_developer_mode", { enabled });
      return EngineSnapshotSchema.parse(raw);
    },
    onSuccess: (snapshot) => {
      queryClient.setQueryData(QUERY_KEYS.snapshot, snapshot);
      void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.config });
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

  const requestPermission = useMutation({
    mutationFn: async () => {
      const raw = await invoke<unknown>("request_screen_permission");
      return z.boolean().parse(raw);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: QUERY_KEYS.snapshot });
    },
  });

  const testNudge = useMutation({
    mutationFn: async (): Promise<NudgeEventPayload> => {
      const raw = await invoke<unknown>("fire_test_nudge");
      return NudgeEventPayloadSchema.parse(raw);
    },
  });

  const hidePopover = useMutation({
    mutationFn: async (): Promise<void> => {
      await invoke<unknown>("hide_tray_window");
    },
  });

  return {
    startSession,
    pauseSession,
    resumeSession,
    stopSession,
    toggleDeveloperMode,
    saveConfig,
    requestPermission,
    testNudge,
    hidePopover,
  };
}
