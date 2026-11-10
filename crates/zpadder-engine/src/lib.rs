//! The ZPadder mapping engine: a pure state machine from controller input to keyboard and mouse
//! output.
//!
//! The engine never reads the clock or touches the OS. Callers pass the current time into
//! [`Engine::step`], so turbo, sequences, hold zones and stick-to-mouse are reproducible in tests
//! with a virtual clock.
//!
//! Rules every behaviour follows:
//!
//! - **A press owns its release.** Whatever a press put down, its release lifts, even if the layer
//!   changed in between. This is what keeps keys from getting stuck.
//! - **Outputs are reference-counted** across controls and pads: a key goes up only when the
//!   last control holding it lets go. A combination such as Ctrl+Z taps its key again when
//!   another control already holds it, so the application still sees the combination.
//! - **While-held layers form a stack**, so releasing them in any order lands on the right layer
//!   (Xpadder's "while held glitch" cannot happen).
//! - **Analog thresholds have hysteresis**, so values resting on a threshold do not chatter.
//! - **A held trigger or stick follows the layer it was pressed in** (its threshold, dead zone
//!   and mode) until it is let go, so a layer it selects cannot release it.
//!
//! A profile with a `virtual_pad` also drives a virtual Xbox 360 controller per pad: controls
//! with nothing assigned pass through to it ([`Engine::take_virtual_pad_changes`]). Whether a
//! button or trigger passes through is decided when it is pressed, like everything else, so a
//! press that went to the virtual controller stays there until it is let go.

mod activation;
mod held;
mod plan;
mod stick;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use zpadder_model::profile::{
    Assignment, Direction, Layer, MIN_TRIGGER_THRESHOLD, SelectMode, Stick, StickMode,
    WheelDirection,
};
use zpadder_model::{Axis, Button, InputEvent, OutputAction, PadId, Profile, XboxState, xbox};

use crate::activation::{Activation, Context, Sequence};
use crate::held::HeldOutputs;

/// How often the engine wants to run while the pointer is moving.
pub const TICK: Duration = Duration::from_millis(1);
/// Analog thresholds release this far below where they engage, or at half the threshold when
/// that is higher (see [`release_point`]).
const HYSTERESIS: f32 = 0.05;
/// One wheel notch, in Windows `WHEEL_DELTA` units.
const WHEEL_NOTCH: i32 = 120;
/// Smallest wheel movement sent by a scroll stick: a third of a notch, so apps that ignore tiny
/// deltas still scroll smoothly.
const SCROLL_STEP: f32 = 40.0;
/// Longest gap integrated as pointer movement, so a late step cannot fling the pointer.
const MAX_MOUSE_STEP: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Side {
    Left,
    Right,
}

const SIDES: [Side; 2] = [Side::Left, Side::Right];

impl Side {
    fn index(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }
}

/// Where an analog control that engaged at `threshold` lets go again. It is always above zero,
/// so a control back at rest (with a little noise) is released even when the threshold is small.
pub(crate) fn release_point(threshold: f32) -> f32 {
    (threshold - HYSTERESIS).max(threshold * 0.5)
}

/// A digital source that can start an assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Switch {
    Button(Button),
    Trigger(Side),
    Stick(Side, Direction),
}

/// Maps controller input to keyboard and mouse actions.
#[derive(Debug, Default)]
pub struct Engine {
    profile: Option<Arc<Profile>>,
    pads: HashMap<PadId, PadState>,
    held: HeldOutputs,
    now: Duration,
    /// Pads currently told to rumble.
    rumbling: HashSet<PadId>,
    rumble_changes: Vec<(PadId, bool)>,
    /// The virtual controller state last reported for each pad; neutral when missing.
    virtual_reported: HashMap<PadId, XboxState>,
    virtual_changes: Vec<(PadId, XboxState)>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn profile(&self) -> Option<&Arc<Profile>> {
        self.profile.as_ref()
    }

    /// Index of the layer active on `pad`, or `None` for a pad the engine has not seen.
    pub fn active_layer(&self, pad: PadId) -> Option<usize> {
        self.pads.get(&pad).map(PadState::active_layer)
    }

    /// Rumble changes since the last call, as `(pad, on)`. A pad rumbles while any of its held
    /// assignments has `rumble = true`.
    pub fn take_rumble_changes(&mut self) -> Vec<(PadId, bool)> {
        std::mem::take(&mut self.rumble_changes)
    }

