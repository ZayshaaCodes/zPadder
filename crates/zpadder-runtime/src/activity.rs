//! The activity log: a short history of what each press did, for the editor's test bench.
//!
//! The engine thread hands every step to a [`Recorder`], which keeps an entry when the step did
//! something worth showing: a button went down or up, a stick or trigger fired an assignment, a
//! timer fired (turbo, a sequence, a hold zone), or mapping let go of everything. Pointer and
//! scroll movement from sticks is added up into one entry per burst rather than one per step, and
//! axis motion that sends nothing else is not logged, so a moving stick cannot flood the log.
//!
//! Recording runs whenever mapping does, so it is kept cheap: a step that logs nothing allocates
//! nothing beyond the axis bookkeeping.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::time::Duration;

use zpadder_model::profile::{Assignment, Layer, Slot, Stick, StickMode};
use zpadder_model::{Axis, Button, InputEvent, OutputAction, PadId, Profile};

use crate::axis_index;

/// How many entries the log keeps; older ones are dropped.
pub const ACTIVITY_CAPACITY: usize = 200;
/// A burst of pointer or scroll movement ends after this long without any...
const BURST_GAP: Duration = Duration::from_millis(300);
/// ...or once it has lasted this long, so a stick held over still shows up every second.
const BURST_SPAN: Duration = Duration::from_secs(1);

/// Why mouse output was held back while the test bench is in use.
const MOUSE_HELD_BACK: &str = "not sent: the mouse stays still while testing in ZPadder";
/// Why a mouse button release was held back after testing ended: its press never went out.
const PRESS_HELD_BACK: &str = "not sent: its press was held back while testing";
/// Why a press that would pass through to the virtual Xbox controller does nothing.
const XBOX_HELD_BACK: &str = "the Xbox controller stays neutral while testing in ZPadder";

/// Which of the two sticks or triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

const SIDES: [Side; 2] = [Side::Left, Side::Right];

/// Something that happened on the pad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadInput {
    /// A button went down or up. On release, `held` is how long it was down.
    Button {
        button: Button,
        pressed: bool,
        held: Option<Duration>,
    },
    /// A trigger moved, ending at `value` (0–1).
    Trigger {
        side: Side,
        value: f32,
    },
    /// A stick moved, ending at `(x, y)` (Y positive downwards).
    Stick {
        side: Side,
        x: f32,
        y: f32,
    },
    Connected,
    Disconnected,
}

/// Short readable form for logs, e.g. `south up after 142 ms` or `left trigger 64%`.
impl fmt::Display for PadInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let side = |side: Side| match side {
            Side::Left => "left",
            Side::Right => "right",
        };
        match *self {
            PadInput::Button { button, pressed: true, .. } => write!(f, "{} down", button.name()),
            PadInput::Button { button, pressed: false, held: Some(held) } => {
                write!(f, "{} up after {} ms", button.name(), held.as_millis())
            }
            PadInput::Button { button, pressed: false, held: None } => {
                write!(f, "{} up", button.name())
            }
            PadInput::Trigger { side: s, value } => {
                write!(f, "{} trigger {:.0}%", side(s), value * 100.0)
            }
            PadInput::Stick { side: s, x, y } => write!(f, "{} stick {x:+.2},{y:+.2}", side(s)),
            PadInput::Connected => f.write_str("controller connected"),
            PadInput::Disconnected => f.write_str("controller disconnected"),
        }
    }
}

/// What started a logged step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Something happened on the pad; [`Activity::input`] says what.
    Pad,
    /// A timer, with no new input: turbo, a sequence or a hold zone.
    Timer,
    /// Mapping was switched off, paused or resumed, or the profile changed.
    Mapping,
}

/// One entry in the activity log.
#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    /// One higher than the previous entry's; the first entry is 1.
    pub seq: u64,
    /// Time since the mapper started.
    pub at: Duration,
    pub origin: Origin,
    /// What happened on the pad, for [`Origin::Pad`]. For a burst of pointer movement, the
    /// sticks doing the moving.
    pub input: Vec<PadInput>,
    /// What was sent to Windows, in order. Pointer and scroll movement from sticks is added up
    /// over a burst into one move and one wheel action.
    pub sent: Vec<OutputAction>,
    /// Mouse output held back because the test bench is in use, added up the same way.
    pub held_back: Vec<OutputAction>,
    /// Why, in a few plain words, e.g. "held 142 ms" or "turbo". May be empty.
    pub why: String,
}

