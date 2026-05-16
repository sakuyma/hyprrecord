mod app;
mod capture;
mod cli;
mod config;
mod hyprland;
mod ui;

fn main() -> anyhow::Result<()> {
    cli::run()
}
