//! Runs the mapper in the background, for both the app and `zpadder run`.
//!
//! [`Mapper::start`] spawns four threads:
//!
//! - the **input thread** owns SDL. It forwards controller events, keeps the live [`Status`]
//!   (connected pads, pressed buttons, battery), applies rumble, watches the foreground window
//!   for AutoProfile, elevated windows and ZPadder's own window, and notices when Windows locks,
//!   shows a secure screen or goes to sleep;
//! - the **engine thread** owns the [`Engine`] and the output sink, decides which profile is
//!   in effect, and records what each step did in the [activity log](Mapper::activity_since);
//! - a low-priority **watch thread** looks out for other remappers that could make games see each
//!   press twice;
//! - the **xbox thread** plugs in and drives the virtual Xbox controllers of a profile with a
//!   `virtual_pad` (see [`virtual_pad`]), and passes rumble from games back to the input thread.
//!
//! Mapping pauses while a ZPadder window has focus, so editing a profile never types into the
//! editor itself. The editor's test bench can keep it on ([`Mapper::set_testing`]): keys are then
//! sent as usual, but mouse output is held back and only logged, so the pad cannot click around
//! the editor. Mapping also lets go of everything while Windows is locked or shows a secure
//! screen, and resumes when the desktop is back.
//!
//! Nothing can leave a key stuck down: the output side keeps its own record of every key and
//! mouse button it holds (a [`TrackingSink`]), releases it all if the engine thread panics, and
//! retries releases that Windows refused until they go out. Likewise no virtual controller is
//! left behind: they are unplugged when no longer wanted, when mapping stops and if anything
//! panics, and they stay neutral while mapping is paused or the test bench is in use.

mod activity;
mod gate;
#[cfg(windows)]
mod vigem;
mod virtual_pad;

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tracing::{debug, error, info, warn};
use zpadder_engine::Engine;
use zpadder_input::InputBackend;
pub use zpadder_input::sdl::Power;
use zpadder_input::sdl::{SdlBackend, xinput_slot};
use zpadder_model::{Axis, Button, InputEvent, OutputAction, PadId, Profile, XboxState};
use zpadder_output::{OutputSink, SendError, SendInputSink, TrackingSink};
use zpadder_platform::{
    Elevation, ForegroundWindow, RunningProgram, ThreadPriority, current_process_elevation,
    desktop_locked, foreground_window, foreground_window_id, keep_timer_resolution,
    running_programs, set_thread_priority, system_suspending,
};

pub use crate::activity::{ACTIVITY_CAPACITY, Activity, Origin, PadInput, Side};
use crate::activity::{ActivityLog, Cause, Recorder, Step};
use crate::gate::MouseGate;
pub use crate::virtual_pad::VirtualPadStatus;
use crate::virtual_pad::{OwnPadFilter, OwnPads, VirtualCmd};

/// Loads a profile for a program when that program's window has focus.
#[derive(Debug, Clone)]
pub struct AutoProfile {
    /// Executable file name, e.g. `game.exe`, ignoring case. `*` stands for any run of
    /// characters and `?` for any one, so `FiveM_*GTAProcess.exe` matches every FiveM build.
    pub program: String,
    /// If set, the window title must also contain this text (case-insensitive).
    pub title_contains: Option<String>,
    pub profile: Arc<Profile>,
}

impl AutoProfile {
    fn matches(&self, window: &ForegroundWindow) -> bool {
        let exe = window.exe.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy());
        let program_ok = exe.is_some_and(|exe| wildcard_match(&self.program, &exe));
        let title_ok = self
            .title_contains
            .as_ref()
            .is_none_or(|text| window.title.to_lowercase().contains(&text.to_lowercase()));
        program_ok && title_ok
    }
}

