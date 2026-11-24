use crate::capture::primary_screen_metrics;
use crate::engine::{EngineSnapshot, TaskPhase};
use serde::{Deserialize, Serialize};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
#[cfg(not(target_os = "macos"))]
use tauri::{LogicalPosition, LogicalSize, Position, Size};
use uuid::Uuid;

#[cfg(target_os = "macos")]
use std::cell::Cell;

pub const TRAY_ID: &str = "task-companion-tray";
pub const TRAY_WINDOW_LABEL: &str = "main";
pub const OVERLAY_WINDOW_LABEL: &str = "overlay";
const POPOVER_WIDTH: f64 = 400.0;
const POPOVER_HEIGHT: f64 = 620.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerCuePayload {
    pub id: String,
    pub message: String,
    pub action_badge: String,
    pub target_x: f64,
    pub target_y: f64,
    pub target_ocr_id: Option<u32>,
    pub step_number: u32,
    pub is_acting: bool,
    pub dismiss_after_ms: u64,
}

#[cfg(target_os = "macos")]
mod macos_panel {
    use super::*;
    use objc2::runtime::{AnyClass, AnyObject, NSObject};
    use objc2::{define_class, msg_send, ClassType, DefinedClass, MainThreadOnly};
    use objc2_app_kit::{
        NSColor, NSEvent, NSPanel, NSPopUpMenuWindowLevel, NSResponder, NSScreen, NSWindow,
        NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect};

    #[derive(Debug)]
    pub struct PanelCompatIvars {
        pub focusable: Cell<bool>,
        pub maximizable: Cell<bool>,
    }

    define_class!(
        #[unsafe(super(NSPanel, NSWindow, NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FocusCompanionPanel"]
        #[ivars = PanelCompatIvars]
        #[derive(Debug)]
        pub struct FocusCompanionPanel;

        impl FocusCompanionPanel {
            #[unsafe(method(setStyleMask:))]
            fn set_style_mask(&self, mask: NSWindowStyleMask) {
                unsafe {
                    let patched = mask | NSWindowStyleMask::NonactivatingPanel;
                    let _: () = msg_send![super(self), setStyleMask: patched];
                }
            }

            #[unsafe(method(canBecomeMainWindow))]
            fn can_become_main_window(&self) -> bool {
                false
            }

            #[unsafe(method(canBecomeKeyWindow))]
            fn can_become_key_window(&self) -> bool {
                self.ivars().focusable.get()
            }

            #[unsafe(method(sendEvent:))]
            fn send_event(&self, event: &NSEvent) {
                unsafe {
                    let _: () = msg_send![super(self), sendEvent: event];
                }
            }
        }
    );

    extern "C-unwind" {
        fn object_setClass(obj: *mut AnyObject, cls: *const AnyClass) -> *const AnyClass;
    }

    pub fn promote_to_panel(
        window: &WebviewWindow,
        can_become_key: bool,
        level_offset: isize,
        stationary: bool,
    ) {
        let Ok(raw_ptr) = window.ns_window() else {
            return;
        };
        if raw_ptr.is_null() {
            return;
        }

        unsafe {
            let obj_ptr = raw_ptr.cast::<AnyObject>();
            let current_cls = (*obj_ptr).class();
            let target_cls = FocusCompanionPanel::class();

            if !std::ptr::eq(current_cls, target_cls) {
                if target_cls.instance_size() > current_cls.instance_size() {
                    return;
                }
                object_setClass(obj_ptr, target_cls);
            }

            let panel_ref: &FocusCompanionPanel = &*raw_ptr.cast::<FocusCompanionPanel>();
            panel_ref.ivars().focusable.set(can_become_key);
            panel_ref.ivars().maximizable.set(false);

            let ns_panel: &NSPanel = panel_ref;
            ns_panel.setStyleMask(
                NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            );
            ns_panel.setFloatingPanel(true);
            ns_panel.setHidesOnDeactivate(false);
            ns_panel.setWorksWhenModal(true);
            ns_panel.setBecomesKeyOnlyIfNeeded(!can_become_key);
            ns_panel.setLevel(NSPopUpMenuWindowLevel + level_offset);

            let space_mode = if stationary {
                NSWindowCollectionBehavior::Stationary
            } else {
                NSWindowCollectionBehavior::Transient
            };
            let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle
                | space_mode;
            ns_panel.setCollectionBehavior(behavior);
            ns_panel.setOpaque(false);
            ns_panel.setBackgroundColor(Some(&NSColor::clearColor()));
            ns_panel.setHasShadow(false);

            if !can_become_key {
                ns_panel.setIgnoresMouseEvents(true);
            }
        }
    }

