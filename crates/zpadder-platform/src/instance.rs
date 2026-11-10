//! One ZPadder per user and session. A second launch hands its command line to the running one
//! over a named pipe and exits, which is how Xpadder-style front-ends drive it.
//!
//! The single-instance mutex and the pipe are named after the session and the user's SID, and
//! only that user and SYSTEM may open them. Both carry a medium integrity label, so a launch that
//! is not elevated still finds, and can talk to, a ZPadder that was restarted as administrator.
//! The pipe accepts local clients only, one at a time, and gives each a couple of seconds and
//! 64 KiB at most, so a stalled client cannot block later launches. Clients connect at the
//! identification impersonation level, so a server squatting on the name could not act as them.

use std::io::{self, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tracing::{error, info, warn};
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA,
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_INBOUND, ReadFile,
    SECURITY_IDENTIFICATION,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, INFINITE, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE, SetEvent,
    WaitForMultipleObjects, WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR};

use crate::security::{
    OwnedHandle, SecurityDescriptor, SendHandle, current_session_id, current_user_sid, io_error, is,
};

/// The longest command line the pipe takes.
const MAX_MESSAGE: usize = 64 * 1024;
/// How long a later launch may take to send its command line once connected.
const CLIENT_DEADLINE: Duration = Duration::from_secs(2);
/// How long a later launch keeps trying to reach a ZPadder that is busy or just starting.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// How long to wait before making the pipe again after it failed.
const PIPE_RETRY: Duration = Duration::from_millis(500);

/// Held by the first instance for as long as it runs. Dropping it stops listening for later
/// launches and frees the slot.
#[derive(Debug)]
pub struct InstanceGuard {
    // Fields drop in order: the pipe closes before the mutex, so whoever takes the slot next can
    // make the pipe at once.
    _server: Server,
    _mutex: OwnedHandle,
}

pub enum Instance {
    /// This is the only ZPadder. Command lines from later launches arrive on the receiver.
    First(InstanceGuard, Receiver<Vec<String>>),
    /// Another ZPadder is already running.
    Running,
}

impl std::fmt::Debug for Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Instance::First(..) => "First",
            Instance::Running => "Running",
        })
    }
}

/// Separate copies (for example a test build) can run side by side by setting
/// `ZPADDER_INSTANCE` to a name; each name gets its own slot, pipe and autostart entry.
/// Characters that named objects cannot hold become `_`.
pub(crate) fn instance_suffix() -> String {
    std::env::var("ZPADDER_INSTANCE").map(|name| format!("-{}", clean(&name))).unwrap_or_default()
}

fn clean(name: &str) -> String {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    name.chars().map(|c| if allowed(c) { c } else { '_' }).collect()
}

/// The single-instance mutex and the pipe.
#[derive(Debug, Clone)]
struct Names {
    mutex: String,
    pipe: String,
}

impl Names {
    fn new(suffix: &str) -> io::Result<Self> {
        let base = format!("ZPadder-{}-{}{suffix}", current_session_id()?, current_user_sid()?);
        Ok(Self { mutex: format!(r"Local\{base}"), pipe: format!(r"\\.\pipe\{base}") })
    }

    fn current() -> io::Result<Self> {
        Self::new(&instance_suffix())
    }
}

/// Claims the single-instance slot, or reports that another ZPadder holds it.
pub fn acquire() -> io::Result<Instance> {
    acquire_with(&Names::current()?, CLIENT_DEADLINE)
}

/// Whether a ZPadder holds the single-instance slot, without claiming it.
pub fn is_running() -> bool {
    Names::current().is_ok_and(|names| mutex_exists(&names.mutex))
}

/// Sends a command line to the running ZPadder. An empty list asks it to show its window.
pub fn send_to_running(args: &[String]) -> io::Result<()> {
    send_to(&Names::current()?.pipe, args)
}

