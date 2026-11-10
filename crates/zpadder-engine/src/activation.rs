//! Running assignments and sequences.

use std::time::Duration;

use zpadder_model::OutputAction;
use zpadder_model::profile::Assignment;

use crate::held::{Held, HeldOutputs};
use crate::plan::{Body, Output, Plan, Step};
use crate::{Switch, release_point, wheel};

/// What an activation touches when it changes.
pub(crate) struct Context<'a> {
    pub(crate) now: Duration,
    pub(crate) held: &'a mut HeldOutputs,
    pub(crate) out: &'a mut Vec<OutputAction>,
    pub(crate) sequences: &'a mut Vec<Sequence>,
}

/// A running assignment, created when its switch is pressed. It owns everything it holds down.
#[derive(Debug)]
pub(crate) struct Activation {
    pub(crate) switch: Switch,
    plan: Plan,
    pressed_at: Duration,
    /// Index of the active segment in `plan.held` or, for analog plans, `plan.distance`.
    segment: usize,
    /// The active segment's outputs.
    current: Vec<Output>,
    /// What this activation has pressed and not yet released.
    holding: Vec<Held>,
    turbo: Option<Turbo>,
    toggle: bool,
    /// The pad vibrates while this activation is alive.
    pub(crate) rumble: bool,
    /// A toggle whose switch was released: its outputs stay down until the next press.
    pub(crate) latched: bool,
}

#[derive(Debug)]
struct Turbo {
    half_period: Duration,
    /// Deadline of the next flip.
    next: Duration,
    down: bool,
}

impl Activation {
    /// Starts one alternative (`plan`) of `assignment`. `value` is the control's deflection (1
    /// for buttons), used to pick a distance zone.
    pub(crate) fn start(
        switch: Switch,
        plan: Plan,
        assignment: &Assignment,
        value: f32,
        ctx: &mut Context<'_>,
    ) -> Self {
        let mut activation = Self {
            switch,
            plan,
            pressed_at: ctx.now,
            segment: 0,
            current: Vec::new(),
            holding: Vec::new(),
            turbo: None,
            toggle: assignment.toggle,
            rumble: assignment.rumble,
            latched: false,
        };
        if let Some(rate) = assignment.turbo {
            let half_period = Duration::from_nanos((5e8 / f64::from(rate)).round() as u64);
            activation.turbo = Some(Turbo { half_period, next: ctx.now + half_period, down: true });
        }
        if activation.plan.distance.is_empty() {
            activation.enter_held(0, ctx);
        } else {
            activation.segment = activation.distance_segment(value, None);
            let outputs = activation.plan.distance[activation.segment].1.clone();
            activation.set_outputs(outputs, ctx);
        }
        activation
    }

    /// Moves to the next hold zone when it is due, and runs turbo.
    pub(crate) fn update(&mut self, ctx: &mut Context<'_>) {
        self.enter_due_hold_zones(ctx);
        self.tick_turbo(ctx);
    }

    /// Moves through every hold zone that is due by now.
    pub(crate) fn enter_due_hold_zones(&mut self, ctx: &mut Context<'_>) {
        while let Some(start) = self.next_hold_zone()
            && ctx.now >= start
        {
            self.enter_held(self.segment + 1, ctx);
        }
    }

    /// Follows an analog control through its distance zones.
    pub(crate) fn set_value(&mut self, value: f32, ctx: &mut Context<'_>) {
        if self.plan.distance.is_empty() {
            return;
        }
        let segment = self.distance_segment(value, Some(self.segment));
        if segment != self.segment {
            self.segment = segment;
            let outputs = self.plan.distance[segment].1.clone();
            self.set_outputs(outputs, ctx);
        }
    }

    /// The switch was released. Returns `true` if the activation stays alive (a latched toggle).
    pub(crate) fn release(&mut self, ctx: &mut Context<'_>) -> bool {
        if self.toggle {
            self.latched = true;
            return true;
        }
        self.disengage(ctx.held, ctx.out);
        let held_for = ctx.now - self.pressed_at;
        if let Some((_, Body::Sequence(steps))) =
            self.plan.release.iter().rev().find(|(start, _)| *start <= held_for)
            && !steps.is_empty()
        {
            ctx.sequences.push(Sequence::new(self.switch, steps.clone(), ctx.now, false));
        }
        false
    }

    /// Lets go of everything at once (profile change, disconnect, shutdown).
    pub(crate) fn abort(&mut self, held: &mut HeldOutputs, out: &mut Vec<OutputAction>) {
        self.disengage(held, out);
    }

    pub(crate) fn next_deadline(&self) -> Option<Duration> {
        let turbo = self.turbo.as_ref().map(|turbo| turbo.next);
        turbo.into_iter().chain(self.next_hold_zone()).min()
    }

    fn next_hold_zone(&self) -> Option<Duration> {
        if self.latched || !self.plan.distance.is_empty() {
            return None;
        }
        self.plan.held.get(self.segment + 1).map(|(start, _)| self.pressed_at + *start)
    }

    fn enter_held(&mut self, index: usize, ctx: &mut Context<'_>) {
        self.segment = index;
        match &self.plan.held[index].1 {
            Body::Hold(outputs) => {
                let outputs = outputs.clone();
                self.set_outputs(outputs, ctx);
            }
            Body::Sequence(steps) => {
                let steps = steps.clone();
                self.set_outputs(Vec::new(), ctx);
                ctx.sequences.push(Sequence::new(self.switch, steps, ctx.now, true));
            }
        }
    }

