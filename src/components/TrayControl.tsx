import { useCallback, useEffect, useState, type FormEvent } from "react";
import {
  useAppConfigQuery,
  useEngineEventSync,
  useEngineSnapshotQuery,
  useFocusPermissionRefresh,
  useTaskHistoryQuery,
  useTaskMutations,
  useThemeEventSync,
} from "../api";
import { useTaskUiStore } from "../store";
import { CLOUD_OLLAMA_BASE_URL, isActivePhase } from "../types";
import { ConfirmationCard } from "./ConfirmationCard";
import { HistoryView } from "./HistoryView";
import { OrchestrationInspector } from "./OrchestrationPanel";
import { PermissionChecklist } from "./PermissionChecklist";
import { PlanChecklist } from "./PlanChecklist";
import { RunControls } from "./RunControls";
import { SettingsForm } from "./SettingsForm";
import { StepFeed } from "./StepFeed";
import {
  Button,
  Card,
  CloseIcon,
  ErrorToast,
  HudIcon,
  IconButton,
  ModelChip,
  MoonIcon,
  StatusBadge,
  StepMeterBar,
  SunIcon,
  TabBar,
  TextArea,
  categoryDisplayName,
  phaseBadgeProps,
} from "./UiPrimitives";

const EXAMPLE_GOALS: ReadonlyArray<string> = [
  "Open Notes and create a checklist for today's sprint",
  "Open Safari to github.com/trending and scroll down",
  "Open Calculator and compute 42 * 18",
];

