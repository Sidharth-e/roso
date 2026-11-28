use crate::capture::{primary_screen_metrics, ScreenMetrics};
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

pub const TRAY_ID: &str = "task-companion-tray";
pub const TRAY_WINDOW_LABEL: &str = "main";
pub const OVERLAY_WINDOW_LABEL: &str = "overlay";
pub const PANEL_WINDOW_LABEL: &str = "panel";
const POPOVER_WIDTH: f64 = 560.0;
const POPOVER_HEIGHT: f64 = 700.0;
const PANEL_WIDTH: f64 = 380.0;
const PANEL_HEIGHT: f64 = 620.0;
const PANEL_COLLAPSED_WIDTH: f64 = 280.0;
const PANEL_COLLAPSED_HEIGHT: f64 = 56.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerCuePayload {
    pub id: String,
    pub message: String,
    pub action_badge: String,
    pub target_x: f64,
    pub target_y: f64,
    pub target_width: Option<f64>,
    pub target_height: Option<f64>,
    pub target_ocr_id: Option<u32>,
    pub step_number: u32,
    pub is_acting: bool,
    pub show_pointer: bool,
    pub dismiss_after_ms: u64,
}

#[cfg(target_os = "macos")]
mod macos_panel {
    use super::*;
    use objc2::runtime::{AnyClass, AnyObject, NSObject};
    use objc2::{define_class, msg_send, ClassType, MainThreadOnly};
    use objc2_app_kit::{
        NSColor, NSEvent, NSPanel, NSPopUpMenuWindowLevel, NSResponder, NSScreen, NSWindow,
        NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
    use parking_lot::Mutex;
    use std::collections::HashSet;
    use std::sync::OnceLock;

    static FOCUSABLE_PANELS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

    fn focusable_set() -> &'static Mutex<HashSet<usize>> {
        FOCUSABLE_PANELS.get_or_init(|| Mutex::new(HashSet::new()))
    }

    define_class!(
        #[unsafe(super(NSPanel, NSWindow, NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FocusCompanionPanel"]
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
                let key = (self as *const FocusCompanionPanel) as usize;
                focusable_set().lock().contains(&key)
            }

