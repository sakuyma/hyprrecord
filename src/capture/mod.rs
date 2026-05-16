//! Capture pipeline for screenshots and recordings.

use anyhow::{Context, Result, anyhow, bail};
use chrono::Local;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::Command;
use std::time::Duration;

pub mod record;
pub mod screenshot;

pub(super) const SELF_APP_CLASS: &str = "land.hypr.Hyprscreen";

const SOCKET_READ_TIMEOUT_SECS: u64 = 2;
const COMPOSITOR_SYNC_SLEEP_MS: u64 = 50;

pub fn generated_filename(extension: &str) -> String {
    let config = crate::config::get();
    let formatted = catch_unwind(AssertUnwindSafe(|| {
        Local::now().format(&config.timestamp_format).to_string()
    }))
    .ok()
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| Local::now().format("%H%M%S%d%m%Y").to_string());

    format!("{}-{}.{}", config.filename_prefix, formatted, extension)
}

pub(super) fn hyprscreen_temp_dir() -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join("hyprscreen");
    std::fs::create_dir_all(&dir).context("failed to create Hyprscreen temp directory")?;
    Ok(dir)
}

pub(super) fn parse_geometry(geometry: &str) -> Result<(i32, i32, i32, i32)> {
    let (origin, size) = geometry
        .split_once(' ')
        .ok_or_else(|| anyhow!("geometry did not contain a size separator"))?;
    let (x, y) = origin
        .split_once(',')
        .ok_or_else(|| anyhow!("geometry origin was invalid"))?;
    let (width, height) = size
        .split_once('x')
        .ok_or_else(|| anyhow!("geometry size was invalid"))?;
    Ok((x.trim().parse()?, y.trim().parse()?, width.trim().parse()?, height.trim().parse()?))
}

pub(super) fn inset_geometry(geometry: &str, px: i32) -> Option<String> {
    let (x, y, w, h) = parse_geometry(geometry).ok()?;
    let new_w = (w - 2 * px).max(1);
    let new_h = (h - 2 * px).max(1);
    Some(format!("{},{} {}x{}", x + px, y + px, new_w, new_h))
}

/// Returns the inset in logical pixels that removes Hyprland window borders and rounded corners
/// from window captures. Queries `hyprctl getoption` for live values; falls back to 8px (the
/// sum of the common defaults: border_size=2, rounding=6).
pub(super) fn hyprland_window_inset() -> i32 {
    let border_size = hyprland_int_option("general:border_size").unwrap_or(2);
    let rounding = hyprland_int_option("decoration:rounding").unwrap_or(6);
    (border_size + rounding).max(2)
}

fn hyprland_int_option(option: &str) -> Option<i32> {
    let output = Command::new("hyprctl")
        .args(["getoption", option, "-j"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    json.get("int")?.as_i64().map(|v| v as i32)
}

/// Suppresses slurp's close animation and waits until slurp's layer surface is
/// fully gone before a capture runs.
///
/// Hyprland emits `closelayer>>selection` at the *start* of the close animation.
/// We apply `noanim,selection` to suppress the animation, but rather than
/// trusting that rule with a fixed sleep, `wait()` polls `hyprctl layers -j`
/// until the "selection" namespace is absent — the ground truth that the surface
/// is gone regardless of animation state.
pub(super) struct CompositorRepaintGuard {
    stream: Option<UnixStream>,
}

impl CompositorRepaintGuard {
    pub(super) fn arm() -> Self {
        // Re-apply on every capture: `hyprctl keyword` rules are cleared by
        // `hyprctl reload`, so a one-time set would silently disappear.
        let _ = Command::new("hyprctl")
            .args(["keyword", "layerrule", "noanim,selection"])
            .output();

        let stream = (|| {
            let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
            let path = format!("/tmp/hypr/{}/.socket2.sock", sig);
            UnixStream::connect(path).ok()
        })();
        Self { stream }
    }

    pub(super) fn wait(self) {
        if let Some(stream) = self.stream {
            stream.set_read_timeout(Some(Duration::from_secs(SOCKET_READ_TIMEOUT_SECS))).ok();
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                match line {
                    Ok(l) if l.starts_with("closelayer>>selection") => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        }
        wait_for_selection_layer_gone();
    }
}

#[derive(serde::Deserialize)]
struct WindowQueryMonitor {
    id: i32,
    disabled: bool,
    #[serde(rename = "activeWorkspace")]
    active_workspace: WindowQueryWorkspace,
}

#[derive(serde::Deserialize)]
struct WindowQueryWorkspace {
    id: i32,
}

#[derive(serde::Deserialize)]
struct WindowQueryClient {
    mapped: bool,
    hidden: bool,
    class: String,
    title: String,
    monitor: i32,
    workspace: WindowQueryWorkspace,
    at: [i32; 2],
    size: [i32; 2],
}

/// Returns slurp-ready geometry strings for all visible, eligible windows.
pub(super) fn visible_window_geometries() -> Result<Vec<String>> {
    let monitor_out = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .context("failed to query Hyprland monitors")?;
    if !monitor_out.status.success() {
        bail!("hyprctl monitors failed");
    }
    let monitors: Vec<WindowQueryMonitor> = serde_json::from_slice(&monitor_out.stdout)
        .context("failed to parse Hyprland monitors JSON")?;
    let active_workspaces: HashMap<i32, i32> = monitors
        .into_iter()
        .filter(|m| !m.disabled)
        .map(|m| (m.id, m.active_workspace.id))
        .collect();

    let client_out = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .context("failed to query Hyprland clients")?;
    if !client_out.status.success() {
        bail!("hyprctl clients failed");
    }
    let clients: Vec<WindowQueryClient> = serde_json::from_slice(&client_out.stdout)
        .context("failed to parse Hyprland clients JSON")?;

    Ok(clients
        .into_iter()
        .filter(|c| c.mapped && !c.hidden && c.class != SELF_APP_CLASS)
        .filter(|c| c.size[0] > 0 && c.size[1] > 0)
        .filter(|c| active_workspaces.get(&c.monitor).is_some_and(|ws| c.workspace.id == *ws))
        .map(|c| {
            let title = if c.title.is_empty() {
                c.class.clone()
            } else {
                format!("{} - {}", c.class, c.title.replace('\n', " "))
            };
            format!("{},{} {}x{} {}", c.at[0], c.at[1], c.size[0], c.size[1], title)
        })
        .collect())
}

fn wait_for_selection_layer_gone() {
    let deadline = std::time::Instant::now() + Duration::from_millis(1000);
    loop {
        if std::time::Instant::now() >= deadline {
            break;
        }
        let is_gone = Command::new("hyprctl")
            .args(["layers", "-j"])
            .output()
            .ok()
            .map(|o| !String::from_utf8_lossy(&o.stdout).contains("\"selection\""))
            .unwrap_or(false);
        if is_gone {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    std::thread::sleep(Duration::from_millis(COMPOSITOR_SYNC_SLEEP_MS));
}