function formatElapsed(startedAt: string | null | undefined, nowMs: number): string {
  if (!startedAt) {
    return "0:00";
  }
  const startMs = Date.parse(startedAt);
  if (Number.isNaN(startMs)) {
    return "0:00";
  }
  const totalSec = Math.max(0, Math.floor((nowMs - startMs) / 1000));
  const mins = Math.floor(totalSec / 60);
  const secs = totalSec % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

export function TrayControl() {
  useEngineEventSync();
  useThemeEventSync();
  useFocusPermissionRefresh();

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

  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [connectionStatus, setConnectionStatus] = useState<{
    ok: boolean;
    message: string;
  } | null>(null);
  const [nowMs, setNowMs] = useState(() => Date.now());

  const snapshot = snapshotQuery.data;
  const config = configQuery.data;
  const phase = snapshot?.phase ?? "idle";
  const isActive = isActivePhase(phase);
  const badge = phaseBadgeProps(snapshot?.phase);
  const pendingConf = snapshot?.pendingConfirmation ?? null;

  useEffect(() => {
    if (phase === "awaiting_confirmation" && activeTab !== "task") {
      setActiveTab("task");
    }
  }, [phase, activeTab, setActiveTab]);

  useEffect(() => {
    if (!isActive) {
      return;
    }
    setNowMs(Date.now());
    const timer = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [isActive]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape" && !pendingConf) {
        mutations.hidePopover.mutate();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [mutations.hidePopover, pendingConf]);

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

  function handleTestConnection(targetUrl: string) {
    setConnectionStatus(null);
    mutations.testOllamaConnection.mutate(targetUrl, {
      onSuccess: (models) => {
        setAvailableModels(models);
        setConnectionStatus({
          ok: true,
          message: `Connected (${models.length} models)`,
        });
      },
      onError: (err: unknown) => {
        setConnectionStatus({
          ok: false,
          message: err instanceof Error ? err.message : "Connection failed",
        });
      },
    });
  }

  const isCloud = config
    ? config.ollamaBaseUrl.startsWith(CLOUD_OLLAMA_BASE_URL)
    : true;
  const needsCloudConsent = Boolean(config && isCloud && !config.cloudConsentAccepted);
  const nextPlanStep =
    snapshot && snapshot.planSteps.length > 0
      ? snapshot.planSteps[snapshot.activePlanIndex + 1] ?? null
      : null;

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
            <p className="text-xs text-tertiary">
              Multi-Model Ollama Orchestrator
            </p>
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <StatusBadge tone={badge.tone} label={badge.label} />
          <IconButton
            label="Toggle live orchestration HUD panel"
            onClick={() => mutations.togglePanel.mutate()}
          >
            <HudIcon />
          </IconButton>
          <IconButton
            label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
            onClick={handleToggleTheme}
          >
            {theme === "dark" ? <SunIcon /> : <MoonIcon />}
          </IconButton>
          <IconButton
            label="Close popover"
            onClick={() => mutations.hidePopover.mutate()}
          >
            <CloseIcon />
          </IconButton>
        </div>
      </header>

      <ErrorToast />

      {snapshotQuery.isError ? (
        <div className="mb-2.5 rounded-lg border border-error/40 bg-error/15 px-3 py-2 text-xs text-error">
          Failed to read engine status: {snapshotQuery.error.message}
        </div>
      ) : null}

      {pendingConf ? (
        <div className="mb-2.5">
          <ConfirmationCard
            confirmation={pendingConf}
            isPending={mutations.resolveConfirmation.isPending}
            onResolve={handleResolveConfirmation}
          />
        </div>
      ) : null}

      <TabBar activeTab={activeTab} onSelect={setActiveTab} />

      <div className="mt-3 flex-1 space-y-3 overflow-y-auto pr-0.5">
        {activeTab === "task" ? (
          <div
            id="panel-task"
            role="tabpanel"
            aria-labelledby="tab-task"
            className="space-y-3"
          >
            <PermissionChecklist
              snapshot={snapshot}
              config={config}
              isRequesting={
                mutations.requestScreenPermission.isPending ||
                mutations.requestAccessibilityPermission.isPending
              }
              onRequestScreen={() => mutations.requestScreenPermission.mutate()}
              onRequestAccessibility={() =>
                mutations.requestAccessibilityPermission.mutate()
              }
              onOpenSettings={() => setActiveTab("settings")}
            />

            {needsCloudConsent && config ? (
              <Card className="space-y-2 border-info/40 bg-info/10">
                <p className="text-xs font-bold text-foreground">
                  Cloud Privacy Disclosure
                </p>
                <p className="text-xs text-foreground/85">
                  Using Ollama Cloud sends in-memory screenshots, window titles, and target text to ollama.com during active tasks.
                </p>
                <div className="grid grid-cols-2 gap-2">
                  <Button
                    variant="primary"
                    disabled={mutations.saveConfig.isPending}
                    onClick={() =>
                      mutations.saveConfig.mutate({
                        ollamaBaseUrl: config.ollamaBaseUrl,
                        ollamaApiKey: "",
                        clearApiKey: false,
                        cloudConsentAccepted: true,
                        models: config.models,
                      })
                    }
                  >
                    Accept & Continue
                  </Button>
                  <Button
                    variant="secondary"
                    onClick={() => setActiveTab("settings")}
                  >
                    Use Local Ollama
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
                        className="rounded-md border border-tertiary/25 bg-background px-2 py-1 text-left text-xs font-medium text-tertiary transition hover:border-primary/40 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
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
                      mutations.startTask.isPending ||
                      needsCloudConsent
                    }
                  >
                    {mutations.startTask.isPending ? "Starting..." : "Run Task"}
                  </Button>
                </Card>

                {snapshot &&
                (phase === "completed" ||
                  phase === "failed" ||
                  phase === "stopped") &&
                snapshot.lastSummary ? (
                  <Card className="space-y-1.5">
                    <div className="flex items-center justify-between">
                      <span className="text-xs font-semibold text-tertiary">
                        Last Run
                      </span>
                      <StatusBadge tone={badge.tone} label={badge.label} />
                    </div>
                    <p className="text-xs font-medium text-foreground">
                      {snapshot.lastSummary}
                    </p>
                    {snapshot.lastError ? (
                      <p className="rounded bg-error/15 px-2 py-1 text-xs text-error">
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
                      <p className="text-xs font-semibold text-foreground">
                        {snapshot?.lastNarration || snapshot?.goal}
                      </p>
                      {nextPlanStep ? (
                        <p className="text-xs text-tertiary">
                          Next: {nextPlanStep}
                        </p>
                      ) : null}
                    </div>
                    <span className="shrink-0 rounded border border-tertiary/30 bg-background px-1.5 py-0.5 text-[10px] font-semibold text-tertiary">
                      ⌃⌥S Kill
                    </span>
                  </div>

                  <div className="flex items-center justify-between text-xs text-tertiary">
                    <span>
                      Step {snapshot?.currentStep ?? 0}/{snapshot?.maxSteps ?? 40} ·{" "}
                      {formatElapsed(snapshot?.startedAt, nowMs)}
                    </span>
                    <span className="max-w-[170px] truncate">
                      {snapshot?.activeApp || "Detecting..."}
                    </span>
                  </div>

                  <StepMeterBar
                    value={snapshot?.currentStep ?? 0}
                    max={snapshot?.maxSteps ?? 40}
                  />

                  <div className="flex flex-wrap items-center gap-1.5">
                    {snapshot?.classification ? (
                      <StatusBadge
                        tone="info"
                        label={categoryDisplayName(
                          snapshot.classification.category
                        )}
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
                </Card>

                <PlanChecklist
                  planSteps={snapshot?.planSteps ?? []}
                  activePlanIndex={snapshot?.activePlanIndex ?? 0}
                  phase={phase}
                />

                {snapshot?.lastDiagnosis ? (
                  <Card className="space-y-1 border-warning/40 bg-warning/10">
                    <p className="text-xs font-semibold text-warning">
                      Recovery #{snapshot.recoveryCount}
                    </p>
                    <p className="text-xs text-foreground/85">
                      {snapshot.lastDiagnosis}
                    </p>
                  </Card>
                ) : null}
              </div>
            )}

            <StepFeed
              steps={snapshot?.steps ?? []}
              title="Live Action Feed"
              compact={false}
            />

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
          </div>
        ) : null}

        {activeTab === "live" ? (
          <div
            id="panel-live"
            role="tabpanel"
            aria-labelledby="tab-live"
            className="space-y-3"
          >
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs font-semibold text-tertiary">
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
          <div
            id="panel-history"
            role="tabpanel"
            aria-labelledby="tab-history"
          >
            <HistoryView
              records={historyQuery.data ?? []}
              isError={historyQuery.isError}
              onRunAgain={(goal) => {
                setDraftGoal(goal);
                setActiveTab("task");
              }}
            />
          </div>
        ) : null}

        {activeTab === "settings" && config ? (
          <div
            id="panel-settings"
            role="tabpanel"
            aria-labelledby="tab-settings"
          >
            <SettingsForm
              config={config}
              isSaving={mutations.saveConfig.isPending}
              saveError={
                mutations.saveConfig.isError
                  ? mutations.saveConfig.error instanceof Error
                    ? mutations.saveConfig.error.message
                    : "Failed to save configuration"
                  : null
              }
              saveSuccess={mutations.saveConfig.isSuccess}
              isTesting={mutations.testOllamaConnection.isPending}
              availableModels={availableModels}
              connectionStatus={connectionStatus}
              onSave={(payload) => mutations.saveConfig.mutate(payload)}
              onTestConnection={handleTestConnection}
            />
          </div>
        ) : null}
      </div>
    </div>
  );
}
