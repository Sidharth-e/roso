import { useEffect, useState, type FormEvent } from "react";
import {
  useAppConfigQuery,
  useEngineEventSync,
  useEngineSnapshotQuery,
  useTaskHistoryQuery,
  useTaskMutations,
} from "../api";
import { useTaskUiStore } from "../store";
import type { AppConfig, TaskPhase } from "../types";
import {
  Button,
  Card,
  Input,
  StatusBadge,
  Switch,
  TabBar,
  TextArea,
} from "./UiPrimitives";

const EXAMPLE_GOALS: ReadonlyArray<string> = [
  "Open Notes and create a checklist for today's sprint",
  "Open Safari to github.com/trending and scroll down",
  "Open Calculator and compute 42 * 18",
];

function phaseBadgeProps(phase: TaskPhase | undefined): {
  tone: "success" | "warning" | "error" | "neutral" | "info";
  label: string;
} {
  switch (phase) {
    case "planning":
      return { tone: "info", label: "Planning" };
    case "running":
      return { tone: "info", label: "Acting" };
    case "verifying":
      return { tone: "info", label: "Verifying" };
    case "recovering":
      return { tone: "warning", label: "Recovering" };
    case "awaiting_confirmation":
      return { tone: "warning", label: "Approve?" };
    case "paused":
      return { tone: "neutral", label: "Paused" };
    case "completed":
      return { tone: "success", label: "Done" };
    case "failed":
      return { tone: "error", label: "Failed" };
    case "stopped":
      return { tone: "warning", label: "Stopped" };
    default:
      return { tone: "neutral", label: "Ready" };
  }
}

function outcomeTone(
  outcome: string
): "success" | "warning" | "error" | "neutral" | "info" {
  if (outcome === "completed") {
    return "success";
  }
  if (outcome === "failed") {
    return "error";
  }
  if (outcome === "stopped" || outcome === "max_steps") {
    return "warning";
  }
  return "neutral";
}

function stepStatusClass(status: string): string {
  if (status === "verified" || status === "done") {
    return "text-success";
  }
  if (status === "failed" || status === "unverified_failed") {
    return "text-error";
  }
  if (status === "recovery" || status === "retrying") {
    return "text-warning";
  }
  return "text-tertiary";
}

function StepMeterBar({ value, max }: { value: number; max: number }) {
  const pct = max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0;

  return (
    <svg
      viewBox="0 0 100 6"
      preserveAspectRatio="none"
      role="progressbar"
      aria-label="Task step progress"
      aria-valuenow={Math.round(pct)}
      aria-valuemin={0}
      aria-valuemax={100}
      className="h-1.5 w-full overflow-hidden rounded-full bg-background/80"
    >
      <rect x={0} y={0} width={pct} height={6} className="fill-primary" />
    </svg>
  );
}

