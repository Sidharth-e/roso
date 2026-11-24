use crate::automation::{
    action_type_label, assess_action_risk, describe_action, execute_action,
    has_accessibility_permission, is_kill_switch_pressed, AgentAction, MouseButtonKind,
};
use crate::capture::{
    annotate_rgb_and_encode_jpeg_base64, capture_raw_frame_in_ram, compute_frame_diff_ratio,
    downscale_rgba_to_rgb, has_screen_recording_permission, read_active_window_metadata,
    ActiveWindowMetadata, ScreenMetrics, MAX_DOWNSCALED_WIDTH,
};
use crate::classifier::{
    AgentError, ClassifyTaskContext, CreatePlanContext, NextActionContext, OllamaAgentClient,
    PlannedActionDecision, RecoveryContext, TaskClassification, VerifyStepContext,
};
use crate::config::{
    append_history_record, AppConfig, ModelRoutingConfig, TaskHistoryRecord, TaskStepRecord,
    CONFIRM_RISKY_ACTIONS, MAX_STEPS, OLLAMA_BASE_URL, STEP_DELAY_MS,
};
use crate::ocr::{extract_ax_elements, extract_ocr_targets, merge_targets, ScreenTargetMap};
use crate::overlay::{
    emit_pointer_cue, hide_tray_popover, show_orchestration_panel, show_tray_popover,
    update_tray_status_title, PointerCuePayload,
};
use chrono::{DateTime, Utc};
use image::RgbaImage;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::{oneshot, watch};
use uuid::Uuid;

const MAX_RECOVERY_PLANS: u32 = 3;
const MAX_TRACE_ENTRIES: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Idle,
    Classifying,
    Planning,
    Running,
    Verifying,
    Recovering,
    AwaitingConfirmation,
    Paused,
    Completed,
    Failed,
    Stopped,
}

impl TaskPhase {
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Classifying
                | Self::Planning
                | Self::Running
                | Self::Verifying
                | Self::Recovering
                | Self::AwaitingConfirmation
                | Self::Paused
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationTraceEntry {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub stage: String,
    pub role: String,
    pub model: String,
    pub title: String,
    pub content: String,
    pub step_number: Option<u32>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingConfirmation {
    pub id: String,
    pub step_number: u32,
    pub action_type: String,
    pub description: String,
    pub narration: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    pub phase: TaskPhase,
    pub task_id: Option<String>,
    pub goal: String,
    pub classification: Option<TaskClassification>,
    pub active_role: String,
    pub active_model: String,
    pub current_thought: String,
    pub plan_steps: Vec<String>,
    pub active_plan_index: u32,
    pub current_step: u32,
    pub max_steps: u32,
    pub recovery_count: u32,
    pub last_diagnosis: Option<String>,
    pub active_app: String,
    pub last_narration: String,
    pub last_summary: Option<String>,
    pub last_error: Option<String>,
    pub pending_confirmation: Option<PendingConfirmation>,
    pub screen_permission_granted: bool,
    pub accessibility_permission_granted: bool,
    pub steps: Vec<TaskStepRecord>,
    pub traces: Vec<OrchestrationTraceEntry>,
}

struct TaskRuntimeState {
    phase: TaskPhase,
    task_id: Option<String>,
    goal: String,
    started_at: Option<DateTime<Utc>>,
    classification: Option<TaskClassification>,
    active_role: String,
    active_model: String,
    current_thought: String,
    plan_steps: Vec<String>,
    active_plan_index: u32,
    current_step: u32,
    max_steps: u32,
    recovery_count: u32,
    last_diagnosis: Option<String>,
    active_app: String,
    last_narration: String,
    last_summary: Option<String>,
    last_error: Option<String>,
    pending_confirmation: Option<PendingConfirmation>,
    steps: Vec<TaskStepRecord>,
    traces: Vec<OrchestrationTraceEntry>,
    history_saved: bool,
}

impl TaskRuntimeState {
    fn idle(max_steps: u32) -> Self {
        Self {
            phase: TaskPhase::Idle,
            task_id: None,
            goal: String::new(),
            started_at: None,
            classification: None,
            active_role: String::new(),
            active_model: String::new(),
            current_thought: String::new(),
            plan_steps: Vec::new(),
            active_plan_index: 0,
            current_step: 0,
            max_steps,
            recovery_count: 0,
            last_diagnosis: None,
            active_app: String::new(),
            last_narration: String::new(),
            last_summary: None,
            last_error: None,
            pending_confirmation: None,
            steps: Vec::new(),
            traces: Vec::new(),
            history_saved: false,
        }
    }
}

struct GroundedCapture {
    full_res_rgba: RgbaImage,
    annotated_jpeg: String,
    active_window: ActiveWindowMetadata,
    screen_metrics: ScreenMetrics,
    target_map: ScreenTargetMap,
}

pub struct TaskEngine {
    config: Mutex<AppConfig>,
    state: Mutex<TaskRuntimeState>,
    stop_tx: Mutex<Option<watch::Sender<bool>>>,
    pause_tx: Mutex<Option<watch::Sender<bool>>>,
    confirm_tx: Mutex<Option<(String, oneshot::Sender<bool>)>>,
    client: OllamaAgentClient,
}

impl TaskEngine {
    pub fn new(initial_config: AppConfig) -> Result<Arc<Self>, AgentError> {
        Ok(Arc::new(Self {
            config: Mutex::new(initial_config),
            state: Mutex::new(TaskRuntimeState::idle(MAX_STEPS)),
            stop_tx: Mutex::new(None),
            pause_tx: Mutex::new(None),
            confirm_tx: Mutex::new(None),
            client: OllamaAgentClient::new()?,
        }))
    }

    pub fn config(&self) -> AppConfig {
        self.config.lock().clone()
    }

    pub fn update_config(&self, next: AppConfig) {
        *self.config.lock() = next;
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let st = self.state.lock();
        EngineSnapshot {
            phase: st.phase,
            task_id: st.task_id.clone(),
            goal: st.goal.clone(),
            classification: st.classification.clone(),
            active_role: st.active_role.clone(),
            active_model: st.active_model.clone(),
            current_thought: st.current_thought.clone(),
            plan_steps: st.plan_steps.clone(),
            active_plan_index: st.active_plan_index,
            current_step: st.current_step,
            max_steps: st.max_steps,
            recovery_count: st.recovery_count,
            last_diagnosis: st.last_diagnosis.clone(),
            active_app: st.active_app.clone(),
            last_narration: st.last_narration.clone(),
            last_summary: st.last_summary.clone(),
            last_error: st.last_error.clone(),
            pending_confirmation: st.pending_confirmation.clone(),
            screen_permission_granted: has_screen_recording_permission(),
            accessibility_permission_granted: has_accessibility_permission(),
            steps: st.steps.clone(),
            traces: st.traces.clone(),
        }
    }

    pub fn start_task(self: &Arc<Self>, app_handle: AppHandle, goal: String) -> EngineSnapshot {
        self.stop_task(&app_handle, Some("Replaced by new task".to_string()));

        let task_id = Uuid::new_v4().to_string();
        let cfg = self.config();

        {
            let mut st = self.state.lock();
            st.phase = TaskPhase::Classifying;
            st.task_id = Some(task_id.clone());
            st.goal = goal.clone();
            st.started_at = Some(Utc::now());
            st.classification = None;
            st.active_role = "judge".to_string();
            st.active_model = cfg.models.judge_model.clone();
            st.current_thought = "Analyzing goal and screen context...".to_string();
            st.plan_steps = Vec::new();
            st.active_plan_index = 0;
            st.current_step = 0;
            st.max_steps = MAX_STEPS;
            st.recovery_count = 0;
            st.last_diagnosis = None;
            st.active_app = String::new();
            st.last_narration = "Judge model classifying task and selecting specialist...".to_string();
            st.last_summary = None;
            st.last_error = None;
            st.pending_confirmation = None;
            st.steps.clear();
            st.traces.clear();
            st.history_saved = false;
        }

        let (stop_tx, stop_rx) = watch::channel(false);
        let (pause_tx, pause_rx) = watch::channel(false);
        *self.stop_tx.lock() = Some(stop_tx);
        *self.pause_tx.lock() = Some(pause_tx);
        *self.confirm_tx.lock() = None;

        if cfg.models.auto_open_panel {
            show_orchestration_panel(&app_handle);
        }

        let snap = self.snapshot();
        update_tray_status_title(&app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);

        let engine_clone = Arc::clone(self);
        let handle_clone = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            engine_clone
                .run_task_loop(handle_clone, task_id, goal, stop_rx, pause_rx)
                .await;
        });

        snap
    }

    pub fn pause_task(&self, app_handle: &AppHandle) -> EngineSnapshot {
        {
            let mut st = self.state.lock();
            if st.phase == TaskPhase::Running {
                st.phase = TaskPhase::Paused;
                st.last_narration = "Paused task execution.".to_string();
            }
        }
        if let Some(tx) = self.pause_tx.lock().as_ref() {
            let _ = tx.send(true);
        }
        let snap = self.snapshot();
        update_tray_status_title(app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);
        snap
    }

    pub fn resume_task(&self, app_handle: &AppHandle) -> EngineSnapshot {
        {
            let mut st = self.state.lock();
            if st.phase == TaskPhase::Paused {
                st.phase = TaskPhase::Running;
                st.last_narration = "Resuming task execution...".to_string();
            }
        }
        if let Some(tx) = self.pause_tx.lock().as_ref() {
            let _ = tx.send(false);
        }
        let snap = self.snapshot();
        update_tray_status_title(app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);
        snap
    }

    pub fn stop_task(&self, app_handle: &AppHandle, reason: Option<String>) -> EngineSnapshot {
        if let Some(tx) = self.stop_tx.lock().take() {
            let _ = tx.send(true);
        }
        if let Some((_, confirm)) = self.confirm_tx.lock().take() {
            let _ = confirm.send(false);
        }

        let record_to_save = {
            let mut st = self.state.lock();
            if st.phase.is_active() {
                let msg = reason.unwrap_or_else(|| "Task stopped by user".to_string());
                st.phase = TaskPhase::Stopped;
                st.pending_confirmation = None;
                st.last_summary = Some(msg.clone());
                st.last_narration = msg;
                take_history_record_locked(&mut st, "stopped")
            } else {
                None
            }
        };

        if let Some(record) = record_to_save {
            tauri::async_runtime::spawn_blocking(move || {
                let _ = append_history_record(record);
            });
        }

        let snap = self.snapshot();
        update_tray_status_title(app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);
        snap
    }

    pub fn resolve_confirmation(
        &self,
        app_handle: &AppHandle,
        confirmation_id: &str,
        approved: bool,
    ) -> Result<EngineSnapshot, String> {
        let sender = {
            let mut confirm_guard = self.confirm_tx.lock();
            match confirm_guard.take() {
                Some((id, tx)) if id == confirmation_id => tx,
                Some(existing) => {
                    *confirm_guard = Some(existing);
                    return Err("confirmation id does not match active prompt".to_string());
                }
                None => return Err("no confirmation is currently pending".to_string()),
            }
        };

        {
            let mut st = self.state.lock();
            st.pending_confirmation = None;
            if st.phase == TaskPhase::AwaitingConfirmation {
                st.phase = TaskPhase::Running;
                st.last_narration = if approved {
                    "Action approved—executing now.".to_string()
                } else {
                    "Action skipped—replanning next step.".to_string()
                };
            }
        }

        let _ = sender.send(approved);
        let snap = self.snapshot();
        update_tray_status_title(app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);
        Ok(snap)
    }

    fn is_current_task(&self, task_id: &str) -> bool {
        self.state
            .lock()
            .task_id
            .as_deref()
            .is_some_and(|id| id == task_id)
    }

