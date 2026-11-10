use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zpadder_model::profile::{Assignment, Layer, LayerSelect, SelectMode, StickMode};
use zpadder_model::xbox::buttons as xb;
use zpadder_model::{
    Axis, Button, InputEvent, KeyCode, MouseButton, OutputAction, PadId, Profile, VirtualPad,
    XboxState,
};
use zpadder_output::RecordingSink;
use zpadder_platform::{Elevation, ForegroundWindow};

use crate::virtual_pad::fake::FakeDriver;
use crate::virtual_pad::{OwnPads, VirtualCmd, VirtualPads};
use crate::{
    Activity, AutoProfile, ENGINE_FAILED, ENGINE_FAILED_HOLDING, EngineLoop, FailureLog, Origin,
    PadInput, Rumble, RumbleMix, Shared, Side, Status, ToEngine, VirtualPadStatus, axis_moved,
    engine_failed, is_zpadder, other_mappers, wildcard_match,
};
use zpadder_platform::RunningProgram;

use OutputAction::{KeyDown, KeyUp};

const PAD: PadId = PadId(1);

fn assignment(slots: &str) -> Assignment {
    Assignment {
        slots: slots.split(',').map(|slot| slot.trim().parse().unwrap()).collect(),
        ..Assignment::default()
    }
}

fn click(pressed: bool) -> OutputAction {
    OutputAction::MouseButton { button: MouseButton::Left, pressed }
}

/// South types `a` and north clicks the left mouse button; `edit` adds the rest.
fn profile(edit: impl FnOnce(&mut Layer)) -> Arc<Profile> {
    let mut layer = Layer::new("Main");
    layer.buttons.insert(Button::South, assignment("a"));
    layer.buttons.insert(Button::North, assignment("mouse_left"));
    edit(&mut layer);
    let profile =
        Profile { name: "Test".into(), layout: None, virtual_pad: None, layers: vec![layer] };
    profile.validate().unwrap();
    Arc::new(profile)
}

/// Drives the engine loop with a virtual clock and records what reaches "Windows". Virtual
/// controller commands are applied to a fake driver, as the xbox thread would.
struct Rig {
    shared: Shared,
    runner: EngineLoop<RecordingSink>,
    now: Duration,
    seen: u64,
    commands: Receiver<VirtualCmd>,
    /// Every command, in order, since the last call to [`Rig::commands`].
    command_log: Vec<VirtualCmd>,
    driver: FakeDriver,
    virtual_pads: VirtualPads<FakeDriver>,
}

impl Rig {
    fn new(profile: Arc<Profile>) -> Self {
        let (tx, commands) = mpsc::channel();
        let driver = FakeDriver::installed();
        let own = Arc::new(Mutex::new(OwnPads::default()));
        let mut rig = Self {
            shared: Shared::new(Box::new(|| {})),
            runner: EngineLoop::new(RecordingSink::default(), tx),
            now: Duration::ZERO,
            seen: 0,
            commands,
            command_log: Vec::new(),
            virtual_pads: VirtualPads::new(driver.clone(), own),
            driver,
        };
        rig.send(ToEngine::Profile(Some(profile)));
        rig
    }

    /// Applies the virtual controller commands sent so far.
    fn pump(&mut self) {
        while let Ok(command) = self.commands.try_recv() {
            match command.clone() {
                VirtualCmd::Want { active, pads } => self.virtual_pads.want(active, pads, self.now),
                VirtualCmd::State(pad, state) => self.virtual_pads.set_state(pad, state),
            }
            self.command_log.push(command);
        }
    }

    /// The virtual controllers plugged in, with what each reports.
    fn xbox(&mut self) -> BTreeMap<PadId, XboxState> {
        self.pump();
        self.driver.0.borrow().plugged.clone()
    }

    /// The virtual controller commands sent since the last call.
    fn commands(&mut self) -> Vec<VirtualCmd> {
        self.pump();
        std::mem::take(&mut self.command_log)
    }

    fn connect(&mut self, pad: PadId) {
        self.send(ToEngine::Input(vec![InputEvent::Connected { pad }]));
    }