    pub fn active_screen_frame(mtm: MainThreadMarker) -> (NSRect, NSPoint) {
        let mouse = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let count = screens.count();
        for idx in 0..count {
            let screen = screens.objectAtIndex(idx);
            let frame = screen.frame();
            let in_x = mouse.x >= frame.origin.x && mouse.x <= frame.origin.x + frame.size.width;
            let in_y = mouse.y >= frame.origin.y && mouse.y <= frame.origin.y + frame.size.height;
            if in_x && in_y {
                return (frame, mouse);
            }
        }
        if let Some(main) = NSScreen::mainScreen(mtm) {
            return (main.frame(), mouse);
        }
        if let Some(first) = screens.firstObject() {
            return (first.frame(), mouse);
        }
        (
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                objc2_foundation::NSSize::new(1440.0, 900.0),
            ),
            mouse,
        )
    }

    pub fn show_popover_on_active_space(window: &WebviewWindow, align_to_cursor: bool) {
        promote_to_panel(window, true, 0, false);
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Ok(raw_ptr) = window.ns_window() else {
            return;
        };
        if raw_ptr.is_null() {
            return;
        }

        let (screen_frame, mouse) = active_screen_frame(mtm);
        let min_x = screen_frame.origin.x + 12.0;
        let max_x = (screen_frame.origin.x + screen_frame.size.width - POPOVER_WIDTH - 12.0)
            .max(min_x);
        let desired_x = if align_to_cursor {
            (mouse.x - POPOVER_WIDTH * 0.5).clamp(min_x, max_x)
        } else {
            (screen_frame.origin.x + screen_frame.size.width - POPOVER_WIDTH - 18.0)
                .clamp(min_x, max_x)
        };
        let top_y = screen_frame.origin.y + screen_frame.size.height - 34.0;

        unsafe {
            let ns_panel: &NSPanel = &*raw_ptr.cast::<NSPanel>();
            ns_panel.setFrameTopLeftPoint(NSPoint::new(desired_x, top_y));
            ns_panel.orderFrontRegardless();
            ns_panel.makeKeyWindow();
        }
    }

    pub fn hide_popover_panel(window: &WebviewWindow) {
        let Ok(raw_ptr) = window.ns_window() else {
            return;
        };
        if raw_ptr.is_null() {
            return;
        }
        unsafe {
            let ns_panel: &NSPanel = &*raw_ptr.cast::<NSPanel>();
            ns_panel.orderOut(None);
        }
    }

    pub fn is_popover_visible_on_active_space(window: &WebviewWindow) -> bool {
        let Ok(raw_ptr) = window.ns_window() else {
            return false;
        };
        if raw_ptr.is_null() {
            return false;
        }
        unsafe {
            let ns_panel: &NSPanel = &*raw_ptr.cast::<NSPanel>();
            ns_panel.isVisible() && ns_panel.isOnActiveSpace()
        }
    }

    pub fn sync_overlay_to_active_screen(window: &WebviewWindow) -> (f64, f64) {
        promote_to_panel(window, false, 2, true);
        let Some(mtm) = MainThreadMarker::new() else {
            return (1440.0, 900.0);
        };
        let (screen_frame, _) = active_screen_frame(mtm);
        if let Ok(raw_ptr) = window.ns_window() {
            if !raw_ptr.is_null() {
                unsafe {
                    let ns_panel: &NSPanel = &*raw_ptr.cast::<NSPanel>();
                    ns_panel.setIgnoresMouseEvents(true);
                    ns_panel.setFrame_display(screen_frame, true);
                    ns_panel.orderFrontRegardless();
                }
            }
        }
        (screen_frame.size.width, screen_frame.size.height)
    }
}