    /// Replaces the held outputs. Outputs held in both segments stay down throughout; the old
    /// segment's others come up (in reverse) before the new segment's go down, so the two never
    /// combine: moving from Ctrl+C to X must not type Ctrl+X.
    fn set_outputs(&mut self, outputs: Vec<Output>, ctx: &mut Context<'_>) {
        let mut old = std::mem::take(&mut self.holding);
        self.current = outputs;
        // Each output of the new segment that the old one holds too stays down. (In turbo's up
        // phase nothing is held, and the new outputs wait for the next flip.)
        let mut kept = vec![false; self.current.len()];
        for (keep, output) in kept.iter_mut().zip(&self.current) {
            if let Output::Hold(held) = *output
                && let Some(i) = old.iter().position(|&h| h == held)
            {
                old.remove(i);
                *keep = true;
            }
        }
        for output in old.into_iter().rev() {
            ctx.held.release(output, ctx.out);
        }
        if self.turbo.as_ref().is_none_or(|turbo| turbo.down) {
            self.engage(&kept, ctx);
        }
    }

    /// Presses the current outputs in order, except those marked in `kept`, which this
    /// activation already holds.
    fn engage(&mut self, kept: &[bool], ctx: &mut Context<'_>) {
        let mut modifier = false;
        for (i, &output) in self.current.iter().enumerate() {
            match output {
                Output::Hold(held) => {
                    if !kept.get(i).copied().unwrap_or(false) {
                        ctx.held.press(held, modifier, ctx.out);
                    }
                    modifier |= held.is_modifier();
                    self.holding.push(held);
                }
                Output::Wheel(direction) => ctx.out.push(wheel(direction)),
            }
        }
    }

    fn disengage(&mut self, held: &mut HeldOutputs, out: &mut Vec<OutputAction>) {
        for output in self.holding.drain(..).rev() {
            held.release(output, out);
        }
    }

    /// Flips a turbo's outputs at each deadline. If the engine fell behind, missed flips are
    /// skipped rather than replayed in a burst.
    fn tick_turbo(&mut self, ctx: &mut Context<'_>) {
        let Some(turbo) = &mut self.turbo else { return };
        if ctx.now < turbo.next {
            return;
        }
        turbo.down = !turbo.down;
        turbo.next += turbo.half_period;
        if turbo.next <= ctx.now {
            turbo.next = ctx.now + turbo.half_period;
        }
        if turbo.down {
            self.engage(&[], ctx);
        } else {
            self.disengage(ctx.held, ctx.out);
        }
    }

    /// The distance zone for `value`. Moving down out of the `current` zone needs the value to
    /// drop a little below its edge, so a resting value does not chatter.
    fn distance_segment(&self, value: f32, current: Option<usize>) -> usize {
        let zones = &self.plan.distance;
        let index = zones.iter().rposition(|&(from, _)| value >= from).unwrap_or(0);
        match current {
            Some(current) if index < current && value > release_point(zones[current].0) => current,
            _ => index,
        }
    }
}

/// Taps and pauses that run to completion, independent of the switch that started them.
#[derive(Debug)]
pub(crate) struct Sequence {
    pub(crate) owner: Switch,
    /// Started on press, so pressing the owner again stops it (Xpadder's behaviour). Sequences
    /// fired on release are not stoppable, so quick repeated taps are never swallowed.
    pub(crate) stoppable: bool,
    steps: Vec<Step>,
    index: usize,
    /// When the step at `index` is due.
    next: Duration,
    holding: Vec<Held>,
}

impl Sequence {
    fn new(owner: Switch, steps: Vec<Step>, now: Duration, stoppable: bool) -> Self {
        Self { owner, stoppable, steps, index: 0, next: now, holding: Vec::new() }
    }

    /// Runs every step that is due and returns `true` when the sequence has finished. An
    /// overdue wait still ends the batch, so a tap's press and release never share one
    /// `SendInput` call (a game could miss a zero-length tap).
    pub(crate) fn advance(
        &mut self,
        now: Duration,
        held: &mut HeldOutputs,
        out: &mut Vec<OutputAction>,
    ) -> bool {
        while self.index < self.steps.len() && now >= self.next {
            let step = &self.steps[self.index];
            self.index += 1;
            match step {
                Step::Press(outputs) => {
                    let mut modifier = false;
                    for &output in outputs {
                        match output {
                            Output::Hold(output) => {
                                held.press(output, modifier, out);
                                modifier |= output.is_modifier();
                                self.holding.push(output);
                            }
                            Output::Wheel(direction) => out.push(wheel(direction)),
                        }
                    }
                }
                Step::Release(outputs) => {
                    for output in outputs.iter().rev() {
                        if let Output::Hold(output) = *output
                            && let Some(i) = self.holding.iter().rposition(|&h| h == output)
                        {
                            self.holding.remove(i);
                            held.release(output, out);
                        }
                    }
                }
                Step::Wait(duration) => {
                    self.next = (self.next + *duration).max(now);
                    if !duration.is_zero() && self.next <= now {
                        break;
                    }
                }
            }
        }
        self.index >= self.steps.len()
    }

    pub(crate) fn next_deadline(&self) -> Option<Duration> {
        (self.index < self.steps.len()).then_some(self.next)
    }

    pub(crate) fn stop(&mut self, held: &mut HeldOutputs, out: &mut Vec<OutputAction>) {
        for output in self.holding.drain(..).rev() {
            held.release(output, out);
        }
    }
}
