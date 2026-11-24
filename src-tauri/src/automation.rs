use crate::capture::ScreenMetrics;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::thread;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AutomationError {
    #[error("macOS Accessibility permission has not been granted")]
    AccessibilityPermissionDenied,
    #[error("invalid automation input: {0}")]
    InvalidInput(String),
    #[error("failed to create macOS input event")]
    EventCreationFailed,
    #[error("command execution failed: {0}")]
    CommandFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButtonKind {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentAction {
    Click {
        x: f64,
        y: f64,
        button: MouseButtonKind,
        double: bool,
    },
    TypeText {
        text: String,
    },
    PressKey {
        key: String,
        modifiers: Vec<String>,
    },
    Scroll {
        x: f64,
        y: f64,
        delta_y: i32,
    },
    OpenApp {
        app_name: String,
    },
    OpenUrl {
        url: String,
    },
    Wait {
        ms: u64,
    },
    Done {
        summary: String,
    },
    Fail {
        reason: String,
    },
}

pub fn action_type_label(action: &AgentAction) -> &'static str {
    match action {
        AgentAction::Click { double: true, .. } => "double_click",
        AgentAction::Click {
            button: MouseButtonKind::Right,
            ..
        } => "right_click",
        AgentAction::Click { .. } => "click",
        AgentAction::TypeText { .. } => "type_text",
        AgentAction::PressKey { .. } => "press_key",
        AgentAction::Scroll { .. } => "scroll",
        AgentAction::OpenApp { .. } => "open_app",
        AgentAction::OpenUrl { .. } => "open_url",
        AgentAction::Wait { .. } => "wait",
        AgentAction::Done { .. } => "done",
        AgentAction::Fail { .. } => "fail",
    }
}

pub fn describe_action(action: &AgentAction) -> String {
    match action {
        AgentAction::Click {
            x,
            y,
            button,
            double,
        } => {
            let kind = match (button, *double) {
                (MouseButtonKind::Right, _) => "Right-click",
                (MouseButtonKind::Left, true) => "Double-click",
                (MouseButtonKind::Left, false) => "Click",
            };
            format!("{kind} ({}, {})", x.round() as i64, y.round() as i64)
        }
        AgentAction::TypeText { text } => {
            let preview: String = text.chars().take(28).collect();
            if text.chars().count() > 28 {
                format!("Type \"{preview}...\"")
            } else {
                format!("Type \"{preview}\"")
            }
        }
        AgentAction::PressKey { key, modifiers } => {
            if modifiers.is_empty() {
                format!("Press {key}")
            } else {
                format!("Press {}+{key}", modifiers.join("+"))
            }
        }
        AgentAction::Scroll { delta_y, .. } => {
            if *delta_y >= 0 {
                format!("Scroll up ({delta_y})")
            } else {
                format!("Scroll down ({})", delta_y.abs())
            }
        }
        AgentAction::OpenApp { app_name } => format!("Open {app_name}"),
        AgentAction::OpenUrl { url } => format!("Open URL {url}"),
        AgentAction::Wait { ms } => format!("Wait {ms}ms"),
        AgentAction::Done { summary } => format!("Done: {summary}"),
        AgentAction::Fail { reason } => format!("Failed: {reason}"),
    }
}

pub fn assess_action_risk(
    action: &AgentAction,
    narration: &str,
    target_text: Option<&str>,
    model_flagged: bool,
) -> Option<String> {
    if model_flagged {
        return Some("Agent flagged this action as potentially impactful.".to_string());
    }

    match action {
        AgentAction::PressKey { key, modifiers } => {
            let lower_key = key.trim().to_ascii_lowercase();
            let has_cmd = modifiers
                .iter()
                .any(|m| matches!(m.trim().to_ascii_lowercase().as_str(), "cmd" | "command" | "meta"));

            if has_cmd && matches!(lower_key.as_str(), "q" | "backspace" | "delete" | "enter" | "return") {
                return Some(format!("Shortcut Cmd+{lower_key} requires confirmation."));
            }

            if matches!(lower_key.as_str(), "enter" | "return") {
                if let Some(word) = find_sensitive_keyword(narration, target_text) {
                    return Some(format!("Submitting action involving \"{word}\" requires confirmation."));
                }
            }
            None
        }
        AgentAction::Click { .. } => {
            if let Some(word) = find_sensitive_keyword(narration, target_text) {
                return Some(format!("Clicking element involving \"{word}\" requires confirmation."));
            }
            None
        }
        _ => None,
    }
}

fn find_sensitive_keyword(narration: &str, target_text: Option<&str>) -> Option<&'static str> {
    const KEYWORDS: &[&str] = &[
        "delete",
        "remove",
        "send",
        "submit",
        "purchase",
        "buy",
        "pay",
        "checkout",
        "uninstall",
        "erase",
        "format",
        "transfer",
        "publish",
        "post",
    ];
    let combined = match target_text {
        Some(t) => format!("{} {}", narration.to_ascii_lowercase(), t.to_ascii_lowercase()),
        None => narration.to_ascii_lowercase(),
    };

    for &kw in KEYWORDS {
        if combined
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|token| token == kw)
        {
            return Some(kw);
        }
    }
    None
}