fn acquire_with(names: &Names, client_deadline: Duration) -> io::Result<Instance> {
    let security = SecurityDescriptor::for_current_user()?;
    let attributes = security.attributes();
    // SAFETY: creates (or opens) a named mutex; `attributes` and the descriptor it points to
    // outlive the call, and the handle is owned by the guard.
    let mutex = match unsafe {
        CreateMutexW(Some(&attributes), false, &HSTRING::from(names.mutex.as_str()))
    } {
        Ok(mutex) => OwnedHandle(mutex),
        // It exists, but this process may not open it: an elevated ZPadder made it without the
        // label above (an older version), so it is running all the same.
        Err(e) if is(&e, ERROR_ACCESS_DENIED) => return Ok(Instance::Running),
        Err(e) => return Err(io_error(e)),
    };
    // SAFETY: reads the calling thread's last error, set by CreateMutexW just above.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Ok(Instance::Running);
    }
    let (tx, rx) = mpsc::channel();
    let server = Server::start(names.pipe.clone(), client_deadline, tx)?;
    Ok(Instance::First(InstanceGuard { _server: server, _mutex: mutex }, rx))
}

fn mutex_exists(name: &str) -> bool {
    // SAFETY: opens the mutex only to see whether it exists; the handle is closed at once.
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(name)) } {
        Ok(mutex) => {
            drop(OwnedHandle(mutex));
            true
        }
        // It exists, but this process may not open it.
        Err(e) => is(&e, ERROR_ACCESS_DENIED),
    }
}

/// The thread that reads later launches' command lines. Dropping it stops the thread and waits
/// for it, which closes the pipe.
#[derive(Debug)]
struct Server {
    thread: Option<JoinHandle<()>>,
    /// A manual-reset event that tells the thread to stop. Declared after `thread` but closed
    /// only once `drop` has joined it, as fields drop after `Drop::drop` returns.
    stop: OwnedHandle,
}