    /// Virtual controller states that changed since the last call, as `(pad, state)`, for a
    /// profile with a `virtual_pad`: what each pad's controls with nothing assigned are doing,
    /// in Xbox 360 terms. Pads start out neutral, and are reported neutral again whenever
    /// everything is released (a profile change, [`Engine::release_all`], no profile). Pads
    /// that disconnect are forgotten without a report.
    pub fn take_virtual_pad_changes(&mut self) -> Vec<(PadId, XboxState)> {
        std::mem::take(&mut self.virtual_changes)
    }

    fn update_virtual(&mut self) {
        let profile = self.profile.as_deref();
        self.virtual_reported.retain(|pad, _| self.pads.contains_key(pad));
        for (&id, pad) in &self.pads {
            let state = profile.map_or(XboxState::NEUTRAL, |profile| pad.passthrough(profile));
            let reported = self.virtual_reported.get(&id).copied().unwrap_or_default();
            if state != reported {
                self.virtual_reported.insert(id, state);
                self.virtual_changes.push((id, state));
            }
        }
    }

    /// Rumble and virtual controller changes, after anything that can change them.
    fn update_feedback(&mut self) {
        self.update_rumble();
        self.update_virtual();
    }

    fn update_rumble(&mut self) {
        self.rumbling.retain(|pad| self.pads.contains_key(pad));
        for (&id, pad) in &self.pads {
            let want = pad.activations.iter().any(|activation| activation.rumble);
            if want != self.rumbling.contains(&id) {
                if want {
                    self.rumbling.insert(id);
                } else {
                    self.rumbling.remove(&id);
                }
                self.rumble_changes.push((id, want));
            }
        }
    }

    /// Replaces the profile for every pad. Everything held under the old profile is released
    /// first; sticks and triggers that are already deflected take effect under the new one.
    /// The profile must have passed [`Profile::validate`].
    ///
    /// A virtual controller is reported neutral first, then with what the new profile passes
    /// through of the sticks and triggers already deflected.
    pub fn set_profile(&mut self, profile: Option<Arc<Profile>>, out: &mut Vec<OutputAction>) {
        self.release_all(out);
        self.profile = profile;
        let pads: Vec<PadId> = self.pads.keys().copied().collect();
        for pad in pads {
            if let Some(state) = self.pads.get_mut(&pad) {
                state.at_rest = false;
            }
            self.update_analog(pad, out);
        }
        self.update_feedback();
    }

    /// Releases every held output, stops sequences and resets layers, as if all controls had
    /// been let go. A virtual controller is reported neutral, and stays so until its pad reports
    /// input again.
    pub fn release_all(&mut self, out: &mut Vec<OutputAction>) {
        for pad in self.pads.values_mut() {
            pad.reset(&mut self.held, out);
        }
        debug_assert!(self.held.is_empty(), "outputs still held after release_all");
        self.update_feedback();
    }

    /// Advances the engine to `now` (time since the engine started), applying `events` in order,
    /// and appends the actions to inject to `out`. The caller sends `out` as one batch.
    pub fn step(&mut self, events: &[InputEvent], now: Duration, out: &mut Vec<OutputAction>) {
        let now = now.max(self.now);
        let elapsed = (now - self.now).min(MAX_MOUSE_STEP);
        self.now = now;
        // The pointer kept moving during the gap, at the sticks' previous deflection.
        self.move_mouse(elapsed, out);

        let mut touched = Vec::new();
        for &event in events {
            let pad = match event {
                InputEvent::Connected { pad } => {
                    self.pads.entry(pad).or_default();
                    continue;
                }
                InputEvent::Disconnected { pad } => {
                    if let Some(mut state) = self.pads.remove(&pad) {
                        state.reset(&mut self.held, out);
                    }
                    continue;
                }
                InputEvent::Button { pad, button, pressed } => {
                    let state = self.pads.entry(pad).or_default();
                    state.at_rest = false;
                    let switch = Switch::Button(button);
                    match (&self.profile, pressed) {
                        (Some(profile), true) => {
                            press(profile, state, &mut self.held, switch, now, out);
                        }
                        (_, false) => release(state, &mut self.held, switch, now, out),
                        (None, true) => {}
                    }
                    pad
                }
                InputEvent::Axis { pad, axis, value } => {
                    let state = self.pads.entry(pad).or_default();
                    state.axes[axis_index(axis)] = value;
                    state.at_rest = false;
                    pad
                }
            };
            if !touched.contains(&pad) {
                touched.push(pad);
            }
        }
        // Layer changes and axis moves both affect sticks and triggers.
        for pad in touched {
            self.update_analog(pad, out);
        }

        for pad in self.pads.values_mut() {
            let mut ctx = Context { now, held: &mut self.held, out, sequences: &mut pad.sequences };
            for activation in &mut pad.activations {
                activation.update(&mut ctx);
            }
            pad.sequences.retain_mut(|sequence| !sequence.advance(now, &mut self.held, out));
        }
        self.update_feedback();
    }

