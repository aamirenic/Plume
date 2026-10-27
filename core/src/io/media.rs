//! Still image and movie capture.

use crate::{PlumeError, Result};
use image::{codecs::gif::GifEncoder, Frame, RgbaImage};
use std::path::{Path, PathBuf};

pub fn export_png(image: &RgbaImage, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    image.save(path)?;
    Ok(())
}

pub fn export_png_bytes(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    image
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(PlumeError::Image)?;
    Ok(bytes)
}

pub fn export_gif(frames: &[RgbaImage], path: &Path, delay_ms: u32) -> Result<()> {
    if frames.is_empty() {
        return Err(PlumeError::Invalid("GIF needs at least one frame".into()));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = GifEncoder::new(writer);
    encoder
        .set_repeat(image::codecs::gif::Repeat::Infinite)
        .map_err(|e| PlumeError::Invalid(format!("GIF setup: {e}")))?;
    let delay = image::Delay::from_saturating_duration(std::time::Duration::from_millis(
        delay_ms.max(10) as u64,
    ));
    for image in frames {
        encoder
            .encode_frame(Frame::from_parts(image.clone(), 0, 0, delay))
            .map_err(|e| PlumeError::Invalid(format!("GIF frame: {e}")))?;
    }
    Ok(())
}

/// Encode frames with the local `ffmpeg` executable.
///
/// Frames are written into a temporary directory next to the destination and
/// removed afterwards. This keeps all file operations inside the project tree
/// when the caller exports there.
pub fn export_mp4(frames: &[RgbaImage], path: &Path, fps: u32) -> Result<()> {
    if frames.is_empty() {
        return Err(PlumeError::Invalid("MP4 needs at least one frame".into()));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let dir = frames_dir(path);
    std::fs::create_dir_all(&dir)?;
    let mut cleanup = Cleanup(dir.clone());
    for (i, frame) in frames.iter().enumerate() {
        let frame_path = dir.join(format!("frame_{i:05}.png"));
        frame.save(&frame_path)?;
    }
    let fps = fps.clamp(1, 240);
    let args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-y".to_string(),
        "-framerate".to_string(),
        fps.to_string(),
        "-i".to_string(),
        dir.join("frame_%05d.png").to_string_lossy().to_string(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        path.to_string_lossy().to_string(),
    ];
    let output = std::process::Command::new(ffmpeg_binary())
        .args(&args)
        .output()
        .map_err(|e| {
            PlumeError::Invalid(format!(
                "could not run ffmpeg (install ffmpeg or export GIF): {e}"
            ))
        })?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(PlumeError::Invalid(format!(
            "ffmpeg failed: {}",
            err.lines().take(4).collect::<Vec<_>>().join(" | ")
        )));
    }
    cleanup.remove();
    Ok(())
}

/// Encode an image sequence supplied one frame at a time. Callers can stream
/// rendered frames without keeping every RGBA image in memory.
pub fn export_mp4_stream<F>(path: &Path, frame_count: u32, fps: u32, mut frame: F) -> Result<()>
where
    F: FnMut(u32) -> Result<RgbaImage>,
{
    if frame_count == 0 {
        return Err(PlumeError::Invalid("MP4 needs at least one frame".into()));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let dir = frames_dir(path);
    std::fs::create_dir_all(&dir)?;
    let mut cleanup = Cleanup(dir.clone());
    for i in 0..frame_count {
        let image = frame(i)?;
        image.save(dir.join(format!("frame_{i:05}.png")))?;
    }
    let args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-y".to_string(),
        "-framerate".to_string(),
        fps.clamp(1, 240).to_string(),
        "-i".to_string(),
        dir.join("frame_%05d.png").to_string_lossy().to_string(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        path.to_string_lossy().to_string(),
    ];
    let output = std::process::Command::new(ffmpeg_binary())
        .args(&args)
        .output()
        .map_err(|e| PlumeError::Invalid(format!("could not run ffmpeg: {e}")))?;
    if !output.status.success() {
        return Err(PlumeError::Invalid(format!(
            "ffmpeg failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    cleanup.remove();
    Ok(())
}

pub fn ffmpeg_available() -> bool {
    std::process::Command::new(ffmpeg_binary())
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn ffmpeg_binary() -> String {
    std::env::var("PLUME_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string())
}

fn frames_dir(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("movie");
    parent.join(format!(".plume_frames_{stem}_{}", std::process::id()))
}

struct Cleanup(PathBuf);

impl Cleanup {
    fn remove(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        // Best-effort cleanup when an encoder returns early.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;
    use image::Rgba;

    fn frames() -> Vec<RgbaImage> {
        (0..3)
            .map(|i| {
                let mut img = RgbaImage::new(64, 48);
                for (x, y, p) in img.enumerate_pixels_mut() {
                    *p = Rgba([(x + i * 10) as u8, y as u8, (i * 40) as u8, 255]);
                }
                img
            })
            .collect()
    }

    #[test]
    fn png_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("still.png");
        export_png(&frames()[0], &path).unwrap();
        let loaded = image::open(&path).unwrap();
        assert_eq!(loaded.dimensions(), (64, 48));
    }

    #[test]
    fn gif_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loop.gif");
        export_gif(&frames(), &path, 80).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"GIF89a") || bytes.starts_with(b"GIF87a"));
        assert!(bytes.len() > 100);
    }

    #[test]
    fn mp4_writes_when_ffmpeg_present() {
        if !ffmpeg_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mp4");
        export_mp4(&frames(), &path, 24).unwrap();
        let data = std::fs::read(&path).unwrap();
        assert!(data.len() > 512);
    }
}