            #[unsafe(method(constrainFrameRect:toScreen:))]
            fn constrain_frame_rect(
                &self,
                frame_rect: NSRect,
                _screen: Option<&NSScreen>,
            ) -> NSRect {
                frame_rect
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
        ignores_mouse_events: bool,
        exclude_from_capture: bool,
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

            {
                let mut set = focusable_set().lock();
                let key = raw_ptr as usize;
                if can_become_key {
                    set.insert(key);
                } else {
                    set.remove(&key);
                }
            }

            let ns_panel: &NSPanel = &*raw_ptr.cast::<NSPanel>();
            ns_panel.setStyleMask(
                NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            );
            ns_panel.setFloatingPanel(true);
            ns_panel.setHidesOnDeactivate(false);
            ns_panel.setWorksWhenModal(true);
            ns_panel.setBecomesKeyOnlyIfNeeded(true);
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
            ns_panel.setIgnoresMouseEvents(ignores_mouse_events);

            if exclude_from_capture {
                let _: () = msg_send![ns_panel, setSharingType: 0usize];
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
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0)),
            mouse,
        )
    }

    pub fn show_popover_on_active_space(window: &WebviewWindow, align_to_cursor: bool) {
        promote_to_panel(window, true, 0, false, false, false);
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Ok(raw_ptr) = window.ns_window() else {
            return;
        };
        if raw_ptr.is_null() {
            return;
        }

        // The main window is a centered modal on whichever screen holds the cursor.
        let _ = align_to_cursor;
        let (screen_frame, _) = active_screen_frame(mtm);
        let width = POPOVER_WIDTH.min(screen_frame.size.width - 24.0);
        let height = POPOVER_HEIGHT.min(screen_frame.size.height - 48.0);
        let x = screen_frame.origin.x + (screen_frame.size.width - width) * 0.5;
        let y = screen_frame.origin.y + (screen_frame.size.height - height) * 0.5;
        let rect = NSRect::new(NSPoint::new(x, y), NSSize::new(width, height));

        unsafe {
            let ns_panel: &NSPanel = &*raw_ptr.cast::<NSPanel>();
            ns_panel.setFrame_display(rect, true);
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

    pub fn primary_screen_frame(mtm: MainThreadMarker) -> NSRect {
        let screens = NSScreen::screens(mtm);
        if let Some(first) = screens.firstObject() {
            return first.frame();
        }
        if let Some(main) = NSScreen::mainScreen(mtm) {
            return main.frame();
        }
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0))
    }

    pub fn sync_overlay_to_primary_screen(window: &WebviewWindow) -> (f64, f64) {
        promote_to_panel(window, false, 2, true, true, true);
        let Some(mtm) = MainThreadMarker::new() else {
            return (1440.0, 900.0);
        };
        let screen_frame = primary_screen_frame(mtm);
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

    pub fn sync_overlay_to_screen_metrics(window: &WebviewWindow, metrics: ScreenMetrics) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let primary = primary_screen_frame(mtm);
        let cocoa_y = primary.size.height - (metrics.origin_y + metrics.logical_height);
        let rect = NSRect::new(
            NSPoint::new(metrics.origin_x, cocoa_y),
            NSSize::new(metrics.logical_width, metrics.logical_height),
        );
        if let Ok(raw_ptr) = window.ns_window() {
            if !raw_ptr.is_null() {
                unsafe {
                    let ns_panel: &NSPanel = &*raw_ptr.cast::<NSPanel>();
                    ns_panel.setIgnoresMouseEvents(true);
                    ns_panel.setFrame_display(rect, true);
                }
            }
        }
    }

    pub fn order_overlay_front(window: &WebviewWindow) {
        if let Ok(raw_ptr) = window.ns_window() {
            if !raw_ptr.is_null() {
                unsafe {
                    let ns_panel: &NSPanel = &*raw_ptr.cast::<NSPanel>();
                    ns_panel.setIgnoresMouseEvents(true);
                    ns_panel.orderFrontRegardless();
                }
            }
        }
    }

    pub fn position_and_show_hud_panel(window: &WebviewWindow, width: f64, height: f64) {
        promote_to_panel(window, true, 1, true, false, true);
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Ok(raw_ptr) = window.ns_window() else {
            return;
        };
        if raw_ptr.is_null() {
            return;
        }
        let (screen_frame, _) = active_screen_frame(mtm);
        let x = (screen_frame.origin.x + screen_frame.size.width - width - 14.0)
            .max(screen_frame.origin.x + 12.0);
        let top_y = screen_frame.origin.y + screen_frame.size.height - 42.0;
        let origin_y = (top_y - height).max(screen_frame.origin.y + 12.0);
        let rect = NSRect::new(NSPoint::new(x, origin_y), NSSize::new(width, height));
        unsafe {
            let ns_panel: &NSPanel = &*raw_ptr.cast::<NSPanel>();
            ns_panel.setIgnoresMouseEvents(false);
            ns_panel.setFrame_display(rect, true);
            ns_panel.orderFrontRegardless();
        }
    }

    pub fn resize_hud_panel(window: &WebviewWindow, width: f64, height: f64) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Ok(raw_ptr) = window.ns_window() else {
            return;
        };
        if raw_ptr.is_null() {
            return;
        }
        let (screen_frame, _) = active_screen_frame(mtm);
        let x = (screen_frame.origin.x + screen_frame.size.width - width - 14.0)
            .max(screen_frame.origin.x + 12.0);
        let top_y = screen_frame.origin.y + screen_frame.size.height - 42.0;
        let origin_y = (top_y - height).max(screen_frame.origin.y + 12.0);
        let rect = NSRect::new(NSPoint::new(x, origin_y), NSSize::new(width, height));
        unsafe {
            let ns_panel: &NSPanel = &*raw_ptr.cast::<NSPanel>();
            ns_panel.setFrame_display(rect, true);
        }
    }

    pub fn set_panel_ignores_mouse_events(window: &WebviewWindow, ignore: bool) {
        if let Ok(raw_ptr) = window.ns_window() {
            if !raw_ptr.is_null() {
                unsafe {
                    let ns_panel: &NSPanel = &*raw_ptr.cast::<NSPanel>();
                    ns_panel.setIgnoresMouseEvents(ignore);
                }
            }
        }
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
    let toggle_panel_item =
        MenuItemBuilder::with_id("toggle_panel", "Toggle Live Agent Panel").build(app_handle)?;
    let stop_item =
        MenuItemBuilder::with_id("stop_task", "Stop Active Task (⌃⌥S)").build(app_handle)?;
    let test_pointer_item =
        MenuItemBuilder::with_id("test_pointer", "Test Pointer Overlay").build(app_handle)?;
    let quit_item =
        MenuItemBuilder::with_id("quit_app", "Quit Task Companion").build(app_handle)?;

    let tray_menu = MenuBuilder::new(app_handle)
        .items(&[
            &toggle_item,
            &toggle_panel_item,
            &stop_item,
            &test_pointer_item,
            &quit_item,
        ])
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
            "toggle_panel" => {
                toggle_orchestration_panel(handle);
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
        macos_panel::promote_to_panel(&main_win, true, 0, false, false, false);
    }

    if app_handle.get_webview_window(PANEL_WINDOW_LABEL).is_none() {
        let panel_win = WebviewWindowBuilder::new(
            app_handle,
            PANEL_WINDOW_LABEL,
            WebviewUrl::App("index.html?view=panel".into()),
        )
        .title("Task Companion Live HUD")
        .inner_size(PANEL_WIDTH, PANEL_HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        // The HUD stays visible while the agent types, so it must never become the key
        // window: clicking Pause/Resume would otherwise route agent keystrokes into it.
        .focusable(false)
        .skip_taskbar(true)
        .shadow(false)
        .visible(false)
        .build()?;

        #[cfg(target_os = "macos")]
        macos_panel::promote_to_panel(&panel_win, false, 1, true, false, true);
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
            let _ = macos_panel::sync_overlay_to_primary_screen(&overlay);
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

pub fn sync_overlay_to_metrics(app_handle: &AppHandle, metrics: ScreenMetrics) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(overlay) = handle.get_webview_window(OVERLAY_WINDOW_LABEL) else {
            return;
        };
        #[cfg(target_os = "macos")]
        {
            macos_panel::sync_overlay_to_screen_metrics(&overlay, metrics);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = overlay.set_position(Position::Logical(LogicalPosition::new(
                metrics.origin_x,
                metrics.origin_y,
            )));
            let _ = overlay.set_size(Size::Logical(LogicalSize::new(
                metrics.logical_width,
                metrics.logical_height,
            )));
        }
    });
}

