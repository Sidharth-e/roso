use crate::automation::has_accessibility_permission;
use crate::capture::{AnnotationBox, ScreenMetrics};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use thiserror::Error;

const MAX_TOTAL_TARGETS: usize = 150;
const MAX_AX_TARGETS: usize = 90;
const MAX_RAW_OCR_ITEMS: usize = 250;

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("failed to construct CoreGraphics image from RGBA frame")]
    CgImageCreation,
    #[error("macOS Vision OCR request failed: {0}")]
    VisionExecution(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedBoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScreenBoundingBox {
    pub x: f64,
    pub y_top_left: f64,
    pub width: f64,
    pub height: f64,
    pub center_x: f64,
    pub center_y: f64,
}

#[derive(Debug, Clone, Default)]
pub struct OcrExtractionResult {
    pub formatted_lines: Vec<String>,
    pub bbox_by_id: HashMap<u32, NormalizedBoundingBox>,
}

#[derive(Debug, Clone, Default)]
pub struct FocusedElementInfo {
    pub role: Option<String>,
    pub value_preview: Option<String>,
    pub is_secure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScreenTarget {
    pub id: u32,
    pub role: String,
    pub label: String,
    pub is_interactive: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub center_x: f64,
    pub center_y: f64,
    pub x_norm: f64,
    pub y_norm: f64,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub focused: Option<bool>,
    #[serde(default)]
    pub selected: Option<bool>,
    #[serde(default)]
    pub value_preview: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ScreenTargetMap {
    pub formatted_lines: Vec<String>,
    pub targets_by_id: HashMap<u32, ScreenTarget>,
    pub annotation_boxes: Vec<AnnotationBox>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawTarget {
    pub role: String,
    pub label: String,
    pub is_interactive: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub center_x: f64,
    pub center_y: f64,
    pub enabled: Option<bool>,
    pub focused: Option<bool>,
    pub selected: Option<bool>,
    pub value_preview: Option<String>,
}

pub fn vision_bbox_to_screen_rect(
    bbox: NormalizedBoundingBox,
    screen_width: f64,
    screen_height: f64,
) -> ScreenBoundingBox {
    let clamped_x = bbox.x.clamp(0.0, 1.0);
    let clamped_y = bbox.y.clamp(0.0, 1.0);
    let clamped_w = bbox.width.clamp(0.0, 1.0 - clamped_x);
    let clamped_h = bbox.height.clamp(0.0, 1.0 - clamped_y);

    let x = clamped_x * screen_width;
    let y_top_left = (1.0 - clamped_y - clamped_h) * screen_height;
    let width = clamped_w * screen_width;
    let height = clamped_h * screen_height;

    let raw_center_x = x + width * 0.5;
    let raw_center_y = y_top_left + height * 0.5;
    let margin = 2.0;
    let center_x = raw_center_x.clamp(margin, (screen_width - margin).max(margin));
    let center_y = raw_center_y.clamp(margin, (screen_height - margin).max(margin));

    ScreenBoundingBox {
        x,
        y_top_left,
        width,
        height,
        center_x,
        center_y,
    }
}

pub fn extract_ocr_targets(rgba: &RgbaImage, metrics: ScreenMetrics) -> Vec<RawTarget> {
    let raw_ocr = extract_raw_ocr_from_rgba(rgba).unwrap_or_default();

    raw_ocr
        .into_iter()
        .map(|(text, bbox)| {
            let rect = vision_bbox_to_screen_rect(
                bbox,
                metrics.logical_width,
                metrics.logical_height,
            );
            RawTarget {
                role: "Text".to_string(),
                label: text,
                is_interactive: false,
                x: rect.x,
                y: rect.y_top_left,
                width: rect.width,
                height: rect.height,
                center_x: rect.center_x,
                center_y: rect.center_y,
                enabled: None,
                focused: None,
                selected: None,
                value_preview: None,
            }
        })
        .collect()
}

fn is_meaningful_ocr_label(label: &str) -> bool {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return false;
    }
    let char_count = trimmed.chars().count();
    if char_count >= 2 {
        return true;
    }
    trimmed
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '*' | '/' | '=' | '%'))
}

pub fn merge_targets(
    ax_targets: Vec<RawTarget>,
    ocr_targets: Vec<RawTarget>,
    metrics: ScreenMetrics,
) -> ScreenTargetMap {
    let safe_w = metrics.logical_width.max(1.0);
    let safe_h = metrics.logical_height.max(1.0);

    let mut combined: Vec<RawTarget> = Vec::with_capacity(MAX_TOTAL_TARGETS);
    for ax in ax_targets.into_iter().take(MAX_AX_TARGETS) {
        combined.push(ax);
    }

    let mut unique_ocr: Vec<RawTarget> = Vec::new();
    for ocr_item in ocr_targets {
        if !is_meaningful_ocr_label(&ocr_item.label) {
            continue;
        }
        let ocr_lower = ocr_item.label.to_ascii_lowercase();
        let duplicated = combined.iter().any(|ax| {
            let close_center = (ax.center_x - ocr_item.center_x).abs() <= 16.0
                && (ax.center_y - ocr_item.center_y).abs() <= 16.0;
            if !close_center {
                return false;
            }
            let ax_lower = ax.label.to_ascii_lowercase();
            let similar_label = !ax_lower.is_empty()
                && !ocr_lower.is_empty()
                && (ax_lower.contains(&ocr_lower) || ocr_lower.contains(&ax_lower));
            let overlaps_inside = ocr_item.center_x >= ax.x
                && ocr_item.center_x <= ax.x + ax.width
                && ocr_item.center_y >= ax.y
                && ocr_item.center_y <= ax.y + ax.height;
            similar_label || overlaps_inside
        });
        if !duplicated {
            unique_ocr.push(ocr_item);
        }
    }

    let ocr_budget = MAX_TOTAL_TARGETS.saturating_sub(combined.len());
    if unique_ocr.len() <= ocr_budget {
        combined.extend(unique_ocr);
    } else if ocr_budget > 0 {
        let mut buckets: Vec<VecDeque<RawTarget>> = (0..24).map(|_| VecDeque::new()).collect();
        for item in unique_ocr {
            let xn = (item.center_x / safe_w).clamp(0.0, 0.9999);
            let yn = (item.center_y / safe_h).clamp(0.0, 0.9999);
            let col = (xn * 4.0) as usize;
            let row = (yn * 6.0) as usize;
            buckets[row * 4 + col].push_back(item);
        }
        let mut remaining = ocr_budget;
        while remaining > 0 {
            let mut progressed = false;
            for bucket in &mut buckets {
                if remaining == 0 {
                    break;
                }
                if let Some(item) = bucket.pop_front() {
                    combined.push(item);
                    remaining -= 1;
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
    }

    combined.sort_by(|a, b| {
        a.y.partial_cmp(&b.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut formatted_lines = Vec::with_capacity(combined.len());
    let mut targets_by_id = HashMap::with_capacity(combined.len());
    let mut annotation_boxes = Vec::with_capacity(combined.len());

    for (idx, item) in combined.into_iter().enumerate() {
        let id = (idx + 1) as u32;
        let x_norm = (item.center_x / safe_w).clamp(0.0, 1.0);
        let y_norm = (item.center_y / safe_h).clamp(0.0, 1.0);
        let w_norm = (item.width / safe_w).clamp(0.0, 1.0);
        let h_norm = (item.height / safe_h).clamp(0.0, 1.0);

        let mut flags = String::new();
        if item.enabled == Some(false) {
            flags.push_str(" [disabled]");
        }
        if item.focused == Some(true) {
            flags.push_str(" [focused]");
        }
        if item.selected == Some(true) {
            flags.push_str(" [selected]");
        }
        if let Some(ref val) = item.value_preview {
            flags.push_str(&format!(" [val=\"{val}\"]"));
        }

        let line = format!(
            "[{id}] ({}) \"{}\" @(x={x_norm:.3}, y={y_norm:.3}, w={w_norm:.3}, h={h_norm:.3}){flags}",
            item.role, item.label
        );
        formatted_lines.push(line);

        annotation_boxes.push(AnnotationBox {
            id,
            x_norm: (item.x / safe_w).clamp(0.0, 1.0),
            y_norm: (item.y / safe_h).clamp(0.0, 1.0),
            w_norm,
            h_norm,
            is_interactive: item.is_interactive,
        });

        targets_by_id.insert(
            id,
            ScreenTarget {
                id,
                role: item.role,
                label: item.label,
                is_interactive: item.is_interactive,
                x: item.x,
                y: item.y,
                width: item.width,
                height: item.height,
                center_x: item.center_x,
                center_y: item.center_y,
                x_norm,
                y_norm,
                enabled: item.enabled,
                focused: item.focused,
                selected: item.selected,
                value_preview: item.value_preview,
            },
        );
    }

    ScreenTargetMap {
        formatted_lines,
        targets_by_id,
        annotation_boxes,
    }
}

/// Password fields report role AXTextField with subrole AXSecureTextField; a few custom
/// controls report it as the role itself.
fn is_secure_role(role: Option<&str>, subrole: Option<&str>) -> bool {
    role == Some("AXSecureTextField") || subrole == Some("AXSecureTextField")
}

#[cfg(target_os = "macos")]
mod macos_ax_tree {
    use super::*;
    use std::ffi::c_void;
    use std::time::{Duration, Instant};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct AxCgPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct AxCgSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CfRange {
        location: isize,
        length: isize,
    }

    struct CfGuard(*const c_void);

    impl CfGuard {
        fn new(ptr: *const c_void) -> Option<Self> {
            if ptr.is_null() {
                None
            } else {
                Some(Self(ptr))
            }
        }

        fn as_ptr(&self) -> *const c_void {
            self.0
        }
    }

    impl Drop for CfGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_AX_VALUE_TYPE_CG_POINT: u32 = 1;
    const K_AX_VALUE_TYPE_CG_SIZE: u32 = 2;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
        fn AXUIElementCreateSystemWide() -> *const c_void;
        fn AXUIElementSetMessagingTimeout(element: *const c_void, timeout_in_seconds: f32) -> i32;
        fn AXUIElementSetAttributeValue(
            element: *const c_void,
            attribute: *const c_void,
            value: *const c_void,
        ) -> i32;
        fn AXUIElementCopyAttributeValue(
            element: *const c_void,
            attribute: *const c_void,
            value: *mut *const c_void,
        ) -> i32;
        fn AXUIElementCopyMultipleAttributeValues(
            element: *const c_void,
            attributes: *const c_void,
            options: u32,
            values: *mut *const c_void,
        ) -> i32;
        fn AXUIElementCopyElementAtPosition(
            application: *const c_void,
            x: f32,
            y: f32,
            element: *mut *const c_void,
        ) -> i32;
        fn AXUIElementPerformAction(element: *const c_void, action: *const c_void) -> i32;
        fn AXValueGetValue(value: *const c_void, value_type: u32, value_ptr: *mut c_void) -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: *const c_void;
        static kCFTypeArrayCallBacks: u8;
        fn CFStringCreateWithBytes(
            alloc: *const c_void,
            bytes: *const u8,
            num_bytes: isize,
            encoding: u32,
            is_external_representation: u8,
        ) -> *const c_void;
        fn CFStringGetTypeID() -> usize;
        fn CFStringGetLength(the_string: *const c_void) -> isize;
        fn CFStringGetCharacters(
            the_string: *const c_void,
            range: CfRange,
            buffer: *mut u16,
        );
        fn CFBooleanGetTypeID() -> usize;
        fn CFBooleanGetValue(boolean: *const c_void) -> u8;
        fn CFArrayGetTypeID() -> usize;
        fn CFGetTypeID(cf: *const c_void) -> usize;
        fn CFArrayCreate(
            allocator: *const c_void,
            values: *const *const c_void,
            num_values: isize,
            callbacks: *const u8,
        ) -> *const c_void;
        fn CFArrayGetCount(the_array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(the_array: *const c_void, idx: isize) -> *const c_void;
        fn CFRetain(cf: *const c_void) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    fn make_cf_string(s: &str) -> Option<CfGuard> {
        let ptr = unsafe {
            CFStringCreateWithBytes(
                std::ptr::null(),
                s.as_ptr(),
                s.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
                0,
            )
        };
        CfGuard::new(ptr)
    }

    struct AxAttrKeys {
        focused_window: CfGuard,
        main_window: CfGuard,
        focused_ui_element: CfGuard,
        menu_bar: CfGuard,
        manual_accessibility: CfGuard,
        role: CfGuard,
        subrole: CfGuard,
        value: CfGuard,
        children: CfGuard,
        press_action: CfGuard,
        batch_attrs: CfGuard,
        _batch_guards: Vec<CfGuard>,
    }

    impl AxAttrKeys {
        fn new() -> Option<Self> {
            let role = make_cf_string("AXRole")?;
            let position = make_cf_string("AXPosition")?;
            let size = make_cf_string("AXSize")?;
            let title = make_cf_string("AXTitle")?;
            let description = make_cf_string("AXDescription")?;
            let placeholder = make_cf_string("AXPlaceholderValue")?;
            let help = make_cf_string("AXHelp")?;
            let value = make_cf_string("AXValue")?;
            let enabled = make_cf_string("AXEnabled")?;
            let focused = make_cf_string("AXFocused")?;
            let selected = make_cf_string("AXSelected")?;
            let subrole = make_cf_string("AXSubrole")?;
            let ptrs = [
                role.as_ptr(),
                position.as_ptr(),
                size.as_ptr(),
                title.as_ptr(),
                description.as_ptr(),
                placeholder.as_ptr(),
                help.as_ptr(),
                value.as_ptr(),
                enabled.as_ptr(),
                focused.as_ptr(),
                selected.as_ptr(),
                subrole.as_ptr(),
            ];
            let batch_attrs = CfGuard::new(unsafe {
                CFArrayCreate(
                    std::ptr::null(),
                    ptrs.as_ptr(),
                    ptrs.len() as isize,
                    &kCFTypeArrayCallBacks,
                )
            })?;

            let role_dup = make_cf_string("AXRole")?;
            let value_dup = make_cf_string("AXValue")?;
            let subrole_dup = make_cf_string("AXSubrole")?;

            Some(Self {
                focused_window: make_cf_string("AXFocusedWindow")?,
                main_window: make_cf_string("AXMainWindow")?,
                focused_ui_element: make_cf_string("AXFocusedUIElement")?,
                menu_bar: make_cf_string("AXMenuBar")?,
                manual_accessibility: make_cf_string("AXManualAccessibility")?,
                role: role_dup,
                subrole: subrole_dup,
                value: value_dup,
                children: make_cf_string("AXChildren")?,
                press_action: make_cf_string("AXPress")?,
                batch_attrs,
                _batch_guards: vec![
                    role,
                    position,
                    size,
                    title,
                    description,
                    placeholder,
                    help,
                    value,
                    enabled,
                    focused,
                    selected,
                    subrole,
                ],
            })
        }
    }

    fn copy_attr_key(element: *const c_void, attr_key: &CfGuard) -> Option<CfGuard> {
        let mut out: *const c_void = std::ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(element, attr_key.as_ptr(), &mut out) };
        if err == 0 {
            CfGuard::new(out)
        } else {
            None
        }
    }

    fn cf_string_to_rust(ptr: *const c_void, max_chars: usize) -> Option<String> {
        if ptr.is_null() {
            return None;
        }
        unsafe {
            if CFGetTypeID(ptr) != CFStringGetTypeID() {
                return None;
            }
            let total_len = CFStringGetLength(ptr);
            if total_len <= 0 {
                return None;
            }
            let take_len = total_len.min(256);
            let mut buf = vec![0u16; take_len as usize];
            CFStringGetCharacters(
                ptr,
                CfRange {
                    location: 0,
                    length: take_len,
                },
                buf.as_mut_ptr(),
            );
            let decoded = String::from_utf16_lossy(&buf);
            let clean = decoded
                .chars()
                .filter(|c| !c.is_control())
                .take(max_chars)
                .collect::<String>()
                .trim()
                .to_string();
            if clean.is_empty() {
                None
            } else {
                Some(clean)
            }
        }
    }

    fn cf_bool_to_rust(ptr: *const c_void) -> Option<bool> {
        if ptr.is_null() {
            return None;
        }
        unsafe {
            if CFGetTypeID(ptr) == CFBooleanGetTypeID() {
                Some(CFBooleanGetValue(ptr) != 0)
            } else {
                None
            }
        }
    }

    fn copy_children_key(element: *const c_void, children_key: &CfGuard) -> Vec<CfGuard> {
        let Some(arr) = copy_attr_key(element, children_key) else {
            return Vec::new();
        };
        unsafe {
            if CFGetTypeID(arr.as_ptr()) != CFArrayGetTypeID() {
                return Vec::new();
            }
            let count = CFArrayGetCount(arr.as_ptr()).min(60);
            let mut children = Vec::with_capacity(count.max(0) as usize);
            for i in 0..count {
                let raw_child = CFArrayGetValueAtIndex(arr.as_ptr(), i);
                if !raw_child.is_null() {
                    let retained = CFRetain(raw_child);
                    if let Some(guard) = CfGuard::new(retained) {
                        children.push(guard);
                    }
                }
            }
            children
        }
    }

    fn copy_batch_attributes(element: *const c_void, keys: &AxAttrKeys) -> Option<BatchAttrs> {
        let mut out_arr: *const c_void = std::ptr::null();
        let err = unsafe {
            AXUIElementCopyMultipleAttributeValues(
                element,
                keys.batch_attrs.as_ptr(),
                0,
                &mut out_arr,
            )
        };
        if err != 0 {
            return None;
        }
        let arr = CfGuard::new(out_arr)?;
        unsafe {
            if CFGetTypeID(arr.as_ptr()) != CFArrayGetTypeID() {
                return None;
            }
            if CFArrayGetCount(arr.as_ptr()) < 12 {
                return None;
            }
            let at = |idx: isize| CFArrayGetValueAtIndex(arr.as_ptr(), idx);
            let role = cf_string_to_rust(at(0), 64);

            let mut pt = AxCgPoint { x: 0.0, y: 0.0 };
            let mut sz = AxCgSize {
                width: 0.0,
                height: 0.0,
            };
            let pos_ok = !at(1).is_null()
                && AXValueGetValue(
                    at(1),
                    K_AX_VALUE_TYPE_CG_POINT,
                    (&mut pt as *mut AxCgPoint).cast(),
                ) != 0;
            let sz_ok = !at(2).is_null()
                && AXValueGetValue(
                    at(2),
                    K_AX_VALUE_TYPE_CG_SIZE,
                    (&mut sz as *mut AxCgSize).cast(),
                ) != 0;

            // Password fields report role AXTextField with subrole AXSecureTextField.
            let is_secure = is_secure_role(role.as_deref(), cf_string_to_rust(at(11), 64).as_deref());
            let value_str = if is_secure {
                None
            } else {
                cf_string_to_rust(at(7), 80)
            };

            Some(BatchAttrs {
                role,
                pos_size: if pos_ok && sz_ok {
                    Some((pt, sz))
                } else {
                    None
                },
                title: cf_string_to_rust(at(3), 100),
                description: cf_string_to_rust(at(4), 100),
                placeholder: cf_string_to_rust(at(5), 100),
                help: cf_string_to_rust(at(6), 100),
                value: value_str,
                enabled: cf_bool_to_rust(at(8)),
                focused: cf_bool_to_rust(at(9)),
                selected: cf_bool_to_rust(at(10)),
            })
        }
    }

    struct BatchAttrs {
        role: Option<String>,
        pos_size: Option<(AxCgPoint, AxCgSize)>,
        title: Option<String>,
        description: Option<String>,
        placeholder: Option<String>,
        help: Option<String>,
        value: Option<String>,
        enabled: Option<bool>,
        focused: Option<bool>,
        selected: Option<bool>,
    }

    fn map_interactive_role(role: &str) -> Option<&'static str> {
        match role {
            "AXButton" => Some("Button"),
            "AXTextField" => Some("TextField"),
            "AXTextArea" => Some("TextArea"),
            "AXSearchField" => Some("SearchField"),
            "AXComboBox" => Some("ComboBox"),
            "AXLink" => Some("Link"),
            "AXTab" => Some("Tab"),
            "AXRadioButton" => Some("RadioButton"),
            "AXCheckBox" => Some("CheckBox"),
            "AXPopUpButton" => Some("PopUpButton"),
            "AXMenuButton" => Some("MenuButton"),
            "AXMenuItem" => Some("MenuItem"),
            "AXMenuBarItem" => Some("MenuBarItem"),
            "AXDisclosureTriangle" => Some("Toggle"),
            "AXCell" => Some("Cell"),
            _ => None,
        }
    }

    pub fn focused_element_info() -> FocusedElementInfo {
        if !has_accessibility_permission() {
            return FocusedElementInfo::default();
        }
        let Some(keys) = AxAttrKeys::new() else {
            return FocusedElementInfo::default();
        };
        let Some(sys) = CfGuard::new(unsafe { AXUIElementCreateSystemWide() }) else {
            return FocusedElementInfo::default();
        };
        unsafe {
            AXUIElementSetMessagingTimeout(sys.as_ptr(), 0.1);
        }
        let Some(focused_el) = copy_attr_key(sys.as_ptr(), &keys.focused_ui_element) else {
            return FocusedElementInfo::default();
        };
        let Some(role_val) = copy_attr_key(focused_el.as_ptr(), &keys.role) else {
            return FocusedElementInfo::default();
        };
        let role = cf_string_to_rust(role_val.as_ptr(), 64);
        let subrole = copy_attr_key(focused_el.as_ptr(), &keys.subrole)
            .and_then(|v| cf_string_to_rust(v.as_ptr(), 64));
        let is_secure = is_secure_role(role.as_deref(), subrole.as_deref());
        let value_preview = if is_secure {
            None
        } else {
            copy_attr_key(focused_el.as_ptr(), &keys.value)
                .and_then(|v| cf_string_to_rust(v.as_ptr(), 80))
        };
        FocusedElementInfo {
            role,
            value_preview,
            is_secure,
        }
    }

    pub fn try_ax_press_at_position(
        pid: u64,
        screen_x: f64,
        screen_y: f64,
        expected_label: Option<&str>,
    ) -> bool {
        if pid == 0 || !has_accessibility_permission() {
            return false;
        }
        let Ok(pid_i32) = i32::try_from(pid) else {
            return false;
        };
        let Some(keys) = AxAttrKeys::new() else {
            return false;
        };
        let Some(app_el) = CfGuard::new(unsafe { AXUIElementCreateApplication(pid_i32) }) else {
            return false;
        };
        unsafe {
            AXUIElementSetMessagingTimeout(app_el.as_ptr(), 0.1);
        }
        let mut hit_raw: *const c_void = std::ptr::null();
        let hit_err = unsafe {
            AXUIElementCopyElementAtPosition(
                app_el.as_ptr(),
                screen_x as f32,
                screen_y as f32,
                &mut hit_raw,
            )
        };
        if hit_err != 0 {
            return false;
        }
        let Some(hit_el) = CfGuard::new(hit_raw) else {
            return false;
        };

        let Some(batch) = copy_batch_attributes(hit_el.as_ptr(), &keys) else {
            return false;
        };
        let Some(ref ax_role) = batch.role else {
            return false;
        };
        if map_interactive_role(ax_role).is_none() {
            return false;
        }
        if matches!(
            ax_role.as_str(),
            "AXTextField" | "AXTextArea" | "AXSearchField" | "AXComboBox"
        ) {
            return false;
        }

        if let Some(expected) = expected_label.map(str::trim).filter(|s| !s.is_empty()) {
            let expected_lower = expected.to_ascii_lowercase();
            let actual_label = batch
                .title
                .or(batch.description)
                .or(batch.placeholder)
                .or(batch.help)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !actual_label.is_empty()
                && !actual_label.contains(&expected_lower)
                && !expected_lower.contains(&actual_label)
            {
                return false;
            }
        }

        unsafe { AXUIElementPerformAction(hit_el.as_ptr(), keys.press_action.as_ptr()) == 0 }
    }

    pub fn extract(pid: u64, metrics: ScreenMetrics) -> Vec<RawTarget> {
        if pid == 0 || !has_accessibility_permission() {
            return Vec::new();
        }
        let Ok(pid_i32) = i32::try_from(pid) else {
            return Vec::new();
        };
        let Some(keys) = AxAttrKeys::new() else {
            return Vec::new();
        };
        let Some(app_el) = CfGuard::new(unsafe { AXUIElementCreateApplication(pid_i32) }) else {
            return Vec::new();
        };
        unsafe {
            AXUIElementSetMessagingTimeout(app_el.as_ptr(), 0.1);
            // Only AXManualAccessibility: Electron/Chromium honour it to expose web content,
            // and other apps ignore it. AXEnhancedUserInterface is deliberately not set because
            // it persists for the app's lifetime and breaks window managers and animations.
            let _ = AXUIElementSetAttributeValue(
                app_el.as_ptr(),
                keys.manual_accessibility.as_ptr(),
                kCFBooleanTrue,
            );
        }

        let mut queue: VecDeque<(CfGuard, usize, bool)> = VecDeque::new();
        if let Some(win) = copy_attr_key(app_el.as_ptr(), &keys.focused_window)
            .or_else(|| copy_attr_key(app_el.as_ptr(), &keys.main_window))
        {
            queue.push_back((win, 0, false));
        }
        if let Some(menubar) = copy_attr_key(app_el.as_ptr(), &keys.menu_bar) {
            for item in copy_children_key(menubar.as_ptr(), &keys.children) {
                queue.push_back((item, 1, true));
            }
        }

        let mut results: Vec<RawTarget> = Vec::new();
        let mut visited: usize = 0;
        let start = Instant::now();
        const MAX_VISITED: usize = 400;
        const MAX_DEPTH: usize = 16;
        const AX_TIME_BUDGET: Duration = Duration::from_millis(300);

        while let Some((node, depth, is_menubar_item)) = queue.pop_front() {
            if visited >= MAX_VISITED || results.len() >= MAX_AX_TARGETS {
                break;
            }
            if visited % 4 == 0 && start.elapsed() >= AX_TIME_BUDGET {
                break;
            }
            visited += 1;

            if let Some(batch) = copy_batch_attributes(node.as_ptr(), &keys) {
                if let Some(ref ax_role) = batch.role {
                    if let Some(clean_role) = map_interactive_role(ax_role) {
                        if let Some((pt, sz)) = batch.pos_size {
                            let rel_x = pt.x - metrics.origin_x;
                            let rel_y = pt.y - metrics.origin_y;
                            let cx = rel_x + sz.width * 0.5;
                            let cy = rel_y + sz.height * 0.5;
                            let in_bounds = cx >= 2.0
                                && cy >= 2.0
                                && cx <= metrics.logical_width - 2.0
                                && cy <= metrics.logical_height - 2.0
                                && sz.width >= 6.0
                                && sz.height >= 6.0
                                && sz.width <= metrics.logical_width * 0.95
                                && sz.height <= metrics.logical_height * 0.95;

                            if in_bounds {
                                let label = batch
                                    .title
                                    .or(batch.description)
                                    .or(batch.placeholder)
                                    .or(batch.help)
                                    .or_else(|| batch.value.clone())
                                    .unwrap_or_else(|| clean_role.to_string());

                                let value_preview = batch
                                    .value
                                    .filter(|v| !v.is_empty() && v != &label)
                                    .map(|v| v.chars().take(40).collect());

                                results.push(RawTarget {
                                    role: clean_role.to_string(),
                                    label,
                                    is_interactive: true,
                                    x: rel_x.max(0.0),
                                    y: rel_y.max(0.0),
                                    width: sz.width,
                                    height: sz.height,
                                    center_x: cx,
                                    center_y: cy,
                                    enabled: batch.enabled,
                                    focused: batch.focused,
                                    selected: batch.selected,
                                    value_preview,
                                });
                            }
                        }
                    }
                }
            }

            if !is_menubar_item && depth < MAX_DEPTH {
                let mut cell_count = 0usize;
                for child in copy_children_key(node.as_ptr(), &keys.children) {
                    if let Some(role_val) = copy_attr_key(child.as_ptr(), &keys.role) {
                        if cf_string_to_rust(role_val.as_ptr(), 32).as_deref() == Some("AXCell") {
                            cell_count += 1;
                            if cell_count > 15 {
                                continue;
                            }
                        }
                    }
                    queue.push_back((child, depth + 1, false));
                }
            }
        }

        results
    }
}

pub fn extract_ax_elements(pid: u64, metrics: ScreenMetrics) -> Vec<RawTarget> {
    #[cfg(target_os = "macos")]
    {
        macos_ax_tree::extract(pid, metrics)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pid, metrics);
        Vec::new()
    }
}

pub fn focused_element_info() -> FocusedElementInfo {
    #[cfg(target_os = "macos")]
    {
        macos_ax_tree::focused_element_info()
    }
    #[cfg(not(target_os = "macos"))]
    {
        FocusedElementInfo::default()
    }
}

pub fn try_ax_press_at_position(
    pid: u64,
    screen_x: f64,
    screen_y: f64,
    expected_label: Option<&str>,
) -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_ax_tree::try_ax_press_at_position(pid, screen_x, screen_y, expected_label)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pid, screen_x, screen_y, expected_label);
        false
    }
}

#[cfg(target_os = "macos")]
fn extract_raw_ocr_from_rgba(
    rgba: &RgbaImage,
) -> Result<Vec<(String, NormalizedBoundingBox)>, OcrError> {
    use objc2::rc::autoreleasepool;
    use objc2::AnyThread;
    use objc2_core_foundation::CFData;
    use objc2_core_graphics::{
        CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
        CGImageAlphaInfo, CGImageByteOrderInfo,
    };
    use objc2_foundation::{NSArray, NSDictionary};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedTextObservation, VNRequest,
        VNRequestTextRecognitionLevel,
    };

    let scaled_storage = crate::capture::downscale_rgba_for_ocr(rgba, 1440);
    let ocr_rgba = scaled_storage.as_ref().unwrap_or(rgba);

    let (width, height) = ocr_rgba.dimensions();
    if width == 0 || height == 0 {
        return Ok(Vec::new());
    }

    autoreleasepool(|_| {
        let raw_bytes = ocr_rgba.as_raw();
        let cf_data = CFData::from_bytes(raw_bytes);
        let provider =
            CGDataProvider::with_cf_data(Some(&cf_data)).ok_or(OcrError::CgImageCreation)?;
        let color_space = CGColorSpace::new_device_rgb().ok_or(OcrError::CgImageCreation)?;
        let bitmap_info = CGBitmapInfo(
            CGImageAlphaInfo::Last.0 | CGImageByteOrderInfo::Order32Big.0,
        );

        let cg_image = unsafe {
            CGImage::new(
                width as usize,
                height as usize,
                8,
                32,
                (width as usize) * 4,
                Some(&color_space),
                bitmap_info,
                Some(&provider),
                std::ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }
        .ok_or(OcrError::CgImageCreation)?;

        unsafe {
            let options = NSDictionary::new();
            let handler = VNImageRequestHandler::initWithCGImage_options(
                VNImageRequestHandler::alloc(),
                &cg_image,
                &options,
            );

            let request = VNRecognizeTextRequest::new();
            request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
            request.setUsesLanguageCorrection(true);
            request.setAutomaticallyDetectsLanguage(true);

            let vn_request_ref: &VNRequest = request.as_ref();
            let requests = NSArray::from_slice(&[vn_request_ref]);

            let active_request = if handler.performRequests_error(&requests).is_ok() {
                request
            } else {
                let fallback_handler = VNImageRequestHandler::initWithCGImage_options(
                    VNImageRequestHandler::alloc(),
                    &cg_image,
                    &options,
                );
                let fallback_req = VNRecognizeTextRequest::new();
                fallback_req.setRecognitionLevel(VNRequestTextRecognitionLevel::Fast);
                fallback_req.setUsesLanguageCorrection(false);
                let fb_ref: &VNRequest = fallback_req.as_ref();
                let fb_requests = NSArray::from_slice(&[fb_ref]);
                fallback_handler
                    .performRequests_error(&fb_requests)
                    .map_err(|err| OcrError::VisionExecution(err.localizedDescription().to_string()))?;
                fallback_req
            };

            let mut raw_items: Vec<(String, NormalizedBoundingBox)> = Vec::new();
            if let Some(observations) = active_request.results() {
                let count = observations.count();
                for idx in 0..count {
                    let obs: &VNRecognizedTextObservation = &observations.objectAtIndex(idx);
                    let candidates = obs.topCandidates(1);
                    if let Some(best) = candidates.firstObject() {
                        let clean = best
                            .string()
                            .to_string()
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(140)
                            .collect::<String>()
                            .trim()
                            .to_string();
                        if !is_meaningful_ocr_label(&clean) {
                            continue;
                        }
                        let rect = obs.boundingBox();
                        let norm = NormalizedBoundingBox {
                            x: rect.origin.x,
                            y: rect.origin.y,
                            width: rect.size.width,
                            height: rect.size.height,
                        };
                        raw_items.push((clean, norm));
                    }
                }
            }

            raw_items.sort_by(|a, b| {
                let top_a = 1.0 - a.1.y - a.1.height;
                let top_b = 1.0 - b.1.y - b.1.height;
                top_a
                    .partial_cmp(&top_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        a.1.x
                            .partial_cmp(&b.1.x)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
            });

            raw_items.truncate(MAX_RAW_OCR_ITEMS);
            Ok(raw_items)
        }
    })
}

#[cfg(not(target_os = "macos"))]
fn extract_raw_ocr_from_rgba(
    _rgba: &RgbaImage,
) -> Result<Vec<(String, NormalizedBoundingBox)>, OcrError> {
    Ok(Vec::new())
}

pub fn extract_ocr_from_rgba(rgba: &RgbaImage) -> Result<OcrExtractionResult, OcrError> {
    let raw_items = extract_raw_ocr_from_rgba(rgba)?;
    let mut formatted_lines = Vec::with_capacity(raw_items.len());
    let mut bbox_by_id = HashMap::with_capacity(raw_items.len());

    for (idx, (text, bbox)) in raw_items.into_iter().enumerate() {
        let id = (idx + 1) as u32;
        formatted_lines.push(format!("[{id}] {text}"));
        bbox_by_id.insert(id, bbox);
    }

    Ok(OcrExtractionResult {
        formatted_lines,
        bbox_by_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn treats_secure_subrole_as_password_field() {
        assert!(is_secure_role(Some("AXTextField"), Some("AXSecureTextField")));
        assert!(is_secure_role(Some("AXSecureTextField"), None));
        assert!(!is_secure_role(Some("AXTextField"), None));
        assert!(!is_secure_role(None, Some("AXSearchField")));
    }

    #[test]
    fn converts_bottom_left_vision_coordinates_and_preserves_menu_bar_edges() {
        let vision_bbox = NormalizedBoundingBox {
            x: 0.25,
            y: 0.70,
            width: 0.20,
            height: 0.10,
        };
        let screen = vision_bbox_to_screen_rect(vision_bbox, 1440.0, 900.0);
        assert!((screen.x - 360.0).abs() < 1e-6);
        assert!((screen.y_top_left - 180.0).abs() < 1e-6);
        assert!((screen.width - 288.0).abs() < 1e-6);
        assert!((screen.height - 90.0).abs() < 1e-6);
        assert!((screen.center_x - 504.0).abs() < 1e-6);
        assert!((screen.center_y - 225.0).abs() < 1e-6);

        let menu_bar_bbox = NormalizedBoundingBox {
            x: 0.05,
            y: 0.985,
            width: 0.04,
            height: 0.014,
        };
        let menu_rect = vision_bbox_to_screen_rect(menu_bar_bbox, 1440.0, 900.0);
        assert!(menu_rect.center_y < 12.0);
    }

    #[test]
    fn merges_ax_and_ocr_targets_with_dedup_and_normalized_coords() {
        let metrics = ScreenMetrics {
            origin_x: 0.0,
            origin_y: 0.0,
            logical_width: 1000.0,
            logical_height: 800.0,
            pixel_width: 2000,
            pixel_height: 1600,
            scale_factor: 2.0,
        };
        let ax = vec![RawTarget {
            role: "Button".to_string(),
            label: "Compose".to_string(),
            is_interactive: true,
            x: 180.0,
            y: 100.0,
            width: 80.0,
            height: 30.0,
            center_x: 220.0,
            center_y: 115.0,
            enabled: Some(true),
            focused: Some(false),
            selected: None,
            value_preview: None,
        }];
        let ocr = vec![
            RawTarget {
                role: "Text".to_string(),
                label: "Compose".to_string(),
                is_interactive: false,
                x: 190.0,
                y: 105.0,
                width: 60.0,
                height: 20.0,
                center_x: 220.0,
                center_y: 115.0,
                enabled: None,
                focused: None,
                selected: None,
                value_preview: None,
            },
            RawTarget {
                role: "Text".to_string(),
                label: "Inbox".to_string(),
                is_interactive: false,
                x: 40.0,
                y: 200.0,
                width: 50.0,
                height: 18.0,
                center_x: 65.0,
                center_y: 209.0,
                enabled: None,
                focused: None,
                selected: None,
                value_preview: None,
            },
        ];

        let merged = merge_targets(ax, ocr, metrics);
        assert_eq!(merged.targets_by_id.len(), 2);
        assert_eq!(
            merged.formatted_lines[0],
            "[1] (Button) \"Compose\" @(x=0.220, y=0.144, w=0.080, h=0.037)"
        );
        assert_eq!(
            merged.formatted_lines[1],
            "[2] (Text) \"Inbox\" @(x=0.065, y=0.261, w=0.050, h=0.022)"
        );
        assert!(merged.targets_by_id[&1].is_interactive);
        assert!(!merged.targets_by_id[&2].is_interactive);
    }

    #[test]
    fn spatial_grid_preserves_bottom_of_screen_ocr_items() {
        let metrics = ScreenMetrics {
            origin_x: 0.0,
            origin_y: 0.0,
            logical_width: 1000.0,
            logical_height: 1000.0,
            pixel_width: 2000,
            pixel_height: 2000,
            scale_factor: 2.0,
        };
        let mut ocr = Vec::new();
        for i in 0..200 {
            ocr.push(RawTarget {
                role: "Text".to_string(),
                label: format!("TopRow{i}"),
                is_interactive: false,
                x: 50.0,
                y: 20.0 + (i as f64) * 1.5,
                width: 80.0,
                height: 12.0,
                center_x: 90.0,
                center_y: 26.0 + (i as f64) * 1.5,
                enabled: None,
                focused: None,
                selected: None,
                value_preview: None,
            });
        }
        ocr.push(RawTarget {
            role: "Text".to_string(),
            label: "SendButton".to_string(),
            is_interactive: false,
            x: 800.0,
            y: 920.0,
            width: 80.0,
            height: 24.0,
            center_x: 840.0,
            center_y: 932.0,
            enabled: None,
            focused: None,
            selected: None,
            value_preview: None,
        });

        let merged = merge_targets(Vec::new(), ocr, metrics);
        assert_eq!(merged.targets_by_id.len(), MAX_TOTAL_TARGETS);
        assert!(merged
            .formatted_lines
            .iter()
            .any(|line| line.contains("SendButton")));
    }

    #[test]
    fn executes_vision_ocr_cleanly_on_blank_in_memory_frame() {
        let blank = RgbaImage::from_pixel(320, 160, Rgba([255, 255, 255, 255]));
        let result = extract_ocr_from_rgba(&blank).expect("Vision OCR should succeed on RGBA image");
        assert!(result.formatted_lines.len() <= MAX_RAW_OCR_ITEMS);
    }
}