impl Activity {
    /// [`Activity::sent`] as text, e.g. `left_shift down, x down`.
    pub fn sent_text(&self) -> String {
        join(&self.sent)
    }

    /// [`Activity::held_back`] as text.
    pub fn held_back_text(&self) -> String {
        join(&self.held_back)
    }
}

fn join(actions: &[OutputAction]) -> String {
    actions.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

/// The newest [`ACTIVITY_CAPACITY`] entries, shared with the UI.
#[derive(Debug, Default)]
pub(crate) struct ActivityLog {
    entries: VecDeque<Activity>,
    last_seq: u64,
}

impl ActivityLog {
    /// Adds `entry`, giving it the next sequence number.
    pub(crate) fn push(&mut self, mut entry: Activity) {
        self.last_seq += 1;
        entry.seq = self.last_seq;
        if self.entries.len() == ACTIVITY_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Clones the entries newer than `seq`, oldest first.
    pub(crate) fn since(&self, seq: u64) -> Vec<Activity> {
        let start = self.entries.partition_point(|entry| entry.seq <= seq);
        self.entries.range(start..).cloned().collect()
    }
}

/// What made the engine step.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Cause<'a> {
    Input(&'a [InputEvent]),
    Timer,
    /// A setting changed; the text says how mapping changed, e.g. "released: mapping switched
    /// off", or is empty when it did not.
    Mapping(&'a str),
}

/// One engine step, as the recorder sees it.
pub(crate) struct Step<'a> {
    pub(crate) now: Duration,
    pub(crate) cause: Cause<'a>,
    /// The profile in effect after the step.
    pub(crate) profile: Option<&'a Profile>,
    /// Why presses do nothing, when no profile is in effect.
    pub(crate) off: Option<&'a str>,
    /// The mouse gate was holding mouse output back.
    pub(crate) testing: bool,
    /// Whether a pad has a virtual Xbox controller, when the profile asks for one.
    pub(crate) passes_through: &'a dyn Fn(PadId) -> bool,
    pub(crate) sent: &'a [OutputAction],
    pub(crate) held_back: &'a [OutputAction],
    /// The layer active on a pad after the step.
    pub(crate) layer_of: &'a dyn Fn(PadId) -> Option<usize>,
}

/// Builds log entries from engine steps. Lives on the engine thread.
#[derive(Debug, Default)]
pub(crate) struct Recorder {
    pads: HashMap<PadId, Track>,
    burst: Option<Burst>,
}

/// What the recorder knows about one pad.
#[derive(Debug, Default)]
struct Track {
    /// Indexed like [`Axis`].
    axes: [f32; 6],
    /// The layer active after the last step.
    layer: usize,
    /// Buttons held down, with when they went down.
    held: Vec<(Button, Duration)>,
}

impl Track {
    fn stick(&self, side: Side) -> (f32, f32) {
        match side {
            Side::Left => (self.axes[axis_index(Axis::LeftX)], self.axes[axis_index(Axis::LeftY)]),
            Side::Right => {
                (self.axes[axis_index(Axis::RightX)], self.axes[axis_index(Axis::RightY)])
            }
        }
    }

    fn trigger(&self, side: Side) -> f32 {
        self.axes[trigger_index(side)]
    }
}

/// A stick or a trigger, which may move in a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Stick(Side),
    Trigger(Side),
}

/// A stick or trigger that moved in a step, with the pad's axes from before the step.
#[derive(Debug)]
struct Moved {
    pad: PadId,
    group: Group,
    before: [f32; 6],
}

/// Pointer and scroll movement added up while a stick keeps moving.
#[derive(Debug)]
struct Burst {
    started: Duration,
    last: Duration,
    sent: Motion,
    held_back: Motion,
    /// The sticks doing the moving, as last seen.
    sticks: Vec<PadInput>,
}

impl Burst {
    fn deadline(&self) -> Duration {
        (self.last + BURST_GAP).min(self.started + BURST_SPAN)
    }

    fn into_activity(self) -> Option<Activity> {
        let (sent, held_back) = (self.sent.actions(), self.held_back.actions());
        if sent.is_empty() && held_back.is_empty() {
            return None;
        }
        let mut why = Why::default();
        let span = self.last.saturating_sub(self.started);
        if span >= Duration::from_millis(1) {
            why.add(format!("added up over {} ms", span.as_millis()));
        }
        if !held_back.is_empty() {
            why.add(MOUSE_HELD_BACK);
        }
        Some(Activity {
            seq: 0,
            at: self.started,
            origin: if self.sticks.is_empty() { Origin::Timer } else { Origin::Pad },
            input: self.sticks,
            sent,
            held_back,
            why: why.text(),
        })
    }
}

/// Pointer and wheel movement, summed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Motion {
    dx: i32,
    dy: i32,
    vertical: i32,
    horizontal: i32,
}