/// Whether `name` matches `pattern`, ignoring case: `*` in the pattern stands for any run of
/// characters (none included) and `?` for any one character.
fn wildcard_match(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let (mut p, mut n) = (0, 0);
    // The last `*` seen, and where in `name` it is currently taken to end.
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((star_p, star_n)) = star {
            // Let the `*` take one more character and try again from there.
            p = star_p + 1;
            n = star_n + 1;
            star = Some((star_p, star_n + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// What the mapper is doing right now, for display.
#[derive(Debug, Clone, Default)]
pub struct Status {
    /// Mapping is switched on (it may still be paused while ZPadder has focus).
    pub enabled: bool,
    /// Mapping is paused because a ZPadder window has focus and the test bench is not in use.
    pub paused: bool,
    /// A ZPadder window has focus and the test bench keeps mapping on (if it is switched on):
    /// keys are sent, mouse output is held back and only logged.
    pub testing: bool,
    /// Windows is locked, showing a secure screen (a UAC prompt, the Ctrl+Alt+Del screen) or going
    /// to sleep: mapping has let go of everything and waits for the desktop to come back.
    pub locked: bool,
    /// Name of the profile in effect, if any.
    pub profile: Option<String>,
    /// The AutoProfile rule's program, when a rule chose the profile.
    pub auto_program: Option<String>,
    pub pads: Vec<PadStatus>,
    /// Program and title of the foreground window.
    pub foreground: Option<(String, String)>,
    /// Title of the foreground window when it is elevated and ZPadder is not, meaning Windows
    /// will block ZPadder's input to it.
    pub blocked_window: Option<String>,
    /// Friendly names of other running programs that may also read the controller, e.g.
    /// "Steam" or "DS4Windows", so games may get each press twice.
    pub other_mappers: Vec<String>,
    /// Whether the profile's virtual Xbox controllers are working, and if not, why.
    pub virtual_pads: VirtualPadStatus,
    /// Why mapping does not work, for the user. Once the mapper has stopped because of an
    /// internal error this stays set; a new [`Mapper`] is needed to map again.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PadStatus {
    pub id: PadId,
    pub name: String,
    /// SDL's controller type, e.g. `xbox360`, `ps4`, `ps5`, `switchpro`.
    pub kind: String,
    pub power: Power,
    pub pressed: BTreeSet<Button>,
    /// Indexed like [`Axis`]: left X/Y, right X/Y, left trigger, right trigger.
    pub axes: [f32; 6],
    /// Index of the active layer.
    pub layer: usize,
}

/// Shown when the engine thread panicked and everything it held could be released.
const ENGINE_FAILED: &str =
    "Mapping stopped because of an internal error; all keys were released. See the log.";
/// Shown when the engine thread panicked and Windows has not taken the releases yet.
const ENGINE_FAILED_HOLDING: &str = "Mapping stopped because of an internal error. Windows has \
     not accepted the release of every held key yet; ZPadder keeps trying. See the log.";
/// Shown when the engine thread ended some other way.
const ENGINE_STOPPED: &str = "Mapping stopped because of an internal error. See the log.";
/// Shown when the input thread panicked.
const INPUT_FAILED: &str = "Controller input stopped because of an internal error, so mapping \
     stopped; all keys were released. See the log.";

enum ToEngine {
    Input(Vec<InputEvent>),
    /// A pad about to be reported connected is read through XInput: games see it as an Xbox
    /// controller already, so it never gets a virtual one.
    XInputPad(PadId),
    Profile(Option<Arc<Profile>>),
    Auto(Option<(String, Arc<Profile>)>),
    Enabled(bool),
    Paused(bool),
    Testing(bool),
    /// Windows is locked, shows a secure screen or is going to sleep, or is back.
    Locked(bool),
    Stop,
}

/// What makes a pad rumble, for the input thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rumble {
    /// An assignment with `rumble = true` is held, or no longer is.
    Assignment(PadId, bool),
    /// A game set the motors of the pad's virtual Xbox controller (0–255 each).
    Game { pad: PadId, large: u8, small: u8 },
}

/// How strongly an assignment with `rumble = true` rumbles, on both motors.
const ASSIGNMENT_RUMBLE: u16 = 0xA000;
/// How long SDL is asked to rumble for...
const RUMBLE_FOR: Duration = Duration::from_secs(60);
/// ...and how often a rumble that goes on is asked for again, so it never runs out.
const RUMBLE_REFRESH: Duration = Duration::from_secs(30);

/// A pad's rumble: the stronger of its assignments' and the game's, motor by motor.
#[derive(Debug, Default)]
struct RumbleMix {
    assignment: bool,
    /// The game's large (low-frequency) and small (high-frequency) motors.
    game: (u8, u8),
    sent: (u16, u16),
    sent_at: Option<Instant>,
}

impl RumbleMix {
    fn apply(&mut self, rumble: Rumble) {
        match rumble {
            Rumble::Assignment(_, on) => self.assignment = on,
            Rumble::Game { large, small, .. } => self.game = (large, small),
        }
    }

    /// Low- and high-frequency strengths.
    fn strengths(&self) -> (u16, u16) {
        let base = if self.assignment { ASSIGNMENT_RUMBLE } else { 0 };
        let scale = |motor: u8| u16::from(motor) * 257;
        (base.max(scale(self.game.0)), base.max(scale(self.game.1)))
    }

    /// What to ask SDL for at `now`, if anything: strengths and for how long.
    fn due(&mut self, now: Instant) -> Option<(u16, u16, Duration)> {
        let want = self.strengths();
        let stale = self.sent_at.is_some_and(|at| now.duration_since(at) >= RUMBLE_REFRESH);
        if want == self.sent && !(want != (0, 0) && stale) {
            return None;
        }
        self.sent = want;
        self.sent_at = Some(now);
        let duration = if want == (0, 0) { Duration::ZERO } else { RUMBLE_FOR };
        Some((want.0, want.1, duration))
    }
}

impl Rumble {
    fn pad(self) -> PadId {
        match self {
            Rumble::Assignment(pad, _) | Rumble::Game { pad, .. } => pad,
        }
    }
}

struct Shared {
    status: Mutex<Status>,
    activity: Mutex<ActivityLog>,
    auto_profiles: Mutex<Vec<AutoProfile>>,
    /// Set when the rules change, so the input thread re-checks the foreground window.
    rules_changed: AtomicBool,
    stop: AtomicBool,
    /// Set when the engine thread has stopped because of an internal error.
    engine_failed: AtomicBool,
    /// ZPadder's own virtual controllers, which the input thread must never read.
    own_pads: Arc<Mutex<OwnPads>>,
    notify: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn new(notify: Box<dyn Fn() + Send + Sync>) -> Self {
        Self {
            status: Mutex::new(Status { enabled: true, ..Status::default() }),
            activity: Mutex::new(ActivityLog::default()),
            auto_profiles: Mutex::new(Vec::new()),
            rules_changed: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            engine_failed: AtomicBool::new(false),
            own_pads: Arc::new(Mutex::new(OwnPads::default())),
            notify,
        }
    }

    fn status(&self) -> MutexGuard<'_, Status> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn activity(&self) -> MutexGuard<'_, ActivityLog> {
        self.activity.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Shows `message` as the mapper's error and tells the UI.
    fn fail(&self, message: &str) {
        self.status().error = Some(message.to_owned());
        (self.notify)();
    }
}

/// The running mapper. Dropping it stops mapping and releases every held key.
pub struct Mapper {
    to_engine: Sender<ToEngine>,
    shared: Arc<Shared>,
    /// Dropped to stop the watch thread.
    stop_watch: Option<Sender<()>>,
    threads: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for Mapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mapper").finish_non_exhaustive()
    }
}

impl Mapper {
    /// Starts mapping, enabled, with no profile. `notify` is called (from a background thread)
    /// whenever [`Mapper::status`] or the activity log changes; a UI uses it to repaint.
    pub fn start(notify: impl Fn() + Send + Sync + 'static) -> Self {
        // Turbo and the pointer run on a 1 ms tick, which needs the timer resolution SDL asks
        // for even while ZPadder sits in the tray.
        if let Err(e) = keep_timer_resolution() {
            debug!("Cannot keep the timer resolution while in the background: {e}");
        }
        let shared = Arc::new(Shared::new(Box::new(notify)));
        let start = Instant::now();
        let (to_engine, engine_rx) = mpsc::channel();
        let (rumble_tx, rumble_rx) = mpsc::channel();
        let (mappers_tx, mappers_rx) = mpsc::channel();
        let (stop_watch, watch_rx) = mpsc::channel();
        let (virtual_tx, virtual_rx) = mpsc::channel();
        let input = thread::Builder::new()
            .name("zpadder-input".into())
            .spawn({
                let shared = Arc::clone(&shared);
                let to_engine = to_engine.clone();
                move || run_input_thread(&shared, &to_engine, &rumble_rx, &mappers_rx)
            })
            .expect("cannot start the input thread");
        let xbox = thread::Builder::new()
            .name("zpadder-xbox".into())
            .spawn({
                let shared = Arc::clone(&shared);
                #[cfg(windows)]
                let backend = vigem::ViGEm::new(rumble_tx.clone());
                #[cfg(not(windows))]
                let backend = virtual_pad::NoDriver;
                // Ends once the engine thread, the only sender, has gone.
                move || virtual_pad::run_xbox_thread(&shared, &virtual_rx, backend, start)
            })
            .expect("cannot start the Xbox controller thread");
        let engine = thread::Builder::new()
            .name("zpadder-engine".into())
            .spawn({
                let shared = Arc::clone(&shared);
                move || engine_thread(&shared, &engine_rx, &rumble_tx, virtual_tx, start)
            })
            .expect("cannot start the engine thread");
        let watch = thread::Builder::new()
            .name("zpadder-watch".into())
            .spawn(move || watch_thread(&watch_rx, &mappers_tx))
            .expect("cannot start the watch thread");
        Self {
            to_engine,
            shared,
            stop_watch: Some(stop_watch),
            threads: vec![input, engine, watch, xbox],
        }
    }

    /// Sets the profile used when no AutoProfile rule matches. `None` maps nothing.
    pub fn set_profile(&self, profile: Option<Arc<Profile>>) {
        let _ = self.to_engine.send(ToEngine::Profile(profile));
    }

    /// Replaces the AutoProfile rules. The first matching rule wins.
    pub fn set_auto_profiles(&self, rules: Vec<AutoProfile>) {
        *self.shared.auto_profiles.lock().unwrap_or_else(|e| e.into_inner()) = rules;
        self.shared.rules_changed.store(true, Ordering::SeqCst);
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.to_engine.send(ToEngine::Enabled(enabled));
    }

    /// Keeps mapping on while ZPadder has focus, for the editor's test bench, or stops doing so.
    ///
    /// While it is on and ZPadder has focus, keys are sent as usual, but mouse buttons, pointer
    /// movement and the wheel are held back and only logged, so the pad cannot click around the
    /// editor. A mouse button already down when testing starts is still released. Turning testing
    /// off while ZPadder has focus releases everything held, as pausing does. Mapping that is
    /// switched off stays off.
    pub fn set_testing(&self, on: bool) {
        let _ = self.to_engine.send(ToEngine::Testing(on));
    }

    pub fn status(&self) -> Status {
        self.shared.status().clone()
    }

    /// The activity log entries newer than `seq`, oldest first. Pass 0 to get every entry kept,
    /// then the newest entry's [`Activity::seq`]: only new entries are cloned, so a UI can call
    /// this every frame. The log keeps the last [`ACTIVITY_CAPACITY`] entries.
    pub fn activity_since(&self, seq: u64) -> Vec<Activity> {
        self.shared.activity().since(seq)
    }

    /// Stops mapping, releases every held key and waits for the threads to finish.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = self.to_engine.send(ToEngine::Stop);
        self.stop_watch.take();
        for thread in self.threads.drain(..) {
            let name = thread.thread().name().unwrap_or("mapper").to_owned();
            // The xbox thread can be waiting on the driver, which ZPadder does not control:
            // quitting must not hang on it.
            if name == "zpadder-xbox" && !finishes_within(&thread, XBOX_STOP_WAIT) {
                warn!("The Xbox controller thread is still waiting for the ViGEmBus driver.");
                continue;
            }
            if let Err(payload) = thread.join() {
                error!("The {name} thread panicked: {}", panic_message(&*payload));
            }
        }
    }
}

impl Drop for Mapper {
    fn drop(&mut self) {
        self.stop();
    }
}

/// How long stopping waits for the xbox thread to unplug the virtual controllers.
const XBOX_STOP_WAIT: Duration = Duration::from_secs(3);

/// Whether `thread` finishes within `wait`.
fn finishes_within(thread: &JoinHandle<()>, wait: Duration) -> bool {
    let start = Instant::now();
    while !thread.is_finished() {
        if start.elapsed() >= wait {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
    true
}

/// The text of a panic, for the log.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message")
}

/// How long the input thread waits for controller input while a pad is connected. SDL checks for
/// input every millisecond meanwhile, so this only bounds how often the rest of the loop runs.
const ACTIVE_POLL: Duration = Duration::from_millis(10);
/// How often the input thread looks for input while no pad is connected: often enough to notice
/// a pad being plugged in promptly, rarely enough that an idle ZPadder barely wakes up.
const IDLE_POLL: Duration = Duration::from_millis(100);
/// How often battery levels are refreshed.
const POWER_INTERVAL: Duration = Duration::from_secs(5);
/// How often the running programs are checked for other remappers.
const MAPPERS_INTERVAL: Duration = Duration::from_secs(10);
/// How often the input desktop is checked, to notice the lock screen and secure screens.
const LOCK_INTERVAL: Duration = Duration::from_millis(250);
/// How often the foreground window's title is read again while an AutoProfile rule looks at
/// titles, since a program can change its title without the focus moving.
const TITLE_INTERVAL: Duration = Duration::from_millis(500);
/// Minimum gap between status notifications caused by stick and trigger motion.
const NOTIFY_INTERVAL: Duration = Duration::from_millis(16);
/// The smallest stick or trigger movement worth a repaint, so jitter at rest causes none.
const AXIS_STEP: f32 = 0.01;

/// Programs that also read controllers, by executable name, with the name users know them by.
/// While one runs alongside ZPadder, games may get each press twice.
const OTHER_MAPPERS: [(&str, &str); 8] = [
    ("steam.exe", "Steam"),
    ("DS4Windows.exe", "DS4Windows"),
    ("reWASDTray.exe", "reWASD"),
    ("reWASDEngine.exe", "reWASD"),
    ("DSX.exe", "DSX"),
    ("JoyToKey.exe", "JoyToKey"),
    ("antimicrox.exe", "AntiMicroX"),
    ("Xpadder.exe", "Xpadder"),
];

/// Runs the input thread. If it panics, mapping stops (the engine lets go of everything) and the
/// status says why.
fn run_input_thread(
    shared: &Shared,
    to_engine: &Sender<ToEngine>,
    rumble: &Receiver<Rumble>,
    mappers: &Receiver<Vec<String>>,
) {
    let run = panic::catch_unwind(AssertUnwindSafe(|| {
        input_thread(shared, to_engine, rumble, mappers);
    }));
    if let Err(payload) = run {
        error!("Controller input stopped after an internal error: {}", panic_message(&*payload));
        let _ = to_engine.send(ToEngine::Stop);
        shared.fail(INPUT_FAILED);
    }
}

fn input_thread(
    shared: &Shared,
    to_engine: &Sender<ToEngine>,
    rumble: &Receiver<Rumble>,
    mappers: &Receiver<Vec<String>>,
) {
    let mut backend = match SdlBackend::new() {
        Ok(backend) => backend,
        Err(e) => {
            error!("SDL failed to start: {e}");
            shared.fail(&format!("Controller input failed to start: {e}"));
            return;
        }
    };
    // ZPadder's own virtual Xbox controllers are never read, or they would be mapped again.
    backend.set_filter(Box::new(OwnPadFilter::new(Arc::clone(&shared.own_pads))));
    let mut rumbles: HashMap<PadId, RumbleMix> = HashMap::new();
    if let Err(e) = set_thread_priority(ThreadPriority::AboveNormal) {
        debug!("Cannot raise the input thread's priority: {e}");
    }
    let we_are_elevated = current_process_elevation() == Elevation::Elevated;
    let own_pid = std::process::id();
    let mut foreground = None;
    // The foreground window's title, while it is not ZPadder's, for title rules.
    let mut foreground_title: Option<String> = None;
    let mut title_rules = false;
    let mut last_title = Instant::now();
    let mut lock = LockWatch::default();
    let mut events = Vec::new();
    let mut last_power = Instant::now();
    let mut last_notify = Instant::now();
    let mut pending_notify = false;
    // The stick and trigger positions the UI was last told about.
    let mut shown: HashMap<PadId, [f32; 6]> = HashMap::new();

    while !shared.stopping() {
        if backend.pad_count() == 0 {
            backend.poll(Duration::ZERO, &mut events);
            if events.is_empty() {
                thread::sleep(IDLE_POLL);
            }
        } else {
            backend.poll(ACTIVE_POLL, &mut events);
        }
        if shared.engine_failed.load(Ordering::SeqCst) {
            engine_lost(shared, &mut backend);
            return;
        }

        while let Ok(change) = rumble.try_recv() {
            if backend.has_pad(change.pad()) {
                rumbles.entry(change.pad()).or_default().apply(change);
            }
        }
        let now = Instant::now();
        for (&pad, mix) in &mut rumbles {
            if let Some((low, high, duration)) = mix.due(now) {
                backend.rumble(pad, low, high, duration);
            }
        }

        if !events.is_empty() {
            let mut xinput_pads = Vec::new();
            let mut status = shared.status();
            for event in &events {
                match *event {
                    InputEvent::Connected { pad } => {
                        if let Some(info) = backend.pad_info(pad) {
                            info!("Controller connected: {} ({})", info.name, info.kind);
                            if xinput_slot(&info.path).is_some() {
                                xinput_pads.push(pad);
                            }
                            status.pads.retain(|p| p.id != pad);
                            status.pads.push(PadStatus {
                                id: pad,
                                name: info.name,
                                kind: info.kind,
                                power: info.power,
                                pressed: BTreeSet::new(),
                                axes: [0.0; 6],
                                layer: 0,
                            });
                        }
                        pending_notify = true;
                    }
                    InputEvent::Disconnected { pad } => {
                        info!("Controller disconnected");
                        status.pads.retain(|p| p.id != pad);
                        rumbles.remove(&pad);
                        pending_notify = true;
                    }
                    InputEvent::Button { pad, button, pressed } => {
                        if let Some(p) = status.pads.iter_mut().find(|p| p.id == pad) {
                            if pressed {
                                p.pressed.insert(button);
                            } else {
                                p.pressed.remove(&button);
                            }
                        }
                        pending_notify = true;
                    }
                    InputEvent::Axis { pad, axis, value } => {
                        if let Some(p) = status.pads.iter_mut().find(|p| p.id == pad) {
                            let i = axis_index(axis);
                            p.axes[i] = value;
                            let was = shown.get(&pad).map_or(0.0, |axes| axes[i]);
                            pending_notify |= axis_moved(was, value);
                        }
                    }
                }
            }
            drop(status);
            for pad in xinput_pads {
                let _ = to_engine.send(ToEngine::XInputPad(pad));
            }
            if to_engine.send(ToEngine::Input(std::mem::take(&mut events))).is_err() {
                if !shared.stopping() {
                    engine_lost(shared, &mut backend);
                }
                return;
            }
        }

        if last_power.elapsed() >= POWER_INTERVAL {
            last_power = Instant::now();
            let mut status = shared.status();
            for pad in &mut status.pads {
                if let Some(info) = backend.pad_info(pad.id)
                    && info.power != pad.power
                {
                    pad.power = info.power;
                    pending_notify = true;
                }
            }
        }

        while let Ok(found) = mappers.try_recv() {
            pending_notify |= update_other_mappers(shared, found);
        }

        if let Some(locked) = lock.check() {
            pending_notify = true;
            let _ = to_engine.send(ToEngine::Locked(locked));
        }

        let id = foreground_window_id();
        let rules_changed = shared.rules_changed.swap(false, Ordering::SeqCst);
        if rules_changed {
            let rules = shared.auto_profiles.lock().unwrap_or_else(|e| e.into_inner());
            title_rules = rules.iter().any(|rule| rule.title_contains.is_some());
        }
        let title_due =
            title_rules && foreground_title.is_some() && last_title.elapsed() >= TITLE_INTERVAL;
        if id != foreground || rules_changed || title_due {
            last_title = Instant::now();
            let moved = id != foreground;
            foreground = id;
            if let Some(window) = foreground_window() {
                let retitled = foreground_title.as_deref() != Some(window.title.as_str());
                if moved || rules_changed || retitled {
                    foreground_title = (window.pid != own_pid).then(|| window.title.clone());
                    on_foreground(shared, to_engine, &window, own_pid, we_are_elevated);
                    pending_notify = true;
                }
            }
        }

        if pending_notify && last_notify.elapsed() >= NOTIFY_INTERVAL {
            pending_notify = false;
            last_notify = Instant::now();
            shown.clear();
            shown.extend(shared.status().pads.iter().map(|pad| (pad.id, pad.axes)));
            (shared.notify)();
        }
    }
}

/// Whether a stick or trigger moving from `shown` (what the UI last showed) to `now` is worth a
/// repaint: it moved visibly, or came back to rest.
fn axis_moved(shown: f32, now: f32) -> bool {
    (now - shown).abs() >= AXIS_STEP || (now == 0.0 && shown != 0.0)
}

/// The engine thread is gone: says so, and stops any rumble it had started.
fn engine_lost(shared: &Shared, backend: &mut SdlBackend) {
    error!("Controller input is no longer mapped: the mapping engine stopped.");
    let pads: Vec<PadId> = {
        let mut status = shared.status();
        status.error.get_or_insert_with(|| ENGINE_STOPPED.to_owned());
        status.pads.iter().map(|pad| pad.id).collect()
    };
    for pad in pads {
        backend.rumble(pad, 0, 0, Duration::ZERO);
    }
    (shared.notify)();
}

/// Notices when input can no longer reach the user's desktop: Windows is locked, shows a secure
/// screen, or is going to sleep.
#[derive(Debug, Default)]
struct LockWatch {
    locked: bool,
    desktop_locked: bool,
    suspending: bool,
    last_check: Option<Instant>,
}

impl LockWatch {
    /// Returns the new state when it changes.
    fn check(&mut self) -> Option<bool> {
        // Checked every time: the system waits only briefly for ZPadder before it sleeps.
        let suspending = system_suspending();
        if suspending != self.suspending {
            self.suspending = suspending;
            // On resume the lock screen is usually up: look again now, rather than resume
            // mapping on what the desktop was before the sleep.
            self.last_check = None;
        }
        if self.last_check.is_none_or(|at| at.elapsed() >= LOCK_INTERVAL) {
            self.last_check = Some(Instant::now());
            self.desktop_locked = desktop_locked();
        }
        let locked = self.desktop_locked || suspending;
        if locked == self.locked {
            return None;
        }
        self.locked = locked;
        if suspending {
            info!("Windows is going to sleep: mapping lets go of everything.");
        } else if locked {
            info!(
                "Windows is locked or showing a secure screen: mapping lets go of everything \
                 until the desktop is back."
            );
        } else {
            info!("The desktop is back: mapping resumes.");
        }
        Some(locked)
    }
}

fn on_foreground(
    shared: &Shared,
    to_engine: &Sender<ToEngine>,
    window: &ForegroundWindow,
    own_pid: u32,
    we_are_elevated: bool,
) {
    let own = window.pid == own_pid;
    let _ = to_engine.send(ToEngine::Paused(own));
    let program = window
        .exe
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let blocked = !we_are_elevated && window.elevation != Elevation::NotElevated;
    let was_blocked = shared.status().blocked_window.is_some();
    if blocked && !was_blocked {
        warn!(
            "\"{}\" runs as administrator, so Windows will block ZPadder's input to it. \
             Run ZPadder as administrator to control it.",
            window.title
        );
    }
    {
        let mut status = shared.status();
        status.foreground = Some((program.clone(), window.title.clone()));
        status.blocked_window = blocked.then(|| window.title.clone());
    }
    // ZPadder's own window keeps whatever profile was in effect.
    if !own {
        let rules = shared.auto_profiles.lock().unwrap_or_else(|e| e.into_inner());
        let matched = rules.iter().find(|rule| rule.matches(window));
        let auto = matched.map(|rule| (rule.program.clone(), Arc::clone(&rule.profile)));
        let _ = to_engine.send(ToEngine::Auto(auto));
    }
}

/// Looks for other remappers every [`MAPPERS_INTERVAL`] and sends what it finds, until `stop` is
/// dropped. It runs at a low priority and shares no lock with the mapper, so listing the
/// processes never delays input.
fn watch_thread(stop: &Receiver<()>, found: &Sender<Vec<String>>) {
    if let Err(e) = set_thread_priority(ThreadPriority::Low) {
        debug!("Cannot lower the watch thread's priority: {e}");
    }
    let own_pid = std::process::id();
    let own_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_name().map(|name| name.to_string_lossy().into_owned()));
    loop {
        match running_programs() {
            Ok(programs) => {
                let others = programs
                    .iter()
                    .filter(|p| !is_zpadder(p, own_pid, own_exe.as_deref()))
                    .map(|p| p.exe.as_str());
                if found.send(other_mappers(others)).is_err() {
                    return;
                }
            }
            Err(e) => debug!("Cannot list the running programs: {e}"),
        }
        if !matches!(stop.recv_timeout(MAPPERS_INTERVAL), Err(RecvTimeoutError::Timeout)) {
            return;
        }
    }
}

/// Whether `program` is this ZPadder, or another launch of it. ZPadder may well have been renamed
/// to Xpadder.exe for a front-end's sake, and then each launch the front-end makes, which hands
/// its command line over and exits, has that name too.
fn is_zpadder(program: &RunningProgram, own_pid: u32, own_exe: Option<&str>) -> bool {
    program.pid == own_pid || own_exe.is_some_and(|exe| program.exe.eq_ignore_ascii_case(exe))
}

/// Records the other remappers found running. Returns `true` if the list changed.
fn update_other_mappers(shared: &Shared, found: Vec<String>) -> bool {
    let mut status = shared.status();
    if status.other_mappers == found {
        return false;
    }
    if !found.is_empty() {
        info!("Also running, and may read the controller too: {}", found.join(", "));
    }
    status.other_mappers = found;
    true
}

/// The friendly names of the known remappers among `programs` (executable names, in any case),
/// each once, in [`OTHER_MAPPERS`] order.
fn other_mappers<'a>(programs: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let programs: Vec<&str> = programs.into_iter().collect();
    let mut found: Vec<String> = Vec::new();
    for (exe, name) in OTHER_MAPPERS {
        if !found.iter().any(|f| f == name) && programs.iter().any(|p| p.eq_ignore_ascii_case(exe))
        {
            found.push(name.to_string());
        }
    }
    found
}

fn engine_thread(
    shared: &Shared,
    rx: &Receiver<ToEngine>,
    rumble: &Sender<Rumble>,
    virtual_pads: Sender<VirtualCmd>,
    start: Instant,
) {
    if let Err(e) = set_thread_priority(ThreadPriority::AboveNormal) {
        debug!("Cannot raise the engine thread's priority: {e}");
    }
    // Dropped with `runner`, however this thread ends, which tells the xbox thread to unplug
    // every virtual controller.
    let mut runner = EngineLoop::new(SendInputSink, virtual_pads);
    // If the engine panics, its state can no longer be trusted; the sink's own record of what it
    // holds can. (And if anything below panics too, dropping the sink still lets go.)
    let run = panic::catch_unwind(AssertUnwindSafe(|| runner.run(shared, rx, rumble, start)));
    match run {
        Ok(()) => {
            runner.release_all(start.elapsed());
            for (pad, on) in runner.engine.take_rumble_changes() {
                let _ = rumble.send(Rumble::Assignment(pad, on));
            }
            // Anything the engine lost track of.
            let _ = runner.sink.release_held();
            info!("Mapping stopped; all keys released.");
        }
        Err(payload) => {
            error!(
                "The mapping engine stopped after an internal error: {}",
                panic_message(&*payload)
            );
            engine_failed(shared, &mut runner.sink);
        }
    }
}

/// After the engine panicked: lets go of everything the sink holds, from its own record, and says
/// what happened. If Windows refuses the releases, keeps trying until it takes them or the mapper
/// is stopped.
fn engine_failed<S: OutputSink>(shared: &Shared, sink: &mut TrackingSink<S>) {
    shared.engine_failed.store(true, Ordering::SeqCst);
    let mut released = sink.release_held();
    if let Err(e) = &released {
        error!("Cannot release the keys ZPadder holds yet ({e}); retrying.");
        shared.fail(ENGINE_FAILED_HOLDING);
        while released.is_err() && !shared.stopping() {
            thread::sleep(Duration::from_millis(100));
            released = sink.retry();
        }
    }
    if released.is_ok() {
        info!("All keys released.");
        shared.fail(ENGINE_FAILED);
    }
}

/// Why no profile is in effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Off {
    Disabled,
    Locked,
    Paused,
    NoProfile,
}

