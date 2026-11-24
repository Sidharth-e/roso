use active_win_pos_rs::get_active_window;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::{Rgb, RgbImage, RgbaImage};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use xcap::Monitor;

pub const MAX_DOWNSCALED_WIDTH: u32 = 1024;
const JPEG_QUALITY: u8 = 72;

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
    pub process_id: u64,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnnotationBox {
    pub id: u32,
    pub x_norm: f64,
    pub y_norm: f64,
    pub w_norm: f64,
    pub h_norm: f64,
    pub is_interactive: bool,
}

pub struct UnencodedCaptureFrame {
    pub full_res_rgba: RgbaImage,
    pub active_window: ActiveWindowMetadata,
    pub screen_metrics: ScreenMetrics,
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
                process_id: win.process_id,
                is_system_locked,
            }
        }
        Err(_) => ActiveWindowMetadata {
            app_name: "Desktop".to_string(),
            window_title: String::new(),
            process_id: 0,
            is_system_locked: false,
        },
    }
}

pub fn primary_screen_metrics() -> Result<ScreenMetrics, CaptureError> {
    let monitor = select_primary_monitor()?;
    extract_screen_metrics(&monitor, None)
}

pub fn capture_raw_frame_in_ram() -> Result<UnencodedCaptureFrame, CaptureError> {
    if !has_screen_recording_permission() {
        return Err(CaptureError::PermissionDenied);
    }

    let active_window = read_active_window_metadata();
    let monitor = select_primary_monitor()?;
    let full_res_rgba = monitor
        .capture_image()
        .map_err(|err| CaptureError::ScreenCapture(err.to_string()))?;
    let screen_metrics = extract_screen_metrics(&monitor, Some(full_res_rgba.dimensions()))?;

    Ok(UnencodedCaptureFrame {
        full_res_rgba,
        active_window,
        screen_metrics,
    })
}

pub fn capture_primary_display_in_ram() -> Result<RawCaptureFrame, CaptureError> {
    let raw = capture_raw_frame_in_ram()?;
    let base64_jpeg = downscale_and_encode_jpeg_base64(&raw.full_res_rgba)?;

    Ok(RawCaptureFrame {
        full_res_rgba: raw.full_res_rgba,
        base64_jpeg,
        active_window: raw.active_window,
        screen_metrics: raw.screen_metrics,
    })
}

pub fn downscale_and_encode_jpeg_base64(rgba: &RgbaImage) -> Result<String, CaptureError> {
    let rgb_image = downscale_rgba_to_rgb(rgba, MAX_DOWNSCALED_WIDTH);
    encode_rgb_jpeg_base64(&rgb_image)
}

pub fn compute_frame_diff_ratio(before: &RgbaImage, after: &RgbaImage) -> f64 {
    if before.dimensions() != after.dimensions() {
        return 1.0;
    }
    let (w, h) = before.dimensions();
    if w == 0 || h == 0 {
        return 0.0;
    }
    let cols = 64u32.min(w);
    let rows = 36u32.min(h);
    let mut changed: u64 = 0;
    let total = u64::from(cols) * u64::from(rows);

    for gy in 0..rows {
        let py = ((u64::from(gy) * u64::from(h)) / u64::from(rows)) as u32;
        for gx in 0..cols {
            let px = ((u64::from(gx) * u64::from(w)) / u64::from(cols)) as u32;
            let p1 = before.get_pixel(px, py);
            let p2 = after.get_pixel(px, py);
            let diff = (i32::from(p1[0]) - i32::from(p2[0])).abs()
                + (i32::from(p1[1]) - i32::from(p2[1])).abs()
                + (i32::from(p1[2]) - i32::from(p2[2])).abs();
            if diff > 36 {
                changed += 1;
            }
        }
    }

    changed as f64 / total as f64
}

pub fn render_annotated_jpeg_base64(
    rgba: &RgbaImage,
    boxes: &[AnnotationBox],
) -> Result<String, CaptureError> {
    let mut rgb_image = downscale_rgba_to_rgb(rgba, MAX_DOWNSCALED_WIDTH);
    annotate_rgb_and_encode_jpeg_base64(&mut rgb_image, boxes)
}

