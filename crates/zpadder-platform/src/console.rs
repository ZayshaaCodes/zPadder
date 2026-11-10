//! Console control events, for the command-line mapper.
//!
//! Ctrl+C, Ctrl+Break, closing the console window, logging off and shutting down all ask it to
//! stop. For the last three Windows ends the process as soon as the handler returns, so the
//! handler holds on, for a bounded time, until the mapper has let go of every key.

use std::io;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use windows::Win32::System::Console::{
    CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT, SetConsoleCtrlHandler,
};
use windows::core::BOOL;

use crate::security::io_error;

struct Handler {
    stop: Mutex<Sender<()>>,
    /// Set once the caller has finished stopping.
    finished: Mutex<bool>,
    finished_changed: Condvar,
    grace: Duration,
}

static HANDLER: OnceLock<Handler> = OnceLock::new();

/// Requests to stop, from the console. See the [module docs](self).
#[derive(Debug)]
pub struct ConsoleStop {
    requests: Receiver<()>,
}

/// Routes the console's stop requests to the returned [`ConsoleStop`]. When the console window
/// is closed (or the user logs off, or Windows shuts down), the process is kept alive until
/// [`ConsoleStop::finished`] is called or `grace` has passed; keep `grace` under the five
/// seconds Windows allows.
///
/// Can be called once per process.
pub fn console_stop(grace: Duration) -> io::Result<ConsoleStop> {
    let (stop, requests) = mpsc::channel();
    let handler = Handler {
        stop: Mutex::new(stop),
        finished: Mutex::new(false),
        finished_changed: Condvar::new(),
        grace,
    };
    if HANDLER.set(handler).is_err() {
        return Err(io::Error::other("the console handler is already installed"));
    }
    // SAFETY: `on_console_event` is a plain function that stays valid for the whole process.
    unsafe { SetConsoleCtrlHandler(Some(on_console_event), true) }.map_err(io_error)?;
    Ok(ConsoleStop { requests })
}

impl ConsoleStop {
    /// Waits up to `timeout` for a request to stop. Returns `true` once one has come.
    pub fn wait(&self, timeout: Duration) -> bool {
        !matches!(self.requests.recv_timeout(timeout), Err(RecvTimeoutError::Timeout))
    }

    /// Says that stopping is done, so a pending close can let the process end.
    pub fn finished(&self) {
        if let Some(handler) = HANDLER.get() {
            *handler.finished.lock().unwrap_or_else(|e| e.into_inner()) = true;
            handler.finished_changed.notify_all();
        }
    }
}

impl Drop for ConsoleStop {
    fn drop(&mut self) {
        self.finished();
    }
}

/// Called by Windows, on a thread of its own, for each console control event.
unsafe extern "system" fn on_console_event(kind: u32) -> BOOL {
    let Some(handler) = HANDLER.get() else { return false.into() };
    let _ = handler.stop.lock().unwrap_or_else(|e| e.into_inner()).send(());
    if matches!(kind, CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT) {
        let finished = handler.finished.lock().unwrap_or_else(|e| e.into_inner());
        let _ = handler
            .finished_changed
            .wait_timeout_while(finished, handler.grace, |finished| !*finished);
    }
    true.into()
}