impl Off {
    /// For the log, when a press does nothing.
    fn not_mapped(self) -> &'static str {
        match self {
            Off::Disabled => "not mapped: mapping is switched off",
            Off::Locked => "not mapped: Windows is locked or showing a secure screen",
            Off::Paused => "not mapped: mapping pauses while ZPadder has focus",
            Off::NoProfile => "not mapped: no profile is in use",
        }
    }

    /// For the log, when mapping stops and lets go of everything.
    fn released(self) -> &'static str {
        match self {
            Off::Disabled => "released: mapping switched off",
            Off::Locked => "released: Windows locked or showing a secure screen",
            Off::Paused => "released: mapping paused while ZPadder has focus",
            Off::NoProfile => "released: no profile in use",
        }
    }
}

/// How soon a send that failed is tried again, at first...
const RETRY_SOON: Duration = Duration::from_millis(20);
/// ...and once it has kept failing for a while ([`RETRY_SOON_TIMES`] attempts).
const RETRY_LATER: Duration = Duration::from_millis(250);
const RETRY_SOON_TIMES: u64 = 50;
/// The least time between two log messages about failed sends.
const FAILURE_LOG_INTERVAL: Duration = Duration::from_secs(10);

/// Rate-limits the log of failed sends: the first failure is logged at once, later ones at most
/// every [`FAILURE_LOG_INTERVAL`], with a count of those left out.
#[derive(Debug, Default)]
struct FailureLog {
    /// Failures since the last success.
    run: u64,
    /// A failure of the current run was logged.
    run_logged: bool,
    /// Failures not logged since the last message.
    unlogged: u64,
    last_logged: Option<Duration>,
}