    fn at(&mut self, ms: u64) -> &mut Self {
        self.now = Duration::from_millis(ms);
        self
    }

    fn send(&mut self, message: ToEngine) {
        assert!(self.runner.handle(&self.shared, Some(message), self.now));
    }

    fn button(&mut self, button: Button, pressed: bool) {
        self.send(ToEngine::Input(vec![InputEvent::Button { pad: PAD, button, pressed }]));
    }

    fn axis(&mut self, axis: Axis, value: f32) {
        self.send(ToEngine::Input(vec![InputEvent::Axis { pad: PAD, axis, value }]));
    }

    /// Runs every wakeup due up to `ms`, as the engine thread would.
    fn run_until(&mut self, ms: u64) {
        let end = Duration::from_millis(ms);
        for _ in 0..100_000 {
            match self.runner.next_wakeup() {
                Some(at) if at <= end => {
                    self.now = at.max(self.now);
                    assert!(self.runner.handle(&self.shared, None, self.now));
                }
                _ => {
                    self.now = end;
                    return;
                }
            }
        }
        panic!("the engine loop never went idle");
    }

    /// Everything sent since the last call.
    fn sent(&mut self) -> Vec<OutputAction> {
        std::mem::take(&mut self.runner.sink.inner_mut().actions)
    }

    /// Makes "Windows" refuse all input (`Some(0)`), or take it again (`None`).
    fn accept(&mut self, count: Option<usize>) {
        self.runner.sink.inner_mut().accept = count;
    }

    fn status(&self) -> Status {
        self.shared.status().clone()
    }

    /// Log entries added since the last call.
    fn log(&mut self) -> Vec<Activity> {
        let entries = self.shared.activity().since(self.seen);
        if let Some(last) = entries.last() {
            self.seen = last.seq;
        }
        entries
    }
}

#[test]
fn testing_keeps_keys_on_inside_zpadder_but_holds_the_mouse_back() {
    let mut rig = Rig::new(profile(|_| {}));
    rig.send(ToEngine::Paused(true));
    rig.button(Button::South, true);
    rig.button(Button::South, false);
    assert!(rig.sent().is_empty());
    let log = rig.log();
    assert!(log.iter().all(|e| e.why.contains("pauses while ZPadder has focus")), "{log:#?}");
    assert!(rig.status().paused && !rig.status().testing);

    rig.send(ToEngine::Testing(true));
    assert!(!rig.status().paused && rig.status().testing);
    rig.button(Button::South, true);
    assert_eq!(rig.sent(), [KeyDown(KeyCode::A)]);
    rig.button(Button::North, true);
    assert!(rig.sent().is_empty());
    let log = rig.log();
    let held_back = log.last().unwrap();
    assert_eq!(held_back.held_back, [click(true)]);
    assert!(held_back.why.contains("the mouse stays still"), "{held_back:#?}");

    // Leaving the box releases everything. The click never went out, so its release does not.
    rig.send(ToEngine::Testing(false));
    assert_eq!(rig.sent(), [KeyUp(KeyCode::A)]);
    let log = rig.log();
    let released = log.last().unwrap();
    assert_eq!(released.origin, Origin::Mapping);
    assert_eq!(released.held_back, [click(false)]);
    assert!(released.why.starts_with("released: mapping paused"), "{released:#?}");
    assert!(rig.status().paused && !rig.status().testing);
}

#[test]
fn a_mouse_button_down_when_testing_starts_is_still_released() {
    let mut rig = Rig::new(profile(|_| {}));
    rig.button(Button::North, true);
    assert_eq!(rig.sent(), [click(true)]);
    // The click that focuses the box can reach the mapper before ZPadder's focus does.
    rig.send(ToEngine::Testing(true));
    rig.send(ToEngine::Paused(true));
    assert!(rig.sent().is_empty(), "mapping stays on, so nothing is released");
    rig.button(Button::North, false);
    assert_eq!(rig.sent(), [click(false)]);
}