pub fn annotate_rgb_and_encode_jpeg_base64(
    rgb_image: &mut RgbImage,
    boxes: &[AnnotationBox],
) -> Result<String, CaptureError> {
    let (w, h) = rgb_image.dimensions();
    if w < 16 || h < 16 {
        return encode_rgb_jpeg_base64(rgb_image);
    }

    draw_coordinate_grid(rgb_image);

    for b in boxes {
        let bx = (b.x_norm.clamp(0.0, 1.0) * f64::from(w)).round() as i32;
        let by = (b.y_norm.clamp(0.0, 1.0) * f64::from(h)).round() as i32;
        let bw = (b.w_norm.clamp(0.0, 1.0) * f64::from(w)).round().max(4.0) as i32;
        let bh = (b.h_norm.clamp(0.0, 1.0) * f64::from(h)).round().max(4.0) as i32;

        let x0 = bx.clamp(0, w as i32 - 1);
        let y0 = by.clamp(0, h as i32 - 1);
        let x1 = (bx + bw).clamp(x0 + 1, w as i32);
        let y1 = (by + bh).clamp(y0 + 1, h as i32);

        let color = if b.is_interactive {
            [37u8, 99u8, 235u8]
        } else {
            [217u8, 119u8, 6u8]
        };
        draw_rect_outline(rgb_image, x0, y0, x1 - x0, y1 - y0, color);

        let label = b.id.to_string();
        let pill_w = (label.len() as i32) * 6 + 4;
        let pill_h = 11i32;
        let pill_x = x0.clamp(0, (w as i32 - pill_w).max(0));
        let pill_y = if y0 >= pill_h {
            y0 - pill_h
        } else {
            y0.clamp(0, (h as i32 - pill_h).max(0))
        };
        fill_rect(rgb_image, pill_x, pill_y, pill_w, pill_h, [15, 23, 42]);
        draw_bitmap_text(rgb_image, pill_x + 2, pill_y + 2, &label, [248, 250, 252]);
    }

    encode_rgb_jpeg_base64(rgb_image)
}

pub fn downscale_rgba_to_rgb(rgba: &RgbaImage, max_width: u32) -> RgbImage {
    let (src_w, src_h) = rgba.dimensions();
    let src = rgba.as_raw();
    if src_w == 0 || src_h == 0 {
        return RgbImage::new(src_w, src_h);
    }

    if src_w <= max_width || max_width == 0 {
        let mut out = Vec::with_capacity((src_w as usize) * (src_h as usize) * 3);
        for px in src.chunks_exact(4) {
            out.push(px[0]);
            out.push(px[1]);
            out.push(px[2]);
        }
        return RgbImage::from_raw(src_w, src_h, out).unwrap_or_else(|| RgbImage::new(src_w, src_h));
    }

    let dst_w = max_width;
    let dst_h = ((f64::from(src_h) * (f64::from(dst_w) / f64::from(src_w))).round() as u32).max(1);
    let mut out = vec![0u8; (dst_w as usize) * (dst_h as usize) * 3];

    let x_map: Vec<(usize, usize, u32, u32)> = (0..dst_w)
        .map(|dx| {
            let fx = (u64::from(dx) * u64::from(src_w - 1) * 256) / u64::from((dst_w - 1).max(1));
            let x0 = (fx >> 8) as usize;
            let x1 = (x0 + 1).min((src_w - 1) as usize);
            let wx = (fx & 0xFF) as u32;
            (x0 * 4, x1 * 4, 256 - wx, wx)
        })
        .collect();

    let src_stride = (src_w as usize) * 4;
    let dst_stride = (dst_w as usize) * 3;

    for dy in 0..dst_h {
        let fy = (u64::from(dy) * u64::from(src_h - 1) * 256) / u64::from((dst_h - 1).max(1));
        let y0 = (fy >> 8) as usize;
        let y1 = (y0 + 1).min((src_h - 1) as usize);
        let wy = (fy & 0xFF) as u32;
        let inv_wy = 256 - wy;

        let row0 = &src[y0 * src_stride..(y0 + 1) * src_stride];
        let row1 = &src[y1 * src_stride..(y1 + 1) * src_stride];
        let dst_row = &mut out[(dy as usize) * dst_stride..((dy as usize) + 1) * dst_stride];

        for (dx, &(x0_off, x1_off, inv_wx, wx)) in x_map.iter().enumerate() {
            let w00 = inv_wx * inv_wy;
            let w10 = wx * inv_wy;
            let w01 = inv_wx * wy;
            let w11 = wx * wy;
            let dst_off = dx * 3;

            for c in 0..3 {
                let p00 = u32::from(row0[x0_off + c]);
                let p10 = u32::from(row0[x1_off + c]);
                let p01 = u32::from(row1[x0_off + c]);
                let p11 = u32::from(row1[x1_off + c]);
                let val = (p00 * w00 + p10 * w10 + p01 * w01 + p11 * w11 + 32768) >> 16;
                dst_row[dst_off + c] = val as u8;
            }
        }
    }

    RgbImage::from_raw(dst_w, dst_h, out).unwrap_or_else(|| RgbImage::new(dst_w, dst_h))
}