impl FailureLog {
    /// Counts a failure at `now`. Returns how many failures were left out of the log before it,
    /// when this one should be logged.
    fn failed(&mut self, now: Duration) -> Option<u64> {
        self.run += 1;
        if self.last_logged.is_some_and(|at| now < at + FAILURE_LOG_INTERVAL) {
            self.unlogged += 1;
            return None;
        }
        self.last_logged = Some(now);
        self.run_logged = true;
        Some(std::mem::take(&mut self.unlogged))
    }

    /// Counts a success. Returns how many sends had failed in a row, when that was logged.
    fn succeeded(&mut self) -> Option<u64> {
        let run = std::mem::take(&mut self.run);
        let logged = std::mem::take(&mut self.run_logged);
        (logged && run > 0).then_some(run)
    }

    /// How long to wait before trying again.
    fn retry_after(&self) -> Duration {
        if self.run <= RETRY_SOON_TIMES { RETRY_SOON } else { RETRY_LATER }
    }
}

/// The engine thread's state and decisions, kept apart from the thread and its channels so
/// tests can drive them with a virtual clock and a recording sink.
struct EngineLoop<S: OutputSink> {
    engine: Engine,
    sink: TrackingSink<S>,
    /// The current step's outputs, before the mouse gate.
    out: Vec<OutputAction>,
    /// The current step's outputs after the mouse gate: sent, and held back.
    sent: Vec<OutputAction>,
    held_back: Vec<OutputAction>,
    default_profile: Option<Arc<Profile>>,
    auto: Option<(String, Arc<Profile>)>,
    enabled: bool,
    /// A ZPadder window has focus.
    own_focus: bool,
    /// The test bench asks for mapping to stay on while ZPadder has focus.
    testing: bool,
    /// Windows is locked, shows a secure screen or is going to sleep.
    locked: bool,
    gate: MouseGate,
    recorder: Recorder,
    /// When to try again after a send failed.
    retry_at: Option<Duration>,
    failures: FailureLog,
    /// Where virtual controller commands go: the xbox thread.
    virtual_pads: Sender<VirtualCmd>,
    /// Pads connected, and those of them read through XInput.
    connected: BTreeSet<PadId>,
    xinput: BTreeSet<PadId>,
    /// Each pad's virtual controller state as the engine last reported it...
    xbox_states: BTreeMap<PadId, XboxState>,
    /// ...and as the xbox thread was last told (neutral while testing); neutral when missing.
    xbox_sent: BTreeMap<PadId, XboxState>,
    /// The last [`VirtualCmd::Want`] sent.
    xbox_want: Option<(bool, BTreeSet<PadId>)>,
}

