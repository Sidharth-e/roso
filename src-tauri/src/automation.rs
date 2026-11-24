use crate::capture::ScreenMetrics;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionExecutionOutput {
    pub note: Option<String>,
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
    Drag {
        from_x: f64,
        from_y: f64,
        to_x: f64,
        to_y: f64,
    },
    TypeText {
        text: String,
        #[serde(default)]
        replace: bool,
        #[serde(default)]
        submit: bool,
    },
    PasteText {
        text: String,
        #[serde(default)]
        replace: bool,
        #[serde(default)]
        submit: bool,
    },
    ReadClipboard,
    PressKey {
        key: String,
        modifiers: Vec<String>,
    },
    Scroll {
        x: f64,
        y: f64,
        delta_y: i32,
        #[serde(default)]
        delta_x: i32,
    },
    OpenApp {
        app_name: String,
    },
    FocusApp {
        app_name: String,
    },
    OpenUrl {
        url: String,
        #[serde(default)]
        app_name: Option<String>,
    },
    OpenPath {
        path: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedKeyChord {
    pub keycode: u16,
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

pub fn action_type_label(action: &AgentAction) -> &'static str {
    match action {
        AgentAction::Click { double: true, .. } => "double_click",
        AgentAction::Click {
            button: MouseButtonKind::Right,
            ..
        } => "right_click",
        AgentAction::Click { .. } => "click",
        AgentAction::Drag { .. } => "drag",
        AgentAction::TypeText { .. } => "type_text",
        AgentAction::PasteText { .. } => "paste_text",
        AgentAction::ReadClipboard => "read_clipboard",
        AgentAction::PressKey { .. } => "press_key",
        AgentAction::Scroll { .. } => "scroll",
        AgentAction::OpenApp { .. } => "open_app",
        AgentAction::FocusApp { .. } => "focus_app",
        AgentAction::OpenUrl { .. } => "open_url",
        AgentAction::OpenPath { .. } => "open_path",
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
        AgentAction::Drag {
            from_x,
            from_y,
            to_x,
            to_y,
        } => format!(
            "Drag ({}, {}) -> ({}, {})",
            from_x.round() as i64,
            from_y.round() as i64,
            to_x.round() as i64,
            to_y.round() as i64
        ),
        AgentAction::TypeText {
            text,
            replace,
            submit,
        } => {
            let verb = if *replace { "Replace with" } else { "Type" };
            let suffix = if *submit { " + Enter" } else { "" };
            let preview: String = text.chars().take(28).collect();
            if text.chars().count() > 28 {
                format!("{verb} \"{preview}...\"{suffix}")
            } else {
                format!("{verb} \"{preview}\"{suffix}")
            }
        }
        AgentAction::PasteText {
            text,
            replace,
            submit,
        } => {
            let verb = if *replace {
                "Replace & paste"
            } else {
                "Paste"
            };
            let suffix = if *submit { " + Enter" } else { "" };
            let preview: String = text.chars().take(28).collect();
            if text.chars().count() > 28 {
                format!("{verb} \"{preview}...\"{suffix}")
            } else {
                format!("{verb} \"{preview}\"{suffix}")
            }
        }
        AgentAction::ReadClipboard => "Read clipboard".to_string(),
        AgentAction::PressKey { key, modifiers } => {
            if modifiers.is_empty() {
                format!("Press {key}")
            } else {
                format!("Press {}+{key}", modifiers.join("+"))
            }
        }
        AgentAction::Scroll {
            delta_y, delta_x, ..
        } => {
            if *delta_x != 0 && *delta_y == 0 {
                if *delta_x > 0 {
                    format!("Scroll left ({delta_x})")
                } else {
                    format!("Scroll right ({})", delta_x.abs())
                }
            } else if *delta_y >= 0 {
                format!("Scroll up ({delta_y})")
            } else {
                format!("Scroll down ({})", delta_y.abs())
            }
        }
        AgentAction::OpenApp { app_name } => format!("Open {app_name}"),
        AgentAction::FocusApp { app_name } => format!("Focus {app_name}"),
        AgentAction::OpenUrl { url, app_name } => match app_name {
            Some(app) if !app.trim().is_empty() => format!("Open {url} in {app}"),
            _ => format!("Open URL {url}"),
        },
        AgentAction::OpenPath { path } => format!("Open path {path}"),
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
            if let Ok(chord) = parse_key_chord(key, modifiers) {
                if chord.cmd && matches!(chord.keycode, 0x0C | 0x33 | 0x75 | 0x24 | 0x4C) {
                    return Some(format!("Shortcut Cmd+{key} requires confirmation."));
                }
                if matches!(chord.keycode, 0x24 | 0x4C) {
                    if let Some(word) = find_sensitive_keyword(narration, target_text) {
                        return Some(format!(
                            "Submitting action involving \"{word}\" requires confirmation."
                        ));
                    }
                }
            }
            None
        }
        AgentAction::TypeText { submit: true, .. } | AgentAction::PasteText { submit: true, .. } => {
            if let Some(word) = find_sensitive_keyword(narration, target_text) {
                return Some(format!(
                    "Submitting input involving \"{word}\" requires confirmation."
                ));
            }
            None
        }
        AgentAction::Click { .. } => {
            if let Some(word) = find_sensitive_keyword(narration, target_text) {
                return Some(format!(
                    "Clicking element involving \"{word}\" requires confirmation."
                ));
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

pub fn validate_open_path(raw: &str) -> Result<PathBuf, AutomationError> {
    let clean = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if clean.is_empty() || clean.len() > 512 {
        return Err(AutomationError::InvalidInput(
            "path must be between 1 and 512 characters".to_string(),
        ));
    }
    if clean.starts_with('-') {
        return Err(AutomationError::InvalidInput(
            "path must not start with '-'".to_string(),
        ));
    }
    let resolved = if clean == "~" {
        dirs::home_dir().ok_or_else(|| {
            AutomationError::InvalidInput("unable to resolve home directory".to_string())
        })?
    } else if let Some(rest) = clean.strip_prefix("~/") {
        let home = dirs::home_dir().ok_or_else(|| {
            AutomationError::InvalidInput("unable to resolve home directory".to_string())
        })?;
        home.join(rest)
    } else {
        PathBuf::from(&clean)
    };

    if !resolved.exists() {
        return Err(AutomationError::InvalidInput(format!(
            "path does not exist: {}",
            resolved.display()
        )));
    }
    Ok(resolved)
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
        "=" | "equal" | "equals" => Some(0x18),
        "9" => Some(0x19),
        "7" => Some(0x1A),
        "-" | "minus" | "hyphen" => Some(0x1B),
        "8" => Some(0x1C),
        "0" => Some(0x1D),
        "]" | "rightbracket" | "bracketright" => Some(0x1E),
        "o" => Some(0x1F),
        "u" => Some(0x20),
        "[" | "leftbracket" | "bracketleft" => Some(0x21),
        "i" => Some(0x22),
        "p" => Some(0x23),
        "enter" | "return" => Some(0x24),
        "l" => Some(0x25),
        "j" => Some(0x26),
        "'" | "quote" | "apostrophe" => Some(0x27),
        "k" => Some(0x28),
        ";" | "semicolon" => Some(0x29),
        "\\" | "backslash" => Some(0x2A),
        "," | "comma" => Some(0x2B),
        "/" | "slash" => Some(0x2C),
        "n" => Some(0x2D),
        "m" => Some(0x2E),
        "." | "period" | "dot" => Some(0x2F),
        "tab" => Some(0x30),
        "space" | "spacebar" => Some(0x31),
        "`" | "grave" | "backtick" => Some(0x32),
        "backspace" | "back_space" | "delete" | "del_back" => Some(0x33),
        "escape" | "esc" => Some(0x35),
        "command" | "cmd" => Some(0x37),
        "shift" => Some(0x38),
        "capslock" | "caps_lock" => Some(0x39),
        "option" | "alt" => Some(0x3A),
        "control" | "ctrl" => Some(0x3B),
        "f17" => Some(0x40),
        "kp_decimal" => Some(0x41),
        "kp_multiply" | "multiply" => Some(0x43),
        "kp_plus" | "add" => Some(0x45),
        "clear" | "kp_clear" => Some(0x47),
        "kp_divide" | "divide" => Some(0x4B),
        "kp_enter" | "numpadenter" => Some(0x4C),
        "kp_minus" | "subtract" => Some(0x4E),
        "f18" => Some(0x4F),
        "f19" => Some(0x50),
        "kp_equals" => Some(0x51),
        "kp0" | "numpad0" => Some(0x52),
        "kp1" | "numpad1" => Some(0x53),
        "kp2" | "numpad2" => Some(0x54),
        "kp3" | "numpad3" => Some(0x55),
        "kp4" | "numpad4" => Some(0x56),
        "kp5" | "numpad5" => Some(0x57),
        "kp6" | "numpad6" => Some(0x58),
        "kp7" | "numpad7" => Some(0x59),
        "f20" => Some(0x5A),
        "kp8" | "numpad8" => Some(0x5B),
        "kp9" | "numpad9" => Some(0x5C),
        "f5" => Some(0x60),
        "f6" => Some(0x61),
        "f7" => Some(0x62),
        "f3" => Some(0x63),
        "f8" => Some(0x64),
        "f9" => Some(0x65),
        "f11" => Some(0x67),
        "f13" => Some(0x69),
        "f16" => Some(0x6A),
        "f14" => Some(0x6B),
        "f10" => Some(0x6D),
        "f12" => Some(0x6F),
        "f15" => Some(0x71),
        "home" => Some(0x73),
        "pageup" | "page_up" | "pgup" => Some(0x74),
        "forwarddelete" | "forward_delete" | "del" => Some(0x75),
        "f4" => Some(0x76),
        "end" => Some(0x77),
        "f2" => Some(0x78),
        "pagedown" | "page_down" | "pgdn" => Some(0x79),
        "f1" => Some(0x7A),
        "left" | "arrowleft" | "arrow_left" => Some(0x7B),
        "right" | "arrowright" | "arrow_right" => Some(0x7C),
        "down" | "arrowdown" | "arrow_down" => Some(0x7D),
        "up" | "arrowup" | "arrow_up" => Some(0x7E),
        _ => None,
    }
}

pub fn map_char_to_keycode_and_shift(ch: char) -> Option<(u16, bool)> {
    match ch {
        'a'..='z' => {
            let key = ch.to_string();
            map_key_to_keycode(&key).map(|kc| (kc, false))
        }
        'A'..='Z' => {
            let key = ch.to_ascii_lowercase().to_string();
            map_key_to_keycode(&key).map(|kc| (kc, true))
        }
        '0'..='9' => {
            let key = ch.to_string();
            map_key_to_keycode(&key).map(|kc| (kc, false))
        }
        ')' => Some((0x1D, true)),
        '!' => Some((0x12, true)),
        '@' => Some((0x13, true)),
        '#' => Some((0x14, true)),
        '$' => Some((0x15, true)),
        '%' => Some((0x17, true)),
        '^' => Some((0x16, true)),
        '&' => Some((0x1A, true)),
        '*' => Some((0x1C, true)),
        '(' => Some((0x19, true)),
        ' ' => Some((0x31, false)),
        '\n' => Some((0x24, false)),
        '\t' => Some((0x30, false)),
        '-' => Some((0x1B, false)),
        '_' => Some((0x1B, true)),
        '=' => Some((0x18, false)),
        '+' => Some((0x18, true)),
        '[' => Some((0x21, false)),
        '{' => Some((0x21, true)),
        ']' => Some((0x1E, false)),
        '}' => Some((0x1E, true)),
        '\\' => Some((0x2A, false)),
        '|' => Some((0x2A, true)),
        ';' => Some((0x29, false)),
        ':' => Some((0x29, true)),
        '\'' => Some((0x27, false)),
        '"' => Some((0x27, true)),
        ',' => Some((0x2B, false)),
        '<' => Some((0x2B, true)),
        '.' => Some((0x2F, false)),
        '>' => Some((0x2F, true)),
        '/' => Some((0x2C, false)),
        '?' => Some((0x2C, true)),
        '`' => Some((0x32, false)),
        '~' => Some((0x32, true)),
        _ => None,
    }
}

pub fn parse_key_chord(key: &str, modifiers: &[String]) -> Result<ParsedKeyChord, AutomationError> {
    let mut cmd = false;
    let mut shift = false;
    let mut alt = false;
    let mut ctrl = false;

    for m in modifiers {
        apply_modifier_token(m, &mut cmd, &mut shift, &mut alt, &mut ctrl)?;
    }

    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(AutomationError::InvalidInput("empty key".to_string()));
    }

    let base_key: &str = if trimmed == "+" {
        "+"
    } else if let Some(prefix) = trimmed.strip_suffix("++") {
        for part in prefix.split('+').filter(|p| !p.trim().is_empty()) {
            apply_modifier_token(part, &mut cmd, &mut shift, &mut alt, &mut ctrl)?;
        }
        "+"
    } else if trimmed.contains('+') {
        let parts: Vec<&str> = trimmed.split('+').collect();
        if let Some((last, mod_parts)) = parts.split_last() {
            for part in mod_parts {
                if !part.trim().is_empty() {
                    apply_modifier_token(part, &mut cmd, &mut shift, &mut alt, &mut ctrl)?;
                }
            }
            last.trim()
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    if let Some(keycode) = map_key_to_keycode(base_key) {
        return Ok(ParsedKeyChord {
            keycode,
            cmd,
            shift,
            alt,
            ctrl,
        });
    }

    let mut chars = base_key.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if let Some((keycode, needs_shift)) = map_char_to_keycode_and_shift(ch) {
            return Ok(ParsedKeyChord {
                keycode,
                cmd,
                shift: shift || needs_shift,
                alt,
                ctrl,
            });
        }
    }

    Err(AutomationError::InvalidInput(format!(
        "unsupported key \"{key}\""
    )))
}

fn apply_modifier_token(
    token: &str,
    cmd: &mut bool,
    shift: &mut bool,
    alt: &mut bool,
    ctrl: &mut bool,
) -> Result<(), AutomationError> {
    match token.trim().to_ascii_lowercase().as_str() {
        "cmd" | "command" | "meta" | "super" | "⌘" => {
            *cmd = true;
            Ok(())
        }
        "shift" | "⇧" => {
            *shift = true;
            Ok(())
        }
        "alt" | "option" | "opt" | "⌥" => {
            *alt = true;
            Ok(())
        }
        "ctrl" | "control" | "⌃" => {
            *ctrl = true;
            Ok(())
        }
        other => Err(AutomationError::InvalidInput(format!(
            "unsupported modifier \"{other}\""
        ))),
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

fn write_clipboard_text(text: &str) -> Result<(), AutomationError> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| AutomationError::CommandFailed(e.to_string()))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| AutomationError::CommandFailed(e.to_string()))?;
    }
    let status = child
        .wait()
        .map_err(|e| AutomationError::CommandFailed(e.to_string()))?;
    if !status.success() {
        return Err(AutomationError::CommandFailed(
            "pbcopy exited with non-zero status".to_string(),
        ));
    }
    Ok(())
}

fn read_clipboard_text() -> Result<String, AutomationError> {
    let output = Command::new("pbpaste")
        .output()
        .map_err(|e| AutomationError::CommandFailed(e.to_string()))?;
    if !output.status.success() {
        return Err(AutomationError::CommandFailed(
            "pbpaste exited with non-zero status".to_string(),
        ));
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(1000)
        .collect::<String>()
        .trim()
        .to_string();
    Ok(cleaned)
}

pub fn execute_action(
    action: &AgentAction,
    metrics: ScreenMetrics,
) -> Result<ActionExecutionOutput, AutomationError> {
    match action {
        AgentAction::OpenApp { app_name } | AgentAction::FocusApp { app_name } => {
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
            Ok(ActionExecutionOutput { note: None })
        }
        AgentAction::OpenUrl { url, app_name } => {
            let clean_url = validate_open_url(url)?;
            let status = if let Some(app) = app_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let clean_app = validate_app_name(app)?;
                Command::new("open")
                    .arg("-a")
                    .arg(&clean_app)
                    .arg(&clean_url)
                    .status()
                    .map_err(|e| AutomationError::CommandFailed(e.to_string()))?
            } else {
                Command::new("open")
                    .arg(&clean_url)
                    .status()
                    .map_err(|e| AutomationError::CommandFailed(e.to_string()))?
            };
            if !status.success() {
                return Err(AutomationError::CommandFailed(format!(
                    "failed to open URL \"{clean_url}\""
                )));
            }
            Ok(ActionExecutionOutput { note: None })
        }
        AgentAction::OpenPath { path } => {
            let resolved = validate_open_path(path)?;
            let status = Command::new("open")
                .arg(&resolved)
                .status()
                .map_err(|e| AutomationError::CommandFailed(e.to_string()))?;
            if !status.success() {
                return Err(AutomationError::CommandFailed(format!(
                    "failed to open path \"{}\"",
                    resolved.display()
                )));
            }
            Ok(ActionExecutionOutput { note: None })
        }
        AgentAction::ReadClipboard => {
            let text = read_clipboard_text()?;
            let note = if text.is_empty() {
                "Clipboard is empty".to_string()
            } else {
                format!("Clipboard: {text}")
            };
            Ok(ActionExecutionOutput { note: Some(note) })
        }
        AgentAction::Wait { ms } => {
            let clamped = (*ms).clamp(100, 5000);
            thread::sleep(Duration::from_millis(clamped));
            Ok(ActionExecutionOutput { note: None })
        }
        AgentAction::Done { .. } | AgentAction::Fail { .. } => {
            Ok(ActionExecutionOutput { note: None })
        }
        AgentAction::Click { .. }
        | AgentAction::Drag { .. }
        | AgentAction::TypeText { .. }
        | AgentAction::PasteText { .. }
        | AgentAction::PressKey { .. }
        | AgentAction::Scroll { .. } => {
            if !has_accessibility_permission() {
                return Err(AutomationError::AccessibilityPermissionDenied);
            }
            execute_hid_action(action, metrics)?;
            Ok(ActionExecutionOutput { note: None })
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

    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .or_else(|| CGEventSource::new(CGEventSourceStateID::HIDSystemState))
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
            CGEvent::set_flags(Some(&move_ev), CGEventFlags::empty());
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&move_ev));
            thread::sleep(Duration::from_millis(40));

            let clicks = if *double { 2 } else { 1 };
            for click_state in 1..=clicks {
                let down = CGEvent::new_mouse_event(Some(&source), down_type, point, cg_btn)
                    .ok_or(AutomationError::EventCreationFailed)?;
                CGEvent::set_flags(Some(&down), CGEventFlags::empty());
                CGEvent::set_integer_value_field(
                    Some(&down),
                    CGEventField::MouseEventClickState,
                    click_state,
                );
                let up = CGEvent::new_mouse_event(Some(&source), up_type, point, cg_btn)
                    .ok_or(AutomationError::EventCreationFailed)?;
                CGEvent::set_flags(Some(&up), CGEventFlags::empty());
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
        AgentAction::Drag {
            from_x,
            from_y,
            to_x,
            to_y,
        } => {
            let (sx, sy) = clamp_point(*from_x, *from_y, metrics);
            let (ex, ey) = clamp_point(*to_x, *to_y, metrics);
            let start_pt = CGPoint::new(sx, sy);

            let move_ev = CGEvent::new_mouse_event(
                Some(&source),
                CGEventType::MouseMoved,
                start_pt,
                CGMouseButton::Left,
            )
            .ok_or(AutomationError::EventCreationFailed)?;
            CGEvent::set_flags(Some(&move_ev), CGEventFlags::empty());
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&move_ev));
            thread::sleep(Duration::from_millis(35));

            let down = CGEvent::new_mouse_event(
                Some(&source),
                CGEventType::LeftMouseDown,
                start_pt,
                CGMouseButton::Left,
            )
            .ok_or(AutomationError::EventCreationFailed)?;
            CGEvent::set_flags(Some(&down), CGEventFlags::empty());
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down));
            thread::sleep(Duration::from_millis(25));

            let steps = 12;
            for step in 1..=steps {
                let t = f64::from(step) / f64::from(steps);
                let ix = sx + (ex - sx) * t;
                let iy = sy + (ey - sy) * t;
                let drag_ev = CGEvent::new_mouse_event(
                    Some(&source),
                    CGEventType::LeftMouseDragged,
                    CGPoint::new(ix, iy),
                    CGMouseButton::Left,
                )
                .ok_or(AutomationError::EventCreationFailed)?;
                CGEvent::set_flags(Some(&drag_ev), CGEventFlags::empty());
                CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&drag_ev));
                thread::sleep(Duration::from_millis(14));
            }

            let up = CGEvent::new_mouse_event(
                Some(&source),
                CGEventType::LeftMouseUp,
                CGPoint::new(ex, ey),
                CGMouseButton::Left,
            )
            .ok_or(AutomationError::EventCreationFailed)?;
            CGEvent::set_flags(Some(&up), CGEventFlags::empty());
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));
            Ok(())
        }
        AgentAction::TypeText {
            text,
            replace,
            submit,
        } => {
            if *replace {
                select_all_and_delete(&source)?;
            }

            let filtered: String = text
                .chars()
                .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                .take(500)
                .collect();

            for ch in filtered.chars() {
                if let Some((keycode, needs_shift)) = map_char_to_keycode_and_shift(ch) {
                    post_key_chord(
                        &source,
                        ParsedKeyChord {
                            keycode,
                            cmd: false,
                            shift: needs_shift,
                            alt: false,
                            ctrl: false,
                        },
                    )?;
                } else {
                    let mut utf16 = [0u16; 2];
                    let slice = ch.encode_utf16(&mut utf16);
                    let down = CGEvent::new_keyboard_event(Some(&source), 0, true)
                        .ok_or(AutomationError::EventCreationFailed)?;
                    let up = CGEvent::new_keyboard_event(Some(&source), 0, false)
                        .ok_or(AutomationError::EventCreationFailed)?;
                    CGEvent::set_flags(Some(&down), CGEventFlags::empty());
                    CGEvent::set_flags(Some(&up), CGEventFlags::empty());
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
                    thread::sleep(Duration::from_millis(12));
                    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));
                }
                thread::sleep(Duration::from_millis(10));
            }

            if *submit {
                thread::sleep(Duration::from_millis(30));
                post_key_chord(
                    &source,
                    ParsedKeyChord {
                        keycode: 0x24,
                        cmd: false,
                        shift: false,
                        alt: false,
                        ctrl: false,
                    },
                )?;
            }
            Ok(())
        }
        AgentAction::PasteText {
            text,
            replace,
            submit,
        } => {
            let filtered: String = text
                .chars()
                .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                .take(4000)
                .collect();
            write_clipboard_text(&filtered)?;
            thread::sleep(Duration::from_millis(30));

            if *replace {
                select_all_and_delete(&source)?;
            }

            post_key_chord(
                &source,
                ParsedKeyChord {
                    keycode: 0x09,
                    cmd: true,
                    shift: false,
                    alt: false,
                    ctrl: false,
                },
            )?;

            if *submit {
                thread::sleep(Duration::from_millis(50));
                post_key_chord(
                    &source,
                    ParsedKeyChord {
                        keycode: 0x24,
                        cmd: false,
                        shift: false,
                        alt: false,
                        ctrl: false,
                    },
                )?;
            }
            Ok(())
        }
        AgentAction::PressKey { key, modifiers } => {
            let chord = parse_key_chord(key, modifiers)?;
            post_key_chord(&source, chord)
        }
        AgentAction::Scroll {
            x,
            y,
            delta_y,
            delta_x,
        } => {
            let (cx, cy) = clamp_point(*x, *y, metrics);
            let point = CGPoint::new(cx, cy);
            let move_ev = CGEvent::new_mouse_event(
                Some(&source),
                CGEventType::MouseMoved,
                point,
                CGMouseButton::Left,
            )
            .ok_or(AutomationError::EventCreationFailed)?;
            CGEvent::set_flags(Some(&move_ev), CGEventFlags::empty());
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&move_ev));
            thread::sleep(Duration::from_millis(20));

            let clamped_y = (*delta_y).clamp(-20, 20);
            let clamped_x = (*delta_x).clamp(-20, 20);
            let scroll = CGEvent::new_scroll_wheel_event2(
                Some(&source),
                CGScrollEventUnit::Line,
                2,
                clamped_y,
                clamped_x,
                0,
            )
            .ok_or(AutomationError::EventCreationFailed)?;
            CGEvent::set_flags(Some(&scroll), CGEventFlags::empty());
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&scroll));
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(target_os = "macos")]
fn select_all_and_delete(
    source: &objc2_core_graphics::CGEventSource,
) -> Result<(), AutomationError> {
    post_key_chord(
        source,
        ParsedKeyChord {
            keycode: 0x00,
            cmd: true,
            shift: false,
            alt: false,
            ctrl: false,
        },
    )?;
    thread::sleep(Duration::from_millis(40));
    post_key_chord(
        source,
        ParsedKeyChord {
            keycode: 0x33,
            cmd: false,
            shift: false,
            alt: false,
            ctrl: false,
        },
    )?;
    thread::sleep(Duration::from_millis(40));
    Ok(())
}

