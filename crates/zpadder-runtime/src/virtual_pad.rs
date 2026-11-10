//! Virtual Xbox 360 controllers, one per physical pad, for profiles with a `virtual_pad`.
//!
//! The engine works out what each pad's unassigned controls are doing
//! ([`Engine::take_virtual_pad_changes`](zpadder_engine::Engine::take_virtual_pad_changes)).
//! The engine thread decides which pads get a virtual controller and hands that and the states
//! to the **xbox thread** ([`VirtualCmd`]), which owns the driver connection and plugs
//! controllers in and out ([`VirtualPads`]). Plugging in can take a moment, so it never holds up
//! keys. Rumble that games send to a virtual controller goes back to the input thread, which
//! passes it on to the physical pad.
//!
//! ZPadder must never read its own virtual controllers, or it would map them again. They reach
//! SDL only through its XInput driver (RawInput, Windows.Gaming.Input and GameInput are off by
//! default, and SDL's HIDAPI and DirectInput drivers skip XInput devices), which names each
//! device `XInput#<user index>`. So the xbox thread records each controller's XInput user index
//! in [`OwnPads`], and the input thread's [`OwnPadFilter`] keeps matching gamepads from ever
//! being opened. A gamepad that looks like one of ours while one of ours has no known index yet
//! waits ([`Verdict::Defer`]) until it does. As a last line of defence, pads read through XInput
//! never get a virtual controller of their own, so even a misjudged one cannot multiply.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tracing::{debug, error, info, warn};
use zpadder_input::sdl::{DeviceFilter, DeviceIdentity, Verdict, xinput_slot};
use zpadder_model::{PadId, XboxState};

use crate::activity::{Activity, Origin};
use crate::{Shared, panic_message};

/// How long after a failure connecting to the driver, or plugging a controller in, it is tried
/// again, so installing ViGEmBus while ZPadder runs works without a restart.
pub(crate) const RETRY_INTERVAL: Duration = Duration::from_secs(10);
/// How often a controller's XInput user index is asked for until Windows has given it one...
const INDEX_INTERVAL: Duration = Duration::from_millis(250);
/// ...and how many times, about five seconds in all.
const INDEX_QUERIES: u32 = 20;
/// The vendor and product ids every virtual controller has: a wired Xbox 360 controller's.
pub(crate) const XBOX360_IDS: (u16, u16) = (0x045E, 0x028E);

/// What "Act as an Xbox controller" is doing, for display.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum VirtualPadStatus {
    /// The profile in use does not ask for it, or mapping is switched off.
    #[default]
    Off,
    /// The driver is there, and `count` virtual controllers are plugged in: one per controller
    /// connected (Xbox controllers excepted, which games see already).
    Active { count: usize },
    /// The ViGEmBus driver is not installed. ZPadder looks again every few seconds.
    DriverMissing,
    /// Something else went wrong; the text says what, in plain words.
    Failed(String),
}

/// Why the driver could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fault {
    DriverMissing,
    /// Anything else, in plain words.
    Failed(String),
}

/// A virtual controller driver.
pub(crate) trait Backend {
    type Target: Target;
    /// Connects to the driver.
    fn connect(&mut self) -> Result<(), Fault>;
    /// Lets go of the driver. Controllers plugged in stay plugged in.
    fn disconnect(&mut self);
    /// Plugs a new virtual controller in for `pad` and waits until it is ready.
    fn plug(&mut self, pad: PadId) -> Result<Self::Target, Fault>;
}

/// A virtual controller that is plugged in. Dropping it unplugs it.
pub(crate) trait Target {
    /// The XInput user index (0–3) Windows gave the controller, once it has given it one.
    fn user_index(&mut self) -> Option<u8>;
    fn update(&mut self, state: &XboxState) -> Result<(), Fault>;
}

/// The virtual controllers ZPadder has plugged in, as far as telling them apart from other
/// gamepads goes. Shared by the xbox thread, which keeps it up to date, and the input thread.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct OwnPads {
    /// The XInput user indices of controllers plugged in.
    pub(crate) slots: BTreeSet<u8>,
    /// Controllers plugged in (or being plugged in) whose user index is still being asked for.
    pub(crate) unresolved: usize,
    /// Controllers plugged in whose user index Windows never gave.
    pub(crate) unknown: usize,
}

/// What [`classify`] makes of a gamepad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Classified {
    /// Not one of ZPadder's: read it.
    Theirs,
    /// One of ZPadder's, by its XInput user index.
    OursBySlot,
    /// Taken for one of ZPadder's controllers whose user index is unknown.
    OursByCount,
    /// Can't tell until every controller ZPadder plugged in has a user index.
    Unsure,
}

