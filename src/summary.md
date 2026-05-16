
# ui/mod.rs

```
pub mod main_window;

```

# ui/main_window.rs

```
use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

const WINDOW_WIDTH: i32 = 400;
const INITIAL_HIDE_DELAY_MS: u64 = 60;
const MONITOR_OVERLAY_EXTRA_DELAY_MS: u64 = 220;
const SELECTION_POLL_INTERVAL_MS: u64 = 50;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Screenshot,
    Record,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Target {
    Area,
    Window,
    Monitor,
}

impl Target {
    fn name(self) -> &'static str {
        match self {
            Self::Area => "area",
            Self::Window => "window",
            Self::Monitor => "monitor",
        }
    }
}

#[derive(Debug, Default)]
struct PreviewState {
    temp_path: Option<PathBuf>,
    current_path: Option<PathBuf>,
    thumbnail_path: Option<PathBuf>,
    kind: PreviewKind,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum PreviewKind {
    #[default]
    Screenshot,
    Recording,
}

#[derive(Clone, Copy, Debug)]
struct LastAction {
    mode: Mode,
    target: Target,
    show_recording_hud: bool,
}

struct ActiveRecording {
    child: Child,
    temp_path: PathBuf,
    hud_window: Option<gtk::Window>,
    indicator_window: Option<gtk::Window>,
    started_at: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StatusKind {
    Neutral,
    Error,
    Success,
    Live,
}


pub fn build(
    app: &gtk::Application,
    startup: Option<crate::cli::StartupAction>,
) -> gtk::ApplicationWindow {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Hyprscreen")
        .default_width(WINDOW_WIDTH)
        .resizable(false)
        .build();
    window.set_decorated(false);
    window.connect_map(|_| crate::hyprland::float_window_once());

    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .hhomogeneous(true)
        .vhomogeneous(false)
        .build();

    let preview_state = Rc::new(RefCell::new(PreviewState::default()));
    let last_action = Rc::new(RefCell::new(None::<LastAction>));
    let recording_state = Rc::new(RefCell::new(None::<ActiveRecording>));
    let setup_cta = Rc::new(RefCell::new(None::<gtk::Button>));
    let show_recording_hud = Rc::new(RefCell::new(crate::config::get().show_recording_hud));

    let preview_picture = gtk::Picture::builder()
        .can_shrink(true)
        .hexpand(true)
        .vexpand(true)
        .build();

    let preview_meta_label = gtk::Label::builder()
        .halign(gtk::Align::Center)
        .hexpand(true)
        .wrap(true)
        .css_classes(["hs-meta"])
        .build();

    let preview_status_label = gtk::Label::builder()
        .halign(gtk::Align::Center)
        .hexpand(true)
        .wrap(true)
        .css_classes(["hs-status"])
        .build();

    let setup_status_label = gtk::Label::builder()
        .label("")
        .halign(gtk::Align::Center)
        .css_classes(["hs-status"])
        .build();

    let save_button = gtk::Button::new();
    save_button.add_css_class("hs-abtn");
    save_button.add_css_class("is-primary");
    save_button.add_css_class("mode-shot");
    save_button.set_sensitive(false);
    set_action_button_content(&save_button, "save", "Save");

    let copy_button = gtk::Button::new();
    copy_button.add_css_class("hs-abtn");
    copy_button.set_sensitive(false);
    set_action_button_content(&copy_button, "copy", "Copy");

    let reveal_button = gtk::Button::new();
    reveal_button.add_css_class("hs-abtn");
    reveal_button.set_sensitive(false);
    set_action_button_content(&reveal_button, "reveal", "Reveal");

    let setup_page = build_setup_page(
        &window,
        &stack,
        &preview_state,
        &last_action,
        &recording_state,
        &setup_cta,
        &show_recording_hud,
        &preview_picture,
        &preview_meta_label,
        &preview_status_label,
        &setup_status_label,
        &save_button,
        &copy_button,
        &reveal_button,
        startup,
    );
    stack.add_named(&setup_page, Some("setup"));

    let preview_page = build_preview_page(
        &window,
        &stack,
        &preview_state,
        &last_action,
        &recording_state,
        &setup_cta,
        &preview_picture,
        &preview_meta_label,
        &preview_status_label,
        &setup_status_label,
        &save_button,
        &copy_button,
        &reveal_button,
    );
    stack.add_named(&preview_page, Some("preview"));
    stack.set_visible_child_name("setup");

    window.set_child(Some(&stack));

    window
}

#[allow(clippy::too_many_arguments)]
fn build_setup_page(
    window: &gtk::ApplicationWindow,
    stack: &gtk::Stack,
    preview_state: &Rc<RefCell<PreviewState>>,
    last_action: &Rc<RefCell<Option<LastAction>>>,
    recording_state: &Rc<RefCell<Option<ActiveRecording>>>,
    setup_cta: &Rc<RefCell<Option<gtk::Button>>>,
    show_recording_hud: &Rc<RefCell<bool>>,
    preview_picture: &gtk::Picture,
    preview_meta_label: &gtk::Label,
    preview_status_label: &gtk::Label,
    status_label: &gtk::Label,
    save_button: &gtk::Button,
    copy_button: &gtk::Button,
    reveal_button: &gtk::Button,
    startup: Option<crate::cli::StartupAction>,
) -> gtk::Widget {
    let config = crate::config::get();

    let default_is_record = config.default_mode == crate::config::DefaultMode::Record;

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .css_classes(["hs-body"])
        .build();

    // ── Mode segmented toggle ──────────────────────────────────
    let seg = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .homogeneous(true)
        .css_classes(["hs-seg"])
        .build();

    let screenshot_button = gtk::ToggleButton::new();
    screenshot_button.set_active(!default_is_record);
    let shot_seg_label = gtk::Label::builder()
        .label("Screenshot")
        .css_classes(["hs-seg-label"])
        .build();
    screenshot_button.set_child(Some(&shot_seg_label));

    let record_button = gtk::ToggleButton::new();
    record_button.set_active(default_is_record);
    let rec_seg_label = gtk::Label::builder()
        .label("Record")
        .css_classes(["hs-seg-label"])
        .build();
    record_button.set_child(Some(&rec_seg_label));

    seg.append(&screenshot_button);
    seg.append(&record_button);

    // ── Target row ─────────────────────────────────────────────
    let target_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .homogeneous(true)
        .build();

    let initial_mode_class = if default_is_record {
        "mode-rec"
    } else {
        "mode-shot"
    };

    let area_button = make_target_button("Area", "area", initial_mode_class);
    area_button.set_active(config.default_target == crate::config::DefaultTarget::Area);

    let window_button = make_target_button("Window", "window", initial_mode_class);
    window_button.set_active(config.default_target == crate::config::DefaultTarget::Window);

    let monitor_button = make_target_button("Monitor", "monitor", initial_mode_class);
    monitor_button.set_active(config.default_target == crate::config::DefaultTarget::Monitor);

    target_row.append(&area_button);
    target_row.append(&window_button);
    target_row.append(&monitor_button);

    let hud_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(gtk::Align::Fill)
        .css_classes(["hs-optrow"])
        .build();
    if *show_recording_hud.borrow() {
        hud_row.add_css_class("is-on");
    }

    let hud_label = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(7)
        .valign(gtk::Align::Center)
        .css_classes(["hs-opt-label"])
        .build();
    let hud_dot = gtk::Box::builder()
        .css_classes(["hs-opt-dot"])
        .valign(gtk::Align::Center)
        .build();
    let hud_text = gtk::Label::builder()
        .label("Show HUD while recording")
        .build();
    hud_label.append(&hud_dot);
    hud_label.append(&hud_text);

    let hud_right = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    let hud_hint = gtk::Label::builder()
        .label(if *show_recording_hud.borrow() {
            "on"
        } else {
            "flash"
        })
        .css_classes(["hs-opt-hint"])
        .build();
    let hud_toggle = gtk::Switch::builder()
        .active(*show_recording_hud.borrow())
        .valign(gtk::Align::Center)
        .css_classes(["hs-switch"])
        .build();
    hud_toggle.set_can_focus(false);
    hud_right.append(&hud_hint);
    hud_right.append(&hud_toggle);
    hud_row.append(&hud_label);
    hud_row.append(&hud_right);

    // ── Primary CTA ────────────────────────────────────────────
    let cta_button = gtk::Button::builder()
        .hexpand(true)
        .css_classes(["hs-primary"])
        .build();
    set_primary_button_content(
        &cta_button,
        if default_is_record {
            Mode::Record
        } else {
            Mode::Screenshot
        },
    );

    if default_is_record {
        cta_button.add_css_class("mode-rec");
    } else {
        cta_button.add_css_class("mode-shot");
    }
    *setup_cta.borrow_mut() = Some(cta_button.clone());

    // ── Mode toggle handlers ───────────────────────────────────
    let target_buttons = [
        area_button.clone(),
        window_button.clone(),
        monitor_button.clone(),
    ];

    screenshot_button.connect_toggled(glib::clone!(
        #[weak]
        screenshot_button,
        #[weak]
        record_button,
        #[weak]
        cta_button,
        #[weak]
        status_label,
        #[weak]
        area_button,
        #[weak]
        window_button,
        #[weak]
        monitor_button,
        #[weak]
        hud_row,
        move |_| {
            if !screenshot_button.is_active() && !record_button.is_active() {
                screenshot_button.set_active(true);
            }
            if screenshot_button.is_active() {
                record_button.set_active(false);
                set_primary_button_content(&cta_button, Mode::Screenshot);
                cta_button.remove_css_class("mode-rec");
                cta_button.add_css_class("mode-shot");
                for btn in [&area_button, &window_button, &monitor_button] {
                    btn.remove_css_class("mode-rec");
                    btn.add_css_class("mode-shot");
                }
                hud_row.set_visible(false);
                set_status_neutral(&status_label, "");
            }
        }
    ));

    record_button.connect_toggled(glib::clone!(
        #[weak]
        screenshot_button,
        #[weak]
        record_button,
        #[weak]
        cta_button,
        #[weak]
        status_label,
        #[weak]
        area_button,
        #[weak]
        window_button,
        #[weak]
        monitor_button,
        #[weak]
        hud_row,
        move |_| {
            if !screenshot_button.is_active() && !record_button.is_active() {
                record_button.set_active(true);
            }
            if record_button.is_active() {
                screenshot_button.set_active(false);
                set_primary_button_content(&cta_button, Mode::Record);
                cta_button.remove_css_class("mode-shot");
                cta_button.add_css_class("mode-rec");
                for btn in [&area_button, &window_button, &monitor_button] {
                    btn.remove_css_class("mode-shot");
                    btn.add_css_class("mode-rec");
                }
                hud_row.set_visible(true);
                set_status_neutral(&status_label, "");
            }
        }
    ));

    hud_toggle.connect_active_notify(glib::clone!(
        #[weak]
        hud_hint,
        #[weak]
        hud_row,
        #[weak]
        status_label,
        #[strong]
        show_recording_hud,
        move |switch| {
            let enabled = switch.is_active();
            *show_recording_hud.borrow_mut() = enabled;
            hud_hint.set_label(if enabled { "on" } else { "flash" });
            if enabled {
                hud_row.add_css_class("is-on");
                set_status_neutral(&status_label, "");
            } else {
                hud_row.remove_css_class("is-on");
                set_status_stop_hint(&status_label);
            }
        }
    ));

    // ── Target mutual-exclusion ────────────────────────────────
    for current in &target_buttons {
        let all = target_buttons.clone();
        current.connect_toggled(move |button| {
            if button.is_active() {
                for other in &all {
                    if other != button {
                        other.set_active(false);
                    }
                }
            } else if !all.iter().any(|b| b.is_active()) {
                button.set_active(true);
            }
        });
    }

    // ── CTA click ──────────────────────────────────────────────
    cta_button.connect_clicked(glib::clone!(
        #[weak]
        screenshot_button,
        #[weak]
        area_button,
        #[weak]
        window_button,
        #[weak]
        status_label,
        #[weak]
        cta_button,
        #[weak]
        window,
        #[weak]
        stack,
        #[weak]
        preview_picture,
        #[weak]
        preview_meta_label,
        #[weak]
        preview_status_label,
        #[weak]
        save_button,
        #[weak]
        copy_button,
        #[weak]
        reveal_button,
        #[strong]
        preview_state,
        #[strong]
        last_action,
        #[strong]
        recording_state,
        #[strong]
        setup_cta,
        #[strong]
        show_recording_hud,
        move |_| {
            let target = active_target(&area_button, &window_button);

            if screenshot_button.is_active() {
                set_status_live(&status_label, &format!("selecting {}...", target.name()));
                *last_action.borrow_mut() = Some(LastAction {
                    mode: Mode::Screenshot,
                    target,
                    show_recording_hud: false,
                });
                run_capture_action(
                    &window,
                    &stack,
                    &preview_state,
                    &preview_picture,
                    &preview_meta_label,
                    &preview_status_label,
                    &save_button,
                    &copy_button,
                    &reveal_button,
                    target,
                    Some((&cta_button, &status_label)),
                );
                return;
            }

            let show_hud = *show_recording_hud.borrow();
            if show_hud {
                set_status_live(&status_label, &format!("recording {}...", target.name()));
            } else {
                set_status_stop_hint(&status_label);
            }
            *last_action.borrow_mut() = Some(LastAction {
                mode: Mode::Record,
                target,
                show_recording_hud: show_hud,
            });

            start_recording_action(
                &window,
                &stack,
                &preview_state,
                &recording_state,
                &setup_cta,
                &preview_picture,
                &preview_meta_label,
                &preview_status_label,
                &status_label,
                &save_button,
                &copy_button,
                &reveal_button,
                target,
                show_hud,
                Some((&cta_button, &status_label)),
            );
        }
    ));

    body.append(&seg);
    body.append(&target_row);
    body.append(&hud_row);
    body.append(&cta_button);
    body.append(status_label);

    hud_row.set_visible(default_is_record);

    if let Some(action) = startup {
        let screenshot_btn = screenshot_button.clone();
        let record_btn = record_button.clone();
        let area_btn = area_button.clone();
        let window_btn = window_button.clone();
        let monitor_btn = monitor_button.clone();
        let cta = cta_button.clone();
        glib::timeout_add_local_once(Duration::from_millis(120), move || {
            match action {
                crate::cli::StartupAction::Screenshot(target) => {
                    if !screenshot_btn.is_active() {
                        screenshot_btn.set_active(true);
                    }
                    apply_startup_target(target, &area_btn, &window_btn, &monitor_btn);
                }
                crate::cli::StartupAction::Record(target) => {
                    if !record_btn.is_active() {
                        record_btn.set_active(true);
                    }
                    apply_startup_target(target, &area_btn, &window_btn, &monitor_btn);
                }
            }
            cta.emit_clicked();
        });
    }

    body.upcast()
}

fn apply_startup_target(
    target: crate::cli::StartupTarget,
    area: &gtk::ToggleButton,
    window: &gtk::ToggleButton,
    monitor: &gtk::ToggleButton,
) {
    let to_activate = match target {
        crate::cli::StartupTarget::Area => area,
        crate::cli::StartupTarget::Window => window,
        crate::cli::StartupTarget::Monitor => monitor,
    };
    if !to_activate.is_active() {
        to_activate.set_active(true);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_preview_page(
    window: &gtk::ApplicationWindow,
    stack: &gtk::Stack,
    preview_state: &Rc<RefCell<PreviewState>>,
    last_action: &Rc<RefCell<Option<LastAction>>>,
    recording_state: &Rc<RefCell<Option<ActiveRecording>>>,
    setup_cta: &Rc<RefCell<Option<gtk::Button>>>,
    preview_picture: &gtk::Picture,
    preview_meta_label: &gtk::Label,
    preview_status_label: &gtk::Label,
    setup_status_label: &gtk::Label,
    save_button: &gtk::Button,
    copy_button: &gtk::Button,
    reveal_button: &gtk::Button,
) -> gtk::Widget {
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .css_classes(["hs-body"])
        .build();

    // ── Preview frame ──────────────────────────────────────────
    let preview_aspect = gtk::AspectFrame::builder()
        .xalign(0.5)
        .yalign(0.5)
        .ratio(16.0 / 10.0)
        .obey_child(false)
        .hexpand(true)
        .css_classes(["hs-preview-frame"])
        .build();
    preview_aspect.set_child(Some(preview_picture));

    preview_meta_label.set_halign(gtk::Align::Start);
    preview_meta_label.set_hexpand(true);
    preview_status_label.set_halign(gtk::Align::Center);
    preview_status_label.set_hexpand(true);

    // ── Action row — 5 buttons ─────────────────────────────────
    let back_button = gtk::Button::new();
    back_button.add_css_class("hs-abtn");
    set_action_button_content(&back_button, "back", "Back");

    let new_button = gtk::Button::new();
    new_button.add_css_class("hs-abtn");
    set_action_button_content(&new_button, "refresh", "New");

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .homogeneous(true)
        .hexpand(true)
        .build();

    actions.append(&back_button);
    actions.append(&new_button);
    actions.append(save_button);
    actions.append(copy_button);
    actions.append(reveal_button);

    // ── Back ───────────────────────────────────────────────────
    back_button.connect_clicked(glib::clone!(
        #[weak]
        stack,
        #[weak]
        window,
        #[weak]
        preview_picture,
        #[weak]
        preview_meta_label,
        #[weak]
        preview_status_label,
        #[weak]
        setup_status_label,
        #[weak]
        save_button,
        #[weak]
        copy_button,
        #[weak]
        reveal_button,
        #[strong]
        preview_state,
        #[strong]
        setup_cta,
        move |_| {
            clear_preview(
                &preview_state,
                &preview_picture,
                &preview_meta_label,
                &preview_status_label,
                &save_button,
                &copy_button,
                &reveal_button,
            );
            enable_setup_cta(&setup_cta);
            set_status_neutral(&setup_status_label, "");
            stack.set_visible_child_name("setup");
            window.present();
        }
    ));

    // ── New ────────────────────────────────────────────────────
    new_button.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        stack,
        #[weak]
        preview_picture,
        #[weak]
        preview_meta_label,
        #[weak]
        preview_status_label,
        #[weak]
        setup_status_label,
        #[weak]
        save_button,
        #[weak]
        copy_button,
        #[weak]
        reveal_button,
        #[strong]
        preview_state,
        #[strong]
        last_action,
        #[strong]
        recording_state,
        #[strong]
        setup_cta,
        move |_| {
            let Some(action) = *last_action.borrow() else {
                stack.set_visible_child_name("setup");
                return;
            };

            match action.mode {
                Mode::Screenshot => run_capture_action(
                    &window,
                    &stack,
                    &preview_state,
                    &preview_picture,
                    &preview_meta_label,
                    &preview_status_label,
                    &save_button,
                    &copy_button,
                    &reveal_button,
                    action.target,
                    None,
                ),
                Mode::Record => start_recording_action(
                    &window,
                    &stack,
                    &preview_state,
                    &recording_state,
                    &setup_cta,
                    &preview_picture,
                    &preview_meta_label,
                    &preview_status_label,
                    &setup_status_label,
                    &save_button,
                    &copy_button,
                    &reveal_button,
                    action.target,
                    action.show_recording_hud,
                    None,
                ),
            }
        }
    ));

    // ── Copy / Open ────────────────────────────────────────────
    copy_button.connect_clicked(glib::clone!(
        #[weak]
        preview_status_label,
        #[strong]
        preview_state,
        move |_| match preview_state.borrow().kind {
            PreviewKind::Screenshot => {
                match copy_preview_to_clipboard(&preview_state.borrow().current_path) {
                    Ok(()) => set_status_ok(&preview_status_label, "copied to clipboard"),
                    Err(error) => {
                        set_status_err(&preview_status_label, &format!("Copy failed: {error}"))
                    }
                }
            }
            PreviewKind::Recording => {
                match open_preview_file(&preview_state.borrow()) {
                    Ok(method) => set_status_ok(&preview_status_label, &method.feedback_message()),
                    Err(error) => {
                        set_status_err(&preview_status_label, &format!("Open failed: {error}"))
                    }
                }
            }
        }
    ));

    // ── Reveal ─────────────────────────────────────────────────
    reveal_button.connect_clicked(glib::clone!(
        #[weak]
        preview_status_label,
        #[strong]
        preview_state,
        move |_| match reveal_preview_file(&preview_state.borrow()) {
            Ok(method) => set_status_ok(&preview_status_label, &method.feedback_message()),
            Err(error) => set_status_err(&preview_status_label, &format!("Reveal failed: {error}")),
        }
    ));

    // ── Save ───────────────────────────────────────────────────
    save_button.connect_clicked(glib::clone!(
        #[weak]
        preview_status_label,
        #[weak]
        copy_button,
        #[weak]
        reveal_button,
        #[strong]
        preview_state,
        move |_| {
            let mut preview = preview_state.borrow_mut();
            let preview_kind = preview.kind;

            match save_preview_file(&mut preview) {
                Ok(path) => {
                    let can_reveal = preview.current_path.is_some();
                    drop(preview);

                    if preview_kind == PreviewKind::Recording {
                        copy_button.set_sensitive(true);
                        set_action_button_content(&copy_button, "open", "Open");
                    }

                    if can_reveal {
                        reveal_button.set_sensitive(true);
                    }
                    set_status_ok(
                        &preview_status_label,
                        &format!("saved → {}", path.display()),
                    );
                }
                Err(error) => {
                    drop(preview);
                    set_status_err(&preview_status_label, &format!("Save failed: {error}"));
                }
            }
        }
    ));

    body.append(&preview_aspect);
    body.append(preview_meta_label);
    body.append(&actions);
    body.append(preview_status_label);

    body.upcast()
}

fn make_target_button(label_text: &str, icon_name: &str, mode_class: &str) -> gtk::ToggleButton {
    let btn = gtk::ToggleButton::new();
    btn.add_css_class("hs-tbtn");
    btn.add_css_class(mode_class);

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(7)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .margin_top(14)
        .margin_bottom(10)
        .margin_start(8)
        .margin_end(8)
        .build();

    let img = icon_image(icon_name, 22, Some("hs-tbtn-icon"));

    let lbl = gtk::Label::builder()
        .label(label_text)
        .css_classes(["hs-tbtn-label"])
        .build();

    inner.append(&img);
    inner.append(&lbl);
    btn.set_child(Some(&inner));
    btn
}

fn load_preview_image(
    path: &Path,
    preview_picture: &gtk::Picture,
    preview_meta_label: &gtk::Label,
) {
    let file = gio::File::for_path(path);
    preview_picture.set_file(Some(&file));
    set_preview_meta(
        preview_meta_label,
        &format!("{}", path.file_name().unwrap_or_default().to_string_lossy()),
    );
}

fn load_preview_recording(
    path: &Path,
    preview_state: &Rc<RefCell<PreviewState>>,
    preview_picture: &gtk::Picture,
    preview_meta_label: &gtk::Label,
    copy_button: &gtk::Button,
    reveal_button: &gtk::Button,
) {
    let preview_info = crate::capture::record::build_video_preview_info(path).ok();

    {
        let mut preview = preview_state.borrow_mut();
        preview.kind = PreviewKind::Recording;
        preview.current_path = Some(path.to_path_buf());
        preview.thumbnail_path = preview_info
            .as_ref()
            .and_then(|info| info.thumbnail_path.clone());
    }

    if let Some(thumbnail_path) = preview_info
        .as_ref()
        .and_then(|info| info.thumbnail_path.as_ref())
    {
        let file = gio::File::for_path(thumbnail_path);
        preview_picture.set_file(Some(&file));
    } else {
        preview_picture.set_file(Option::<&gio::File>::None);
    }

    set_action_button_content(copy_button, "open", "Open");
    copy_button.set_sensitive(false);
    reveal_button.set_sensitive(false);

    if let Some(info) = preview_info {
        set_preview_meta(preview_meta_label, &info.metadata_summary);
    } else {
        set_preview_meta(
            preview_meta_label,
            &format!("{}", path.file_name().unwrap_or_default().to_string_lossy()),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn run_capture_action(
    window: &gtk::ApplicationWindow,
    stack: &gtk::Stack,
    preview_state: &Rc<RefCell<PreviewState>>,
    preview_picture: &gtk::Picture,
    preview_meta_label: &gtk::Label,
    preview_status_label: &gtk::Label,
    save_button: &gtk::Button,
    copy_button: &gtk::Button,
    reveal_button: &gtk::Button,
    target: Target,
    setup_feedback: Option<(&gtk::Button, &gtk::Label)>,
) {
    let window = window.clone();
    let stack = stack.clone();
    let preview_state = preview_state.clone();
    let preview_picture = preview_picture.clone();
    let preview_meta_label = preview_meta_label.clone();
    let preview_status_label = preview_status_label.clone();
    let save_button = save_button.clone();
    let copy_button = copy_button.clone();
    let reveal_button = reveal_button.clone();
    let setup_feedback = setup_feedback.map(|(b, l)| (b.clone(), l.clone()));

    window.hide();

    let overlays = if target == Target::Monitor {
        show_monitor_identifiers(&crate::hyprland::enumerate_monitors())
    } else {
        Vec::new()
    };
    let delay_ms = if target == Target::Monitor { MONITOR_OVERLAY_EXTRA_DELAY_MS } else { INITIAL_HIDE_DELAY_MS };

    glib::timeout_add_local_once(Duration::from_millis(delay_ms), move || {
        // Phase 1: run slurp on a worker thread.
        // Area/Window return a geometry string; Monitor returns the output name.
        let (sel_tx, sel_rx) = std::sync::mpsc::channel::<anyhow::Result<String>>();
        std::thread::spawn(move || {
            let result = match target {
                Target::Area => crate::capture::screenshot::select_area(),
                Target::Window => crate::capture::screenshot::select_window(),
                Target::Monitor => crate::capture::screenshot::select_monitor(),
            };
            let _ = sel_tx.send(result);
        });

        let overlays_cell = Rc::new(RefCell::new(Some(overlays)));
        glib::timeout_add_local(Duration::from_millis(SELECTION_POLL_INTERVAL_MS), move || {
            let sel_result = match sel_rx.try_recv() {
                Ok(r) => r,
                Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(_) => return glib::ControlFlow::Break,
            };

            if let Some(ov) = overlays_cell.borrow_mut().take() {
                close_monitor_identifiers(ov);
            }

            let selection = match sel_result {
                Err(error) => {
                    if let Some((cta_button, _)) = &setup_feedback {
                        cta_button.set_sensitive(true);
                    }
                    report_action_error(
                        "Capture failed", &error, &window, &stack,
                        setup_feedback.as_ref(), &preview_status_label, true,
                    );
                    return glib::ControlFlow::Break;
                }
                Ok(s) => s,
            };

            // Phase 2: CompositorRepaintGuard on the worker thread already waited for
            // closelayer + one frame. This idle hands back to the GTK main loop before
            // spawning the grim thread.
            let window2 = window.clone();
            let stack2 = stack.clone();
            let preview_state2 = preview_state.clone();
            let preview_picture2 = preview_picture.clone();
            let preview_meta_label2 = preview_meta_label.clone();
            let preview_status_label2 = preview_status_label.clone();
            let save_button2 = save_button.clone();
            let copy_button2 = copy_button.clone();
            let reveal_button2 = reveal_button.clone();
            let setup_feedback2 = setup_feedback.clone();

            wait_compositor_frame(move || {
                let (cap_tx, cap_rx) =
                    std::sync::mpsc::channel::<anyhow::Result<std::path::PathBuf>>();
                std::thread::spawn(move || {
                    let result = match target {
                        Target::Area => {
                            crate::capture::screenshot::capture_geometry(&selection)
                        }
                        Target::Window => {
                            crate::capture::screenshot::capture_window_geometry(&selection)
                        }
                        Target::Monitor => {
                            crate::capture::screenshot::capture_by_monitor_name(&selection)
                        }
                    };
                    let _ = cap_tx.send(result);
                });

                glib::timeout_add_local(Duration::from_millis(SELECTION_POLL_INTERVAL_MS), move || {
                    let result = match cap_rx.try_recv() {
                        Ok(r) => r,
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            return glib::ControlFlow::Continue
                        }
                        Err(_) => return glib::ControlFlow::Break,
                    };

                    window2.present();
                    if let Some((cta_button, _)) = &setup_feedback2 {
                        cta_button.set_sensitive(true);
                    }
                    match result {
                        Ok(path) => {
                            {
                                let mut preview = preview_state2.borrow_mut();
                                preview.temp_path = Some(path.clone());
                                preview.current_path = Some(path.clone());
                                preview.thumbnail_path = None;
                                preview.kind = PreviewKind::Screenshot;
                            }
                            save_button2.remove_css_class("mode-rec");
                            save_button2.add_css_class("mode-shot");
                            load_preview_image(&path, &preview_picture2, &preview_meta_label2);
                            set_status_neutral(&preview_status_label2, "");
                            save_button2.set_sensitive(true);
                            set_action_button_content(&copy_button2, "copy", "Copy");
                            copy_button2.set_sensitive(true);
                            reveal_button2.set_sensitive(false);
                            stack2.set_visible_child_name("preview");
                        }
                        Err(error) => {
                            report_action_error(
                                "Capture failed", &error, &window2, &stack2,
                                setup_feedback2.as_ref(), &preview_status_label2, true,
                            );
                        }
                    }
                    glib::ControlFlow::Break
                });
            });

            glib::ControlFlow::Break
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn start_recording_action(
    window: &gtk::ApplicationWindow,
    stack: &gtk::Stack,
    preview_state: &Rc<RefCell<PreviewState>>,
    recording_state: &Rc<RefCell<Option<ActiveRecording>>>,
    setup_cta: &Rc<RefCell<Option<gtk::Button>>>,
    preview_picture: &gtk::Picture,
    preview_meta_label: &gtk::Label,
    preview_status_label: &gtk::Label,
    setup_status_label: &gtk::Label,
    save_button: &gtk::Button,
    copy_button: &gtk::Button,
    reveal_button: &gtk::Button,
    target: Target,
    show_hud: bool,
    setup_feedback: Option<(&gtk::Button, &gtk::Label)>,
) {
    let window = window.clone();
    let stack = stack.clone();
    let preview_state = preview_state.clone();
    let recording_state = recording_state.clone();
    let setup_cta = setup_cta.clone();
    let preview_picture = preview_picture.clone();
    let preview_meta_label = preview_meta_label.clone();
    let preview_status_label = preview_status_label.clone();
    let setup_status_label = setup_status_label.clone();
    let save_button = save_button.clone();
    let copy_button = copy_button.clone();
    let reveal_button = reveal_button.clone();
    let setup_feedback = setup_feedback.map(|(b, l)| (b.clone(), l.clone()));

    if let Some((cta_button, status_label)) = &setup_feedback {
        cta_button.set_sensitive(false);
        if !status_label.label().is_empty() {
            set_status_live(status_label, status_label.label().as_str());
        }
    }

    window.hide();

    let overlays = if target == Target::Monitor {
        show_monitor_identifiers(&crate::hyprland::enumerate_monitors())
    } else {
        Vec::new()
    };
    let delay_ms = if target == Target::Monitor { MONITOR_OVERLAY_EXTRA_DELAY_MS } else { INITIAL_HIDE_DELAY_MS };

    glib::timeout_add_local_once(Duration::from_millis(delay_ms), move || {
        // Phase 1: run slurp on a worker thread.
        let (sel_tx, sel_rx) = std::sync::mpsc::channel::<
            anyhow::Result<crate::capture::record::RecordingSelection>,
        >();
        std::thread::spawn(move || {
            let result = match target {
                Target::Area => crate::capture::record::select_area(),
                Target::Monitor => crate::capture::record::select_monitor(),
                Target::Window => crate::capture::record::select_window(),
            };
            let _ = sel_tx.send(result);
        });

        let overlays_cell = Rc::new(RefCell::new(Some(overlays)));
        glib::timeout_add_local(Duration::from_millis(SELECTION_POLL_INTERVAL_MS), move || {
            let sel_result = match sel_rx.try_recv() {
                Ok(r) => r,
                Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(_) => return glib::ControlFlow::Break,
            };

            if let Some(ov) = overlays_cell.borrow_mut().take() {
                close_monitor_identifiers(ov);
            }

            let selection = match sel_result {
                Err(error) => {
                    enable_setup_cta(&setup_cta);
                    report_action_error(
                        "Recording failed", &error, &window, &stack,
                        setup_feedback.as_ref(), &setup_status_label, false,
                    );
                    return glib::ControlFlow::Break;
                }
                Ok(s) => s,
            };

            // Phase 2: wait for one compositor frame, then launch wf-recorder.
            // launch_recording is fast (spawn + file write) so runs on the GTK thread.
            let window2 = window.clone();
            let stack2 = stack.clone();
            let preview_state2 = preview_state.clone();
            let recording_state2 = recording_state.clone();
            let setup_cta2 = setup_cta.clone();
            let preview_picture2 = preview_picture.clone();
            let preview_meta_label2 = preview_meta_label.clone();
            let preview_status_label2 = preview_status_label.clone();
            let setup_status_label2 = setup_status_label.clone();
            let save_button2 = save_button.clone();
            let copy_button2 = copy_button.clone();
            let reveal_button2 = reveal_button.clone();
            let setup_feedback2 = setup_feedback.clone();

            wait_compositor_frame(move || {
                match crate::capture::record::launch_recording(selection) {
                    Err(error) => {
                        enable_setup_cta(&setup_cta2);
                        report_action_error(
                            "Recording failed", &error, &window2, &stack2,
                            setup_feedback2.as_ref(), &setup_status_label2, false,
                        );
                    }
                    Ok(session) => {
                        let hud_window = if show_hud {
                            Some(create_recording_hud(&recording_state2))
                        } else {
                            None
                        };

                        let monitor = session.monitor;
                        let indicator_window = if show_hud {
                            None
                        } else if crate::config::get().recording_indicator_enabled {
                            let (w, _dot) = create_recording_indicator(monitor, &recording_state2);
                            Some(w)
                        } else {
                            None
                        };

                        *recording_state2.borrow_mut() = Some(ActiveRecording {
                            child: session.child,
                            temp_path: session.temp_path,
                            hud_window,
                            indicator_window,
                            started_at: Instant::now(),
                        });

                        start_recording_poll(
                            &window2,
                            &stack2,
                            &preview_state2,
                            &recording_state2,
                            &setup_cta2,
                            &preview_picture2,
                            &preview_meta_label2,
                            &preview_status_label2,
                            &setup_status_label2,
                            &save_button2,
                            &copy_button2,
                            &reveal_button2,
                        );
                    }
                }
            });

            glib::ControlFlow::Break
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn start_recording_poll(
    window: &gtk::ApplicationWindow,
    stack: &gtk::Stack,
    preview_state: &Rc<RefCell<PreviewState>>,
    recording_state: &Rc<RefCell<Option<ActiveRecording>>>,
    setup_cta: &Rc<RefCell<Option<gtk::Button>>>,
    preview_picture: &gtk::Picture,
    preview_meta_label: &gtk::Label,
    preview_status_label: &gtk::Label,
    setup_status_label: &gtk::Label,
    save_button: &gtk::Button,
    copy_button: &gtk::Button,
    reveal_button: &gtk::Button,
) {
    let window = window.clone();
    let stack = stack.clone();
    let preview_state = preview_state.clone();
    let recording_state = recording_state.clone();
    let setup_cta = setup_cta.clone();
    let preview_picture = preview_picture.clone();
    let preview_meta_label = preview_meta_label.clone();
    let preview_status_label = preview_status_label.clone();
    let setup_status_label = setup_status_label.clone();
    let save_button = save_button.clone();
    let copy_button = copy_button.clone();
    let reveal_button = reveal_button.clone();

    glib::timeout_add_local(Duration::from_millis(250), move || {
        let mut borrowed = recording_state.borrow_mut();
        let Some(active) = borrowed.as_mut() else {
            return glib::ControlFlow::Break;
        };

        match active.child.try_wait() {
            Ok(Some(status)) => {
                let finished = borrowed.take().expect("active recording disappeared");
                drop(borrowed);

                if let Some(hud) = finished.hud_window {
                    hud.close();
                }
                if let Some(indicator) = finished.indicator_window {
                    indicator.close();
                }
                crate::capture::record::clear_state_file();
                window.present();

                if !status.success() || !finished.temp_path.exists() {
                    enable_setup_cta(&setup_cta);
                    set_status_err(&setup_status_label, "Recording failed or was cancelled");
                    stack.set_visible_child_name("setup");
                    return glib::ControlFlow::Break;
                }

                {
                    let mut preview = preview_state.borrow_mut();
                    preview.temp_path = Some(finished.temp_path.clone());
                }
                save_button.remove_css_class("mode-shot");
                save_button.add_css_class("mode-rec");
                load_preview_recording(
                    &finished.temp_path,
                    &preview_state,
                    &preview_picture,
                    &preview_meta_label,
                    &copy_button,
                    &reveal_button,
                );
                set_status_neutral(&preview_status_label, "");
                save_button.set_sensitive(true);
                copy_button.set_sensitive(false);
                stack.set_visible_child_name("preview");
                glib::ControlFlow::Break
            }
            Ok(None) => glib::ControlFlow::Continue,
            Err(error) => {
                drop(borrowed);
                crate::capture::record::clear_state_file();
                window.present();
                enable_setup_cta(&setup_cta);
                set_status_err(
                    &setup_status_label,
                    &format!("Recording poll failed: {error}"),
                );
                stack.set_visible_child_name("setup");
                glib::ControlFlow::Break
            }
        }
    });
}

fn wait_compositor_frame<F: FnOnce() + 'static>(callback: F) {
    // The worker thread already waited for Hyprland's closelayer event + one
    // frame via CompositorRepaintGuard. This idle just hands control back to
    // the GTK main loop before spawning the capture thread.
    glib::idle_add_local_once(callback);
}

fn enable_setup_cta(setup_cta: &Rc<RefCell<Option<gtk::Button>>>) {
    if let Some(button) = setup_cta.borrow().as_ref() {
        button.set_sensitive(true);
    }
}

fn create_recording_hud(
    recording_state: &Rc<RefCell<Option<ActiveRecording>>>,
) -> gtk::Window {
    let hud = gtk::Window::builder()
        .title("Hyprscreen HUD")
        .decorated(false)
        .resizable(false)
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["hs-hud"])
        .build();

    // Red pulse dot
    let rec_dot = gtk::Box::builder()
        .width_request(9)
        .height_request(9)
        .valign(gtk::Align::Center)
        .css_classes(["hs-hud-dot"])
        .build();

    // REC label
    let rec_label = gtk::Label::builder()
        .label("REC")
        .css_classes(["hs-hud-rec"])
        .build();

    // Timer
    let counter = gtk::Label::builder()
        .label("00:00")
        .css_classes(["hs-hud-timer"])
        .build();

    // Separator
    let sep = gtk::Box::builder()
        .width_request(1)
        .height_request(16)
        .valign(gtk::Align::Center)
        .css_classes(["hs-hud-sep"])
        .build();

    // Stop button
    let stop = gtk::Button::builder().css_classes(["hs-hud-stop"]).build();
    set_button_text(&stop, "STOP");

    content.append(&rec_dot);
    content.append(&rec_label);
    content.append(&counter);
    content.append(&sep);
    content.append(&stop);
    hud.set_child(Some(&content));

    stop.connect_clicked(move |_| {
        let _ = crate::capture::record::stop_active_recording();
    });

    let recording_state_for_timer = recording_state.clone();
    glib::timeout_add_local(Duration::from_secs(1), move || {
        let borrowed = recording_state_for_timer.borrow();
        let Some(active) = borrowed.as_ref() else {
            return glib::ControlFlow::Break;
        };
        let elapsed = active.started_at.elapsed().as_secs();
        let m = elapsed / 60;
        let s = elapsed % 60;
        counter.set_label(&format!("{m:02}:{s:02}"));
        glib::ControlFlow::Continue
    });

    hud.present();
    hud
}

fn show_monitor_identifiers(monitors: &[crate::hyprland::Monitor]) -> Vec<gtk::Window> {
    monitors
        .iter()
        .map(|monitor| {
            let title = format!("Hyprscreen Monitor ID {}", monitor.name);
            let window = gtk::Window::builder()
                .title(&title)
                .decorated(false)
                .resizable(false)
                .default_width(180)
                .default_height(96)
                .build();
            window.add_css_class("hs-mon-id");

            let label = gtk::Label::builder()
                .label(&monitor.name)
                .css_classes(["hs-mon-id-label"])
                .build();
            window.set_child(Some(&label));
            window.present();

            let x = monitor.x + (monitor.width - 180) / 2;
            let y = monitor.y + (monitor.height - 96) / 2;
            crate::hyprland::make_window_plain(&title);
            crate::hyprland::place_window_exact(&title, x, y);

            window
        })
        .collect()
}

fn close_monitor_identifiers(overlays: Vec<gtk::Window>) {
    for overlay in overlays {
        overlay.close();
    }
    if let Some(display) = gtk::gdk::Display::default() {
        display.sync();
    }
}

fn create_recording_indicator(
    monitor: crate::capture::record::MonitorPlacement,
    recording_state: &Rc<RefCell<Option<ActiveRecording>>>,
) -> (gtk::Window, gtk::Label) {
    let indicator = gtk::Window::builder()
        .title("Hyprscreen Recording Indicator")
        .decorated(false)
        .resizable(false)
        .default_width(16)
        .default_height(16)
        .build();
    indicator.add_css_class("hs-rec-indicator");

    let dot = gtk::Label::builder()
        .label("●")
        .css_classes(["hs-rec-flash"])
        .margin_top(2)
        .margin_bottom(2)
        .margin_start(2)
        .margin_end(2)
        .build();
    indicator.set_child(Some(&dot));
    indicator.present();

    let x = monitor.x + ((monitor.width - 16) / 2).max(0);
    let y = monitor.y + monitor.height - 16 - 20;
    crate::hyprland::make_window_plain("Hyprscreen Recording Indicator");
    crate::hyprland::place_window_exact("Hyprscreen Recording Indicator", x, y);
    dot.set_visible(false);

    let dot_for_first_flash = dot.clone();
    glib::timeout_add_local_once(Duration::from_millis(180), move || {
        flash_indicator(&dot_for_first_flash);
    });

    let dot_for_timer = dot.clone();
    let recording_state_for_timer = recording_state.clone();
    let interval = crate::config::get().recording_indicator_interval_seconds;
    glib::timeout_add_local(Duration::from_secs(interval), move || {
        if recording_state_for_timer.borrow().is_none() {
            return glib::ControlFlow::Break;
        }
        flash_indicator(&dot_for_timer);
        glib::ControlFlow::Continue
    });

    (indicator, dot)
}

fn flash_indicator(dot: &gtk::Label) {
    dot.set_visible(true);
    let dot = dot.clone();
    glib::timeout_add_local_once(
        Duration::from_millis(crate::config::get().recording_indicator_duration_ms),
        move || {
            dot.set_visible(false);
        },
    );
}

fn active_target(area_button: &gtk::ToggleButton, window_button: &gtk::ToggleButton) -> Target {
    if area_button.is_active() {
        Target::Area
    } else if window_button.is_active() {
        Target::Window
    } else {
        Target::Monitor
    }
}

fn clear_preview(
    preview_state: &Rc<RefCell<PreviewState>>,
    preview_picture: &gtk::Picture,
    preview_meta_label: &gtk::Label,
    preview_status_label: &gtk::Label,
    save_button: &gtk::Button,
    copy_button: &gtk::Button,
    reveal_button: &gtk::Button,
) {
    let mut preview = preview_state.borrow_mut();
    if let Some(path) = preview.temp_path.take() {
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = preview.thumbnail_path.take() {
        let _ = std::fs::remove_file(path);
    }
    preview.current_path = None;
    preview.kind = PreviewKind::Screenshot;
    drop(preview);

    preview_picture.set_file(Option::<&gio::File>::None);
    clear_preview_meta(preview_meta_label);
    set_status_neutral(preview_status_label, "");
    save_button.set_sensitive(false);
    set_action_button_content(copy_button, "copy", "Copy");
    copy_button.set_sensitive(false);
    reveal_button.set_sensitive(false);
}

fn open_preview_file(
    preview_state: &PreviewState,
) -> anyhow::Result<crate::capture::record::OpenMethod> {
    if preview_state.kind != PreviewKind::Recording {
        anyhow::bail!("open is only available for recordings")
    }
    let Some(path) = &preview_state.current_path else {
        anyhow::bail!("there is no recording to open")
    };
    if preview_state.temp_path.is_some() && preview_state.current_path == preview_state.temp_path {
        anyhow::bail!("save the recording before opening it")
    }
    crate::capture::record::open_video_file(path)
}

fn set_status(label: &gtk::Label, message: &str, kind: StatusKind) {
    label.set_label(message);
    for cls in ["err", "ok", "live"] {
        label.remove_css_class(cls);
    }
    match kind {
        StatusKind::Neutral => {}
        StatusKind::Error => label.add_css_class("err"),
        StatusKind::Success => label.add_css_class("ok"),
        StatusKind::Live => label.add_css_class("live"),
    }
}

fn set_button_text(button: &impl IsA<gtk::Button>, text: &str) {
    let button = button.as_ref();

    if let Some(label) = button
        .child()
        .and_then(|child| child.downcast::<gtk::Label>().ok())
    {
        label.set_label(text);
        return;
    }

    let label = gtk::Label::new(Some(text));
    button.set_child(Some(&label));
}

fn set_status_neutral(label: &gtk::Label, message: &str) {
    set_status(label, message, StatusKind::Neutral);
}

fn set_status_live(label: &gtk::Label, message: &str) {
    set_status(label, message, StatusKind::Live);
}

fn set_status_ok(label: &gtk::Label, message: &str) {
    set_status(label, message, StatusKind::Success);
}

fn set_status_err(label: &gtk::Label, message: &str) {
    set_status(label, message, StatusKind::Error);
}

fn report_action_error(
    prefix: &str,
    error: &anyhow::Error,
    window: &gtk::ApplicationWindow,
    stack: &gtk::Stack,
    setup_feedback: Option<&(gtk::Button, gtk::Label)>,
    fallback_label: &gtk::Label,
    navigate_on_feedback: bool,
) {
    window.present();
    if let Some((_, status_label)) = setup_feedback {
        set_status_err(status_label, &format!("{prefix}: {error}"));
        if navigate_on_feedback {
            stack.set_visible_child_name("setup");
        }
    } else {
        set_status_err(fallback_label, &format!("{prefix}: {error}"));
        if !navigate_on_feedback {
            stack.set_visible_child_name("setup");
        }
    }
}

fn set_status_stop_hint(label: &gtk::Label) {
    for cls in ["err", "ok", "live"] {
        label.remove_css_class(cls);
    }
    label.set_markup("run \"<b>hyprscreen stop</b>\" to end recording");
}

fn set_preview_meta(label: &gtk::Label, message: &str) {
    label.set_label(message);
}

fn clear_preview_meta(label: &gtk::Label) {
    label.set_label("");
}

fn set_action_button_content(button: &gtk::Button, icon_key: &str, text: &str) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(5)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    let icon = icon_image(icon_key, 16, Some("hs-abtn-icon"));
    let label = gtk::Label::builder()
        .label(text)
        .css_classes(["hs-abtn-label"])
        .build();
    content.append(&icon);
    content.append(&label);
    button.set_child(Some(&content));
}

fn set_primary_button_content(button: &gtk::Button, mode: Mode) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(9)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    let icon: gtk::Widget = match mode {
        Mode::Screenshot => icon_image("shutter", 16, None).upcast(),
        Mode::Record => gtk::Box::builder()
            .css_classes(["hs-primary-pulse"])
            .valign(gtk::Align::Center)
            .build()
            .upcast(),
    };
    let label = gtk::Label::builder()
        .label(match mode {
            Mode::Screenshot => "Capture",
            Mode::Record => "Start recording",
        })
        .css_classes(["hs-primary-label"])
        .build();
    content.append(&icon);
    content.append(&label);
    button.set_child(Some(&content));
}

fn icon_image(icon_key: &str, size: i32, css_class: Option<&str>) -> gtk::Image {
    let bytes = glib::Bytes::from_static(icon_bytes(icon_key));
    let stream = gio::MemoryInputStream::from_bytes(&bytes);
    let render = size * 2;
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        render,
        render,
        true,
        gio::Cancellable::NONE,
    )
    .expect("failed to rasterize embedded SVG");
    let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
    let image = gtk::Image::from_paintable(Some(&texture));
    image.set_pixel_size(size);
    if let Some(css_class) = css_class {
        image.add_css_class(css_class);
    }
    image
}

fn icon_bytes(icon_key: &str) -> &'static [u8] {
    match icon_key {
        "area" => include_bytes!("../../assets/icons/area.svg"),
        "window" => include_bytes!("../../assets/icons/window.svg"),
        "monitor" => include_bytes!("../../assets/icons/monitor.svg"),
        "back" => include_bytes!("../../assets/icons/back.svg"),
        "refresh" => include_bytes!("../../assets/icons/refresh.svg"),
        "save" => include_bytes!("../../assets/icons/save.svg"),
        "copy" => include_bytes!("../../assets/icons/copy.svg"),
        "reveal" => include_bytes!("../../assets/icons/reveal.svg"),
        "open" => include_bytes!("../../assets/icons/open.svg"),
        "shutter" => include_bytes!("../../assets/icons/shutter.svg"),
        _ => include_bytes!("../../assets/icons/area.svg"),
    }
}

fn copy_preview_to_clipboard(path: &Option<PathBuf>) -> anyhow::Result<()> {
    let Some(path) = path else {
        anyhow::bail!("there is no screenshot to copy")
    };

    let mut child = Command::new("wl-copy")
        .arg("--type")
        .arg("image/png")
        .stdin(Stdio::piped())
        .spawn()?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to open wl-copy stdin"))?;
    let bytes = std::fs::read(path)?;
    stdin.write_all(&bytes)?;
    drop(stdin);

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("wl-copy failed")
    }
    Ok(())
}

fn save_preview_file(preview_state: &mut PreviewState) -> anyhow::Result<PathBuf> {
    let Some(source) = &preview_state.current_path else {
        anyhow::bail!("there is no file to save")
    };

    let save_dir = match preview_state.kind {
        PreviewKind::Screenshot => crate::config::get().save_dir_screenshots.clone(),
        PreviewKind::Recording => crate::config::get().save_dir_recordings.clone(),
    };
    std::fs::create_dir_all(&save_dir)?;

    let file_name = source
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("temporary file path has no file name"))?;
    let destination = save_dir.join(file_name);

    if *source == destination {
        return Ok(destination);
    }

    std::fs::copy(source, &destination)?;
    preview_state.current_path = Some(destination.clone());
    Ok(destination)
}

fn reveal_preview_file(
    preview_state: &PreviewState,
) -> anyhow::Result<crate::capture::record::RevealMethod> {
    let Some(path) = &preview_state.current_path else {
        anyhow::bail!("there is no file to reveal")
    };
    if preview_state.temp_path.is_some() && preview_state.current_path == preview_state.temp_path {
        anyhow::bail!("save the file before revealing it")
    }
    crate::capture::record::reveal_in_file_manager(path)
}

```

