use crate::automation::has_accessibility_permission;
use crate::capture::{AnnotationBox, ScreenMetrics};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use thiserror::Error;

const MAX_OCR_LINES: usize = 120;

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

pub fn build_unified_targets(
    rgba: &RgbaImage,
    pid: u64,
    metrics: ScreenMetrics,
) -> ScreenTargetMap {
    let ax_targets = extract_ax_elements(pid, metrics);
    let raw_ocr = extract_raw_ocr_from_rgba(rgba).unwrap_or_default();

    let ocr_targets: Vec<RawTarget> = raw_ocr
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
            }
        })
        .collect();

    merge_targets(ax_targets, ocr_targets, metrics)
}

pub fn merge_targets(
    ax_targets: Vec<RawTarget>,
    ocr_targets: Vec<RawTarget>,
    metrics: ScreenMetrics,
) -> ScreenTargetMap {
    let mut combined: Vec<RawTarget> = Vec::with_capacity(ax_targets.len() + ocr_targets.len());
    combined.extend(ax_targets);

    for ocr_item in ocr_targets {
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
            combined.push(ocr_item);
        }
    }

    combined.sort_by(|a, b| {
        a.y.partial_cmp(&b.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });

    combined.truncate(MAX_OCR_LINES);

    let safe_w = metrics.logical_width.max(1.0);
    let safe_h = metrics.logical_height.max(1.0);

    let mut formatted_lines = Vec::with_capacity(combined.len());
    let mut targets_by_id = HashMap::with_capacity(combined.len());
    let mut annotation_boxes = Vec::with_capacity(combined.len());

    for (idx, item) in combined.into_iter().enumerate() {
        let id = (idx + 1) as u32;
        let x_norm = (item.center_x / safe_w).clamp(0.0, 1.0);
        let y_norm = (item.center_y / safe_h).clamp(0.0, 1.0);

        let line = format!(
            "[{id}] ({}) \"{}\" @(x={x_norm:.3}, y={y_norm:.3})",
            item.role, item.label
        );
        formatted_lines.push(line);

        annotation_boxes.push(AnnotationBox {
            id,
            x_norm: (item.x / safe_w).clamp(0.0, 1.0),
            y_norm: (item.y / safe_h).clamp(0.0, 1.0),
            w_norm: (item.width / safe_w).clamp(0.0, 1.0),
            h_norm: (item.height / safe_h).clamp(0.0, 1.0),
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
            },
        );
    }

    ScreenTargetMap {
        formatted_lines,
        targets_by_id,
        annotation_boxes,
    }
}

#[cfg(target_os = "macos")]
mod macos_ax_tree {
    use super::*;
    use std::ffi::{c_void, CStr};

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
        fn AXUIElementCopyAttributeValue(
            element: *const c_void,
            attribute: *const c_void,
            value: *mut *const c_void,
        ) -> i32;
        fn AXValueGetValue(value: *const c_void, value_type: u32, value_ptr: *mut c_void) -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithBytes(
            alloc: *const c_void,
            bytes: *const u8,
            num_bytes: isize,
            encoding: u32,
            is_external_representation: u8,
        ) -> *const c_void;
        fn CFStringGetTypeID() -> usize;
        fn CFArrayGetTypeID() -> usize;
        fn CFGetTypeID(cf: *const c_void) -> usize;
        fn CFStringGetCString(
            the_string: *const c_void,
            buffer: *mut u8,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
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

    fn copy_attr(element: *const c_void, attr_name: &str) -> Option<CfGuard> {
        let attr = make_cf_string(attr_name)?;
        let mut out: *const c_void = std::ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(element, attr.as_ptr(), &mut out) };
        if err == 0 {
            CfGuard::new(out)
        } else {
            None
        }
    }