impl Server {
    fn start(pipe: String, client_deadline: Duration, tx: Sender<Vec<String>>) -> io::Result<Self> {
        let stop = new_event()?;
        let stop_for_thread = SendHandle(stop.0);
        // Made here, so a launch right after this one returns finds it. If it cannot be made yet,
        // the thread keeps trying.
        let first = SecurityDescriptor::for_current_user()
            .and_then(|security| create_pipe(&pipe, &security))
            .inspect_err(|e| warn!("Later launches cannot reach this ZPadder yet ({e}); retrying."))
            .ok();
        let thread = thread::Builder::new().name("zpadder-ipc".into()).spawn(move || {
            serve(&pipe, first, stop_for_thread, client_deadline, &tx);
        })?;
        Ok(Self { thread: Some(thread), stop })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // SAFETY: the event is open, and stays open until the thread has been joined below.
        let _ = unsafe { SetEvent(self.stop.0) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A new unsignalled manual-reset event.
fn new_event() -> io::Result<OwnedHandle> {
    // SAFETY: creates an unnamed event owned by the returned guard.
    unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map(OwnedHandle).map_err(io_error)
}

/// What became of one client.
#[derive(Debug, PartialEq, Eq)]
enum Client {
    Message(Vec<String>),
    /// The client was dropped; why, for the log.
    Refused(&'static str),
    /// The [`Server`] is being dropped.
    Stop,
}

/// Serves one client after another on `pipe` (made again when missing or broken) until stopped
/// or nobody listens any more. Errors are logged and the pipe made again, so later launches keep
/// getting through.
fn serve(
    name: &str,
    mut pipe: Option<OwnedHandle>,
    stop: SendHandle,
    client_deadline: Duration,
    tx: &Sender<Vec<String>>,
) {
    let stop = stop.0;
    let (security, event) = match (SecurityDescriptor::for_current_user(), new_event()) {
        (Ok(security), Ok(event)) => (security, event),
        (Err(e), _) | (_, Err(e)) => {
            error!("Later launches cannot reach this ZPadder: {e}");
            return;
        }
    };
    // The first failure to make the pipe was already logged when there was no pipe to start with.
    let mut failures = u32::from(pipe.is_none());
    loop {
        let handle = match &pipe {
            Some(OwnedHandle(handle)) => *handle,
            None => match create_pipe(name, &security) {
                Ok(created) => {
                    if failures > 0 {
                        info!("Later launches can reach this ZPadder again.");
                    }
                    failures = 0;
                    pipe.insert(created).0
                }
                Err(e) => {
                    if failures == 0 {
                        warn!("Later launches cannot reach this ZPadder yet ({e}); retrying.");
                    }
                    failures = failures.saturating_add(1);
                    if wait_for(stop, PIPE_RETRY) {
                        return;
                    }
                    continue;
                }
            },
        };
        match serve_client(handle, event.0, stop, client_deadline) {
            Ok(Client::Message(args)) => {
                if tx.send(args).is_err() {
                    return;
                }
            }
            Ok(Client::Refused(why)) => warn!("Ignored a command line from another launch: {why}"),
            Ok(Client::Stop) => return,
            Err(e) => {
                warn!("The pipe for later launches failed ({e}); making it again.");
                pipe = None;
                if wait_for(stop, PIPE_RETRY) {
                    return;
                }
            }
        }
    }
}

fn create_pipe(name: &str, security: &SecurityDescriptor) -> io::Result<OwnedHandle> {
    let attributes = security.attributes();
    // SAFETY: `attributes` and the descriptor it points to outlive the call; the handle is owned
    // by the returned guard.
    let pipe = unsafe {
        CreateNamedPipeW(
            &HSTRING::from(name),
            // If anyone else already made a pipe of this name, fail rather than join it.
            PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            0,
            MAX_MESSAGE as u32,
            0,
            Some(&attributes),
        )
    };
    if pipe.is_invalid() {
        return Err(io::Error::last_os_error());
    }
    Ok(OwnedHandle(pipe))
}

/// Waits for a client, reads its command line and disconnects it, keeping the pipe for the next.
/// An error means the pipe itself is in trouble.
fn serve_client(
    pipe: HANDLE,
    event: HANDLE,
    stop: HANDLE,
    deadline: Duration,
) -> io::Result<Client> {
    let mut overlapped = OVERLAPPED { hEvent: event, ..Default::default() };
    // SAFETY: `overlapped` stays in place until the operation is over: `finish` waits for it
    // (cancelling it first if need be) before anything else touches `overlapped`.
    match unsafe { ConnectNamedPipe(pipe, Some(&mut overlapped)) } {
        Ok(()) => {}
        // A client connected before we started waiting ("no data": and has already left, perhaps
        // after writing its command line; it is still there to read).
        Err(e) if is(&e, ERROR_PIPE_CONNECTED) || is(&e, ERROR_NO_DATA) => {}
        Err(e) if is(&e, ERROR_IO_PENDING) => match finish(pipe, &mut overlapped, stop, None)? {
            Wait::Done(_) => {}
            Wait::Stopped => return Ok(Client::Stop),
            Wait::TimedOut => return Ok(Client::Refused("connecting took too long")),
        },
        Err(e) => return Err(io_error(e)),
    }
    let client = read_message(pipe, &mut overlapped, stop, deadline);
    // SAFETY: no operation is pending on the pipe; this drops the client and readies the pipe for
    // the next one.
    let _ = unsafe { DisconnectNamedPipe(pipe) };
    client
}

/// Reads until the client closes its end, for at most `deadline` and [`MAX_MESSAGE`] bytes.
fn read_message(
    pipe: HANDLE,
    overlapped: &mut OVERLAPPED,
    stop: HANDLE,
    deadline: Duration,
) -> io::Result<Client> {
    let end = Instant::now() + deadline;
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        // SAFETY: `buf` and `overlapped` stay in place until the read is over: `finish` waits for
        // it (cancelling it first if need be) before either is touched again.
        let started = unsafe { ReadFile(pipe, Some(&mut buf), None, Some(overlapped)) };
        match started {
            Ok(()) => {}
            Err(e) if is(&e, ERROR_IO_PENDING) => {}
            // The client has closed its end: that is the whole message.
            Err(e) if is(&e, ERROR_BROKEN_PIPE) || is(&e, ERROR_NO_DATA) => break,
            Err(e) => return Err(io_error(e)),
        }
        let left = end.saturating_duration_since(Instant::now());
        match finish(pipe, overlapped, stop, Some(left)) {
            Ok(Wait::Done(0)) => break,
            Ok(Wait::Done(read)) => {
                if data.len() + read > MAX_MESSAGE {
                    return Ok(Client::Refused("it is longer than 64 KiB"));
                }
                data.extend_from_slice(&buf[..read]);
            }
            Ok(Wait::TimedOut) => return Ok(Client::Refused("it did not finish sending in time")),
            Ok(Wait::Stopped) => return Ok(Client::Stop),
            Err(e) if e.raw_os_error() == Some(ERROR_BROKEN_PIPE.0 as i32) => break,
            Err(e) => return Err(e),
        }
    }
    let text = String::from_utf8_lossy(&data);
    Ok(Client::Message(
        text.split('\0').filter(|arg| !arg.is_empty()).map(str::to_string).collect(),
    ))
}

/// How an overlapped operation ended.
#[derive(Debug)]
enum Wait {
    /// It completed, transferring this many bytes.
    Done(usize),
    TimedOut,
    Stopped,
}

/// Waits for the overlapped operation on `pipe` to complete, for `stop`, or for `timeout`. An
/// operation that has not completed is cancelled and waited for, so when this returns nothing
/// uses `overlapped` (or the operation's buffer) any more.
fn finish(
    pipe: HANDLE,
    overlapped: &mut OVERLAPPED,
    stop: HANDLE,
    timeout: Option<Duration>,
) -> io::Result<Wait> {
    let ms = timeout.map_or(INFINITE, |t| u32::try_from(t.as_millis()).unwrap_or(INFINITE - 1));
    // SAFETY: both handles are open events.
    let woke = unsafe { WaitForMultipleObjects(&[overlapped.hEvent, stop], false, ms) };
    let early = match woke {
        WAIT_OBJECT_0 => None,
        w if w.0 == WAIT_OBJECT_0.0 + 1 => Some(Ok(Wait::Stopped)),
        WAIT_TIMEOUT => Some(Ok(Wait::TimedOut)),
        _ => Some(Err(io::Error::last_os_error())),
    };
    if early.is_some() {
        // SAFETY: cancels this operation on `pipe`; harmless if it has just completed.
        let _ = unsafe { CancelIoEx(pipe, Some(overlapped)) };
    }
    let mut transferred = 0;
    // SAFETY: `overlapped` belongs to an operation started on `pipe`; with bWait set this returns
    // only once the operation is over.
    let result = unsafe { GetOverlappedResult(pipe, overlapped, &mut transferred, true) };
    match early {
        Some(early) => early,
        None => result.map(|()| Wait::Done(transferred as usize)).map_err(io_error),
    }
}

/// Waits up to `timeout` for `stop`. Returns `true` if it was signalled.
fn wait_for(stop: HANDLE, timeout: Duration) -> bool {
    let ms = u32::try_from(timeout.as_millis()).unwrap_or(INFINITE - 1);
    // SAFETY: the event is open.
    let woke = unsafe { WaitForSingleObject(stop, ms) };
    woke == WAIT_OBJECT_0
}

fn send_to(pipe: &str, args: &[String]) -> io::Result<()> {
    let message = args.join("\0");
    if message.len() > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the command line is longer than 64 KiB",
        ));
    }
    let give_up = Instant::now() + CONNECT_TIMEOUT;
    loop {
        let opened = std::fs::OpenOptions::new()
            .write(true)
            // The server may find out who this is, but not act as this process.
            .security_qos_flags(SECURITY_IDENTIFICATION.0)
            .open(pipe);
        match opened {
            Ok(mut pipe) => return pipe.write_all(message.as_bytes()),
            // Busy with another launch, or not listening yet.
            Err(e)
                if (e.kind() == io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32))
                    && Instant::now() < give_up =>
            {
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// Names no real ZPadder uses, different for each test.
    fn test_names() -> Names {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        Names::new(&format!("-unit-test-{}-{n}", std::process::id())).unwrap()
    }

    const SHORT: Duration = Duration::from_millis(300);
    const ARRIVAL: Duration = Duration::from_secs(5);

    fn first(names: &Names) -> (InstanceGuard, Receiver<Vec<String>>) {
        match acquire_with(names, SHORT).unwrap() {
            Instance::First(guard, rx) => (guard, rx),
            Instance::Running => panic!("{} is taken", names.mutex),
        }
    }

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn names_carry_the_session_and_user() {
        let names = Names::new("-x").unwrap();
        let sid = current_user_sid().unwrap();
        assert!(names.mutex.starts_with(r"Local\ZPadder-") && names.mutex.ends_with("-x"));
        assert!(names.pipe.starts_with(r"\\.\pipe\ZPadder-") && names.pipe.contains(&sid));
        assert_eq!(clean(r"dev\copy 2"), "dev_copy_2");
    }

    #[test]
    fn the_second_launch_finds_the_first() {
        let names = test_names();
        assert!(!mutex_exists(&names.mutex));
        let (guard, _rx) = first(&names);
        assert!(mutex_exists(&names.mutex));
        assert!(matches!(acquire_with(&names, SHORT).unwrap(), Instance::Running));
        drop(guard);
        assert!(!mutex_exists(&names.mutex));
        // The slot and the pipe are free again at once.
        let (_guard, rx) = first(&names);
        send_to(&names.pipe, &args(&["again"])).unwrap();
        assert_eq!(rx.recv_timeout(ARRIVAL).unwrap(), ["again"]);
    }

    #[test]
    fn command_lines_reach_the_first_instance() {
        let names = test_names();
        let (_guard, rx) = first(&names);
        send_to(&names.pipe, &args(&[r"C:\p 1.toml", "/M"])).unwrap();
        assert_eq!(rx.recv_timeout(ARRIVAL).unwrap(), [r"C:\p 1.toml", "/M"]);
        // Nothing at all means "show the window".
        send_to(&names.pipe, &[]).unwrap();
        assert!(rx.recv_timeout(ARRIVAL).unwrap().is_empty());
    }

    #[test]
    fn a_stalled_client_does_not_block_the_next() {
        let names = test_names();
        let (_guard, rx) = first(&names);
        // Connects and sends nothing, holding the pipe.
        let stalled = std::fs::OpenOptions::new().write(true).open(&names.pipe).unwrap();
        let started = Instant::now();
        send_to(&names.pipe, &args(&["after"])).unwrap();
        assert_eq!(rx.recv_timeout(ARRIVAL).unwrap(), ["after"]);
        assert!(started.elapsed() < ARRIVAL);
        drop(stalled);
    }

    #[test]
    fn oversized_command_lines_are_refused() {
        let names = test_names();
        let (_guard, rx) = first(&names);
        assert!(send_to(&names.pipe, &["x".repeat(MAX_MESSAGE), "y".into()]).is_err());
        // A client that ignores the limit is cut off, and the next one still gets through.
        let pipe_name = names.pipe.clone();
        let writer = thread::spawn(move || {
            let mut pipe = std::fs::OpenOptions::new().write(true).open(&pipe_name).unwrap();
            let _ = pipe.write_all(&vec![b'z'; MAX_MESSAGE + 4096]);
        });
        writer.join().unwrap();
        send_to(&names.pipe, &args(&["fine"])).unwrap();
        assert_eq!(rx.recv_timeout(ARRIVAL).unwrap(), ["fine"]);
        assert!(rx.try_recv().is_err());
    }
}
