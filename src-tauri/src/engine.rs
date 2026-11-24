use crate::automation::{
    action_type_label, assess_action_risk, describe_action, execute_action,
    has_accessibility_permission, is_kill_switch_pressed, AgentAction, MouseButtonKind,
};
use crate::capture::{
    capture_primary_display_in_ram, compute_frame_diff_ratio, has_screen_recording_permission,
    render_annotated_jpeg_base64, ActiveWindowMetadata, ScreenMetrics,
};
use crate::classifier::{
    AgentError, NextActionContext, OllamaAgentClient, PlannedActionDecision, RecoveryContext,
    VerifyStepContext,
};
use crate::config::{
    append_history_record, AppConfig, TaskHistoryRecord, TaskStepRecord,
};
use crate::ocr::{build_unified_targets, ScreenTargetMap};
use crate::overlay::{
    emit_pointer_cue, hide_tray_popover, show_tray_popover, update_tray_status_title,
    PointerCuePayload,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Idle,
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
            Self::Planning
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
}

struct TaskRuntimeState {
    phase: TaskPhase,
    task_id: Option<String>,
    goal: String,
    started_at: Option<DateTime<Utc>>,
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
    history_saved: bool,
}

impl TaskRuntimeState {
    fn idle(max_steps: u32) -> Self {
        Self {
            phase: TaskPhase::Idle,
            task_id: None,
            goal: String::new(),
            started_at: None,
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
        let max_steps = initial_config.max_steps;
        Ok(Arc::new(Self {
            config: Mutex::new(initial_config),
            state: Mutex::new(TaskRuntimeState::idle(max_steps)),
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
        let next_max = next.max_steps;
        *self.config.lock() = next;
        let mut st = self.state.lock();
        if !st.phase.is_active() {
            st.max_steps = next_max;
        }
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let st = self.state.lock();
        EngineSnapshot {
            phase: st.phase,
            task_id: st.task_id.clone(),
            goal: st.goal.clone(),
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
        }
    }

    pub fn start_task(self: &Arc<Self>, app_handle: AppHandle, goal: String) -> EngineSnapshot {
        self.stop_task(&app_handle, Some("Replaced by new task".to_string()));

        let cfg = self.config();
        let task_id = Uuid::new_v4().to_string();

        {
            let mut st = self.state.lock();
            st.phase = TaskPhase::Planning;
            st.task_id = Some(task_id.clone());
            st.goal = goal.clone();
            st.started_at = Some(Utc::now());
            st.plan_steps = Vec::new();
            st.active_plan_index = 0;
            st.current_step = 0;
            st.max_steps = cfg.max_steps;
            st.recovery_count = 0;
            st.last_diagnosis = None;
            st.active_app = String::new();
            st.last_narration = "Inspecting your screen to plan the task...".to_string();
            st.last_summary = None;
            st.last_error = None;
            st.pending_confirmation = None;
            st.steps.clear();
            st.history_saved = false;
        }

        let (stop_tx, stop_rx) = watch::channel(false);
        let (pause_tx, pause_rx) = watch::channel(false);
        *self.stop_tx.lock() = Some(stop_tx);
        *self.pause_tx.lock() = Some(pause_tx);
        *self.confirm_tx.lock() = None;

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

        let cfg = self.config();
        let api_key = cfg.resolved_api_key();

        let initial_capture = tokio::task::spawn_blocking(capture_and_ground_screen).await;

        let init_cap = match initial_capture {
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
                return;
            }
        };

        if *stop_rx.borrow() || !self.is_current_task(&task_id) {
            return;
        }

        let mut plan_steps = match self
            .client
            .create_task_plan(
                &cfg.ollama_base_url,
                &api_key,
                &cfg.vision_model,
                &goal,
                &init_cap.active_window.app_name,
                &init_cap.target_map.formatted_lines,
                init_cap.annotated_jpeg,
            )
            .await
        {
            Ok(plan) => plan.steps,
            Err(_) => vec![goal.clone()],
        };

        if *stop_rx.borrow() || !self.is_current_task(&task_id) {
            return;
        }

        {
            let mut st = self.state.lock();
            if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                st.plan_steps = plan_steps.clone();
                st.active_app = init_cap.active_window.app_name;
                st.phase = TaskPhase::Running;
                st.last_narration = format!("Plan ready ({} steps). Starting...", plan_steps.len());
            }
        }
        let snap = self.snapshot();
        update_tray_status_title(&app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);

        hide_tray_popover(&app_handle);
        if sleep_interruptible(Duration::from_millis(320), &mut stop_rx).await {
            return;
        }

        let max_steps = cfg.max_steps;
        let mut recent_steps: Vec<String> = Vec::new();
        let mut action_signatures: Vec<String> = Vec::new();
        let mut last_pointer_pos: Option<(f64, f64)> = None;
        let mut consecutive_agent_errors: u32 = 0;
        let mut recovery_count: u32 = 0;
        let mut last_opened_url: Option<String> = None;
        let mut last_action_was_open_url = false;
        let mut cached_capture: Option<GroundedCapture> = None;

        for step_num in 1..=max_steps {
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

            let cycle_cfg = self.config();
            let cycle_api_key = cycle_cfg.resolved_api_key();

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

            let decision_res = self
                .client
                .decide_next_action(
                    &cycle_cfg.ollama_base_url,
                    &cycle_api_key,
                    &cycle_cfg.vision_model,
                    NextActionContext {
                        goal: goal.clone(),
                        plan_steps: plan_steps.clone(),
                        step_number: step_num,
                        max_steps,
                        active_app: before_cap.active_window.app_name.clone(),
                        window_title: before_cap.active_window.window_title.clone(),
                        screen_width: before_cap.screen_metrics.logical_width.round() as u32,
                        screen_height: before_cap.screen_metrics.logical_height.round() as u32,
                        ocr_lines: before_cap.target_map.formatted_lines.clone(),
                        recent_steps: recent_steps.clone(),
                        base64_jpeg: before_cap.annotated_jpeg.clone(),
                    },
                )
                .await;

            let decision = match decision_res {
                Ok(d) => {
                    consecutive_agent_errors = 0;
                    d
                }
                Err(err) => {
                    consecutive_agent_errors += 1;
                    let msg = err.to_string();
                    if consecutive_agent_errors < 3 {
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
                        if sleep_interruptible(Duration::from_millis(750), &mut stop_rx).await {
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

            let metrics = before_cap.screen_metrics;
            let (action, (overlay_x, overlay_y), target_dims, target_text) =
                match resolve_action_from_decision(&decision, &before_cap.target_map, metrics) {
                    Ok(v) => v,
                    Err(reason) => {
                        let step_rec = TaskStepRecord {
                            step_number: step_num,
                            timestamp: Utc::now(),
                            action_type: decision.action_type.clone(),
                            description: format!("Invalid target: {reason}"),
                            narration: decision.narration.clone(),
                            status: "skipped".to_string(),
                            target_ocr_id: decision.target_ocr_id,
                            target_x: None,
                            target_y: None,
                            verification_note: Some(reason.clone()),
                        };
                        self.record_step(
                            &app_handle,
                            &task_id,
                            step_num,
                            decision.plan_step_index,
                            &before_cap.active_window.app_name,
                            step_rec,
                        );
                        push_recent_step(
                            &mut recent_steps,
                            format!(
                                "Step {step_num} skipped: {reason}. Pick an exact [N] ID from Visible targets or valid x_norm/y_norm."
                            ),
                        );
                        continue;
                    }
                };

            let action_label = action_type_label(&action).to_string();
            let action_desc = describe_action(&action);
            let is_terminal = matches!(action, AgentAction::Done { .. } | AgentAction::Fail { .. });

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
                    target_ocr_id: decision.target_ocr_id,
                    target_x: None,
                    target_y: None,
                    verification_note: Some("Duplicate URL entry blocked".to_string()),
                };
                self.record_step(
                    &app_handle,
                    &task_id,
                    step_num,
                    decision.plan_step_index,
                    &before_cap.active_window.app_name,
                    step_rec,
                );
                last_action_was_open_url = false;
                push_recent_step(
                    &mut recent_steps,
                    "DUPLICATE URL BLOCKED: Already opened/entered this URL—wait or proceed to the next step (e.g. scroll or done)."
                        .to_string(),
                );
                continue;
            }

            let action_sig = format!(
                "{action_label}|{}|{action_desc}",
                decision.target_ocr_id.unwrap_or(0)
            );
            let mut repetition_warned = false;

            if !is_terminal {
                match detect_repetition_level(&action_signatures, &action_sig) {
                    RepetitionLevel::Abort => {
                        let msg =
                            "Stopped: repeated the same action without making progress.".to_string();
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
                            target_ocr_id: decision.target_ocr_id,
                            target_x: None,
                            target_y: None,
                            verification_note: Some("Loop abort".to_string()),
                        };
                        self.record_step(
                            &app_handle,
                            &task_id,
                            step_num,
                            decision.plan_step_index,
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
                        push_recent_step(
                            &mut recent_steps,
                            "LOOP WARNING: You repeated this action without changing the UI. Use open_url with app_name, paste_text with replace=true, or press_key cmd+l."
                                .to_string(),
                        );
                    }
                    RepetitionLevel::None => {}
                }
            }

            let has_explicit_target = decision.target_ocr_id.is_some()
                || (decision.x_norm.is_some() && decision.y_norm.is_some())
                || (decision.from_x_norm.is_some() && decision.from_y_norm.is_some());
            let show_pointer = matches!(action, AgentAction::Click { .. } | AgentAction::Drag { .. })
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
                        narration: decision.narration.clone(),
                        status: "done".to_string(),
                        target_ocr_id: decision.target_ocr_id,
                        target_x: Some(cue_x),
                        target_y: Some(cue_y),
                        verification_note: Some("Task marked complete".to_string()),
                    };
                    self.record_step(
                        &app_handle,
                        &task_id,
                        step_num,
                        decision.plan_step_index,
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
                        narration: decision.narration.clone(),
                        status: "failed".to_string(),
                        target_ocr_id: decision.target_ocr_id,
                        target_x: Some(cue_x),
                        target_y: Some(cue_y),
                        verification_note: Some(reason.clone()),
                    };
                    self.record_step(
                        &app_handle,
                        &task_id,
                        step_num,
                        decision.plan_step_index,
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
                message: decision.narration.clone(),
                action_badge: action_badge_text(&action).to_string(),
                target_x: cue_x,
                target_y: cue_y,
                target_width,
                target_height,
                target_ocr_id: if show_pointer {
                    decision.target_ocr_id
                } else {
                    None
                },
                step_number: step_num,
                is_acting: show_pointer,
                show_pointer,
                dismiss_after_ms: 3600,
            };
            let _ = emit_pointer_cue(&app_handle, cue);

            {
                let mut st = self.state.lock();
                if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                    st.phase = TaskPhase::Running;
                    st.current_step = step_num;
                    st.active_plan_index = decision.plan_step_index;
                    st.active_app = before_cap.active_window.app_name.clone();
                    st.last_narration = decision.narration.clone();
                }
            }
            let snap = self.snapshot();
            update_tray_status_title(&app_handle, Some(&snap));
            let _ = app_handle.emit("engine-snapshot", &snap);

            if cycle_cfg.confirm_risky_actions {
                if let Some(risk_reason) = assess_action_risk(
                    &action,
                    &decision.narration,
                    target_text.as_deref(),
                    decision.requires_confirmation,
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
                            narration: decision.narration.clone(),
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
                            narration: decision.narration.clone(),
                            status: "denied".to_string(),
                            target_ocr_id: decision.target_ocr_id,
                            target_x: Some(cue_x),
                            target_y: Some(cue_y),
                            verification_note: Some("Denied by user".to_string()),
                        };
                        self.record_step(
                            &app_handle,
                            &task_id,
                            step_num,
                            decision.plan_step_index,
                            &before_cap.active_window.app_name,
                            step_rec,
                        );
                        push_recent_step(
                            &mut recent_steps,
                            format!("Step {step_num} ({action_desc}) was denied by user"),
                        );
                        hide_tray_popover(&app_handle);
                        continue;
                    }

                    hide_tray_popover(&app_handle);
                    if sleep_interruptible(Duration::from_millis(250), &mut stop_rx).await {
                        return;
                    }
                }
            }

            if sleep_interruptible(Duration::from_millis(280), &mut stop_rx).await {
                return;
            }
            if !self.is_current_task(&task_id) {
                return;
            }

            let action_for_exec = action.clone();
            let exec_res =
                tokio::task::spawn_blocking(move || execute_action(&action_for_exec, metrics)).await;
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
                            narration: decision.narration.clone(),
                            status: "failed".to_string(),
                            target_ocr_id: decision.target_ocr_id,
                            target_x: Some(cue_x),
                            target_y: Some(cue_y),
                            verification_note: Some(msg.clone()),
                        };
                        self.record_step(
                            &app_handle,
                            &task_id,
                            step_num,
                            decision.plan_step_index,
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
                narration: decision.narration.clone(),
                status: step_status.clone(),
                target_ocr_id: decision.target_ocr_id,
                target_x: Some(cue_x),
                target_y: Some(cue_y),
                verification_note: err_text.clone().or_else(|| action_output_note.clone()),
            };
            self.record_step(
                &app_handle,
                &task_id,
                step_num,
                decision.plan_step_index,
                &before_cap.active_window.app_name,
                step_rec,
            );
            action_signatures.push(action_sig);

            if matches!(action, AgentAction::Wait { .. }) {
                cached_capture = None;
                push_recent_step(
                    &mut recent_steps,
                    format!(
                        "Step {step_num} ({action_desc}) in \"{}\" -> {step_status}",
                        before_cap.active_window.window_title
                    ),
                );
                if sleep_interruptible(Duration::from_millis(cycle_cfg.step_delay_ms), &mut stop_rx)
                    .await
                {
                    return;
                }
                continue;
            }

            let settle_ms = action_settle_delay_ms(&action, cycle_cfg.step_delay_ms);
            if sleep_interruptible(Duration::from_millis(settle_ms), &mut stop_rx).await {
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
                            "Step {step_num} ({action_desc}) in \"{}\" -> {step_status}",
                            before_cap.active_window.window_title
                        ),
                    );
                    continue;
                }
            };

            let visual_diff_pct =
                compute_frame_diff_ratio(&before_cap.full_res_rgba, &after_cap.full_res_rgba)
                    * 100.0;

            {
                let mut st = self.state.lock();
                if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                    st.phase = TaskPhase::Verifying;
                    st.active_app = after_cap.active_window.app_name.clone();
                    st.last_narration = format!("Verifying step #{step_num}...");
                }
            }
            let ver_snap = self.snapshot();
            update_tray_status_title(&app_handle, Some(&ver_snap));
            let _ = app_handle.emit("engine-snapshot", &ver_snap);

            let ver_res = self
                .client
                .verify_step_outcome(
                    &cycle_cfg.ollama_base_url,
                    &cycle_api_key,
                    &cycle_cfg.vision_model,
                    VerifyStepContext {
                        goal: goal.clone(),
                        step_number: step_num,
                        action_description: action_desc.clone(),
                        expected_narration: decision.narration.clone(),
                        before_app: before_cap.active_window.app_name.clone(),
                        after_app: after_cap.active_window.app_name.clone(),
                        before_title: before_cap.active_window.window_title.clone(),
                        after_title: after_cap.active_window.window_title.clone(),
                        visual_diff_pct,
                        action_output: action_output_note.clone(),
                        visible_targets: after_cap.target_map.formatted_lines.clone(),
                        after_base64_jpeg: after_cap.annotated_jpeg.clone(),
                    },
                )
                .await;

            if *stop_rx.borrow() || !self.is_current_task(&task_id) {
                return;
            }

            let mut verification_failed = err_text.is_some();
            let mut observation_summary = err_text
                .clone()
                .unwrap_or_else(|| format!("Visual diff {visual_diff_pct:.1}%"));
            let mut recovery_hint: Option<String> = None;

            match ver_res {
                Ok(ver) => {
                    observation_summary = ver.observation.clone();
                    recovery_hint = ver.recovery_hint.clone();
                    let note_text = match &action_output_note {
                        Some(out_note) => format!("{} ({out_note})", ver.observation),
                        None => ver.observation.clone(),
                    };

                    let final_step_status = if ver.status == "succeeded" || ver.status == "partial"
                    {
                        if err_text.is_some() {
                            "failed"
                        } else {
                            "verified"
                        }
                    } else {
                        verification_failed = true;
                        "unverified_failed"
                    };

                    self.update_last_step_verification(
                        &app_handle,
                        &task_id,
                        step_num,
                        final_step_status,
                        note_text,
                    );

                    if ver.goal_achieved && err_text.is_none() {
                        let done_cue = PointerCuePayload {
                            id: Uuid::new_v4().to_string(),
                            message: ver.observation.clone(),
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
                        let _ = emit_pointer_cue(&app_handle, done_cue);
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
                }
                Err(_) => {
                    if let Some(out_note) = &action_output_note {
                        observation_summary = out_note.clone();
                    }
                }
            }

            let out_suffix = match &action_output_note {
                Some(note) => format!(" | {note}"),
                None => String::new(),
            };
            push_recent_step(
                &mut recent_steps,
                format!(
                    "Step {step_num} ({action_desc}) in \"{}\" -> {} (diff {visual_diff_pct:.1}%): {observation_summary}{out_suffix}",
                    after_cap.active_window.window_title,
                    if verification_failed { "needs fix" } else { "ok" }
                ),
            );

            if (verification_failed || repetition_warned) && recovery_count < MAX_RECOVERY_PLANS
            {
                if matches!(action, AgentAction::OpenUrl { .. }) {
                    last_opened_url = None;
                }

                {
                    let mut st = self.state.lock();
                    if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                        st.phase = TaskPhase::Recovering;
                        st.last_narration =
                            "Step didn't land as expected—building a recovery plan...".to_string();
                    }
                }
                let rec_snap = self.snapshot();
                update_tray_status_title(&app_handle, Some(&rec_snap));
                let _ = app_handle.emit("engine-snapshot", &rec_snap);

                let rec_cue = PointerCuePayload {
                    id: Uuid::new_v4().to_string(),
                    message: "Adapting with a recovery plan...".to_string(),
                    action_badge: "Recovering".to_string(),
                    target_x: cue_x,
                    target_y: cue_y,
                    target_width: None,
                    target_height: None,
                    target_ocr_id: None,
                    step_number: step_num,
                    is_acting: false,
                    show_pointer: false,
                    dismiss_after_ms: 3000,
                };
                let _ = emit_pointer_cue(&app_handle, rec_cue);

                if let Ok(rec_plan) = self
                    .client
                    .create_recovery_plan(
                        &cycle_cfg.ollama_base_url,
                        &cycle_api_key,
                        &cycle_cfg.vision_model,
                        RecoveryContext {
                            goal: goal.clone(),
                            failed_action: action_desc.clone(),
                            observation: observation_summary.clone(),
                            recovery_hint,
                            active_app: after_cap.active_window.app_name.clone(),
                            window_title: after_cap.active_window.window_title.clone(),
                            recent_steps: recent_steps.clone(),
                            visible_targets: after_cap.target_map.formatted_lines.clone(),
                            base64_jpeg: after_cap.annotated_jpeg.clone(),
                        },
                    )
                    .await
                {
                    recovery_count += 1;
                    plan_steps = rec_plan.recovery_steps.clone();
                    {
                        let mut st = self.state.lock();
                        if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                            st.recovery_count = recovery_count;
                            st.last_diagnosis = Some(rec_plan.diagnosis.clone());
                            st.plan_steps = plan_steps.clone();
                            st.active_plan_index = 0;
                            st.phase = TaskPhase::Running;
                            st.last_narration = format!("Recovery plan: {}", rec_plan.diagnosis);
                        }
                    }
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
                    let mut st = self.state.lock();
                    if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                        st.phase = TaskPhase::Running;
                    }
                }
            } else {
                let mut st = self.state.lock();
                if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                    st.phase = TaskPhase::Running;
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
                st.last_narration = summary;
                st.last_error = error;
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
    let frame = capture_primary_display_in_ram()?;
    let target_map = build_unified_targets(
        &frame.full_res_rgba,
        frame.active_window.process_id,
        frame.screen_metrics,
    );
    let annotated_jpeg =
        render_annotated_jpeg_base64(&frame.full_res_rgba, &target_map.annotation_boxes)
            .unwrap_or(frame.base64_jpeg);
    Ok(GroundedCapture {
        full_res_rgba: frame.full_res_rgba,
        annotated_jpeg,
        active_window: frame.active_window,
        screen_metrics: frame.screen_metrics,
        target_map,
    })
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

fn action_settle_delay_ms(action: &AgentAction, base_delay_ms: u64) -> u64 {
    match action {
        AgentAction::OpenUrl { .. }
        | AgentAction::OpenApp { .. }
        | AgentAction::FocusApp { .. }
        | AgentAction::OpenPath { .. } => base_delay_ms.max(1700),
        AgentAction::TypeText { submit: true, .. }
        | AgentAction::PasteText { submit: true, .. } => base_delay_ms.max(1400),
        AgentAction::PressKey { key, .. }
            if matches!(key.trim().to_ascii_lowercase().as_str(), "enter" | "return") =>
        {
            base_delay_ms.max(1400)
        }
        _ => base_delay_ms,
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
}
