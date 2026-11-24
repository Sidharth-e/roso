import { useEffect, useState, type FormEvent } from "react";
import {
  useAppConfigQuery,
  useEngineEventSync,
  useEngineSnapshotQuery,
  useFocusHistoryQuery,
  useFocusMutations,
} from "../api";
import { useFocusUiStore } from "../store";
import type { AppConfig, TimelineSegment } from "../types";
import { Button, Card, Input, StatusBadge, Switch, TabBar } from "./UiPrimitives";

const DURATION_PRESETS: ReadonlyArray<number> = [15, 25, 45, 60, 90];

function formatClock(totalSeconds: number): string {
  const safe = Math.max(0, Math.ceil(totalSeconds));
  const mins = Math.floor(safe / 60);
  const secs = safe % 60;
  return `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
}

function statusBadgeProps(choice: string): {
  tone: "success" | "warning" | "neutral" | "info";
  label: string;
} {
  if (choice === "on_task") {
    return { tone: "success", label: "On Task" };
  }
  if (choice === "off_task") {
    return { tone: "warning", label: "Off Task" };
  }
  if (choice === "uncertain") {
    return { tone: "neutral", label: "Uncertain" };
  }
  return { tone: "info", label: "Ready" };
}

function segmentFillClass(status: string): string {
  if (status === "on_task") {
    return "fill-success";
  }
  if (status === "off_task") {
    return "fill-warning";
  }
  return "fill-tertiary";
}

function TimelineBar({ segments }: { segments: ReadonlyArray<TimelineSegment> }) {
  if (segments.length === 0) {
    return (
      <div className="flex h-3 w-full items-center justify-center rounded-full bg-background/80">
        <span className="sr-only">No classifications recorded yet</span>
      </div>
    );
  }

  const total = segments.length;
  const stepWidth = 100 / total;

  return (
    <svg
      viewBox="0 0 100 8"
      preserveAspectRatio="none"
      role="img"
      aria-label="Live session activity timeline"
      className="h-3 w-full overflow-hidden rounded-full bg-background/80"
    >
      {segments.map((seg, idx) => (
        <rect
          key={`${seg.timestamp}-${idx}`}
          x={idx * stepWidth}
          y={0}
          width={Math.max(stepWidth - 0.35, 0.4)}
          height={8}
          className={segmentFillClass(seg.status)}
        />
      ))}
    </svg>
  );
}

function MeterBar({
  value,
  max,
  tone,
  label,
}: {
  value: number;
  max: number;
  tone: "primary" | "warning";
  label: string;
}) {
  const pct = max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0;
  const fillClass = tone === "warning" ? "fill-warning" : "fill-primary";

  return (
    <svg
      viewBox="0 0 100 6"
      preserveAspectRatio="none"
      role="progressbar"
      aria-label={label}
      aria-valuenow={Math.round(pct)}
      aria-valuemin={0}
      aria-valuemax={100}
      className="h-1.5 w-full overflow-hidden rounded-full bg-background/80"
    >
      <rect x={0} y={0} width={pct} height={6} className={fillClass} />
    </svg>
  );
}

export function TrayControl() {
  useEngineEventSync();

  const activeTab = useFocusUiStore((s) => s.activeTab);
  const setActiveTab = useFocusUiStore((s) => s.setActiveTab);
  const theme = useFocusUiStore((s) => s.theme);
  const toggleTheme = useFocusUiStore((s) => s.toggleTheme);
  const draftTask = useFocusUiStore((s) => s.draftTask);
  const setDraftTask = useFocusUiStore((s) => s.setDraftTask);
  const draftDurationMinutes = useFocusUiStore((s) => s.draftDurationMinutes);
  const setDraftDurationMinutes = useFocusUiStore((s) => s.setDraftDurationMinutes);

  const snapshotQuery = useEngineSnapshotQuery();
  const configQuery = useAppConfigQuery();
  const historyQuery = useFocusHistoryQuery();
  const mutations = useFocusMutations();

  const [configDraft, setConfigDraft] = useState<AppConfig | null>(null);

  useEffect(() => {
    if (configQuery.data && configDraft === null) {
      setConfigDraft(configQuery.data);
      setDraftDurationMinutes(configQuery.data.defaultDurationMinutes);
    }
  }, [configQuery.data, configDraft, setDraftDurationMinutes]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        mutations.hidePopover.mutate();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [mutations.hidePopover]);

  const snapshot = snapshotQuery.data;
  const isIdle = !snapshot || snapshot.phase === "idle";
  const isRunning = snapshot?.phase === "running";
  const isPaused = snapshot?.phase === "paused";
  const badge = statusBadgeProps(snapshot?.lastChoice ?? "idle");

  function handleStart(event: FormEvent) {
    event.preventDefault();
    if (draftTask.trim().length === 0) {
      return;
    }
    mutations.startSession.mutate({
      declaredTask: draftTask.trim(),
      durationMinutes: draftDurationMinutes,
    });
  }

  function handleSaveSettings(event: FormEvent) {
    event.preventDefault();
    if (!configDraft) {
      return;
    }
    mutations.saveConfig.mutate(configDraft);
  }

  return (
    <div className="popover-shell p-3.5">
      <header className="mb-3 flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <span className="flex h-6 w-6 items-center justify-center rounded-lg bg-primary/15 text-xs font-bold text-primary">
            FC
          </span>
          <div>
            <h1 className="text-sm font-bold leading-tight text-foreground">
              Focus Companion
            </h1>
            <p className="text-[11px] text-tertiary">
              Clef + DeepSeek Cloud Companion
            </p>
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <StatusBadge tone={badge.tone} label={badge.label} />
          <button
            type="button"
            onClick={toggleTheme}
            aria-label="Toggle color theme"
            className="rounded-lg border border-tertiary/30 bg-secondary px-2 py-1 text-[11px] font-semibold text-foreground hover:bg-secondary/80 focus:outline-none focus:ring-2 focus:ring-primary/40"
          >
            {theme === "dark" ? "Light" : "Dark"}
          </button>
          <button
            type="button"
            onClick={() => mutations.hidePopover.mutate()}
            aria-label="Close popover"
            className="rounded-lg border border-tertiary/30 bg-secondary px-2 py-1 text-[11px] font-semibold text-tertiary hover:text-foreground focus:outline-none focus:ring-2 focus:ring-primary/40"
          >
            ✕
          </button>
        </div>
      </header>

      <TabBar activeTab={activeTab} onSelect={setActiveTab} />

      <div className="mt-3 flex-1 space-y-3 overflow-y-auto pr-0.5">
        {activeTab === "focus" ? (
          <>
            {snapshot && !snapshot.screenPermissionGranted ? (
              <Card className="border-warning/40 bg-warning/10">
                <div className="flex items-center justify-between gap-2">
                  <div>
                    <p className="text-xs font-semibold text-warning">
                      Screen Recording Needed
                    </p>
                    <p className="text-[11px] text-foreground/80">
                      Grant macOS Screen Recording so Vision OCR &amp; Clef can run in RAM.
                    </p>
                  </div>
                  <Button
                    variant="secondary"
                    onClick={() => mutations.requestPermission.mutate()}
                  >
                    Grant
                  </Button>
                </div>
              </Card>
            ) : null}

            {isIdle ? (
              <form onSubmit={handleStart} className="space-y-3">
                <Card className="space-y-3">
                  <Input
                    id="declared-task-input"
                    label="What are you focusing on?"
                    placeholder="e.g., Implementing Rust Vision OCR pipeline"
                    maxLength={240}
                    value={draftTask}
                    onChange={(e) => setDraftTask(e.target.value)}
                  />

                  <div className="space-y-1.5">
                    <div className="flex items-center justify-between">
                      <label
                        htmlFor="duration-minutes-input"
                        className="text-xs font-medium text-foreground/80"
                      >
                        Duration (1–240 mins)
                      </label>
                      <span className="text-xs font-bold text-primary">
                        {draftDurationMinutes} min
                      </span>
                    </div>
                    <input
                      id="duration-minutes-input"
                      type="range"
                      min={1}
                      max={240}
                      value={draftDurationMinutes}
                      onChange={(e) =>
                        setDraftDurationMinutes(Number(e.target.value))
                      }
                      className="w-full accent-primary"
                    />
                    <div className="grid grid-cols-5 gap-1 pt-0.5">
                      {DURATION_PRESETS.map((preset) => (
                        <button
                          key={preset}
                          type="button"
                          onClick={() => setDraftDurationMinutes(preset)}
                          className={`rounded-md py-1 text-[11px] font-semibold transition ${
                            draftDurationMinutes === preset
                              ? "bg-primary text-background"
                              : "bg-background text-tertiary hover:text-foreground"
                          }`}
                        >
                          {preset}m
                        </button>
                      ))}
                    </div>
                  </div>

                  <Button
                    type="submit"
                    className="w-full"
                    disabled={
                      draftTask.trim().length === 0 ||
                      mutations.startSession.isPending
                    }
                  >
                    Start Focus Session
                  </Button>
                </Card>
              </form>
            ) : (
              <Card className="space-y-3">
                <div className="flex items-start justify-between gap-2">
                  <div className="min-w-0">
                    <p className="text-[11px] uppercase tracking-wider text-tertiary">
                      Active Task
                    </p>
                    <p className="truncate text-xs font-semibold text-foreground">
                      {snapshot.declaredTask}
                    </p>
                  </div>
                  {snapshot.developerMode ? (
                    <StatusBadge tone="info" label="4x Dev" />
                  ) : null}
                </div>

                <div className="flex items-baseline justify-between">
                  <span className="text-3xl font-bold tabular-nums tracking-tight text-foreground">
                    {formatClock(snapshot.remainingFocusSeconds)}
                  </span>
                  <span className="text-[11px] text-tertiary">
                    Checks: {snapshot.checkCount} · Nudges: {snapshot.nudgeCount}
                  </span>
                </div>

                <MeterBar
                  value={snapshot.elapsedFocusSeconds}
                  max={snapshot.durationMinutes * 60}
                  tone="primary"
                  label="Session progress"
                />

                <div className="grid grid-cols-2 gap-2 pt-1">
                  {isRunning ? (
                    <Button
                      variant="secondary"
                      onClick={() => mutations.pauseSession.mutate()}
                    >
                      Pause
                    </Button>
                  ) : isPaused ? (
                    <Button
                      variant="primary"
                      onClick={() => mutations.resumeSession.mutate()}
                    >
                      Resume
                    </Button>
                  ) : null}
                  <Button
                    variant="danger"
                    onClick={() => mutations.stopSession.mutate()}
                  >
                    End Session
                  </Button>
                </div>
              </Card>
            )}

            <Card className="space-y-2.5">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold text-foreground">
                  Session Timeline
                </span>
                <span className="text-[11px] text-tertiary">
                  p(off_task): {Math.round((snapshot?.lastProbOffTask ?? 0) * 100)}%
                </span>
              </div>
              <TimelineBar segments={snapshot?.timeline ?? []} />
              <div className="flex items-center justify-between text-[10px] text-tertiary">
                <span className="inline-flex items-center gap-1">
                  <span className="h-2 w-2 rounded-full bg-success" /> On-task
                </span>
                <span className="inline-flex items-center gap-1">
                  <span className="h-2 w-2 rounded-full bg-warning" /> Off-task
                </span>
                <span className="inline-flex items-center gap-1">
                  <span className="h-2 w-2 rounded-full bg-tertiary" /> Uncertain
                </span>
              </div>

              <div className="space-y-1 pt-1">
                <div className="flex items-center justify-between text-[11px] text-tertiary">
                  <span>Off-task Evidence Span</span>
                  <span>
                    {Math.round(snapshot?.evidenceFocusSeconds ?? 0)}s /{" "}
                    {Math.round(snapshot?.requiredEvidenceFocusSeconds ?? 120)}s
                  </span>
                </div>
                <MeterBar
                  value={snapshot?.evidenceFocusSeconds ?? 0}
                  max={snapshot?.requiredEvidenceFocusSeconds ?? 120}
                  tone="warning"
                  label="Off-task evidence accumulation"
                />
              </div>

              {snapshot?.lastError ? (
                <p className="rounded bg-error/15 px-2 py-1 text-[11px] text-error">
                  {snapshot.lastError}
                </p>
              ) : null}
            </Card>

            <Card className="space-y-3">
              <Switch
                id="developer-mode-switch"
                checked={snapshot?.developerMode ?? false}
                onChange={(enabled) =>
                  mutations.toggleDeveloperMode.mutate(enabled)
                }
                label="Developer Mode"
                description="4x focus clock speed & 10s check interval"
              />

              <Button
                variant="secondary"
                className="w-full"
                onClick={() => mutations.testNudge.mutate()}
                disabled={mutations.testNudge.isPending}
              >
                Test Nudge Overlay
              </Button>
            </Card>
          </>
        ) : null}

        {activeTab === "history" ? (
          <div className="space-y-2">
            {(historyQuery.data ?? []).length === 0 ? (
              <Card>
                <p className="text-center text-xs text-tertiary">
                  Completed focus sessions will appear here. Screenshots and OCR text are never stored.
                </p>
              </Card>
            ) : (
              (historyQuery.data ?? []).map((item) => (
                <Card key={item.sessionId} className="space-y-1.5">
                  <div className="flex items-center justify-between gap-2">
                    <p className="truncate text-xs font-semibold text-foreground">
                      {item.declaredTask}
                    </p>
                    <span className="shrink-0 text-[10px] font-medium text-primary">
                      {item.plannedDurationMinutes}m
                    </span>
                  </div>
                  <div className="flex items-center justify-between text-[11px] text-tertiary">
                    <span>Checks: {item.checkCount}</span>
                    <span>Nudges: {item.nudgeTimestamps.length}</span>
                    <span>
                      {new Date(item.startedAt).toLocaleTimeString([], {
                        hour: "2-digit",
                        minute: "2-digit",
                      })}
                    </span>
                  </div>
                </Card>
              ))
            )}
          </div>
        ) : null}

        {activeTab === "settings" && configDraft ? (
          <form onSubmit={handleSaveSettings} className="space-y-3">
            <Card className="space-y-2.5">
              <Input
                id="ollama-base-url"
                label="Ollama Endpoint"
                value={configDraft.ollamaBaseUrl}
                onChange={(e) =>
                  setConfigDraft({
                    ...configDraft,
                    ollamaBaseUrl: e.target.value,
                  })
                }
              />
              <Input
                id="ollama-api-key"
                label="Ollama Cloud API Key"
                type="password"
                placeholder="Stored in Application Support (0600)"
                value={configDraft.ollamaApiKey}
                onChange={(e) =>
                  setConfigDraft({
                    ...configDraft,
                    ollamaApiKey: e.target.value,
                  })
                }
              />
              <Input
                id="classifier-model-input"
                label="System One Classifier Model (/v1/systemone)"
                placeholder="clef or clef-flash"
                value={configDraft.classifierModel}
                onChange={(e) =>
                  setConfigDraft({
                    ...configDraft,
                    classifierModel: e.target.value,
                  })
                }
              />
              <Input
                id="nudge-model-input"
                label="Nudge & Targeting Model (/api/chat)"
                placeholder="deepseek-v4.1-flash:cloud"
                value={configDraft.nudgeModel}
                onChange={(e) =>
                  setConfigDraft({
                    ...configDraft,
                    nudgeModel: e.target.value,
                  })
                }
              />
              <Button
                type="submit"
                className="w-full"
                disabled={mutations.saveConfig.isPending}
              >
                {mutations.saveConfig.isPending ? "Saving..." : "Save Configuration"}
              </Button>
              {mutations.saveConfig.isError ? (
                <p className="text-[11px] text-error">
                  {mutations.saveConfig.error.message}
                </p>
              ) : null}
              {mutations.saveConfig.isSuccess ? (
                <p className="text-[11px] text-success">
                  Configuration saved to FocusCompanion/config.json.
                </p>
              ) : null}
            </Card>
          </form>
        ) : null}
      </div>
    </div>
  );
}