    fn copy_string_attr(element: *const c_void, attr_name: &str) -> Option<String> {
        let val = copy_attr(element, attr_name)?;
        unsafe {
            if CFGetTypeID(val.as_ptr()) != CFStringGetTypeID() {
                return None;
            }
            let mut buf = [0u8; 256];
            if CFStringGetCString(
                val.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
            ) == 0
            {
                return None;
            }
            let cstr = CStr::from_ptr(buf.as_ptr().cast());
            let clean = cstr
                .to_string_lossy()
                .chars()
                .filter(|c| !c.is_control())
                .take(100)
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

    fn copy_children(element: *const c_void) -> Vec<CfGuard> {
        let Some(arr) = copy_attr(element, "AXChildren") else {
            return Vec::new();
        };
        unsafe {
            if CFGetTypeID(arr.as_ptr()) != CFArrayGetTypeID() {
                return Vec::new();
            }
            let count = CFArrayGetCount(arr.as_ptr()).min(80);
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

    fn copy_position_and_size(element: *const c_void) -> Option<(AxCgPoint, AxCgSize)> {
        let pos_val = copy_attr(element, "AXPosition")?;
        let size_val = copy_attr(element, "AXSize")?;
        let mut pt = AxCgPoint { x: 0.0, y: 0.0 };
        let mut sz = AxCgSize {
            width: 0.0,
            height: 0.0,
        };
        unsafe {
            if AXValueGetValue(
                pos_val.as_ptr(),
                K_AX_VALUE_TYPE_CG_POINT,
                (&mut pt as *mut AxCgPoint).cast(),
            ) == 0
            {
                return None;
            }
            if AXValueGetValue(
                size_val.as_ptr(),
                K_AX_VALUE_TYPE_CG_SIZE,
                (&mut sz as *mut AxCgSize).cast(),
            ) == 0
            {
                return None;
            }
        }
        Some((pt, sz))
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

    pub fn extract(pid: u64, metrics: ScreenMetrics) -> Vec<RawTarget> {
        if pid == 0 || !has_accessibility_permission() {
            return Vec::new();
        }
        let Ok(pid_i32) = i32::try_from(pid) else {
            return Vec::new();
        };
        let Some(app_el) = CfGuard::new(unsafe { AXUIElementCreateApplication(pid_i32) }) else {
            return Vec::new();
        };

        let mut queue: VecDeque<(CfGuard, usize)> = VecDeque::new();
        if let Some(win) = copy_attr(app_el.as_ptr(), "AXFocusedWindow")
            .or_else(|| copy_attr(app_el.as_ptr(), "AXMainWindow"))
        {
            queue.push_back((win, 0));
        }
        if let Some(menubar) = copy_attr(app_el.as_ptr(), "AXMenuBar") {
            queue.push_back((menubar, 0));
        }

        let mut results: Vec<RawTarget> = Vec::new();
        let mut visited: usize = 0;
        const MAX_VISITED: usize = 300;
        const MAX_DEPTH: usize = 10;
        const MAX_TARGETS: usize = 60;

        while let Some((node, depth)) = queue.pop_front() {
            if visited >= MAX_VISITED || results.len() >= MAX_TARGETS {
                break;
            }
            visited += 1;

            if let Some(ax_role) = copy_string_attr(node.as_ptr(), "AXRole") {
                if let Some(clean_role) = map_interactive_role(&ax_role) {
                    if let Some((pt, sz)) = copy_position_and_size(node.as_ptr()) {
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
                            let label = copy_string_attr(node.as_ptr(), "AXTitle")
                                .or_else(|| copy_string_attr(node.as_ptr(), "AXDescription"))
                                .or_else(|| copy_string_attr(node.as_ptr(), "AXPlaceholderValue"))
                                .or_else(|| copy_string_attr(node.as_ptr(), "AXHelp"))
                                .or_else(|| copy_string_attr(node.as_ptr(), "AXValue"))
                                .unwrap_or_else(|| clean_role.to_string());

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
                            });
                        }
                    }
                }
            }

            if depth < MAX_DEPTH {
                for child in copy_children(node.as_ptr()) {
                    queue.push_back((child, depth + 1));
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

    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return Ok(Vec::new());
    }

    autoreleasepool(|_| {
        let raw_bytes = rgba.as_raw();
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

            let vn_request_ref: &VNRequest = request.as_ref();
            let requests = NSArray::from_slice(&[vn_request_ref]);

            handler
                .performRequests_error(&requests)
                .map_err(|err| OcrError::VisionExecution(err.localizedDescription().to_string()))?;

            let mut raw_items: Vec<(String, NormalizedBoundingBox)> = Vec::new();
            if let Some(observations) = request.results() {
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
                        if clean.is_empty() {
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

            raw_items.truncate(MAX_OCR_LINES);
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
            },
        ];

        let merged = merge_targets(ax, ocr, metrics);
        assert_eq!(merged.targets_by_id.len(), 2);
        assert_eq!(
            merged.formatted_lines[0],
            "[1] (Button) \"Compose\" @(x=0.220, y=0.144)"
        );
        assert_eq!(
            merged.formatted_lines[1],
            "[2] (Text) \"Inbox\" @(x=0.065, y=0.261)"
        );
        assert!(merged.targets_by_id[&1].is_interactive);
        assert!(!merged.targets_by_id[&2].is_interactive);
    }

    #[test]
    fn executes_vision_ocr_cleanly_on_blank_in_memory_frame() {
        let blank = RgbaImage::from_pixel(320, 160, Rgba([255, 255, 255, 255]));
        let result = extract_ocr_from_rgba(&blank).expect("Vision OCR should succeed on RGBA image");
        assert!(result.formatted_lines.len() <= MAX_OCR_LINES);
    }
}