pub fn downscale_rgba_for_ocr(rgba: &RgbaImage, max_width: u32) -> Option<RgbaImage> {
    let (src_w, src_h) = rgba.dimensions();
    if src_w <= max_width || max_width == 0 || src_w == 0 || src_h == 0 {
        return None;
    }

    let src = rgba.as_raw();
    let dst_w = max_width;
    let dst_h = ((f64::from(src_h) * (f64::from(dst_w) / f64::from(src_w))).round() as u32).max(1);
    let mut out = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];

    let x_map: Vec<(usize, usize, u32, u32)> = (0..dst_w)
        .map(|dx| {
            let fx = (u64::from(dx) * u64::from(src_w - 1) * 256) / u64::from((dst_w - 1).max(1));
            let x0 = (fx >> 8) as usize;
            let x1 = (x0 + 1).min((src_w - 1) as usize);
            let wx = (fx & 0xFF) as u32;
            (x0 * 4, x1 * 4, 256 - wx, wx)
        })
        .collect();

    let src_stride = (src_w as usize) * 4;
    let dst_stride = (dst_w as usize) * 4;

    for dy in 0..dst_h {
        let fy = (u64::from(dy) * u64::from(src_h - 1) * 256) / u64::from((dst_h - 1).max(1));
        let y0 = (fy >> 8) as usize;
        let y1 = (y0 + 1).min((src_h - 1) as usize);
        let wy = (fy & 0xFF) as u32;
        let inv_wy = 256 - wy;

        let row0 = &src[y0 * src_stride..(y0 + 1) * src_stride];
        let row1 = &src[y1 * src_stride..(y1 + 1) * src_stride];
        let dst_row = &mut out[(dy as usize) * dst_stride..((dy as usize) + 1) * dst_stride];

        for (dx, &(x0_off, x1_off, inv_wx, wx)) in x_map.iter().enumerate() {
            let w00 = inv_wx * inv_wy;
            let w10 = wx * inv_wy;
            let w01 = inv_wx * wy;
            let w11 = wx * wy;
            let dst_off = dx * 4;

            for c in 0..3 {
                let p00 = u32::from(row0[x0_off + c]);
                let p10 = u32::from(row0[x1_off + c]);
                let p01 = u32::from(row1[x0_off + c]);
                let p11 = u32::from(row1[x1_off + c]);
                let val = (p00 * w00 + p10 * w10 + p01 * w01 + p11 * w11 + 32768) >> 16;
                dst_row[dst_off + c] = val as u8;
            }
            dst_row[dst_off + 3] = 255;
        }
    }

    RgbaImage::from_raw(dst_w, dst_h, out)
}

