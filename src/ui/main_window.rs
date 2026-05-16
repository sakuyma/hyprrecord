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
    let delay_ms = if target == Target::Monitor {
        MONITOR_OVERLAY_EXTRA_DELAY_MS
    } else {
        INITIAL_HIDE_DELAY_MS
    };

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
        glib::timeout_add_local(
            Duration::from_millis(SELECTION_POLL_INTERVAL_MS),
            move || {
                let sel_result = match sel_rx.try_recv() {
                    Ok(r) => r,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        return glib::ControlFlow::Continue;
                    }
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
                            "Capture failed",
                            &error,
                            &window,
                            &stack,
                            setup_feedback.as_ref(),
                            &preview_status_label,
                            true,
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

                    glib::timeout_add_local(
                        Duration::from_millis(SELECTION_POLL_INTERVAL_MS),
                        move || {
                            let result = match cap_rx.try_recv() {
                                Ok(r) => r,
                                Err(std::sync::mpsc::TryRecvError::Empty) => {
                                    return glib::ControlFlow::Continue;
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
                                    load_preview_image(
                                        &path,
                                        &preview_picture2,
                                        &preview_meta_label2,
                                    );
                                    set_status_neutral(&preview_status_label2, "");
                                    save_button2.set_sensitive(true);
                                    set_action_button_content(&copy_button2, "copy", "Copy");
                                    copy_button2.set_sensitive(true);
                                    reveal_button2.set_sensitive(false);
                                    stack2.set_visible_child_name("preview");
                                }
                                Err(error) => {
                                    report_action_error(
                                        "Capture failed",
                                        &error,
                                        &window2,
                                        &stack2,
                                        setup_feedback2.as_ref(),
                                        &preview_status_label2,
                                        true,
                                    );
                                }
                            }
                            glib::ControlFlow::Break
                        },
                    );
                });

                glib::ControlFlow::Break
            },
        );
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
    let delay_ms = if target == Target::Monitor {
        MONITOR_OVERLAY_EXTRA_DELAY_MS
    } else {
        INITIAL_HIDE_DELAY_MS
    };

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
        glib::timeout_add_local(
            Duration::from_millis(SELECTION_POLL_INTERVAL_MS),
            move || {
                let sel_result = match sel_rx.try_recv() {
                    Ok(r) => r,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        return glib::ControlFlow::Continue;
                    }
                    Err(_) => return glib::ControlFlow::Break,
                };

                if let Some(ov) = overlays_cell.borrow_mut().take() {
                    close_monitor_identifiers(ov);
                }

                let selection = match sel_result {
                    Err(error) => {
                        enable_setup_cta(&setup_cta);
                        report_action_error(
                            "Recording failed",
                            &error,
                            &window,
                            &stack,
                            setup_feedback.as_ref(),
                            &setup_status_label,
                            false,
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
                                "Recording failed",
                                &error,
                                &window2,
                                &stack2,
                                setup_feedback2.as_ref(),
                                &setup_status_label2,
                                false,
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
                                let (w, _dot) =
                                    create_recording_indicator(monitor, &recording_state2);
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
            },
        );
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

fn create_recording_hud(recording_state: &Rc<RefCell<Option<ActiveRecording>>>) -> gtk::Window {
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
    label.set_text("run \"hyprscreen stop\" to end recording");
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