pub fn initialize_windows_and_tray<F, S>(
    app_handle: &AppHandle,
    on_test_pointer: F,
    on_stop_task: S,
) -> tauri::Result<()>
where
    F: Fn(&AppHandle) + Send + Sync + 'static,
    S: Fn(&AppHandle) + Send + Sync + 'static,
{
    ensure_windows_created(app_handle)?;
    configure_overlay_window(app_handle);

    let toggle_item =
        MenuItemBuilder::with_id("toggle_popover", "Toggle Task Companion").build(app_handle)?;
    let stop_item =
        MenuItemBuilder::with_id("stop_task", "Stop Active Task (⌃⌥S)").build(app_handle)?;
    let test_pointer_item =
        MenuItemBuilder::with_id("test_pointer", "Test Pointer Overlay").build(app_handle)?;
    let quit_item =
        MenuItemBuilder::with_id("quit_app", "Quit Task Companion").build(app_handle)?;

    let tray_menu = MenuBuilder::new(app_handle)
        .items(&[&toggle_item, &stop_item, &test_pointer_item, &quit_item])
        .build()?;

    let mut tray_builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&tray_menu)
        .tooltip("Task Companion — Click to open task orchestrator")
        .show_menu_on_left_click(false);

    if let Some(default_icon) = app_handle.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(default_icon).icon_as_template(true);
    } else {
        tray_builder = tray_builder.title("Task");
    }

    tray_builder
        .on_menu_event(move |handle, event| match event.id().as_ref() {
            "toggle_popover" => {
                toggle_tray_popover(handle, false);
            }
            "stop_task" => {
                on_stop_task(handle);
            }
            "test_pointer" => {
                on_test_pointer(handle);
            }
            "quit_app" => {
                handle.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Down,
                ..
            } = event
            {
                toggle_tray_popover(tray.app_handle(), true);
            }
        })
        .build(app_handle)?;

    show_tray_popover(app_handle, false);

    Ok(())
}

fn ensure_windows_created(app_handle: &AppHandle) -> tauri::Result<()> {
    if app_handle.get_webview_window(OVERLAY_WINDOW_LABEL).is_none() {
        WebviewWindowBuilder::new(
            app_handle,
            OVERLAY_WINDOW_LABEL,
            WebviewUrl::App("index.html?view=overlay".into()),
        )
        .title("Task Companion Overlay")
        .inner_size(1440.0, 900.0)
        .position(0.0, 0.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        .focusable(false)
        .skip_taskbar(true)
        .shadow(false)
        .visible(false)
        .build()?;
    }

    if app_handle.get_webview_window(TRAY_WINDOW_LABEL).is_none() {
        let main_win = WebviewWindowBuilder::new(
            app_handle,
            TRAY_WINDOW_LABEL,
            WebviewUrl::App("index.html?view=tray".into()),
        )
        .title("Task Companion")
        .inner_size(POPOVER_WIDTH, POPOVER_HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        .focusable(true)
        .skip_taskbar(true)
        .shadow(false)
        .visible(false)
        .build()?;

        #[cfg(target_os = "macos")]
        macos_panel::promote_to_panel(&main_win, true, 0, false);
    }

    Ok(())
}

pub fn configure_overlay_window(app_handle: &AppHandle) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(overlay) = handle.get_webview_window(OVERLAY_WINDOW_LABEL) else {
            return;
        };
        let _ = overlay.set_ignore_cursor_events(true);

        #[cfg(target_os = "macos")]
        {
            let _ = macos_panel::sync_overlay_to_active_screen(&overlay);
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = overlay.set_always_on_top(true);
            let _ = overlay.set_visible_on_all_workspaces(true);
            if let Ok(metrics) = primary_screen_metrics() {
                let _ = overlay.set_position(Position::Logical(LogicalPosition::new(
                    metrics.origin_x,
                    metrics.origin_y,
                )));
                let _ = overlay.set_size(Size::Logical(LogicalSize::new(
                    metrics.logical_width,
                    metrics.logical_height,
                )));
            }
            let _ = overlay.show();
        }
    });
}

