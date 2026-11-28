import { z } from "zod";

export const CLOUD_OLLAMA_BASE_URL = "https://ollama.com";
export const LOCAL_OLLAMA_BASE_URL = "http://localhost:11434";

export const TaskPhaseSchema = z.enum([
  "idle",
  "classifying",
  "planning",
  "running",
  "verifying",
  "recovering",
  "awaiting_confirmation",
  "paused",
  "completed",
  "failed",
  "stopped",
]);
export type TaskPhase = z.infer<typeof TaskPhaseSchema>;

export function isActivePhase(phase: TaskPhase): boolean {
  return (
    phase === "classifying" ||
    phase === "planning" ||
    phase === "running" ||
    phase === "verifying" ||
    phase === "recovering" ||
    phase === "paused" ||
    phase === "awaiting_confirmation"
  );
}

export const TaskClassificationSchema = z.object({
  category: z.string(),
  complexity: z.string(),
  recommendedRole: z.string(),
  reasoning: z.string(),
  needsDedicatedPlanner: z.boolean(),
  initialPlanSteps: z.array(z.string()),
});
export type TaskClassification = z.infer<typeof TaskClassificationSchema>;

export const OrchestrationTraceEntrySchema = z.object({
  id: z.string(),
  timestamp: z.string(),
  stage: z.string(),
  role: z.string(),
  model: z.string(),
  title: z.string(),
  content: z.string(),
  stepNumber: z.number().int().nonnegative().nullable(),
  status: z.string(),
});
export type OrchestrationTraceEntry = z.infer<
  typeof OrchestrationTraceEntrySchema
>;

export const TaskStepRecordSchema = z.object({
  stepNumber: z.number().int().nonnegative(),
  timestamp: z.string(),
  actionType: z.string(),
  description: z.string(),
  narration: z.string(),
  status: z.string(),
  targetOcrId: z.number().int().nullable(),
  targetX: z.number().nullable(),
  targetY: z.number().nullable(),
  verificationNote: z.string().nullable().optional(),
  model: z.string().nullable().optional(),
  thought: z.string().nullable().optional(),
});
export type TaskStepRecord = z.infer<typeof TaskStepRecordSchema>;

export const PendingConfirmationSchema = z.object({
  id: z.string(),
  stepNumber: z.number().int().positive(),
  actionType: z.string(),
  description: z.string(),
  narration: z.string(),
  reason: z.string(),
  fullPayload: z.string().nullable().optional(),
  targetApp: z.string().nullable().optional(),
  targetUrl: z.string().nullable().optional(),
  expiresAt: z.string().nullable().optional(),
});
export type PendingConfirmation = z.infer<typeof PendingConfirmationSchema>;

export const EngineSnapshotSchema = z.object({
  phase: TaskPhaseSchema,
  taskId: z.string().nullable(),
  goal: z.string(),
  startedAt: z.string().nullable().optional(),
  classification: TaskClassificationSchema.nullable(),
  activeRole: z.string(),
  activeModel: z.string(),
  currentThought: z.string(),
  planSteps: z.array(z.string()),
  activePlanIndex: z.number().int().nonnegative(),
  currentStep: z.number().int().nonnegative(),
  maxSteps: z.number().int().positive(),
  recoveryCount: z.number().int().nonnegative(),
  lastDiagnosis: z.string().nullable(),
  activeApp: z.string(),
  lastNarration: z.string(),
  lastSummary: z.string().nullable(),
  lastError: z.string().nullable(),
  pendingConfirmation: PendingConfirmationSchema.nullable(),
  screenPermissionGranted: z.boolean(),
  accessibilityPermissionGranted: z.boolean(),
  steps: z.array(TaskStepRecordSchema),
  traces: z.array(OrchestrationTraceEntrySchema),
});
export type EngineSnapshot = z.infer<typeof EngineSnapshotSchema>;

export const ModelRoutingConfigSchema = z.object({
  judgeModel: z.string(),
  plannerModel: z.string(),
  visionActorModel: z.string(),
  codingActorModel: z.string(),
  writingActorModel: z.string(),
  recoveryModel: z.string(),
  enableJudgeVerification: z.boolean(),
  autoOpenPanel: z.boolean(),
});
export type ModelRoutingConfig = z.infer<typeof ModelRoutingConfigSchema>;

export const DEFAULT_MODEL_ROUTING: ModelRoutingConfig = {
  judgeModel: "gemma4:31b-cloud",
  plannerModel: "kimi-k3.1-pro:cloud",
  visionActorModel: "deepseek-v4.1-flash:cloud",
  codingActorModel: "qwen3-vl:235b-cloud",
  writingActorModel: "kimi-k3.1-pro:cloud",
  recoveryModel: "qwen3-vl:235b-cloud",
  enableJudgeVerification: true,
  autoOpenPanel: true,
};

export const AppConfigSchema = z.object({
  ollamaBaseUrl: z.string(),
  hasApiKey: z.boolean(),
  apiKeyPreview: z.string(),
  cloudConsentAccepted: z.boolean(),
  models: ModelRoutingConfigSchema,
});
export type AppConfig = z.infer<typeof AppConfigSchema>;

export interface UpdateAppConfigPayload {
  ollamaBaseUrl: string;
  ollamaApiKey: string;
  clearApiKey: boolean;
  cloudConsentAccepted: boolean;
  models: ModelRoutingConfig;
}

export const TaskHistoryRecordSchema = z.object({
  taskId: z.string(),
  goal: z.string(),
  startedAt: z.string(),
  endedAt: z.string(),
  outcome: z.string(),
  summary: z.string(),
  stepCount: z.number().int().nonnegative(),
  steps: z.array(TaskStepRecordSchema),
  category: z.string().nullable().optional(),
  selectedModel: z.string().nullable().optional(),
});
export type TaskHistoryRecord = z.infer<typeof TaskHistoryRecordSchema>;

export const PointerCuePayloadSchema = z.object({
  id: z.string(),
  message: z.string(),
  actionBadge: z.string(),
  targetX: z.number(),
  targetY: z.number(),
  targetWidth: z.number().nullable(),
  targetHeight: z.number().nullable(),
  targetOcrId: z.number().int().nullable(),
  stepNumber: z.number().int().nonnegative(),
  isActing: z.boolean(),
  showPointer: z.boolean(),
  dismissAfterMs: z.number().int().positive(),
});
export type PointerCuePayload = z.infer<typeof PointerCuePayloadSchema>;

export type TrayTab = "task" | "live" | "history" | "settings";
export type PanelFilter = "all" | "plan" | "think" | "steps";
export type ThemeMode = "light" | "dark";
