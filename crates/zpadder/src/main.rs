//! ZPadder: map game controller input to keyboard and mouse.
//!
//! Command line, compatible with Xpadder's so front-ends such as LaunchBox can drive it:
//!
//! ```text
//! zpadder ["profile.toml" ...] ["Program.lnk"] [/M] [/C]
//! ```
//!
//! - a profile path opens that profile (Xpadder's per-controller profiles beyond the first are
//!   not supported yet; the first applies to every controller);
//! - a `.lnk`, `.exe`, `.url` or `.bat` is launched once ZPadder is running;
//! - `/M` starts minimized to the tray; `/C` closes a running ZPadder.
//!
//! The log is `zpadder.log` in ZPadder's data folder (see [`logging`]).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod logging;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use zpadder_platform::Elevation;
use zpadder_platform::instance::{self, Instance};

/// Tells the OpenGL loader to load another `opengl32.dll` (such as Mesa's software renderer). An
/// elevated ZPadder ignores it, so nothing in the environment can make it run someone's DLL.
const GL_LIBRARY_VAR: &str = "GLUTIN_WGL_OPENGL_DLL";

#[derive(Debug, Default)]
struct Args {
    profiles: Vec<PathBuf>,
    launch: Option<PathBuf>,
    minimized: bool,
    close: bool,
}

fn parse(args: impl Iterator<Item = String>) -> Args {
    let mut parsed = Args::default();
    for arg in args {
        let lower = arg.to_ascii_lowercase();
        match lower.as_str() {
            "/m" | "-m" | "--minimized" => parsed.minimized = true,
            "/c" | "-c" | "--close" => parsed.close = true,
            _ if [".lnk", ".exe", ".url", ".bat", ".cmd"]
                .iter()
                .any(|ext| lower.ends_with(ext)) =>
            {
                parsed.launch = Some(PathBuf::from(arg));
            }
            _ => parsed.profiles.push(PathBuf::from(arg)),
        }
    }
    parsed
}

/// Passed by ZPadder to itself when restarting as administrator: wait for the old instance to
/// exit instead of handing over to it.
const AFTER_RESTART: &str = "--after-restart";

fn main() -> ExitCode {
    // First, before anything can load a DLL.
    let dll_search = zpadder_platform::restrict_dll_search();
    let elevated = zpadder_platform::current_process_elevation() == Elevation::Elevated;
    let ignore_gl_library = elevated && std::env::var_os(GL_LIBRARY_VAR).is_some();
    if ignore_gl_library {
        // SAFETY: changing the environment is always sound on Windows, which guards it (see
        // `std::env::remove_var`); besides, no other thread of ours runs yet.
        unsafe { std::env::remove_var(GL_LIBRARY_VAR) };
    }
    let log = logging::init();
    logging::log_panics();
    let log_path = log.as_ref().map(|log| log.path().to_owned());
    tracing::info!("ZPadder {} starting", env!("CARGO_PKG_VERSION"));
    if let Err(e) = dll_search {
        tracing::warn!("Cannot limit where DLLs are loaded from: {e}");
    }
    if ignore_gl_library {
        tracing::info!(
            "Ignoring {GL_LIBRARY_VAR}: running as administrator, so OpenGL comes from Windows."
        );
    }

    let raw: Vec<String> = std::env::args().skip(1).collect();
    let after_restart = raw.iter().any(|arg| arg == AFTER_RESTART);
    let args = parse(raw.into_iter().filter(|arg| arg != AFTER_RESTART));

    let (guard, commands) = match acquire_instance(after_restart) {
        Ok(Instance::Running) => return hand_over(&args, log_path.as_deref()),
        Ok(Instance::First(guard, commands)) => (guard, commands),
        Err(e) => {
            fatal(
                &format!("ZPadder cannot tell whether it is already running: {e}"),
                log_path.as_deref(),
            );
            return ExitCode::FAILURE;
        }
    };
    if let Some(log) = &log {
        log.keep_small();
    }
    if args.close {
        // Nothing was running to close.
        return ExitCode::SUCCESS;
    }
    if args.profiles.len() > 1 {
        tracing::warn!(
            "Only the first profile is used; per-controller profiles are not supported yet."
        );
    }
    if let Some(program) = args.launch.clone() {
        // In the background, so the window opens meanwhile.
        std::thread::spawn(move || launch(&program));
    }
    let launch = zpadder_gui::Launch {
        profile: args.profiles.into_iter().next(),
        minimized: args.minimized,
        remote: Some(commands),
    };
    // A panic is logged by the hook; this tells the user, who has no console to see it on.
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| zpadder_gui::run(launch)));
    let code = match run {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Err(_) => {
            fatal("ZPadder stopped because of an internal error.", log_path.as_deref());
            ExitCode::FAILURE
        }
        Ok(Err(e)) => {
            fatal(
                &format!(
                    "ZPadder cannot open its window: {e}\n\nThis usually means the graphics \
                     driver does not offer OpenGL 2.0 or later. Updating the graphics driver \
                     often helps."
                ),
                log_path.as_deref(),
            );
            ExitCode::FAILURE
        }
    };
    drop(guard);
    tracing::info!("ZPadder stopped");
    code
}