#[cfg(target_os = "macos")]
fn post_key_chord(
    source: &objc2_core_graphics::CGEventSource,
    chord: ParsedKeyChord,
) -> Result<(), AutomationError> {
    use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventTapLocation};

    let mut mod_keys: Vec<(u16, CGEventFlags)> = Vec::with_capacity(4);
    if chord.cmd {
        mod_keys.push((0x37, CGEventFlags::MaskCommand));
    }
    if chord.ctrl {
        mod_keys.push((0x3B, CGEventFlags::MaskControl));
    }
    if chord.alt {
        mod_keys.push((0x3A, CGEventFlags::MaskAlternate));
    }
    if chord.shift {
        mod_keys.push((0x38, CGEventFlags::MaskShift));
    }

    let mut active_flags = CGEventFlags::empty();
    for (mk, flag) in &mod_keys {
        active_flags |= *flag;
        let m_down = CGEvent::new_keyboard_event(Some(source), *mk, true)
            .ok_or(AutomationError::EventCreationFailed)?;
        CGEvent::set_flags(Some(&m_down), active_flags);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&m_down));
        thread::sleep(Duration::from_millis(10));
    }

    let down = CGEvent::new_keyboard_event(Some(source), chord.keycode, true)
        .ok_or(AutomationError::EventCreationFailed)?;
    let up = CGEvent::new_keyboard_event(Some(source), chord.keycode, false)
        .ok_or(AutomationError::EventCreationFailed)?;
    CGEvent::set_flags(Some(&down), active_flags);
    CGEvent::set_flags(Some(&up), active_flags);
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down));
    thread::sleep(Duration::from_millis(18));
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));

    for (mk, flag) in mod_keys.iter().rev() {
        thread::sleep(Duration::from_millis(10));
        active_flags.remove(*flag);
        let m_up = CGEvent::new_keyboard_event(Some(source), *mk, false)
            .ok_or(AutomationError::EventCreationFailed)?;
        CGEvent::set_flags(Some(&m_up), active_flags);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&m_up));
    }

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
        assert_eq!(map_key_to_keycode("arrow_down"), Some(0x7D));
        assert_eq!(map_key_to_keycode("forward_delete"), Some(0x75));
        assert_eq!(map_key_to_keycode("clear"), Some(0x47));
        assert_eq!(map_key_to_keycode("f15"), Some(0x71));
        assert_eq!(map_key_to_keycode("unknown_key"), None);
    }

    #[test]
    fn maps_ascii_characters_to_virtual_keycodes_and_shift() {
        assert_eq!(map_char_to_keycode_and_shift('h'), Some((0x04, false)));
        assert_eq!(map_char_to_keycode_and_shift('H'), Some((0x04, true)));
        assert_eq!(map_char_to_keycode_and_shift(':'), Some((0x29, true)));
        assert_eq!(map_char_to_keycode_and_shift('/'), Some((0x2C, false)));
        assert_eq!(map_char_to_keycode_and_shift('.'), Some((0x2F, false)));
        assert_eq!(map_char_to_keycode_and_shift('-'), Some((0x1B, false)));
        assert_eq!(map_char_to_keycode_and_shift('é'), None);
        for ch in "https://github.com/trending".chars() {
            assert!(map_char_to_keycode_and_shift(ch).is_some());
        }
    }

    #[test]
    fn parses_inline_and_modifier_key_chords() {
        assert_eq!(
            parse_key_chord("cmd+a", &[]).unwrap(),
            ParsedKeyChord {
                keycode: 0x00,
                cmd: true,
                shift: false,
                alt: false,
                ctrl: false,
            }
        );
        assert_eq!(
            parse_key_chord("Command+Shift+T", &[]).unwrap(),
            ParsedKeyChord {
                keycode: 0x11,
                cmd: true,
                shift: true,
                alt: false,
                ctrl: false,
            }
        );
        assert_eq!(
            parse_key_chord("+", &[]).unwrap(),
            ParsedKeyChord {
                keycode: 0x18,
                cmd: false,
                shift: true,
                alt: false,
                ctrl: false,
            }
        );
        assert_eq!(
            parse_key_chord("cmd++", &[]).unwrap(),
            ParsedKeyChord {
                keycode: 0x18,
                cmd: true,
                shift: true,
                alt: false,
                ctrl: false,
            }
        );
        assert!(parse_key_chord("bad_key", &[]).is_err());
        assert!(parse_key_chord("a", &["unknown_mod".to_string()]).is_err());
    }

    #[test]
    fn clamps_points_within_display_bounds() {
        let m = sample_metrics();
        assert_eq!(clamp_point(0.0, 0.0, m), (102.0, 52.0));
        assert_eq!(clamp_point(5000.0, 5000.0, m), (1538.0, 948.0));
        assert_eq!(clamp_point(500.0, 400.0, m), (500.0, 400.0));
    }

    #[test]
    fn validates_urls_paths_and_app_names_strictly() {
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

        assert!(validate_open_path("/tmp").is_ok());
        assert!(validate_open_path("-rf").is_err());
        assert!(validate_open_path("/nonexistent_path_xyz_12345").is_err());
    }

    #[test]
    fn describes_and_assesses_expanded_toolbelt_actions() {
        let drag = AgentAction::Drag {
            from_x: 100.0,
            from_y: 200.0,
            to_x: 300.0,
            to_y: 400.0,
        };
        assert_eq!(action_type_label(&drag), "drag");
        assert_eq!(describe_action(&drag), "Drag (100, 200) -> (300, 400)");

        let open_in_safari = AgentAction::OpenUrl {
            url: "https://github.com/trending".to_string(),
            app_name: Some("Safari".to_string()),
        };
        assert_eq!(
            describe_action(&open_in_safari),
            "Open https://github.com/trending in Safari"
        );

        let paste_submit = AgentAction::PasteText {
            text: "Confirm payment".to_string(),
            replace: true,
            submit: true,
        };
        assert_eq!(action_type_label(&paste_submit), "paste_text");
        assert!(assess_action_risk(&paste_submit, "Send message", None, false).is_some());

        let safe_paste = AgentAction::PasteText {
            text: "- Item 1\n- Item 2".to_string(),
            replace: false,
            submit: false,
        };
        assert!(assess_action_risk(&safe_paste, "Pasting sprint checklist", None, false).is_none());
    }
}