export function TrayControl() {
  useEngineEventSync();

  const activeTab = useTaskUiStore((s) => s.activeTab);
  const setActiveTab = useTaskUiStore((s) => s.setActiveTab);
  const theme = useTaskUiStore((s) => s.theme);
  const toggleTheme = useTaskUiStore((s) => s.toggleTheme);
  const draftGoal = useTaskUiStore((s) => s.draftGoal);
  const setDraftGoal = useTaskUiStore((s) => s.setDraftGoal);

  const snapshotQuery = useEngineSnapshotQuery();
  const configQuery = useAppConfigQuery();
  const historyQuery = useTaskHistoryQuery();
  const mutations = useTaskMutations();

  const [configDraft, setConfigDraft] = useState<AppConfig | null>(null);

  useEffect(() => {
    if (configQuery.data && configDraft === null) {
      setConfigDraft(configQuery.data);
    }
  }, [configQuery.data, configDraft]);

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
  const phase = snapshot?.phase ?? "idle";
  const isActive =
    phase === "planning" ||
    phase === "running" ||
    phase === "verifying" ||
    phase === "recovering" ||
    phase === "paused" ||
    phase === "awaiting_confirmation";
  const badge = phaseBadgeProps(snapshot?.phase);

  function handleRunTask(event: FormEvent) {
    event.preventDefault();
    const clean = draftGoal.trim();
    if (clean.length === 0) {
      return;
    }
    mutations.startTask.mutate({ goal: clean });
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
            TC
          </span>
          <div>
            <h1 className="text-sm font-bold leading-tight text-foreground">
              Task Companion
            </h1>
            <p className="text-[11px] text-tertiary">
              Ollama Computer-Use Agent
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
        {activeTab === "task" ? (
          <>
            {snapshot && !snapshot.screenPermissionGranted ? (
              <Card className="border-warning/40 bg-warning/10">
                <div className="flex items-center justify-between gap-2">
                  <div>
                    <p className="text-xs font-semibold text-warning">
                      Screen Recording Needed
                    </p>
                    <p className="text-[11px] text-foreground/80">
                      Grant macOS Screen Recording so the agent can inspect your screen in RAM.
                    </p>
                  </div>
                  <Button
                    variant="secondary"
                    onClick={() => mutations.requestScreenPermission.mutate()}
                  >
                    Grant
                  </Button>
                </div>
              </Card>
            ) : null}

            {snapshot && !snapshot.accessibilityPermissionGranted ? (
              <Card className="border-warning/40 bg-warning/10">
                <div className="flex items-center justify-between gap-2">
                  <div>
                    <p className="text-xs font-semibold text-warning">
                      Accessibility Needed
                    </p>
                    <p className="text-[11px] text-foreground/80">
                      Grant macOS Accessibility so the agent can click, type, and scroll.
                    </p>
                  </div>
                  <Button
                    variant="secondary"
                    onClick={() =>
                      mutations.requestAccessibilityPermission.mutate()
                    }
                  >
                    Grant
                  </Button>
                </div>
              </Card>
            ) : null}

            {!isActive ? (
              <form onSubmit={handleRunTask} className="space-y-3">
                <Card className="space-y-2.5">
                  <TextArea
                    id="task-goal-input"
                    label="What should I do on your Mac?"
                    placeholder="e.g., Open Notes, create a new note, and write a sprint checklist"
                    rows={3}
                    maxLength={500}
                    value={draftGoal}
                    onChange={(e) => setDraftGoal(e.target.value)}
                  />

                  <div className="flex flex-wrap gap-1.5">
                    {EXAMPLE_GOALS.map((sample) => (
                      <button
                        key={sample}
                        type="button"
                        onClick={() => setDraftGoal(sample)}
                        className="rounded-md border border-tertiary/25 bg-background px-2 py-1 text-left text-[10px] font-medium text-tertiary transition hover:border-primary/40 hover:text-foreground"
                      >
                        {sample}
                      </button>
                    ))}
                  </div>

                  <Button
                    type="submit"
                    className="w-full"
                    disabled={
                      draftGoal.trim().length === 0 ||
                      mutations.startTask.isPending
                    }
                  >
                    {mutations.startTask.isPending
                      ? "Starting..."
                      : "Run Task"}
                  </Button>
                </Card>

                {snapshot &&
                (phase === "completed" ||
                  phase === "failed" ||
                  phase === "stopped") &&
                snapshot.lastSummary ? (
                  <Card className="space-y-1.5">
                    <div className="flex items-center justify-between">
                      <span className="text-[11px] font-semibold uppercase tracking-wider text-tertiary">
                        Last Run
                      </span>
                      <StatusBadge tone={badge.tone} label={badge.label} />
                    </div>
                    <p className="text-xs font-medium text-foreground">
                      {snapshot.lastSummary}
                    </p>
                    {snapshot.lastError ? (
                      <p className="rounded bg-error/15 px-2 py-1 text-[11px] text-error">
                        {snapshot.lastError}
                      </p>
                    ) : null}
                  </Card>
                ) : null}
              </form>
            ) : (
              <div className="space-y-3">
                <Card className="space-y-2.5">
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <p className="text-[10px] font-semibold uppercase tracking-wider text-tertiary">
                        Active Goal
                      </p>
                      <p className="text-xs font-semibold text-foreground">
                        {snapshot?.goal}
                      </p>
                    </div>
                    <span className="shrink-0 rounded border border-tertiary/30 bg-background px-1.5 py-0.5 text-[10px] font-semibold text-tertiary">
                      ⌃⌥S Kill
                    </span>
                  </div>

                  <div className="flex items-center justify-between text-[11px] text-tertiary">
                    <span>
                      Step {snapshot?.currentStep ?? 0} /{" "}
                      {snapshot?.maxSteps ?? 20}
                    </span>
                    <span className="truncate max-w-[170px]">
                      {snapshot?.activeApp || "Detecting..."}
                    </span>
                  </div>

                  <StepMeterBar
                    value={snapshot?.currentStep ?? 0}
                    max={snapshot?.maxSteps ?? 20}
                  />

                  <p className="rounded-lg bg-background/80 px-2.5 py-2 text-xs text-foreground">
                    {snapshot?.lastNarration}
                  </p>

                  <div className="grid grid-cols-2 gap-2 pt-0.5">
                    {phase === "running" ? (
                      <Button
                        variant="secondary"
                        onClick={() => mutations.pauseTask.mutate()}
                      >
                        Pause
                      </Button>
                    ) : phase === "paused" ? (
                      <Button
                        variant="primary"
                        onClick={() => mutations.resumeTask.mutate()}
                      >
                        Resume
                      </Button>
                    ) : (
                      <Button variant="secondary" disabled>
                        {phase === "planning" ? "Planning..." : "Waiting..."}
                      </Button>
                    )}
                    <Button
                      variant="danger"
                      onClick={() => mutations.stopTask.mutate()}
                    >
                      Stop (⌃⌥S)
                    </Button>
                  </div>
                </Card>

                {snapshot?.pendingConfirmation ? (
                  <Card className="space-y-2.5 border-warning/50 bg-warning/10">
                    <div className="flex items-center justify-between gap-2">
                      <span className="text-xs font-bold text-warning">
                        Confirm Step #{snapshot.pendingConfirmation.stepNumber}
                      </span>
                      <span className="rounded bg-warning/20 px-1.5 py-0.5 text-[10px] font-semibold text-warning">
                        {snapshot.pendingConfirmation.actionType}
                      </span>
                    </div>
                    <p className="text-xs font-semibold text-foreground">
                      {snapshot.pendingConfirmation.description}
                    </p>
                    <p className="text-[11px] text-foreground/80">
                      {snapshot.pendingConfirmation.reason}
                    </p>
                    <div className="grid grid-cols-2 gap-2">
                      <Button
                        variant="primary"
                        onClick={() =>
                          mutations.resolveConfirmation.mutate({
                            confirmationId: snapshot.pendingConfirmation!.id,
                            approved: true,
                          })
                        }
                      >
                        Approve
                      </Button>
                      <Button
                        variant="danger"
                        onClick={() =>
                          mutations.resolveConfirmation.mutate({
                            confirmationId: snapshot.pendingConfirmation!.id,
                            approved: false,
                          })
                        }
                      >
                        Deny
                      </Button>
                    </div>
                  </Card>
                ) : null}

                {(snapshot?.planSteps.length ?? 0) > 0 ? (
                  <Card className="space-y-2">
                    <span className="text-xs font-semibold text-foreground">
                      Orchestrated Plan
                    </span>
                    <ol className="space-y-1">
                      {snapshot?.planSteps.map((stepText, idx) => {
                        const activeIdx = snapshot.activePlanIndex;
                        const done = idx < activeIdx;
                        const current = idx === activeIdx;
                        return (
                          <li
                            key={`${idx}-${stepText}`}
                            className={`flex items-start gap-2 rounded-md px-2 py-1 text-[11px] ${
                              current
                                ? "bg-primary/15 font-semibold text-foreground"
                                : done
                                  ? "text-tertiary line-through"
                                  : "text-foreground/80"
                            }`}
                          >
                            <span className="shrink-0 font-mono text-[10px] text-primary">
                              {done ? "✓" : `${idx + 1}.`}
                            </span>
                            <span>{stepText}</span>
                          </li>
                        );
                      })}
                    </ol>
                  </Card>
                ) : null}

                {snapshot?.lastDiagnosis ? (
                  <Card className="space-y-1 border-warning/40 bg-warning/10">
                    <p className="text-[11px] font-semibold text-warning">
                      Recovery #{snapshot.recoveryCount}
                    </p>
                    <p className="text-[11px] text-foreground/85">
                      {snapshot.lastDiagnosis}
                    </p>
                  </Card>
                ) : null}
              </div>
            )}

            {(snapshot?.steps.length ?? 0) > 0 ? (
              <Card className="space-y-2">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-semibold text-foreground">
                    Live Action Feed
                  </span>
                  <span className="text-[10px] text-tertiary">
                    {snapshot?.steps.length} actions
                  </span>
                </div>
                <div className="max-h-40 space-y-1.5 overflow-y-auto pr-0.5">
                  {[...(snapshot?.steps ?? [])].reverse().map((step, idx) => (
                    <div
                      key={`${step.stepNumber}-${step.actionType}-${idx}-${step.timestamp}`}
                      className="rounded-lg border border-tertiary/20 bg-background/70 px-2.5 py-1.5"
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="text-[10px] font-bold uppercase tracking-wider text-primary">
                          #{step.stepNumber} · {step.actionType}
                        </span>
                        <span
                          className={`text-[10px] font-medium ${stepStatusClass(step.status)}`}
                        >
                          {step.status}
                        </span>
                      </div>
                      <p className="text-[11px] font-medium text-foreground">
                        {step.description}
                      </p>
                      <p className="text-[10px] text-tertiary">
                        {step.narration}
                      </p>
                      {step.verificationNote ? (
                        <p className="mt-0.5 text-[10px] text-foreground/75">
                          Check: {step.verificationNote}
                        </p>
                      ) : null}
                    </div>
                  ))}
                </div>
              </Card>
            ) : null}

            <Card>
              <Button
                variant="secondary"
                className="w-full"
                onClick={() => mutations.testPointer.mutate()}
                disabled={mutations.testPointer.isPending}
              >
                Test Pointer Overlay
              </Button>
            </Card>
          </>
        ) : null}

        {activeTab === "history" ? (
          <div className="space-y-2">
            {(historyQuery.data ?? []).length === 0 ? (
              <Card>
                <p className="text-center text-xs text-tertiary">
                  Completed task runs will appear here. Screenshots and OCR text are never stored.
                </p>
              </Card>
            ) : (
              (historyQuery.data ?? []).map((item) => (
                <Card key={item.taskId} className="space-y-1.5">
                  <div className="flex items-start justify-between gap-2">
                    <p className="text-xs font-semibold text-foreground">
                      {item.goal}
                    </p>
                    <StatusBadge
                      tone={outcomeTone(item.outcome)}
                      label={item.outcome}
                    />
                  </div>
                  <p className="text-[11px] text-foreground/80">
                    {item.summary}
                  </p>
                  <div className="flex items-center justify-between text-[10px] text-tertiary">
                    <span>Steps: {item.stepCount}</span>
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
                id="vision-model-input"
                label="Vision Agent Model (/api/chat)"
                placeholder="deepseek-v4.1-flash:cloud"
                value={configDraft.visionModel}
                onChange={(e) =>
                  setConfigDraft({
                    ...configDraft,
                    visionModel: e.target.value,
                  })
                }
              />
              <div className="grid grid-cols-2 gap-2">
                <Input
                  id="max-steps-input"
                  label="Max Steps (1–50)"
                  type="number"
                  min={1}
                  max={50}
                  value={configDraft.maxSteps}
                  onChange={(e) =>
                    setConfigDraft({
                      ...configDraft,
                      maxSteps: Number(e.target.value) || 20,
                    })
                  }
                />
                <Input
                  id="step-delay-input"
                  label="Step Delay (ms)"
                  type="number"
                  min={50}
                  max={5000}
                  step={50}
                  value={configDraft.stepDelayMs}
                  onChange={(e) =>
                    setConfigDraft({
                      ...configDraft,
                      stepDelayMs: Number(e.target.value) || 250,
                    })
                  }
                />
              </div>
              <Switch
                id="confirm-risky-switch"
                checked={configDraft.confirmRiskyActions}
                onChange={(checked) =>
                  setConfigDraft({
                    ...configDraft,
                    confirmRiskyActions: checked,
                  })
                }
                label="Confirm Risky Actions"
                description="Ask before deleting, sending, submitting, or Cmd+Q"
              />
              <Button
                type="submit"
                className="w-full"
                disabled={mutations.saveConfig.isPending}
              >
                {mutations.saveConfig.isPending
                  ? "Saving..."
                  : "Save Configuration"}
              </Button>
              {mutations.saveConfig.isError ? (
                <p className="text-[11px] text-error">
                  {mutations.saveConfig.error.message}
                </p>
              ) : null}
              {mutations.saveConfig.isSuccess ? (
                <p className="text-[11px] text-success">
                  Configuration saved to TaskCompanion/config.json.
                </p>
              ) : null}
            </Card>
          </form>
        ) : null}
      </div>
    </div>
  );
}