    async fn run_task_loop(
        self: Arc<Self>,
        app_handle: AppHandle,
        task_id: String,
        goal: String,
        mut stop_rx: watch::Receiver<bool>,
        mut pause_rx: watch::Receiver<bool>,
    ) {
        let kill_engine = Arc::clone(&self);
        let kill_handle = app_handle.clone();
        let kill_task_id = task_id.clone();
        let mut kill_stop_rx = stop_rx.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    _ = kill_stop_rx.changed() => {
                        if *kill_stop_rx.borrow() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(80)) => {
                        if *kill_stop_rx.borrow() || !kill_engine.is_current_task(&kill_task_id) {
                            break;
                        }
                        if is_kill_switch_pressed() {
                            kill_engine.stop_task(
                                &kill_handle,
                                Some("Stopped by emergency shortcut (⌃⌥S)".to_string()),
                            );
                            show_tray_popover(&kill_handle, false);
                            break;
                        }
                    }
                }
            }
        });

        hide_tray_popover(&app_handle);
        if sleep_interruptible(Duration::from_millis(60), &mut stop_rx).await {
            return;
        }

        let routing = self.config().models;
        let max_steps = MAX_STEPS;
        let mut plan_steps: Vec<String> = Vec::new();
        let mut recent_steps: Vec<String> = Vec::new();
        let mut stalled_action_signatures: Vec<String> = Vec::new();
        let mut all_action_signatures: Vec<String> = Vec::new();
        let mut last_pointer_pos: Option<(f64, f64)> = None;
        let mut consecutive_agent_errors: u32 = 0;
        let mut consecutive_stalls: u32 = 0;
        let mut consecutive_waits: u32 = 0;
        let mut recovery_count: u32 = 0;
        let mut done_rejections: u32 = 0;
        let mut last_opened_url: Option<String> = None;
        let mut last_action_was_open_url = false;
        let mut cached_capture: Option<GroundedCapture>;
        let mut step_num: u32 = 0;

        let initial_cap = match tokio::task::spawn_blocking(capture_and_ground_screen).await {
            Ok(Ok(cap)) => cap,
            Ok(Err(err)) => {
                self.finalize_task(
                    &app_handle,
                    &task_id,
                    TaskPhase::Failed,
                    "failed",
                    err.to_string(),
                    Some(err.to_string()),
                );
                show_tray_popover(&app_handle, false);
                return;
            }
            Err(err) => {
                self.finalize_task(
                    &app_handle,
                    &task_id,
                    TaskPhase::Failed,
                    "failed",
                    err.to_string(),
                    Some(err.to_string()),
                );
                show_tray_popover(&app_handle, false);
                return;
            }
        };

        if initial_cap.active_window.is_system_locked {
            self.finalize_task(
                &app_handle,
                &task_id,
                TaskPhase::Stopped,
                "stopped",
                "Stopped because macOS screen locked".to_string(),
                None,
            );
            return;
        }

        if *stop_rx.borrow() || !self.is_current_task(&task_id) {
            return;
        }

        let initial_api_key = self.config().resolved_api_key();
        self.set_active_stage(
            &app_handle,
            &task_id,
            TaskPhase::Classifying,
            "judge",
            &routing.judge_model,
            Some("Judge model analyzing goal and screen...".to_string()),
        );

        let classify_ctx = ClassifyTaskContext {
            goal: goal.clone(),
            active_app: initial_cap.active_window.app_name.clone(),
            window_title: initial_cap.active_window.window_title.clone(),
            ocr_lines: initial_cap.target_map.formatted_lines.clone(),
            base64_jpeg: initial_cap.annotated_jpeg.clone(),
        };

        let mut judge_model_used = routing.judge_model.clone();
        let classification = match self
            .client
            .classify_task(
                OLLAMA_BASE_URL,
                &initial_api_key,
                &routing.judge_model,
                classify_ctx.clone(),
            )
            .await
        {
            Ok(cls) => cls,
            Err(primary_err) => {
                if routing.judge_model != routing.vision_actor_model {
                    self.push_trace(
                        &app_handle,
                        &task_id,
                        OrchestrationTraceEntry {
                            id: Uuid::new_v4().to_string(),
                            timestamp: Utc::now(),
                            stage: "fallback".to_string(),
                            role: "judge".to_string(),
                            model: routing.judge_model.clone(),
                            title: "Judge model fallback".to_string(),
                            content: format!(
                                "Judge {} unavailable ({}); retrying with {}.",
                                routing.judge_model, primary_err, routing.vision_actor_model
                            ),
                            step_number: None,
                            status: "warning".to_string(),
                        },
                    );
                    match self
                        .client
                        .classify_task(
                            OLLAMA_BASE_URL,
                            &initial_api_key,
                            &routing.vision_actor_model,
                            classify_ctx,
                        )
                        .await
                    {
                        Ok(cls) => {
                            judge_model_used = routing.vision_actor_model.clone();
                            cls
                        }
                        Err(fallback_err) => {
                            self.push_trace(
                                &app_handle,
                                &task_id,
                                OrchestrationTraceEntry {
                                    id: Uuid::new_v4().to_string(),
                                    timestamp: Utc::now(),
                                    stage: "classify".to_string(),
                                    role: "judge".to_string(),
                                    model: routing.vision_actor_model.clone(),
                                    title: "Heuristic classification fallback".to_string(),
                                    content: format!(
                                        "Cloud judge unavailable ({fallback_err}); using local classification."
                                    ),
                                    step_number: None,
                                    status: "warning".to_string(),
                                },
                            );
                            fallback_classification(&goal)
                        }
                    }
                } else {
                    self.push_trace(
                        &app_handle,
                        &task_id,
                        OrchestrationTraceEntry {
                            id: Uuid::new_v4().to_string(),
                            timestamp: Utc::now(),
                            stage: "classify".to_string(),
                            role: "judge".to_string(),
                            model: routing.judge_model.clone(),
                            title: "Heuristic classification fallback".to_string(),
                            content: format!(
                                "Cloud judge unavailable ({primary_err}); using local classification."
                            ),
                            step_number: None,
                            status: "warning".to_string(),
                        },
                    );
                    fallback_classification(&goal)
                }
            }
        };

        if *stop_rx.borrow() || !self.is_current_task(&task_id) {
            return;
        }

        let (actor_role, actor_model) = select_actor_for_classification(&classification, &routing);
        {
            let mut st = self.state.lock();
            if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                st.classification = Some(classification.clone());
                st.active_app = initial_cap.active_window.app_name.clone();
            }
        }

        self.push_trace(
            &app_handle,
            &task_id,
            OrchestrationTraceEntry {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now(),
                stage: "classify".to_string(),
                role: "judge".to_string(),
                model: judge_model_used.clone(),
                title: format!(
                    "Classified: {} ({}) -> {}",
                    classification.category, classification.complexity, actor_model
                ),
                content: classification.reasoning.clone(),
                step_number: None,
                status: "success".to_string(),
            },
        );

        if classification.needs_dedicated_planner || classification.initial_plan_steps.is_empty() {
            self.set_active_stage(
                &app_handle,
                &task_id,
                TaskPhase::Planning,
                "planner",
                &routing.planner_model,
                Some("Planner model building execution steps...".to_string()),
            );

            let plan_ctx = CreatePlanContext {
                goal: goal.clone(),
                category: classification.category.clone(),
                complexity: classification.complexity.clone(),
                judge_reasoning: classification.reasoning.clone(),
                active_app: initial_cap.active_window.app_name.clone(),
                window_title: initial_cap.active_window.window_title.clone(),
                ocr_lines: initial_cap.target_map.formatted_lines.clone(),
                base64_jpeg: initial_cap.annotated_jpeg.clone(),
            };

            let mut planner_model_used = routing.planner_model.clone();
            let plan_res = match self
                .client
                .create_task_plan(
                    OLLAMA_BASE_URL,
                    &initial_api_key,
                    &routing.planner_model,
                    plan_ctx.clone(),
                )
                .await
            {
                Ok(p) => Ok(p),
                Err(err) => {
                    if routing.planner_model != routing.vision_actor_model {
                        self.push_trace(
                            &app_handle,
                            &task_id,
                            OrchestrationTraceEntry {
                                id: Uuid::new_v4().to_string(),
                                timestamp: Utc::now(),
                                stage: "fallback".to_string(),
                                role: "planner".to_string(),
                                model: routing.planner_model.clone(),
                                title: "Planner model fallback".to_string(),
                                content: format!(
                                    "Planner {} failed ({}); retrying with {}.",
                                    routing.planner_model, err, routing.vision_actor_model
                                ),
                                step_number: None,
                                status: "warning".to_string(),
                            },
                        );
                        planner_model_used = routing.vision_actor_model.clone();
                        self.client
                            .create_task_plan(
                                OLLAMA_BASE_URL,
                                &initial_api_key,
                                &routing.vision_actor_model,
                                plan_ctx,
                            )
                            .await
                    } else {
                        Err(err)
                    }
                }
            };

            match plan_res {
                Ok(plan) => {
                    plan_steps = plan.steps.clone();
                    {
                        let mut st = self.state.lock();
                        if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                            st.plan_steps = plan_steps.clone();
                        }
                    }
                    let summary = if plan.thinking.is_empty() {
                        plan_steps
                            .iter()
                            .enumerate()
                            .map(|(i, s)| format!("{}. {s}", i + 1))
                            .collect::<Vec<_>>()
                            .join(" | ")
                    } else {
                        format!(
                            "{} — {}",
                            plan.thinking,
                            plan_steps
                                .iter()
                                .enumerate()
                                .map(|(i, s)| format!("{}. {s}", i + 1))
                                .collect::<Vec<_>>()
                                .join(" | ")
                        )
                    };
                    self.push_trace(
                        &app_handle,
                        &task_id,
                        OrchestrationTraceEntry {
                            id: Uuid::new_v4().to_string(),
                            timestamp: Utc::now(),
                            stage: "plan".to_string(),
                            role: "planner".to_string(),
                            model: planner_model_used,
                            title: format!("Plan ready ({} steps)", plan_steps.len()),
                            content: summary,
                            step_number: None,
                            status: "success".to_string(),
                        },
                    );
                }
                Err(err) => {
                    if !classification.initial_plan_steps.is_empty() {
                        plan_steps = classification.initial_plan_steps.clone();
                        {
                            let mut st = self.state.lock();
                            if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                                st.plan_steps = plan_steps.clone();
                            }
                        }
                    }
                    self.push_trace(
                        &app_handle,
                        &task_id,
                        OrchestrationTraceEntry {
                            id: Uuid::new_v4().to_string(),
                            timestamp: Utc::now(),
                            stage: "plan".to_string(),
                            role: "planner".to_string(),
                            model: planner_model_used,
                            title: "Using Judge initial plan".to_string(),
                            content: format!("Dedicated planner skipped ({err})."),
                            step_number: None,
                            status: "warning".to_string(),
                        },
                    );
                }
            }
        } else {
            plan_steps = classification.initial_plan_steps.clone();
            {
                let mut st = self.state.lock();
                if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                    st.plan_steps = plan_steps.clone();
                }
            }
            self.push_trace(
                &app_handle,
                &task_id,
                OrchestrationTraceEntry {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    stage: "plan".to_string(),
                    role: "judge".to_string(),
                    model: judge_model_used,
                    title: format!("Fast-path plan ({} steps)", plan_steps.len()),
                    content: plan_steps
                        .iter()
                        .enumerate()
                        .map(|(i, s)| format!("{}. {s}", i + 1))
                        .collect::<Vec<_>>()
                        .join(" | "),
                    step_number: None,
                    status: "success".to_string(),
                },
            );
        }

        if *stop_rx.borrow() || !self.is_current_task(&task_id) {
            return;
        }

        cached_capture = Some(initial_cap);
        self.set_active_stage(
            &app_handle,
            &task_id,
            TaskPhase::Running,
            actor_role,
            &actor_model,
            Some(format!("Running with {actor_model}...")),
        );

        while step_num < max_steps {
            if *stop_rx.borrow() || !self.is_current_task(&task_id) {
                return;
            }

            while *pause_rx.borrow() {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            return;
                        }
                    }
                    _ = pause_rx.changed() => {}
                }
            }

            if !self.is_current_task(&task_id) {
                return;
            }

            let cycle_api_key = self.config().resolved_api_key();

            let before_cap = match cached_capture.take() {
                Some(cap) => cap,
                None => {
                    let capture_res = tokio::task::spawn_blocking(capture_and_ground_screen).await;
                    match capture_res {
                        Ok(Ok(cap)) => cap,
                        Ok(Err(err)) => {
                            self.finalize_task(
                                &app_handle,
                                &task_id,
                                TaskPhase::Failed,
                                "failed",
                                err.to_string(),
                                Some(err.to_string()),
                            );
                            show_tray_popover(&app_handle, false);
                            return;
                        }
                        Err(err) => {
                            self.finalize_task(
                                &app_handle,
                                &task_id,
                                TaskPhase::Failed,
                                "failed",
                                err.to_string(),
                                Some(err.to_string()),
                            );
                            show_tray_popover(&app_handle, false);
                            return;
                        }
                    }
                }
            };

            if before_cap.active_window.is_system_locked {
                self.finalize_task(
                    &app_handle,
                    &task_id,
                    TaskPhase::Stopped,
                    "stopped",
                    "Stopped because macOS screen locked".to_string(),
                    None,
                );
                return;
            }

            if *stop_rx.borrow() || !self.is_current_task(&task_id) {
                return;
            }

            self.set_active_stage(
                &app_handle,
                &task_id,
                TaskPhase::Running,
                actor_role,
                &actor_model,
                None,
            );

            let next_ctx = NextActionContext {
                goal: goal.clone(),
                task_category: classification.category.clone(),
                actor_role: actor_role.to_string(),
                plan_steps: plan_steps.clone(),
                step_number: step_num + 1,
                max_steps,
                active_app: before_cap.active_window.app_name.clone(),
                window_title: before_cap.active_window.window_title.clone(),
                screen_width: before_cap.screen_metrics.logical_width.round() as u32,
                screen_height: before_cap.screen_metrics.logical_height.round() as u32,
                ocr_lines: before_cap.target_map.formatted_lines.clone(),
                recent_steps: recent_steps.clone(),
                base64_jpeg: before_cap.annotated_jpeg.clone(),
            };

            let mut used_actor_model = actor_model.clone();
            let decision_res = match self
                .client
                .decide_next_action(OLLAMA_BASE_URL, &cycle_api_key, &actor_model, next_ctx.clone())
                .await
            {
                Ok(d) => Ok(d),
                Err(err) if actor_model != routing.vision_actor_model => {
                    self.push_trace(
                        &app_handle,
                        &task_id,
                        OrchestrationTraceEntry {
                            id: Uuid::new_v4().to_string(),
                            timestamp: Utc::now(),
                            stage: "fallback".to_string(),
                            role: actor_role.to_string(),
                            model: actor_model.clone(),
                            title: format!("Actor fallback at step {}", step_num + 1),
                            content: format!(
                                "Model {} errored ({}); retrying with {}.",
                                actor_model, err, routing.vision_actor_model
                            ),
                            step_number: Some(step_num + 1),
                            status: "warning".to_string(),
                        },
                    );
                    used_actor_model = routing.vision_actor_model.clone();
                    self.client
                        .decide_next_action(
                            OLLAMA_BASE_URL,
                            &cycle_api_key,
                            &routing.vision_actor_model,
                            next_ctx,
                        )
                        .await
                }
                Err(err) => Err(err),
            };

            let decision = match decision_res {
                Ok(d) => {
                    consecutive_agent_errors = 0;
                    d
                }
                Err(err) => {
                    consecutive_agent_errors += 1;
                    step_num += 1;
                    let msg = err.to_string();
                    if consecutive_agent_errors < 3 && step_num < max_steps {
                        let retry_rec = TaskStepRecord {
                            step_number: step_num,
                            timestamp: Utc::now(),
                            action_type: "agent_retry".to_string(),
                            description: "Retrying model response".to_string(),
                            narration: "Re-checking the screen...".to_string(),
                            status: "retrying".to_string(),
                            target_ocr_id: None,
                            target_x: None,
                            target_y: None,
                            verification_note: None,
                            model: Some(used_actor_model.clone()),
                            thought: Some(msg.clone()),
                        };
                        self.record_step(
                            &app_handle,
                            &task_id,
                            step_num,
                            0,
                            &before_cap.active_window.app_name,
                            retry_rec,
                        );
                        push_recent_step(
                            &mut recent_steps,
                            format!("Step {step_num} model error (retrying): {msg}"),
                        );
                        cached_capture = Some(before_cap);
                        if sleep_interruptible(Duration::from_millis(300), &mut stop_rx).await {
                            return;
                        }
                        continue;
                    }
                    self.finalize_task(
                        &app_handle,
                        &task_id,
                        TaskPhase::Failed,
                        "failed",
                        msg.clone(),
                        Some(msg),
                    );
                    show_tray_popover(&app_handle, false);
                    return;
                }
            };

            if *stop_rx.borrow() || !self.is_current_task(&task_id) {
                return;
            }

            let decision_thought = if decision.thought.is_empty() {
                decision.narration.clone()
            } else {
                decision.thought.clone()
            };
            self.push_trace(
                &app_handle,
                &task_id,
                OrchestrationTraceEntry {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    stage: "think".to_string(),
                    role: actor_role.to_string(),
                    model: used_actor_model.clone(),
                    title: format!("Step {} · {}", step_num + 1, decision.action_type),
                    content: decision_thought,
                    step_number: Some(step_num + 1),
                    status: "info".to_string(),
                },
            );

            if plan_steps.is_empty() && !decision.plan_steps.is_empty() {
                plan_steps = decision.plan_steps.clone();
                {
                    let mut st = self.state.lock();
                    if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                        st.plan_steps = plan_steps.clone();
                        st.active_app = before_cap.active_window.app_name.clone();
                        st.phase = TaskPhase::Running;
                    }
                }
                let plan_snap = self.snapshot();
                update_tray_status_title(&app_handle, Some(&plan_snap));
                let _ = app_handle.emit("engine-snapshot", &plan_snap);
            } else {
                let mut st = self.state.lock();
                if st.task_id.as_deref() == Some(&task_id) && st.phase == TaskPhase::Planning {
                    st.phase = TaskPhase::Running;
                }
            }

            let metrics = before_cap.screen_metrics;
            let action_queue = build_turn_action_queue(decision);
            let queue_len = action_queue.len();

            let mut executed_count: u32 = 0;
            let mut last_executed_action: Option<AgentAction> = None;
            let mut last_action_desc = String::new();
            let mut last_action_output_note: Option<String> = None;
            let mut last_err_text: Option<String> = None;
            let mut last_cue_pos: (f64, f64) = (metrics.logical_width * 0.5, 56.0);
            let mut batch_repetition_warned = false;
            let mut turn_signatures: Vec<String> = Vec::new();

            for (batch_idx, item_decision) in action_queue.into_iter().enumerate() {
                if step_num >= max_steps || *stop_rx.borrow() || !self.is_current_task(&task_id) {
                    break;
                }

                while *pause_rx.borrow() {
                    tokio::select! {
                        _ = stop_rx.changed() => {
                            if *stop_rx.borrow() {
                                return;
                            }
                        }
                        _ = pause_rx.changed() => {}
                    }
                }

                if !self.is_current_task(&task_id) {
                    return;
                }

                step_num += 1;

                let (action, (overlay_x, overlay_y), target_dims, target_text) =
                    match resolve_action_from_decision(
                        &item_decision,
                        &before_cap.target_map,
                        metrics,
                    ) {
                        Ok(v) => v,
                        Err(reason) => {
                            let step_rec = TaskStepRecord {
                                step_number: step_num,
                                timestamp: Utc::now(),
                                action_type: item_decision.action_type.clone(),
                                description: format!("Invalid target: {reason}"),
                                narration: item_decision.narration.clone(),
                                status: "skipped".to_string(),
                                target_ocr_id: item_decision.target_ocr_id,
                                target_x: None,
                                target_y: None,
                                verification_note: Some(reason.clone()),
                                model: Some(used_actor_model.clone()),
                                thought: (!item_decision.thought.is_empty())
                                    .then(|| item_decision.thought.clone()),
                            };
                            self.record_step(
                                &app_handle,
                                &task_id,
                                step_num,
                                item_decision.plan_step_index,
                                &before_cap.active_window.app_name,
                                step_rec,
                            );
                            push_recent_step(
                                &mut recent_steps,
                                format!(
                                    "Step {step_num} skipped: {reason}. Pick an exact [N] ID from Visible targets or valid x_norm/y_norm."
                                ),
                            );
                            break;
                        }
                    };

                let action_label = action_type_label(&action).to_string();
                let action_desc = describe_action(&action);
                let is_terminal =
                    matches!(action, AgentAction::Done { .. } | AgentAction::Fail { .. });

                if !is_terminal
                    && is_duplicate_url_action(
                        &action,
                        last_opened_url.as_deref(),
                        last_action_was_open_url,
                    )
                {
                    let step_rec = TaskStepRecord {
                        step_number: step_num,
                        timestamp: Utc::now(),
                        action_type: action_label,
                        description: action_desc,
                        narration: "Already opened this URL—advancing instead of duplicating."
                            .to_string(),
                        status: "skipped".to_string(),
                        target_ocr_id: item_decision.target_ocr_id,
                        target_x: None,
                        target_y: None,
                        verification_note: Some("Duplicate URL entry blocked".to_string()),
                        model: Some(used_actor_model.clone()),
                        thought: (!item_decision.thought.is_empty())
                            .then(|| item_decision.thought.clone()),
                    };
                    self.record_step(
                        &app_handle,
                        &task_id,
                        step_num,
                        item_decision.plan_step_index,
                        &before_cap.active_window.app_name,
                        step_rec,
                    );
                    last_action_was_open_url = false;
                    push_recent_step(
                        &mut recent_steps,
                        "DUPLICATE URL BLOCKED: Already opened/entered this URL—wait or proceed to the next step (e.g. scroll or done)."
                            .to_string(),
                    );
                    break;
                }

                let action_sig = format!(
                    "{action_label}|{}|{action_desc}",
                    item_decision.target_ocr_id.unwrap_or(0)
                );
                let mut repetition_warned = false;

                if !is_terminal {
                    let rep_level = if matches!(action, AgentAction::Wait { .. }) {
                        if consecutive_waits >= 6 {
                            RepetitionLevel::Abort
                        } else if consecutive_waits >= 4 {
                            RepetitionLevel::Warn
                        } else {
                            RepetitionLevel::None
                        }
                    } else {
                        let stalled_level =
                            detect_repetition_level(&stalled_action_signatures, &action_sig);
                        if stalled_level == RepetitionLevel::None
                            && !is_naturally_repeatable_action(&action)
                            && detect_progressing_cycle_abort(&all_action_signatures, &action_sig)
                        {
                            RepetitionLevel::Abort
                        } else {
                            stalled_level
                        }
                    };

                    match rep_level {
                        RepetitionLevel::Abort => {
                            let msg = if matches!(action, AgentAction::Wait { .. }) {
                                "Stopped: waited repeatedly without the page or app changing."
                                    .to_string()
                            } else {
                                "Stopped: repeated the same action without making progress."
                                    .to_string()
                            };
                            let cue = PointerCuePayload {
                                id: Uuid::new_v4().to_string(),
                                message: msg.clone(),
                                action_badge: "Loop Stopped".to_string(),
                                target_x: metrics.logical_width * 0.5,
                                target_y: 56.0,
                                target_width: None,
                                target_height: None,
                                target_ocr_id: None,
                                step_number: step_num,
                                is_acting: false,
                                show_pointer: false,
                                dismiss_after_ms: 4200,
                            };
                            let _ = emit_pointer_cue(&app_handle, cue);
                            let step_rec = TaskStepRecord {
                                step_number: step_num,
                                timestamp: Utc::now(),
                                action_type: action_label,
                                description: action_desc,
                                narration: msg.clone(),
                                status: "failed".to_string(),
                                target_ocr_id: item_decision.target_ocr_id,
                                target_x: None,
                                target_y: None,
                                verification_note: Some("Loop abort".to_string()),
                                model: Some(used_actor_model.clone()),
                                thought: (!item_decision.thought.is_empty())
                                    .then(|| item_decision.thought.clone()),
                            };
                            self.record_step(
                                &app_handle,
                                &task_id,
                                step_num,
                                item_decision.plan_step_index,
                                &before_cap.active_window.app_name,
                                step_rec,
                            );
                            self.finalize_task(
                                &app_handle,
                                &task_id,
                                TaskPhase::Failed,
                                "failed",
                                msg.clone(),
                                Some(msg),
                            );
                            show_tray_popover(&app_handle, false);
                            return;
                        }
                        RepetitionLevel::Warn => {
                            repetition_warned = true;
                            batch_repetition_warned = true;
                            push_recent_step(
                                &mut recent_steps,
                                "LOOP WARNING: You repeated this action without changing the UI. Use open_url with app_name, paste_text with replace=true, or press_key cmd+l."
                                    .to_string(),
                            );
                        }
                        RepetitionLevel::None => {}
                    }
                }

                let has_explicit_target = item_decision.target_ocr_id.is_some()
                    || (item_decision.x_norm.is_some() && item_decision.y_norm.is_some())
                    || (item_decision.from_x_norm.is_some() && item_decision.from_y_norm.is_some());
                let show_pointer =
                    matches!(action, AgentAction::Click { .. } | AgentAction::Drag { .. })
                        || (matches!(action, AgentAction::Scroll { .. }) && has_explicit_target);

                let (cue_x, cue_y) = if show_pointer {
                    last_pointer_pos = Some((overlay_x, overlay_y));
                    (overlay_x, overlay_y)
                } else {
                    match last_pointer_pos {
                        Some((lx, ly)) => (
                            lx.clamp(180.0, (metrics.logical_width - 180.0).max(180.0)),
                            ly.clamp(44.0, (metrics.logical_height - 120.0).max(44.0)),
                        ),
                        None => (metrics.logical_width * 0.5, 56.0),
                    }
                };
                last_cue_pos = (cue_x, cue_y);

                let (target_width, target_height) = if show_pointer {
                    match target_dims {
                        Some((w, h)) => (Some(w), Some(h)),
                        None => (None, None),
                    }
                } else {
                    (None, None)
                };

                match &action {
                    AgentAction::Done { summary } => {
                        if routing.enable_judge_verification && done_rejections < 2 {
                            self.set_active_stage(
                                &app_handle,
                                &task_id,
                                TaskPhase::Verifying,
                                "judge",
                                &routing.judge_model,
                                Some("Judge verifying task completion...".to_string()),
                            );
                            let verify_ctx = VerifyStepContext {
                                goal: goal.clone(),
                                step_number: step_num,
                                action_description: action_desc.clone(),
                                expected_narration: summary.clone(),
                                before_app: before_cap.active_window.app_name.clone(),
                                after_app: before_cap.active_window.app_name.clone(),
                                before_title: before_cap.active_window.window_title.clone(),
                                after_title: before_cap.active_window.window_title.clone(),
                                visual_diff_pct: 0.0,
                                action_output: Some(summary.clone()),
                                visible_targets: before_cap.target_map.formatted_lines.clone(),
                                after_base64_jpeg: before_cap.annotated_jpeg.clone(),
                            };
                            let mut judge_ver_model = routing.judge_model.clone();
                            let judge_res = match self
                                .client
                                .verify_step_outcome(
                                    OLLAMA_BASE_URL,
                                    &cycle_api_key,
                                    &routing.judge_model,
                                    verify_ctx.clone(),
                                )
                                .await
                            {
                                Ok(v) => Ok(v),
                                Err(_) if routing.judge_model != routing.vision_actor_model => {
                                    judge_ver_model = routing.vision_actor_model.clone();
                                    self.client
                                        .verify_step_outcome(
                                            OLLAMA_BASE_URL,
                                            &cycle_api_key,
                                            &routing.vision_actor_model,
                                            verify_ctx,
                                        )
                                        .await
                                }
                                Err(e) => Err(e),
                            };
                            if let Ok(ver) = judge_res {
                                if !ver.goal_achieved {
                                    done_rejections += 1;
                                    let hint = ver
                                        .recovery_hint
                                        .clone()
                                        .unwrap_or_else(|| "Continue working on the remaining goal.".to_string());
                                    self.push_trace(
                                        &app_handle,
                                        &task_id,
                                        OrchestrationTraceEntry {
                                            id: Uuid::new_v4().to_string(),
                                            timestamp: Utc::now(),
                                            stage: "verify".to_string(),
                                            role: "judge".to_string(),
                                            model: judge_ver_model,
                                            title: format!("Judge rejected completion (check #{done_rejections})"),
                                            content: format!("{} Hint: {}", ver.observation, hint),
                                            step_number: Some(step_num),
                                            status: "warning".to_string(),
                                        },
                                    );
                                    push_recent_step(
                                        &mut recent_steps,
                                        format!(
                                            "JUDGE VERIFICATION REJECTED DONE: {}. Hint: {}. Continue until goal is actually complete.",
                                            ver.observation, hint
                                        ),
                                    );
                                    let step_rec = TaskStepRecord {
                                        step_number: step_num,
                                        timestamp: Utc::now(),
                                        action_type: action_label,
                                        description: action_desc,
                                        narration: item_decision.narration.clone(),
                                        status: "retrying".to_string(),
                                        target_ocr_id: item_decision.target_ocr_id,
                                        target_x: Some(cue_x),
                                        target_y: Some(cue_y),
                                        verification_note: Some(format!("Judge rejected Done: {}", ver.observation)),
                                        model: Some(used_actor_model.clone()),
                                        thought: (!item_decision.thought.is_empty())
                                            .then(|| item_decision.thought.clone()),
                                    };
                                    self.record_step(
                                        &app_handle,
                                        &task_id,
                                        step_num,
                                        item_decision.plan_step_index,
                                        &before_cap.active_window.app_name,
                                        step_rec,
                                    );
                                    self.set_active_stage(
                                        &app_handle,
                                        &task_id,
                                        TaskPhase::Running,
                                        actor_role,
                                        &actor_model,
                                        None,
                                    );
                                    break;
                                }
                                self.push_trace(
                                    &app_handle,
                                    &task_id,
                                    OrchestrationTraceEntry {
                                        id: Uuid::new_v4().to_string(),
                                        timestamp: Utc::now(),
                                        stage: "verify".to_string(),
                                        role: "judge".to_string(),
                                        model: judge_ver_model,
                                        title: "Judge confirmed goal completion".to_string(),
                                        content: ver.observation,
                                        step_number: Some(step_num),
                                        status: "success".to_string(),
                                    },
                                );
                            }
                        }

                        let cue = PointerCuePayload {
                            id: Uuid::new_v4().to_string(),
                            message: summary.clone(),
                            action_badge: "Done".to_string(),
                            target_x: cue_x,
                            target_y: cue_y,
                            target_width: None,
                            target_height: None,
                            target_ocr_id: None,
                            step_number: step_num,
                            is_acting: false,
                            show_pointer: false,
                            dismiss_after_ms: 4200,
                        };
                        let _ = emit_pointer_cue(&app_handle, cue);

                        let step_rec = TaskStepRecord {
                            step_number: step_num,
                            timestamp: Utc::now(),
                            action_type: action_label,
                            description: action_desc,
                            narration: item_decision.narration.clone(),
                            status: "done".to_string(),
                            target_ocr_id: item_decision.target_ocr_id,
                            target_x: Some(cue_x),
                            target_y: Some(cue_y),
                            verification_note: Some("Task marked complete".to_string()),
                            model: Some(used_actor_model.clone()),
                            thought: (!item_decision.thought.is_empty())
                                .then(|| item_decision.thought.clone()),
                        };
                        self.record_step(
                            &app_handle,
                            &task_id,
                            step_num,
                            item_decision.plan_step_index,
                            &before_cap.active_window.app_name,
                            step_rec,
                        );
                        self.finalize_task(
                            &app_handle,
                            &task_id,
                            TaskPhase::Completed,
                            "completed",
                            summary.clone(),
                            None,
                        );
                        show_tray_popover(&app_handle, false);
                        return;
                    }
                    AgentAction::Fail { reason } => {
                        let cue = PointerCuePayload {
                            id: Uuid::new_v4().to_string(),
                            message: reason.clone(),
                            action_badge: "Blocked".to_string(),
                            target_x: cue_x,
                            target_y: cue_y,
                            target_width: None,
                            target_height: None,
                            target_ocr_id: None,
                            step_number: step_num,
                            is_acting: false,
                            show_pointer: false,
                            dismiss_after_ms: 4200,
                        };
                        let _ = emit_pointer_cue(&app_handle, cue);

                        let step_rec = TaskStepRecord {
                            step_number: step_num,
                            timestamp: Utc::now(),
                            action_type: action_label,
                            description: action_desc,
                            narration: item_decision.narration.clone(),
                            status: "failed".to_string(),
                            target_ocr_id: item_decision.target_ocr_id,
                            target_x: Some(cue_x),
                            target_y: Some(cue_y),
                            verification_note: Some(reason.clone()),
                            model: Some(used_actor_model.clone()),
                            thought: (!item_decision.thought.is_empty())
                                .then(|| item_decision.thought.clone()),
                        };
                        self.record_step(
                            &app_handle,
                            &task_id,
                            step_num,
                            item_decision.plan_step_index,
                            &before_cap.active_window.app_name,
                            step_rec,
                        );
                        self.finalize_task(
                            &app_handle,
                            &task_id,
                            TaskPhase::Failed,
                            "failed",
                            reason.clone(),
                            Some(reason.clone()),
                        );
                        show_tray_popover(&app_handle, false);
                        return;
                    }
                    _ => {}
                }

                let cue = PointerCuePayload {
                    id: Uuid::new_v4().to_string(),
                    message: item_decision.narration.clone(),
                    action_badge: action_badge_text(&action).to_string(),
                    target_x: cue_x,
                    target_y: cue_y,
                    target_width,
                    target_height,
                    target_ocr_id: if show_pointer {
                        item_decision.target_ocr_id
                    } else {
                        None
                    },
                    step_number: step_num,
                    is_acting: show_pointer,
                    show_pointer,
                    dismiss_after_ms: 2800,
                };
                let _ = emit_pointer_cue(&app_handle, cue);

                {
                    let mut st = self.state.lock();
                    if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                        st.phase = TaskPhase::Running;
                        st.current_step = step_num;
                        st.active_plan_index = item_decision.plan_step_index;
                        st.active_app = before_cap.active_window.app_name.clone();
                        st.last_narration = item_decision.narration.clone();
                    }
                }
                let snap = self.snapshot();
                update_tray_status_title(&app_handle, Some(&snap));
                let _ = app_handle.emit("engine-snapshot", &snap);

                if CONFIRM_RISKY_ACTIONS {
                    if let Some(risk_reason) = assess_action_risk(
                        &action,
                        &item_decision.narration,
                        target_text.as_deref(),
                        item_decision.requires_confirmation,
                    ) {
                        let conf_id = Uuid::new_v4().to_string();
                        let (tx, rx) = oneshot::channel();
                        *self.confirm_tx.lock() = Some((conf_id.clone(), tx));

                        {
                            let mut st = self.state.lock();
                            if st.task_id.as_deref() != Some(&task_id) || !st.phase.is_active() {
                                return;
                            }
                            st.phase = TaskPhase::AwaitingConfirmation;
                            st.pending_confirmation = Some(PendingConfirmation {
                                id: conf_id,
                                step_number: step_num,
                                action_type: action_label.clone(),
                                description: action_desc.clone(),
                                narration: item_decision.narration.clone(),
                                reason: risk_reason,
                            });
                        }
                        let snap = self.snapshot();
                        update_tray_status_title(&app_handle, Some(&snap));
                        let _ = app_handle.emit("engine-snapshot", &snap);
                        show_tray_popover(&app_handle, false);

                        let approved = tokio::select! {
                            _ = stop_rx.changed() => {
                                if *stop_rx.borrow() {
                                    return;
                                }
                                false
                            }
                            res = rx => res.unwrap_or(false)
                        };

                        if *stop_rx.borrow() || !self.is_current_task(&task_id) {
                            return;
                        }

                        if !approved {
                            let step_rec = TaskStepRecord {
                                step_number: step_num,
                                timestamp: Utc::now(),
                                action_type: action_label,
                                description: action_desc.clone(),
                                narration: item_decision.narration.clone(),
                                status: "denied".to_string(),
                                target_ocr_id: item_decision.target_ocr_id,
                                target_x: Some(cue_x),
                                target_y: Some(cue_y),
                                verification_note: Some("Denied by user".to_string()),
                                model: Some(used_actor_model.clone()),
                                thought: (!item_decision.thought.is_empty())
                                    .then(|| item_decision.thought.clone()),
                            };
                            self.record_step(
                                &app_handle,
                                &task_id,
                                step_num,
                                item_decision.plan_step_index,
                                &before_cap.active_window.app_name,
                                step_rec,
                            );
                            push_recent_step(
                                &mut recent_steps,
                                format!("Step {step_num} ({action_desc}) was denied by user"),
                            );
                            hide_tray_popover(&app_handle);
                            break;
                        }

                        hide_tray_popover(&app_handle);
                        if sleep_interruptible(Duration::from_millis(120), &mut stop_rx).await {
                            return;
                        }
                    }
                }

                if show_pointer
                    && sleep_interruptible(Duration::from_millis(50), &mut stop_rx).await
                {
                    return;
                }
                if !self.is_current_task(&task_id) {
                    return;
                }

                let action_for_exec = action.clone();
                let exec_res =
                    tokio::task::spawn_blocking(move || execute_action(&action_for_exec, metrics))
                        .await;
                let (step_status, action_output_note, err_text) = match exec_res {
                    Ok(Ok(out)) => ("executed".to_string(), out.note, None),
                    Ok(Err(err)) => {
                        let msg = err.to_string();
                        if matches!(
                            err,
                            crate::automation::AutomationError::AccessibilityPermissionDenied
                        ) {
                            let step_rec = TaskStepRecord {
                                step_number: step_num,
                                timestamp: Utc::now(),
                                action_type: action_label,
                                description: action_desc,
                                narration: item_decision.narration.clone(),
                                status: "failed".to_string(),
                                target_ocr_id: item_decision.target_ocr_id,
                                target_x: Some(cue_x),
                                target_y: Some(cue_y),
                                verification_note: Some(msg.clone()),
                                model: Some(used_actor_model.clone()),
                                thought: (!item_decision.thought.is_empty())
                                    .then(|| item_decision.thought.clone()),
                            };
                            self.record_step(
                                &app_handle,
                                &task_id,
                                step_num,
                                item_decision.plan_step_index,
                                &before_cap.active_window.app_name,
                                step_rec,
                            );
                            self.finalize_task(
                                &app_handle,
                                &task_id,
                                TaskPhase::Failed,
                                "failed",
                                msg.clone(),
                                Some(msg),
                            );
                            show_tray_popover(&app_handle, false);
                            return;
                        }
                        ("failed".to_string(), None, Some(msg))
                    }
                    Err(err) => ("failed".to_string(), None, Some(err.to_string())),
                };

                if err_text.is_none() {
                    if let AgentAction::OpenUrl { url, .. } = &action {
                        last_opened_url = Some(normalize_url_for_dedup(url));
                        last_action_was_open_url = true;
                    } else {
                        last_action_was_open_url = false;
                    }
                } else {
                    last_action_was_open_url = false;
                }

                let step_rec = TaskStepRecord {
                    step_number: step_num,
                    timestamp: Utc::now(),
                    action_type: action_label,
                    description: action_desc.clone(),
                    narration: item_decision.narration.clone(),
                    status: step_status.clone(),
                    target_ocr_id: item_decision.target_ocr_id,
                    target_x: Some(cue_x),
                    target_y: Some(cue_y),
                    verification_note: err_text.clone().or_else(|| action_output_note.clone()),
                    model: Some(used_actor_model.clone()),
                    thought: (!item_decision.thought.is_empty())
                        .then(|| item_decision.thought.clone()),
                };
                self.record_step(
                    &app_handle,
                    &task_id,
                    step_num,
                    item_decision.plan_step_index,
                    &before_cap.active_window.app_name,
                    step_rec,
                );
                all_action_signatures.push(action_sig.clone());
                turn_signatures.push(action_sig);
                if matches!(action, AgentAction::Wait { .. }) {
                    consecutive_waits += 1;
                } else {
                    consecutive_waits = 0;
                }

                executed_count += 1;
                last_executed_action = Some(action.clone());
                last_action_desc = action_desc.clone();
                last_action_output_note = action_output_note.clone();
                last_err_text = err_text.clone();

                if err_text.is_some()
                    || matches!(action, AgentAction::Wait { .. } | AgentAction::ReadClipboard)
                {
                    break;
                }

                if batch_idx + 1 < queue_len && step_num < max_steps {
                    if adaptive_inter_batch_settle(
                        &action,
                        &before_cap.active_window,
                        &mut stop_rx,
                    )
                    .await
                    {
                        return;
                    }
                    if repetition_warned {
                        break;
                    }
                }
            }

            if executed_count == 0 {
                cached_capture = Some(before_cap);
                continue;
            }

            let Some(last_action) = last_executed_action else {
                cached_capture = Some(before_cap);
                continue;
            };

            if executed_count == 1
                && matches!(last_action, AgentAction::ReadClipboard)
                && last_err_text.is_none()
            {
                let note = last_action_output_note
                    .clone()
                    .unwrap_or_else(|| "Clipboard read".to_string());
                self.update_last_step_verification(
                    &app_handle,
                    &task_id,
                    step_num,
                    "verified",
                    note.clone(),
                );
                push_recent_step(
                    &mut recent_steps,
                    format!("Step {step_num} ({last_action_desc}) -> ok: {note}"),
                );
                consecutive_stalls = 0;
                cached_capture = Some(before_cap);
                continue;
            }

            if matches!(last_action, AgentAction::Wait { .. }) {
                cached_capture = None;
                push_recent_step(
                    &mut recent_steps,
                    format!(
                        "Step {step_num} ({last_action_desc}) in \"{}\"",
                        before_cap.active_window.window_title
                    ),
                );
                continue;
            }

            if adaptive_post_action_settle(
                &last_action,
                &before_cap.active_window,
                STEP_DELAY_MS,
                &mut stop_rx,
            )
            .await
            {
                return;
            }
            if !self.is_current_task(&task_id) {
                return;
            }

            let after_cap = match tokio::task::spawn_blocking(capture_and_ground_screen).await {
                Ok(Ok(cap)) => cap,
                Ok(Err(_)) | Err(_) => {
                    push_recent_step(
                        &mut recent_steps,
                        format!(
                            "Step {step_num} ({last_action_desc}) in \"{}\"",
                            before_cap.active_window.window_title
                        ),
                    );
                    continue;
                }
            };

            let visual_diff_pct =
                compute_frame_diff_ratio(&before_cap.full_res_rgba, &after_cap.full_res_rgba)
                    * 100.0;

            let local_ver = evaluate_local_verification(
                &before_cap.active_window,
                &after_cap.active_window,
                visual_diff_pct,
                last_action_output_note.as_deref(),
                last_err_text.as_deref(),
            );

            self.update_last_step_verification(
                &app_handle,
                &task_id,
                step_num,
                local_ver.step_status,
                local_ver.note.clone(),
            );

            let mut effective_stalled = local_ver.step_stalled;
            let mut judge_needs_recovery = false;
            let mut judge_recovery_hint: Option<String> = None;
            let mut verification_summary = local_ver.note.clone();

            if routing.enable_judge_verification && (local_ver.step_stalled || batch_repetition_warned) {
                self.set_active_stage(
                    &app_handle,
                    &task_id,
                    TaskPhase::Verifying,
                    "judge",
                    &routing.judge_model,
                    Some("Judge inspecting screen change...".to_string()),
                );
                let verify_ctx = VerifyStepContext {
                    goal: goal.clone(),
                    step_number: step_num,
                    action_description: last_action_desc.clone(),
                    expected_narration: verification_summary.clone(),
                    before_app: before_cap.active_window.app_name.clone(),
                    after_app: after_cap.active_window.app_name.clone(),
                    before_title: before_cap.active_window.window_title.clone(),
                    after_title: after_cap.active_window.window_title.clone(),
                    visual_diff_pct,
                    action_output: last_action_output_note.clone(),
                    visible_targets: after_cap.target_map.formatted_lines.clone(),
                    after_base64_jpeg: after_cap.annotated_jpeg.clone(),
                };
                let mut judge_ver_model = routing.judge_model.clone();
                let judge_res = match self
                    .client
                    .verify_step_outcome(
                        OLLAMA_BASE_URL,
                        &cycle_api_key,
                        &routing.judge_model,
                        verify_ctx.clone(),
                    )
                    .await
                {
                    Ok(v) => Ok(v),
                    Err(_) if routing.judge_model != routing.vision_actor_model => {
                        judge_ver_model = routing.vision_actor_model.clone();
                        self.client
                            .verify_step_outcome(
                                OLLAMA_BASE_URL,
                                &cycle_api_key,
                                &routing.vision_actor_model,
                                verify_ctx,
                            )
                            .await
                    }
                    Err(e) => Err(e),
                };

                if let Ok(ver) = judge_res {
                    let trace_status = if ver.status == "succeeded" || ver.goal_achieved {
                        "success"
                    } else {
                        "warning"
                    };
                    let trace_content = if ver.thinking.is_empty() {
                        ver.observation.clone()
                    } else {
                        format!("{} — {}", ver.thinking, ver.observation)
                    };
                    self.push_trace(
                        &app_handle,
                        &task_id,
                        OrchestrationTraceEntry {
                            id: Uuid::new_v4().to_string(),
                            timestamp: Utc::now(),
                            stage: "verify".to_string(),
                            role: "judge".to_string(),
                            model: judge_ver_model,
                            title: format!("Judge check ({})", ver.status),
                            content: trace_content,
                            step_number: Some(step_num),
                            status: trace_status.to_string(),
                        },
                    );
                    verification_summary = ver.observation.clone();
                    if ver.goal_achieved {
                        self.update_last_step_verification(
                            &app_handle,
                            &task_id,
                            step_num,
                            "verified",
                            ver.observation.clone(),
                        );
                        self.finalize_task(
                            &app_handle,
                            &task_id,
                            TaskPhase::Completed,
                            "completed",
                            ver.observation,
                            None,
                        );
                        show_tray_popover(&app_handle, false);
                        return;
                    }
                    if ver.status == "succeeded" {
                        effective_stalled = false;
                        self.update_last_step_verification(
                            &app_handle,
                            &task_id,
                            step_num,
                            "verified",
                            ver.observation,
                        );
                    } else if ver.status == "needs_recovery" || ver.status == "failed" {
                        effective_stalled = true;
                        judge_needs_recovery = ver.status == "needs_recovery";
                        judge_recovery_hint = ver.recovery_hint;
                        self.update_last_step_verification(
                            &app_handle,
                            &task_id,
                            step_num,
                            "unverified_failed",
                            ver.observation,
                        );
                    }
                }
                self.set_active_stage(
                    &app_handle,
                    &task_id,
                    TaskPhase::Running,
                    actor_role,
                    &actor_model,
                    None,
                );
            }

            let out_suffix = match &last_action_output_note {
                Some(note) => format!(" | {note}"),
                None => String::new(),
            };
            push_recent_step(
                &mut recent_steps,
                format!(
                    "Step {step_num} ({last_action_desc}) [{} -> {}] in \"{}\" -> {} ({}){out_suffix}",
                    before_cap.active_window.app_name,
                    after_cap.active_window.app_name,
                    after_cap.active_window.window_title,
                    if effective_stalled { "stalled" } else { "ok" },
                    verification_summary
                ),
            );

            if effective_stalled {
                stalled_action_signatures.extend(turn_signatures);
            } else {
                stalled_action_signatures.clear();
            }

            consecutive_stalls = if effective_stalled || batch_repetition_warned {
                consecutive_stalls + 1
            } else {
                0
            };

            if (consecutive_stalls >= 2 || batch_repetition_warned || judge_needs_recovery)
                && recovery_count < MAX_RECOVERY_PLANS
            {
                if matches!(last_action, AgentAction::OpenUrl { .. }) {
                    last_opened_url = None;
                }

                self.set_active_stage(
                    &app_handle,
                    &task_id,
                    TaskPhase::Recovering,
                    "recovery",
                    &routing.recovery_model,
                    Some("Step stalled—building a quick recovery plan...".to_string()),
                );

                let rec_cue = PointerCuePayload {
                    id: Uuid::new_v4().to_string(),
                    message: "Adapting with a recovery plan...".to_string(),
                    action_badge: "Recovering".to_string(),
                    target_x: last_cue_pos.0,
                    target_y: last_cue_pos.1,
                    target_width: None,
                    target_height: None,
                    target_ocr_id: None,
                    step_number: step_num,
                    is_acting: false,
                    show_pointer: false,
                    dismiss_after_ms: 2400,
                };
                let _ = emit_pointer_cue(&app_handle, rec_cue);

                let rec_ctx = RecoveryContext {
                    goal: goal.clone(),
                    failed_action: last_action_desc.clone(),
                    observation: verification_summary.clone(),
                    recovery_hint: judge_recovery_hint.clone(),
                    active_app: after_cap.active_window.app_name.clone(),
                    window_title: after_cap.active_window.window_title.clone(),
                    recent_steps: recent_steps.clone(),
                    visible_targets: after_cap.target_map.formatted_lines.clone(),
                    base64_jpeg: after_cap.annotated_jpeg.clone(),
                };
                let mut used_recovery_model = routing.recovery_model.clone();
                let rec_res = match self
                    .client
                    .create_recovery_plan(
                        OLLAMA_BASE_URL,
                        &cycle_api_key,
                        &routing.recovery_model,
                        rec_ctx.clone(),
                    )
                    .await
                {
                    Ok(r) => Ok(r),
                    Err(_) if routing.recovery_model != routing.vision_actor_model => {
                        used_recovery_model = routing.vision_actor_model.clone();
                        self.client
                            .create_recovery_plan(
                                OLLAMA_BASE_URL,
                                &cycle_api_key,
                                &routing.vision_actor_model,
                                rec_ctx,
                            )
                            .await
                    }
                    Err(e) => Err(e),
                };

                if let Ok(rec_plan) = rec_res {
                    recovery_count += 1;
                    consecutive_stalls = 0;
                    plan_steps = rec_plan.recovery_steps.clone();
                    {
                        let mut st = self.state.lock();
                        if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                            st.recovery_count = recovery_count;
                            st.last_diagnosis = Some(rec_plan.diagnosis.clone());
                            st.plan_steps = plan_steps.clone();
                            st.active_plan_index = 0;
                            st.phase = TaskPhase::Running;
                            st.active_role = actor_role.to_string();
                            st.active_model = actor_model.clone();
                            st.last_narration = format!("Recovery plan: {}", rec_plan.diagnosis);
                        }
                    }
                    let rec_content = if rec_plan.thinking.is_empty() {
                        format!(
                            "{} -> {}",
                            rec_plan.diagnosis,
                            rec_plan.recovery_steps.join(" | ")
                        )
                    } else {
                        format!(
                            "{} — {} -> {}",
                            rec_plan.thinking,
                            rec_plan.diagnosis,
                            rec_plan.recovery_steps.join(" | ")
                        )
                    };
                    self.push_trace(
                        &app_handle,
                        &task_id,
                        OrchestrationTraceEntry {
                            id: Uuid::new_v4().to_string(),
                            timestamp: Utc::now(),
                            stage: "recover".to_string(),
                            role: "recovery".to_string(),
                            model: used_recovery_model.clone(),
                            title: format!("Recovery #{recovery_count}"),
                            content: rec_content,
                            step_number: Some(step_num),
                            status: "warning".to_string(),
                        },
                    );
                    let rec_record = TaskStepRecord {
                        step_number: step_num,
                        timestamp: Utc::now(),
                        action_type: "recovery_plan".to_string(),
                        description: format!("Recovery #{recovery_count}"),
                        narration: rec_plan.diagnosis.clone(),
                        status: "recovery".to_string(),
                        target_ocr_id: None,
                        target_x: None,
                        target_y: None,
                        verification_note: Some(rec_plan.recovery_steps.join(" -> ")),
                        model: Some(used_recovery_model),
                        thought: (!rec_plan.thinking.is_empty()).then(|| rec_plan.thinking),
                    };
                    self.record_step(
                        &app_handle,
                        &task_id,
                        step_num,
                        0,
                        &after_cap.active_window.app_name,
                        rec_record,
                    );
                    push_recent_step(
                        &mut recent_steps,
                        format!(
                            "RECOVERY PLAN #{recovery_count}: {}. Follow the updated plan using a different tool/approach.",
                            rec_plan.diagnosis
                        ),
                    );
                } else {
                    self.set_active_stage(
                        &app_handle,
                        &task_id,
                        TaskPhase::Running,
                        actor_role,
                        &actor_model,
                        None,
                    );
                }
            }

            cached_capture = Some(after_cap);
        }

        self.finalize_task(
            &app_handle,
            &task_id,
            TaskPhase::Completed,
            "max_steps",
            format!("Reached maximum limit of {max_steps} steps."),
            None,
        );
        show_tray_popover(&app_handle, false);
    }

    fn set_active_stage(
        &self,
        app_handle: &AppHandle,
        task_id: &str,
        phase: TaskPhase,
        role: &str,
        model: &str,
        narration: Option<String>,
    ) {
        {
            let mut st = self.state.lock();
            if st.task_id.as_deref() != Some(task_id) || !st.phase.is_active() {
                return;
            }
            st.phase = phase;
            st.active_role = role.to_string();
            st.active_model = model.to_string();
            if let Some(msg) = narration {
                st.last_narration = msg;
            }
        }
        let snap = self.snapshot();
        update_tray_status_title(app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);
    }

    fn push_trace(
        &self,
        app_handle: &AppHandle,
        task_id: &str,
        entry: OrchestrationTraceEntry,
    ) {
        {
            let mut st = self.state.lock();
            if st.task_id.as_deref() != Some(task_id) {
                return;
            }
            if !entry.content.is_empty() {
                st.current_thought = entry.content.clone();
            }
            st.traces.push(entry);
            if st.traces.len() > MAX_TRACE_ENTRIES {
                let overflow = st.traces.len() - MAX_TRACE_ENTRIES;
                st.traces.drain(0..overflow);
            }
        }
        let snap = self.snapshot();
        let _ = app_handle.emit("engine-snapshot", &snap);
    }

    fn record_step(
        &self,
        app_handle: &AppHandle,
        task_id: &str,
        step_number: u32,
        plan_index: u32,
        active_app: &str,
        record: TaskStepRecord,
    ) {
        {
            let mut st = self.state.lock();
            if st.task_id.as_deref() != Some(task_id) {
                return;
            }
            st.current_step = step_number;
            st.active_plan_index = plan_index;
            st.active_app = active_app.to_string();
            st.steps.push(record);
        }
        let snap = self.snapshot();
        update_tray_status_title(app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);
    }

    fn update_last_step_verification(
        &self,
        app_handle: &AppHandle,
        task_id: &str,
        step_number: u32,
        status: &str,
        note: String,
    ) {
        {
            let mut st = self.state.lock();
            if st.task_id.as_deref() != Some(task_id) {
                return;
            }
            if let Some(last) = st
                .steps
                .iter_mut()
                .rev()
                .find(|s| s.step_number == step_number && s.action_type != "recovery_plan")
            {
                last.status = status.to_string();
                last.verification_note = Some(note);
            }
        }
        let snap = self.snapshot();
        update_tray_status_title(app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);
    }

    fn finalize_task(
        &self,
        app_handle: &AppHandle,
        task_id: &str,
        final_phase: TaskPhase,
        outcome: &str,
        summary: String,
        error: Option<String>,
    ) {
        if let Some(tx) = self.stop_tx.lock().take() {
            let _ = tx.send(true);
        }

        let record_to_save = {
            let mut st = self.state.lock();
            if st.task_id.as_deref() != Some(task_id) || !st.phase.is_active() {
                None
            } else {
                st.phase = final_phase;
                st.pending_confirmation = None;
                st.last_summary = Some(summary.clone());
                st.last_narration = summary.clone();
                st.last_error = error;
                let trace_status = match final_phase {
                    TaskPhase::Completed => "success",
                    TaskPhase::Failed => "error",
                    _ => "warning",
                };
                let active_model = st.active_model.clone();
                let current_step = st.current_step;
                st.traces.push(OrchestrationTraceEntry {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    stage: "complete".to_string(),
                    role: "system".to_string(),
                    model: active_model,
                    title: format!("Task {outcome}"),
                    content: summary,
                    step_number: Some(current_step),
                    status: trace_status.to_string(),
                });
                if st.traces.len() > MAX_TRACE_ENTRIES {
                    let overflow = st.traces.len() - MAX_TRACE_ENTRIES;
                    st.traces.drain(0..overflow);
                }
                take_history_record_locked(&mut st, outcome)
            }
        };

        if let Some(record) = record_to_save {
            tauri::async_runtime::spawn_blocking(move || {
                let _ = append_history_record(record);
            });
        }

        let snap = self.snapshot();
        update_tray_status_title(app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);
    }
}

