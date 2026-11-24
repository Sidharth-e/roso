pub mod automation;
pub mod capture;
pub mod classifier;
pub mod config;
pub mod engine;
pub mod ocr;
pub mod overlay;

use crate::automation::request_accessibility_permission as request_ax_permission;
use crate::capture::request_screen_recording_permission;
use crate::config::{
    load_config, load_history, save_config, validate_goal_input, AppConfig, TaskHistoryRecord,
};
use crate::engine::{EngineSnapshot, TaskEngine};
use crate::overlay::{
    hide_tray_popover, initialize_windows_and_tray, show_tray_popover, trigger_mock_pointer_cue,
    PointerCuePayload,
};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
async fn get_engine_snapshot(engine: State<'_, Arc<TaskEngine>>) -> Result<EngineSnapshot, String> {
    Ok(engine.snapshot())
}

#[tauri::command]
async fn get_app_config(engine: State<'_, Arc<TaskEngine>>) -> Result<AppConfig, String> {
    Ok(engine.config())
}

#[tauri::command]
async fn update_app_config(
    engine: State<'_, Arc<TaskEngine>>,
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
async fn start_task(
    app_handle: AppHandle,
    engine: State<'_, Arc<TaskEngine>>,
    goal: String,
) -> Result<EngineSnapshot, String> {
    let clean_goal = validate_goal_input(&goal).map_err(|e| e.to_string())?;
    Ok(engine.start_task(app_handle, clean_goal))
}

#[tauri::command]
async fn pause_task(
    app_handle: AppHandle,
    engine: State<'_, Arc<TaskEngine>>,
) -> Result<EngineSnapshot, String> {
    Ok(engine.pause_task(&app_handle))
}

#[tauri::command]
async fn resume_task(
    app_handle: AppHandle,
    engine: State<'_, Arc<TaskEngine>>,
) -> Result<EngineSnapshot, String> {
    Ok(engine.resume_task(&app_handle))
}

#[tauri::command]
async fn stop_task(
    app_handle: AppHandle,
    engine: State<'_, Arc<TaskEngine>>,
) -> Result<EngineSnapshot, String> {
    Ok(engine.stop_task(&app_handle, Some("Task stopped by user".to_string())))
}

#[tauri::command]
async fn resolve_confirmation(
    app_handle: AppHandle,
    engine: State<'_, Arc<TaskEngine>>,
    confirmation_id: String,
    approved: bool,
) -> Result<EngineSnapshot, String> {
    engine.resolve_confirmation(&app_handle, &confirmation_id, approved)
}

#[tauri::command]
async fn get_task_history() -> Result<Vec<TaskHistoryRecord>, String> {
    tokio::task::spawn_blocking(load_history)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn request_screen_permission() -> Result<bool, String> {
    tokio::task::spawn_blocking(request_screen_recording_permission)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn request_accessibility_permission() -> Result<bool, String> {
    tokio::task::spawn_blocking(request_ax_permission)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn fire_test_pointer(
    app_handle: AppHandle,
    engine: State<'_, Arc<TaskEngine>>,
) -> Result<PointerCuePayload, String> {
    let current_goal = engine.snapshot().goal;
    trigger_mock_pointer_cue(&app_handle, &current_goal).map_err(|e| e.to_string())
}

#[tauri::command]
async fn hide_tray_window(app_handle: AppHandle) -> Result<(), String> {
    hide_tray_popover(&app_handle);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial_config = load_config().unwrap_or_default();
    let engine = TaskEngine::new(initial_config).expect("failed to initialize task engine");

    let engine_for_setup = Arc::clone(&engine);

    tauri::Builder::default()
        .manage(engine)
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let engine_for_pointer = Arc::clone(&engine_for_setup);
            let engine_for_stop = Arc::clone(&engine_for_setup);
            initialize_windows_and_tray(
                app.handle(),
                move |handle| {
                    let goal = engine_for_pointer.snapshot().goal;
                    let _ = trigger_mock_pointer_cue(handle, &goal);
                },
                move |handle| {
                    let _ = engine_for_stop
                        .stop_task(handle, Some("Stopped from menu bar".to_string()));
                    show_tray_popover(handle, false);
                },
            )?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_engine_snapshot,
            get_app_config,
            update_app_config,
            start_task,
            pause_task,
            resume_task,
            stop_task,
            resolve_confirmation,
            get_task_history,
            request_screen_permission,
            request_accessibility_permission,
            fire_test_pointer,
            hide_tray_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Task Companion");
}