#[test]
fn testing_outside_zpadder_changes_nothing() {
    let mut rig = Rig::new(profile(|_| {}));
    rig.send(ToEngine::Testing(true));
    assert!(!rig.status().testing);
    rig.button(Button::North, true);
    rig.button(Button::North, false);
    assert_eq!(rig.sent(), [click(true), click(false)]);
}

#[test]
fn mapping_switched_off_stays_off_while_testing() {
    let mut rig = Rig::new(profile(|_| {}));
    rig.send(ToEngine::Enabled(false));
    rig.send(ToEngine::Paused(true));
    rig.send(ToEngine::Testing(true));
    rig.button(Button::South, true);
    assert!(rig.sent().is_empty());
    let log = rig.log();
    assert!(log.last().unwrap().why.contains("switched off"), "{log:#?}");
}

#[test]
fn releases_say_how_long_the_button_was_held() {
    let mut rig = Rig::new(profile(|_| {}));
    rig.at(1000).button(Button::South, true);
    rig.at(1142).button(Button::South, false);
    rig.at(1200).button(Button::West, true);
    let log = rig.log();
    let [down, up, unassigned] = log.as_slice() else { panic!("{log:#?}") };
    assert_eq!(
        (down.origin, down.sent.as_slice()),
        (Origin::Pad, [KeyDown(KeyCode::A)].as_slice())
    );
    assert_eq!(down.at, Duration::from_millis(1000));
    let held = Some(Duration::from_millis(142));
    assert_eq!(up.input, [PadInput::Button { button: Button::South, pressed: false, held }]);
    assert_eq!((up.sent_text().as_str(), up.why.as_str()), ("a up", "held 142 ms"));
    assert_eq!(unassigned.why, "nothing assigned");
}

#[test]
fn turbo_steps_are_logged_as_timer_steps() {
    let mut rig = Rig::new(profile(|layer| {
        let turbo = Assignment { turbo: Some(10.0), ..assignment("x") };
        layer.buttons.insert(Button::East, turbo);
    }));
    rig.button(Button::East, true);
    rig.run_until(60);
    assert_eq!(rig.sent(), [KeyDown(KeyCode::X), KeyUp(KeyCode::X)]);
    let log = rig.log();
    let tick = log.last().unwrap();
    assert_eq!((tick.origin, tick.why.as_str()), (Origin::Timer, "turbo"));
    assert!(tick.input.is_empty());
}

#[test]
fn a_trigger_is_logged_when_it_fires() {
    let mut rig = Rig::new(profile(|layer| layer.right_trigger = Some(assignment("space"))));
    rig.axis(Axis::RightTrigger, 0.1);
    assert!(rig.log().is_empty(), "motion that sends nothing is not logged");
    rig.axis(Axis::RightTrigger, 0.6);
    assert_eq!(rig.sent(), [KeyDown(KeyCode::SPACE)]);
    let log = rig.log();
    let [fired] = log.as_slice() else { panic!("{log:#?}") };
    assert_eq!(fired.input, [PadInput::Trigger { side: Side::Right, value: 0.6 }]);
    assert_eq!(fired.why, "trigger past press point");
}

#[test]
fn layer_switches_are_explained() {
    let mut main = Layer::new("Main");
    let select = LayerSelect { target: "Aim".into(), mode: SelectMode::WhileHeld };
    main.buttons
        .insert(Button::LeftShoulder, Assignment { layer: Some(select), ..Assignment::default() });
    let profile = Profile {
        name: "Layers".into(),
        layout: None,
        virtual_pad: None,
        layers: vec![main, Layer::new("Aim")],
    };
    profile.validate().unwrap();
    let mut rig = Rig::new(Arc::new(profile));
    rig.button(Button::LeftShoulder, true);
    let log = rig.log();
    assert_eq!(log.last().unwrap().why, "now on layer Aim");
}

