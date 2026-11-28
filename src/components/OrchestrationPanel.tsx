import { memo, useCallback, useMemo, type KeyboardEvent } from "react";
import {
  useEngineEventSync,
  useEngineSnapshotQuery,
  useTaskMutations,
  useThemeEventSync,
} from "../api";
import { useTaskUiStore } from "../store";
import {
  isActivePhase,
  type EngineSnapshot,
  type OrchestrationTraceEntry,
  type PanelFilter,
} from "../types";
import { ConfirmationCard } from "./ConfirmationCard";
import { AiOrb, orbModeForPhase } from "./AiOrb";
import { PlanChecklist } from "./PlanChecklist";
import { RunControls } from "./RunControls";
import { StepFeed } from "./StepFeed";
import {
  Card,
  CloseIcon,
  ErrorToast,
  ExpandIcon,
  IconButton,
  MinimizeIcon,
  ModelChip,
  MoonIcon,
  StatusBadge,
  StepMeterBar,
  SunIcon,
  categoryDisplayName,
  phaseBadgeProps,
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

const TraceRow = memo(function TraceRow({
  trace,
}: {
  trace: OrchestrationTraceEntry;
}) {
  const badge = traceBadgeProps(trace.stage, trace.status);
  return (
    <div className="trace-card space-y-1">
      <div className="flex items-center justify-between gap-2">
        <StatusBadge tone={badge.tone} label={badge.label} />
        <span className="text-xs text-tertiary">
          {trace.stepNumber !== null ? `#${trace.stepNumber} · ` : ""}
          {new Date(trace.timestamp).toLocaleTimeString([], {
            hour: "2-digit",
            minute: "2-digit",
            second: "2-digit",
          })}
        </span>
      </div>
      <p className="text-xs font-semibold text-foreground">{trace.title}</p>
      <p className="text-xs leading-relaxed text-foreground/85">
        {trace.content}
      </p>
      {trace.model ? (
        <div className="pt-0.5">
          <ModelChip role={trace.role} model={trace.model} />
        </div>
      ) : null}
    </div>
  );
});

export function OrchestrationInspector({
  snapshot,
}: {
  snapshot: EngineSnapshot | undefined;
}) {
  const panelFilter = useTaskUiStore((s) => s.panelFilter);
  const setPanelFilter = useTaskUiStore((s) => s.setPanelFilter);
  const mutations = useTaskMutations();

  const reversedTraces = useMemo(
    () => [...(snapshot?.traces ?? [])].reverse(),
    [snapshot?.traces]
  );

  const pendingConf = snapshot?.pendingConfirmation ?? null;
  const handleResolveConfirmation = useCallback(
    (approved: boolean) => {
      if (!pendingConf) {
        return;
      }
      mutations.resolveConfirmation.mutate({
        confirmationId: pendingConf.id,
        approved,
      });
    },
    [pendingConf, mutations.resolveConfirmation]
  );

  function handleTabKeyDown(event: KeyboardEvent<HTMLButtonElement>, idx: number) {
    let nextIdx: number | null = null;
    if (event.key === "ArrowRight") {
      nextIdx = (idx + 1) % FILTER_OPTIONS.length;
    } else if (event.key === "ArrowLeft") {
      nextIdx = (idx - 1 + FILTER_OPTIONS.length) % FILTER_OPTIONS.length;
    }
    if (nextIdx !== null) {
      event.preventDefault();
      const target = FILTER_OPTIONS[nextIdx];
      setPanelFilter(target.id);
      document.getElementById(`hud-tab-${target.id}`)?.focus();
    }
  }

  if (
    !snapshot ||
    (snapshot.phase === "idle" &&
      snapshot.traces.length === 0 &&
      snapshot.steps.length === 0)
  ) {
    return (
      <Card className="space-y-2 text-center">
        <p className="text-xs font-semibold text-foreground">
          Orchestration Panel Ready
        </p>
        <p className="text-xs text-tertiary">
          Start a task from Task Companion to inspect live Judge classification, planning, thinking traces, model routing, and execution steps.
        </p>
      </Card>
    );
  }

  const active = isActivePhase(snapshot.phase);
  const showPlan = panelFilter === "all" || panelFilter === "plan";
  const showThink = panelFilter === "all" || panelFilter === "think";
  const showSteps = panelFilter === "all" || panelFilter === "steps";
  const phaseBadge = phaseBadgeProps(snapshot.phase);

  return (
    <div className="space-y-2.5">
      <div className="sr-only" aria-live="polite">
        Task phase: {phaseBadge.label}
      </div>

      <Card className="space-y-2">
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0">
            <p className="text-xs font-semibold text-foreground break-words">
              {snapshot.lastNarration || snapshot.goal || "No active goal"}
            </p>
            <p className="text-xs text-tertiary break-words">
              {snapshot.goal}
            </p>
          </div>
          {snapshot.activeApp ? (
            <span className="shrink-0 rounded border border-tertiary/30 bg-background px-1.5 py-0.5 text-[10px] font-medium text-tertiary">
              {snapshot.activeApp}
            </span>
          ) : null}
        </div>

        <div className="flex items-center justify-between text-xs text-tertiary">
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

      {pendingConf ? (
        <ConfirmationCard
          confirmation={pendingConf}
          isPending={mutations.resolveConfirmation.isPending}
          onResolve={handleResolveConfirmation}
          keyboardShortcuts={false}
        />
      ) : null}

      {snapshot.classification ? (
        <Card className="space-y-1.5">
          <div className="flex flex-wrap items-center justify-between gap-1.5">
            <span className="text-xs font-semibold text-tertiary">
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
          <p className="text-xs leading-relaxed text-foreground/90">
            {snapshot.classification.reasoning}
          </p>
          <div className="pt-0.5">
            <ModelChip
              role={snapshot.classification.recommendedRole}
              model={
                snapshot.activeModel || snapshot.classification.recommendedRole
              }
            />
          </div>
        </Card>
      ) : null}

      {snapshot.currentThought ? (
        <div className="rounded-2xl border border-primary/30 bg-primary/10 px-3 py-2.5">
          <p className="hud-label">Live Thinking</p>
          <p className="mt-0.5 text-xs leading-relaxed text-foreground">
            {snapshot.currentThought}
          </p>
        </div>
      ) : null}

      <div
        role="tablist"
        aria-label="Orchestration panel sections"
        className="grid grid-cols-4 gap-1 rounded-full border border-primary/15 bg-secondary/50 p-1"
      >
        {FILTER_OPTIONS.map((option, idx) => {
          const selected = panelFilter === option.id;
          return (
            <button
              key={option.id}
              id={`hud-tab-${option.id}`}
              type="button"
              role="tab"
              aria-selected={selected}
              aria-controls="hud-tabpanel"
              tabIndex={selected ? 0 : -1}
              onClick={() => setPanelFilter(option.id)}
              onKeyDown={(e) => handleTabKeyDown(e, idx)}
              className={selected ? "filter-pill-active" : "filter-pill"}
            >
              {option.label}
            </button>
          );
        })}
      </div>

      <div
        id="hud-tabpanel"
        role="tabpanel"
        aria-labelledby={`hud-tab-${panelFilter}`}
        className="space-y-2.5"
      >
        {showPlan ? (
          <PlanChecklist
            planSteps={snapshot.planSteps}
            activePlanIndex={snapshot.activePlanIndex}
            phase={snapshot.phase}
            showEmptyPlaceholder
          />
        ) : null}

        {showThink ? (
          <Card className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-xs font-semibold text-foreground">
                Thinking & Agent Trace
              </span>
              <span className="text-xs text-tertiary">
                {snapshot.traces.length} events
              </span>
            </div>
            {snapshot.traces.length === 0 ? (
              <p className="text-xs text-tertiary">
                Reasoning and model switching events will appear here.
              </p>
            ) : (
              <div className="max-h-52 space-y-1.5 overflow-y-auto pr-0.5">
                {reversedTraces.map((trace) => (
                  <TraceRow key={trace.id} trace={trace} />
                ))}
              </div>
            )}
          </Card>
        ) : null}

        {showSteps ? (
          <StepFeed
            steps={snapshot.steps}
            title="Execution Steps"
            compact
            showEmpty
          />
        ) : null}
      </div>
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
  const active = isActivePhase(phase);
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

  const orbMode = orbModeForPhase(snapshot?.phase);

  if (panelCollapsed) {
    return (
      <div className="hud-pill">
        <div className="flex min-w-0 items-center gap-2">
          <AiOrb size={46} mode={orbMode} rings={false} className="shrink-0" />
          <div className="min-w-0">
            <p className="hud-label leading-none">{badge.label}</p>
            <p className="mt-1 truncate font-mono text-[11px] text-foreground/80">
              {snapshot?.activeModel || "Orchestrator"}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {active ? (
            <button
              type="button"
              onClick={() => mutations.stopTask.mutate()}
              aria-label="Stop active task"
              className="rounded-full bg-error/15 px-2.5 py-1 text-xs font-semibold text-error hover:bg-error/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-error/40"
            >
              Stop
            </button>
          ) : null}
          <IconButton label="Expand live orchestration panel" onClick={handleToggleCollapse}>
            <ExpandIcon />
          </IconButton>
        </div>
      </div>
    );
  }

  return (
    <div className="jarvis-shell hud-shell p-3.5">
      <header className="mb-3 flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2.5">
          <AiOrb size={56} mode={orbMode} rings={false} className="-m-1.5 shrink-0" />
          <div className="min-w-0">
            <p className="hud-label">Live Orchestrator</p>
            <p className="truncate text-sm font-semibold text-foreground">{badge.label}</p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <IconButton
            label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
            onClick={handleToggleTheme}
          >
            {theme === "dark" ? <SunIcon /> : <MoonIcon />}
          </IconButton>
          <IconButton label="Collapse orchestration panel" onClick={handleToggleCollapse}>
            <MinimizeIcon />
          </IconButton>
          <IconButton label="Hide orchestration panel" onClick={() => mutations.hidePanel.mutate()}>
            <CloseIcon />
          </IconButton>
        </div>
      </header>

      <ErrorToast />

      {snapshotQuery.isError ? (
        <div className="mb-2 rounded-lg border border-error/40 bg-error/15 px-2.5 py-1.5 text-xs text-error">
          Snapshot error: {snapshotQuery.error.message}
        </div>
      ) : null}

      <div className="flex-1 overflow-y-auto pr-0.5">
        <OrchestrationInspector snapshot={snapshot} />
      </div>

      {active ? (
        <footer className="mt-3 border-t border-primary/10 pt-3">
          <RunControls
            phase={phase}
            isBusy={
              mutations.pauseTask.isPending ||
              mutations.resumeTask.isPending ||
              mutations.stopTask.isPending
            }
            onPause={() => mutations.pauseTask.mutate()}
            onResume={() => mutations.resumeTask.mutate()}
            onStop={() => mutations.stopTask.mutate()}
          />
        </footer>
      ) : null}
    </div>
  );
}
