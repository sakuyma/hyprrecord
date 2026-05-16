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
