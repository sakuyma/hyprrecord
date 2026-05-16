//! Hyprland-specific integration points.

use std::process::Command;
use std::time::Duration;

use gtk::glib;
use serde::Deserialize;

#[derive(Deserialize)]
struct ClientInfo {
    address: String,
    title: String,
}

#[derive(Debug, Clone)]
pub struct Monitor {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Deserialize)]
struct MonitorInfoRaw {
    name: String,
    disabled: bool,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale: f64,
}

pub fn enumerate_monitors() -> Vec<Monitor> {
    let Ok(output) = Command::new("hyprctl").args(["monitors", "-j"]).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(monitors) = serde_json::from_slice::<Vec<MonitorInfoRaw>>(&output.stdout) else {
        return Vec::new();
    };
    monitors
        .into_iter()
        .filter(|m| !m.disabled)
        .map(|m| Monitor {
            name: m.name,
            x: m.x,
            y: m.y,
            width: ((m.width as f64) / m.scale).round() as i32,
            height: ((m.height as f64) / m.scale).round() as i32,
        })
        .collect()
}

fn is_hyprland_session() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

fn dispatch(command: &str) {
    let _ = Command::new("hyprctl")
        .arg("dispatch")
        .args(command.split_whitespace())
        .output();
}

fn dispatch_setprop(selector: &str, property: &str, value: &str) {
    let _ = Command::new("hyprctl")
        .arg("dispatch")
        .arg("setprop")
        .arg(selector)
        .arg(property)
        .arg(value)
        .output();
}

fn dispatch_move_exact(selector: &str, x: i32, y: i32) {
    let _ = Command::new("hyprctl")
        .arg("dispatch")
        .arg("movewindowpixel")
        .arg(format!("exact {x} {y},{selector}"))
        .output();
}

fn dispatch_setfloating(selector: &str) {
    let _ = Command::new("hyprctl")
        .arg("dispatch")
        .arg("setfloating")
        .arg(selector)
        .output();
}

pub fn float_window_once() {
    if !is_hyprland_session() {
        return;
    }

    // Mapping is asynchronous, so apply the floating/centering hint shortly
    // after the main window is presented and once more as a lightweight retry.
    // Target hyprscreen by title — using `active` floats whatever happens to be
    // focused, which in CLI subcommand flow can be the user's terminal.
    let title = "Hyprscreen".to_string();
    for delay in [120_u64, 320_u64] {
        let title = title.clone();
        glib::timeout_add_local_once(Duration::from_millis(delay), move || {
            if let Some(selector) = selector_for_title(&title) {
                dispatch_setfloating(&selector);
                if is_active_window(&title) {
                    dispatch("centerwindow");
                }
            }
        });
    }
}

fn is_active_window(title: &str) -> bool {
    let Ok(output) = Command::new("hyprctl").args(["activewindow", "-j"]).output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let Ok(value): Result<serde_json::Value, _> = serde_json::from_slice(&output.stdout) else {
        return false;
    };
    value.get("title").and_then(|t| t.as_str()) == Some(title)
}

pub fn place_window_exact(window_match: &str, x: i32, y: i32) {
    if !is_hyprland_session() {
        return;
    }

    for delay in [40_u64, 90_u64] {
        let title = window_match.to_string();
        glib::timeout_add_local_once(Duration::from_millis(delay), move || {
            if let Some(selector) = selector_for_title(&title) {
                dispatch_setfloating(&selector);
                dispatch_move_exact(&selector, x, y);
            }
        });
    }
}

pub fn make_window_plain(window_match: &str) {
    if !is_hyprland_session() {
        return;
    }

    for delay in [40_u64, 90_u64, 160_u64] {
        let title = window_match.to_string();
        glib::timeout_add_local_once(Duration::from_millis(delay), move || {
            if let Some(selector) = selector_for_title(&title) {
                dispatch_setprop(&selector, "decorate", "0");
                dispatch_setprop(&selector, "border_size", "0");
                dispatch_setprop(&selector, "no_blur", "1");
                dispatch_setprop(&selector, "no_shadow", "1");
                dispatch_setprop(&selector, "rounding", "0");
                dispatch_setprop(&selector, "no_anim", "1");
            }
        });
    }
}

fn selector_for_title(title: &str) -> Option<String> {
    let output = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let clients: Vec<ClientInfo> = serde_json::from_slice(&output.stdout).ok()?;
    clients
        .into_iter()
        .find(|client| client.title == title)
        .map(|client| format!("address:{}", client.address))
}