impl Motion {
    fn add(&mut self, action: OutputAction) {
        match action {
            OutputAction::MouseMove { dx, dy } => {
                self.dx = self.dx.saturating_add(dx);
                self.dy = self.dy.saturating_add(dy);
            }
            OutputAction::Wheel { vertical, horizontal } => {
                self.vertical = self.vertical.saturating_add(vertical);
                self.horizontal = self.horizontal.saturating_add(horizontal);
            }
            OutputAction::KeyDown(_)
            | OutputAction::KeyUp(_)
            | OutputAction::MouseButton { .. } => {}
        }
    }

    fn merge(&mut self, other: Motion) {
        self.add(OutputAction::MouseMove { dx: other.dx, dy: other.dy });
        self.add(OutputAction::Wheel { vertical: other.vertical, horizontal: other.horizontal });
    }

    fn is_empty(self) -> bool {
        self == Self::default()
    }

    fn actions(self) -> Vec<OutputAction> {
        let mut actions = Vec::new();
        if self.dx != 0 || self.dy != 0 {
            actions.push(OutputAction::MouseMove { dx: self.dx, dy: self.dy });
        }
        if self.vertical != 0 || self.horizontal != 0 {
            actions
                .push(OutputAction::Wheel { vertical: self.vertical, horizontal: self.horizontal });
        }
        actions
    }
}

/// The parts of an entry's "why", each once, in order.
#[derive(Debug, Default)]
struct Why(Vec<String>);

impl Why {
    fn add(&mut self, part: impl Into<String>) {
        let part = part.into();
        if !part.is_empty() && !self.0.contains(&part) {
            self.0.push(part);
        }
    }

    fn text(self) -> String {
        self.0.join(", ")
    }
}

impl Recorder {
    /// When the recorder needs a step even without input: to close a burst of movement.
    pub(crate) fn next_deadline(&self) -> Option<Duration> {
        self.burst.as_ref().map(Burst::deadline)
    }

