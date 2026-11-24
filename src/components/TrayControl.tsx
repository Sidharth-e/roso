import { useEffect, useState, type FormEvent } from "react";
import {
  useAppConfigQuery,
  useEngineEventSync,
  useEngineSnapshotQuery,
  useTaskHistoryQuery,
  useTaskMutations,
  useThemeEventSync,
} from "../api";
import { useTaskUiStore } from "../store";
import {
  DEFAULT_MODEL_ROUTING,
  type AppConfig,
  type ModelRoutingConfig,
} from "../types";
import { OrchestrationInspector } from "./OrchestrationPanel";
import {
  Button,
  Card,
  Input,
  ModelChip,
  StatusBadge,
  StepMeterBar,
  Switch,
  TabBar,
  TextArea,
  categoryDisplayName,
  outcomeTone,
  phaseBadgeProps,
  stepStatusClass,
} from "./UiPrimitives";

const EXAMPLE_GOALS: ReadonlyArray<string> = [
  "Open Notes and create a checklist for today's sprint",
  "Open Safari to github.com/trending and scroll down",
  "Open Calculator and compute 42 * 18",
];

export function TrayControl() {
  useEngineEventSync();
  useThemeEventSync();

  const activeTab = useTaskUiStore((s) => s.activeTab);
  const setActiveTab = useTaskUiStore((s) => s.setActiveTab);
  const theme = useTaskUiStore((s) => s.theme);
  const setTheme = useTaskUiStore((s) => s.setTheme);
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
    phase === "classifying" ||
    phase === "planning" ||
    phase === "running" ||
    phase === "verifying" ||
    phase === "recovering" ||
    phase === "paused" ||
    phase === "awaiting_confirmation";
  const badge = phaseBadgeProps(snapshot?.phase);

  function handleToggleTheme() {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    mutations.broadcastTheme.mutate(next);
  }

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
    mutations.saveConfig.mutate(configDraft, {
      onSuccess: (saved) => setConfigDraft(saved),
    });
  }

  function updateModelField<K extends keyof ModelRoutingConfig>(
    key: K,
    value: ModelRoutingConfig[K]
  ) {
    if (!configDraft) {
      return;
    }
    setConfigDraft({
      ...configDraft,
      models: {
        ...configDraft.models,
        [key]: value,
      },
    });
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
              Multi-Model Ollama Orchestrator
            </p>
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <StatusBadge tone={badge.tone} label={badge.label} />
          <button
            type="button"
            onClick={() => mutations.togglePanel.mutate()}
            aria-label="Toggle live orchestration HUD panel"
            className="rounded-lg border border-primary/35 bg-primary/10 px-2 py-1 text-[11px] font-semibold text-primary hover:bg-primary/20 focus:outline-none focus:ring-2 focus:ring-primary/40"
          >
            HUD
          </button>
          <button
            type="button"
            onClick={handleToggleTheme}
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

                  <div className="flex flex-wrap items-center gap-1.5">
                    {snapshot?.classification ? (
                      <StatusBadge
                        tone="info"
                        label={categoryDisplayName(snapshot.classification.category)}
                      />
                    ) : null}
                    {snapshot?.activeModel ? (
                      <ModelChip
                        role={snapshot.activeRole}
                        model={snapshot.activeModel}
                        active={isActive}
                      />
                    ) : null}
                  </div>

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
                        {badge.label}...
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

        {activeTab === "live" ? (
          <div className="space-y-3">
            <div className="flex items-center justify-between gap-2">
              <span className="text-[11px] font-semibold uppercase tracking-wider text-tertiary">
                Synced Live Inspector
              </span>
              <Button
                variant="secondary"
                onClick={() => mutations.showPanel.mutate()}
              >
                Pop Out Floating HUD
              </Button>
            </div>
            <OrchestrationInspector snapshot={snapshot} />
          </div>
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
                  {(item.category || item.selectedModel) ? (
                    <div className="flex flex-wrap items-center gap-1.5">
                      {item.category ? (
                        <StatusBadge
                          tone="info"
                          label={categoryDisplayName(item.category)}
                        />
                      ) : null}
                      {item.selectedModel ? (
                        <ModelChip role="actor" model={item.selectedModel} />
                      ) : null}
                    </div>
                  ) : null}
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
            </Card>

            <Card className="space-y-2.5">
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs font-semibold text-foreground">
                  Multi-Model Routing (Ollama Cloud)
                </span>
                <button
                  type="button"
                  onClick={() =>
                    setConfigDraft({
                      ...configDraft,
                      models: { ...DEFAULT_MODEL_ROUTING },
                    })
                  }
                  className="text-[11px] font-semibold text-primary hover:underline focus:outline-none"
                >
                  Reset Defaults
                </button>
              </div>
              <Input
                id="model-judge"
                label="Judge & Classifier Model"
                type="text"
                value={configDraft.models.judgeModel}
                onChange={(e) => updateModelField("judgeModel", e.target.value)}
              />
              <Input
                id="model-planner"
                label="Planner Model"
                type="text"
                value={configDraft.models.plannerModel}
                onChange={(e) => updateModelField("plannerModel", e.target.value)}
              />
              <Input
                id="model-vision-actor"
                label="Vision / General UI Actor Model"
                type="text"
                value={configDraft.models.visionActorModel}
                onChange={(e) =>
                  updateModelField("visionActorModel", e.target.value)
                }
              />
              <Input
                id="model-coding-actor"
                label="Coding / Terminal Actor Model"
                type="text"
                value={configDraft.models.codingActorModel}
                onChange={(e) =>
                  updateModelField("codingActorModel", e.target.value)
                }
              />
              <Input
                id="model-writing-actor"
                label="Writing / Notes Actor Model"
                type="text"
                value={configDraft.models.writingActorModel}
                onChange={(e) =>
                  updateModelField("writingActorModel", e.target.value)
                }
              />
              <Input
                id="model-recovery"
                label="Recovery Specialist Model"
                type="text"
                value={configDraft.models.recoveryModel}
                onChange={(e) => updateModelField("recoveryModel", e.target.value)}
              />
            </Card>

            <Card className="space-y-3">
              <span className="text-xs font-semibold text-foreground">
                Orchestration Options
              </span>
              <Switch
                id="toggle-judge-verify"
                checked={configDraft.models.enableJudgeVerification}
                onChange={(checked) =>
                  updateModelField("enableJudgeVerification", checked)
                }
                label="Judge Step Verification"
                description="Judge verifies stalled steps and completion before stopping."
              />
              <Switch
                id="toggle-auto-panel"
                checked={configDraft.models.autoOpenPanel}
                onChange={(checked) => updateModelField("autoOpenPanel", checked)}
                label="Auto-Open Live HUD Panel"
                description="Show the floating live planning & thinking HUD when a task starts."
              />
            </Card>

            <Card className="space-y-2">
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
