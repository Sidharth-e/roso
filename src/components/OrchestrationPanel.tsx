import {
  useEngineEventSync,
  useEngineSnapshotQuery,
  useTaskMutations,
  useThemeEventSync,
} from "../api";
import { useTaskUiStore } from "../store";
import type { EngineSnapshot, PanelFilter } from "../types";
import {
  Button,
  Card,
  ModelChip,
  StatusBadge,
  StepMeterBar,
  categoryDisplayName,
  phaseBadgeProps,
  stepStatusClass,
} from "./UiPrimitives";

const FILTER_OPTIONS: ReadonlyArray<{ id: PanelFilter; label: string }> = [
  { id: "all", label: "All" },
  { id: "plan", label: "Plan" },
  { id: "think", label: "Thinking" },
  { id: "steps", label: "Steps" },
];

function traceBadgeProps(stage: string, status: string): {
  tone: "success" | "warning" | "error" | "neutral" | "info";
  label: string;
} {
  const tone: "success" | "warning" | "error" | "neutral" | "info" =
    status === "success"
      ? "success"
      : status === "warning"
        ? "warning"
        : status === "error"
          ? "error"
          : "info";

  switch (stage) {
    case "classify":
      return { tone, label: "Classify" };
    case "plan":
      return { tone, label: "Plan" };
    case "think":
      return { tone, label: "Think" };
    case "verify":
      return { tone, label: "Verify" };
    case "recover":
      return { tone, label: "Recover" };
    case "fallback":
      return { tone, label: "Fallback" };
    case "complete":
      return { tone, label: "Complete" };
    default:
      return { tone, label: stage };
  }
}