fn capture_and_ground_screen() -> Result<GroundedCapture, crate::capture::CaptureError> {
    let frame = capture_raw_frame_in_ram()?;
    let (mut rgb_image, ax_targets, ocr_targets) = std::thread::scope(|scope| {
        let rgb_handle =
            scope.spawn(|| downscale_rgba_to_rgb(&frame.full_res_rgba, MAX_DOWNSCALED_WIDTH));
        let ax_handle =
            scope.spawn(|| extract_ax_elements(frame.active_window.process_id, frame.screen_metrics));
        let ocr_targets = extract_ocr_targets(&frame.full_res_rgba, frame.screen_metrics);
        let rgb_image = rgb_handle
            .join()
            .unwrap_or_else(|_| downscale_rgba_to_rgb(&frame.full_res_rgba, MAX_DOWNSCALED_WIDTH));
        let ax_targets = ax_handle.join().unwrap_or_default();
        (rgb_image, ax_targets, ocr_targets)
    });

    let target_map = merge_targets(ax_targets, ocr_targets, frame.screen_metrics);
    let annotated_jpeg =
        annotate_rgb_and_encode_jpeg_base64(&mut rgb_image, &target_map.annotation_boxes)?;

    Ok(GroundedCapture {
        full_res_rgba: frame.full_res_rgba,
        annotated_jpeg,
        active_window: frame.active_window,
        screen_metrics: frame.screen_metrics,
        target_map,
    })
}

