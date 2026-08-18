//! Preview watermark for operator append (photo via `image`, video via FFmpeg in PATH).

use std::path::{Path, PathBuf};
use std::process::Command;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, RgbaImage};

const PREVIEW_STEMPEL: &str = "preview_stempel.png";

pub fn find_preview_stamp() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidates = vec![
        manifest.join("resources").join("assets").join(PREVIEW_STEMPEL),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("resources").join("assets").join(PREVIEW_STEMPEL));
            candidates.push(dir.join("assets").join(PREVIEW_STEMPEL));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn watermark_photo(input: &Path, output: &Path) -> Result<(), String> {
    let stamp = find_preview_stamp().ok_or_else(|| {
        format!(
            "Preview-Stempel '{PREVIEW_STEMPEL}' fehlt (unter src-tauri/resources/assets/ ablegen)."
        )
    })?;
    let foto = image::open(input)
        .map_err(|e| format!("Foto öffnen {}: {e}", input.display()))?;
    let stamp_img = image::open(&stamp)
        .map_err(|e| format!("Stempel öffnen {}: {e}", stamp.display()))?;

    let target_h = 720u32;
    let aspect = foto.width() as f64 / foto.height().max(1) as f64;
    let new_w = ((target_h as f64) * aspect).round().max(1.0) as u32;
    let mut foto_rgba = foto
        .resize_exact(new_w, target_h, FilterType::Lanczos3)
        .to_rgba8();

    let wm = stamp_img.to_rgba8();
    let wm_aspect = wm.width() as f64 / wm.height().max(1) as f64;
    let foto_aspect = foto_rgba.width() as f64 / foto_rgba.height().max(1) as f64;
    let (wm_w, wm_h) = if wm_aspect > foto_aspect {
        let w = foto_rgba.width();
        let h = ((w as f64) / wm_aspect).round().max(1.0) as u32;
        (w, h)
    } else {
        let h = foto_rgba.height();
        let w = ((h as f64) * wm_aspect).round().max(1.0) as u32;
        (w, h)
    };
    let wm_scaled = DynamicImage::ImageRgba8(wm)
        .resize_exact(wm_w, wm_h, FilterType::Lanczos3)
        .to_rgba8();
    let paste_x = (foto_rgba.width().saturating_sub(wm_w)) / 2;
    let paste_y = (foto_rgba.height().saturating_sub(wm_h)) / 2;
    overlay_rgba(&mut foto_rgba, &wm_scaled, paste_x, paste_y);

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Ordner anlegen: {e}"))?;
    }
    DynamicImage::ImageRgba8(foto_rgba)
        .to_rgb8()
        .save_with_format(output, ImageFormat::Jpeg)
        .map_err(|e| format!("Preview-Foto speichern: {e}"))?;
    Ok(())
}

fn overlay_rgba(base: &mut RgbaImage, overlay: &RgbaImage, ox: u32, oy: u32) {
    for (x, y, pixel) in overlay.enumerate_pixels() {
        let dx = ox + x;
        let dy = oy + y;
        if dx >= base.width() || dy >= base.height() {
            continue;
        }
        let src = pixel.0;
        let alpha = src[3] as f32 / 255.0;
        if alpha <= 0.0 {
            continue;
        }
        let dst = base.get_pixel_mut(dx, dy);
        for c in 0..3 {
            dst.0[c] =
                ((src[c] as f32) * alpha + (dst.0[c] as f32) * (1.0 - alpha)).round() as u8;
        }
        dst.0[3] = 255;
    }
}

pub fn watermark_video(input: &Path, output: &Path, stamp: &Path) -> Result<(), String> {
    let filter = concat!(
        "[0]scale=320:240:force_original_aspect_ratio=decrease,",
        "pad=320:240:(ow-iw)/2:(oh-ih)/2:black,format=yuv420p[v];",
        "[1]scale=320:240:force_original_aspect_ratio=decrease:eval=init[wm_scaled];",
        "[v][wm_scaled]overlay=(W-w)/2:(H-h)/2"
    );
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-i",
            &stamp.to_string_lossy(),
            "-filter_complex",
            filter,
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "28",
            "-movflags",
            "+faststart",
            "-an",
            &output.to_string_lossy(),
        ])
        .status()
        .map_err(|e| format!("FFmpeg starten: {e}"))?;
    if !status.success() {
        return Err("FFmpeg Preview-Video fehlgeschlagen (FFmpeg im PATH erforderlich).".into());
    }
    Ok(())
}

pub fn write_preview_media(src: &Path, dest: &Path, is_video: bool) -> Result<(), String> {
    if is_video {
        let stamp = find_preview_stamp().ok_or_else(|| {
            format!(
                "Preview-Stempel '{PREVIEW_STEMPEL}' fehlt (unter src-tauri/resources/assets/ ablegen)."
            )
        })?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Ordner anlegen: {e}"))?;
        }
        watermark_video(src, dest, &stamp)
    } else {
        watermark_photo(src, dest)
    }
}