#[test]
fn pointer_movement_is_added_up_into_one_entry() {
    let mut rig = Rig::new(profile(|layer| layer.left_stick.mode = StickMode::Mouse));
    rig.send(ToEngine::Paused(true));
    rig.send(ToEngine::Testing(true));
    rig.axis(Axis::LeftX, 1.0);
    rig.run_until(100);
    rig.at(100).axis(Axis::LeftX, 0.0);
    assert!(rig.log().is_empty(), "the burst is still open");
    rig.run_until(1000);
    assert!(rig.sent().is_empty(), "the pointer stays still while testing");
    let log = rig.log();
    let [burst] = log.as_slice() else { panic!("{log:#?}") };
    let [OutputAction::MouseMove { dx, dy: 0 }] = burst.held_back.as_slice() else {
        panic!("{burst:#?}")
    };
    assert!((99..=101).contains(dx), "{dx}");
    assert_eq!(burst.input, [PadInput::Stick { side: Side::Left, x: 1.0, y: 0.0 }]);
    assert!(burst.why.contains("the mouse stays still"), "{burst:#?}");
    assert_eq!(rig.runner.next_wakeup(), None, "nothing left to wait for");
}

#[test]
fn known_remappers_are_named_once() {
    let running =
        ["explorer.exe", "STEAM.EXE", "reWASDTray.exe", "reWASDEngine.exe", "zpadder.exe"];
    assert_eq!(other_mappers(running), ["Steam", "reWASD"]);
    assert!(other_mappers(["zpadder.exe", "notepad.exe"]).is_empty());
}

#[test]
fn locking_lets_go_of_everything_until_the_desktop_is_back() {
    let mut rig = Rig::new(profile(|_| {}));
    rig.button(Button::South, true);
    rig.button(Button::North, true);
    rig.sent();
    rig.send(ToEngine::Locked(true));
    assert_eq!(rig.sent(), [KeyUp(KeyCode::A), click(false)]);
    assert!(rig.status().locked);
    let log = rig.log();
    assert!(log.last().unwrap().why.starts_with("released: Windows locked"), "{log:#?}");

    rig.button(Button::South, false);
    rig.button(Button::South, true);
    assert!(rig.sent().is_empty());
    let log = rig.log();
    assert!(log.last().unwrap().why.contains("Windows is locked"), "{log:#?}");

    rig.send(ToEngine::Locked(false));
    assert!(!rig.status().locked);
    rig.button(Button::South, false);
    rig.button(Button::South, true);
    assert_eq!(rig.sent(), [KeyDown(KeyCode::A)]);
}

#[test]
fn a_release_windows_refused_goes_out_on_a_later_tick() {
    let mut rig = Rig::new(profile(|_| {}));
    rig.at(0).button(Button::South, true);
    assert_eq!(rig.sent(), [KeyDown(KeyCode::A)]);
    rig.accept(Some(0));
    rig.at(10).button(Button::South, false);
    assert!(rig.sent().is_empty());
    assert_eq!(rig.runner.next_wakeup(), Some(Duration::from_millis(30)));
    rig.run_until(100);
    assert!(rig.sent().is_empty(), "still refused");
    rig.accept(None);
    rig.run_until(200);
    assert_eq!(rig.sent(), [KeyUp(KeyCode::A)]);
    assert_eq!(rig.runner.next_wakeup(), None, "nothing owed any more");
}

#[test]
fn releases_owed_while_locked_go_out_once_unlocked() {
    let mut rig = Rig::new(profile(|_| {}));
    rig.button(Button::South, true);
    rig.sent();
    // The secure desktop takes no input.
    rig.accept(Some(0));
    rig.send(ToEngine::Locked(true));
    assert!(rig.sent().is_empty());
    assert_eq!(rig.runner.next_wakeup(), None, "no retries while locked");
    rig.accept(None);
    rig.send(ToEngine::Locked(false));
    assert_eq!(rig.sent(), [KeyUp(KeyCode::A)]);
}