pub struct LocalVerificationResult {
    pub step_status: &'static str,
    pub step_stalled: bool,
    pub note: String,
}

pub fn evaluate_local_verification(
    before: &ActiveWindowMetadata,
    after: &ActiveWindowMetadata,
    visual_diff_pct: f64,
    action_output: Option<&str>,
    err_text: Option<&str>,
) -> LocalVerificationResult {
    if let Some(err) = err_text {
        return LocalVerificationResult {
            step_status: "failed",
            step_stalled: true,
            note: err.to_string(),
        };
    }

    let app_changed = before.app_name != after.app_name;
    let title_changed = before.window_title != after.window_title;
    let ui_changed = app_changed
        || title_changed
        || visual_diff_pct >= 0.4
        || action_output.is_some();

    let note = if let Some(out) = action_output {
        out.to_string()
    } else if app_changed {
        format!("App -> {} (diff {visual_diff_pct:.1}%)", after.app_name)
    } else if title_changed {
        format!("Window updated (diff {visual_diff_pct:.1}%)")
    } else if ui_changed {
        format!("UI updated (diff {visual_diff_pct:.1}%)")
    } else {
        format!("No visible UI change ({visual_diff_pct:.1}%)")
    };

    LocalVerificationResult {
        step_status: if ui_changed {
            "verified"
        } else {
            "unverified_failed"
        },
        step_stalled: !ui_changed,
        note,
    }
}

