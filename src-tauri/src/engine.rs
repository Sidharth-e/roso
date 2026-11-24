use crate::automation::{
    action_type_label, assess_action_risk, describe_action, execute_action,
    has_accessibility_permission, is_kill_switch_pressed, AgentAction, MouseButtonKind,
};
use crate::capture::{
    capture_primary_display_in_ram, has_screen_recording_permission, render_annotated_jpeg_base64,
    ScreenMetrics,
};
use crate::classifier::{
    AgentError, NextActionContext, OllamaAgentClient, PlannedActionDecision,
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
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::{oneshot, watch};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Idle,
    Planning,
    Running,
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
            Self::Planning | Self::Running | Self::AwaitingConfirmation | Self::Paused
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

        let (init_jpeg, init_win, _, init_targets) = match initial_capture {
            Ok(Ok(tuple)) => tuple,
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

        let plan_steps = match self
            .client
            .create_task_plan(
                &cfg.ollama_base_url,
                &api_key,
                &cfg.vision_model,
                &goal,
                &init_win.app_name,
                &init_targets.formatted_lines,
                init_jpeg,
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
                st.active_app = init_win.app_name;
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

            let capture_res = tokio::task::spawn_blocking(capture_and_ground_screen).await;

            let (annotated_jpeg, active_win, metrics, target_map) = match capture_res {
                Ok(Ok(tuple)) => tuple,
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

            if active_win.is_system_locked {
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
                        active_app: active_win.app_name.clone(),
                        window_title: active_win.window_title.clone(),
                        screen_width: metrics.logical_width.round() as u32,
                        screen_height: metrics.logical_height.round() as u32,
                        ocr_lines: target_map.formatted_lines.clone(),
                        recent_steps: recent_steps.clone(),
                        base64_jpeg: annotated_jpeg,
                    },
                )
                .await;

            let decision = match decision_res {
                Ok(d) => d,
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

            if *stop_rx.borrow() || !self.is_current_task(&task_id) {
                return;
            }

            let (action, (overlay_x, overlay_y), target_dims, target_text) =
                match resolve_action_from_decision(&decision, &target_map, metrics) {
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
                        };
                        self.record_step(
                            &app_handle,
                            &task_id,
                            step_num,
                            decision.plan_step_index,
                            &active_win.app_name,
                            step_rec,
                        );
                        recent_steps.push(format!(
                            "Step {step_num} skipped: {reason}. Pick an exact [N] ID from Visible targets or valid x_norm/y_norm."
                        ));
                        if recent_steps.len() > 8 {
                            recent_steps.remove(0);
                        }
                        continue;
                    }
                };

            let action_label = action_type_label(&action).to_string();
            let action_desc = describe_action(&action);
            let (target_width, target_height) = match target_dims {
                Some((w, h)) => (Some(w), Some(h)),
                None => (None, None),
            };

            match &action {
                AgentAction::Done { summary } => {
                    let cue = PointerCuePayload {
                        id: Uuid::new_v4().to_string(),
                        message: summary.clone(),
                        action_badge: "Done".to_string(),
                        target_x: overlay_x,
                        target_y: overlay_y,
                        target_width,
                        target_height,
                        target_ocr_id: decision.target_ocr_id,
                        step_number: step_num,
                        is_acting: false,
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
                        target_x: Some(overlay_x),
                        target_y: Some(overlay_y),
                    };
                    self.record_step(
                        &app_handle,
                        &task_id,
                        step_num,
                        decision.plan_step_index,
                        &active_win.app_name,
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
                        target_x: overlay_x,
                        target_y: overlay_y,
                        target_width,
                        target_height,
                        target_ocr_id: decision.target_ocr_id,
                        step_number: step_num,
                        is_acting: false,
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
                        target_x: Some(overlay_x),
                        target_y: Some(overlay_y),
                    };
                    self.record_step(
                        &app_handle,
                        &task_id,
                        step_num,
                        decision.plan_step_index,
                        &active_win.app_name,
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
                target_x: overlay_x,
                target_y: overlay_y,
                target_width,
                target_height,
                target_ocr_id: decision.target_ocr_id,
                step_number: step_num,
                is_acting: true,
                dismiss_after_ms: 3600,
            };
            let _ = emit_pointer_cue(&app_handle, cue);

            {
                let mut st = self.state.lock();
                if st.task_id.as_deref() == Some(&task_id) && st.phase.is_active() {
                    st.current_step = step_num;
                    st.active_plan_index = decision.plan_step_index;
                    st.active_app = active_win.app_name.clone();
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
                            target_x: Some(overlay_x),
                            target_y: Some(overlay_y),
                        };
                        self.record_step(
                            &app_handle,
                            &task_id,
                            step_num,
                            decision.plan_step_index,
                            &active_win.app_name,
                            step_rec,
                        );
                        recent_steps
                            .push(format!("Step {step_num} ({action_desc}) was denied by user"));
                        if recent_steps.len() > 8 {
                            recent_steps.remove(0);
                        }
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
            let (step_status, err_text) = match exec_res {
                Ok(Ok(())) => ("executed".to_string(), None),
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
                            target_x: Some(overlay_x),
                            target_y: Some(overlay_y),
                        };
                        self.record_step(
                            &app_handle,
                            &task_id,
                            step_num,
                            decision.plan_step_index,
                            &active_win.app_name,
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
                    ("failed".to_string(), Some(msg))
                }
                Err(err) => ("failed".to_string(), Some(err.to_string())),
            };

            let step_rec = TaskStepRecord {
                step_number: step_num,
                timestamp: Utc::now(),
                action_type: action_label,
                description: action_desc.clone(),
                narration: decision.narration.clone(),
                status: step_status.clone(),
                target_ocr_id: decision.target_ocr_id,
                target_x: Some(overlay_x),
                target_y: Some(overlay_y),
            };
            self.record_step(
                &app_handle,
                &task_id,
                step_num,
                decision.plan_step_index,
                &active_win.app_name,
                step_rec,
            );

            let recent_entry = match err_text {
                Some(e) => format!("Step {step_num} ({action_desc}) failed: {e}"),
                None => format!("Step {step_num} ({action_desc}) -> {step_status}"),
            };
            recent_steps.push(recent_entry);
            if recent_steps.len() > 8 {
                recent_steps.remove(0);
            }

            if sleep_interruptible(Duration::from_millis(cycle_cfg.step_delay_ms), &mut stop_rx)
                .await
            {
                return;
            }
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

fn capture_and_ground_screen() -> Result<
    (
        String,
        crate::capture::ActiveWindowMetadata,
        ScreenMetrics,
        ScreenTargetMap,
    ),
    crate::capture::CaptureError,
> {
    let frame = capture_primary_display_in_ram()?;
    let target_map = build_unified_targets(
        &frame.full_res_rgba,
        frame.active_window.process_id,
        frame.screen_metrics,
    );
    let annotated_jpeg =
        render_annotated_jpeg_base64(&frame.full_res_rgba, &target_map.annotation_boxes)
            .unwrap_or(frame.base64_jpeg);
    Ok((
        annotated_jpeg,
        frame.active_window,
        frame.screen_metrics,
        target_map,
    ))
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
        "scroll" => {
            let ((ox, oy), (ax, ay), dims, text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((
                AgentAction::Scroll {
                    x: ax,
                    y: ay,
                    delta_y: decision.scroll_delta_y.unwrap_or(-5),
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
            Ok((AgentAction::TypeText { text }, (ox, oy), dims, target_text))
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
        "open_url" => {
            let url = decision
                .url
                .clone()
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
                .ok_or_else(|| "open_url missing url".to_string())?;
            let ((ox, oy), _, dims, target_text) =
                resolve_target_optional(decision, target_map, metrics);
            Ok((AgentAction::OpenUrl { url }, (ox, oy), dims, target_text))
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

fn action_badge_text(action: &AgentAction) -> &'static str {
    match action {
        AgentAction::Click { double: true, .. } => "Double-Click",
        AgentAction::Click {
            button: MouseButtonKind::Right,
            ..
        } => "Right-Click",
        AgentAction::Click { .. } => "Click",
        AgentAction::TypeText { .. } => "Type",
        AgentAction::PressKey { .. } => "Shortcut",
        AgentAction::Scroll { .. } => "Scroll",
        AgentAction::OpenApp { .. } => "Open App",
        AgentAction::OpenUrl { .. } => "Open URL",
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
            text: None,
            key: None,
            modifiers: Vec::new(),
            scroll_delta_y: None,
            app_name: None,
            url: None,
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
    fn resolves_open_app_open_url_and_press_key() {
        let target_map = ScreenTargetMap::default();
        let metrics = sample_metrics();

        let mut d_app = base_decision("open_app");
        d_app.app_name = Some("Notes".to_string());
        let (act_app, _, _, _) =
            resolve_action_from_decision(&d_app, &target_map, metrics).unwrap();
        assert_eq!(
            act_app,
            AgentAction::OpenApp {
                app_name: "Notes".to_string()
            }
        );

        let mut d_url = base_decision("open_url");
        d_url.url = Some("https://github.com".to_string());
        let (act_url, _, _, _) =
            resolve_action_from_decision(&d_url, &target_map, metrics).unwrap();
        assert_eq!(
            act_url,
            AgentAction::OpenUrl {
                url: "https://github.com".to_string()
            }
        );

        let mut d_key = base_decision("press_key");
        d_key.key = Some("enter".to_string());
        d_key.modifiers = vec!["cmd".to_string()];
        let (act_key, _, _, _) =
            resolve_action_from_decision(&d_key, &target_map, metrics).unwrap();
        assert_eq!(
            act_key,
            AgentAction::PressKey {
                key: "enter".to_string(),
                modifiers: vec!["cmd".to_string()],
            }
        );
    }
}