/// Whether a gamepad SDL reports is one of ZPadder's own virtual controllers. Only pads SDL
/// reads through XInput (`XInput#<user index>`) with a wired Xbox 360 controller's ids can be.
/// `taken_by_count` is how many gamepads still connected were already taken for ours by count.
pub(crate) fn classify(
    device: &DeviceIdentity,
    own: &OwnPads,
    taken_by_count: usize,
) -> Classified {
    let Some(slot) = xinput_slot(&device.path) else { return Classified::Theirs };
    if (device.vendor_id, device.product_id) != XBOX360_IDS {
        return Classified::Theirs;
    }
    if own.slots.contains(&slot) {
        Classified::OursBySlot
    } else if own.unresolved > 0 {
        Classified::Unsure
    } else if own.unknown > taken_by_count {
        Classified::OursByCount
    } else {
        Classified::Theirs
    }
}

/// Keeps ZPadder's own virtual controllers away from the mapper: installed on the input
/// thread's SDL backend. Once a gamepad is taken for one of ours, it stays ignored until it
/// goes away, whatever happens to the controller it was taken for.
#[derive(Debug)]
pub(crate) struct OwnPadFilter {
    own: Arc<Mutex<OwnPads>>,
    /// SDL instances ignored as ours, and whether by count.
    ours: HashMap<u32, bool>,
}

impl OwnPadFilter {
    pub(crate) fn new(own: Arc<Mutex<OwnPads>>) -> Self {
        Self { own, ours: HashMap::new() }
    }
}

impl DeviceFilter for OwnPadFilter {
    fn check(&mut self, device: &DeviceIdentity) -> Verdict {
        if self.ours.contains_key(&device.instance) {
            return Verdict::Ignore;
        }
        let own = lock(&self.own).clone();
        let by_count = self.ours.values().filter(|&&by_count| by_count).count();
        match classify(device, &own, by_count) {
            Classified::Theirs => Verdict::Admit,
            Classified::Unsure => Verdict::Defer,
            classified => {
                let by_count = classified == Classified::OursByCount;
                info!(
                    "Not reading {} (ZPadder's own Xbox controller{}).",
                    device.path,
                    if by_count { ", going by how many there are" } else { "" }
                );
                self.ours.insert(device.instance, by_count);
                Verdict::Ignore
            }
        }
    }