pub fn build_turn_action_queue(mut decision: PlannedActionDecision) -> Vec<PlannedActionDecision> {
    let plan_idx = decision.plan_step_index;
    let follow_ups = std::mem::take(&mut decision.follow_up_actions);
    let mut queue = Vec::with_capacity(1 + follow_ups.len());
    queue.push(decision);
    for follow_up in follow_ups {
        queue.push(follow_up.into_planned_decision(plan_idx));
    }
    queue
}

fn push_recent_step(recent_steps: &mut Vec<String>, entry: String) {
    recent_steps.push(entry);
    if recent_steps.len() > 10 {
        recent_steps.remove(0);
    }
}

pub fn normalize_url_for_dedup(url: &str) -> String {
    url.trim().trim_end_matches('/').to_ascii_lowercase()
}

pub fn is_duplicate_url_action(
    action: &AgentAction,
    last_opened_url: Option<&str>,
    last_action_was_open_url: bool,
) -> bool {
    let Some(prev_norm) = last_opened_url else {
        return false;
    };
    match action {
        AgentAction::OpenUrl { url, .. } => normalize_url_for_dedup(url) == prev_norm,
        AgentAction::TypeText { text, .. } | AgentAction::PasteText { text, .. }
            if last_action_was_open_url =>
        {
            let typed_norm = normalize_url_for_dedup(text);
            let bare_prev = prev_norm
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            !typed_norm.is_empty()
                && (typed_norm == prev_norm || typed_norm == bare_prev || typed_norm.contains(bare_prev))
        }
        _ => false,
    }
}