pub fn clamp_point(x: f64, y: f64, metrics: ScreenMetrics) -> (f64, f64) {
    let min_x = metrics.origin_x + 2.0;
    let max_x = (metrics.origin_x + metrics.logical_width - 2.0).max(min_x);
    let min_y = metrics.origin_y + 2.0;
    let max_y = (metrics.origin_y + metrics.logical_height - 2.0).max(min_y);
    (x.clamp(min_x, max_x), y.clamp(min_y, max_y))
}

pub fn validate_app_name(app_name: &str) -> Result<String, AutomationError> {
    let clean = app_name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if clean.is_empty() || clean.len() > 80 {
        return Err(AutomationError::InvalidInput(
            "app_name must be between 1 and 80 characters".to_string(),
        ));
    }
    if clean.starts_with('-') || clean.contains("..") {
        return Err(AutomationError::InvalidInput(
            "app_name contains forbidden sequence".to_string(),
        ));
    }
    let valid = clean
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '+'));
    if !valid {
        return Err(AutomationError::InvalidInput(
            "app_name contains unsupported characters".to_string(),
        ));
    }
    Ok(clean)
}

pub fn validate_open_url(url: &str) -> Result<String, AutomationError> {
    let clean = url
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if clean.is_empty() || clean.len() > 1024 {
        return Err(AutomationError::InvalidInput(
            "url must be between 1 and 1024 characters".to_string(),
        ));
    }
    if clean.chars().any(char::is_whitespace) {
        return Err(AutomationError::InvalidInput(
            "url must not contain whitespace".to_string(),
        ));
    }
    let lower = clean.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(AutomationError::InvalidInput(
            "url must start with https:// or http://".to_string(),
        ));
    }
    Ok(clean)
}

