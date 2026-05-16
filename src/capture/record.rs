use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub struct MonitorPlacement {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone)]
pub enum RevealMethod {
    Configured(String),
    Detected(String),
}

#[derive(Debug, Clone)]
pub enum OpenMethod {
    Configured(String),
    Detected(String),
}

#[derive(Debug, Clone)]
pub struct VideoPreviewInfo {
    pub thumbnail_path: Option<PathBuf>,
    pub metadata_summary: String,
}

#[derive(Debug)]
pub struct RecordingSession {
    pub child: Child,
    pub temp_path: PathBuf,
    pub monitor: MonitorPlacement,
}

#[derive(Debug, Deserialize)]
struct MonitorInfo {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale: f64,
    focused: bool,
    disabled: bool,
}


#[derive(Debug, Serialize, Deserialize)]
struct RecordingStateFile {
    pid: u32,
    temp_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    format: FfprobeFormat,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn state_file_path() -> PathBuf {
    runtime_dir().join("hyprscreen-recording.json")
}

fn temp_recording_path() -> Result<PathBuf> {
    Ok(super::hyprscreen_temp_dir()?.join(crate::capture::generated_filename("mkv")))
}

pub enum RecordingSelection {
    Geometry { geometry: String, monitor: MonitorPlacement, is_window: bool },
    OutputName { name: String, placement: MonitorPlacement },
}

pub fn select_area() -> Result<RecordingSelection> {
    let guard = super::CompositorRepaintGuard::arm();
    let geometry = select_recording_area_geometry()?;
    guard.wait();
    let monitor = monitor_for_geometry(&geometry)?;
    Ok(RecordingSelection::Geometry { geometry, monitor, is_window: false })
}

pub fn select_window() -> Result<RecordingSelection> {
    let windows = super::visible_window_geometries()?;
    if windows.is_empty() {
        bail!("no eligible windows found")
    }
    let guard = super::CompositorRepaintGuard::arm();
    let geometry = select_recording_window_geometry(&windows)?;
    guard.wait();
    let monitor = monitor_for_geometry(&geometry)?;
    Ok(RecordingSelection::Geometry { geometry, monitor, is_window: true })
}

pub fn select_monitor() -> Result<RecordingSelection> {
    let monitors = crate::hyprland::enumerate_monitors();
    if monitors.is_empty() {
        bail!("no eligible monitors found")
    }
    let guard = super::CompositorRepaintGuard::arm();
    let geometry = select_recording_monitor_geometry(&monitors)?;
    guard.wait();
    let (x, y, width, height) = super::parse_geometry(&geometry)?;
    let target = monitors
        .into_iter()
        .find(|m| m.x == x && m.y == y && m.width == width && m.height == height)
        .ok_or_else(|| anyhow!("selected monitor could not be resolved"))?;
    Ok(RecordingSelection::OutputName {
        name: target.name,
        placement: MonitorPlacement { x: target.x, y: target.y, width: target.width, height: target.height },
    })
}

pub fn launch_recording(sel: RecordingSelection) -> Result<RecordingSession> {
    match sel {
        RecordingSelection::Geometry { geometry, monitor, is_window } => {
            let temp_path = temp_recording_path()?;
            let inset_px = if is_window { super::hyprland_window_inset() } else { 2 };
            let inset = super::inset_geometry(&geometry, inset_px).unwrap_or(geometry.clone());
            let child = Command::new("wf-recorder")
                .arg("-g")
                .arg(&inset)
                .arg("-f")
                .arg(&temp_path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("failed to launch wf-recorder")?;
            write_state_file(child.id(), &temp_path)?;
            Ok(RecordingSession { child, temp_path, monitor })
        }
        RecordingSelection::OutputName { name, placement } => {
            let temp_path = temp_recording_path()?;
            let child = Command::new("wf-recorder")
                .arg("-o")
                .arg(&name)
                .arg("-f")
                .arg(&temp_path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("failed to launch wf-recorder")?;
            write_state_file(child.id(), &temp_path)?;
            Ok(RecordingSession { child, temp_path, monitor: placement })
        }
    }
}

pub fn stop_active_recording() -> Result<()> {
    let Ok(state) = read_state_file() else {
        return Ok(());
    };
    stop_direct_recording(state.pid)
}

fn stop_direct_recording(pid: u32) -> Result<()> {
    let status = Command::new("kill")
        .arg("-INT")
        .arg(pid.to_string())
        .status()
        .context("failed to send stop signal to wf-recorder")?;

    if !status.success() {
        bail!("failed to stop active recording")
    }

    Ok(())
}

pub fn clear_state_file() {
    let _ = fs::remove_file(state_file_path());
}


fn select_recording_area_geometry() -> Result<String> {
    let output = Command::new("slurp")
        .args([
            "-b",
            "#00000088",
            "-c",
            "#ff4d4dff",
            "-s",
            "#00000000",
            "-w",
            "3",
            "-d",
        ])
        .output()
        .context("failed to launch slurp")?;

    if !output.status.success() {
        bail!("area selection was cancelled")
    }

    let geometry = String::from_utf8(output.stdout)
        .context("slurp returned non-utf8 geometry")?
        .trim()
        .to_owned();

    if geometry.is_empty() {
        bail!("area selection returned no geometry")
    }

    Ok(geometry)
}

fn monitor_for_geometry(geometry: &str) -> Result<MonitorPlacement> {
    let (x, y, width, height) = super::parse_geometry(geometry)?;
    let center_x = x + (width / 2);
    let center_y = y + (height / 2);

    let output = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .context("failed to query Hyprland monitors")?;

    if !output.status.success() {
        bail!("hyprctl monitors failed")
    }

    let monitors: Vec<MonitorInfo> =
        serde_json::from_slice(&output.stdout).context("failed to parse Hyprland monitors JSON")?;

    let placements = monitors
        .iter()
        .filter(|monitor| !monitor.disabled)
        .map(logical_monitor_placement)
        .collect::<Vec<_>>();

    if let Some(monitor) = placements.iter().copied().find(|monitor| {
        center_x >= monitor.x
            && center_x < monitor.x + monitor.width
            && center_y >= monitor.y
            && center_y < monitor.y + monitor.height
    }) {
        return Ok(monitor);
    }

    monitors
        .iter()
        .find(|monitor| monitor.focused)
        .map(logical_monitor_placement)
        .ok_or_else(|| anyhow!("no suitable monitor found for recording area"))
}

fn select_recording_window_geometry(choices: &[String]) -> Result<String> {
    let mut child = Command::new("slurp")
        .args([
            "-r",
            "-b",
            "#00000088",
            "-c",
            "#ff4d4dff",
            "-s",
            "#00000000",
            "-w",
            "3",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("failed to launch slurp for window selection")?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("failed to open slurp stdin"))?;
    stdin.write_all(choices.join("\n").as_bytes())?;
    drop(stdin);

    let output = child
        .wait_with_output()
        .context("failed while waiting for slurp window selection")?;

    if !output.status.success() {
        bail!("window selection was cancelled")
    }

    let geometry = String::from_utf8(output.stdout)
        .context("slurp returned non-utf8 geometry")?
        .trim()
        .to_owned();

    if geometry.is_empty() {
        bail!("window selection returned no geometry")
    }

    Ok(geometry)
}

fn logical_monitor_placement(monitor: &MonitorInfo) -> MonitorPlacement {
    MonitorPlacement {
        x: monitor.x,
        y: monitor.y,
        width: ((monitor.width as f64) / monitor.scale).round() as i32,
        height: ((monitor.height as f64) / monitor.scale).round() as i32,
    }
}

fn select_recording_monitor_geometry(monitors: &[crate::hyprland::Monitor]) -> Result<String> {
    let choices = monitors
        .iter()
        .map(|m| format!("{},{} {}x{}", m.x, m.y, m.width, m.height))
        .collect::<Vec<_>>();

    let mut child = Command::new("slurp")
        .args([
            "-r",
            "-b",
            "#00000088",
            "-c",
            "#ff4d4dff",
            "-s",
            "#00000000",
            "-w",
            "8",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("failed to launch slurp for monitor selection")?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("failed to open slurp stdin"))?;
    stdin.write_all(choices.join("\n").as_bytes())?;
    drop(stdin);

    let output = child
        .wait_with_output()
        .context("failed while waiting for slurp monitor selection")?;

    if !output.status.success() {
        bail!("monitor selection was cancelled")
    }

    let geometry = String::from_utf8(output.stdout)
        .context("slurp returned non-utf8 geometry")?
        .trim()
        .to_owned();

    if geometry.is_empty() {
        bail!("monitor selection returned no geometry")
    }

    Ok(geometry)
}



fn write_state_file(pid: u32, temp_path: &Path) -> Result<()> {
    let state = RecordingStateFile {
        pid,
        temp_path: temp_path.to_path_buf(),
    };
    let bytes = serde_json::to_vec(&state).context("failed to serialize recording state")?;
    fs::write(state_file_path(), bytes).context("failed to write recording state file")?;
    Ok(())
}

fn read_state_file() -> Result<RecordingStateFile> {
    let bytes = fs::read(state_file_path()).context("no active recording state file found")?;
    serde_json::from_slice(&bytes).context("failed to parse recording state file")
}

pub fn reveal_in_file_manager(path: &Path) -> Result<RevealMethod> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("recording path has no parent directory"))?;

    if let Some(command) = crate::config::get().reveal_folder_command.as_deref() {
        launch_reveal_command(command, parent)
            .with_context(|| format!("failed to launch configured reveal command `{command}`"))?;
        return Ok(RevealMethod::Configured(command.to_string()));
    }

    for command in ["thunar", "dolphin", "nautilus", "pcmanfm"] {
        if launch_reveal_command(command, parent).is_ok() {
            return Ok(RevealMethod::Detected(command.to_string()));
        }
    }

    bail!(
        "no file manager could be launched; set reveal_folder_command in ~/.config/hyprscreen.conf"
    )
}

impl RevealMethod {
    pub fn feedback_message(&self) -> String {
        match self {
            RevealMethod::Configured(command) => {
                format!("Opened with config command: {command}")
            }
            RevealMethod::Detected(command) => format!("Opened with {command}"),
        }
    }
}

impl OpenMethod {
    pub fn feedback_message(&self) -> String {
        match self {
            OpenMethod::Configured(command) => format!("Opened with config command: {command}"),
            OpenMethod::Detected(command) => format!("Opened with {command}"),
        }
    }
}

pub fn open_video_file(path: &Path) -> Result<OpenMethod> {
    if let Some(command) = crate::config::get().open_video_command.as_deref() {
        launch_open_command(command, path)
            .with_context(|| format!("failed to launch configured open command `{command}`"))?;
        return Ok(OpenMethod::Configured(command.to_string()));
    }

    for command in ["mpv", "vlc", "celluloid"] {
        if launch_open_command(command, path).is_ok() {
            return Ok(OpenMethod::Detected(command.to_string()));
        }
    }

    bail!("no video player could be launched; set open_video_command in ~/.config/hyprscreen.conf")
}

pub fn build_video_preview_info(path: &Path) -> Result<VideoPreviewInfo> {
    let metadata = probe_video_metadata(path)?;
    let thumbnail_path = generate_video_thumbnail(path).ok();
    let file_size_bytes = fs::metadata(path).ok().map(|metadata| metadata.len());

    Ok(VideoPreviewInfo {
        thumbnail_path,
        metadata_summary: format_video_metadata(&metadata, file_size_bytes),
    })
}

fn launch_reveal_command(command: &str, parent: &std::path::Path) -> Result<()> {
    Command::new(command)
        .arg(parent)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch reveal command `{command}`"))?;

    Ok(())
}

fn launch_open_command(command: &str, path: &std::path::Path) -> Result<()> {
    Command::new(command)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch open command `{command}`"))?;

    Ok(())
}

fn probe_video_metadata(path: &Path) -> Result<(Option<f64>, Option<u32>, Option<u32>)> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .context("failed to launch ffprobe")?;

    if !output.status.success() {
        bail!("ffprobe failed to inspect the recording")
    }

    let parsed: FfprobeOutput =
        serde_json::from_slice(&output.stdout).context("failed to parse ffprobe JSON")?;
    let stream = parsed.streams.first();
    let width = stream.and_then(|stream| stream.width);
    let height = stream.and_then(|stream| stream.height);
    let duration = parsed
        .format
        .duration
        .as_deref()
        .and_then(|value| value.parse::<f64>().ok());

    Ok((duration, width, height))
}

fn generate_video_thumbnail(path: &Path) -> Result<PathBuf> {
    let thumbnail_path = super::hyprscreen_temp_dir()?.join(format!(
        "{}-thumb.png",
        crate::capture::generated_filename("video-preview")
    ));

    let status = Command::new("ffmpeg")
        .args(["-y", "-ss", "1", "-i"])
        .arg(path)
        .args(["-frames:v", "1"])
        .arg(&thumbnail_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to launch ffmpeg for thumbnail generation")?;

    if !status.success() {
        bail!("ffmpeg failed to generate a thumbnail")
    }

    Ok(thumbnail_path)
}

fn format_video_metadata(
    metadata: &(Option<f64>, Option<u32>, Option<u32>),
    file_size_bytes: Option<u64>,
) -> String {
    let duration = metadata
        .0
        .map(format_duration)
        .unwrap_or_else(|| "unknown length".to_string());

    let resolution = match (metadata.1, metadata.2) {
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "unknown size".to_string(),
    };

    let file_size = file_size_bytes
        .map(format_file_size)
        .unwrap_or_else(|| "unknown file size".to_string());

    format!("Temporary recording · {duration} · {resolution} · {file_size}")
}

fn format_duration(duration: f64) -> String {
    let total_seconds = duration.round().max(0.0) as u64;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn format_file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes / KB)
    } else {
        format!("{} B", bytes as u64)
    }
}