/// True when one of the companion's own windows holds keyboard focus, meaning
/// synthesized keystrokes would land in our UI instead of the target app.
pub fn companion_window_is_key(app_handle: &AppHandle) -> bool {
    [TRAY_WINDOW_LABEL, PANEL_WINDOW_LABEL].iter().any(|label| {
        app_handle
            .get_webview_window(label)
            .is_some_and(|w| w.is_focused().unwrap_or(false))
    })
}

pub fn set_hud_ignores_mouse_events(app_handle: &AppHandle, ignore: bool) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(win) = handle.get_webview_window(PANEL_WINDOW_LABEL) else {
            return;
        };
        let _ = win.set_ignore_cursor_events(ignore);
        #[cfg(target_os = "macos")]
        {
            macos_panel::set_panel_ignores_mouse_events(&win, ignore);
        }
    });
}

pub fn clear_pointer_cue(app_handle: &AppHandle) {
    let _ = app_handle.emit("clear-pointer-cue", ());
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
            let _ = win.center();
            let _ = win.show();
            let _ = win.set_focus();
        }
    });
}

/// Task started: hide the main modal and show only the live orchestrator HUD.
pub fn enter_task_mode(app_handle: &AppHandle) {
    hide_tray_popover(app_handle);
    show_orchestration_panel(app_handle);
}