#[test]
fn a_panicking_engine_still_lets_go_of_everything() {
    let mut rig = Rig::new(profile(|_| {}));
    rig.button(Button::South, true);
    rig.button(Button::North, true);
    rig.sent();
    let crashed = panic::catch_unwind(AssertUnwindSafe(|| {
        rig.button(Button::East, true);
        panic!("a bug in the engine");
    }));
    assert!(crashed.is_err());
    // The engine's state is not used: only the sink's own record.
    engine_failed(&rig.shared, &mut rig.runner.sink);
    assert_eq!(rig.sent(), [click(false), KeyUp(KeyCode::A)]);
    assert!(rig.shared.engine_failed.load(Ordering::SeqCst));
    assert_eq!(rig.status().error.as_deref(), Some(ENGINE_FAILED));
}

#[test]
fn a_panicking_engine_says_so_when_windows_refuses_the_releases() {
    let mut rig = Rig::new(profile(|_| {}));
    rig.button(Button::South, true);
    rig.accept(Some(0));
    // Stopping, so it gives up after one try instead of waiting for Windows.
    rig.shared.stop.store(true, Ordering::SeqCst);
    engine_failed(&rig.shared, &mut rig.runner.sink);
    assert_eq!(rig.status().error.as_deref(), Some(ENGINE_FAILED_HOLDING));
    assert!(rig.runner.sink.needs_retry());
}

#[test]
fn send_failures_are_logged_sparingly() {
    let s = Duration::from_secs;
    let mut log = FailureLog::default();
    assert_eq!(log.failed(s(0)), Some(0));
    assert_eq!(log.failed(s(1)), None);
    assert_eq!(log.failed(s(5)), None);
    assert_eq!(log.failed(s(10)), Some(2), "two left out since the last message");
    assert_eq!(log.succeeded(), Some(4));
    assert_eq!(log.succeeded(), None);
    // Too soon after the last message: neither this failure nor its recovery is logged.
    assert_eq!(log.failed(s(11)), None);
    assert_eq!(log.succeeded(), None);
}

#[test]
fn only_visible_stick_movement_repaints() {
    assert!(!axis_moved(0.0, 0.004), "jitter at rest");
    assert!(!axis_moved(0.004, -0.004), "jitter across the centre");
    assert!(!axis_moved(0.0, 0.0));
    assert!(axis_moved(0.0, 0.01));
    assert!(axis_moved(0.5, 0.48));
    assert!(axis_moved(0.004, 0.0), "back to rest");
}

#[test]
fn zpadder_is_never_another_remapper() {
    let program = |pid, exe: &str| RunningProgram { pid, exe: exe.into() };
    // Renamed for a front-end: neither this process nor a launch handing over counts.
    assert!(is_zpadder(&program(7, "Xpadder.exe"), 7, Some("Xpadder.exe")));
    assert!(is_zpadder(&program(8, "XPADDER.EXE"), 7, Some("Xpadder.exe")));
    // Under its own name, a real Xpadder still counts.
    assert!(!is_zpadder(&program(8, "Xpadder.exe"), 7, Some("zpadder.exe")));
}

// ---- AutoProfile wildcards ----

#[test]
fn wildcards_match_program_names() {
    assert!(wildcard_match("FiveM_*GTAProcess.exe", "FiveM_b3889_GTAProcess.exe"));
    assert!(wildcard_match("fivem_*gtaprocess.EXE", "FiveM_b3095_GTAProcess.exe"));
    assert!(wildcard_match("FiveM_*GTAProcess.exe", "FiveM_GTAProcess.exe"), "* can be empty");
    assert!(!wildcard_match("FiveM_*GTAProcess.exe", "FiveM_b3889_ChromeBrowser.exe"));
    assert!(wildcard_match("game?.exe", "game2.exe"));
    assert!(!wildcard_match("game?.exe", "game.exe"));
    assert!(!wildcard_match("game?.exe", "game10.exe"));
    assert!(wildcard_match("*", "anything.exe"));
    assert!(wildcard_match("*.exe", "a.exe"));
    assert!(wildcard_match("a*b*c.exe", "aXXbYYbc.exe"), "backtracks");
    assert!(!wildcard_match("a*b*c.exe", "aXXcYYb.exe"));
    assert!(wildcard_match("ÉLITE.exe", "élite.exe"));
    // Without wildcards, names match exactly, ignoring case, as before.
    assert!(wildcard_match("Game.exe", "GAME.EXE"));
    assert!(!wildcard_match("Game.exe", "Game.exe.bak"));
    assert!(!wildcard_match("Game.exe", "MyGame.exe"));
}