impl<S: OutputSink> EngineLoop<S> {
    fn new(sink: S, virtual_pads: Sender<VirtualCmd>) -> Self {
        Self {
            engine: Engine::new(),
            sink: TrackingSink::new(sink),
            out: Vec::new(),
            sent: Vec::new(),
            held_back: Vec::new(),
            default_profile: None,
            auto: None,
            enabled: true,
            own_focus: false,
            testing: false,
            locked: false,
            gate: MouseGate::default(),
            recorder: Recorder::default(),
            retry_at: None,
            failures: FailureLog::default(),
            virtual_pads,
            connected: BTreeSet::new(),
            xinput: BTreeSet::new(),
            xbox_states: BTreeMap::new(),
            xbox_sent: BTreeMap::new(),
            xbox_want: None,
        }
    }

    /// Handles messages and wakeups until asked to stop or the channel closes.
    fn run(
        &mut self,
        shared: &Shared,
        rx: &Receiver<ToEngine>,
        rumble: &Sender<Rumble>,
        start: Instant,
    ) {
        loop {
            let message = match self.next_wakeup() {
                Some(at) => match rx.recv_timeout(at.saturating_sub(start.elapsed())) {
                    Ok(message) => Some(message),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match rx.recv() {
                    Ok(message) => Some(message),
                    Err(_) => return,
                },
            };
            if !self.handle(shared, message, start.elapsed()) {
                return;
            }
            for (pad, on) in self.engine.take_rumble_changes() {
                let _ = rumble.send(Rumble::Assignment(pad, on));
            }
        }
    }

    /// When [`EngineLoop::handle`] next needs calling without a message.
    fn next_wakeup(&self) -> Option<Duration> {
        // Retrying is pointless while Windows is locked; unlocking retries at once.
        let retry = self.retry_at.filter(|_| self.sink.needs_retry() && !self.locked);
        self.engine
            .next_wakeup()
            .into_iter()
            .chain(self.recorder.next_deadline())
            .chain(retry)
            .min()
    }

    /// Applies `message` at `now` (`None` when a wakeup is due), sends what the engine produced
    /// and records it. Returns `false` once asked to stop.
    fn handle(&mut self, shared: &Shared, message: Option<ToEngine>, now: Duration) -> bool {
        let mut events = Vec::new();
        let mut setting = false;
        let mut reselect = false;
        let mut lock_started = false;
        match message {
            None => self.engine.step(&[], now, &mut self.out),
            Some(ToEngine::Input(input)) => {
                log_events(&input);
                for event in &input {
                    match *event {
                        InputEvent::Connected { pad } => {
                            self.connected.insert(pad);
                        }
                        InputEvent::Disconnected { pad } => {
                            self.connected.remove(&pad);
                            self.xinput.remove(&pad);
                        }
                        InputEvent::Button { .. } | InputEvent::Axis { .. } => {}
                    }
                }
                self.engine.step(&input, now, &mut self.out);
                update_layers(shared, &self.engine);
                events = input;
            }
            Some(ToEngine::Profile(profile)) => {
                self.default_profile = profile;
                (setting, reselect) = (true, true);
            }
            Some(ToEngine::Auto(rule)) => {
                let same = match (&self.auto, &rule) {
                    (Some((_, a)), Some((_, b))) => Arc::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                };
                self.auto = rule;
                (setting, reselect) = (true, !same);
            }
            Some(ToEngine::Enabled(on)) => {
                self.enabled = on;
                (setting, reselect) = (true, true);
            }
            Some(ToEngine::Paused(on)) => {
                (setting, reselect) = (true, self.own_focus != on);
                self.own_focus = on;
            }
            Some(ToEngine::Testing(on)) => {
                (setting, reselect) = (true, self.testing != on);
                self.testing = on;
            }
            Some(ToEngine::Locked(on)) => {
                (setting, reselect) = (true, self.locked != on);
                lock_started = on && !self.locked;
                self.locked = on;
                if !on {
                    // Whatever is still owed goes out now that it can.
                    self.retry_at = None;
                }
            }
            Some(ToEngine::XInputPad(pad)) => {
                self.xinput.insert(pad);
                return true;
            }
            Some(ToEngine::Stop) => return false,
        }
        let change = if reselect { self.reselect(shared) } else { "" };
        let cause = if setting {
            Cause::Mapping(change)
        } else if events.is_empty() {
            Cause::Timer
        } else {
            Cause::Input(&events)
        };
        self.gate.active = self.testing && self.own_focus;
        self.send(now);
        if lock_started {
            // Also let go of anything the engine lost track of.
            let released = self.sink.release_held();
            self.sent_result(now, released);
        }
        self.sync_virtual_pads();
        self.record(shared, now, cause);
        true
    }

    /// The pads that get a virtual controller while the profile asks for them: every pad
    /// connected, except those read through XInput, which games see as Xbox controllers already.
    fn xbox_pads(&self) -> BTreeSet<PadId> {
        self.connected.difference(&self.xinput).copied().collect()
    }

    /// Tells the xbox thread which pads want a virtual controller, and what each should report.
    /// They stay plugged in while the wanted profile asks for them and mapping is on, even
    /// while paused (games can drop a controller that disappears), and are neutral meanwhile
    /// and while the test bench is in use.
    fn sync_virtual_pads(&mut self) {
        for (pad, state) in self.engine.take_virtual_pad_changes() {
            self.xbox_states.insert(pad, state);
        }
        let pads = self.xbox_pads();
        self.xbox_states.retain(|pad, _| self.connected.contains(pad));
        self.xbox_sent.retain(|pad, _| pads.contains(pad));
        let active = self.enabled && self.wanted().is_some_and(|p| p.virtual_pad.is_some());
        let want = (active, pads);
        if self.xbox_want.as_ref() != Some(&want) {
            let (active, pads) = want.clone();
            let _ = self.virtual_pads.send(VirtualCmd::Want { active, pads });
            self.xbox_want = Some(want);
        }
        let Some((_, pads)) = &self.xbox_want else { return };
        for &pad in pads {
            let state = if self.gate.active {
                XboxState::NEUTRAL
            } else {
                self.xbox_states.get(&pad).copied().unwrap_or_default()
            };
            if self.xbox_sent.get(&pad).copied().unwrap_or_default() != state {
                self.xbox_sent.insert(pad, state);
                let _ = self.virtual_pads.send(VirtualCmd::State(pad, state));
            }
        }
    }

    /// The profile wanted right now: the AutoProfile rule's, else the default.
    fn wanted(&self) -> Option<&Arc<Profile>> {
        self.auto.as_ref().map(|(_, profile)| profile).or(self.default_profile.as_ref())
    }

    /// Why no profile should be in effect, if none should.
    fn off(&self) -> Option<Off> {
        if !self.enabled {
            Some(Off::Disabled)
        } else if self.locked {
            Some(Off::Locked)
        } else if self.own_focus && !self.testing {
            Some(Off::Paused)
        } else if self.wanted().is_none() {
            Some(Off::NoProfile)
        } else {
            None
        }
    }

    /// Puts the right profile in effect after a setting changed and updates the status. Returns
    /// how mapping changed, for the log.
    fn reselect(&mut self, shared: &Shared) -> &'static str {
        let off = self.off();
        let wanted = self.wanted().cloned();
        let effective = if off.is_none() { wanted.clone() } else { None };
        let changed = match (self.engine.profile(), &effective) {
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            (None, None) => false,
            _ => true,
        };
        if changed {
            self.engine.set_profile(effective, &mut self.out);
        }
        let mut status = shared.status();
        status.enabled = self.enabled;
        status.paused = self.own_focus && !self.testing;
        status.testing = self.own_focus && self.testing;
        status.locked = self.locked;
        status.profile = wanted.map(|p| p.name.clone());
        status.auto_program = self.auto.as_ref().map(|(program, _)| program.clone());
        drop(status);
        update_layers(shared, &self.engine);
        (shared.notify)();
        off.map_or("the profile in effect changed", Off::released)
    }