    /// Records one step and returns the entries it completes, not yet numbered: a burst of
    /// movement that has ended, and the step itself if it is worth showing.
    pub(crate) fn record(&mut self, step: &Step<'_>) -> Vec<Activity> {
        let events: &[InputEvent] = match step.cause {
            Cause::Input(events) => events,
            Cause::Timer | Cause::Mapping(_) => &[],
        };
        let mut input = Vec::new();
        let mut why = Why::default();
        let mut button_pads: Vec<PadId> = Vec::new();
        let mut moved: Vec<Moved> = Vec::new();
        for event in events {
            match *event {
                InputEvent::Connected { pad } => {
                    self.pads.entry(pad).or_default();
                    input.push(PadInput::Connected);
                }
                InputEvent::Disconnected { pad } => {
                    self.pads.remove(&pad);
                    input.push(PadInput::Disconnected);
                }
                InputEvent::Button { pad, button, pressed } => {
                    let track = self.pads.entry(pad).or_default();
                    if !button_pads.contains(&pad) {
                        button_pads.push(pad);
                    }
                    let held = if pressed {
                        track.held.retain(|&(b, _)| b != button);
                        track.held.push((button, step.now));
                        for part in press_reasons(step, pad, track.layer, button) {
                            why.add(part);
                        }
                        None
                    } else {
                        let i = track.held.iter().position(|&(b, _)| b == button);
                        let held = i.map(|i| step.now.saturating_sub(track.held.swap_remove(i).1));
                        if let Some(held) = held {
                            why.add(format!("held {} ms", held.as_millis()));
                        }
                        why.add(step.off.unwrap_or_default());
                        held
                    };
                    input.push(PadInput::Button { button, pressed, held });
                }
                InputEvent::Axis { pad, axis, value } => {
                    let track = self.pads.entry(pad).or_default();
                    let group = group_of(axis);
                    if !moved.iter().any(|m| m.pad == pad && m.group == group) {
                        moved.push(Moved { pad, group, before: track.axes });
                    }
                    track.axes[axis_index(axis)] = value;
                }
            }
        }
        if let Some(profile) = step.profile {
            for &pad in &button_pads {
                let (Some(track), Some(layer)) = (self.pads.get(&pad), (step.layer_of)(pad)) else {
                    continue;
                };
                if layer != track.layer
                    && let Some(layer) = profile.layers.get(layer)
                {
                    why.add(format!("now on layer {}", layer.name));
                }
            }
        }

        // Pointer movement is always a stick's doing. So is the wheel while a stick scrolls,
        // unless a button press caused it.
        let buttons = !button_pads.is_empty();
        let wheel =
            step.sent.iter().chain(step.held_back).any(|a| matches!(a, OutputAction::Wheel { .. }));
        let scrolling = !buttons && wheel && self.scrolling(step.profile);
        let is_motion = |action: &OutputAction| match action {
            OutputAction::MouseMove { .. } => true,
            OutputAction::Wheel { .. } => scrolling,
            OutputAction::KeyDown(_)
            | OutputAction::KeyUp(_)
            | OutputAction::MouseButton { .. } => false,
        };
        let (mut sent, mut held_back) = (Vec::new(), Vec::new());
        let (mut sent_motion, mut held_motion) = (Motion::default(), Motion::default());
        for &action in step.sent {
            if is_motion(&action) { sent_motion.add(action) } else { sent.push(action) }
        }
        for &action in step.held_back {
            if is_motion(&action) { held_motion.add(action) } else { held_back.push(action) }
        }
        let outputs = !sent.is_empty() || !held_back.is_empty();
        let log = match step.cause {
            Cause::Input(_) => !input.is_empty() || outputs,
            Cause::Timer | Cause::Mapping(_) => outputs,
        };

        let mut entries = Vec::new();
        if self.burst.as_ref().is_some_and(|burst| log || step.now >= burst.deadline()) {
            entries.extend(self.burst.take().and_then(Burst::into_activity));
        }
        if !sent_motion.is_empty() || !held_motion.is_empty() {
            let burst = self.burst.get_or_insert_with(|| Burst {
                started: step.now,
                last: step.now,
                sent: Motion::default(),
                held_back: Motion::default(),
                sticks: Vec::new(),
            });
            burst.last = step.now;
            burst.sent.merge(sent_motion);
            burst.held_back.merge(held_motion);
            moving_sticks(&self.pads, step.profile, &mut burst.sticks);
        }

        if log {
            let origin = match step.cause {
                Cause::Input(_) => Origin::Pad,
                Cause::Timer => Origin::Timer,
                Cause::Mapping(_) => Origin::Mapping,
            };
            match step.cause {
                Cause::Input(_) if !buttons && outputs => {
                    for (pad_input, reason) in self.fired_axes(&moved, step.profile) {
                        input.push(pad_input);
                        why.add(reason);
                    }
                }
                Cause::Input(_) => {}
                Cause::Timer => why.add(self.timer_reason(step.profile)),
                Cause::Mapping(text) => why.add(text),
            }
            if outputs && input.contains(&PadInput::Disconnected) {
                why.add("released what it held");
            }
            if !held_back.is_empty() {
                why.add(if step.testing { MOUSE_HELD_BACK } else { PRESS_HELD_BACK });
            }
            entries.push(Activity {
                seq: 0,
                at: step.now,
                origin,
                input,
                sent,
                held_back,
                why: why.text(),
            });
        }

        for (&pad, track) in &mut self.pads {
            track.layer = (step.layer_of)(pad).unwrap_or(0);
        }
        entries
    }

    /// Whether some stick is scrolling: in scroll mode and outside its dead zone.
    fn scrolling(&self, profile: Option<&Profile>) -> bool {
        let Some(profile) = profile else { return false };
        self.pads.values().any(|track| {
            let Some(layer) = profile.layers.get(track.layer) else { return false };
            SIDES.into_iter().any(|side| {
                let stick = stick_of(layer, side);
                let (x, y) = track.stick(side);
                stick.mode == StickMode::Scroll && x.hypot(y) > stick.dead_zone
            })
        })
    }