#[test]
fn auto_profile_rules_use_wildcards() {
    let window = |exe: &str, title: &str| ForegroundWindow {
        id: 1,
        title: title.into(),
        pid: 2,
        exe: Some(PathBuf::from(format!(r"C:\Games\FiveM\{exe}"))),
        elevation: Elevation::NotElevated,
    };
    let rule = AutoProfile {
        program: "FiveM_*GTAProcess.exe".into(),
        title_contains: None,
        profile: profile(|_| {}),
    };
    assert!(rule.matches(&window("FiveM_b3889_GTAProcess.exe", "FiveM")));
    assert!(!rule.matches(&window("FiveM.exe", "FiveM")));
    let titled = AutoProfile { title_contains: Some("grand theft".into()), ..rule };
    assert!(titled.matches(&window("FiveM_b3889_GTAProcess.exe", "Grand Theft Auto V")));
    assert!(!titled.matches(&window("FiveM_b3889_GTAProcess.exe", "Launcher")));
}

// ---- Virtual Xbox controllers ----

/// The basic test profile, as an Xbox controller: south types `a`, north clicks, and
/// everything else passes through.
fn xbox_profile() -> Arc<Profile> {
    let mut profile = Profile::clone(&profile(|_| {}));
    profile.virtual_pad = Some(VirtualPad::Xbox360);
    Arc::new(profile)
}

fn pressed(buttons: u16) -> XboxState {
    XboxState { buttons, ..XboxState::NEUTRAL }
}

#[test]
fn each_pad_gets_a_controller_while_the_profile_asks_for_one() {
    let mut rig = Rig::new(xbox_profile());
    assert!(rig.xbox().is_empty(), "no pad yet");
    rig.connect(PAD);
    assert_eq!(rig.xbox(), BTreeMap::from([(PAD, XboxState::NEUTRAL)]));
    rig.button(Button::East, true);
    assert_eq!(rig.xbox()[&PAD], pressed(xb::B));
    assert!(rig.sent().is_empty(), "nothing goes through SendInput");
    rig.button(Button::South, true);
    assert_eq!(rig.sent(), [KeyDown(KeyCode::A)]);
    assert_eq!(rig.xbox()[&PAD], pressed(xb::B), "an assigned control stays released");
    let log = rig.log();
    assert!(log.iter().any(|e| e.why == "passed to the Xbox controller as B"), "{log:#?}");
    assert_eq!(rig.virtual_pads.status(), VirtualPadStatus::Active { count: 1 });

    // A profile without one: unplugged.
    rig.send(ToEngine::Profile(Some(profile(|_| {}))));
    assert!(rig.xbox().is_empty());
    assert_eq!(rig.virtual_pads.status(), VirtualPadStatus::Off);
}

#[test]
fn pausing_keeps_the_controller_plugged_in_but_neutral() {
    let mut rig = Rig::new(xbox_profile());
    rig.connect(PAD);
    rig.button(Button::East, true);
    rig.axis(Axis::LeftX, 1.0);
    assert_ne!(rig.xbox()[&PAD], XboxState::NEUTRAL);
    let plugs = rig.driver.0.borrow().plugs;

    // ZPadder has focus, then Windows locks: plugged in, neutral, never replugged.
    rig.send(ToEngine::Paused(true));
    assert_eq!(rig.xbox(), BTreeMap::from([(PAD, XboxState::NEUTRAL)]));
    rig.send(ToEngine::Locked(true));
    rig.send(ToEngine::Paused(false));
    rig.axis(Axis::LeftX, 0.5);
    assert_eq!(rig.xbox(), BTreeMap::from([(PAD, XboxState::NEUTRAL)]));
    rig.send(ToEngine::Locked(false));
    let stick = XboxState { left_x: zpadder_model::xbox::stick_axis(0.5), ..XboxState::NEUTRAL };
    assert_eq!(rig.xbox()[&PAD], stick, "the stick still pushed comes back");
    assert_eq!(rig.driver.0.borrow().plugs, plugs);
}