fn encode_rgb_jpeg_base64(rgb_image: &RgbImage) -> Result<String, CaptureError> {
    let mut jpeg_bytes: Vec<u8> = Vec::with_capacity(128 * 1024);
    let mut encoder = JpegEncoder::new_with_quality(&mut jpeg_bytes, JPEG_QUALITY);
    encoder
        .encode_image(rgb_image)
        .map_err(|err| CaptureError::ImageEncode(err.to_string()))?;

    Ok(BASE64_STANDARD.encode(&jpeg_bytes))
}

fn draw_coordinate_grid(img: &mut RgbImage) {
    let (w, h) = img.dimensions();
    let grid_color = [148u8, 163u8, 184u8];

    for step in 1..=9 {
        let frac = f64::from(step) / 10.0;
        let gx = ((f64::from(w) * frac).round() as i32).clamp(0, w as i32 - 1);
        let gy = ((f64::from(h) * frac).round() as i32).clamp(0, h as i32 - 1);

        for py in 0..h as i32 {
            blend_pixel(img, gx, py, grid_color, 0.22);
        }
        for px in 0..w as i32 {
            blend_pixel(img, px, gy, grid_color, 0.22);
        }

        let label = format!("0.{step}");
        let top_x = (gx + 2).clamp(0, (w as i32 - 20).max(0));
        fill_rect(img, top_x, 2, 19, 10, [15, 23, 42]);
        draw_bitmap_text(img, top_x + 1, 3, &label, [226, 232, 240]);

        let left_y = (gy + 2).clamp(0, (h as i32 - 10).max(0));
        fill_rect(img, 2, left_y, 19, 10, [15, 23, 42]);
        draw_bitmap_text(img, 3, left_y + 1, &label, [226, 232, 240]);
    }
}

fn blend_pixel(img: &mut RgbImage, x: i32, y: i32, color: [u8; 3], alpha: f32) {
    let (w, h) = img.dimensions();
    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
        return;
    }
    let px = img.get_pixel_mut(x as u32, y as u32);
    let inv = 1.0 - alpha;
    px[0] = (f32::from(px[0]) * inv + f32::from(color[0]) * alpha).round() as u8;
    px[1] = (f32::from(px[1]) * inv + f32::from(color[1]) * alpha).round() as u8;
    px[2] = (f32::from(px[2]) * inv + f32::from(color[2]) * alpha).round() as u8;
}

fn fill_rect(img: &mut RgbImage, x: i32, y: i32, rw: i32, rh: i32, color: [u8; 3]) {
    let (w, h) = img.dimensions();
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + rw).min(w as i32);
    let y1 = (y + rh).min(h as i32);
    for py in y0..y1 {
        for px in x0..x1 {
            img.put_pixel(px as u32, py as u32, Rgb(color));
        }
    }
}

fn draw_rect_outline(img: &mut RgbImage, x: i32, y: i32, rw: i32, rh: i32, color: [u8; 3]) {
    let (w, h) = img.dimensions();
    let x0 = x.clamp(0, w as i32 - 1);
    let y0 = y.clamp(0, h as i32 - 1);
    let x1 = (x + rw - 1).clamp(x0, w as i32 - 1);
    let y1 = (y + rh - 1).clamp(y0, h as i32 - 1);

    for px in x0..=x1 {
        img.put_pixel(px as u32, y0 as u32, Rgb(color));
        img.put_pixel(px as u32, y1 as u32, Rgb(color));
    }
    for py in y0..=y1 {
        img.put_pixel(x0 as u32, py as u32, Rgb(color));
        img.put_pixel(x1 as u32, py as u32, Rgb(color));
    }
}

fn draw_bitmap_text(img: &mut RgbImage, start_x: i32, start_y: i32, text: &str, color: [u8; 3]) {
    let (w, h) = img.dimensions();
    let mut cursor_x = start_x;
    for ch in text.chars() {
        let rows = glyph_5x7(ch);
        for (ry, row_bits) in rows.iter().enumerate() {
            for rx in 0..5 {
                if (row_bits >> (4 - rx)) & 1 == 1 {
                    let px = cursor_x + rx;
                    let py = start_y + ry as i32;
                    if px >= 0 && py >= 0 && px < w as i32 && py < h as i32 {
                        img.put_pixel(px as u32, py as u32, Rgb(color));
                    }
                }
            }
        }
        cursor_x += 6;
    }
}

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch {
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111],
        '3' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        _ => [0; 7],
    }
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