    /// Sends the step's outputs through the mouse gate, after anything owed from a failed send
    /// once a retry is due.
    fn send(&mut self, now: Duration) {
        self.sent.clear();
        self.held_back.clear();
        self.gate.filter(&self.out, &mut self.sent, &mut self.held_back);
        self.out.clear();
        let retry_due =
            self.sink.needs_retry() && !self.locked && self.retry_at.is_none_or(|at| now >= at);
        if self.sent.is_empty() && !retry_due {
            return;
        }
        if tracing::enabled!(tracing::Level::DEBUG) {
            let actions: Vec<String> = self
                .sent
                .iter()
                .filter(|action| !matches!(action, OutputAction::MouseMove { .. }))
                .map(ToString::to_string)
                .collect();
            if !actions.is_empty() {
                debug!("-> {}", actions.join(", "));
            }
        }
        // The tracking sink catches up on anything owed first.
        let result = self.sink.send(&self.sent);
        self.sent_result(now, result);
    }

    /// Logs a failed send (sparingly) and schedules the retry, or notes that sending works again.
    fn sent_result(&mut self, now: Duration, result: Result<(), SendError>) {
        match result {
            Ok(()) => {
                self.retry_at = None;
                if let Some(failed) = self.failures.succeeded() {
                    info!("Sending input works again, after {failed} failed attempts.");
                }
            }
            Err(e) if self.locked => {
                // Expected: the secure desktop takes no input. Retried once unlocked.
                debug!("Sending input failed while Windows is locked: {e}");
            }
            Err(e) => {
                match self.failures.failed(now) {
                    Some(0) => error!("Sending input failed: {e}. Releases are retried."),
                    Some(left_out) => error!(
                        "Sending input still fails ({left_out} more failures since the last \
                         message): {e}"
                    ),
                    None => {}
                }
                self.retry_at = Some(now + self.failures.retry_after());
            }
        }
    }

