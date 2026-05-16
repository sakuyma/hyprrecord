use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DefaultMode {
    Screenshot,
    Record,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DefaultTarget {
    Area,
    Window,
    Monitor,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub default_mode: DefaultMode,
    pub default_target: DefaultTarget,
    pub show_recording_hud: bool,
    pub recording_indicator_enabled: bool,
    pub recording_indicator_interval_seconds: u64,
    pub recording_indicator_duration_ms: u64,
    pub save_dir_screenshots: PathBuf,
    pub save_dir_recordings: PathBuf,
    pub open_video_command: Option<String>,
    pub reveal_folder_command: Option<String>,
    pub filename_prefix: String,
    pub timestamp_format: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_mode: DefaultMode::Screenshot,
            default_target: DefaultTarget::Area,
            show_recording_hud: true,
            recording_indicator_enabled: true,
            recording_indicator_interval_seconds: 5,
            recording_indicator_duration_ms: 300,
            save_dir_screenshots: expand_home("~/Pictures/Screenshots"),
            save_dir_recordings: expand_home("~/Videos/Recordings"),
            open_video_command: None,
            reveal_folder_command: None,
            filename_prefix: "hyprscreen".to_string(),
            timestamp_format: "%H%M%S%d%m%Y".to_string(),
        }
    }
}

static CONFIG: OnceLock<AppConfig> = OnceLock::new();

pub fn get() -> &'static AppConfig {
    CONFIG.get_or_init(load)
}

fn load() -> AppConfig {
    let defaults = AppConfig::default();
    let path = config_path();
    let Ok(contents) = fs::read_to_string(path) else {
        return defaults;
    };

    let pairs = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect::<HashMap<_, _>>();

    AppConfig {
        default_mode: parse_default_mode(pairs.get("default_mode"))
            .unwrap_or(defaults.default_mode),
        default_target: parse_default_target(pairs.get("default_target"))
            .unwrap_or(defaults.default_target),
        show_recording_hud: parse_bool(pairs.get("show_recording_hud"))
            .unwrap_or(defaults.show_recording_hud),
        recording_indicator_enabled: parse_bool(pairs.get("recording_indicator_enabled"))
            .unwrap_or(defaults.recording_indicator_enabled),
        recording_indicator_interval_seconds: parse_positive_u64(
            pairs.get("recording_indicator_interval_seconds"),
        )
        .unwrap_or(defaults.recording_indicator_interval_seconds),
        recording_indicator_duration_ms: parse_positive_u64(
            pairs.get("recording_indicator_duration_ms"),
        )
        .unwrap_or(defaults.recording_indicator_duration_ms),
        save_dir_screenshots: pairs
            .get("save_dir_screenshots")
            .filter(|value| !value.is_empty())
            .map(|value| expand_home(value))
            .unwrap_or(defaults.save_dir_screenshots),
        save_dir_recordings: pairs
            .get("save_dir_recordings")
            .filter(|value| !value.is_empty())
            .map(|value| expand_home(value))
            .unwrap_or(defaults.save_dir_recordings),
        open_video_command: pairs
            .get("open_video_command")
            .filter(|value| !value.is_empty())
            .cloned(),
        reveal_folder_command: pairs
            .get("reveal_folder_command")
            .filter(|value| !value.is_empty())
            .cloned(),
        filename_prefix: pairs
            .get("filename_prefix")
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or(defaults.filename_prefix),
        timestamp_format: pairs
            .get("timestamp_format")
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or(defaults.timestamp_format),
    }
}

fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));

    base.join("hyprscreen").join("hyprscreen.conf")
}

fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return home_dir();
    }

    if let Some(stripped) = value.strip_prefix("~/") {
        return home_dir().join(stripped);
    }

    PathBuf::from(value)
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn parse_default_mode(value: Option<&String>) -> Option<DefaultMode> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "screenshot" => Some(DefaultMode::Screenshot),
        "record" | "recording" => Some(DefaultMode::Record),
        _ => None,
    }
}

fn parse_default_target(value: Option<&String>) -> Option<DefaultTarget> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "area" => Some(DefaultTarget::Area),
        "window" => Some(DefaultTarget::Window),
        "monitor" => Some(DefaultTarget::Monitor),
        _ => None,
    }
}

fn parse_bool(value: Option<&String>) -> Option<bool> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_positive_u64(value: Option<&String>) -> Option<u64> {
    let parsed = value?.trim().parse::<u64>().ok()?;
    (parsed > 0).then_some(parsed)
}