    /// The sticks and triggers that moved in a step that sent something with no button involved,
    /// with what each did. Only those that can fire an assignment are listed, unless none can.
    fn fired_axes(&self, moved: &[Moved], profile: Option<&Profile>) -> Vec<(PadInput, String)> {
        let mut fired = Vec::new();
        let mut others = Vec::new();
        for m in moved {
            let Some(track) = self.pads.get(&m.pad) else { continue };
            let layer = profile.and_then(|p| p.layers.get(track.layer));
            let (pad_input, reason, fires) = match m.group {
                Group::Trigger(side) => {
                    let (value, before) = (track.trigger(side), m.before[trigger_index(side)]);
                    let reason = layer.map_or_else(String::new, |layer| {
                        trigger_reason(before, value, layer.trigger_threshold)
                    });
                    let fires = layer.is_some_and(|layer| trigger_of(layer, side).is_some());
                    (PadInput::Trigger { side, value }, reason, fires)
                }
                Group::Stick(side) => {
                    let (x, y) = track.stick(side);
                    let stick = layer.map(|layer| stick_of(layer, side));
                    let reason = stick.map_or_else(String::new, |stick| {
                        if x.hypot(y) >= stick.dead_zone {
                            format!("stick pushed {}", compass(x, y))
                        } else {
                            "stick back to centre".to_string()
                        }
                    });
                    let fires = stick.is_some_and(|stick| stick.mode == StickMode::Directions);
                    (PadInput::Stick { side, x, y }, reason, fires)
                }
            };
            if fires {
                fired.push((pad_input, reason));
            } else {
                others.push((pad_input, reason));
            }
        }
        if fired.is_empty() { others } else { fired }
    }

    /// A guess at which timer fired, from the controls held down.
    fn timer_reason(&self, profile: Option<&Profile>) -> &'static str {
        let Some(profile) = profile else { return "on a timer" };
        let mut hold_zone = false;
        for track in self.pads.values() {
            let Some(layer) = profile.layers.get(track.layer) else { continue };
            let buttons = track.held.iter().filter_map(|(button, _)| layer.buttons.get(button));
            let triggers = SIDES
                .into_iter()
                .filter(|&side| track.trigger(side) >= layer.trigger_threshold)
                .filter_map(|side| trigger_of(layer, side));
            for assignment in buttons.chain(triggers) {
                if assignment.turbo.is_some() {
                    return "turbo";
                }
                hold_zone |= assignment.slots.iter().any(|slot| matches!(slot, Slot::HoldZone(_)));
            }
        }
        if hold_zone { "hold zone reached" } else { "next step of a sequence" }
    }
}

/// Why a press did what it did, given the layer active before it.
fn press_reasons(step: &Step<'_>, pad: PadId, layer: usize, button: Button) -> Vec<String> {
    if let Some(off) = step.off {
        return vec![off.to_string()];
    }
    let Some(profile) = step.profile else { return Vec::new() };
    let Some(layer) = profile.layers.get(layer) else { return Vec::new() };
    let Some(assignment) = layer.buttons.get(&button).filter(|a| !a.is_empty()) else {
        let xbox = zpadder_model::xbox::button_name(button)
            .filter(|_| profile.virtual_pad.is_some() && (step.passes_through)(pad));
        return vec![match xbox {
            Some(_) if step.testing => XBOX_HELD_BACK.to_string(),
            Some(name) => format!("passed to the Xbox controller as {name}"),
            None => "nothing assigned".to_string(),
        }];
    };
    let mut parts = Vec::new();
    let name = assignment.name.trim();
    if !name.is_empty() {
        parts.push(format!("\"{name}\""));
    }
    if assignment.turbo.is_some() {
        parts.push("turbo".to_string());
    }
    if assignment.toggle {
        parts.push("toggle".to_string());
    }
    if assignment.slots.iter().any(|slot| matches!(slot, Slot::HoldZone(_) | Slot::ReleaseZone(_)))
    {
        parts.push("what it sends depends on how long it is held".to_string());
    }
    parts
}

fn trigger_reason(before: f32, value: f32, threshold: f32) -> String {
    if value < threshold {
        "trigger let go".to_string()
    } else if before < threshold {
        "trigger past press point".to_string()
    } else {
        format!("trigger pulled to {:.0}%", value * 100.0)
    }
}

/// Replaces `sticks` with the sticks moving the pointer or scrolling, if any are.
fn moving_sticks(
    pads: &HashMap<PadId, Track>,
    profile: Option<&Profile>,
    sticks: &mut Vec<PadInput>,
) {
    let Some(profile) = profile else { return };
    let moving = |track: &Track, side: Side| {
        let stick = stick_of(profile.layers.get(track.layer)?, side);
        let (x, y) = track.stick(side);
        let moves = matches!(stick.mode, StickMode::Mouse | StickMode::Scroll);
        (moves && x.hypot(y) > stick.dead_zone).then_some(PadInput::Stick { side, x, y })
    };
    let all = || {
        pads.values()
            .flat_map(|track| SIDES.into_iter().filter_map(move |side| moving(track, side)))
    };
    if all().next().is_some() {
        sticks.clear();
        sticks.extend(all());
    }
}