function isPhaseActive(phase: EngineSnapshot["phase"]): boolean {
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

export function OrchestrationInspector({
  snapshot,
}: {
  snapshot: EngineSnapshot | undefined;
}) {
  const panelFilter = useTaskUiStore((s) => s.panelFilter);
  const setPanelFilter = useTaskUiStore((s) => s.setPanelFilter);
  const mutations = useTaskMutations();

  if (!snapshot || (snapshot.phase === "idle" && snapshot.traces.length === 0 && snapshot.steps.length === 0)) {
    return (
      <Card className="space-y-2 text-center">
        <p className="text-xs font-semibold text-foreground">
          Orchestration Panel Ready
        </p>
        <p className="text-[11px] text-tertiary">
          Start a task from Task Companion to inspect live Judge classification, planning, thinking traces, model routing, and execution steps.
        </p>
      </Card>
    );
  }

  const active = isPhaseActive(snapshot.phase);
  const showPlan = panelFilter === "all" || panelFilter === "plan";
  const showThink = panelFilter === "all" || panelFilter === "think";
  const showSteps = panelFilter === "all" || panelFilter === "steps";

  return (
    <div className="space-y-2.5">
      <Card className="space-y-2">
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0">
            <p className="text-[10px] font-semibold uppercase tracking-wider text-tertiary">
              Goal
            </p>
            <p className="text-xs font-semibold text-foreground break-words">
              {snapshot.goal || "No active goal"}
            </p>
          </div>
          {snapshot.activeApp ? (
            <span className="shrink-0 rounded border border-tertiary/30 bg-background px-1.5 py-0.5 text-[10px] font-medium text-tertiary">
              {snapshot.activeApp}
            </span>
          ) : null}
        </div>

        <div className="flex items-center justify-between text-[11px] text-tertiary">
          <span>
            Step {snapshot.currentStep} / {snapshot.maxSteps}
          </span>
          {snapshot.activeModel ? (
            <ModelChip
              role={snapshot.activeRole}
              model={snapshot.activeModel}
              active={active}
            />
          ) : null}
        </div>

        <StepMeterBar value={snapshot.currentStep} max={snapshot.maxSteps} />
      </Card>

      {snapshot.pendingConfirmation ? (
        <Card className="space-y-2 border-warning/50 bg-warning/10">
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs font-bold text-warning">
              Approve Step #{snapshot.pendingConfirmation.stepNumber}
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

      {snapshot.classification ? (
        <Card className="space-y-1.5">
          <div className="flex flex-wrap items-center justify-between gap-1.5">
            <span className="text-[10px] font-semibold uppercase tracking-wider text-tertiary">
              Judge Classification
            </span>
            <div className="flex flex-wrap items-center gap-1">
              <StatusBadge
                tone="info"
                label={categoryDisplayName(snapshot.classification.category)}
              />
              <StatusBadge
                tone="neutral"
                label={snapshot.classification.complexity}
              />
            </div>
          </div>
          <p className="text-[11px] leading-relaxed text-foreground/90">
            {snapshot.classification.reasoning}
          </p>
          <div className="pt-0.5">
            <ModelChip
              role={snapshot.classification.recommendedRole}
              model={snapshot.activeModel || snapshot.classification.recommendedRole}
            />
          </div>
        </Card>
      ) : null}

      {snapshot.currentThought ? (
        <div
          aria-live="polite"
          className="rounded-card border border-primary/30 bg-primary/10 px-3 py-2"
        >
          <p className="text-[10px] font-bold uppercase tracking-wider text-primary">
            Live Thinking
          </p>
          <p className="mt-0.5 text-[11px] leading-relaxed text-foreground">
            {snapshot.currentThought}
          </p>
        </div>
      ) : null}

      <div
        role="tablist"
        aria-label="Orchestration panel sections"
        className="grid grid-cols-4 gap-1 rounded-lg bg-secondary/80 p-1"
      >
        {FILTER_OPTIONS.map((option) => (
          <button
            key={option.id}
            type="button"
            role="tab"
            aria-selected={panelFilter === option.id}
            onClick={() => setPanelFilter(option.id)}
            className={
              panelFilter === option.id ? "filter-pill-active" : "filter-pill"
            }
          >
            {option.label}
          </button>
        ))}
      </div>

      {showPlan ? (
        <Card className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold text-foreground">
              Orchestrated Plan
            </span>
            <span className="text-[10px] text-tertiary">
              {snapshot.planSteps.length} steps
            </span>
          </div>
          {snapshot.planSteps.length === 0 ? (
            <p className="text-[11px] text-tertiary">
              Waiting for plan generation...
            </p>
          ) : (
            <ol className="space-y-1">
              {snapshot.planSteps.map((stepText, idx) => {
                const activeIdx = snapshot.activePlanIndex;
                const done = idx < activeIdx || snapshot.phase === "completed";
                const current =
                  idx === activeIdx && snapshot.phase !== "completed";
                return (
                  <li
                    key={`${idx}-${stepText}`}
                    className={`flex items-start gap-2 rounded-md px-2 py-1 text-[11px] ${
                      current
                        ? "bg-primary/15 font-semibold text-foreground"
                        : done
                          ? "text-tertiary line-through"
                          : "text-foreground/85"
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
          )}
        </Card>
      ) : null}

      {showThink ? (
        <Card className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold text-foreground">
              Thinking & Agent Trace
            </span>
            <span className="text-[10px] text-tertiary">
              {snapshot.traces.length} events
            </span>
          </div>
          {snapshot.traces.length === 0 ? (
            <p className="text-[11px] text-tertiary">
              Reasoning and model switching events will appear here.
            </p>
          ) : (
            <div className="max-h-52 space-y-1.5 overflow-y-auto pr-0.5">
              {[...snapshot.traces].reverse().map((trace) => {
                const badge = traceBadgeProps(trace.stage, trace.status);
                return (
                  <div key={trace.id} className="trace-card space-y-1">
                    <div className="flex items-center justify-between gap-2">
                      <StatusBadge tone={badge.tone} label={badge.label} />
                      <span className="text-[10px] text-tertiary">
                        {trace.stepNumber !== null ? `#${trace.stepNumber} · ` : ""}
                        {new Date(trace.timestamp).toLocaleTimeString([], {
                          hour: "2-digit",
                          minute: "2-digit",
                          second: "2-digit",
                        })}
                      </span>
                    </div>
                    <p className="text-[11px] font-semibold text-foreground">
                      {trace.title}
                    </p>
                    <p className="text-[11px] leading-relaxed text-foreground/85">
                      {trace.content}
                    </p>
                    {trace.model ? (
                      <div className="pt-0.5">
                        <ModelChip role={trace.role} model={trace.model} />
                      </div>
                    ) : null}
                  </div>
                );
              })}
            </div>
          )}
        </Card>
      ) : null}

      {showSteps ? (
        <Card className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold text-foreground">
              Execution Steps
            </span>
            <span className="text-[10px] text-tertiary">
              {snapshot.steps.length} actions
            </span>
          </div>
          {snapshot.steps.length === 0 ? (
            <p className="text-[11px] text-tertiary">
              Actions taken on screen will stream here in real time.
            </p>
          ) : (
            <div className="max-h-52 space-y-1.5 overflow-y-auto pr-0.5">
              {[...snapshot.steps].reverse().map((step, idx) => (
                <div
                  key={`${step.stepNumber}-${step.actionType}-${idx}-${step.timestamp}`}
                  className="trace-card space-y-1"
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
                  {step.thought ? (
                    <p className="text-[10px] text-foreground/80">
                      {step.thought}
                    </p>
                  ) : null}
                  {step.model ? (
                    <p className="font-mono text-[10px] text-tertiary">
                      Model: {step.model}
                    </p>
                  ) : null}
                  {step.verificationNote ? (
                    <p className="text-[10px] text-tertiary">
                      Check: {step.verificationNote}
                    </p>
                  ) : null}
                </div>
              ))}
            </div>
          )}
        </Card>
      ) : null}
    </div>
  );
}

export function FloatingOrchestrationHud() {
  useEngineEventSync();
  useThemeEventSync();

  const snapshotQuery = useEngineSnapshotQuery();
  const mutations = useTaskMutations();
  const theme = useTaskUiStore((s) => s.theme);
  const setTheme = useTaskUiStore((s) => s.setTheme);
  const panelCollapsed = useTaskUiStore((s) => s.panelCollapsed);
  const setPanelCollapsed = useTaskUiStore((s) => s.setPanelCollapsed);

  const snapshot = snapshotQuery.data;
  const phase = snapshot?.phase ?? "idle";
  const active = isPhaseActive(phase);
  const badge = phaseBadgeProps(snapshot?.phase);

  function handleToggleTheme() {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    mutations.broadcastTheme.mutate(next);
  }

  function handleToggleCollapse() {
    const next = !panelCollapsed;
    setPanelCollapsed(next);
    mutations.setPanelCollapsed.mutate(next);
  }

  if (panelCollapsed) {
    return (
      <div className="hud-pill">
        <div className="flex min-w-0 items-center gap-2">
          <StatusBadge tone={badge.tone} label={badge.label} />
          <span className="truncate font-mono text-[10px] text-foreground/85">
            {snapshot?.activeModel || "Orchestrator"}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {active ? (
            <button
              type="button"
              onClick={() => mutations.stopTask.mutate()}
              aria-label="Stop active task"
              className="rounded-md bg-error/15 px-1.5 py-0.5 text-[10px] font-semibold text-error hover:bg-error/25 focus:outline-none focus:ring-2 focus:ring-error/40"
            >
              Stop
            </button>
          ) : null}
          <button
            type="button"
            onClick={handleToggleCollapse}
            aria-label="Expand live orchestration panel"
            className="rounded-md border border-tertiary/30 bg-secondary px-2 py-0.5 text-[10px] font-semibold text-foreground hover:bg-secondary/80 focus:outline-none focus:ring-2 focus:ring-primary/40"
          >
            Expand
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="hud-shell p-3">
      <header className="mb-2.5 flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-lg bg-primary/15 text-[11px] font-bold text-primary">
            AI
          </span>
          <div className="min-w-0">
            <h1 className="truncate text-xs font-bold text-foreground">
              Live Orchestrator
            </h1>
            <p className="truncate text-[10px] text-tertiary">
              Multi-Model Agent HUD
            </p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <StatusBadge tone={badge.tone} label={badge.label} />
          <button
            type="button"
            onClick={handleToggleTheme}
            aria-label="Toggle color theme"
            className="rounded-md border border-tertiary/30 bg-secondary px-1.5 py-1 text-[10px] font-semibold text-foreground hover:bg-secondary/80 focus:outline-none focus:ring-2 focus:ring-primary/40"
          >
            {theme === "dark" ? "Light" : "Dark"}
          </button>
          <button
            type="button"
            onClick={handleToggleCollapse}
            aria-label="Collapse orchestration panel"
            className="rounded-md border border-tertiary/30 bg-secondary px-1.5 py-1 text-[10px] font-semibold text-foreground hover:bg-secondary/80 focus:outline-none focus:ring-2 focus:ring-primary/40"
          >
            _
          </button>
          <button
            type="button"
            onClick={() => mutations.hidePanel.mutate()}
            aria-label="Hide orchestration panel"
            className="rounded-md border border-tertiary/30 bg-secondary px-1.5 py-1 text-[10px] font-semibold text-tertiary hover:text-foreground focus:outline-none focus:ring-2 focus:ring-primary/40"
          >
            ✕
          </button>
        </div>
      </header>

      <div className="flex-1 overflow-y-auto pr-0.5">
        <OrchestrationInspector snapshot={snapshot} />
      </div>

      {active ? (
        <footer className="mt-2.5 grid grid-cols-2 gap-2 border-t border-tertiary/20 pt-2.5">
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
        </footer>
      ) : null}
    </div>
  );
}
