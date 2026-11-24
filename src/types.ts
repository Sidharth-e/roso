import { z } from "zod";

export const SessionPhaseSchema = z.enum(["idle", "running", "paused"]);
export type SessionPhase = z.infer<typeof SessionPhaseSchema>;

export const TimelineSegmentSchema = z.object({
  timestamp: z.string(),
  status: z.string(),
  probOffTask: z.number(),
  confidence: z.number(),
});
export type TimelineSegment = z.infer<typeof TimelineSegmentSchema>;

export const EngineSnapshotSchema = z.object({
  phase: SessionPhaseSchema,
  sessionId: z.string().nullable(),
  declaredTask: z.string(),
  durationMinutes: z.number().int().min(1).max(240),
  remainingFocusSeconds: z.number().nonnegative(),
  elapsedFocusSeconds: z.number().nonnegative(),
  developerMode: z.boolean(),
  evidenceFocusSeconds: z.number().nonnegative(),
  requiredEvidenceFocusSeconds: z.number().positive(),
  cooldownRemainingRealSeconds: z.number().nonnegative(),
  lastChoice: z.string(),
  lastProbOffTask: z.number().min(0).max(1),
  lastConfidence: z.number().min(0).max(1),
  activeApp: z.string(),
  checkCount: z.number().int().nonnegative(),
  nudgeCount: z.number().int().nonnegative(),
  screenPermissionGranted: z.boolean(),
  lastError: z.string().nullable(),
  timeline: z.array(TimelineSegmentSchema),
});
export type EngineSnapshot = z.infer<typeof EngineSnapshotSchema>;

export const AppConfigSchema = z.object({
  ollamaBaseUrl: z.string().min(1),
  ollamaApiKey: z.string(),
  classifierModel: z.string().min(1),
  nudgeModel: z.string().min(1),
  developerMode: z.boolean(),
  defaultDurationMinutes: z.number().int().min(1).max(240),
});
export type AppConfig = z.infer<typeof AppConfigSchema>;

export const ProbabilityLogEntrySchema = z.object({
  timestamp: z.string(),
  choice: z.string(),
  probOffTask: z.number(),
  confidence: z.number(),
  evidenceFocusSeconds: z.number(),
});
export type ProbabilityLogEntry = z.infer<typeof ProbabilityLogEntrySchema>;

export const SessionHistoryRecordSchema = z.object({
  sessionId: z.string(),
  declaredTask: z.string(),
  startedAt: z.string(),
  endedAt: z.string(),
  plannedDurationMinutes: z.number().int(),
  developerMode: z.boolean(),
  checkCount: z.number().int(),
  probabilityLogs: z.array(ProbabilityLogEntrySchema),
  nudgeTimestamps: z.array(z.string()),
});
export type SessionHistoryRecord = z.infer<typeof SessionHistoryRecordSchema>;

export const NudgeEventPayloadSchema = z.object({
  id: z.string(),
  nudgeText: z.string(),
  targetX: z.number(),
  targetY: z.number(),
  targetOcrId: z.number().int().nullable(),
  activeApp: z.string(),
  dismissAfterMs: z.number().int().positive(),
});
export type NudgeEventPayload = z.infer<typeof NudgeEventPayloadSchema>;

export type TrayTab = "focus" | "history" | "settings";
export type ThemeMode = "light" | "dark";