    /// Adds the step just sent to the activity log, if it is worth showing.
    fn record(&mut self, shared: &Shared, now: Duration, cause: Cause<'_>) {
        let off = if self.engine.profile().is_none() { self.off() } else { None };
        let engine = &self.engine;
        let layer_of = |pad| engine.active_layer(pad);
        let passes_through =
            |pad: PadId| self.connected.contains(&pad) && !self.xinput.contains(&pad);
        let step = Step {
            now,
            cause,
            profile: engine.profile().map(Arc::as_ref),
            off: off.map(Off::not_mapped),
            testing: self.gate.active,
            passes_through: &passes_through,
            sent: &self.sent,
            held_back: &self.held_back,
            layer_of: &layer_of,
        };
        let entries = self.recorder.record(&step);
        if entries.is_empty() {
            return;
        }
        let mut log = shared.activity();
        for entry in entries {
            log.push(entry);
        }
        drop(log);
        (shared.notify)();
    }

    /// Lets go of everything, when mapping stops.
    fn release_all(&mut self, now: Duration) {
        self.engine.release_all(&mut self.out);
        self.send(now);
        self.sync_virtual_pads();
    }
}

fn update_layers(shared: &Shared, engine: &Engine) {
    let mut status = shared.status();
    for pad in &mut status.pads {
        pad.layer = engine.active_layer(pad.id).unwrap_or(0);
    }
}

/// At debug level, logs button presses (axis motion would flood the log).
fn log_events(events: &[InputEvent]) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    for event in events {
        if let InputEvent::Button { pad, button, pressed } = event {
            debug!("pad {} {} {}", pad.0, button.name(), if *pressed { "down" } else { "up" });
        }
    }
}

pub(crate) fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::LeftX => 0,
        Axis::LeftY => 1,
        Axis::RightX => 2,
        Axis::RightY => 3,
        Axis::LeftTrigger => 4,
        Axis::RightTrigger => 5,
    }
}

#[cfg(test)]
mod tests;
