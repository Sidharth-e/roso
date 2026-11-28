import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
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
import { AiOrb, orbModeForPhase } from "./AiOrb";
import { PermissionChecklist } from "./PermissionChecklist";
import { RunControls } from "./RunControls";
import { SettingsForm } from "./SettingsForm";
import {
  Button,
  Card,
  CloseIcon,
  ErrorToast,
  HudIcon,
  IconButton,
  ModelChip,
  MoonIcon,
  SendIcon,
  StepMeterBar,
  SunIcon,
  TabBar,
  phaseBadgeProps,
} from "./UiPrimitives";

const EXAMPLE_GOALS: ReadonlyArray<string> = [
  "Open Notes and create a checklist for today's sprint",
  "Open Safari to github.com/trending and scroll down",
  "Open Calculator and compute 42 * 18",
];

function greeting(nowMs: number): string {
  const hour = new Date(nowMs).getHours();
  if (hour < 5) return "Working late.";
  if (hour < 12) return "Good morning.";
  if (hour < 18) return "Good afternoon.";
  return "Good evening.";
}

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
  const [typingEnergy, setTypingEnergy] = useState(0);
  const typingTimer = useRef<number | null>(null);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);

  const snapshot = snapshotQuery.data;
  const config = configQuery.data;
  const phase = snapshot?.phase ?? "idle";
  const isActive = isActivePhase(phase);
  const badge = phaseBadgeProps(snapshot?.phase);
  const pendingConf = snapshot?.pendingConfirmation ?? null;
  const isFinished = phase === "completed" || phase === "failed" || phase === "stopped";

  useEffect(() => {
    if (phase === "awaiting_confirmation" && activeTab !== "task") {
      setActiveTab("task");
    }
  }, [phase, activeTab, setActiveTab]);

  useEffect(() => {
    setNowMs(Date.now());
    const timer = window.setInterval(() => setNowMs(Date.now()), isActive ? 1000 : 30000);
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

  // Re-focus the command bar whenever the modal comes back.
  useEffect(() => {
    function onFocus() {
      if (useTaskUiStore.getState().activeTab === "task") {
        inputRef.current?.focus();
      }
    }
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);

  useEffect(
    () => () => {
      if (typingTimer.current !== null) {
        window.clearTimeout(typingTimer.current);
      }
    },
    []
  );

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

  function pulseOrb() {
    setTypingEnergy(1);
    if (typingTimer.current !== null) {
      window.clearTimeout(typingTimer.current);
    }
    typingTimer.current = window.setTimeout(() => setTypingEnergy(0), 450);
  }

  function handleToggleTheme() {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    mutations.broadcastTheme.mutate(next);
  }

  const isCloud = config ? config.ollamaBaseUrl.startsWith(CLOUD_OLLAMA_BASE_URL) : true;
  const needsCloudConsent = Boolean(config && isCloud && !config.cloudConsentAccepted);
  const canRun =
    draftGoal.trim().length > 0 && !mutations.startTask.isPending && !needsCloudConsent;

  function runTask() {
    const clean = draftGoal.trim();
    if (!canRun || clean.length === 0) {
      return;
    }
    mutations.startTask.mutate({ goal: clean });
  }

  function handleRunTask(event: FormEvent) {
    event.preventDefault();
    runTask();
  }

  function handleInputKeyDown(event: ReactKeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      runTask();
    }
  }

  function handleTestConnection(targetUrl: string) {
    setConnectionStatus(null);
    mutations.testOllamaConnection.mutate(targetUrl, {
      onSuccess: (models) => {
        setAvailableModels(models);
        setConnectionStatus({ ok: true, message: `Connected (${models.length} models)` });
      },
      onError: (err: unknown) => {
        setConnectionStatus({
          ok: false,
          message: err instanceof Error ? err.message : "Connection failed",
        });
      },
    });
  }

  const orbMode =
    mutations.startTask.isPending
      ? "thinking"
      : draftGoal.length > 0 && !isActive && !isFinished
        ? "listening"
        : orbModeForPhase(phase);

  const headline = isActive
    ? snapshot?.lastNarration || "Working on it…"
    : isFinished && snapshot?.lastSummary
      ? snapshot.lastSummary
      : greeting(nowMs);

  const subline = isActive
    ? snapshot?.goal
    : isFinished
      ? phase === "completed"
        ? "Task complete. What's next?"
        : phase === "failed"
          ? "That didn't go to plan. Try again or rephrase."
          : "Task stopped. Ready when you are."
      : "What should I do on your Mac?";

  const onTaskTab = activeTab === "task";

  return (
    <div className="jarvis-shell">
      <header className="flex items-center justify-between px-5 pt-4">
        <div className="flex items-center gap-2.5">
          <span
            className={`h-2 w-2 rounded-full ${
              badge.tone === "error"
                ? "bg-error"
                : badge.tone === "warning"
                  ? "bg-warning"
                  : badge.tone === "success"
                    ? "bg-success"
                    : "bg-primary"
            } ${isActive ? "animate-pulse" : ""}`}
            style={{ boxShadow: "0 0 10px currentColor" }}
          />
          <span className="hud-label">Task Companion</span>
          <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-tertiary">
            / {badge.label}
          </span>
        </div>
        <div className="flex items-center gap-1.5">
          {isActive ? (
            <IconButton
              label="Show live orchestrator"
              onClick={() => mutations.showPanel.mutate()}
            >
              <HudIcon />
            </IconButton>
          ) : null}
          <IconButton
            label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
            onClick={handleToggleTheme}
          >
            {theme === "dark" ? <SunIcon /> : <MoonIcon />}
          </IconButton>
          <IconButton label="Close (Esc)" onClick={() => mutations.hidePopover.mutate()}>
            <CloseIcon />
          </IconButton>
        </div>
      </header>

      <div className="px-5 pt-2">
        <ErrorToast />
        {snapshotQuery.isError ? (
          <div className="mb-2 rounded-xl border border-error/40 bg-error/10 px-3 py-2 text-xs text-error">
            Failed to read engine status: {snapshotQuery.error.message}
          </div>
        ) : null}
      </div>

      <main className="flex-1 overflow-y-auto px-5 pb-3">
        {onTaskTab ? (
          <div id="panel-task" role="tabpanel" aria-labelledby="tab-task" className="flex flex-col">
            <section className="flex flex-col items-center pt-1 text-center">
              <AiOrb size={210} mode={orbMode} energy={typingEnergy} />
              <h1
                className="-mt-2 max-w-[440px] text-balance text-xl font-semibold tracking-tight text-foreground"
                aria-live="polite"
              >
                {headline}
              </h1>
              {subline ? (
                <p className="mt-1 max-w-[440px] text-sm text-tertiary">{subline}</p>
              ) : null}
              {snapshot?.lastError && isFinished ? (
                <p className="mt-2 max-w-[440px] rounded-lg bg-error/10 px-3 py-1.5 text-xs text-error">
                  {snapshot.lastError}
                </p>
              ) : null}
            </section>

            <div className="mt-5 space-y-3">
              {pendingConf ? (
                <ConfirmationCard
                  confirmation={pendingConf}
                  isPending={mutations.resolveConfirmation.isPending}
                  onResolve={handleResolveConfirmation}
                />
              ) : null}

              <PermissionChecklist
                snapshot={snapshot}
                config={config}
                isRequesting={
                  mutations.requestScreenPermission.isPending ||
                  mutations.requestAccessibilityPermission.isPending
                }
                onRequestScreen={() => mutations.requestScreenPermission.mutate()}
                onRequestAccessibility={() => mutations.requestAccessibilityPermission.mutate()}
                onOpenSettings={() => setActiveTab("settings")}
              />

              {needsCloudConsent && config ? (
                <Card className="space-y-2 border-info/30">
                  <p className="hud-label">Cloud privacy</p>
                  <p className="text-xs text-foreground/85">
                    Using Ollama Cloud sends in-memory screenshots, window titles, and target
                    text to ollama.com during active tasks.
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
                    <Button variant="secondary" onClick={() => setActiveTab("settings")}>
                      Use Local Ollama
                    </Button>
                  </div>
                </Card>
              ) : null}

              {!isActive ? (
                <form onSubmit={handleRunTask} className="space-y-3">
                  <label htmlFor="task-goal-input" className="sr-only">
                    What should I do on your Mac?
                  </label>
                  <div className="command-bar">
                    <textarea
                      id="task-goal-input"
                      ref={inputRef}
                      autoFocus
                      rows={1}
                      maxLength={500}
                      className="command-input"
                      placeholder="Ask me to do something on your Mac…"
                      value={draftGoal}
                      onKeyDown={handleInputKeyDown}
                      onChange={(e) => {
                        setDraftGoal(e.target.value);
                        pulseOrb();
                        const el = e.target;
                        el.style.height = "auto";
                        el.style.height = `${Math.min(el.scrollHeight, 128)}px`;
                      }}
                    />
                    <button
                      type="submit"
                      className="send-btn"
                      disabled={!canRun}
                      aria-label={mutations.startTask.isPending ? "Starting task" : "Run task"}
                      title="Run (Enter)"
                    >
                      <SendIcon />
                    </button>
                  </div>

                  <div className="flex flex-wrap justify-center gap-1.5">
                    {EXAMPLE_GOALS.map((sample) => (
                      <button
                        key={sample}
                        type="button"
                        onClick={() => {
                          setDraftGoal(sample);
                          pulseOrb();
                          inputRef.current?.focus();
                        }}
                        className="suggestion-chip"
                      >
                        {sample}
                      </button>
                    ))}
                  </div>
                </form>
              ) : (
                <Card className="space-y-3">
                  <div className="flex items-center justify-between font-mono text-[11px] text-tertiary">
                    <span>
                      STEP {snapshot?.currentStep ?? 0}/{snapshot?.maxSteps ?? 100} ·{" "}
                      {formatElapsed(snapshot?.startedAt, nowMs)}
                    </span>
                    <span className="max-w-[200px] truncate">
                      {snapshot?.activeApp || "Detecting…"}
                    </span>
                  </div>
                  <StepMeterBar
                    value={snapshot?.currentStep ?? 0}
                    max={snapshot?.maxSteps ?? 100}
                  />
                  {snapshot?.activeModel ? (
                    <ModelChip role={snapshot.activeRole} model={snapshot.activeModel} active />
                  ) : null}
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
              )}
            </div>
          </div>
        ) : null}

        {activeTab === "history" ? (
          <div id="panel-history" role="tabpanel" aria-labelledby="tab-history" className="pt-2">
            <SectionHeading title="History" orbMode={orbModeForPhase(phase)} />
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
          <div id="panel-settings" role="tabpanel" aria-labelledby="tab-settings" className="pt-2">
            <SectionHeading title="Settings" orbMode={orbModeForPhase(phase)} />
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
            <Card className="mt-3">
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
      </main>

      <footer className="flex items-center justify-between gap-2 border-t border-primary/10 px-5 py-3">
        <span className="w-20 font-mono text-[10px] uppercase tracking-[0.16em] text-tertiary/70">
          Esc close
        </span>
        <TabBar activeTab={activeTab} onSelect={setActiveTab} />
        <span className="w-20 text-right font-mono text-[10px] uppercase tracking-[0.16em] text-tertiary/70">
          ⌃⌥S kill
        </span>
      </footer>
    </div>
  );
}

function SectionHeading({
  title,
  orbMode,
}: {
  title: string;
  orbMode: ReturnType<typeof orbModeForPhase>;
}) {
  return (
    <div className="mb-3 flex items-center gap-3">
      <AiOrb size={44} mode={orbMode} rings={false} />
      <div>
        <p className="hud-label">Task Companion</p>
        <h2 className="text-lg font-semibold tracking-tight text-foreground">{title}</h2>
      </div>
    </div>
  );
}