    /// When [`Engine::step`] next needs calling without new input (for turbo, sequences, hold
    /// zones or a moving pointer), or `None` if the engine can wait for input indefinitely.
    pub fn next_wakeup(&self) -> Option<Duration> {
        let activations = self
            .pads
            .values()
            .flat_map(|pad| &pad.activations)
            .filter_map(Activation::next_deadline);
        let sequences =
            self.pads.values().flat_map(|pad| &pad.sequences).filter_map(Sequence::next_deadline);
        let mouse = self.pads.values().any(|pad| pad.mouse_moving).then_some(self.now + TICK);
        activations.chain(sequences).chain(mouse).min()
    }

    fn update_analog(&mut self, id: PadId, out: &mut Vec<OutputAction>) {
        let Some(profile) = self.profile.clone() else { return };
        let Some(pad) = self.pads.get_mut(&id) else { return };
        let now = self.now;

        for (i, side) in SIDES.into_iter().enumerate() {
            let value = pad.axes[axis_index(trigger_axis(side))];
            let pulled = match pad.trigger_on[i] {
                // A pulled trigger keeps the threshold it was pulled past, so a layer it selects
                // cannot release it.
                Some(threshold) => (value > release_point(threshold)).then_some(threshold),
                None => {
                    let layer = &profile.layers[pad.active_layer()];
                    // Validation enforces the minimum; it is repeated here so that no profile
                    // can leave a trigger unable to let go.
                    let threshold = layer.trigger_threshold.max(MIN_TRIGGER_THRESHOLD);
                    (value >= threshold).then_some(threshold)
                }
            };
            if pulled.is_some() != pad.trigger_on[i].is_some() {
                pad.trigger_on[i] = pulled;
                let switch = Switch::Trigger(side);
                if pulled.is_some() {
                    press(&profile, pad, &mut self.held, switch, now, out);
                } else {
                    release(pad, &mut self.held, switch, now, out);
                }
            }
        }

        for (i, side) in SIDES.into_iter().enumerate() {
            // A stick pressing directions keeps the settings of the layer it was pushed in until
            // it centres, so a layer one of its directions selects cannot release it.
            let layer = pad.stick_layer[i].unwrap_or_else(|| pad.active_layer());
            let stick = stick_of(&profile.layers[layer], side);
            let (x, y) = pad.stick(side);
            let new = if stick.mode == StickMode::Directions {
                let previous = &pad.stick_directions[i];
                stick::directions(x, y, stick.dead_zone, &stick.directions, previous)
            } else {
                Vec::new()
            };
            pad.stick_layer[i] = (!new.is_empty()).then_some(layer);
            let old = std::mem::take(&mut pad.stick_directions[i]);
            for &direction in old.iter().filter(|d| !new.contains(d)) {
                release(pad, &mut self.held, Switch::Stick(side, direction), now, out);
            }
            for &direction in new.iter().filter(|d| !old.contains(d)) {
                press(&profile, pad, &mut self.held, Switch::Stick(side, direction), now, out);
            }
            pad.stick_directions[i] = new;
        }

        // Analog controls that are held follow their distance zones.
        let axes = pad.axes;
        let mut ctx = Context { now, held: &mut self.held, out, sequences: &mut pad.sequences };
        for activation in &mut pad.activations {
            activation.set_value(switch_value(&axes, activation.switch), &mut ctx);
        }

        pad.mouse_moving = SIDES.into_iter().any(|side| {
            pad.mouse_velocity(&profile, side).is_some()
                || pad.scroll_velocity(&profile, side).is_some()
        });
    }