#[test]
fn the_test_bench_keeps_the_controller_neutral() {
    let mut rig = Rig::new(xbox_profile());
    rig.connect(PAD);
    rig.send(ToEngine::Paused(true));
    rig.send(ToEngine::Testing(true));
    rig.button(Button::South, true);
    assert_eq!(rig.sent(), [KeyDown(KeyCode::A)], "keys go out while testing");
    rig.button(Button::East, true);
    assert_eq!(rig.xbox()[&PAD], XboxState::NEUTRAL);
    let log = rig.log();
    assert!(log.last().unwrap().why.contains("Xbox controller stays neutral"), "{log:#?}");
    // Leaving ZPadder: the press held through it shows up.
    rig.send(ToEngine::Paused(false));
    assert_eq!(rig.xbox()[&PAD], pressed(xb::B));
}

#[test]
fn switching_mapping_off_unplugs_and_on_plugs_in_again() {
    let mut rig = Rig::new(xbox_profile());
    rig.connect(PAD);
    assert_eq!(rig.xbox().len(), 1);
    rig.send(ToEngine::Enabled(false));
    assert!(rig.xbox().is_empty());
    rig.send(ToEngine::Enabled(true));
    assert_eq!(rig.xbox().len(), 1);
}

#[test]
fn a_disconnected_pad_loses_its_controller() {
    let mut rig = Rig::new(xbox_profile());
    rig.connect(PAD);
    rig.connect(PadId(2));
    assert_eq!(rig.xbox().len(), 2);
    rig.send(ToEngine::Input(vec![InputEvent::Disconnected { pad: PadId(2) }]));
    assert_eq!(rig.xbox().keys().copied().collect::<Vec<_>>(), [PAD]);
}

#[test]
fn xbox_pads_get_no_controller_of_their_own() {
    let mut rig = Rig::new(xbox_profile());
    rig.send(ToEngine::XInputPad(PadId(2)));
    rig.connect(PadId(2));
    rig.connect(PAD);
    assert_eq!(rig.xbox().keys().copied().collect::<Vec<_>>(), [PAD]);
    // Its assignments still work.
    rig.send(ToEngine::Input(vec![InputEvent::Button {
        pad: PadId(2),
        button: Button::South,
        pressed: true,
    }]));
    assert_eq!(rig.sent(), [KeyDown(KeyCode::A)]);
}

#[test]
fn an_auto_profile_can_ask_for_the_controller_and_a_pause_keeps_it() {
    let mut rig = Rig::new(profile(|_| {}));
    rig.connect(PAD);
    assert!(rig.xbox().is_empty());
    rig.send(ToEngine::Auto(Some(("FiveM_*GTAProcess.exe".into(), xbox_profile()))));
    assert_eq!(rig.xbox().len(), 1);
    // Focus moves to ZPadder: the rule's profile is still wanted.
    rig.send(ToEngine::Paused(true));
    assert_eq!(rig.xbox().len(), 1);
    rig.send(ToEngine::Paused(false));
    rig.send(ToEngine::Auto(None));
    assert!(rig.xbox().is_empty());
}

#[test]
fn only_changes_are_sent_to_the_xbox_thread() {
    let mut rig = Rig::new(xbox_profile());
    rig.commands();
    rig.connect(PAD);
    let commands = rig.commands();
    assert_eq!(commands.len(), 1, "{commands:?}");
    assert!(matches!(&commands[0], VirtualCmd::Want { active: true, pads } if pads.len() == 1));
    rig.button(Button::South, true);
    rig.button(Button::South, false);
    assert!(rig.commands().is_empty(), "assigned presses change nothing");
    rig.button(Button::East, true);
    assert_eq!(rig.commands(), [VirtualCmd::State(PAD, pressed(xb::B))]);
    // Edits to the profile keep the controller: no Want.
    rig.send(ToEngine::Profile(Some(xbox_profile())));
    assert_eq!(rig.commands(), [VirtualCmd::State(PAD, XboxState::NEUTRAL)]);
}