# hyprland/mod.rs

```
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

```

# config/mod.rs

```
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

```

# cli/mod.rs

```
#[derive(Clone, Copy, Debug)]
pub enum StartupAction {
    Screenshot(StartupTarget),
    Record(StartupTarget),
}

#[derive(Clone, Copy, Debug)]
pub enum StartupTarget {
    Area,
    Window,
    Monitor,
}

pub fn run() -> anyhow::Result<()> {
    // SAFETY: set before any threads are spawned and before gtk::init reads it.
    unsafe {
        std::env::set_var("GTK_THEME", "Adwaita");
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();

    let startup = match refs.as_slice() {
        [] => None,
        ["stop"] => return crate::capture::record::stop_active_recording(),
        ["--version" | "-V"] => {
            print_version();
            return Ok(());
        }
        ["--help" | "-h"] => {
            print_help();
            return Ok(());
        }
        ["screenshot", target] => Some(StartupAction::Screenshot(parse_target(target))),
        ["record", target] => Some(StartupAction::Record(parse_target(target))),
        _ => {
            print_help();
            std::process::exit(2);
        }
    };

    detach_if_interactive();

    gtk::init()?;
    let app = crate::app::build(startup);
    let _exit = gtk::prelude::ApplicationExtManual::run_with_args::<&str>(&app, &[]);
    Ok(())
}

fn detach_if_interactive() {
    use std::os::fd::AsRawFd;

    let stdin_is_tty = unsafe { libc::isatty(std::io::stdin().as_raw_fd()) } == 1;
    if !stdin_is_tty {
        return;
    }

    match unsafe { libc::fork() } {
        -1 => eprintln!("hyprscreen: fork failed, running in foreground"),
        0 => {
            unsafe { libc::setsid() };
        }
        _ => std::process::exit(0),
    }
}

fn parse_target(value: &str) -> StartupTarget {
    match value {
        "area" => StartupTarget::Area,
        "window" => StartupTarget::Window,
        "monitor" => StartupTarget::Monitor,
        _ => {
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_version() {
    println!("hyprscreen {}", env!("CARGO_PKG_VERSION"));
}

fn print_help() {
    println!(
        "hyprscreen — Hyprland screenshot and recording

USAGE:
    hyprscreen                                    Open the GUI.
    hyprscreen screenshot <area|window|monitor>   Open the GUI and capture immediately.
    hyprscreen record <area|window|monitor>       Open the GUI and start recording immediately.
    hyprscreen stop                               Stop the active recording.
    hyprscreen --version, -V                      Print version.
    hyprscreen --help, -h                         Print this help."
    );
}

```

