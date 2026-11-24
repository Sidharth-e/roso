use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

const MAX_OCR_LINES: usize = 80;

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

#[derive(Debug, Clone)]
pub struct OcrLineObservation {
    pub id: u32,
    pub text: String,
    pub formatted_line: String,
    pub vision_bbox: NormalizedBoundingBox,
}

#[derive(Debug, Clone, Default)]
pub struct OcrExtractionResult {
    pub formatted_lines: Vec<String>,
    pub bbox_by_id: HashMap<u32, NormalizedBoundingBox>,
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
    let margin = 24.0;
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

#[cfg(target_os = "macos")]
pub fn extract_ocr_from_rgba(rgba: &RgbaImage) -> Result<OcrExtractionResult, OcrError> {
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
        return Ok(OcrExtractionResult::default());
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

            let mut formatted_lines = Vec::with_capacity(raw_items.len());
            let mut bbox_by_id = HashMap::with_capacity(raw_items.len());

            for (idx, (text, bbox)) in raw_items.into_iter().enumerate() {
                let id = (idx + 1) as u32;
                let formatted = format!("[{id}] {text}");
                formatted_lines.push(formatted);
                bbox_by_id.insert(id, bbox);
            }

            Ok(OcrExtractionResult {
                formatted_lines,
                bbox_by_id,
            })
        }
    })
}

#[cfg(not(target_os = "macos"))]
pub fn extract_ocr_from_rgba(_rgba: &RgbaImage) -> Result<OcrExtractionResult, OcrError> {
    Ok(OcrExtractionResult::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn converts_bottom_left_vision_coordinates_to_top_left_screen_coordinates() {
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
    }

    #[test]
    fn executes_vision_ocr_cleanly_on_blank_in_memory_frame() {
        let blank = RgbaImage::from_pixel(320, 160, Rgba([255, 255, 255, 255]));
        let result = extract_ocr_from_rgba(&blank).expect("Vision OCR should succeed on RGBA image");
        assert!(result.formatted_lines.len() <= MAX_OCR_LINES);
    }
}