async fn wait_for_window_change_or_timeout(
    before: &ActiveWindowMetadata,
    target_app: Option<&str>,
    max_wait_ms: u64,
    paint_settle_ms: u64,
    stop_rx: &mut watch::Receiver<bool>,
) -> bool {
    let initial_wait = 100.min(max_wait_ms);
    if sleep_interruptible(Duration::from_millis(initial_wait), stop_rx).await {
        return true;
    }

    let deadline = tokio::time::Instant::now()
        + Duration::from_millis(max_wait_ms.saturating_sub(initial_wait));

    loop {
        let current = tokio::task::spawn_blocking(read_active_window_metadata).await;
        if let Ok(win) = current {
            let app_matched = target_app.is_some_and(|t| {
                !t.trim().is_empty() && win.app_name.eq_ignore_ascii_case(t.trim())
            });
            let changed = win.app_name != before.app_name
                || win.window_title != before.window_title
                || app_matched;
            if changed {
                return sleep_interruptible(Duration::from_millis(paint_settle_ms), stop_rx).await;
            }
        }

        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        if sleep_interruptible(Duration::from_millis(50), stop_rx).await {
            return true;
        }
    }
}

async fn adaptive_inter_batch_settle(
    action: &AgentAction,
    before_win: &ActiveWindowMetadata,
    stop_rx: &mut watch::Receiver<bool>,
) -> bool {
    match action {
        AgentAction::OpenApp { app_name } | AgentAction::FocusApp { app_name } => {
            wait_for_window_change_or_timeout(before_win, Some(app_name), 900, 120, stop_rx).await
        }
        AgentAction::OpenUrl { app_name, .. } => {
            wait_for_window_change_or_timeout(before_win, app_name.as_deref(), 900, 120, stop_rx)
                .await
        }
        AgentAction::OpenPath { .. } => {
            wait_for_window_change_or_timeout(before_win, None, 900, 120, stop_rx).await
        }
        _ => sleep_interruptible(Duration::from_millis(80), stop_rx).await,
    }
}

async fn adaptive_post_action_settle(
    action: &AgentAction,
    before_win: &ActiveWindowMetadata,
    base_delay_ms: u64,
    stop_rx: &mut watch::Receiver<bool>,
) -> bool {
    match action {
        AgentAction::OpenApp { app_name } | AgentAction::FocusApp { app_name } => {
            wait_for_window_change_or_timeout(
                before_win,
                Some(app_name),
                base_delay_ms.max(1200),
                150,
                stop_rx,
            )
            .await
        }
        AgentAction::OpenUrl { app_name, .. } => {
            wait_for_window_change_or_timeout(
                before_win,
                app_name.as_deref(),
                base_delay_ms.max(1200),
                150,
                stop_rx,
            )
            .await
        }
        AgentAction::OpenPath { .. } => {
            wait_for_window_change_or_timeout(
                before_win,
                None,
                base_delay_ms.max(1200),
                150,
                stop_rx,
            )
            .await
        }
        AgentAction::TypeText { submit: true, .. }
        | AgentAction::PasteText { submit: true, .. } => {
            sleep_interruptible(Duration::from_millis(base_delay_ms.min(350)), stop_rx).await
        }
        AgentAction::PressKey { key, .. }
            if matches!(key.trim().to_ascii_lowercase().as_str(), "enter" | "return") =>
        {
            sleep_interruptible(Duration::from_millis(base_delay_ms.min(350)), stop_rx).await
        }
        _ => sleep_interruptible(Duration::from_millis(base_delay_ms.min(220)), stop_rx).await,
    }
}

pub fn select_actor_for_classification(
    classification: &TaskClassification,
    routing: &ModelRoutingConfig,
) -> (&'static str, String) {
    if classification.category == "coding" || classification.recommended_role == "coding_actor" {
        ("coding_actor", routing.coding_actor_model.clone())
    } else if classification.category == "writing"
        || classification.recommended_role == "writing_actor"
    {
        ("writing_actor", routing.writing_actor_model.clone())
    } else {
        ("vision_actor", routing.vision_actor_model.clone())
    }
}

pub fn fallback_classification(goal: &str) -> TaskClassification {
    let lower = goal.to_ascii_lowercase();
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let has_any = |keywords: &[&str]| tokens.iter().any(|tok| keywords.contains(tok));
    if has_any(&["code", "terminal", "vscode", "xcode", "git", "rust", "typescript", "debug", "ide"]) {
        TaskClassification {
            category: "coding".to_string(),
            complexity: "moderate".to_string(),
            recommended_role: "coding_actor".to_string(),
            reasoning: "Local heuristic routed coding/terminal task to coding actor.".to_string(),
            needs_dedicated_planner: true,
            initial_plan_steps: Vec::new(),
        }
    } else if has_any(&["note", "notes", "email", "write", "draft", "document", "checklist", "mail"]) {
        TaskClassification {
            category: "writing".to_string(),
            complexity: "moderate".to_string(),
            recommended_role: "writing_actor".to_string(),
            reasoning: "Local heuristic routed writing/notes task to writing actor.".to_string(),
            needs_dedicated_planner: true,
            initial_plan_steps: Vec::new(),
        }
    } else if has_any(&["safari", "chrome", "http", "https", "github", "search", "browser", "web"]) {
        TaskClassification {
            category: "browser_research".to_string(),
            complexity: "moderate".to_string(),
            recommended_role: "vision_actor".to_string(),
            reasoning: "Local heuristic routed browser research task to vision actor.".to_string(),
            needs_dedicated_planner: true,
            initial_plan_steps: Vec::new(),
        }
    } else {
        TaskClassification {
            category: "general_ui".to_string(),
            complexity: "simple".to_string(),
            recommended_role: "vision_actor".to_string(),
            reasoning: "Local heuristic routed UI task to vision actor.".to_string(),
            needs_dedicated_planner: false,
            initial_plan_steps: Vec::new(),
        }
    }
}

