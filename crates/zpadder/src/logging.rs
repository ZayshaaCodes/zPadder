//! The app's log: `zpadder.log` in ZPadder's data folder, and the console in debug builds.
//!
//! The level is INFO unless `ZPADDER_LOG`, or else `RUST_LOG`, says otherwise, in `tracing`'s
//! directive syntax: `ZPADDER_LOG=debug`, or `RUST_LOG=zpadder_runtime=debug`. The file is kept
//! to about 1 MiB: past that it becomes `zpadder.old.log`, replacing the one before, and a new
//! one starts. Panics are logged too, with their thread and location.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::{self, MakeWriter};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// The size past which the log starts afresh.
const LOG_LIMIT: u64 = 1024 * 1024;

/// The level filter: `ZPADDER_LOG`, else `RUST_LOG`, else INFO.
fn filter() -> EnvFilter {
    let directives =
        std::env::var("ZPADDER_LOG").or_else(|_| std::env::var("RUST_LOG")).unwrap_or_default();
    EnvFilter::builder().with_default_directive(LevelFilter::INFO.into()).parse_lossy(directives)
}

/// The open log file, shared with the logger.
#[derive(Debug, Clone)]
pub(crate) struct Log {
    file: Arc<Mutex<LogFile>>,
    path: PathBuf,
}

impl Log {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Starts keeping the file to [`LOG_LIMIT`]: now, and whenever it grows past it. Only the
    /// ZPadder that keeps running does this, so a launch that merely hands its command line over
    /// never renames the log under the running one.
    pub(crate) fn keep_small(&self) {
        let mut file = self.lock();
        file.rotating = true;
        file.rotate_if_full();
    }

    fn lock(&self) -> MutexGuard<'_, LogFile> {
        self.file.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl<'a> MakeWriter<'a> for Log {
    type Writer = LogWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter(self.lock())
    }
}

/// Writes one log event to the file.
pub(crate) struct LogWriter<'a>(MutexGuard<'a, LogFile>);

impl Write for LogWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// An append-only file that starts afresh once it is full.
#[derive(Debug)]
struct LogFile {
    path: PathBuf,
    old_path: PathBuf,
    file: File,
    size: u64,
    limit: u64,
    /// The size at which to try starting afresh next.
    next_rotation: u64,
    /// Starting afresh is allowed (see [`Log::keep_small`]).
    rotating: bool,
}

impl LogFile {
    fn open(path: PathBuf, old_path: PathBuf, limit: u64) -> io::Result<Self> {
        let file = append(&path)?;
        let size = file.metadata().map_or(0, |metadata| metadata.len());
        Ok(Self { path, old_path, file, size, limit, next_rotation: limit, rotating: false })
    }

    /// Renames a full log to the old log's name and starts a new one. If that fails (say,
    /// someone has the old log open), the file just grows, and the next try is a limit later.
    fn rotate_if_full(&mut self) {
        if !self.rotating || self.size < self.next_rotation {
            return;
        }
        // The file stays open while it is renamed: std opens files with FILE_SHARE_DELETE.
        match fs::rename(&self.path, &self.old_path).and_then(|()| append(&self.path)) {
            Ok(file) => {
                self.file = file;
                self.size = 0;
                self.next_rotation = self.limit;
            }
            Err(_) => self.next_rotation = self.size.saturating_add(self.limit),
        }
    }
}

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Each log event is one write, so a new file always starts with a whole event.
        self.rotate_if_full();
        let written = self.file.write(buf)?;
        self.size += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn open_log() -> io::Result<Log> {
    let (Some(path), Some(old_path)) =
        (zpadder_platform::log_path(), zpadder_platform::old_log_path())
    else {
        return Err(io::Error::other("there is no folder for ZPadder's files"));
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let file = LogFile::open(path.clone(), old_path, LOG_LIMIT)?;
    Ok(Log { file: Arc::new(Mutex::new(file)), path })
}

/// Sets up logging, to the log file and, in debug builds, the console. Returns the log file, if
/// it could be opened.
pub(crate) fn init() -> Option<Log> {
    let opened = open_log();
    let log = opened.as_ref().ok().cloned();
    let file = log.clone().map(|log| {
        fmt::layer().with_writer(log).with_ansi(false).with_target(false).with_thread_names(true)
    });
    let console = cfg!(debug_assertions).then(|| fmt::layer().with_target(false));
    tracing_subscriber::registry().with(filter()).with(file).with(console).init();
    if let Err(e) = opened {
        tracing::warn!("Logging to the console only: cannot open the log file: {e}");
    }
    log
}

/// Logs every panic, with its thread and location, then hands it to the default handling, which
/// prints it to the console in debug builds. (Release builds have no console.)
pub(crate) fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        let location = info.location().map_or_else(
            || "an unknown place".to_owned(),
            |at| format!("{}:{}:{}", at.file(), at.line(), at.column()),
        );
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("no message");
        tracing::error!("Thread '{name}' panicked at {location}: {message}");
        let backtrace = std::backtrace::Backtrace::capture();
        if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
            tracing::error!("Backtrace:\n{backtrace}");
        }
        if cfg!(debug_assertions) {
            default(info);
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system's temporary folder.
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("zpadder-log-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_full_log_starts_afresh_once_allowed() {
        let dir = temp_dir("rotate");
        let (path, old) = (dir.join("zpadder.log"), dir.join("zpadder.old.log"));
        fs::write(&path, "x".repeat(20)).unwrap();
        let mut log = LogFile::open(path.clone(), old.clone(), 16).unwrap();
        // Not allowed yet: a second launch appends without renaming anything.
        log.write_all(b"second launch\n").unwrap();
        assert!(!old.exists());

        log.rotating = true;
        log.write_all(b"first line\n").unwrap();
        assert!(fs::read_to_string(&old).unwrap().ends_with("second launch\n"));
        log.write_all(b"second line\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first line\nsecond line\n");
        // Full again: the old log is replaced.
        log.write_all(b"third\n").unwrap();
        assert_eq!(fs::read_to_string(&old).unwrap(), "first line\nsecond line\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), "third\n");
        drop(log);
        let _ = fs::remove_dir_all(&dir);
    }
}