# capture/screenshot.rs

```
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

fn temp_file_path() -> Result<PathBuf> {
    Ok(super::hyprscreen_temp_dir()?.join(crate::capture::generated_filename("png")))
}

pub fn select_area() -> Result<String> {
    let guard = super::CompositorRepaintGuard::arm();
    let output = Command::new("slurp")
        .args([
            "-b", "#00000088",
            "-c", "#e8eefcff",
            "-s", "#00000000",
            "-w", "3",
            "-d",
        ])
        .output()
        .context("failed to launch slurp")?;

    if !output.status.success() {
        bail!("area selection was cancelled")
    }

    guard.wait();

    let geometry = String::from_utf8(output.stdout)
        .context("slurp returned non-utf8 geometry")?
        .trim()
        .to_owned();

    if geometry.is_empty() {
        bail!("area selection returned no geometry")
    }

    Ok(geometry)
}

pub fn select_window() -> Result<String> {
    let choices = super::visible_window_geometries()?;
    if choices.is_empty() {
        bail!("no eligible windows found")
    }

    let guard = super::CompositorRepaintGuard::arm();
    let mut child = Command::new("slurp")
        .args([
            "-r",
            "-b", "#00000088",
            "-c", "#e8eefcff",
            "-s", "#00000000",
            "-w", "3",
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

    guard.wait();

    let geometry = String::from_utf8(output.stdout)
        .context("slurp returned non-utf8 geometry")?
        .trim()
        .to_owned();

    if geometry.is_empty() {
        bail!("window selection returned no geometry")
    }

    Ok(geometry)
}

pub fn select_monitor() -> Result<String> {
    let monitors = crate::hyprland::enumerate_monitors();
    if monitors.is_empty() {
        bail!("no eligible monitors found")
    }
    let guard = super::CompositorRepaintGuard::arm();
    let geometry = select_monitor_geometry(&monitors)?;
    guard.wait();
    let (x, y, width, height) = super::parse_geometry(&geometry)?;
    monitors
        .into_iter()
        .find(|m| m.x == x && m.y == y && m.width == width && m.height == height)
        .map(|m| m.name)
        .ok_or_else(|| anyhow!("selected monitor could not be resolved"))
}

pub fn capture_geometry(geometry: &str) -> Result<PathBuf> {
    // slurp's 3px border is centered on the selection boundary, so ~1.5 logical
    // pixels of it fall inside the reported geometry. Insetting by 2px removes
    // the two artifact rows/columns that would otherwise appear in the capture.
    capture_geometry_inset(geometry, 2)
}

pub fn capture_window_geometry(geometry: &str) -> Result<PathBuf> {
    // Window captures need a larger inset: Hyprland draws its border + rounded
    // corners over the region reported by `hyprctl clients`. The inset must cover
    // border_size + rounding so that neither the straight-edge border nor the
    // corner arc bleeds into the captured image.
    capture_geometry_inset(geometry, super::hyprland_window_inset())
}

fn capture_geometry_inset(geometry: &str, inset: i32) -> Result<PathBuf> {
    let path = temp_file_path()?;
    let inset_geom = super::inset_geometry(geometry, inset).unwrap_or_else(|| geometry.to_owned());
    let status = Command::new("grim")
        .arg("-g")
        .arg(&inset_geom)
        .arg(&path)
        .status()
        .context("failed to launch grim")?;

    if !status.success() {
        return Err(anyhow!("grim failed to capture the selected geometry"));
    }

    Ok(path)
}

pub fn capture_by_monitor_name(name: &str) -> Result<PathBuf> {
    let path = temp_file_path()?;
    let status = Command::new("grim")
        .arg("-o")
        .arg(name)
        .arg(&path)
        .status()
        .context("failed to launch grim")?;
    if !status.success() {
        return Err(anyhow!("grim failed to capture the monitor"));
    }
    Ok(path)
}

fn select_monitor_geometry(monitors: &[crate::hyprland::Monitor]) -> Result<String> {
    let choices = monitors
        .iter()
        .map(|m| format!("{},{} {}x{}", m.x, m.y, m.width, m.height))
        .collect::<Vec<_>>();

    let mut child = Command::new("slurp")
        .args([
            "-r",
            "-b", "#00000088",
            "-c", "#e8eefcff",
            "-s", "#00000000",
            "-w", "8",
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

```

