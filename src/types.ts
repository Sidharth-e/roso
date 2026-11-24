import { z } from "zod";

export const TaskPhaseSchema = z.enum([
  "idle",
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
});
export type TaskStepRecord = z.infer<typeof TaskStepRecordSchema>;

export const PendingConfirmationSchema = z.object({
  id: z.string(),
  stepNumber: z.number().int().positive(),
  actionType: z.string(),
  description: z.string(),
  narration: z.string(),
  reason: z.string(),
});
export type PendingConfirmation = z.infer<typeof PendingConfirmationSchema>;

export const EngineSnapshotSchema = z.object({
  phase: TaskPhaseSchema,
  taskId: z.string().nullable(),
  goal: z.string(),
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
});
export type EngineSnapshot = z.infer<typeof EngineSnapshotSchema>;

export const AppConfigSchema = z.object({
  ollamaBaseUrl: z.string().min(1),
  ollamaApiKey: z.string(),
  visionModel: z.string().min(1),
  maxSteps: z.number().int().min(1).max(50),
  stepDelayMs: z.number().int().min(200).max(5000),
  confirmRiskyActions: z.boolean(),
});
export type AppConfig = z.infer<typeof AppConfigSchema>;

export const TaskHistoryRecordSchema = z.object({
  taskId: z.string(),
  goal: z.string(),
  startedAt: z.string(),
  endedAt: z.string(),
  outcome: z.string(),
  summary: z.string(),
  stepCount: z.number().int().nonnegative(),
  steps: z.array(TaskStepRecordSchema),
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

export type TrayTab = "task" | "history" | "settings";
export type ThemeMode = "light" | "dark";