/// Claims the single-instance slot. After an elevated restart the old instance may still be
/// closing, so wait for it rather than handing over.
fn acquire_instance(wait_for_old: bool) -> std::io::Result<Instance> {
    let attempts = if wait_for_old { 50 } else { 1 };
    for _ in 1..attempts {
        match instance::acquire()? {
            Instance::Running => std::thread::sleep(std::time::Duration::from_millis(100)),
            first => return Ok(first),
        }
    }
    instance::acquire()
}

/// Another ZPadder is running: hands it this command line, starts the program asked for, and
/// exits. Never starts a second mapper; if the running one cannot be reached, says so.
fn hand_over(args: &Args, log: Option<&Path>) -> ExitCode {
    let code = match forward(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            fatal(
                &format!(
                    "ZPadder is already running, but did not answer: {e}\n\nIt may be busy or \
                     closing; try again in a moment. If this keeps happening, quit ZPadder from \
                     its tray icon (or end it in Task Manager) and start it again."
                ),
                log,
            );
            ExitCode::FAILURE
        }
    };
    if let Some(program) = &args.launch {
        launch(program);
    }
    code
}

/// Hands this command line to the running ZPadder.
fn forward(args: &Args) -> std::io::Result<()> {
    let mut message: Vec<String> = args
        .profiles
        .iter()
        .map(|path| {
            std::path::absolute(path).unwrap_or_else(|_| path.clone()).display().to_string()
        })
        .collect();
    if args.minimized {
        message.push("/M".into());
    }
    if args.close {
        message.push("/C".into());
    }
    instance::send_to_running(&message)
}

/// Opens a shortcut or program the way Explorer would.
fn launch(program: &Path) {
    if let Err(e) = zpadder_platform::shell_open(program) {
        tracing::error!("Cannot start {}: {e}", program.display());
    }
}

/// Reports an error that stops ZPadder, in the log and in a message box: release builds have no
/// console.
fn fatal(message: &str, log: Option<&Path>) {
    tracing::error!("{message}");
    let text = match log {
        Some(path) => format!("{message}\n\nDetails are in the log: {}", path.display()),
        None => message.to_owned(),
    };
    zpadder_platform::error_box(&text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xpadder_style_arguments() {
        let args = ["p1.toml", "P2.toml", "Game.lnk", "/M"].map(String::from);
        let parsed = parse(args.into_iter());
        assert_eq!(parsed.profiles, [PathBuf::from("p1.toml"), PathBuf::from("P2.toml")]);
        assert_eq!(parsed.launch, Some(PathBuf::from("Game.lnk")));
        assert!(parsed.minimized);
        assert!(!parsed.close);
        assert!(parse(["/C".to_string()].into_iter()).close);
    }
}
