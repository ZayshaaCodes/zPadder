//! OS integration that is neither input nor output: the foreground window and its elevation
//! (for AutoProfile and the UIPI warning), the running programs (to warn about other remappers),
//! whether Windows is locked or going to sleep, autostart, restarting elevated, opening programs,
//! start-up hardening, thread priorities, the data folder and log file, console control events,
//! and single-instance handling with command-line forwarding.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[cfg(windows)]
pub mod console;
#[cfg(windows)]
pub mod instance;
#[cfg(windows)]
mod security;
#[cfg(windows)]
mod session;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use session::{desktop_locked, system_suspending};
#[cfg(windows)]
pub use win::{
    autostart_enabled, autostart_points_elsewhere, autostart_status, current_process_elevation,
    error_box, foreground_window, foreground_window_id, keep_timer_resolution, open_url,
    restart_elevated, restrict_dll_search, running_programs, set_autostart, set_thread_priority,
    shell_open,
};

/// Whether a process runs elevated ("as administrator").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevation {
    Elevated,
    NotElevated,
    /// The process token could not be read. For another process this usually means it runs
    /// with higher privileges than we do.
    Unknown,
}

/// The window that currently receives keyboard input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundWindow {
    /// Opaque identifier, stable while the window exists.
    pub id: u64,
    pub title: String,
    pub pid: u32,
    /// Full path of the owning executable, if it could be read.
    pub exe: Option<PathBuf>,
    pub elevation: Elevation,
}

/// A running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProgram {
    pub pid: u32,
    /// Executable file name, e.g. `steam.exe`.
    pub exe: String,
}

/// Whether ZPadder starts with Windows, from [`autostart_status`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Autostart {
    Off,
    /// The entry starts this copy of ZPadder.
    On,
    /// The entry starts something else, usually another copy of ZPadder that was moved or
    /// unpacked elsewhere. `command` is what it runs (empty if it could not be read). Turning
    /// autostart on with [`set_autostart`] points it at this copy.
    Elsewhere {
        command: String,
    },
}

/// How eagerly the scheduler runs a thread, for [`set_thread_priority`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPriority {
    /// Background chores that must never delay input.
    Low,
    Normal,
    /// The mapper's input and engine threads, so a busy game cannot delay presses.
    AboveNormal,
}

/// The log file's name, in [`data_dir`].
pub const LOG_FILE: &str = "zpadder.log";
/// What the log file is renamed to when it gets too big, replacing the one before.
pub const OLD_LOG_FILE: &str = "zpadder.old.log";

/// Where ZPadder keeps its own files: `ZPADDER_DATA_DIR` if set, beside the executable in
/// portable mode (a `portable.txt` file next to it), otherwise `%APPDATA%\ZPadder`. `None` if the
/// executable's folder cannot be found, or, outside portable mode, `APPDATA` is not set.
///
/// The folder may not exist yet.
pub fn data_dir() -> Option<PathBuf> {
    data_dir_from(
        std::env::var_os("ZPADDER_DATA_DIR"),
        || std::env::current_exe().ok()?.parent().map(Path::to_owned),
        |exe_dir| exe_dir.join("portable.txt").exists(),
        || std::env::var_os("APPDATA"),
    )
}

/// The log file, `zpadder.log` in [`data_dir`].
pub fn log_path() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join(LOG_FILE))
}

/// The previous log file, `zpadder.old.log` in [`data_dir`].
pub fn old_log_path() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join(OLD_LOG_FILE))
}

/// [`data_dir`], with its inputs passed in.
fn data_dir_from(
    env_dir: Option<OsString>,
    exe_dir: impl FnOnce() -> Option<PathBuf>,
    is_portable: impl FnOnce(&Path) -> bool,
    appdata: impl FnOnce() -> Option<OsString>,
) -> Option<PathBuf> {
    // A separate folder, for test runs that must not touch the real settings.
    if let Some(dir) = env_dir {
        return Some(PathBuf::from(dir));
    }
    let exe_dir = exe_dir()?;
    if is_portable(&exe_dir) {
        return Some(exe_dir);
    }
    appdata().map(|appdata| PathBuf::from(appdata).join("ZPadder"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe_dir() -> Option<PathBuf> {
        Some(PathBuf::from(r"D:\Games\ZPadder"))
    }

    fn appdata() -> Option<OsString> {
        Some(r"C:\Users\me\AppData\Roaming".into())
    }

    #[test]
    fn the_data_folder_follows_the_override_then_portable_mode_then_appdata() {
        let dir = data_dir_from(Some(r"T:\dev-data".into()), exe_dir, |_| true, appdata);
        assert_eq!(dir, Some(PathBuf::from(r"T:\dev-data")));

        let dir =
            data_dir_from(None, exe_dir, |dir| dir == Path::new(r"D:\Games\ZPadder"), appdata);
        assert_eq!(dir, Some(PathBuf::from(r"D:\Games\ZPadder")));

        let dir = data_dir_from(None, exe_dir, |_| false, appdata);
        assert_eq!(dir, Some(PathBuf::from(r"C:\Users\me\AppData\Roaming\ZPadder")));
    }

    #[test]
    fn no_data_folder_without_an_executable_folder_or_appdata() {
        // As before: an unknown executable folder means no folder, even with APPDATA set.
        assert_eq!(data_dir_from(None, || None, |_| true, appdata), None);
        assert_eq!(data_dir_from(None, exe_dir, |_| false, || None), None);
    }
}