    fn move_mouse(&mut self, elapsed: Duration, out: &mut Vec<OutputAction>) {
        let Some(profile) = &self.profile else { return };
        let dt = elapsed.as_secs_f32();
        for pad in self.pads.values_mut() {
            for (i, side) in SIDES.into_iter().enumerate() {
                let Some((vx, vy)) = pad.mouse_velocity(profile, side) else {
                    pad.mouse_remainder[i] = (0.0, 0.0);
                    continue;
                };
                let remainder = &mut pad.mouse_remainder[i];
                remainder.0 += vx * dt;
                remainder.1 += vy * dt;
                if !(remainder.0.is_finite() && remainder.1.is_finite()) {
                    *remainder = (0.0, 0.0);
                    continue;
                }
                let (dx, dy) = (remainder.0.trunc(), remainder.1.trunc());
                remainder.0 -= dx;
                remainder.1 -= dy;
                if dx != 0.0 || dy != 0.0 {
                    // `as` saturates, so even an absurd speed cannot wrap around.
                    out.push(OutputAction::MouseMove { dx: dx as i32, dy: dy as i32 });
                }
            }
            for (i, side) in SIDES.into_iter().enumerate() {
                let Some((vertical, horizontal)) = pad.scroll_velocity(profile, side) else {
                    pad.scroll_remainder[i] = (0.0, 0.0);
                    continue;
                };
                let remainder = &mut pad.scroll_remainder[i];
                remainder.0 += vertical * dt;
                remainder.1 += horizontal * dt;
                let take = |r: &mut f32| {
                    if r.abs() < SCROLL_STEP {
                        return 0;
                    }
                    let amount = r.trunc();
                    *r -= amount;
                    amount as i32
                };
                let (vertical, horizontal) = (take(&mut remainder.0), take(&mut remainder.1));
                if vertical != 0 || horizontal != 0 {
                    out.push(OutputAction::Wheel { vertical, horizontal });
                }
            }
        }
    }
}

/// Everything the engine tracks for one pad.
#[derive(Debug, Default)]
struct PadState {
    /// The layer selected by one-way and two-way selectors.
    base_layer: usize,
    /// Where a two-way selector returns to.
    two_way_previous: Option<usize>,
    /// While-held selectors currently held, in press order; the last one wins.
    held_layers: Vec<(Switch, usize)>,
    pressed: HashSet<Switch>,
    /// Buttons and triggers pressed while nothing was assigned to them, which therefore pass
    /// through to the virtual controller until they are let go.
    passing: HashSet<Switch>,
    /// Everything was just released: the virtual controller's sticks and triggers stay centred
    /// until the pad reports input again, as if they had been let go too.
    at_rest: bool,
    activations: Vec<Activation>,
    sequences: Vec<Sequence>,
    /// Next alternative for each cycling assignment, by layer and switch.
    cycles: HashMap<(usize, Switch), usize>,
    axes: [f32; 6],
    /// For each pulled trigger, the threshold it was pulled past.
    trigger_on: [Option<f32>; 2],
    stick_directions: [Vec<Direction>; 2],
    /// For each stick pressing directions, the layer whose stick settings it follows.
    stick_layer: [Option<usize>; 2],
    /// Sub-pixel pointer movement carried between steps.
    mouse_remainder: [(f32, f32); 2],
    /// Partial wheel movement carried between steps, per stick.
    scroll_remainder: [(f32, f32); 2],
    mouse_moving: bool,
}

impl PadState {
    fn active_layer(&self) -> usize {
        self.held_layers.last().map_or(self.base_layer, |&(_, layer)| layer)
    }

    fn stick(&self, side: Side) -> (f32, f32) {
        stick_position(&self.axes, side)
    }