fn take_history_record_locked(
    st: &mut TaskRuntimeState,
    outcome: &str,
) -> Option<TaskHistoryRecord> {
    if st.history_saved {
        return None;
    }
    let task_id = st.task_id.clone()?;
    let started_at = st.started_at?;
    st.history_saved = true;
    Some(TaskHistoryRecord {
        task_id,
        goal: st.goal.clone(),
        started_at,
        ended_at: Utc::now(),
        outcome: outcome.to_string(),
        summary: st.last_summary.clone().unwrap_or_default(),
        step_count: st.steps.len() as u32,
        steps: st.steps.clone(),
        category: st.classification.as_ref().map(|c| c.category.clone()),
        selected_model: (!st.active_model.is_empty()).then(|| st.active_model.clone()),
    })
}

async fn sleep_interruptible(duration: Duration, stop_rx: &mut watch::Receiver<bool>) -> bool {
    if *stop_rx.borrow() {
        return true;
    }
    tokio::select! {
        _ = stop_rx.changed() => *stop_rx.borrow(),
        _ = tokio::time::sleep(duration) => *stop_rx.borrow(),
    }
}

type ResolvedTarget = ((f64, f64), (f64, f64), Option<(f64, f64)>, Option<String>);

fn resolve_target_strict(
    decision: &PlannedActionDecision,
    target_map: &ScreenTargetMap,
    metrics: ScreenMetrics,
) -> Result<ResolvedTarget, String> {
    if let Some(id) = decision.target_ocr_id {
        if let Some(target) = target_map.targets_by_id.get(&id) {
            return Ok((
                (target.center_x, target.center_y),
                (
                    metrics.origin_x + target.center_x,
                    metrics.origin_y + target.center_y,
                ),
                Some((target.width, target.height)),
                Some(target.label.clone()),
            ));
        }
        if let (Some(xn), Some(yn)) = (decision.x_norm, decision.y_norm) {
            let ox = xn.clamp(0.0, 1.0) * metrics.logical_width;
            let oy = yn.clamp(0.0, 1.0) * metrics.logical_height;
            return Ok((
                (ox, oy),
                (metrics.origin_x + ox, metrics.origin_y + oy),
                None,
                None,
            ));
        }
        return Err(format!("target_ocr_id #{id} not found in visible targets"));
    }

    if let (Some(xn), Some(yn)) = (decision.x_norm, decision.y_norm) {
        let ox = xn.clamp(0.0, 1.0) * metrics.logical_width;
        let oy = yn.clamp(0.0, 1.0) * metrics.logical_height;
        return Ok((
            (ox, oy),
            (metrics.origin_x + ox, metrics.origin_y + oy),
            None,
            None,
        ));
    }

    Err("click action missing valid target_ocr_id or x_norm/y_norm".to_string())
}

fn resolve_target_optional(
    decision: &PlannedActionDecision,
    target_map: &ScreenTargetMap,
    metrics: ScreenMetrics,
) -> ResolvedTarget {
    if let Ok(found) = resolve_target_strict(decision, target_map, metrics) {
        return found;
    }
    let cx = metrics.logical_width * 0.5;
    let cy = metrics.logical_height * 0.45;
    (
        (cx, cy),
        (metrics.origin_x + cx, metrics.origin_y + cy),
        None,
        None,
    )
}

