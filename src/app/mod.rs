use gtk::prelude::*;

pub fn build(startup: Option<crate::cli::StartupAction>) -> gtk::Application {
    let app = gtk::Application::builder()
        .application_id("land.hypr.record")
        .build();

    app.connect_activate(move |app| {
        // Включаем темную тему GTK
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
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

const CSS: &str = r#"
window {
    background: @theme_bg_color;
    color: @theme_fg_color;
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

button:focus, button:focus-visible {
    outline: 2px solid @theme_selected_bg_color;
    outline-offset: 2px;
}

.hs-body {
    padding: 16px 18px 14px 18px;
}

.hs-seg {
    background: mix(@theme_bg_color, @theme_fg_color, 0.05);
    border: 1px solid mix(@theme_bg_color, @theme_fg_color, 0.1);
    border-radius: 8px;
    padding: 3px;
}

.hs-seg > button {
    border: none;
    background: transparent;
    border-radius: 6px;
    padding: 7px 0;
    min-height: 0;
    transition: all 0.1s ease;
}

.hs-seg > button:hover {
    color: @theme_fg_color;
}

.hs-seg > button:checked {
    background: mix(@theme_bg_color, @theme_fg_color, 0.08);
    box-shadow: 0 1px 0 alpha(@theme_fg_color, 0.05) inset, 0 1px 2px alpha(#000, 0.25);
}

.hs-seg > button:focus, .hs-seg > button:focus-visible {
    background: @theme_selected_bg_color;
}

.hs-seg > button:focus .hs-seg-label,
.hs-seg > button:focus-visible .hs-seg-label {
    color: @theme_selected_fg_color;
}

.hs-seg-label {
    color: @insensitive_fg_color;
    font-size: 12.5px;
    font-weight: 500;
    letter-spacing: 0.02em;
}

.hs-seg > button:hover .hs-seg-label,
.hs-seg > button:checked .hs-seg-label {
    color: @theme_fg_color;
}

.hs-tbtn {
    background: mix(@theme_bg_color, @theme_fg_color, 0.05);
    border: 1px solid mix(@theme_bg_color, @theme_fg_color, 0.1);
    border-radius: 8px;
    padding: 0;
    min-height: 0;
    transition: all 0.1s ease;
}

.hs-tbtn:hover {
    background: mix(@theme_bg_color, @theme_fg_color, 0.07);
    border-color: mix(@theme_bg_color, @theme_fg_color, 0.16);
}

.hs-tbtn:active {
    transform: translateY(1px);
}

.hs-tbtn:checked {
    border-color: mix(@theme_bg_color, @theme_fg_color, 0.25);
    background: mix(@theme_bg_color, @theme_fg_color, 0.09);
    box-shadow: 0 0 0 1px alpha(@theme_fg_color, 0.05) inset;
}

.hs-tbtn:focus, .hs-tbtn:focus-visible {
    background: @theme_selected_bg_color;
    border-color: @theme_selected_bg_color;
}

.hs-tbtn:focus .hs-tbtn-label,
.hs-tbtn:focus-visible .hs-tbtn-label {
    color: @theme_selected_fg_color;
}

.hs-tbtn:focus .hs-tbtn-icon,
.hs-tbtn:focus-visible .hs-tbtn-icon {
    opacity: 1;
    filter: brightness(0) invert(1);
}

.hs-tbtn.mode-rec:checked {
    border-color: mix(@error_color, @theme_fg_color, 0.6);
    background: mix(@error_color, @theme_bg_color, 0.15);
}

.hs-tbtn.mode-shot:checked {
    border-color: mix(@theme_selected_bg_color, @theme_fg_color, 0.5);
    background: mix(@theme_selected_bg_color, @theme_bg_color, 0.15);
}

.hs-tbtn-label {
    color: @insensitive_fg_color;
    font-size: 12px;
    font-weight: 500;
    line-height: 1;
}

.hs-tbtn-icon {
    opacity: 0.5;
    transition: all 0.1s ease;
}

.hs-tbtn:hover .hs-tbtn-label,
.hs-tbtn:checked .hs-tbtn-label {
    color: @theme_fg_color;
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
    transition: all 0.1s ease;
}

.hs-primary.mode-shot {
    background: @theme_selected_bg_color;
    color: @theme_selected_fg_color;
}

.hs-primary.mode-rec {
    background: @error_color;
    color: @theme_fg_color;
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

.hs-primary.mode-shot:focus, .hs-primary.mode-shot:focus-visible {
    background: @theme_fg_color;
    color: @theme_selected_bg_color;
    outline: 2px solid @theme_selected_bg_color;
}

.hs-primary.mode-rec:focus, .hs-primary.mode-rec:focus-visible {
    background: @theme_fg_color;
    color: @error_color;
    outline: 2px solid @error_color;
}

.hs-primary:focus .hs-primary-pulse,
.hs-primary:focus-visible .hs-primary-pulse {
    background: currentColor;
}

.hs-primary-label {
    font-size: 13.5px;
    font-weight: 600;
    letter-spacing: 0.01em;
}

.hs-primary-pulse {
    min-width: 8px;
    min-height: 8px;
    background: currentColor;
    border-radius: 999px;
    box-shadow: 0 0 0 0 alpha(currentColor, 0.5);
    animation: hsPulse 1.4s ease-out infinite;
}

@keyframes hsPulse {
    0% { box-shadow: 0 0 0 0 alpha(currentColor, 0.5); }
    70% { box-shadow: 0 0 0 8px alpha(currentColor, 0); }
    100% { box-shadow: 0 0 0 0 alpha(currentColor, 0); }
}

/* Action buttons (Save, Copy, Reveal, Back, New) */
.hs-abtn {
    background: mix(@theme_bg_color, @theme_fg_color, 0.05);
    border: 1px solid mix(@theme_bg_color, @theme_fg_color, 0.1);
    border-radius: 7px;
    padding: 9px 4px 8px 4px;
    min-height: 0;
    transition: all 0.1s ease;
}

.hs-abtn:hover {
    background: mix(@theme_bg_color, @theme_fg_color, 0.07);
    border-color: mix(@theme_bg_color, @theme_fg_color, 0.16);
}

.hs-abtn:active {
    transform: translateY(1px);
}

.hs-abtn:disabled {
    opacity: 0.38;
}

/* Фокус на action кнопках - инверсия */
.hs-abtn:focus, .hs-abtn:focus-visible {
    background: @theme_selected_bg_color;
    border-color: @theme_selected_bg_color;
}

.hs-abtn:focus .hs-abtn-label,
.hs-abtn:focus-visible .hs-abtn-label {
    color: @theme_selected_fg_color;
}

.hs-abtn:focus .hs-abtn-icon,
.hs-abtn:focus-visible .hs-abtn-icon {
    filter: brightness(0) invert(1);
}

.hs-abtn-label {
    color: @theme_fg_color;
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.02em;
    line-height: 1;
}

.hs-abtn-icon {
    transition: all 0.1s ease;
}

.hs-abtn.is-primary {
    background: mix(@theme_selected_bg_color, @theme_bg_color, 0.15);
    border-color: mix(@theme_selected_bg_color, @theme_fg_color, 0.35);
}

.hs-abtn.is-primary.mode-rec {
    background: mix(@error_color, @theme_bg_color, 0.15);
    border-color: mix(@error_color, @theme_fg_color, 0.45);
}

/* Фокус на is-primary action кнопках */
.hs-abtn.is-primary:focus, .hs-abtn.is-primary:focus-visible {
    background: @theme_selected_bg_color;
}

/* HUD кнопка Stop */
.hs-hud-stop {
    border: none;
    background: @error_color;
    color: @theme_fg_color;
    border-radius: 999px;
    padding: 7px 12px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    min-height: 0;
    transition: all 0.1s ease;
}

.hs-hud-stop:hover {
    filter: brightness(1.08);
}

/* Фокус на Stop кнопке */
.hs-hud-stop:focus, .hs-hud-stop:focus-visible {
    background: @theme_fg_color;
    color: @error_color;
    outline: 2px solid @error_color;
}

/* Switch - не меняем при фокусе, только outline */
.hs-switch:focus, .hs-switch:focus-visible {
    outline: 2px solid @theme_selected_bg_color;
    outline-offset: 2px;
}

/* Остальные стили без изменений... */
.hs-optrow {
    padding: 4px 2px 0 2px;
}

.hs-opt-label {
    color: @insensitive_fg_color;
    font-size: 11.5px;
    font-weight: 500;
    letter-spacing: 0.01em;
}

.hs-opt-hint {
    color: @insensitive_fg_color;
    font-family: "JetBrains Mono", "Fira Mono", monospace;
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.04em;
}

.hs-opt-dot {
    min-width: 6px;
    min-height: 6px;
    background: @insensitive_fg_color;
    border-radius: 999px;
}

.hs-optrow.is-on .hs-opt-dot {
    background: @error_color;
    box-shadow: 0 0 6px 1px alpha(@error_color, 0.45);
}

.hs-switch {
    min-width: 34px;
    min-height: 18px;
    padding: 2px;
    border-radius: 999px;
    border: 1px solid mix(@theme_bg_color, @theme_fg_color, 0.1);
    background: mix(@theme_bg_color, @theme_fg_color, 0.09);
    outline: none;
    box-shadow: none;
}

.hs-switch:checked {
    background: mix(@error_color, @theme_bg_color, 0.15);
    border-color: mix(@error_color, @theme_fg_color, 0.6);
}

.hs-switch slider {
    min-width: 12px;
    min-height: 12px;
    border-radius: 999px;
    background: @insensitive_fg_color;
    border: none;
    box-shadow: none;
}

.hs-switch:checked slider {
    background: @error_color;
}

.hs-status {
    font-family: "JetBrains Mono", "Fira Mono", monospace;
    font-size: 11.5px;
    font-weight: 500;
    letter-spacing: 0.02em;
    color: @insensitive_fg_color;
    min-height: 14px;
}

.hs-status.err { color: @error_color; }
.hs-status.ok { color: @success_color; }
.hs-status.live { color: @theme_fg_color; }

.hs-meta {
    color: @insensitive_fg_color;
    font-family: "JetBrains Mono", "Fira Mono", monospace;
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.03em;
}

.hs-preview-frame {
    background: mix(@theme_bg_color, #000, 0.2);
    border: 1px solid mix(@theme_bg_color, @theme_fg_color, 0.1);
    border-radius: 10px;
}

.hs-hud {
    background: alpha(@theme_bg_color, 0.85);
    border: 1px solid mix(@theme_bg_color, @theme_fg_color, 0.15);
    padding: 8px 10px 8px 14px;
    border-radius: 8px;
}

.hs-hud-dot {
    background: @error_color;
    border-radius: 999px;
    min-width: 9px;
    min-height: 9px;
    animation: hudPulse 1.6s ease-out infinite;
}

@keyframes hudPulse {
    0% { box-shadow: 0 0 0 0 alpha(@error_color, 0.55); }
    70% { box-shadow: 0 0 0 7px alpha(@error_color, 0); }
    100% { box-shadow: 0 0 0 0 alpha(@error_color, 0); }
}

.hs-hud-rec {
    color: @insensitive_fg_color;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.14em;
}

.hs-hud-timer {
    color: @theme_fg_color;
    font-family: "JetBrains Mono", "Fira Mono", monospace;
    font-size: 13px;
    font-weight: 500;
    letter-spacing: 0.02em;
}

.hs-hud-sep {
    background: mix(@theme_bg_color, @theme_fg_color, 0.1);
    min-width: 1px;
    min-height: 16px;
}

window.hs-mon-id {
    background: alpha(@theme_bg_color, 0.9);
    border: 2px solid mix(@theme_bg_color, @theme_fg_color, 0.15);
    border-radius: 14px;
}

.hs-mon-id-label {
    color: @theme_fg_color;
    font-family: "JetBrains Mono", "Fira Mono", monospace;
    font-size: 32px;
    font-weight: 600;
    letter-spacing: 0.04em;
    padding: 18px 24px;
}

label.hs-rec-flash {
    color: @error_color;
    font-size: 14px;
    font-weight: 700;
    background: transparent;
}

/* Tinted target-button labels in checked state */
.hs-tbtn.mode-rec:checked .hs-tbtn-label { color: mix(@error_color, @theme_fg_color, 0.7); }
.hs-tbtn.mode-shot:checked .hs-tbtn-label { color: mix(@theme_selected_bg_color, @theme_fg_color, 0.7); }

/* Primary action-button (Save) accent label */
.hs-abtn.is-primary .hs-abtn-label { color: @theme_selected_fg_color; }
.hs-abtn.is-primary.mode-rec .hs-abtn-label { color: mix(@error_color, @theme_fg_color, 0.8); }
"#;