    /// The settings `side`'s stick follows: those of the layer it was pushed in while it presses
    /// directions, otherwise the active layer's.
    fn stick_settings<'p>(&self, profile: &'p Profile, side: Side) -> &'p Stick {
        let layer = self.stick_layer[side.index()].unwrap_or_else(|| self.active_layer());
        stick_of(&profile.layers[layer], side)
    }

    fn mouse_velocity(&self, profile: &Profile, side: Side) -> Option<(f32, f32)> {
        let stick = self.stick_settings(profile, side);
        if stick.mode != StickMode::Mouse {
            return None;
        }
        let (x, y) = self.stick(side);
        stick::mouse_velocity(x, y, stick)
    }

    fn scroll_velocity(&self, profile: &Profile, side: Side) -> Option<(f32, f32)> {
        let stick = self.stick_settings(profile, side);
        if stick.mode != StickMode::Scroll {
            return None;
        }
        let (x, y) = self.stick(side);
        stick::scroll_velocity(x, y, stick)
    }

    /// What the pad's controls with nothing assigned are doing, in Xbox 360 terms, or neutral
    /// if the profile has no virtual controller.
    ///
    /// - Buttons pressed with nothing assigned in the layer active at the time, or with an
    ///   assignment marked `passthrough`, pass through by position (see [`xbox::button_bit`])
    ///   until they are let go.
    /// - A trigger passes its pull through while it is not pulled past its press point and has
    ///   nothing assigned (or a `passthrough` assignment) in the active layer, and once pulled,
    ///   if that was so when it was.
    /// - A stick passes through while its mode is `off` or it is marked `passthrough`, in the
    ///   layer it follows.
    fn passthrough(&self, profile: &Profile) -> XboxState {
        let mut state = XboxState::NEUTRAL;
        if profile.virtual_pad.is_none() {
            return state;
        }
        if self.at_rest {
            // Nothing can be held either: `reset` cleared `passing`.
            return state;
        }
        let layer = &profile.layers[self.active_layer()];
        for switch in &self.passing {
            if let Switch::Button(button) = *switch
                && let Some(bit) = xbox::button_bit(button)
            {
                state.buttons |= bit;
            }
        }
        for (i, side) in SIDES.into_iter().enumerate() {
            let switch = Switch::Trigger(side);
            let passes = match self.trigger_on[i] {
                Some(_) => self.passing.contains(&switch),
                None => passes(lookup(layer, switch)),
            };
            let value =
                if passes { xbox::trigger(self.axes[axis_index(trigger_axis(side))]) } else { 0 };
            let stick = self.stick_settings(profile, side);
            let (x, y) = if stick.mode == StickMode::Off || stick.passthrough {
                let (x, y) = self.stick(side);
                // ZPadder's Y is positive downwards, XInput's upwards.
                (xbox::stick_axis(x), xbox::stick_axis(-y))
            } else {
                (0, 0)
            };
            match side {
                Side::Left => (state.left_trigger, state.left_x, state.left_y) = (value, x, y),
                Side::Right => (state.right_trigger, state.right_x, state.right_y) = (value, x, y),
            }
        }
        state
    }

    /// Releases everything this pad holds, stops its sequences and returns it to the first
    /// layer. Axis values are kept, since they still describe the physical controls.
    fn reset(&mut self, held: &mut HeldOutputs, out: &mut Vec<OutputAction>) {
        for mut activation in self.activations.drain(..) {
            activation.abort(held, out);
        }
        for mut sequence in self.sequences.drain(..) {
            sequence.stop(held, out);
        }
        *self = Self { axes: self.axes, at_rest: true, ..Self::default() };
    }
}

/// Whether a control with this assignment also works as its Xbox counterpart: when it has
/// nothing assigned, or its assignment says so.
fn passes(assignment: Option<&Assignment>) -> bool {
    assignment.is_none_or(|assignment| assignment.is_empty() || assignment.passthrough)
}