pub fn show_tray_popover(app_handle: &AppHandle, align_to_cursor: bool) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(win) = handle.get_webview_window(TRAY_WINDOW_LABEL) else {
            return;
        };
        #[cfg(target_os = "macos")]
        {
            macos_panel::show_popover_on_active_space(&win, align_to_cursor);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = align_to_cursor;
            let _ = win.show();
            let _ = win.set_focus();
        }
    });
}

pub fn hide_tray_popover(app_handle: &AppHandle) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(win) = handle.get_webview_window(TRAY_WINDOW_LABEL) else {
            return;
        };
        #[cfg(target_os = "macos")]
        {
            macos_panel::hide_popover_panel(&win);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = win.hide();
        }
    });
}

pub fn toggle_tray_popover(app_handle: &AppHandle, align_to_cursor: bool) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(win) = handle.get_webview_window(TRAY_WINDOW_LABEL) else {
            return;
        };

        #[cfg(target_os = "macos")]
        {
            if macos_panel::is_popover_visible_on_active_space(&win) {
                macos_panel::hide_popover_panel(&win);
            } else {
                macos_panel::show_popover_on_active_space(&win, align_to_cursor);
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = align_to_cursor;
            if win.is_visible().unwrap_or(false) {
                let _ = win.hide();
            } else {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
    });
}

pub fn emit_pointer_cue(
    app_handle: &AppHandle,
    payload: PointerCuePayload,
) -> tauri::Result<()> {
    configure_overlay_window(app_handle);
    app_handle.emit("pointer-cue", payload)
}

pub fn trigger_mock_pointer_cue(
    app_handle: &AppHandle,
    goal: &str,
) -> tauri::Result<PointerCuePayload> {
    let (screen_w, screen_h) = app_handle
        .get_webview_window(OVERLAY_WINDOW_LABEL)
        .and_then(|w| w.current_monitor().ok().flatten())
        .map(|m| {
            let scale = m.scale_factor();
            (
                f64::from(m.size().width) / scale,
                f64::from(m.size().height) / scale,
            )
        })
        .or_else(|| {
            primary_screen_metrics()
                .ok()
                .map(|m| (m.logical_width, m.logical_height))
        })
        .unwrap_or((1440.0, 900.0));

    let clean_goal = if goal.trim().is_empty() {
        "your next macOS task"
    } else {
        goal.trim()
    };

    let payload = PointerCuePayload {
        id: Uuid::new_v4().to_string(),
        message: format!("Ready to glide here and tackle: {clean_goal}"),
        action_badge: "Preview".to_string(),
        target_x: (screen_w * 0.56).clamp(120.0, screen_w - 160.0),
        target_y: (screen_h * 0.36).clamp(100.0, screen_h - 140.0),
        target_ocr_id: Some(1),
        step_number: 1,
        is_acting: true,
        dismiss_after_ms: 4200,
    };

    emit_pointer_cue(app_handle, payload.clone())?;
    Ok(payload)
}

pub fn update_tray_status_title(app_handle: &AppHandle, snapshot: Option<&EngineSnapshot>) {
    let Some(tray) = app_handle.tray_by_id(TRAY_ID) else {
        return;
    };
    let title = match snapshot {
        Some(s) => match s.phase {
            TaskPhase::Planning => Some("Planning...".to_string()),
            TaskPhase::Running => Some(format!("Step {}/{}", s.current_step, s.max_steps)),
            TaskPhase::AwaitingConfirmation => Some("Approve?".to_string()),
            TaskPhase::Paused => Some("Paused".to_string()),
            _ => Some(String::new()),
        },
        None => Some(String::new()),
    };
    let _ = tray.set_title(title.as_deref());
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::macos_panel::FocusCompanionPanel;
    use objc2::ClassType;
    use objc2_app_kit::{NSPanel, NSWindow};

    #[test]
    fn focus_panel_class_preserves_tao_window_ivar_footprint() {
        let window_size = NSWindow::class().instance_size();
        let panel_size = NSPanel::class().instance_size();
        let custom_panel_size = FocusCompanionPanel::class().instance_size();
        assert_eq!(window_size, panel_size);
        assert!(custom_panel_size >= panel_size + 2);
    }
}
