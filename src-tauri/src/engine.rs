use crate::capture::{capture_primary_display_in_ram, has_screen_recording_permission};
use crate::classifier::{
    ActivityChoice, ClassificationOutcome, ClassifierError, OllamaCloudClient, SystemOneState,
    MAX_CLASSIFICATION_LATENCY,
};
use crate::config::{
    append_history_record, AppConfig, ProbabilityLogEntry, SessionHistoryRecord,
};
use crate::ocr::{extract_ocr_from_rgba, vision_bbox_to_screen_rect};
use crate::overlay::{emit_nudge_event, update_tray_timer_title, NudgeEventPayload};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use uuid::Uuid;

pub const NORMAL_CHECK_INTERVAL_SECS: u64 = 30;
pub const DEV_CHECK_INTERVAL_SECS: u64 = 10;
pub const DEV_CLOCK_MULTIPLIER: f64 = 4.0;
pub const NORMAL_COOLDOWN_REAL_SECS: f64 = 120.0;
pub const DEV_COOLDOWN_REAL_SECS: f64 = 30.0;
const MAX_TIMELINE_POINTS: usize = 120;
const MAX_SESSION_TICKS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Idle,
    Running,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineSegment {
    pub timestamp: DateTime<Utc>,
    pub status: String,
    pub prob_off_task: f64,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    pub phase: SessionPhase,
    pub session_id: Option<String>,
    pub declared_task: String,
    pub duration_minutes: u32,
    pub remaining_focus_seconds: f64,
    pub elapsed_focus_seconds: f64,
    pub developer_mode: bool,
    pub evidence_focus_seconds: f64,
    pub required_evidence_focus_seconds: f64,
    pub cooldown_remaining_real_seconds: f64,
    pub last_choice: String,
    pub last_prob_off_task: f64,
    pub last_confidence: f64,
    pub active_app: String,
    pub check_count: u32,
    pub nudge_count: u32,
    pub screen_permission_granted: bool,
    pub last_error: Option<String>,
    pub timeline: Vec<TimelineSegment>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObservationDecision {
    ResetEvidence,
    InCooldown,
    Accumulated {
        evidence_focus_seconds: f64,
        required_focus_seconds: f64,
    },
    TriggerNudge {
        evidence_focus_seconds: f64,
        required_focus_seconds: f64,
    },
}

#[derive(Debug, Clone)]
pub struct EvidenceStateMachine {
    pub developer_mode: bool,
    pub evidence_focus_seconds: f64,
    pub cooldown_remaining_real_seconds: f64,
    pub last_required_focus_seconds: f64,
}

impl EvidenceStateMachine {
    pub fn new(developer_mode: bool) -> Self {
        let initial_req = required_off_task_span(0.95, developer_mode);
        Self {
            developer_mode,
            evidence_focus_seconds: 0.0,
            cooldown_remaining_real_seconds: 0.0,
            last_required_focus_seconds: initial_req,
        }
    }

    pub fn set_developer_mode(&mut self, developer_mode: bool) {
        if self.developer_mode != developer_mode {
            self.developer_mode = developer_mode;
            self.reset_evidence();
            if self.cooldown_remaining_real_seconds > cooldown_duration_real_secs(developer_mode) {
                self.cooldown_remaining_real_seconds = cooldown_duration_real_secs(developer_mode);
            }
        }
    }

    pub fn reset_evidence(&mut self) {
        self.evidence_focus_seconds = 0.0;
    }

    pub fn advance_cooldown(&mut self, real_delta_secs: f64) {
        if real_delta_secs > 0.0 {
            self.cooldown_remaining_real_seconds =
                (self.cooldown_remaining_real_seconds - real_delta_secs).max(0.0);
        }
    }

    pub fn on_pause_or_system_sleep(&mut self) {
        self.reset_evidence();
    }

    pub fn record_observation(
        &mut self,
        outcome: Result<&ClassificationOutcome, &ClassifierError>,
        observation_real_secs: f64,
        is_paused_or_locked: bool,
    ) -> ObservationDecision {
        if is_paused_or_locked {
            self.reset_evidence();
            return ObservationDecision::ResetEvidence;
        }

        let expected_interval = check_interval_real_secs(self.developer_mode) as f64;
        if observation_real_secs > expected_interval * 2.5 {
            self.reset_evidence();
            return ObservationDecision::ResetEvidence;
        }

        let outcome = match outcome {
            Ok(val) => val,
            Err(_) => {
                self.reset_evidence();
                return ObservationDecision::ResetEvidence;
            }
        };

        if outcome.latency > MAX_CLASSIFICATION_LATENCY {
            self.reset_evidence();
            return ObservationDecision::ResetEvidence;
        }

        if self.cooldown_remaining_real_seconds > 0.0 {
            self.reset_evidence();
            return ObservationDecision::InCooldown;
        }

        if outcome.choice != ActivityChoice::OffTask || outcome.prob_off_task < 0.50 {
            self.reset_evidence();
            return ObservationDecision::ResetEvidence;
        }

        let focus_delta = real_to_focus_seconds(observation_real_secs, self.developer_mode);
        self.evidence_focus_seconds += focus_delta;

        let required = required_off_task_span(outcome.prob_off_task, self.developer_mode);
        self.last_required_focus_seconds = required;

        if self.evidence_focus_seconds + 1e-6 >= required {
            let triggered_evidence = self.evidence_focus_seconds;
            self.evidence_focus_seconds = 0.0;
            self.cooldown_remaining_real_seconds = cooldown_duration_real_secs(self.developer_mode);
            ObservationDecision::TriggerNudge {
                evidence_focus_seconds: triggered_evidence,
                required_focus_seconds: required,
            }
        } else {
            ObservationDecision::Accumulated {
                evidence_focus_seconds: self.evidence_focus_seconds,
                required_focus_seconds: required,
            }
        }
    }
}

pub fn required_off_task_span(prob_off_task: f64, developer_mode: bool) -> f64 {
    let clamped = prob_off_task.clamp(0.50, 0.95);
    let ratio = (clamped - 0.50) / (0.95 - 0.50);
    let (max_span, min_span) = if developer_mode {
        (80.0, 40.0)
    } else {
        (240.0, 120.0)
    };
    max_span - ratio * (max_span - min_span)
}

pub fn real_to_focus_seconds(real_secs: f64, developer_mode: bool) -> f64 {
    if developer_mode {
        real_secs * DEV_CLOCK_MULTIPLIER
    } else {
        real_secs
    }
}

pub fn check_interval_real_secs(developer_mode: bool) -> u64 {
    if developer_mode {
        DEV_CHECK_INTERVAL_SECS
    } else {
        NORMAL_CHECK_INTERVAL_SECS
    }
}

pub fn cooldown_duration_real_secs(developer_mode: bool) -> f64 {
    if developer_mode {
        DEV_COOLDOWN_REAL_SECS
    } else {
        NORMAL_COOLDOWN_REAL_SECS
    }
}

struct ActiveSessionData {
    session_id: String,
    declared_task: String,
    started_at: DateTime<Utc>,
    duration_minutes: u32,
    total_focus_seconds: f64,
    elapsed_focus_seconds: f64,
    phase: SessionPhase,
    evidence: EvidenceStateMachine,
    last_choice: String,
    last_prob_off_task: f64,
    last_confidence: f64,
    active_app: String,
    check_count: u32,
    probability_logs: Vec<ProbabilityLogEntry>,
    nudge_timestamps: Vec<DateTime<Utc>>,
    timeline: Vec<TimelineSegment>,
    last_error: Option<String>,
    real_since_last_check: f64,
    check_in_flight: bool,
}

pub struct FocusEngine {
    config: Mutex<AppConfig>,
    session: Mutex<Option<ActiveSessionData>>,
    stop_sender: Mutex<Option<watch::Sender<bool>>>,
    client: OllamaCloudClient,
}

impl FocusEngine {
    pub fn new(initial_config: AppConfig) -> Result<Arc<Self>, ClassifierError> {
        Ok(Arc::new(Self {
            config: Mutex::new(initial_config),
            session: Mutex::new(None),
            stop_sender: Mutex::new(None),
            client: OllamaCloudClient::new()?,
        }))
    }

    pub fn config(&self) -> AppConfig {
        self.config.lock().clone()
    }

    pub fn update_config(&self, next: AppConfig) {
        let dev_mode = next.developer_mode;
        *self.config.lock() = next;
        if let Some(session) = self.session.lock().as_mut() {
            session.evidence.set_developer_mode(dev_mode);
        }
    }

    pub fn set_developer_mode(&self, enabled: bool) -> AppConfig {
        let mut cfg = self.config.lock();
        cfg.developer_mode = enabled;
        let updated = cfg.clone();
        drop(cfg);
        if let Some(session) = self.session.lock().as_mut() {
            session.evidence.set_developer_mode(enabled);
        }
        updated
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let cfg = self.config.lock().clone();
        let guard = self.session.lock();
        let screen_permission_granted = has_screen_recording_permission();

        match guard.as_ref() {
            Some(s) => {
                let remaining = (s.total_focus_seconds - s.elapsed_focus_seconds).max(0.0);
                EngineSnapshot {
                    phase: s.phase,
                    session_id: Some(s.session_id.clone()),
                    declared_task: s.declared_task.clone(),
                    duration_minutes: s.duration_minutes,
                    remaining_focus_seconds: remaining,
                    elapsed_focus_seconds: s.elapsed_focus_seconds,
                    developer_mode: s.evidence.developer_mode,
                    evidence_focus_seconds: s.evidence.evidence_focus_seconds,
                    required_evidence_focus_seconds: s.evidence.last_required_focus_seconds,
                    cooldown_remaining_real_seconds: s.evidence.cooldown_remaining_real_seconds,
                    last_choice: s.last_choice.clone(),
                    last_prob_off_task: s.last_prob_off_task,
                    last_confidence: s.last_confidence,
                    active_app: s.active_app.clone(),
                    check_count: s.check_count,
                    nudge_count: s.nudge_timestamps.len() as u32,
                    screen_permission_granted,
                    last_error: s.last_error.clone(),
                    timeline: s.timeline.clone(),
                }
            }
            None => EngineSnapshot {
                phase: SessionPhase::Idle,
                session_id: None,
                declared_task: String::new(),
                duration_minutes: cfg.default_duration_minutes,
                remaining_focus_seconds: f64::from(cfg.default_duration_minutes) * 60.0,
                elapsed_focus_seconds: 0.0,
                developer_mode: cfg.developer_mode,
                evidence_focus_seconds: 0.0,
                required_evidence_focus_seconds: required_off_task_span(0.95, cfg.developer_mode),
                cooldown_remaining_real_seconds: 0.0,
                last_choice: "idle".to_string(),
                last_prob_off_task: 0.0,
                last_confidence: 0.0,
                active_app: String::new(),
                check_count: 0,
                nudge_count: 0,
                screen_permission_granted,
                last_error: None,
                timeline: Vec::new(),
            },
        }
    }

    pub fn start_session(
        self: &Arc<Self>,
        app_handle: AppHandle,
        declared_task: String,
        duration_minutes: u32,
    ) -> EngineSnapshot {
        self.stop_session(&app_handle);

        let dev_mode = self.config.lock().developer_mode;
        let total_focus_seconds = f64::from(duration_minutes) * 60.0;
        let session_id = Uuid::new_v4().to_string();

        let data = ActiveSessionData {
            session_id: session_id.clone(),
            declared_task,
            started_at: Utc::now(),
            duration_minutes,
            total_focus_seconds,
            elapsed_focus_seconds: 0.0,
            phase: SessionPhase::Running,
            evidence: EvidenceStateMachine::new(dev_mode),
            last_choice: "on_task".to_string(),
            last_prob_off_task: 0.0,
            last_confidence: 0.0,
            active_app: String::new(),
            check_count: 0,
            probability_logs: Vec::new(),
            nudge_timestamps: Vec::new(),
            timeline: Vec::new(),
            last_error: None,
            real_since_last_check: 0.0,
            check_in_flight: false,
        };

        *self.session.lock() = Some(data);

        let (tx, rx) = watch::channel(false);
        *self.stop_sender.lock() = Some(tx);

        let engine_clone = Arc::clone(self);
        let handle_clone = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            engine_clone
                .run_session_loop(handle_clone, session_id, rx)
                .await;
        });

        let snap = self.snapshot();
        let _ = app_handle.emit("engine-snapshot", &snap);
        snap
    }

    pub fn pause_session(&self, app_handle: &AppHandle) -> EngineSnapshot {
        if let Some(s) = self.session.lock().as_mut() {
            if s.phase == SessionPhase::Running {
                s.phase = SessionPhase::Paused;
                s.evidence.on_pause_or_system_sleep();
                s.real_since_last_check = 0.0;
            }
        }
        let snap = self.snapshot();
        update_tray_timer_title(app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);
        snap
    }

    pub fn resume_session(&self, app_handle: &AppHandle) -> EngineSnapshot {
        if let Some(s) = self.session.lock().as_mut() {
            if s.phase == SessionPhase::Paused {
                s.phase = SessionPhase::Running;
                s.evidence.reset_evidence();
                s.real_since_last_check = 0.0;
            }
        }
        let snap = self.snapshot();
        update_tray_timer_title(app_handle, Some(&snap));
        let _ = app_handle.emit("engine-snapshot", &snap);
        snap
    }

    pub fn stop_session(&self, app_handle: &AppHandle) -> EngineSnapshot {
        if let Some(tx) = self.stop_sender.lock().take() {
            let _ = tx.send(true);
        }
        let finished = self.session.lock().take();
        if let Some(s) = finished {
            let record = SessionHistoryRecord {
                session_id: s.session_id,
                declared_task: s.declared_task,
                started_at: s.started_at,
                ended_at: Utc::now(),
                planned_duration_minutes: s.duration_minutes,
                developer_mode: s.evidence.developer_mode,
                check_count: s.check_count,
                probability_logs: s.probability_logs,
                nudge_timestamps: s.nudge_timestamps,
            };
            let _ = append_history_record(record);
        }
        update_tray_timer_title(app_handle, None);
        let snap = self.snapshot();
        let _ = app_handle.emit("engine-snapshot", &snap);
        snap
    }

    async fn run_session_loop(
        self: Arc<Self>,
        app_handle: AppHandle,
        session_id: String,
        mut stop_rx: watch::Receiver<bool>,
    ) {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_tick = Instant::now();

        for _ in 0..MAX_SESSION_TICKS {
            tokio::select! {
                _ = stop_rx.changed() => {
                    if *stop_rx.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    if *stop_rx.borrow() {
                        break;
                    }

                    let now = Instant::now();
                    let real_delta = now.duration_since(last_tick).as_secs_f64();
                    last_tick = now;

                    let mut should_finish = false;
                    let mut trigger_check: Option<f64> = None;

                    {
                        let mut guard = self.session.lock();
                        let Some(s) = guard.as_mut() else {
                            break;
                        };
                        if s.session_id != session_id {
                            break;
                        }

                        if real_delta > 15.0 {
                            s.evidence.on_pause_or_system_sleep();
                            s.real_since_last_check = 0.0;
                        } else if s.phase == SessionPhase::Running {
                            let focus_delta = real_to_focus_seconds(real_delta, s.evidence.developer_mode);
                            s.elapsed_focus_seconds = (s.elapsed_focus_seconds + focus_delta).min(s.total_focus_seconds);
                            s.evidence.advance_cooldown(real_delta);
                            s.real_since_last_check += real_delta;

                            let interval = check_interval_real_secs(s.evidence.developer_mode) as f64;
                            if s.real_since_last_check + 1e-3 >= interval && !s.check_in_flight {
                                let accumulated_span = s.real_since_last_check;
                                s.real_since_last_check = 0.0;
                                s.check_in_flight = true;
                                trigger_check = Some(accumulated_span);
                            }

                            if s.elapsed_focus_seconds + 1e-3 >= s.total_focus_seconds {
                                should_finish = true;
                            }
                        }
                    }

                    let snap = self.snapshot();
                    update_tray_timer_title(&app_handle, Some(&snap));
                    let _ = app_handle.emit("engine-snapshot", &snap);

                    if should_finish {
                        self.stop_session(&app_handle);
                        break;
                    }

                    if let Some(observation_real_secs) = trigger_check {
                        let engine_ref = Arc::clone(&self);
                        let handle_ref = app_handle.clone();
                        let sid = session_id.clone();
                        tauri::async_runtime::spawn(async move {
                            engine_ref
                                .execute_observation_cycle(handle_ref, sid, observation_real_secs)
                                .await;
                        });
                    }
                }
            }
        }
    }

    async fn execute_observation_cycle(
        self: Arc<Self>,
        app_handle: AppHandle,
        session_id: String,
        observation_real_secs: f64,
    ) {
        let (declared_task, elapsed_minutes, cfg) = {
            let guard = self.session.lock();
            let Some(s) = guard.as_ref() else {
                return;
            };
            if s.session_id != session_id || s.phase != SessionPhase::Running {
                if let Some(s_mut) = self.session.lock().as_mut() {
                    s_mut.check_in_flight = false;
                }
                return;
            }
            let mins = (s.elapsed_focus_seconds / 60.0).floor() as u32;
            (s.declared_task.clone(), mins, self.config())
        };

        let capture_and_ocr = tokio::task::spawn_blocking(|| {
            let frame = capture_primary_display_in_ram()?;
            let ocr = extract_ocr_from_rgba(&frame.full_res_rgba)
                .unwrap_or_default();
            Ok::<_, crate::capture::CaptureError>((
                frame.base64_jpeg,
                frame.active_window,
                frame.screen_metrics,
                ocr,
            ))
        })
        .await;

        let (base64_jpeg, active_win, screen_metrics, ocr) = match capture_and_ocr {
            Ok(Ok(tuple)) => tuple,
            Ok(Err(err)) => {
                self.record_failed_cycle(&app_handle, &session_id, err.to_string());
                return;
            }
            Err(err) => {
                self.record_failed_cycle(&app_handle, &session_id, err.to_string());
                return;
            }
        };

        if active_win.is_system_locked {
            if let Some(s) = self.session.lock().as_mut() {
                if s.session_id == session_id {
                    s.evidence.on_pause_or_system_sleep();
                    s.check_in_flight = false;
                }
            }
            let snap = self.snapshot();
            let _ = app_handle.emit("engine-snapshot", &snap);
            return;
        }

        let api_key = cfg.resolved_api_key();
        let state = SystemOneState {
            declared_task: declared_task.clone(),
            active_app: active_win.app_name.clone(),
            elapsed_minutes,
            ocr_lines: ocr.formatted_lines.clone(),
        };

        let outcome_res = self
            .client
            .classify_activity(
                &cfg.ollama_base_url,
                &api_key,
                &cfg.classifier_model,
                state,
                base64_jpeg,
            )
            .await;

        let mut should_fire_nudge = false;
        {
            let mut guard = self.session.lock();
            let Some(s) = guard.as_mut() else {
                return;
            };
            if s.session_id != session_id {
                return;
            }
            s.check_in_flight = false;
            s.active_app = active_win.app_name.clone();

            let is_paused = s.phase != SessionPhase::Running;
            let decision = s.evidence.record_observation(
                outcome_res.as_ref(),
                observation_real_secs,
                is_paused,
            );

            match &outcome_res {
                Ok(outcome) => {
                    s.check_count += 1;
                    s.last_error = None;
                    s.last_choice = outcome.choice.as_str().to_string();
                    s.last_prob_off_task = outcome.prob_off_task;
                    s.last_confidence = outcome.confidence;

                    let now_utc = Utc::now();
                    s.probability_logs.push(ProbabilityLogEntry {
                        timestamp: now_utc,
                        choice: outcome.choice.as_str().to_string(),
                        prob_off_task: outcome.prob_off_task,
                        confidence: outcome.confidence,
                        evidence_focus_seconds: s.evidence.evidence_focus_seconds,
                    });
                    s.timeline.push(TimelineSegment {
                        timestamp: now_utc,
                        status: outcome.choice.as_str().to_string(),
                        prob_off_task: outcome.prob_off_task,
                        confidence: outcome.confidence,
                    });
                    if s.timeline.len() > MAX_TIMELINE_POINTS {
                        let drain_len = s.timeline.len() - MAX_TIMELINE_POINTS;
                        s.timeline.drain(0..drain_len);
                    }
                }
                Err(err) => {
                    s.last_error = Some(err.to_string());
                }
            }

            if matches!(decision, ObservationDecision::TriggerNudge { .. }) {
                s.nudge_timestamps.push(Utc::now());
                should_fire_nudge = true;
            }
        }

        let snap = self.snapshot();
        let _ = app_handle.emit("engine-snapshot", &snap);

        if should_fire_nudge {
            let directive = self
                .client
                .generate_nudge(
                    &cfg.ollama_base_url,
                    &api_key,
                    &cfg.nudge_model,
                    &declared_task,
                    &active_win.app_name,
                    &active_win.window_title,
                    &ocr.formatted_lines,
                )
                .await
                .unwrap_or_else(|_| crate::classifier::NudgeDirective {
                    nudge_text: format!(
                        "Quick detour in {}? Let's glide back to: {}",
                        active_win.app_name, declared_task
                    ),
                    target_ocr_id: ocr.bbox_by_id.keys().copied().min(),
                });

            let (target_x, target_y) = directive
                .target_ocr_id
                .and_then(|id| ocr.bbox_by_id.get(&id).copied())
                .map(|bbox| {
                    let rect = vision_bbox_to_screen_rect(
                        bbox,
                        screen_metrics.logical_width,
                        screen_metrics.logical_height,
                    );
                    (rect.center_x, rect.center_y)
                })
                .unwrap_or((
                    screen_metrics.logical_width * 0.52,
                    screen_metrics.logical_height * 0.34,
                ));

            let payload = NudgeEventPayload {
                id: Uuid::new_v4().to_string(),
                nudge_text: directive.nudge_text,
                target_x,
                target_y,
                target_ocr_id: directive.target_ocr_id,
                active_app: active_win.app_name,
                dismiss_after_ms: 5200,
            };
            let _ = emit_nudge_event(&app_handle, payload);
        }
    }

    fn record_failed_cycle(&self, app_handle: &AppHandle, session_id: &str, message: String) {
        if let Some(s) = self.session.lock().as_mut() {
            if s.session_id == session_id {
                s.evidence.reset_evidence();
                s.check_in_flight = false;
                s.last_error = Some(message);
            }
        }
        let snap = self.snapshot();
        let _ = app_handle.emit("engine-snapshot", &snap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_outcome(
        choice: ActivityChoice,
        prob_off_task: f64,
        latency_ms: u64,
    ) -> ClassificationOutcome {
        ClassificationOutcome {
            choice,
            prob_off_task,
            confidence: 0.92,
            latency: Duration::from_millis(latency_ms),
        }
    }

    #[test]
    fn scales_required_off_task_span_linearly_between_050_and_095() {
        assert!((required_off_task_span(0.50, false) - 240.0).abs() < 1e-6);
        assert!((required_off_task_span(0.725, false) - 180.0).abs() < 1e-6);
        assert!((required_off_task_span(0.95, false) - 120.0).abs() < 1e-6);
        assert!((required_off_task_span(0.99, false) - 120.0).abs() < 1e-6);

        assert!((required_off_task_span(0.50, true) - 80.0).abs() < 1e-6);
        assert!((required_off_task_span(0.725, true) - 60.0).abs() < 1e-6);
        assert!((required_off_task_span(0.95, true) - 40.0).abs() < 1e-6);
    }

    #[test]
    fn normal_mode_triggers_at_120s_for_high_prob_and_240s_for_050() {
        let mut sm = EvidenceStateMachine::new(false);
        let high = sample_outcome(ActivityChoice::OffTask, 0.96, 350);

        for step in 1..=3 {
            let dec = sm.record_observation(Ok(&high), 30.0, false);
            assert_eq!(
                dec,
                ObservationDecision::Accumulated {
                    evidence_focus_seconds: f64::from(step) * 30.0,
                    required_focus_seconds: 120.0,
                }
            );
        }
        let fourth = sm.record_observation(Ok(&high), 30.0, false);
        assert_eq!(
            fourth,
            ObservationDecision::TriggerNudge {
                evidence_focus_seconds: 120.0,
                required_focus_seconds: 120.0,
            }
        );

        let mut sm_low = EvidenceStateMachine::new(false);
        let low = sample_outcome(ActivityChoice::OffTask, 0.50, 350);
        for _ in 1..=7 {
            let dec = sm_low.record_observation(Ok(&low), 30.0, false);
            assert!(matches!(dec, ObservationDecision::Accumulated { .. }));
        }
        let eighth = sm_low.record_observation(Ok(&low), 30.0, false);
        assert_eq!(
            eighth,
            ObservationDecision::TriggerNudge {
                evidence_focus_seconds: 240.0,
                required_focus_seconds: 240.0,
            }
        );
    }

    #[test]
    fn developer_mode_uses_4x_focus_clock_and_30s_cooldown() {
        let mut sm = EvidenceStateMachine::new(true);
        let border = sample_outcome(ActivityChoice::OffTask, 0.50, 250);

        let first = sm.record_observation(Ok(&border), 10.0, false);
        assert_eq!(
            first,
            ObservationDecision::Accumulated {
                evidence_focus_seconds: 40.0,
                required_focus_seconds: 80.0,
            }
        );

        let second = sm.record_observation(Ok(&border), 10.0, false);
        assert_eq!(
            second,
            ObservationDecision::TriggerNudge {
                evidence_focus_seconds: 80.0,
                required_focus_seconds: 80.0,
            }
        );
        assert!((sm.cooldown_remaining_real_seconds - 30.0).abs() < 1e-6);

        sm.advance_cooldown(10.0);
        let during_cooldown = sm.record_observation(Ok(&border), 10.0, false);
        assert_eq!(during_cooldown, ObservationDecision::InCooldown);

        sm.advance_cooldown(20.0);
        let after_cooldown = sm.record_observation(Ok(&border), 10.0, false);
        assert!(matches!(
            after_cooldown,
            ObservationDecision::Accumulated { .. }
        ));
    }

    #[test]
    fn resets_immediately_on_on_task_uncertain_low_prob_pause_or_stale_timeout() {
        let mut sm = EvidenceStateMachine::new(false);
        let off = sample_outcome(ActivityChoice::OffTask, 0.90, 300);
        sm.record_observation(Ok(&off), 30.0, false);
        assert!(sm.evidence_focus_seconds > 0.0);

        let on_task = sample_outcome(ActivityChoice::OnTask, 0.10, 300);
        assert_eq!(
            sm.record_observation(Ok(&on_task), 30.0, false),
            ObservationDecision::ResetEvidence
        );
        assert_eq!(sm.evidence_focus_seconds, 0.0);

        sm.record_observation(Ok(&off), 30.0, false);
        let uncertain = sample_outcome(ActivityChoice::Uncertain, 0.65, 300);
        assert_eq!(
            sm.record_observation(Ok(&uncertain), 30.0, false),
            ObservationDecision::ResetEvidence
        );
        assert_eq!(sm.evidence_focus_seconds, 0.0);

        sm.record_observation(Ok(&off), 30.0, false);
        let low_prob_off = sample_outcome(ActivityChoice::OffTask, 0.49, 300);
        assert_eq!(
            sm.record_observation(Ok(&low_prob_off), 30.0, false),
            ObservationDecision::ResetEvidence
        );

        sm.record_observation(Ok(&off), 30.0, false);
        assert_eq!(
            sm.record_observation(Ok(&off), 30.0, true),
            ObservationDecision::ResetEvidence
        );

        sm.record_observation(Ok(&off), 30.0, false);
        let stale = sample_outcome(ActivityChoice::OffTask, 0.95, 21_000);
        assert_eq!(
            sm.record_observation(Ok(&stale), 30.0, false),
            ObservationDecision::ResetEvidence
        );
        assert_eq!(sm.evidence_focus_seconds, 0.0);
    }
}
