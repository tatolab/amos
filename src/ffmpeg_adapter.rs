use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::adapter::{Adapter, ResourceFields};

/// FFmpeg adapter — extracts keyframes from video, transcodes media for Claude Code.
///
/// URI format: `ffmpeg:path/to/video.mp4`
///
/// For video files: extracts frames at regular intervals, returns as local image paths.
/// For audio files: extracts a waveform image (visual representation).
///
/// Requires `ffmpeg` installed on the system.
pub struct FfmpegAdapter {
    scan_root: PathBuf,
}

impl FfmpegAdapter {
    pub fn new(scan_root: &Path) -> Self {
        FfmpegAdapter {
            scan_root: scan_root.to_path_buf(),
        }
    }

    fn extract_video_frames(&self, video_path: &Path) -> Result<Vec<PathBuf>> {
        let cache_dir = std::env::temp_dir().join("amos-cache").join("ffmpeg");
        std::fs::create_dir_all(&cache_dir).context("creating ffmpeg cache dir")?;

        let stem = video_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("video");

        // Use a hash of the path to namespace frames
        let hash = simple_hash(&video_path.to_string_lossy());
        let frame_dir = cache_dir.join(format!("{}_{}", stem, hash));

        // Skip extraction if already cached
        if frame_dir.exists() {
            return collect_frames(&frame_dir);
        }

        std::fs::create_dir_all(&frame_dir).context("creating frame output dir")?;

        // Get video duration first
        let duration = get_duration(video_path)?;

        // Extract up to 8 frames spread across the video
        let max_frames = 8u32;
        let interval = if duration > 0.0 {
            (duration / max_frames as f64).max(1.0)
        } else {
            2.0
        };

        let output_pattern = frame_dir.join(format!("{}_frame_%03d.png", stem));

        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-i"]);
        cmd.arg(video_path);
        cmd.args([
            "-vf",
            &format!("fps=1/{:.1}", interval),
            "-frames:v",
            &max_frames.to_string(),
            "-y",
        ]);
        cmd.arg(&output_pattern);
        cmd.args(["-loglevel", "error"]);

        let output = cmd.output().context("failed to run ffmpeg")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("ffmpeg failed: {}", stderr.trim());
        }

        collect_frames(&frame_dir)
    }

    fn extract_audio_waveform(&self, audio_path: &Path) -> Result<PathBuf> {
        let cache_dir = std::env::temp_dir().join("amos-cache").join("ffmpeg");
        std::fs::create_dir_all(&cache_dir).context("creating ffmpeg cache dir")?;

        let stem = audio_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("audio");
        let hash = simple_hash(&audio_path.to_string_lossy());
        let waveform_path = cache_dir.join(format!("{}_{}_waveform.png", stem, hash));

        if waveform_path.exists() {
            return Ok(waveform_path);
        }

        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-i"]);
        cmd.arg(audio_path);
        cmd.args([
            "-filter_complex",
            "showwavespic=s=1200x400:colors=#4a9eff",
            "-frames:v",
            "1",
            "-y",
        ]);
        cmd.arg(&waveform_path);
        cmd.args(["-loglevel", "error"]);

        let output = cmd.output().context("failed to run ffmpeg")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("ffmpeg waveform failed: {}", stderr.trim());
        }

        Ok(waveform_path)
    }
}

fn get_duration(path: &Path) -> Result<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .context("failed to run ffprobe")?;

    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(s.parse::<f64>().unwrap_or(30.0))
}

fn collect_frames(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut frames: Vec<PathBuf> = std::fs::read_dir(dir)
        .context("reading frame dir")?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    frames.sort();
    Ok(frames)
}

fn is_video(path: &Path) -> bool {
    let video_extensions = ["mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v"];
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| video_extensions.contains(&ext.to_lowercase().as_str()))
}

fn is_audio(path: &Path) -> bool {
    let audio_extensions = ["mp3", "wav", "flac", "aac", "ogg", "m4a", "wma"];
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| audio_extensions.contains(&ext.to_lowercase().as_str()))
}

fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

impl Adapter for FfmpegAdapter {
    fn scheme(&self) -> &str {
        "ffmpeg"
    }

    fn resolve(&self, reference: &str) -> Result<ResourceFields> {
        let ref_path = Path::new(reference);
        let full_path = if ref_path.is_absolute() {
            ref_path.to_path_buf()
        } else {
            self.scan_root.join(reference)
        };

        if !full_path.exists() {
            bail!("file not found: {}", full_path.display());
        }

        if is_video(&full_path) {
            let frames = self.extract_video_frames(&full_path)?;

            if frames.is_empty() {
                bail!("ffmpeg extracted no frames from {}", reference);
            }

            let mut body = format!(
                "**Video: {}** ({} keyframes extracted)\n\n",
                reference,
                frames.len()
            );
            for (i, frame) in frames.iter().enumerate() {
                let filename = frame
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("frame");
                body.push_str(&format!(
                    "![Frame {} - {}]({})\n\n",
                    i + 1,
                    filename,
                    frame.display()
                ));
            }

            Ok(ResourceFields {
                name: None,
                description: None,
                status: None,
                body: Some(body),
            })
        } else if is_audio(&full_path) {
            let waveform = self.extract_audio_waveform(&full_path)?;
            let filename = full_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("audio");

            Ok(ResourceFields {
                name: None,
                description: None,
                status: None,
                body: Some(format!(
                    "**Audio: {}**\n\n![{} waveform]({})\n",
                    reference, filename, waveform.display()
                )),
            })
        } else {
            bail!(
                "unsupported media type for ffmpeg adapter: {}",
                reference
            );
        }
    }
}
