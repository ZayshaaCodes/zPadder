use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use zpadder_platform::{
    Elevation, ForegroundWindow, current_process_elevation, foreground_window, foreground_window_id,
};

use crate::log::Log;

pub(crate) fn run() -> ExitCode {
    let mut log = Log::create("foreground");
    log.line(format_args!("This probe runs {}.", elevation_text(current_process_elevation())));
    log.line("For 60 s, each new foreground window is reported. Open one normally and one with");
    log.line("'Run as administrator' (e.g. Notepad) to check that elevation is detected.");

    let start = Instant::now();
    let mut last = None;
    while start.elapsed() < Duration::from_secs(60) {
        let id = foreground_window_id();
        if id != last {
            last = id;
            if let Some(window) = foreground_window() {
                log.line(describe(&window));
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    ExitCode::SUCCESS
}

/// One-line description of a window, with a warning when UIPI will block injected input.
pub(crate) fn describe(window: &ForegroundWindow) -> String {
    let exe = window
        .exe
        .as_deref()
        .and_then(|p| p.file_name())
        .map_or_else(|| "?".into(), |name| name.to_string_lossy().into_owned());
    let mut text = format!(
        "\"{}\" - {exe} (pid {}), {}",
        window.title,
        window.pid,
        elevation_text(window.elevation)
    );
    if window.elevation != Elevation::NotElevated
        && current_process_elevation() == Elevation::NotElevated
    {
        text.push_str(" -- WARNING: Windows will silently drop ZPadder's input to this window");
    }
    text
}

fn elevation_text(elevation: Elevation) -> &'static str {
    match elevation {
        Elevation::Elevated => "elevated",
        Elevation::NotElevated => "not elevated",
        Elevation::Unknown => "elevation unknown (probably elevated)",
    }
}