pub fn map_key_to_keycode(key: &str) -> Option<u16> {
    match key.trim().to_ascii_lowercase().as_str() {
        "a" => Some(0x00),
        "s" => Some(0x01),
        "d" => Some(0x02),
        "f" => Some(0x03),
        "h" => Some(0x04),
        "g" => Some(0x05),
        "z" => Some(0x06),
        "x" => Some(0x07),
        "c" => Some(0x08),
        "v" => Some(0x09),
        "b" => Some(0x0B),
        "q" => Some(0x0C),
        "w" => Some(0x0D),
        "e" => Some(0x0E),
        "r" => Some(0x0F),
        "y" => Some(0x10),
        "t" => Some(0x11),
        "1" => Some(0x12),
        "2" => Some(0x13),
        "3" => Some(0x14),
        "4" => Some(0x15),
        "6" => Some(0x16),
        "5" => Some(0x17),
        "=" | "equal" => Some(0x18),
        "9" => Some(0x19),
        "7" => Some(0x1A),
        "-" | "minus" => Some(0x1B),
        "8" => Some(0x1C),
        "0" => Some(0x1D),
        "]" => Some(0x1E),
        "o" => Some(0x1F),
        "u" => Some(0x20),
        "[" => Some(0x21),
        "i" => Some(0x22),
        "p" => Some(0x23),
        "enter" | "return" => Some(0x24),
        "l" => Some(0x25),
        "j" => Some(0x26),
        "k" => Some(0x28),
        ";" | "semicolon" => Some(0x29),
        "," | "comma" => Some(0x2B),
        "/" | "slash" => Some(0x2C),
        "n" => Some(0x2D),
        "m" => Some(0x2E),
        "." | "period" => Some(0x2F),
        "tab" => Some(0x30),
        "space" => Some(0x31),
        "backspace" | "delete" => Some(0x33),
        "escape" | "esc" => Some(0x35),
        "f5" => Some(0x60),
        "f6" => Some(0x61),
        "f7" => Some(0x62),
        "f3" => Some(0x63),
        "f8" => Some(0x64),
        "f9" => Some(0x65),
        "f11" => Some(0x67),
        "f10" => Some(0x6D),
        "f12" => Some(0x6F),
        "home" => Some(0x73),
        "pageup" => Some(0x74),
        "end" => Some(0x77),
        "f4" => Some(0x76),
        "f2" => Some(0x78),
        "pagedown" => Some(0x79),
        "f1" => Some(0x7A),
        "left" => Some(0x7B),
        "right" => Some(0x7C),
        "down" => Some(0x7D),
        "up" => Some(0x7E),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
mod macos_ax {
    use std::ffi::c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
        static kAXTrustedCheckOptionPrompt: *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: *const c_void;
        static kCFTypeDictionaryKeyCallBacks: u8;
        static kCFTypeDictionaryValueCallBacks: u8;
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: isize,
            key_callbacks: *const u8,
            value_callbacks: *const u8,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    pub fn is_trusted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }

    pub fn request_with_prompt() -> bool {
        unsafe {
            if AXIsProcessTrusted() != 0 {
                return true;
            }
            let keys = [kAXTrustedCheckOptionPrompt];
            let values = [kCFBooleanTrue];
            let dict = CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            if dict.is_null() {
                return AXIsProcessTrusted() != 0;
            }
            let trusted = AXIsProcessTrustedWithOptions(dict) != 0;
            CFRelease(dict);
            trusted
        }
    }
}

pub fn has_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_ax::is_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn request_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_ax::request_with_prompt()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn is_kill_switch_pressed() -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_core_graphics::{CGEventFlags, CGEventSource, CGEventSourceStateID};
        let flags = CGEventSource::flags_state(CGEventSourceStateID::CombinedSessionState);
        let ctrl_opt = flags.contains(CGEventFlags::MaskControl)
            && flags.contains(CGEventFlags::MaskAlternate);
        if !ctrl_opt {
            return false;
        }
        CGEventSource::key_state(CGEventSourceStateID::CombinedSessionState, 0x01)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn execute_action(action: &AgentAction, metrics: ScreenMetrics) -> Result<(), AutomationError> {
    match action {
        AgentAction::OpenApp { app_name } => {
            let clean = validate_app_name(app_name)?;
            let status = Command::new("open")
                .arg("-a")
                .arg(&clean)
                .status()
                .map_err(|e| AutomationError::CommandFailed(e.to_string()))?;
            if !status.success() {
                return Err(AutomationError::CommandFailed(format!(
                    "failed to open application \"{clean}\""
                )));
            }
            Ok(())
        }
        AgentAction::OpenUrl { url } => {
            let clean = validate_open_url(url)?;
            let status = Command::new("open")
                .arg(&clean)
                .status()
                .map_err(|e| AutomationError::CommandFailed(e.to_string()))?;
            if !status.success() {
                return Err(AutomationError::CommandFailed(format!(
                    "failed to open URL \"{clean}\""
                )));
            }
            Ok(())
        }
        AgentAction::Wait { ms } => {
            let clamped = (*ms).clamp(100, 5000);
            thread::sleep(Duration::from_millis(clamped));
            Ok(())
        }
        AgentAction::Done { .. } | AgentAction::Fail { .. } => Ok(()),
        AgentAction::Click { .. }
        | AgentAction::TypeText { .. }
        | AgentAction::PressKey { .. }
        | AgentAction::Scroll { .. } => {
            if !has_accessibility_permission() {
                return Err(AutomationError::AccessibilityPermissionDenied);
            }
            execute_hid_action(action, metrics)
        }
    }
}

#[cfg(target_os = "macos")]
fn execute_hid_action(action: &AgentAction, metrics: ScreenMetrics) -> Result<(), AutomationError> {
    use objc2_core_foundation::CGPoint;
    use objc2_core_graphics::{
        CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID,
        CGEventTapLocation, CGEventType, CGMouseButton, CGScrollEventUnit,
    };

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .ok_or(AutomationError::EventCreationFailed)?;

    match action {
        AgentAction::Click {
            x,
            y,
            button,
            double,
        } => {
            let (cx, cy) = clamp_point(*x, *y, metrics);
            let point = CGPoint::new(cx, cy);
            let (cg_btn, down_type, up_type) = match button {
                MouseButtonKind::Left => (
                    CGMouseButton::Left,
                    CGEventType::LeftMouseDown,
                    CGEventType::LeftMouseUp,
                ),
                MouseButtonKind::Right => (
                    CGMouseButton::Right,
                    CGEventType::RightMouseDown,
                    CGEventType::RightMouseUp,
                ),
            };

            let move_ev = CGEvent::new_mouse_event(
                Some(&source),
                CGEventType::MouseMoved,
                point,
                cg_btn,
            )
            .ok_or(AutomationError::EventCreationFailed)?;
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&move_ev));
            thread::sleep(Duration::from_millis(35));

            let clicks = if *double { 2 } else { 1 };
            for click_state in 1..=clicks {
                let down = CGEvent::new_mouse_event(Some(&source), down_type, point, cg_btn)
                    .ok_or(AutomationError::EventCreationFailed)?;
                CGEvent::set_integer_value_field(
                    Some(&down),
                    CGEventField::MouseEventClickState,
                    click_state,
                );
                let up = CGEvent::new_mouse_event(Some(&source), up_type, point, cg_btn)
                    .ok_or(AutomationError::EventCreationFailed)?;
                CGEvent::set_integer_value_field(
                    Some(&up),
                    CGEventField::MouseEventClickState,
                    click_state,
                );
                CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down));
                thread::sleep(Duration::from_millis(20));
                CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));
                if click_state < clicks {
                    thread::sleep(Duration::from_millis(45));
                }
            }
            Ok(())
        }
        AgentAction::TypeText { text } => {
            let filtered: String = text
                .chars()
                .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                .take(500)
                .collect();

            for ch in filtered.chars() {
                match ch {
                    '\n' => post_virtual_key(&source, 0x24, CGEventFlags::empty())?,
                    '\t' => post_virtual_key(&source, 0x30, CGEventFlags::empty())?,
                    other => {
                        let mut utf16 = [0u16; 2];
                        let slice = other.encode_utf16(&mut utf16);
                        let down = CGEvent::new_keyboard_event(Some(&source), 0, true)
                            .ok_or(AutomationError::EventCreationFailed)?;
                        let up = CGEvent::new_keyboard_event(Some(&source), 0, false)
                            .ok_or(AutomationError::EventCreationFailed)?;
                        unsafe {
                            CGEvent::keyboard_set_unicode_string(
                                Some(&down),
                                slice.len() as _,
                                slice.as_ptr(),
                            );
                            CGEvent::keyboard_set_unicode_string(
                                Some(&up),
                                slice.len() as _,
                                slice.as_ptr(),
                            );
                        }
                        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down));
                        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));
                    }
                }
                thread::sleep(Duration::from_millis(10));
            }
            Ok(())
        }
        AgentAction::PressKey { key, modifiers } => {
            let keycode = map_key_to_keycode(key).ok_or_else(|| {
                AutomationError::InvalidInput(format!("unsupported key \"{key}\""))
            })?;
            let mut flags = CGEventFlags::empty();
            for m in modifiers {
                match m.trim().to_ascii_lowercase().as_str() {
                    "cmd" | "command" | "meta" => flags |= CGEventFlags::MaskCommand,
                    "shift" => flags |= CGEventFlags::MaskShift,
                    "alt" | "option" => flags |= CGEventFlags::MaskAlternate,
                    "ctrl" | "control" => flags |= CGEventFlags::MaskControl,
                    other => {
                        return Err(AutomationError::InvalidInput(format!(
                            "unsupported modifier \"{other}\""
                        )));
                    }
                }
            }
            post_virtual_key(&source, keycode, flags)
        }
        AgentAction::Scroll { x, y, delta_y } => {
            let (cx, cy) = clamp_point(*x, *y, metrics);
            let point = CGPoint::new(cx, cy);
            let move_ev = CGEvent::new_mouse_event(
                Some(&source),
                CGEventType::MouseMoved,
                point,
                CGMouseButton::Left,
            )
            .ok_or(AutomationError::EventCreationFailed)?;
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&move_ev));
            thread::sleep(Duration::from_millis(20));

            let clamped_delta = (*delta_y).clamp(-20, 20);
            let scroll = CGEvent::new_scroll_wheel_event2(
                Some(&source),
                CGScrollEventUnit::Line,
                1,
                clamped_delta,
                0,
                0,
            )
            .ok_or(AutomationError::EventCreationFailed)?;
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&scroll));
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(target_os = "macos")]
fn post_virtual_key(
    source: &objc2_core_graphics::CGEventSource,
    keycode: u16,
    flags: objc2_core_graphics::CGEventFlags,
) -> Result<(), AutomationError> {
    use objc2_core_graphics::{CGEvent, CGEventTapLocation};

    let down = CGEvent::new_keyboard_event(Some(source), keycode, true)
        .ok_or(AutomationError::EventCreationFailed)?;
    let up = CGEvent::new_keyboard_event(Some(source), keycode, false)
        .ok_or(AutomationError::EventCreationFailed)?;
    if !flags.is_empty() {
        CGEvent::set_flags(Some(&down), flags);
        CGEvent::set_flags(Some(&up), flags);
    }
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down));
    thread::sleep(Duration::from_millis(15));
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn execute_hid_action(_action: &AgentAction, _metrics: ScreenMetrics) -> Result<(), AutomationError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metrics() -> ScreenMetrics {
        ScreenMetrics {
            origin_x: 100.0,
            origin_y: 50.0,
            logical_width: 1440.0,
            logical_height: 900.0,
            pixel_width: 2880,
            pixel_height: 1800,
            scale_factor: 2.0,
        }
    }

    #[test]
    fn maps_common_keys_to_macos_virtual_keycodes() {
        assert_eq!(map_key_to_keycode("a"), Some(0x00));
        assert_eq!(map_key_to_keycode("Enter"), Some(0x24));
        assert_eq!(map_key_to_keycode("RETURN"), Some(0x24));
        assert_eq!(map_key_to_keycode("space"), Some(0x31));
        assert_eq!(map_key_to_keycode("esc"), Some(0x35));
        assert_eq!(map_key_to_keycode("left"), Some(0x7B));
        assert_eq!(map_key_to_keycode("unknown_key"), None);
    }

    #[test]
    fn clamps_points_within_display_bounds() {
        let m = sample_metrics();
        assert_eq!(clamp_point(0.0, 0.0, m), (102.0, 52.0));
        assert_eq!(clamp_point(5000.0, 5000.0, m), (1538.0, 948.0));
        assert_eq!(clamp_point(500.0, 400.0, m), (500.0, 400.0));
    }

    #[test]
    fn validates_urls_and_app_names_strictly() {
        assert!(validate_open_url("https://example.com/docs").is_ok());
        assert!(validate_open_url("http://localhost:3000").is_ok());
        assert!(validate_open_url("file:///etc/passwd").is_err());
        assert!(validate_open_url("javascript:alert(1)").is_err());
        assert!(validate_open_url("https://example.com with spaces").is_err());

        assert!(validate_app_name("Notes").is_ok());
        assert!(validate_app_name("Visual Studio Code").is_ok());
        assert!(validate_app_name("-rf").is_err());
        assert!(validate_app_name("Notes; rm -rf /").is_err());
        assert!(validate_app_name("../Secrets").is_err());
    }

    #[test]
    fn assesses_risky_actions_accurately() {
        let safe_click = AgentAction::Click {
            x: 300.0,
            y: 200.0,
            button: MouseButtonKind::Left,
            double: false,
        };
        assert!(assess_action_risk(&safe_click, "Opening the New Note button", Some("New Note"), false).is_none());

        assert!(assess_action_risk(&safe_click, "Clicking Delete Note", Some("Delete"), false).is_some());
        assert!(assess_action_risk(&safe_click, "Clicking Submit", None, true).is_some());

        let cmd_q = AgentAction::PressKey {
            key: "q".to_string(),
            modifiers: vec!["cmd".to_string()],
        };
        assert!(assess_action_risk(&cmd_q, "Closing window", None, false).is_some());

        let cmd_c = AgentAction::PressKey {
            key: "c".to_string(),
            modifiers: vec!["cmd".to_string()],
        };
        assert!(assess_action_risk(&cmd_c, "Copying selected text", None, false).is_none());
    }
}