pub fn resolve_action_from_decision(
    decision: &PlannedActionDecision,
    target_map: &ScreenTargetMap,
    metrics: ScreenMetrics,
) -> Result<(AgentAction, (f64, f64), Option<(f64, f64)>, Option<String>), String> {
    match decision.action_type.as_str() {
        "click" => {
            let ((ox, oy), (ax, ay), dims, text) =
                resolve_target_strict(decision, target_map, metrics)?;
            Ok((
                AgentAction::Click {
                    x: ax,
                    y: ay,
                    button: MouseButtonKind::Left,
                    double: false,
                },
                (ox, oy),
                dims,
                text,
            ))
        }
        "double_click" => {
            let ((ox, oy), (ax, ay), dims, text) =
                resolve_target_strict(decision, target_map, metrics)?;
            Ok((
                AgentAction::Click {
                    x: ax,
                    y: ay,
                    button: MouseButtonKind::Left,
                    double: true,
                },
                (ox, oy),
                dims,
                text,
            ))
        }
        "right_click" => {
            let ((ox, oy), (ax, ay), dims, text) =
                resolve_target_strict(decision, target_map, metrics)?;
            Ok((
                AgentAction::Click {
                    x: ax,
                    y: ay,
                    button: MouseButtonKind::Right,
                    double: false,
                },
                (ox, oy),
                dims,
                text,
            ))
        }
        "drag" => {
            let (fxn, fyn, txn, tyn) = match (
                decision.from_x_norm,
                decision.from_y_norm,
                decision.to_x_norm,
                decision.to_y_norm,
            ) {
                (Some(fx), Some(fy), Some(tx), Some(ty)) => (
                    fx.clamp(0.0, 1.0),
                    fy.clamp(0.0, 1.0),
                    tx.clamp(0.0, 1.0),
                    ty.clamp(0.0, 1.0),
                ),
                _ => return Err("drag missing from/to normalized coordinates".to_string()),
            };
            let from_ox = fxn * metrics.logical_width;
            let from_oy = fyn * metrics.logical_height;
            let to_ox = txn * metrics.logical_width;
            let to_oy = tyn * metrics.logical_height;
            Ok((
                AgentAction::Drag {
                    from_x: metrics.origin_x + from_ox,
                    from_y: metrics.origin_y + from_oy,
                    to_x: metrics.origin_x + to_ox,
                    to_y: metrics.origin_y + to_oy,
                },
                (from_ox, from_oy),
                None,
                None,
            ))
        }
        "scroll" => {
            let ((ox, oy), (ax, ay), dims, text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((
                AgentAction::Scroll {
                    x: ax,
                    y: ay,
                    delta_y: decision.scroll_delta_y.unwrap_or(-5),
                    delta_x: decision.scroll_delta_x.unwrap_or(0),
                },
                (ox, oy),
                dims,
                text,
            ))
        }
        "type_text" => {
            let text = decision
                .text
                .clone()
                .filter(|t| !t.is_empty())
                .ok_or_else(|| "type_text missing text".to_string())?;
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((
                AgentAction::TypeText {
                    text,
                    replace: decision.replace,
                    submit: decision.submit,
                },
                (ox, oy),
                dims,
                target_text,
            ))
        }
        "paste_text" => {
            let text = decision
                .text
                .clone()
                .filter(|t| !t.is_empty())
                .ok_or_else(|| "paste_text missing text".to_string())?;
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((
                AgentAction::PasteText {
                    text,
                    replace: decision.replace,
                    submit: decision.submit,
                },
                (ox, oy),
                dims,
                target_text,
            ))
        }
        "read_clipboard" => {
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((AgentAction::ReadClipboard, (ox, oy), dims, target_text))
        }
        "press_key" => {
            let key = decision
                .key
                .clone()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .ok_or_else(|| "press_key missing key".to_string())?;
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((
                AgentAction::PressKey {
                    key,
                    modifiers: decision.modifiers.clone(),
                },
                (ox, oy),
                dims,
                target_text,
            ))
        }
        "open_app" => {
            let app_name = decision
                .app_name
                .clone()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
                .ok_or_else(|| "open_app missing app_name".to_string())?;
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((AgentAction::OpenApp { app_name }, (ox, oy), dims, target_text))
        }
        "focus_app" => {
            let app_name = decision
                .app_name
                .clone()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
                .ok_or_else(|| "focus_app missing app_name".to_string())?;
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((
                AgentAction::FocusApp { app_name },
                (ox, oy),
                dims,
                target_text,
            ))
        }
        "open_url" => {
            let url = decision
                .url
                .clone()
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
                .ok_or_else(|| "open_url missing url".to_string())?;
            let app_name = decision
                .app_name
                .clone()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty());
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((
                AgentAction::OpenUrl { url, app_name },
                (ox, oy),
                dims,
                target_text,
            ))
        }
        "open_path" => {
            let path = decision
                .path
                .clone()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .ok_or_else(|| "open_path missing path".to_string())?;
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((AgentAction::OpenPath { path }, (ox, oy), dims, target_text))
        }
        "wait" => {
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((
                AgentAction::Wait {
                    ms: decision.wait_ms.unwrap_or(1000).clamp(100, 5000),
                },
                (ox, oy),
                dims,
                target_text,
            ))
        }
        "done" => {
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((
                AgentAction::Done {
                    summary: decision
                        .summary
                        .clone()
                        .unwrap_or_else(|| decision.narration.clone()),
                },
                (ox, oy),
                dims,
                target_text,
            ))
        }
        "fail" => {
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((
                AgentAction::Fail {
                    reason: decision
                        .summary
                        .clone()
                        .unwrap_or_else(|| decision.narration.clone()),
                },
                (ox, oy),
                dims,
                target_text,
            ))
        }
        other => Err(format!("unsupported action_type \"{other}\"")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepetitionLevel {
    None,
    Warn,
    Abort,
}

pub fn detect_repetition_level(history: &[String], next_sig: &str) -> RepetitionLevel {
    let trailing_same = history.iter().rev().take_while(|s| s.as_str() == next_sig).count();
    if trailing_same >= 3 {
        return RepetitionLevel::Abort;
    }
    if trailing_same >= 1 {
        return RepetitionLevel::Warn;
    }

    let n = history.len();
    if n >= 5 {
        let a = &history[n - 5];
        let b = &history[n - 4];
        if a != b && history[n - 3] == *a && history[n - 2] == *b && history[n - 1] == *a && next_sig == b {
            return RepetitionLevel::Abort;
        }
    }
    if n >= 3 {
        let a = &history[n - 3];
        let b = &history[n - 2];
        if a != b && history[n - 1] == *a && next_sig == b {
            return RepetitionLevel::Warn;
        }
    }

    RepetitionLevel::None
}

pub fn is_naturally_repeatable_action(action: &AgentAction) -> bool {
    match action {
        AgentAction::Scroll { .. } | AgentAction::Wait { .. } | AgentAction::Drag { .. } => true,
        AgentAction::PressKey { key, modifiers } if modifiers.is_empty() => matches!(
            key.trim().to_ascii_lowercase().as_str(),
            "up" | "down"
                | "left"
                | "right"
                | "arrowup"
                | "arrowdown"
                | "arrowleft"
                | "arrowright"
                | "arrow_up"
                | "arrow_down"
                | "arrow_left"
                | "arrow_right"
                | "pageup"
                | "page_up"
                | "pgup"
                | "pagedown"
                | "page_down"
                | "pgdn"
                | "space"
                | "spacebar"
                | "tab"
                | "backspace"
                | "delete"
                | "forward_delete"
                | "del"
        ),
        _ => false,
    }
}

pub fn detect_progressing_cycle_abort(history: &[String], next_sig: &str) -> bool {
    let trailing_same = history.iter().rev().take_while(|s| s.as_str() == next_sig).count();
    if trailing_same >= 6 {
        return true;
    }

    let n = history.len();
    if n >= 7 {
        let a = &history[n - 7];
        let b = &history[n - 6];
        if a != b
            && history[n - 5] == *a
            && history[n - 4] == *b
            && history[n - 3] == *a
            && history[n - 2] == *b
            && history[n - 1] == *a
            && next_sig == b
        {
            return true;
        }
    }

    false
}

fn action_badge_text(action: &AgentAction) -> &'static str {
    match action {
        AgentAction::Click { double: true, .. } => "Double-Click",
        AgentAction::Click {
            button: MouseButtonKind::Right,
            ..
        } => "Right-Click",
        AgentAction::Click { .. } => "Click",
        AgentAction::Drag { .. } => "Drag",
        AgentAction::TypeText { .. } => "Type",
        AgentAction::PasteText { .. } => "Paste",
        AgentAction::ReadClipboard => "Clipboard",
        AgentAction::PressKey { .. } => "Shortcut",
        AgentAction::Scroll { .. } => "Scroll",
        AgentAction::OpenApp { .. } => "Open App",
        AgentAction::FocusApp { .. } => "Focus App",
        AgentAction::OpenUrl { .. } => "Open URL",
        AgentAction::OpenPath { .. } => "Open Path",
        AgentAction::Wait { .. } => "Wait",
        AgentAction::Done { .. } => "Done",
        AgentAction::Fail { .. } => "Blocked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::{merge_targets, RawTarget};

    fn sample_metrics() -> ScreenMetrics {
        ScreenMetrics {
            origin_x: 100.0,
            origin_y: 50.0,
            logical_width: 1000.0,
            logical_height: 800.0,
            pixel_width: 2000,
            pixel_height: 1600,
            scale_factor: 2.0,
        }
    }

    fn base_decision(action_type: &str) -> PlannedActionDecision {
        PlannedActionDecision {
            thought: String::new(),
            narration: "Working on it".to_string(),
            plan_step_index: 0,
            action_type: action_type.to_string(),
            target_ocr_id: None,
            x_norm: None,
            y_norm: None,
            from_x_norm: None,
            from_y_norm: None,
            to_x_norm: None,
            to_y_norm: None,
            text: None,
            replace: false,
            submit: false,
            key: None,
            modifiers: Vec::new(),
            scroll_delta_y: None,
            scroll_delta_x: None,
            app_name: None,
            url: None,
            path: None,
            wait_ms: None,
            requires_confirmation: false,
            summary: None,
            plan_steps: Vec::new(),
            follow_up_actions: Vec::new(),
        }
    }

    #[test]
    fn resolves_click_from_target_id_with_dimensions_and_explicit_norm() {
        let metrics = sample_metrics();
        let target_map = merge_targets(
            vec![RawTarget {
                role: "Button".to_string(),
                label: "Compose".to_string(),
                is_interactive: true,
                x: 200.0,
                y: 340.0,
                width: 100.0,
                height: 40.0,
                center_x: 250.0,
                center_y: 360.0,
            }],
            Vec::new(),
            metrics,
        );

        let mut d_ocr = base_decision("click");
        d_ocr.target_ocr_id = Some(1);
        let (action, (ox, oy), dims, text) =
            resolve_action_from_decision(&d_ocr, &target_map, metrics).expect("target resolve");
        assert!((ox - 250.0).abs() < 1e-6);
        assert!((oy - 360.0).abs() < 1e-6);
        assert_eq!(dims, Some((100.0, 40.0)));
        assert_eq!(text.as_deref(), Some("Compose"));
        assert_eq!(
            action,
            AgentAction::Click {
                x: 350.0,
                y: 410.0,
                button: MouseButtonKind::Left,
                double: false,
            }
        );

        let mut d_norm = base_decision("right_click");
        d_norm.x_norm = Some(0.4);
        d_norm.y_norm = Some(0.25);
        let (action2, (ox2, oy2), dims2, text2) =
            resolve_action_from_decision(&d_norm, &target_map, metrics).expect("norm resolve");
        assert!((ox2 - 400.0).abs() < 1e-6);
        assert!((oy2 - 200.0).abs() < 1e-6);
        assert!(dims2.is_none());
        assert!(text2.is_none());
        assert_eq!(
            action2,
            AgentAction::Click {
                x: 500.0,
                y: 250.0,
                button: MouseButtonKind::Right,
                double: false,
            }
        );
    }

    #[test]
    fn rejects_click_with_missing_or_invalid_target_instead_of_clicking_center() {
        let metrics = sample_metrics();
        let empty_map = ScreenTargetMap::default();

        let d_missing = base_decision("click");
        assert!(resolve_action_from_decision(&d_missing, &empty_map, metrics).is_err());

        let mut d_bad_id = base_decision("click");
        d_bad_id.target_ocr_id = Some(99);
        assert!(resolve_action_from_decision(&d_bad_id, &empty_map, metrics).is_err());
    }

    #[test]
    fn resolves_expanded_tools_and_detects_duplicate_urls() {
        let target_map = ScreenTargetMap::default();
        let metrics = sample_metrics();

        let mut d_url = base_decision("open_url");
        d_url.url = Some("https://github.com/trending/".to_string());
        d_url.app_name = Some("Safari".to_string());
        let (act_url, _, _, _) =
            resolve_action_from_decision(&d_url, &target_map, metrics).unwrap();
        assert_eq!(
            act_url,
            AgentAction::OpenUrl {
                url: "https://github.com/trending/".to_string(),
                app_name: Some("Safari".to_string()),
            }
        );

        let norm = normalize_url_for_dedup("https://github.com/trending/");
        assert!(is_duplicate_url_action(&act_url, Some(&norm), true));

        let dup_type = AgentAction::TypeText {
            text: "github.com/trending".to_string(),
            replace: true,
            submit: true,
        };
        assert!(is_duplicate_url_action(&dup_type, Some(&norm), true));
        assert!(!is_duplicate_url_action(&dup_type, Some(&norm), false));

        let mut d_drag = base_decision("drag");
        d_drag.from_x_norm = Some(0.2);
        d_drag.from_y_norm = Some(0.5);
        d_drag.to_x_norm = Some(0.6);
        d_drag.to_y_norm = Some(0.5);
        let (act_drag, (ox, oy), _, _) =
            resolve_action_from_decision(&d_drag, &target_map, metrics).unwrap();
        assert!((ox - 200.0).abs() < 1e-6);
        assert!((oy - 400.0).abs() < 1e-6);
        assert_eq!(
            act_drag,
            AgentAction::Drag {
                from_x: 300.0,
                from_y: 450.0,
                to_x: 700.0,
                to_y: 450.0,
            }
        );

        let mut d_paste = base_decision("paste_text");
        d_paste.text = Some("Hello\nWorld".to_string());
        d_paste.replace = true;
        d_paste.submit = true;
        let (act_paste, _, _, _) =
            resolve_action_from_decision(&d_paste, &target_map, metrics).unwrap();
        assert_eq!(
            act_paste,
            AgentAction::PasteText {
                text: "Hello\nWorld".to_string(),
                replace: true,
                submit: true,
            }
        );

        let d_clip = base_decision("read_clipboard");
        let (act_clip, _, _, _) =
            resolve_action_from_decision(&d_clip, &target_map, metrics).unwrap();
        assert_eq!(act_clip, AgentAction::ReadClipboard);

        let mut d_path = base_decision("open_path");
        d_path.path = Some("~/Desktop".to_string());
        let (act_path, _, _, _) =
            resolve_action_from_decision(&d_path, &target_map, metrics).unwrap();
        assert_eq!(
            act_path,
            AgentAction::OpenPath {
                path: "~/Desktop".to_string()
            }
        );
    }

    #[test]
    fn detects_direct_repeats_and_two_step_cycles() {
        let h1 = vec!["a".to_string()];
        assert_eq!(detect_repetition_level(&h1, "b"), RepetitionLevel::None);
        assert_eq!(detect_repetition_level(&h1, "a"), RepetitionLevel::Warn);

        let h3 = vec!["a".to_string(), "a".to_string(), "a".to_string()];
        assert_eq!(detect_repetition_level(&h3, "a"), RepetitionLevel::Abort);

        let cycle2 = vec!["click|23".to_string(), "type|0".to_string(), "click|23".to_string()];
        assert_eq!(detect_repetition_level(&cycle2, "type|0"), RepetitionLevel::Warn);

        let cycle3 = vec![
            "click|23".to_string(),
            "type|0".to_string(),
            "click|23".to_string(),
            "type|0".to_string(),
            "click|23".to_string(),
        ];
        assert_eq!(detect_repetition_level(&cycle3, "type|0"), RepetitionLevel::Abort);
    }

    #[test]
    fn builds_batched_action_queue_and_evaluates_local_verification() {
        let mut primary = base_decision("open_app");
        primary.app_name = Some("Notes".to_string());
        primary.plan_step_index = 2;
        primary.follow_up_actions = vec![
            crate::classifier::FollowUpAction {
                narration: "New note".to_string(),
                action_type: "press_key".to_string(),
                target_ocr_id: None,
                x_norm: None,
                y_norm: None,
                from_x_norm: None,
                from_y_norm: None,
                to_x_norm: None,
                to_y_norm: None,
                text: None,
                replace: false,
                submit: false,
                key: Some("n".to_string()),
                modifiers: vec!["cmd".to_string()],
                scroll_delta_y: None,
                scroll_delta_x: None,
                app_name: None,
                url: None,
                path: None,
                wait_ms: None,
                requires_confirmation: false,
            },
            crate::classifier::FollowUpAction {
                narration: "Paste list".to_string(),
                action_type: "paste_text".to_string(),
                target_ocr_id: None,
                x_norm: None,
                y_norm: None,
                from_x_norm: None,
                from_y_norm: None,
                to_x_norm: None,
                to_y_norm: None,
                text: Some("- Item 1".to_string()),
                replace: false,
                submit: false,
                key: None,
                modifiers: Vec::new(),
                scroll_delta_y: None,
                scroll_delta_x: None,
                app_name: None,
                url: None,
                path: None,
                wait_ms: None,
                requires_confirmation: false,
            },
        ];

        let queue = build_turn_action_queue(primary);
        assert_eq!(queue.len(), 3);
        assert_eq!(queue[0].action_type, "open_app");
        assert_eq!(queue[1].action_type, "press_key");
        assert_eq!(queue[1].plan_step_index, 2);
        assert_eq!(queue[2].action_type, "paste_text");

        let before = ActiveWindowMetadata {
            app_name: "Finder".to_string(),
            window_title: "Desktop".to_string(),
            process_id: 1,
            is_system_locked: false,
        };
        let same_after = before.clone();
        let diff_app = ActiveWindowMetadata {
            app_name: "Notes".to_string(),
            window_title: "Notes".to_string(),
            process_id: 2,
            is_system_locked: false,
        };

        let stalled = evaluate_local_verification(&before, &same_after, 0.1, None, None);
        assert!(stalled.step_stalled);
        assert_eq!(stalled.step_status, "unverified_failed");

        let app_switched = evaluate_local_verification(&before, &diff_app, 0.1, None, None);
        assert!(!app_switched.step_stalled);
        assert_eq!(app_switched.step_status, "verified");

        let visual_changed = evaluate_local_verification(&before, &same_after, 2.5, None, None);
        assert!(!visual_changed.step_stalled);
        assert_eq!(visual_changed.step_status, "verified");

        let errored = evaluate_local_verification(&before, &diff_app, 5.0, None, Some("boom"));
        assert!(errored.step_stalled);
        assert_eq!(errored.step_status, "failed");
    }

    #[test]
    fn allows_progressing_repeatable_actions_and_aborts_excessive_toggles() {
        assert!(is_naturally_repeatable_action(&AgentAction::Scroll {
            x: 100.0,
            y: 200.0,
            delta_y: -5,
            delta_x: 0,
        }));
        assert!(is_naturally_repeatable_action(&AgentAction::Wait { ms: 500 }));
        assert!(is_naturally_repeatable_action(&AgentAction::PressKey {
            key: "pagedown".to_string(),
            modifiers: Vec::new(),
        }));
        assert!(is_naturally_repeatable_action(&AgentAction::PressKey {
            key: "space".to_string(),
            modifiers: Vec::new(),
        }));
        assert!(!is_naturally_repeatable_action(&AgentAction::PressKey {
            key: "l".to_string(),
            modifiers: vec!["cmd".to_string()],
        }));
        assert!(!is_naturally_repeatable_action(&AgentAction::Click {
            x: 100.0,
            y: 200.0,
            button: MouseButtonKind::Left,
            double: false,
        }));
        assert!(!is_naturally_repeatable_action(&AgentAction::OpenUrl {
            url: "https://example.com".to_string(),
            app_name: Some("Safari".to_string()),
        }));

        let five_repeats = vec![
            "click|1".to_string(),
            "click|1".to_string(),
            "click|1".to_string(),
            "click|1".to_string(),
            "click|1".to_string(),
        ];
        assert!(!detect_progressing_cycle_abort(&five_repeats, "click|1"));

        let mut six_repeats = five_repeats;
        six_repeats.push("click|1".to_string());
        assert!(detect_progressing_cycle_abort(&six_repeats, "click|1"));

        let three_toggles = vec![
            "a".to_string(),
            "b".to_string(),
            "a".to_string(),
            "b".to_string(),
            "a".to_string(),
        ];
        assert!(!detect_progressing_cycle_abort(&three_toggles, "b"));

        let four_toggles = vec![
            "a".to_string(),
            "b".to_string(),
            "a".to_string(),
            "b".to_string(),
            "a".to_string(),
            "b".to_string(),
            "a".to_string(),
        ];
        assert!(detect_progressing_cycle_abort(&four_toggles, "b"));
    }

    #[test]
    fn routes_classified_tasks_to_specialist_models_and_fallbacks() {
        let routing = ModelRoutingConfig::default();

        let coding_cls = fallback_classification("Open Terminal and run git status");
        assert_eq!(coding_cls.category, "coding");
        let (role_code, model_code) = select_actor_for_classification(&coding_cls, &routing);
        assert_eq!(role_code, "coding_actor");
        assert_eq!(model_code, routing.coding_actor_model);

        let writing_cls = fallback_classification("Open Notes and write a sprint checklist");
        assert_eq!(writing_cls.category, "writing");
        let (role_write, model_write) = select_actor_for_classification(&writing_cls, &routing);
        assert_eq!(role_write, "writing_actor");
        assert_eq!(model_write, routing.writing_actor_model);

        let browser_cls = fallback_classification("Open Safari to github.com/trending");
        assert_eq!(browser_cls.category, "browser_research");
        let (role_vis, model_vis) = select_actor_for_classification(&browser_cls, &routing);
        assert_eq!(role_vis, "vision_actor");
        assert_eq!(model_vis, routing.vision_actor_model);

        let general_cls = fallback_classification("Open Calculator and compute 42 * 18");
        assert_eq!(general_cls.category, "general_ui");
        assert!(!general_cls.needs_dedicated_planner);
    }
}
