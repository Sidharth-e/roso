pub mod capture;
pub mod classifier;
pub mod config;
pub mod engine;
pub mod ocr;
pub mod overlay;

use crate::capture::{has_screen_recording_permission, request_screen_recording_permission};
use crate::config::{
    load_config, load_history, save_config, validate_duration_minutes, validate_task_input,
    AppConfig, SessionHistoryRecord,
};
use crate::engine::{EngineSnapshot, FocusEngine};
use crate::overlay::{
    hide_tray_popover, initialize_windows_and_tray, trigger_mock_nudge, NudgeEventPayload,
};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
async fn get_engine_snapshot(engine: State<'_, Arc<FocusEngine>>) -> Result<EngineSnapshot, String> {
    Ok(engine.snapshot())
}

#[tauri::command]
async fn get_app_config(engine: State<'_, Arc<FocusEngine>>) -> Result<AppConfig, String> {
    Ok(engine.config())
}

#[tauri::command]
async fn update_app_config(
    engine: State<'_, Arc<FocusEngine>>,
    config: AppConfig,
) -> Result<AppConfig, String> {
    let saved = tokio::task::spawn_blocking(move || save_config(&config))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    engine.update_config(saved.clone());
    Ok(saved)
}

#[tauri::command]
async fn toggle_developer_mode(
    app_handle: AppHandle,
    engine: State<'_, Arc<FocusEngine>>,
    enabled: bool,
) -> Result<EngineSnapshot, String> {
    let updated_cfg = engine.set_developer_mode(enabled);
    tokio::task::spawn_blocking(move || save_config(&updated_cfg))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let snap = engine.snapshot();
    overlay::update_tray_timer_title(&app_handle, Some(&snap));
    Ok(snap)
}

#[tauri::command]
async fn start_focus_session(
    app_handle: AppHandle,
    engine: State<'_, Arc<FocusEngine>>,
    declared_task: String,
    duration_minutes: u32,
) -> Result<EngineSnapshot, String> {
    let task = validate_task_input(&declared_task).map_err(|e| e.to_string())?;
    let minutes = validate_duration_minutes(duration_minutes).map_err(|e| e.to_string())?;
    Ok(engine.start_session(app_handle, task, minutes))
}

#[tauri::command]
async fn pause_focus_session(
    app_handle: AppHandle,
    engine: State<'_, Arc<FocusEngine>>,
) -> Result<EngineSnapshot, String> {
    Ok(engine.pause_session(&app_handle))
}

#[tauri::command]
async fn resume_focus_session(
    app_handle: AppHandle,
    engine: State<'_, Arc<FocusEngine>>,
) -> Result<EngineSnapshot, String> {
    Ok(engine.resume_session(&app_handle))
}

#[tauri::command]
async fn stop_focus_session(
    app_handle: AppHandle,
    engine: State<'_, Arc<FocusEngine>>,
) -> Result<EngineSnapshot, String> {
    Ok(engine.stop_session(&app_handle))
}

#[tauri::command]
async fn get_focus_history() -> Result<Vec<SessionHistoryRecord>, String> {
    tokio::task::spawn_blocking(load_history)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn check_screen_permission() -> Result<bool, String> {
    tokio::task::spawn_blocking(has_screen_recording_permission)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn request_screen_permission() -> Result<bool, String> {
    tokio::task::spawn_blocking(request_screen_recording_permission)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn fire_test_nudge(
    app_handle: AppHandle,
    engine: State<'_, Arc<FocusEngine>>,
) -> Result<NudgeEventPayload, String> {
    let current_task = engine.snapshot().declared_task;
    trigger_mock_nudge(&app_handle, &current_task).map_err(|e| e.to_string())
}

#[tauri::command]
async fn hide_tray_window(app_handle: AppHandle) -> Result<(), String> {
    hide_tray_popover(&app_handle);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial_config = load_config().unwrap_or_default();
    let engine = FocusEngine::new(initial_config).expect("failed to initialize focus engine");

    let engine_for_setup = Arc::clone(&engine);

    tauri::Builder::default()
        .manage(engine)
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let engine_for_nudge = Arc::clone(&engine_for_setup);
            initialize_windows_and_tray(app.handle(), move |handle| {
                let task = engine_for_nudge.snapshot().declared_task;
                let _ = trigger_mock_nudge(handle, &task);
            })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_engine_snapshot,
            get_app_config,
            update_app_config,
            toggle_developer_mode,
            start_focus_session,
            pause_focus_session,
            resume_focus_session,
            stop_focus_session,
            get_focus_history,
            check_screen_permission,
            request_screen_permission,
            fire_test_nudge,
            hide_tray_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Focus Companion");
}