/// The nearest of eight compass directions for a stick at `(x, y)`.
fn compass(x: f32, y: f32) -> &'static str {
    const NAMES: [&str; 8] =
        ["right", "up-right", "up", "up-left", "left", "down-left", "down", "down-right"];
    // 0° is right, 90° is up.
    let angle = (-y).atan2(x).to_degrees().rem_euclid(360.0);
    NAMES[((angle + 22.5) / 45.0) as usize % 8]
}

fn group_of(axis: Axis) -> Group {
    match axis {
        Axis::LeftX | Axis::LeftY => Group::Stick(Side::Left),
        Axis::RightX | Axis::RightY => Group::Stick(Side::Right),
        Axis::LeftTrigger => Group::Trigger(Side::Left),
        Axis::RightTrigger => Group::Trigger(Side::Right),
    }
}

fn trigger_index(side: Side) -> usize {
    match side {
        Side::Left => axis_index(Axis::LeftTrigger),
        Side::Right => axis_index(Axis::RightTrigger),
    }
}

fn stick_of(layer: &Layer, side: Side) -> &Stick {
    match side {
        Side::Left => &layer.left_stick,
        Side::Right => &layer.right_stick,
    }
}

fn trigger_of(layer: &Layer, side: Side) -> Option<&Assignment> {
    match side {
        Side::Left => layer.left_trigger.as_ref(),
        Side::Right => layer.right_trigger.as_ref(),
    }
}

#[cfg(test)]
mod tests {
    use zpadder_model::KeyCode;

    use super::*;

    fn entry(seq: u64) -> Activity {
        Activity {
            seq,
            at: Duration::ZERO,
            origin: Origin::Timer,
            input: Vec::new(),
            sent: Vec::new(),
            held_back: Vec::new(),
            why: String::new(),
        }
    }

    #[test]
    fn the_log_keeps_the_newest_entries_and_reads_from_a_sequence_number() {
        let mut log = ActivityLog::default();
        for _ in 0..ACTIVITY_CAPACITY + 5 {
            log.push(entry(0));
        }
        let all = log.since(0);
        assert_eq!(all.len(), ACTIVITY_CAPACITY);
        assert_eq!(all.first().map(|e| e.seq), Some(6));
        assert_eq!(all.last().map(|e| e.seq), Some(ACTIVITY_CAPACITY as u64 + 5));
        let newer = log.since(ACTIVITY_CAPACITY as u64 + 3);
        assert_eq!(newer.iter().map(|e| e.seq).collect::<Vec<_>>(), [204, 205]);
        assert!(log.since(205).is_empty());
    }

    #[test]
    fn pad_input_reads_plainly() {
        let ms = Duration::from_millis;
        let up = PadInput::Button { button: Button::South, pressed: false, held: Some(ms(142)) };
        assert_eq!(up.to_string(), "south up after 142 ms");
        let trigger = PadInput::Trigger { side: Side::Right, value: 0.64 };
        assert_eq!(trigger.to_string(), "right trigger 64%");
    }

    #[test]
    fn compass_directions() {
        assert_eq!(compass(1.0, 0.0), "right");
        assert_eq!(compass(0.0, -1.0), "up");
        assert_eq!(compass(0.7, -0.7), "up-right");
        assert_eq!(compass(-0.7, 0.7), "down-left");
        assert_eq!(compass(0.0, 1.0), "down");
    }

    #[test]
    fn trigger_reasons() {
        assert_eq!(trigger_reason(0.1, 0.5, 0.2), "trigger past press point");
        assert_eq!(trigger_reason(0.5, 0.1, 0.2), "trigger let go");
        assert_eq!(trigger_reason(0.5, 0.8, 0.2), "trigger pulled to 80%");
    }

    #[test]
    fn motion_sums_and_prints_as_actions() {
        let mut motion = Motion::default();
        motion.add(OutputAction::MouseMove { dx: 3, dy: -1 });
        motion.add(OutputAction::MouseMove { dx: 4, dy: 0 });
        motion.add(OutputAction::KeyDown(KeyCode::A));
        assert_eq!(motion.actions(), [OutputAction::MouseMove { dx: 7, dy: -1 }]);
        motion.merge(Motion { vertical: -40, ..Motion::default() });
        assert_eq!(motion.actions().len(), 2);
    }
}
