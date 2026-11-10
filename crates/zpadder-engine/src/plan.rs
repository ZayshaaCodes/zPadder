//! Compiles an assignment's slot list into the segments the engine runs.
//!
//! Semantics (Xpadder's, made precise; see `docs/profiles.md`):
//!
//! - Zone markers split the list into segments. Slots before the first marker form the press
//!   segment, which starts at once.
//! - `hold T` starts a held segment `T` after the press. Held segments are exclusive: entering
//!   one ends the previous one.
//! - `release T` starts a release segment. On release, the last release segment whose `T` is
//!   not more than the hold time fires as a tap.
//! - `distance P%` starts a segment for analog controls; the segment whose range contains the
//!   current deflection is active. Only its first group of outputs is used.
//! - A segment containing `pause` or `tap_time` is a sequence: groups of outputs between them
//!   are tapped one after another and the sequence runs to completion on its own.
//! - `cycle` splits the list into alternatives used on successive presses.

use std::time::Duration;

use zpadder_model::profile::{Slot, WheelDirection};

use crate::held::Held;

/// How long a tap holds its keys when no `tap_time` says otherwise (Xpadder's default).
pub(crate) const DEFAULT_TAP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Output {
    Hold(Held),
    /// One wheel notch, fired when engaged.
    Wheel(WheelDirection),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Body {
    /// Outputs held while the segment is active.
    Hold(Vec<Output>),
    /// Steps that run to completion on their own.
    Sequence(Vec<Step>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    Press(Vec<Output>),
    Release(Vec<Output>),
    Wait(Duration),
}

/// One alternative of an assignment, ready to run.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Plan {
    /// Segments active while the control is held, by start time after the press. The first
    /// starts at zero.
    pub(crate) held: Vec<(Duration, Body)>,
    /// Segments that fire on release, by minimum hold time. Always sequences.
    pub(crate) release: Vec<(Duration, Body)>,
    /// For analog controls: outputs by the deflection (0–1) where they start. Empty unless the
    /// list has distance zones, in which case `held` and `release` are empty.
    pub(crate) distance: Vec<(f32, Vec<Output>)>,
}

/// Compiles a slot list into one plan per cycle alternative.
pub(crate) fn compile(slots: &[Slot]) -> Vec<Plan> {
    slots.split(|slot| matches!(slot, Slot::CycleStop)).map(compile_alternative).collect()
}

enum Marker {
    Held(Duration),
    Release(Duration),
    Distance(f32),
}

fn compile_alternative(slots: &[Slot]) -> Plan {
    let mut segments: Vec<(Marker, Vec<Slot>)> = vec![(Marker::Held(Duration::ZERO), Vec::new())];
    for &slot in slots {
        match slot {
            Slot::HoldZone(start) => segments.push((Marker::Held(start), Vec::new())),
            Slot::ReleaseZone(start) => segments.push((Marker::Release(start), Vec::new())),
            Slot::DistanceZone(percent) => {
                segments.push((Marker::Distance(percent / 100.0), Vec::new()));
            }
            other => segments.last_mut().expect("never empty").1.push(other),
        }
    }

    let analog = segments.iter().any(|(marker, _)| matches!(marker, Marker::Distance(_)));
    let mut tap = DEFAULT_TAP;
    let mut plan = Plan::default();
    for (marker, slots) in segments {
        match marker {
            Marker::Held(_) if analog => plan.distance.push((0.0, first_group(&slots))),
            Marker::Held(start) => plan.held.push((start, body(&slots, &mut tap, false))),
            Marker::Release(start) => plan.release.push((start, body(&slots, &mut tap, true))),
            Marker::Distance(from) => plan.distance.push((from, first_group(&slots))),
        }
    }
    plan.held.sort_by_key(|(start, _)| *start);
    plan.release.sort_by_key(|(start, _)| *start);
    plan.distance.sort_by(|a, b| a.0.total_cmp(&b.0));
    plan
}

/// A segment's outputs, held while the segment is active, unless it contains pauses or tap
/// times (or fires on release), in which case it is tapped as a sequence.
fn body(slots: &[Slot], tap: &mut Duration, on_release: bool) -> Body {
    let sequence = slots.iter().any(|s| matches!(s, Slot::Pause(_) | Slot::TapTime(_)));
    if !sequence && !on_release {
        return Body::Hold(slots.iter().filter_map(output).collect());
    }
    let mut steps = Vec::new();
    let mut group = Vec::new();
    for slot in slots {
        match *slot {
            Slot::Pause(duration) => {
                flush(&mut group, *tap, &mut steps);
                steps.push(Step::Wait(duration));
            }
            Slot::TapTime(duration) => {
                flush(&mut group, *tap, &mut steps);
                *tap = duration;
            }
            _ => group.extend(output(slot)),
        }
    }
    flush(&mut group, *tap, &mut steps);
    Body::Sequence(steps)
}

/// Taps the outputs collected so far: press together, hold for `tap`, release together.
fn flush(group: &mut Vec<Output>, tap: Duration, steps: &mut Vec<Step>) {
    if group.is_empty() {
        return;
    }
    steps.push(Step::Press(group.clone()));
    steps.push(Step::Wait(tap));
    steps.push(Step::Release(std::mem::take(group)));
}

/// The outputs before the first pause or tap time.
fn first_group(slots: &[Slot]) -> Vec<Output> {
    slots
        .iter()
        .take_while(|s| !matches!(s, Slot::Pause(_) | Slot::TapTime(_)))
        .filter_map(output)
        .collect()
}

fn output(slot: &Slot) -> Option<Output> {
    match *slot {
        Slot::Key(key) => Some(Output::Hold(Held::Key(key))),
        Slot::MouseButton(button) => Some(Output::Hold(Held::Mouse(button))),
        Slot::Wheel(direction) => Some(Output::Wheel(direction)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use zpadder_model::KeyCode;

    use super::*;

    fn slots(text: &str) -> Vec<Slot> {
        text.split(',').map(|s| s.parse().unwrap()).collect()
    }

    fn key(key: KeyCode) -> Output {
        Output::Hold(Held::Key(key))
    }

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn plain_list_is_one_held_segment() {
        let plans = compile(&slots("left_shift, x"));
        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].held,
            [(Duration::ZERO, Body::Hold(vec![key(KeyCode::LEFT_SHIFT), key(KeyCode::X)]))]
        );
    }

    #[test]
    fn pauses_make_a_sequence_of_taps() {
        let plan = &compile(&slots("a, b, pause 0.2s, c"))[0];
        let ab = vec![key(KeyCode::A), key(KeyCode::B)];
        let c = vec![key(KeyCode::C)];
        assert_eq!(
            plan.held[0].1,
            Body::Sequence(vec![
                Step::Press(ab.clone()),
                Step::Wait(DEFAULT_TAP),
                Step::Release(ab),
                Step::Wait(MS(200)),
                Step::Press(c.clone()),
                Step::Wait(DEFAULT_TAP),
                Step::Release(c),
            ])
        );
    }

    #[test]
    fn tap_or_hold() {
        let plan = &compile(&slots("hold 0.5s, y, release 0.01s, x, release 0.25s"))[0];
        assert_eq!(plan.held.len(), 2);
        assert_eq!(plan.held[0], (Duration::ZERO, Body::Hold(vec![])));
        assert_eq!(plan.held[1], (MS(500), Body::Hold(vec![key(KeyCode::Y)])));
        assert_eq!(plan.release.len(), 2);
        assert_eq!(plan.release[1], (MS(250), Body::Sequence(vec![])));
    }

    #[test]
    fn distance_zones_and_cycles() {
        let plans = compile(&slots("a, distance 50%, b, pause 0.1s, c, cycle, d"));
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].distance, [(0.0, vec![key(KeyCode::A)]), (0.5, vec![key(KeyCode::B)])]);
        assert!(plans[0].held.is_empty());
        assert_eq!(plans[1].held, [(Duration::ZERO, Body::Hold(vec![key(KeyCode::D)]))]);
    }
}