fn extract_screen_metrics(
    monitor: &Monitor,
    captured_pixels: Option<(u32, u32)>,
) -> Result<ScreenMetrics, CaptureError> {
    let mon_w = monitor
        .width()
        .map_err(|e| CaptureError::ScreenCapture(e.to_string()))?;
    let mon_h = monitor
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

    Ok(compute_screen_metrics(
        origin_x,
        origin_y,
        mon_w,
        mon_h,
        scale_factor,
        captured_pixels,
    ))
}

pub fn compute_screen_metrics(
    origin_x: f64,
    origin_y: f64,
    mon_w: u32,
    mon_h: u32,
    scale_factor: f64,
    captured_pixels: Option<(u32, u32)>,
) -> ScreenMetrics {
    #[cfg(target_os = "macos")]
    {
        let logical_width = f64::from(mon_w).max(1.0);
        let logical_height = f64::from(mon_h).max(1.0);
        let (pixel_width, pixel_height) = captured_pixels.unwrap_or((
            ((logical_width * scale_factor).round() as u32).max(1),
            ((logical_height * scale_factor).round() as u32).max(1),
        ));
        ScreenMetrics {
            origin_x,
            origin_y,
            logical_width,
            logical_height,
            pixel_width,
            pixel_height,
            scale_factor,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let (pixel_width, pixel_height) = captured_pixels.unwrap_or((mon_w, mon_h));
        let logical_width = (f64::from(pixel_width) / scale_factor).max(1.0);
        let logical_height = (f64::from(pixel_height) / scale_factor).max(1.0);
        ScreenMetrics {
            origin_x,
            origin_y,
            logical_width,
            logical_height,
            pixel_width,
            pixel_height,
            scale_factor,
        }
    }
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

    #[test]
    fn renders_annotated_jpeg_with_grid_and_som_boxes() {
        let img = RgbaImage::from_pixel(1600, 1000, Rgba([240, 244, 248, 255]));
        let boxes = vec![
            AnnotationBox {
                id: 1,
                x_norm: 0.15,
                y_norm: 0.25,
                w_norm: 0.12,
                h_norm: 0.05,
                is_interactive: true,
            },
            AnnotationBox {
                id: 12,
                x_norm: 0.85,
                y_norm: 0.88,
                w_norm: 0.14,
                h_norm: 0.08,
                is_interactive: false,
            },
        ];
        let encoded = render_annotated_jpeg_base64(&img, &boxes).expect("annotated jpeg");
        let bytes = BASE64_STANDARD.decode(&encoded).expect("valid base64");
        let decoded = image::load_from_memory(&bytes).expect("valid annotated jpeg");
        assert_eq!(decoded.width(), 1024);
        assert_eq!(decoded.height(), 640);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_metrics_preserve_logical_cgdisplaybounds_on_retina() {
        let m = compute_screen_metrics(0.0, 0.0, 1440, 900, 2.0, Some((2880, 1800)));
        assert!((m.logical_width - 1440.0).abs() < 1e-6);
        assert!((m.logical_height - 900.0).abs() < 1e-6);
        assert_eq!(m.pixel_width, 2880);
        assert_eq!(m.pixel_height, 1800);
    }

    #[test]
    fn computes_visual_diff_ratio_across_frames() {
        let a = RgbaImage::from_pixel(320, 180, Rgba([20, 30, 40, 255]));
        let b = RgbaImage::from_pixel(320, 180, Rgba([20, 30, 40, 255]));
        let c = RgbaImage::from_pixel(320, 180, Rgba([220, 230, 240, 255]));
        assert!((compute_frame_diff_ratio(&a, &b) - 0.0).abs() < 1e-6);
        assert!(compute_frame_diff_ratio(&a, &c) > 0.99);
    }
}
