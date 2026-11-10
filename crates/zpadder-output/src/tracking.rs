//! A sink wrapper that remembers what it holds down, so it can always let go.

use zpadder_model::{KeyCode, MouseButton, OutputAction};

use crate::{OutputSink, SendError};

/// A key or mouse button, which a sink can hold down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Held {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl Held {
    /// What `action` presses or releases, and whether it presses it.
    fn of(action: OutputAction) -> Option<(Held, bool)> {
        match action {
            OutputAction::KeyDown(key) => Some((Held::Key(key), true)),
            OutputAction::KeyUp(key) => Some((Held::Key(key), false)),
            OutputAction::MouseButton { button, pressed } => Some((Held::Mouse(button), pressed)),
            OutputAction::MouseMove { .. } | OutputAction::Wheel { .. } => None,
        }
    }

    fn action(self, pressed: bool) -> OutputAction {
        match self {
            Held::Key(key) if pressed => OutputAction::KeyDown(key),
            Held::Key(key) => OutputAction::KeyUp(key),
            Held::Mouse(button) => OutputAction::MouseButton { button, pressed },
        }
    }
}

/// Marks `held` as down or up in `set`, which keeps things in the order they went down.
fn mark(set: &mut Vec<Held>, held: Held, down: bool) {
    match (set.iter().position(|&h| h == held), down) {
        (None, true) => set.push(held),
        (Some(i), false) => {
            set.remove(i);
        }
        _ => {}
    }
}

/// Wraps a sink and keeps its own record of every key and mouse button it has sent down and not
/// yet released.
///
/// - [`TrackingSink::release_held`] lets go of all of them from that record alone, so nothing
///   stays stuck down even when whatever produced the outputs (the engine) can no longer be
///   trusted. Dropping the sink does the same.
/// - A failed send loses no release. The sink also remembers what the caller has asked to hold,
///   and the next send, or [`TrackingSink::retry`], first brings Windows in line with it:
///   releasing what should be up but is still down, then pressing what should be down but never
///   went down. Pointer and wheel movement that failed is dropped, and a press and release that
///   both failed before the next attempt cancel out, rather than arrive late as a tap.
/// - The release of something that never went down, because its press failed, is not sent.
#[derive(Debug)]
pub struct TrackingSink<S: OutputSink> {
    sink: S,
    /// Down in Windows: sent down and not yet released, oldest first.
    down: Vec<Held>,
    /// Down as far as the caller is concerned: every press and release it asked for, whether
    /// or not it went out, oldest first.
    wanted: Vec<Held>,
    /// A send failed, so `down` may not match `wanted`.
    behind: bool,
    /// The batch being sent, and what will be down if all of it goes out. Kept to reuse their
    /// allocations.
    batch: Vec<OutputAction>,
    after: Vec<Held>,
}

impl<S: OutputSink> TrackingSink<S> {
    pub fn new(sink: S) -> Self {
        Self {
            sink,
            down: Vec::new(),
            wanted: Vec::new(),
            behind: false,
            batch: Vec::new(),
            after: Vec::new(),
        }
    }

    pub fn inner(&self) -> &S {
        &self.sink
    }

    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.sink
    }

    /// The keys and mouse buttons this sink holds down, oldest first.
    pub fn held(&self) -> &[Held] {
        &self.down
    }

    /// Whether a send failed and Windows may not match what was asked for yet. The next send or
    /// [`TrackingSink::retry`] catches up.
    pub fn needs_retry(&self) -> bool {
        self.behind
    }

    /// Catches up after a failed send. Does nothing if none failed.
    pub fn retry(&mut self) -> Result<(), SendError> {
        self.send(&[])
    }

    /// Lets go of every key and mouse button this sink holds down, newest first, using its own
    /// record. What the caller asked to hold is forgotten, so no retry presses anything again. If
    /// this fails, what is still down is released by the next retry or send.
    pub fn release_held(&mut self) -> Result<(), SendError> {
        self.wanted.clear();
        self.behind = true;
        self.retry()
    }
}

impl<S: OutputSink> OutputSink for TrackingSink<S> {
    fn send(&mut self, actions: &[OutputAction]) -> Result<(), SendError> {
        self.batch.clear();
        self.after.clear();
        self.after.extend_from_slice(&self.down);
        if self.behind {
            // Catch up first, so the new actions apply to the state the caller expects.
            for &held in self.down.iter().rev() {
                if !self.wanted.contains(&held) {
                    self.batch.push(held.action(false));
                    mark(&mut self.after, held, false);
                }
            }
            for &held in &self.wanted {
                if !self.down.contains(&held) {
                    self.batch.push(held.action(true));
                    mark(&mut self.after, held, true);
                }
            }
        }
        for &action in actions {
            if let Some((held, pressed)) = Held::of(action) {
                mark(&mut self.wanted, held, pressed);
                if !pressed && !self.after.contains(&held) {
                    // Its press never went out: there is nothing to release.
                    continue;
                }
                mark(&mut self.after, held, pressed);
            }
            self.batch.push(action);
        }
        if self.batch.is_empty() {
            // Nothing to catch up on, so Windows matches what was asked for.
            self.behind = false;
            return Ok(());
        }
        match self.sink.send(&self.batch) {
            Ok(()) => {
                std::mem::swap(&mut self.down, &mut self.after);
                self.behind = false;
                Ok(())
            }
            Err(error) => {
                for &action in &self.batch[..error.sent.min(self.batch.len())] {
                    if let Some((held, pressed)) = Held::of(action) {
                        mark(&mut self.down, held, pressed);
                    }
                }
                self.behind = true;
                Err(error)
            }
        }
    }
}

