use active_win_pos_rs::get_active_window;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, RgbaImage};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use xcap::Monitor;

const MAX_DOWNSCALED_WIDTH: u32 = 1024;
const JPEG_QUALITY: u8 = 76;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("macOS Screen Recording permission has not been granted")]
    PermissionDenied,
    #[error("no primary display detected")]
    NoPrimaryDisplay,
    #[error("screen capture failed: {0}")]
    ScreenCapture(String),
    #[error("JPEG encoding failed: {0}")]
    ImageEncode(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWindowMetadata {
    pub app_name: String,
    pub window_title: String,
    pub is_system_locked: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenMetrics {
    pub origin_x: f64,
    pub origin_y: f64,
    pub logical_width: f64,
    pub logical_height: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub scale_factor: f64,
}

pub struct RawCaptureFrame {
    pub full_res_rgba: RgbaImage,
    pub base64_jpeg: String,
    pub active_window: ActiveWindowMetadata,
    pub screen_metrics: ScreenMetrics,
}

pub fn has_screen_recording_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_core_graphics::CGPreflightScreenCaptureAccess()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn request_screen_recording_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        if objc2_core_graphics::CGPreflightScreenCaptureAccess() {
            return true;
        }
        objc2_core_graphics::CGRequestScreenCaptureAccess()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn read_active_window_metadata() -> ActiveWindowMetadata {
    match get_active_window() {
        Ok(win) => {
            let app_name = sanitize_metadata_field(&win.app_name, "Unknown");
            let window_title = sanitize_metadata_field(&win.title, "");
            let is_system_locked = matches!(
                app_name.as_str(),
                "loginwindow" | "ScreenSaverEngine" | "LockScreen"
            );
            ActiveWindowMetadata {
                app_name,
                window_title,
                is_system_locked,
            }
        }
        Err(_) => ActiveWindowMetadata {
            app_name: "Desktop".to_string(),
            window_title: String::new(),
            is_system_locked: false,
        },
    }
}

pub fn primary_screen_metrics() -> Result<ScreenMetrics, CaptureError> {
    let monitor = select_primary_monitor()?;
    extract_screen_metrics(&monitor)
}

pub fn capture_primary_display_in_ram() -> Result<RawCaptureFrame, CaptureError> {
    if !has_screen_recording_permission() {
        return Err(CaptureError::PermissionDenied);
    }

    let active_window = read_active_window_metadata();
    let monitor = select_primary_monitor()?;
    let screen_metrics = extract_screen_metrics(&monitor)?;
    let full_res_rgba = monitor
        .capture_image()
        .map_err(|err| CaptureError::ScreenCapture(err.to_string()))?;

    let base64_jpeg = downscale_and_encode_jpeg_base64(&full_res_rgba)?;

    Ok(RawCaptureFrame {
        full_res_rgba,
        base64_jpeg,
        active_window,
        screen_metrics,
    })
}

pub fn downscale_and_encode_jpeg_base64(rgba: &RgbaImage) -> Result<String, CaptureError> {
    let (width, height) = rgba.dimensions();
    let dynamic = DynamicImage::ImageRgba8(rgba.clone());

    let resized = if width > MAX_DOWNSCALED_WIDTH && width > 0 {
        let ratio = f64::from(MAX_DOWNSCALED_WIDTH) / f64::from(width);
        let target_height = ((f64::from(height) * ratio).round() as u32).max(1);
        dynamic.resize_exact(MAX_DOWNSCALED_WIDTH, target_height, FilterType::Triangle)
    } else {
        dynamic
    };

    let rgb_image = resized.to_rgb8();
    let mut jpeg_bytes: Vec<u8> = Vec::with_capacity(96 * 1024);
    let mut encoder = JpegEncoder::new_with_quality(&mut jpeg_bytes, JPEG_QUALITY);
    encoder
        .encode_image(&rgb_image)
        .map_err(|err| CaptureError::ImageEncode(err.to_string()))?;

    Ok(BASE64_STANDARD.encode(&jpeg_bytes))
}

fn select_primary_monitor() -> Result<Monitor, CaptureError> {
    let monitors = Monitor::all().map_err(|err| CaptureError::ScreenCapture(err.to_string()))?;
    if monitors.is_empty() {
        return Err(CaptureError::NoPrimaryDisplay);
    }
    if let Some(primary) = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .cloned()
    {
        return Ok(primary);
    }
    Ok(monitors[0].clone())
}

fn extract_screen_metrics(monitor: &Monitor) -> Result<ScreenMetrics, CaptureError> {
    let pixel_width = monitor
        .width()
        .map_err(|e| CaptureError::ScreenCapture(e.to_string()))?;
    let pixel_height = monitor
        .height()
        .map_err(|e| CaptureError::ScreenCapture(e.to_string()))?;
    let scale_raw = monitor.scale_factor().unwrap_or(1.0);
    let scale_factor = if scale_raw > 0.1 {
        f64::from(scale_raw)
    } else {
        1.0
    };
    let origin_x = f64::from(monitor.x().unwrap_or(0));
    let origin_y = f64::from(monitor.y().unwrap_or(0));

    let logical_width = if pixel_width > 2200 && (scale_factor - 1.0).abs() < f64::EPSILON {
        f64::from(pixel_width)
    } else {
        f64::from(pixel_width) / scale_factor
    };
    let logical_height = if pixel_height > 1400 && (scale_factor - 1.0).abs() < f64::EPSILON {
        f64::from(pixel_height)
    } else {
        f64::from(pixel_height) / scale_factor
    };

    Ok(ScreenMetrics {
        origin_x,
        origin_y,
        logical_width,
        logical_height,
        pixel_width,
        pixel_height,
        scale_factor,
    })
}

fn sanitize_metadata_field(raw: &str, fallback: &str) -> String {
    let cleaned = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn downscales_wide_image_to_1024_and_encodes_raw_base64_without_prefix() {
        let img = RgbaImage::from_pixel(2560, 1440, Rgba([24, 48, 96, 255]));
        let encoded = downscale_and_encode_jpeg_base64(&img).expect("jpeg encode should succeed");
        assert!(!encoded.is_empty());
        assert!(!encoded.starts_with("data:image/"));

        let decoded_bytes = BASE64_STANDARD
            .decode(&encoded)
            .expect("valid base64 output");
        let decoded_img = image::load_from_memory(&decoded_bytes).expect("valid jpeg bytes");
        assert_eq!(decoded_img.width(), 1024);
        assert_eq!(decoded_img.height(), 576);
    }

    #[test]
    fn preserves_dimensions_when_width_below_1024() {
        let img = RgbaImage::from_pixel(800, 600, Rgba([120, 130, 140, 255]));
        let encoded = downscale_and_encode_jpeg_base64(&img).expect("jpeg encode should succeed");
        let decoded_bytes = BASE64_STANDARD
            .decode(&encoded)
            .expect("valid base64 output");
        let decoded_img = image::load_from_memory(&decoded_bytes).expect("valid jpeg bytes");
        assert_eq!(decoded_img.width(), 800);
        assert_eq!(decoded_img.height(), 600);
    }
}