# capture/record.rs

```
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

```

# capture/mod.rs

```
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

```

# app/mod.rs

```
use gtk::prelude::*;

pub fn build(startup: Option<crate::cli::StartupAction>) -> gtk::Application {
    let app = gtk::Application::builder()
        .application_id("land.hypr.Hyprscreen")
        .build();

    app.connect_activate(move |app| {
        if let Some(settings) = gtk::Settings::default() {
            settings.set_gtk_application_prefer_dark_theme(true);
        }
        load_css();
        let window = crate::ui::main_window::build(app, startup);
        if startup.is_none() {
            window.present();
        }
    });

    app
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("display unavailable for CSS provider"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );
}

const CSS: &str = r#"
window {
    background: #16181E;
    color: #E8E9EC;
    font-family: Cantarell, "Inter Tight", "Segoe UI", sans-serif;
}

window.hs-rec-indicator,
window.hs-rec-indicator > * {
    background: transparent;
}

button {
    background-image: none;
    outline: none;
    box-shadow: none;
    min-height: 0;
    padding: 0;
}


.hs-body {
    padding: 16px 18px 14px 18px;
}

.hs-seg {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 8px;
    padding: 3px;
}

.hs-seg > button {
    border: none;
    background: transparent;
    border-radius: 6px;
    padding: 7px 0;
    min-height: 0;
}

.hs-seg > button:hover {
    color: #E8E9EC;
}

.hs-seg > button:checked {
    background: rgba(255, 255, 255, 0.06);
    box-shadow: 0 1px 0 rgba(255,255,255,0.05) inset, 0 1px 2px rgba(0,0,0,0.25);
}

.hs-seg-label {
    color: #8B8D95;
    font-size: 12.5px;
    font-weight: 500;
    letter-spacing: 0.02em;
}

.hs-seg > button:hover .hs-seg-label,
.hs-seg > button:checked .hs-seg-label {
    color: #E8E9EC;
}

.hs-tbtn {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 8px;
    padding: 0;
    min-height: 0;
}

.hs-tbtn:hover {
    background: rgba(255,255,255,0.05);
    border-color: rgba(255,255,255,0.14);
}

.hs-tbtn:active {
    transform: translateY(1px);
}

.hs-tbtn:checked {
    border-color: rgba(255,255,255,0.22);
    background: rgba(255,255,255,0.07);
    box-shadow: 0 0 0 1px rgba(255,255,255,0.04) inset;
}

.hs-tbtn.mode-rec:checked {
    border-color: rgba(229,72,77,0.55);
    background: rgba(229,72,77,0.16);
}

.hs-tbtn.mode-shot:checked {
    border-color: rgba(229,236,245,0.45);
    background: rgba(229,236,245,0.16);
}

.hs-tbtn-label {
    color: #8B8D95;
    font-size: 12px;
    font-weight: 500;
    line-height: 1;
}

.hs-tbtn-icon {
    opacity: 0.5;
}

.hs-tbtn:hover .hs-tbtn-label,
.hs-tbtn:checked .hs-tbtn-label {
    color: #E8E9EC;
}

.hs-tbtn:hover .hs-tbtn-icon,
.hs-tbtn:checked .hs-tbtn-icon {
    opacity: 1;
}

.hs-primary {
    border: none;
    border-radius: 8px;
    padding: 13px 16px;
    min-height: 0;
}

.hs-primary.mode-shot {
    background: #E5ECF5;
    color: #0E1116;
}

.hs-primary.mode-rec {
    background: #E5484D;
    color: #FFFFFF;
}

.hs-primary:hover {
    filter: brightness(1.06);
}

.hs-primary:active {
    transform: translateY(1px);
    filter: brightness(0.94);
}

.hs-primary:disabled {
    opacity: 0.45;
}

.hs-primary-label {
    font-size: 13.5px;
    font-weight: 600;
    letter-spacing: 0.01em;
}

.hs-primary-pulse {
    min-width: 8px;
    min-height: 8px;
    background: #FFFFFF;
    border-radius: 999px;
    box-shadow: 0 0 0 0 rgba(255,255,255,0.5);
    animation: hsPulse 1.4s ease-out infinite;
}


@keyframes hsPulse {
    0% { box-shadow: 0 0 0 0 rgba(255,255,255,0.5); }
    70% { box-shadow: 0 0 0 8px rgba(255,255,255,0); }
    100% { box-shadow: 0 0 0 0 rgba(255,255,255,0); }
}

.hs-optrow {
    padding: 4px 2px 0 2px;
}

.hs-opt-label {
    color: #8B8D95;
    font-size: 11.5px;
    font-weight: 500;
    letter-spacing: 0.01em;
}

.hs-opt-hint {
    color: #5C5E66;
    font-family: "JetBrains Mono", "Fira Mono", monospace;
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.04em;
}

.hs-opt-dot {
    min-width: 6px;
    min-height: 6px;
    background: #5C5E66;
    border-radius: 999px;
}

.hs-optrow.is-on .hs-opt-dot {
    background: #E5484D;
    box-shadow: 0 0 6px 1px rgba(229,72,77,0.45);
}

.hs-switch {
    min-width: 34px;
    min-height: 18px;
    padding: 2px;
    border-radius: 999px;
    border: 1px solid rgba(255,255,255,0.08);
    background: rgba(255,255,255,0.07);
    outline: none;
    box-shadow: none;
}

.hs-switch:checked {
    background: rgba(229,72,77,0.16);
    border-color: rgba(229,72,77,0.55);
}

.hs-switch slider {
    min-width: 12px;
    min-height: 12px;
    border-radius: 999px;
    background: rgba(255,255,255,0.55);
    border: none;
    box-shadow: none;
}

.hs-switch:checked slider {
    background: #E5484D;
}

.hs-status {
    font-family: "JetBrains Mono", "Fira Mono", monospace;
    font-size: 11.5px;
    font-weight: 500;
    letter-spacing: 0.02em;
    color: #5C5E66;
    min-height: 14px;
}

.hs-status.err { color: #F0848A; }
.hs-status.ok { color: #7FCB9B; }
.hs-status.live { color: #E5ECF5; }

.hs-meta {
    color: #8B8D95;
    font-family: "JetBrains Mono", "Fira Mono", monospace;
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.03em;
}

.hs-preview-frame {
    background: #0C0D11;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 10px;
}

.hs-abtn {
    background: rgba(255,255,255,0.04);
    border: 1px solid rgba(255,255,255,0.08);
    border-radius: 7px;
    padding: 9px 4px 8px 4px;
    min-height: 0;
}

.hs-abtn:hover {
    background: rgba(255,255,255,0.06);
    border-color: rgba(255,255,255,0.14);
}

.hs-abtn:active {
    transform: translateY(1px);
}

.hs-abtn:disabled {
    opacity: 0.38;
}

.hs-abtn-label {
    color: #E8E9EC;
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.02em;
    line-height: 1;
}

.hs-abtn.is-primary {
    background: rgba(229,236,245,0.10);
    border-color: rgba(229,236,245,0.30);
}

.hs-abtn.is-primary.mode-rec {
    background: rgba(229,72,77,0.16);
    border-color: rgba(229,72,77,0.42);
}

.hs-hud {
    background: rgba(18, 19, 24, 0.82);
    border: 1px solid rgba(255,255,255,0.10);
    padding: 8px 10px 8px 14px;
}

.hs-hud-dot {
    background: #E5484D;
    border-radius: 999px;
    min-width: 9px;
    min-height: 9px;
    animation: hudPulse 1.6s ease-out infinite;
}

@keyframes hudPulse {
    0% { box-shadow: 0 0 0 0 rgba(229,72,77,0.55); }
    70% { box-shadow: 0 0 0 7px rgba(229,72,77,0); }
    100% { box-shadow: 0 0 0 0 rgba(229,72,77,0); }
}

.hs-hud-rec {
    color: #8B8D95;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.14em;
}

.hs-hud-timer {
    color: #E8E9EC;
    font-family: "JetBrains Mono", "Fira Mono", monospace;
    font-size: 13px;
    font-weight: 500;
    letter-spacing: 0.02em;
}

.hs-hud-sep {
    background: rgba(255,255,255,0.08);
    min-width: 1px;
    min-height: 16px;
}

.hs-hud-stop {
    border: none;
    background: #E5484D;
    color: #FFFFFF;
    border-radius: 999px;
    padding: 7px 12px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    min-height: 0;
}

.hs-hud-stop:hover {
    filter: brightness(1.08);
}

window.hs-mon-id {
    background: #0E1116;
    border: 2px solid rgba(255, 255, 255, 0.10);
    border-radius: 14px;
}

.hs-mon-id-label {
    color: #E8E9EC;
    font-family: "JetBrains Mono", "Fira Mono", monospace;
    font-size: 32px;
    font-weight: 600;
    letter-spacing: 0.04em;
    padding: 18px 24px;
}

label.hs-rec-flash {
    color: #E5484D;
    font-size: 14px;
    font-weight: 700;
    background: transparent;
}


/* ── Tinted target-button labels in checked state ── */
.hs-tbtn.mode-rec:checked  .hs-tbtn-label { color: #FBD5D6; }
.hs-tbtn.mode-shot:checked .hs-tbtn-label { color: #E5ECF5; }

/* ── Primary action-button (Save) accent label ── */
.hs-abtn.is-primary          .hs-abtn-label { color: #E5ECF5; }
.hs-abtn.is-primary.mode-rec .hs-abtn-label { color: #FBD5D6; }
"#;

```

# main.rs

```
mod app;
mod capture;
mod cli;
mod config;
mod hyprland;
mod ui;

fn main() -> anyhow::Result<()> {
    cli::run()
}

```