impl<S: OutputSink> Drop for TrackingSink<S> {
    fn drop(&mut self) {
        // The last chance to let go: nothing retries after this.
        if !self.down.is_empty() {
            let _ = self.release_held();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use OutputAction::{KeyDown, KeyUp};

    use super::*;
    use crate::RecordingSink;

    const A: KeyCode = KeyCode::A;
    const B: KeyCode = KeyCode::B;

    fn click(pressed: bool) -> OutputAction {
        OutputAction::MouseButton { button: MouseButton::Left, pressed }
    }

    fn sink() -> TrackingSink<RecordingSink> {
        TrackingSink::new(RecordingSink::default())
    }

    /// Everything that reached "Windows" since the last call.
    fn sent(sink: &mut TrackingSink<RecordingSink>) -> Vec<OutputAction> {
        std::mem::take(&mut sink.inner_mut().actions)
    }

    /// Makes the wrapped sink refuse everything (`Some(0)`), some, or nothing (`None`).
    fn accept(sink: &mut TrackingSink<RecordingSink>, count: Option<usize>) {
        sink.inner_mut().accept = count;
    }

    #[test]
    fn it_lets_go_of_what_it_holds_from_its_own_record() {
        let mut sink = sink();
        sink.send(&[KeyDown(A), click(true), KeyDown(B), KeyUp(A)]).unwrap();
        assert_eq!(sink.held(), [Held::Mouse(MouseButton::Left), Held::Key(B)]);
        sent(&mut sink);
        sink.release_held().unwrap();
        assert_eq!(sent(&mut sink), [KeyUp(B), click(false)], "newest first");
        assert!(sink.held().is_empty());
        sink.release_held().unwrap();
        assert!(sent(&mut sink).is_empty(), "nothing left to release");
    }

    #[test]
    fn dropping_it_lets_go_too() {
        #[derive(Default)]
        struct Shared(Rc<RefCell<Vec<OutputAction>>>);
        impl OutputSink for Shared {
            fn send(&mut self, actions: &[OutputAction]) -> Result<(), SendError> {
                self.0.borrow_mut().extend_from_slice(actions);
                Ok(())
            }
        }
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut sink = TrackingSink::new(Shared(Rc::clone(&log)));
        sink.send(&[KeyDown(A), click(true)]).unwrap();
        drop(sink);
        assert_eq!(log.borrow().as_slice(), [KeyDown(A), click(true), click(false), KeyUp(A)]);
    }

    #[test]
    fn a_failed_release_is_retried_until_it_goes_out() {
        let mut sink = sink();
        sink.send(&[KeyDown(A)]).unwrap();
        sent(&mut sink);
        accept(&mut sink, Some(0));
        assert!(sink.send(&[KeyUp(A)]).is_err());
        assert!(sink.retry().is_err());
        assert_eq!(sink.held(), [Held::Key(A)]);
        assert!(sink.needs_retry());
        accept(&mut sink, None);
        sink.retry().unwrap();
        assert_eq!(sent(&mut sink), [KeyUp(A)]);
        assert!(sink.held().is_empty() && !sink.needs_retry());
    }

    #[test]
    fn a_failed_release_goes_out_before_the_next_press() {
        let mut sink = sink();
        sink.send(&[KeyDown(A)]).unwrap();
        accept(&mut sink, Some(0));
        assert!(sink.send(&[KeyUp(A)]).is_err());
        accept(&mut sink, None);
        sent(&mut sink);
        sink.send(&[KeyDown(A)]).unwrap();
        assert_eq!(sent(&mut sink), [KeyUp(A), KeyDown(A)]);
    }

    #[test]
    fn a_partial_send_keeps_what_went_out_and_presses_the_rest_later() {
        let mut sink = sink();
        accept(&mut sink, Some(1));
        let error = sink.send(&[KeyDown(A), KeyDown(B)]).unwrap_err();
        assert_eq!(error.sent, 1);
        assert_eq!(sink.held(), [Held::Key(A)]);
        accept(&mut sink, None);
        sink.retry().unwrap();
        assert_eq!(sent(&mut sink), [KeyDown(A), KeyDown(B)]);
        assert_eq!(sink.held(), [Held::Key(A), Held::Key(B)]);
    }

    #[test]
    fn a_tap_that_never_went_out_is_dropped_whole() {
        let mut sink = sink();
        accept(&mut sink, Some(0));
        assert!(
            sink.send(&[KeyDown(A), KeyUp(A), OutputAction::MouseMove { dx: 4, dy: 0 }]).is_err()
        );
        accept(&mut sink, None);
        sink.retry().unwrap();
        assert!(sent(&mut sink).is_empty(), "no late tap, no stray release, no stale movement");
        assert!(!sink.needs_retry());
    }

    #[test]
    fn a_release_whose_press_never_went_out_is_not_sent() {
        let mut sink = sink();
        sink.send(&[KeyUp(A), click(false)]).unwrap();
        assert!(sent(&mut sink).is_empty());
    }
}