/// Task finished/stopped: hide the live orchestrator and bring the main modal back.
pub fn exit_task_mode(app_handle: &AppHandle) {
    hide_orchestration_panel(app_handle);
    show_tray_popover(app_handle, false);
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

pub fn show_orchestration_panel(app_handle: &AppHandle) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(win) = handle.get_webview_window(PANEL_WINDOW_LABEL) else {
            return;
        };
        #[cfg(target_os = "macos")]
        {
            macos_panel::position_and_show_hud_panel(&win, PANEL_WIDTH, PANEL_HEIGHT);
        }
        #[cfg(not(target_os = "macos"))]
        {
            if let Ok(metrics) = primary_screen_metrics() {
                let x = (metrics.origin_x + metrics.logical_width - PANEL_WIDTH - 14.0)
                    .max(metrics.origin_x + 12.0);
                let y = metrics.origin_y + 42.0;
                let _ = win.set_position(Position::Logical(LogicalPosition::new(x, y)));
                let _ = win.set_size(Size::Logical(LogicalSize::new(PANEL_WIDTH, PANEL_HEIGHT)));
            }
            let _ = win.show();
        }
    });
}

pub fn hide_orchestration_panel(app_handle: &AppHandle) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(win) = handle.get_webview_window(PANEL_WINDOW_LABEL) else {
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

pub fn toggle_orchestration_panel(app_handle: &AppHandle) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(win) = handle.get_webview_window(PANEL_WINDOW_LABEL) else {
            return;
        };
        #[cfg(target_os = "macos")]
        {
            if macos_panel::is_popover_visible_on_active_space(&win) {
                macos_panel::hide_popover_panel(&win);
            } else {
                macos_panel::position_and_show_hud_panel(&win, PANEL_WIDTH, PANEL_HEIGHT);
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            if win.is_visible().unwrap_or(false) {
                let _ = win.hide();
            } else {
                let _ = win.show();
            }
        }
    });
}

pub fn set_orchestration_panel_collapsed(app_handle: &AppHandle, collapsed: bool) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(win) = handle.get_webview_window(PANEL_WINDOW_LABEL) else {
            return;
        };
        let (width, height) = if collapsed {
            (PANEL_COLLAPSED_WIDTH, PANEL_COLLAPSED_HEIGHT)
        } else {
            (PANEL_WIDTH, PANEL_HEIGHT)
        };
        #[cfg(target_os = "macos")]
        {
            macos_panel::resize_hud_panel(&win, width, height);
        }
        #[cfg(not(target_os = "macos"))]
        {
            if let Ok(metrics) = primary_screen_metrics() {
                let x = (metrics.origin_x + metrics.logical_width - width - 14.0)
                    .max(metrics.origin_x + 12.0);
                let y = metrics.origin_y + 42.0;
                let _ = win.set_position(Position::Logical(LogicalPosition::new(x, y)));
                let _ = win.set_size(Size::Logical(LogicalSize::new(width, height)));
            }
        }
    });
}

pub fn bring_overlay_to_front(app_handle: &AppHandle) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(overlay) = handle.get_webview_window(OVERLAY_WINDOW_LABEL) else {
            return;
        };
        let _ = overlay.set_ignore_cursor_events(true);

        #[cfg(target_os = "macos")]
        {
            macos_panel::order_overlay_front(&overlay);
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = overlay.show();
        }
    });
}

pub fn emit_pointer_cue(
    app_handle: &AppHandle,
    payload: PointerCuePayload,
) -> tauri::Result<()> {
    bring_overlay_to_front(app_handle);
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
        target_width: Some(140.0),
        target_height: Some(36.0),
        target_ocr_id: Some(1),
        step_number: 1,
        is_acting: true,
        show_pointer: true,
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
            TaskPhase::Classifying => Some("Classifying...".to_string()),
            TaskPhase::Planning => Some("Planning...".to_string()),
            TaskPhase::Running => Some(format!("Step {}/{}", s.current_step, s.max_steps)),
            TaskPhase::Verifying => Some("Verifying...".to_string()),
            TaskPhase::Recovering => Some("Recovering...".to_string()),
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
        assert_eq!(custom_panel_size, panel_size);
    }
}