fn press(
    profile: &Profile,
    pad: &mut PadState,
    held: &mut HeldOutputs,
    switch: Switch,
    now: Duration,
    out: &mut Vec<OutputAction>,
) {
    if !pad.pressed.insert(switch) {
        return;
    }
    // Pressing a latched toggle turns it off.
    if let Some(i) = pad.activations.iter().position(|a| a.switch == switch && a.latched) {
        pad.activations.swap_remove(i).abort(held, out);
        return;
    }
    // Pressing again while its sequence runs stops the sequence.
    if let Some(i) = pad.sequences.iter().position(|s| s.owner == switch && s.stoppable) {
        pad.sequences.swap_remove(i).stop(held, out);
        return;
    }
    let layer = pad.active_layer();
    let assignment = lookup(&profile.layers[layer], switch);
    if passes(assignment) && !matches!(switch, Switch::Stick(..)) {
        pad.passing.insert(switch);
    }
    let Some(assignment) = assignment else { return };
    if let Some(select) = &assignment.layer
        && let Some(target) = profile.layer_index(&select.target)
    {
        match select.mode {
            SelectMode::OneWay => pad.base_layer = target,
            SelectMode::TwoWay if pad.base_layer == target => {
                if let Some(previous) = pad.two_way_previous.take() {
                    pad.base_layer = previous;
                }
            }
            SelectMode::TwoWay => {
                pad.two_way_previous = Some(pad.base_layer);
                pad.base_layer = target;
            }
            SelectMode::WhileHeld => pad.held_layers.push((switch, target)),
        }
    }

    let mut plans = plan::compile(&assignment.slots);
    let alternative = if plans.len() > 1 {
        let next = pad.cycles.entry((layer, switch)).or_insert(0);
        let current = *next % plans.len();
        *next = (current + 1) % plans.len();
        current
    } else {
        0
    };
    let plan = plans.swap_remove(alternative);
    let value = switch_value(&pad.axes, switch);
    let mut ctx = Context { now, held, out, sequences: &mut pad.sequences };
    let activation = Activation::start(switch, plan, assignment, value, &mut ctx);
    pad.activations.push(activation);
}

fn release(
    pad: &mut PadState,
    held: &mut HeldOutputs,
    switch: Switch,
    now: Duration,
    out: &mut Vec<OutputAction>,
) {
    if !pad.pressed.remove(&switch) {
        return;
    }
    pad.passing.remove(&switch);
    pad.held_layers.retain(|&(s, _)| s != switch);
    let Some(i) = pad.activations.iter().position(|a| a.switch == switch && !a.latched) else {
        return;
    };
    let mut ctx = Context { now, held, out, sequences: &mut pad.sequences };
    // The control was held this long even if the engine has not run since a hold zone fell
    // due, so that zone still takes effect before the release.
    pad.activations[i].enter_due_hold_zones(&mut ctx);
    if !pad.activations[i].release(&mut ctx) {
        pad.activations.swap_remove(i);
    }
}

fn lookup(layer: &Layer, switch: Switch) -> Option<&Assignment> {
    match switch {
        Switch::Button(button) => layer.buttons.get(&button),
        Switch::Trigger(Side::Left) => layer.left_trigger.as_ref(),
        Switch::Trigger(Side::Right) => layer.right_trigger.as_ref(),
        Switch::Stick(side, direction) => stick_of(layer, side).directions.get(direction),
    }
}

/// How far a switch is pressed, 0–1: buttons are fully pressed, triggers report their pull and
/// stick directions their stick's deflection.
fn switch_value(axes: &[f32; 6], switch: Switch) -> f32 {
    match switch {
        Switch::Button(_) => 1.0,
        Switch::Trigger(side) => axes[axis_index(trigger_axis(side))],
        Switch::Stick(side, _) => {
            let (x, y) = stick_position(axes, side);
            x.hypot(y).min(1.0)
        }
    }
}

fn stick_position(axes: &[f32; 6], side: Side) -> (f32, f32) {
    match side {
        Side::Left => (axes[axis_index(Axis::LeftX)], axes[axis_index(Axis::LeftY)]),
        Side::Right => (axes[axis_index(Axis::RightX)], axes[axis_index(Axis::RightY)]),
    }
}

fn stick_of(layer: &Layer, side: Side) -> &Stick {
    match side {
        Side::Left => &layer.left_stick,
        Side::Right => &layer.right_stick,
    }
}

fn trigger_axis(side: Side) -> Axis {
    match side {
        Side::Left => Axis::LeftTrigger,
        Side::Right => Axis::RightTrigger,
    }
}

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::LeftX => 0,
        Axis::LeftY => 1,
        Axis::RightX => 2,
        Axis::RightY => 3,
        Axis::LeftTrigger => 4,
        Axis::RightTrigger => 5,
    }
}

fn wheel(direction: WheelDirection) -> OutputAction {
    let (vertical, horizontal) = match direction {
        WheelDirection::Up => (WHEEL_NOTCH, 0),
        WheelDirection::Down => (-WHEEL_NOTCH, 0),
        WheelDirection::Left => (0, -WHEEL_NOTCH),
        WheelDirection::Right => (0, WHEEL_NOTCH),
    };
    OutputAction::Wheel { vertical, horizontal }
}