#[test]
fn stopping_lets_go_of_the_controller() {
    let mut rig = Rig::new(xbox_profile());
    rig.connect(PAD);
    rig.button(Button::East, true);
    rig.xbox();
    rig.runner.release_all(rig.now);
    assert_eq!(rig.xbox()[&PAD], XboxState::NEUTRAL);
    // The engine thread ends: its sender goes, and the xbox thread drops every controller.
    drop(rig.runner);
    assert!(rig.commands.try_recv().is_err());
    drop(rig.virtual_pads);
    assert!(rig.driver.0.borrow().plugged.is_empty());
}

#[test]
fn the_xbox_thread_unplugs_everything_when_the_engine_thread_goes() {
    let shared = Shared::new(Box::new(|| {}));
    let driver = FakeDriver::installed();
    let (tx, rx) = mpsc::channel();
    let pads = [PAD].into_iter().collect();
    tx.send(VirtualCmd::Want { active: true, pads }).unwrap();
    tx.send(VirtualCmd::State(PAD, pressed(xb::A))).unwrap();
    drop(tx);
    // Runs on this thread: the fake driver is not Send.
    crate::virtual_pad::run_xbox_thread(&shared, &rx, driver.clone(), Instant::now());
    assert_eq!(driver.0.borrow().plugs, 1);
    assert!(driver.0.borrow().plugged.is_empty());
    assert_eq!(shared.status().virtual_pads, VirtualPadStatus::Off);
    let log = shared.activity().since(0);
    let whys: Vec<&str> = log.iter().map(|entry| entry.why.as_str()).collect();
    assert_eq!(whys, ["Xbox controller plugged in", "Xbox controller unplugged"]);
}

#[test]
fn a_missing_driver_is_shown_without_stopping_mapping() {
    let shared = Shared::new(Box::new(|| {}));
    let driver = FakeDriver::default();
    let (tx, rx) = mpsc::channel();
    tx.send(VirtualCmd::Want { active: true, pads: [PAD].into_iter().collect() }).unwrap();
    drop(tx);
    crate::virtual_pad::run_xbox_thread(&shared, &rx, driver.clone(), Instant::now());
    assert_eq!(driver.0.borrow().connects, 1, "tried once, no storm");
    let log = shared.activity().since(0);
    let [entry] = log.as_slice() else { panic!("{log:#?}") };
    assert!(entry.why.contains("ViGEmBus is not installed"), "{entry:#?}");
    assert!(shared.status().error.is_none(), "mapping goes on");
}

// ---- Rumble ----

#[test]
fn rumble_from_games_and_assignments_is_combined() {
    let now = Instant::now();
    let mut mix = RumbleMix::default();
    assert_eq!(mix.due(now), None);
    mix.apply(Rumble::Game { pad: PAD, large: 255, small: 0 });
    assert_eq!(mix.due(now), Some((u16::MAX, 0, Duration::from_secs(60))));
    assert_eq!(mix.due(now), None, "sent once");
    // An assignment that rumbles lifts the quiet motor, not the loud one.
    mix.apply(Rumble::Assignment(PAD, true));
    assert_eq!(mix.due(now), Some((u16::MAX, 0xA000, Duration::from_secs(60))));
    mix.apply(Rumble::Game { pad: PAD, large: 0, small: 0 });
    assert_eq!(mix.due(now), Some((0xA000, 0xA000, Duration::from_secs(60))));
    // A long rumble is asked for again before it runs out.
    assert_eq!(mix.due(now + Duration::from_secs(29)), None);
    assert!(mix.due(now + Duration::from_secs(30)).is_some());
    mix.apply(Rumble::Assignment(PAD, false));
    assert_eq!(mix.due(now + Duration::from_secs(31)), Some((0, 0, Duration::ZERO)));
    assert_eq!(mix.due(now + Duration::from_secs(90)), None, "silence is not refreshed");
}