    fn gone(&mut self, instance: u32) {
        self.ours.remove(&instance);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the engine thread tells the xbox thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VirtualCmd {
    /// `active`: the profile wanted (before any pause) asks for virtual controllers and mapping
    /// is on. `pads`: the connected pads that would get one.
    Want { active: bool, pads: BTreeSet<PadId> },
    /// What a pad's virtual controller should report.
    State(PadId, XboxState),
}

/// A plugged-in controller and what is known about it.
#[derive(Debug)]
struct Plugged<T> {
    target: T,
    slot: Option<u8>,
    /// User-index queries left while `slot` is unknown, and when the next is due.
    queries_left: u32,
    next_query: Duration,
    /// An update failed and was logged; cleared once one works again.
    failing: bool,
}

/// Plugs virtual controllers in and out as wanted, and keeps them up to date. Lives on the
/// xbox thread; tests drive it with a fake [`Backend`] and a virtual clock.
#[derive(Debug)]
pub(crate) struct VirtualPads<B: Backend> {
    backend: B,
    own: Arc<Mutex<OwnPads>>,
    connected: bool,
    active: bool,
    /// The connected pads that would get a controller.
    pads: BTreeSet<PadId>,
    /// The latest state for each pad, for a controller plugged in later.
    states: BTreeMap<PadId, XboxState>,
    targets: BTreeMap<PadId, Plugged<B::Target>>,
    /// The last failure, until connecting and plugging in work again.
    fault: Option<Fault>,
    /// When to try again after `fault`.
    retry_at: Option<Duration>,
    /// What happened, for the activity log, until taken.
    events: Vec<String>,
}

impl<B: Backend> VirtualPads<B> {
    pub(crate) fn new(backend: B, own: Arc<Mutex<OwnPads>>) -> Self {
        Self {
            backend,
            own,
            connected: false,
            active: false,
            pads: BTreeSet::new(),
            states: BTreeMap::new(),
            targets: BTreeMap::new(),
            fault: None,
            retry_at: None,
            events: Vec::new(),
        }
    }

    pub(crate) fn status(&self) -> VirtualPadStatus {
        match (&self.fault, self.active) {
            (_, false) => VirtualPadStatus::Off,
            (Some(Fault::DriverMissing), true) => VirtualPadStatus::DriverMissing,
            (Some(Fault::Failed(message)), true) => VirtualPadStatus::Failed(message.clone()),
            (None, true) if self.connected => {
                VirtualPadStatus::Active { count: self.targets.len() }
            }
            // Only until the first attempt to connect.
            (None, true) => VirtualPadStatus::Off,
        }
    }

    /// Takes what happened since the last call, for the activity log.
    pub(crate) fn take_events(&mut self) -> Vec<String> {
        std::mem::take(&mut self.events)
    }

    /// Which pads should have a controller; see [`VirtualCmd::Want`].
    pub(crate) fn want(&mut self, active: bool, pads: BTreeSet<PadId>, now: Duration) {
        self.states.retain(|pad, _| pads.contains(pad));
        self.active = active;
        self.pads = pads;
        self.reconcile(now);
    }

    /// What `pad`'s controller should report. Kept for a controller plugged in later.
    pub(crate) fn set_state(&mut self, pad: PadId, state: XboxState) {
        self.states.insert(pad, state);
        if let Some(plugged) = self.targets.get_mut(&pad) {
            send(plugged, pad, &state);
        }
    }

    /// Does what is due at `now`: retrying after a failure, asking for user indices.
    pub(crate) fn tick(&mut self, now: Duration) {
        if self.retry_at.is_some_and(|at| now >= at) {
            self.reconcile(now);
        }
        let mut own = lock(&self.own);
        for (pad, plugged) in &mut self.targets {
            if plugged.slot.is_some() || plugged.queries_left == 0 || now < plugged.next_query {
                continue;
            }
            plugged.slot = plugged.target.user_index();
            plugged.queries_left -= 1;
            plugged.next_query = now + INDEX_INTERVAL;
            if let Some(slot) = plugged.slot {
                info!("The Xbox controller for pad {} is XInput user {}.", pad.0, slot + 1);
                own.unresolved = own.unresolved.saturating_sub(1);
                own.slots.insert(slot);
            } else if plugged.queries_left == 0 {
                warn!(
                    "Windows has not given the Xbox controller for pad {} an XInput user index. \
                     ZPadder keeps the next Xbox 360 controller that appears out of the mapping, \
                     taking it for this one.",
                    pad.0
                );
                own.unresolved = own.unresolved.saturating_sub(1);
                own.unknown += 1;
            }
        }
    }

    /// When [`VirtualPads::tick`] next has something to do.
    pub(crate) fn next_wakeup(&self) -> Option<Duration> {
        let missing =
            !self.connected || self.pads.iter().any(|pad| !self.targets.contains_key(pad));
        let retry = self.retry_at.filter(|_| self.active && missing);
        let query = self
            .targets
            .values()
            .filter(|plugged| plugged.slot.is_none() && plugged.queries_left > 0)
            .map(|plugged| plugged.next_query)
            .min();
        retry.into_iter().chain(query).min()
    }

    /// Plugs in and unplugs controllers to match what is wanted. After a failure, nothing is
    /// tried again before `retry_at`, however often this is called.
    fn reconcile(&mut self, now: Duration) {
        let wanted: BTreeSet<PadId> = if self.active { self.pads.clone() } else { BTreeSet::new() };
        let unwanted: Vec<PadId> =
            self.targets.keys().filter(|pad| !wanted.contains(pad)).copied().collect();
        for pad in unwanted {
            self.unplug(pad);
        }
        if !self.active {
            if self.connected {
                self.backend.disconnect();
                self.connected = false;
            }
            return;
        }
        if self.retry_at.is_some_and(|at| now < at) {
            return;
        }
        self.retry_at = None;
        if !self.connected {
            if let Err(fault) = self.backend.connect() {
                self.fail(fault, now);
                return;
            }
            self.connected = true;
            if self.fault.is_some() {
                info!("Found the ViGEmBus driver.");
            }
        }
        for pad in wanted {
            if self.targets.contains_key(&pad) {
                continue;
            }
            // Counted before it can appear, so the input thread never takes it for a real pad.
            lock(&self.own).unresolved += 1;
            match self.backend.plug(pad) {
                Ok(mut target) => {
                    let slot = target.user_index();
                    if let Some(slot) = slot {
                        let mut own = lock(&self.own);
                        own.unresolved = own.unresolved.saturating_sub(1);
                        own.slots.insert(slot);
                    }
                    let mut plugged = Plugged {
                        target,
                        slot,
                        queries_left: INDEX_QUERIES,
                        next_query: now + INDEX_INTERVAL,
                        failing: false,
                    };
                    // A new controller starts out neutral.
                    if let Some(state) = self.states.get(&pad).filter(|s| !s.is_neutral()) {
                        send(&mut plugged, pad, state);
                    }
                    self.targets.insert(pad, plugged);
                    match slot {
                        Some(slot) => info!(
                            "Xbox controller plugged in for pad {} (XInput user {}).",
                            pad.0,
                            slot + 1
                        ),
                        None => info!("Xbox controller plugged in for pad {}.", pad.0),
                    }
                    self.events.push("Xbox controller plugged in".to_owned());
                }
                Err(fault) => {
                    {
                        let mut own = lock(&self.own);
                        own.unresolved = own.unresolved.saturating_sub(1);
                    }
                    // Connect afresh next time, in case the driver went away.
                    self.backend.disconnect();
                    self.connected = false;
                    self.fail(fault, now);
                    return;
                }
            }
        }
        self.fault = None;
    }

    fn fail(&mut self, fault: Fault, now: Duration) {
        if self.fault.as_ref() != Some(&fault) {
            match &fault {
                Fault::DriverMissing => {
                    info!(
                        "\"Act as an Xbox controller\" needs the ViGEmBus driver, which is not \
                         installed. ZPadder looks for it again every {} seconds.",
                        RETRY_INTERVAL.as_secs()
                    );
                    self.events
                        .push("Xbox controller not available: ViGEmBus is not installed".into());
                }
                Fault::Failed(message) => {
                    warn!("Xbox controller: {message}");
                    self.events.push(format!("Xbox controller not available: {message}"));
                }
            }
        }
        self.fault = Some(fault);
        self.retry_at = Some(now + RETRY_INTERVAL);
    }

    fn unplug(&mut self, pad: PadId) {
        let Some(plugged) = self.targets.remove(&pad) else { return };
        let (slot, resolving) = (plugged.slot, plugged.queries_left > 0);
        // Unplugged first: until the controller is gone it must still count as ours.
        drop(plugged);
        let mut own = lock(&self.own);
        match slot {
            Some(slot) => {
                own.slots.remove(&slot);
            }
            None if resolving => own.unresolved = own.unresolved.saturating_sub(1),
            None => own.unknown = own.unknown.saturating_sub(1),
        }
        drop(own);
        info!("Xbox controller unplugged for pad {}.", pad.0);
        self.events.push("Xbox controller unplugged".to_owned());
    }
}

impl<B: Backend> Drop for VirtualPads<B> {
    /// Never leaves a controller behind, even when the xbox thread panics.
    fn drop(&mut self) {
        let pads: Vec<PadId> = self.targets.keys().copied().collect();
        for pad in pads {
            self.unplug(pad);
        }
        if self.connected {
            self.backend.disconnect();
        }
    }
}

/// Sends `state` to a controller, logging the first failure in a row.
fn send<T: Target>(plugged: &mut Plugged<T>, pad: PadId, state: &XboxState) {
    match plugged.target.update(state) {
        Ok(()) if plugged.failing => {
            plugged.failing = false;
            info!("The Xbox controller for pad {} takes updates again.", pad.0);
        }
        Ok(()) => {}
        Err(Fault::DriverMissing | Fault::Failed(_)) if plugged.failing => {}
        Err(Fault::DriverMissing) => {
            plugged.failing = true;
            warn!("Cannot update the Xbox controller for pad {}: the driver is gone.", pad.0);
        }
        Err(Fault::Failed(message)) => {
            plugged.failing = true;
            warn!("Cannot update the Xbox controller for pad {}: {message}", pad.0);
        }
    }
}

/// Shown, after "Act as an Xbox controller isn't working:", when the xbox thread panicked.
const XBOX_FAILED: &str =
    "the Xbox controller stopped because of an internal error and was unplugged. See the log.";

/// Runs the xbox thread until the engine thread lets go of its sender. Every controller is
/// unplugged when it ends, even after a panic.
pub(crate) fn run_xbox_thread<B: Backend>(
    shared: &Shared,
    rx: &Receiver<VirtualCmd>,
    backend: B,
    start: Instant,
) {
    let run = panic::catch_unwind(AssertUnwindSafe(|| xbox_thread(shared, rx, backend, start)));
    if let Err(payload) = run {
        error!(
            "The Xbox controller thread stopped after an internal error: {}",
            panic_message(&*payload)
        );
        *lock(&shared.own_pads) = OwnPads::default();
        shared.status().virtual_pads = VirtualPadStatus::Failed(XBOX_FAILED.to_owned());
        (shared.notify)();
    }
}

fn xbox_thread<B: Backend>(shared: &Shared, rx: &Receiver<VirtualCmd>, backend: B, start: Instant) {
    let mut pads = VirtualPads::new(backend, Arc::clone(&shared.own_pads));
    let mut shown = VirtualPadStatus::Off;
    // States are coalesced: only each pad's newest one in a batch of messages is sent.
    let mut states: BTreeMap<PadId, XboxState> = BTreeMap::new();
    loop {
        let first = match pads.next_wakeup() {
            Some(at) => match rx.recv_timeout(at.saturating_sub(start.elapsed())) {
                Ok(message) => Some(message),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(message) => Some(message),
                Err(_) => break,
            },
        };
        let mut stopping = false;
        let mut next = first;
        while let Some(message) = next {
            match message {
                VirtualCmd::State(pad, state) => {
                    states.insert(pad, state);
                }
                VirtualCmd::Want { active, pads: wanted } => {
                    for (pad, state) in std::mem::take(&mut states) {
                        pads.set_state(pad, state);
                    }
                    pads.want(active, wanted, start.elapsed());
                }
            }
            next = match rx.try_recv() {
                Ok(message) => Some(message),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    stopping = true;
                    None
                }
            };
        }
        for (pad, state) in std::mem::take(&mut states) {
            pads.set_state(pad, state);
        }
        pads.tick(start.elapsed());
        publish(shared, &mut shown, pads.status(), pads.take_events(), start);
        if stopping {
            break;
        }
    }
    pads.want(false, BTreeSet::new(), start.elapsed());
    let events = pads.take_events();
    drop(pads);
    publish(shared, &mut shown, VirtualPadStatus::Off, events, start);
    debug!("The Xbox controller thread stopped; every Xbox controller is unplugged.");
}

/// Shows a new status, and logs what happened, telling the UI.
fn publish(
    shared: &Shared,
    shown: &mut VirtualPadStatus,
    status: VirtualPadStatus,
    events: Vec<String>,
    start: Instant,
) {
    let changed = status != *shown;
    if changed {
        *shown = status.clone();
        shared.status().virtual_pads = status;
    }
    if !events.is_empty() {
        let at = start.elapsed();
        let mut log = shared.activity();
        for why in events {
            log.push(Activity {
                seq: 0,
                at,
                origin: Origin::Mapping,
                input: Vec::new(),
                sent: Vec::new(),
                held_back: Vec::new(),
                why,
            });
        }
    } else if !changed {
        return;
    }
    (shared.notify)();
}

/// A driver that is never there, where there is no ViGEmBus.
#[cfg(not(windows))]
#[derive(Debug, Default)]
pub(crate) struct NoDriver;

#[cfg(not(windows))]
impl Backend for NoDriver {
    type Target = NoTarget;

    fn connect(&mut self) -> Result<(), Fault> {
        Err(Fault::DriverMissing)
    }

    fn disconnect(&mut self) {}

    fn plug(&mut self, _pad: PadId) -> Result<NoTarget, Fault> {
        Err(Fault::DriverMissing)
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
pub(crate) struct NoTarget;

#[cfg(not(windows))]
impl Target for NoTarget {
    fn user_index(&mut self) -> Option<u8> {
        None
    }

    fn update(&mut self, _state: &XboxState) -> Result<(), Fault> {
        Err(Fault::DriverMissing)
    }
}

/// A driver for tests: plugged-in controllers are kept in a shared record the test can look at.
#[cfg(test)]
pub(crate) mod fake {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use zpadder_model::{PadId, XboxState};

    use super::{Backend, Fault, Target};

    #[derive(Debug, Default)]
    pub(crate) struct Driver {
        pub(crate) installed: bool,
        /// Plugging in fails with this message.
        pub(crate) broken: Option<String>,
        pub(crate) connects: usize,
        pub(crate) plugs: usize,
        /// The controllers plugged in, with what they report.
        pub(crate) plugged: BTreeMap<PadId, XboxState>,
        /// The user index the next controller gets...
        pub(crate) next_slot: u8,
        /// ...after this many queries that find none.
        pub(crate) slot_delay: u32,
    }

    #[derive(Debug, Clone, Default)]
    pub(crate) struct FakeDriver(pub(crate) Rc<RefCell<Driver>>);

    impl FakeDriver {
        pub(crate) fn installed() -> Self {
            let driver = Self::default();
            driver.0.borrow_mut().installed = true;
            driver
        }
    }

    impl Backend for FakeDriver {
        type Target = FakeTarget;

        fn connect(&mut self) -> Result<(), Fault> {
            let mut driver = self.0.borrow_mut();
            driver.connects += 1;
            if driver.installed { Ok(()) } else { Err(Fault::DriverMissing) }
        }

        fn disconnect(&mut self) {}

        fn plug(&mut self, pad: PadId) -> Result<FakeTarget, Fault> {
            let mut driver = self.0.borrow_mut();
            driver.plugs += 1;
            if !driver.installed {
                return Err(Fault::DriverMissing);
            }
            if let Some(message) = &driver.broken {
                return Err(Fault::Failed(message.clone()));
            }
            driver.plugged.insert(pad, XboxState::NEUTRAL);
            let slot = driver.next_slot;
            driver.next_slot += 1;
            Ok(FakeTarget { driver: Rc::clone(&self.0), pad, slot, delay: driver.slot_delay })
        }
    }

    #[derive(Debug)]
    pub(crate) struct FakeTarget {
        driver: Rc<RefCell<Driver>>,
        pad: PadId,
        slot: u8,
        delay: u32,
    }

    impl Target for FakeTarget {
        fn user_index(&mut self) -> Option<u8> {
            if self.delay == 0 {
                return Some(self.slot);
            }
            self.delay -= 1;
            None
        }

        fn update(&mut self, state: &XboxState) -> Result<(), Fault> {
            self.driver.borrow_mut().plugged.insert(self.pad, *state);
            Ok(())
        }
    }

    impl Drop for FakeTarget {
        fn drop(&mut self) {
            self.driver.borrow_mut().plugged.remove(&self.pad);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeDriver;
    use super::*;

    const ONE: PadId = PadId(1);
    const TWO: PadId = PadId(2);

    fn pads(ids: &[PadId]) -> BTreeSet<PadId> {
        ids.iter().copied().collect()
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn manager(driver: &FakeDriver) -> (VirtualPads<FakeDriver>, Arc<Mutex<OwnPads>>) {
        let own = Arc::new(Mutex::new(OwnPads::default()));
        (VirtualPads::new(driver.clone(), Arc::clone(&own)), own)
    }

    fn device(instance: u32, path: &str, ids: (u16, u16)) -> DeviceIdentity {
        DeviceIdentity { instance, path: path.into(), vendor_id: ids.0, product_id: ids.1 }
    }

    #[test]
    fn controllers_follow_the_pads_wanted() {
        let driver = FakeDriver::installed();
        let (mut virtual_pads, own) = manager(&driver);
        assert_eq!(virtual_pads.status(), VirtualPadStatus::Off);
        assert_eq!(driver.0.borrow().connects, 0, "the driver is connected to lazily");

        virtual_pads.want(true, pads(&[ONE, TWO]), secs(0));
        assert_eq!(virtual_pads.status(), VirtualPadStatus::Active { count: 2 });
        assert_eq!(driver.0.borrow().plugged.len(), 2);
        assert_eq!(lock(&own).slots, BTreeSet::from([0, 1]));
        // Wanting the same again changes nothing: no replugging.
        virtual_pads.want(true, pads(&[ONE, TWO]), secs(1));
        assert_eq!(driver.0.borrow().plugs, 2);

        // A pad that disconnects loses its controller.
        virtual_pads.want(true, pads(&[ONE]), secs(2));
        assert_eq!(driver.0.borrow().plugged.keys().copied().collect::<Vec<_>>(), [ONE]);
        assert_eq!(lock(&own).slots, BTreeSet::from([0]));
        assert_eq!(virtual_pads.status(), VirtualPadStatus::Active { count: 1 });

        // No longer wanted: every controller goes.
        virtual_pads.want(false, pads(&[ONE]), secs(3));
        assert!(driver.0.borrow().plugged.is_empty());
        assert_eq!(*lock(&own), OwnPads::default());
        assert_eq!(virtual_pads.status(), VirtualPadStatus::Off);
        let events = virtual_pads.take_events();
        assert_eq!(events.iter().filter(|e| e.contains("plugged in")).count(), 2, "{events:?}");
        assert_eq!(events.iter().filter(|e| e.contains("unplugged")).count(), 2, "{events:?}");
    }

    #[test]
    fn states_reach_the_controllers_including_ones_plugged_in_later() {
        let driver = FakeDriver::installed();
        let (mut virtual_pads, _) = manager(&driver);
        let pressed = XboxState { buttons: 0x1000, ..XboxState::NEUTRAL };
        // The state arrives before the controller is wanted.
        virtual_pads.set_state(ONE, pressed);
        virtual_pads.want(true, pads(&[ONE]), secs(0));
        assert_eq!(driver.0.borrow().plugged[&ONE], pressed);
        virtual_pads.set_state(ONE, XboxState::NEUTRAL);
        assert_eq!(driver.0.borrow().plugged[&ONE], XboxState::NEUTRAL);
    }

    #[test]
    fn a_missing_driver_is_retried_every_ten_seconds_and_found_once_installed() {
        let driver = FakeDriver::default();
        let (mut virtual_pads, _) = manager(&driver);
        virtual_pads.want(true, pads(&[ONE]), secs(0));
        assert_eq!(virtual_pads.status(), VirtualPadStatus::DriverMissing);
        assert!(virtual_pads.take_events()[0].contains("ViGEmBus"));
        // However often it is asked, nothing is tried before the retry is due.
        for s in 1..10 {
            virtual_pads.want(true, pads(&[ONE, TWO]), secs(s));
            virtual_pads.tick(secs(s));
        }
        assert_eq!(driver.0.borrow().connects, 1);
        assert_eq!(virtual_pads.next_wakeup(), Some(RETRY_INTERVAL));
        virtual_pads.tick(secs(10));
        assert_eq!(driver.0.borrow().connects, 2);
        assert_eq!(virtual_pads.status(), VirtualPadStatus::DriverMissing);
        assert!(virtual_pads.take_events().is_empty(), "said once");

        // Installed meanwhile: found on the next retry, without a restart.
        driver.0.borrow_mut().installed = true;
        virtual_pads.tick(secs(19));
        assert_eq!(driver.0.borrow().connects, 2);
        virtual_pads.tick(secs(20));
        assert_eq!(virtual_pads.status(), VirtualPadStatus::Active { count: 2 });
        assert_eq!(virtual_pads.next_wakeup(), None);
    }

    #[test]
    fn nothing_is_retried_while_not_wanted() {
        let driver = FakeDriver::default();
        let (mut virtual_pads, _) = manager(&driver);
        virtual_pads.want(true, pads(&[]), secs(0));
        assert_eq!(virtual_pads.status(), VirtualPadStatus::DriverMissing, "said even with no pad");
        virtual_pads.want(false, pads(&[ONE]), secs(1));
        assert_eq!(virtual_pads.status(), VirtualPadStatus::Off);
        assert_eq!(virtual_pads.next_wakeup(), None);
        virtual_pads.tick(secs(60));
        assert_eq!(driver.0.borrow().connects, 1);
    }

    #[test]
    fn plugging_in_failures_are_shown_and_retried() {
        let driver = FakeDriver::installed();
        driver.0.borrow_mut().broken = Some("ViGEmBus has no room for another controller.".into());
        let (mut virtual_pads, own) = manager(&driver);
        virtual_pads.want(true, pads(&[ONE]), secs(0));
        let VirtualPadStatus::Failed(message) = virtual_pads.status() else { panic!() };
        assert!(message.contains("no room"));
        assert_eq!(*lock(&own), OwnPads::default(), "the reservation is given back");
        driver.0.borrow_mut().broken = None;
        virtual_pads.tick(secs(10));
        assert_eq!(virtual_pads.status(), VirtualPadStatus::Active { count: 1 });
    }

    #[test]
    fn dropping_unplugs_everything() {
        let driver = FakeDriver::installed();
        let (mut virtual_pads, own) = manager(&driver);
        virtual_pads.want(true, pads(&[ONE, TWO]), secs(0));
        drop(virtual_pads);
        assert!(driver.0.borrow().plugged.is_empty());
        assert_eq!(*lock(&own), OwnPads::default());
    }

    #[test]
    fn a_panic_still_unplugs_everything() {
        let driver = FakeDriver::installed();
        let (mut virtual_pads, _) = manager(&driver);
        virtual_pads.want(true, pads(&[ONE]), secs(0));
        let crashed = panic::catch_unwind(AssertUnwindSafe(move || {
            let _owned = virtual_pads;
            panic!("a bug");
        }));
        assert!(crashed.is_err());
        assert!(driver.0.borrow().plugged.is_empty());
    }

    #[test]
    fn user_indices_that_come_late_are_asked_for_again() {
        let driver = FakeDriver::installed();
        driver.0.borrow_mut().slot_delay = 2;
        let (mut virtual_pads, own) = manager(&driver);
        virtual_pads.want(true, pads(&[ONE]), secs(0));
        assert_eq!(*lock(&own), OwnPads { unresolved: 1, ..OwnPads::default() });
        let first = virtual_pads.next_wakeup().unwrap();
        virtual_pads.tick(first);
        assert_eq!(lock(&own).unresolved, 1);
        virtual_pads.tick(virtual_pads.next_wakeup().unwrap());
        assert_eq!(*lock(&own), OwnPads { slots: BTreeSet::from([0]), ..OwnPads::default() });
        assert_eq!(virtual_pads.next_wakeup(), None);
    }

    #[test]
    fn a_user_index_never_given_is_counted_instead() {
        let driver = FakeDriver::installed();
        driver.0.borrow_mut().slot_delay = u32::MAX;
        let (mut virtual_pads, own) = manager(&driver);
        virtual_pads.want(true, pads(&[ONE]), secs(0));
        while let Some(at) = virtual_pads.next_wakeup() {
            virtual_pads.tick(at);
        }
        assert_eq!(*lock(&own), OwnPads { unknown: 1, ..OwnPads::default() });
        virtual_pads.want(true, pads(&[]), secs(60));
        assert_eq!(*lock(&own), OwnPads::default());
    }

    #[test]
    fn only_xbox_360_pads_read_through_xinput_can_be_ours() {
        let own = OwnPads { slots: BTreeSet::from([1]), ..OwnPads::default() };
        let ids = XBOX360_IDS;
        assert_eq!(classify(&device(1, "XInput#1", ids), &own, 0), Classified::OursBySlot);
        assert_eq!(classify(&device(2, "XInput#0", ids), &own, 0), Classified::Theirs);
        // Another kind of XInput pad, or the same ids over HID: a real pad.
        let series = device(3, "XInput#1", (0x045E, 0x0B12));
        assert_eq!(classify(&series, &own, 0), Classified::Theirs);
        let hid = r"\\?\HID#VID_045E&PID_028E#1";
        assert_eq!(classify(&device(4, hid, ids), &own, 0), Classified::Theirs);
        // A DualShock 4 is never ours, whatever its player index.
        let ds4 = r"\\?\HID#VID_054C&PID_09CC&MI_03#7&1";
        assert_eq!(classify(&device(5, ds4, (0x054C, 0x09CC)), &own, 0), Classified::Theirs);
    }

    #[test]
    fn a_lookalike_waits_while_a_user_index_is_unknown() {
        let mut own = OwnPads { unresolved: 1, ..OwnPads::default() };
        let pad = device(1, "XInput#2", XBOX360_IDS);
        assert_eq!(classify(&pad, &own, 0), Classified::Unsure);
        // Resolved to another slot: a real Xbox 360 controller.
        own = OwnPads { slots: BTreeSet::from([0]), ..OwnPads::default() };
        assert_eq!(classify(&pad, &own, 0), Classified::Theirs);
        // Never resolved: taken for ours, one per controller with no index.
        own = OwnPads { unknown: 1, ..OwnPads::default() };
        assert_eq!(classify(&pad, &own, 0), Classified::OursByCount);
        assert_eq!(classify(&pad, &own, 1), Classified::Theirs);
    }

    #[test]
    fn the_filter_keeps_our_controllers_out_until_they_go() {
        let own = Arc::new(Mutex::new(OwnPads::default()));
        let mut filter = OwnPadFilter::new(Arc::clone(&own));
        let ds4 = device(1, r"\\?\HID#VID_054C&PID_09CC#1", (0x054C, 0x09CC));
        let real_360 = device(2, "XInput#0", XBOX360_IDS);
        assert_eq!(filter.check(&ds4), Verdict::Admit);
        assert_eq!(filter.check(&real_360), Verdict::Admit);

        // ZPadder plugs a controller in; it shows up before its index is known.
        lock(&own).unresolved = 1;
        let ours = device(3, "XInput#1", XBOX360_IDS);
        assert_eq!(filter.check(&ours), Verdict::Defer);
        *lock(&own) = OwnPads { slots: BTreeSet::from([1]), ..OwnPads::default() };
        assert_eq!(filter.check(&ours), Verdict::Ignore);

        // Unplugged, but SDL has not noticed yet: still ours.
        *lock(&own) = OwnPads::default();
        assert_eq!(filter.check(&ours), Verdict::Ignore, "a decision sticks");
        filter.gone(3);

        // Taken by count when no index came, and only as many as there are.
        lock(&own).unknown = 1;
        assert_eq!(filter.check(&device(4, "XInput#2", XBOX360_IDS)), Verdict::Ignore);
        assert_eq!(filter.check(&device(5, "XInput#3", XBOX360_IDS)), Verdict::Admit);
        filter.gone(4);
        assert_eq!(filter.check(&device(6, "XInput#2", XBOX360_IDS)), Verdict::Ignore);
    }
}
