//! Undo and redo for the edited profile, as snapshots of the whole profile.
//!
//! The editor changes the profile in place, so instead of recording commands the history looks at
//! the profile once a frame ([`History::observe`]) and keeps the version from before each edit.
//! Edits close together are one step: typing a name, or dragging a slider, undoes in one go. A
//! new step starts with each click or drag ([`History::interaction`]) and after a pause.

use zpadder_model::Profile;

/// Edits less than this many seconds apart, without a click in between, are one undo step.
pub(crate) const COALESCE_SECONDS: f64 = 0.5;
/// How many undo steps are kept.
pub(crate) const CAPACITY: usize = 100;

#[derive(Debug)]
pub(crate) struct History {
    /// Versions to go back to, oldest first.
    undo: Vec<Profile>,
    /// Versions undone, most recently undone last.
    redo: Vec<Profile>,
    /// The profile as last observed.
    last: Profile,
    /// When the last edit was seen, while it may still grow; `None` once the next edit must
    /// start a new step.
    last_edit: Option<f64>,
    /// A click or drag started since the last edit, so the next edit is a new step.
    interacted: bool,
}

impl History {
    pub(crate) fn new(profile: &Profile) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            last: profile.clone(),
            last_edit: None,
            interacted: false,
        }
    }

    /// Forgets every step, for a profile that was just opened or created.
    pub(crate) fn reset(&mut self, profile: &Profile) {
        *self = Self::new(profile);
    }

    /// A click or drag started: the next edit is a step of its own.
    pub(crate) fn interaction(&mut self) {
        self.interacted = true;
    }

    /// Ends the current step, so the next edit starts a new one. For edits made by commands
    /// rather than by widgets, such as deleting a layer.
    pub(crate) fn break_step(&mut self) {
        self.last_edit = None;
    }

    /// Looks at the profile at time `now` (in seconds). If it changed since the last look and the
    /// change starts a new step, the version from before it is kept for undo. `held` says the
    /// pointer is down, as in a drag: a pause then does not start a new step.
    pub(crate) fn observe(&mut self, profile: &Profile, now: f64, held: bool) {
        if *profile == self.last {
            return;
        }
        let paused = self.last_edit.is_none_or(|last| now - last >= COALESCE_SECONDS);
        if self.interacted || (paused && !held) || self.last_edit.is_none() {
            let before = std::mem::replace(&mut self.last, profile.clone());
            self.push_undo(before);
        } else {
            self.last = profile.clone();
        }
        self.redo.clear();
        self.last_edit = Some(now);
        self.interacted = false;
    }

    fn push_undo(&mut self, profile: Profile) {
        self.undo.push(profile);
        if self.undo.len() > CAPACITY {
            self.undo.remove(0);
        }
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The version before the last step, which `current` is to be replaced with.
    pub(crate) fn undo(&mut self, current: &Profile) -> Option<Profile> {
        let previous = self.undo.pop()?;
        self.redo.push(current.clone());
        self.last = previous.clone();
        self.last_edit = None;
        Some(previous)
    }

    /// The version the last undo went back from.
    pub(crate) fn redo(&mut self, current: &Profile) -> Option<Profile> {
        let next = self.redo.pop()?;
        self.push_undo(current.clone());
        self.last = next.clone();
        self.last_edit = None;
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> Profile {
        Profile { name: name.into(), layout: None, virtual_pad: None, layers: Vec::new() }
    }

    #[test]
    fn typing_is_one_step() {
        let mut history = History::new(&named(""));
        for (i, name) in ["J", "Ju", "Jum", "Jump"].into_iter().enumerate() {
            history.observe(&named(name), i as f64 * 0.2, false);
        }
        assert_eq!(history.undo(&named("Jump")), Some(named("")));
        assert!(!history.can_undo());
        assert_eq!(history.redo(&named("")), Some(named("Jump")));
        assert!(!history.can_redo());
    }

    #[test]
    fn a_pause_or_a_click_starts_a_new_step() {
        let mut history = History::new(&named("a"));
        history.observe(&named("ab"), 0.0, false);
        // After a pause.
        history.observe(&named("abc"), 2.0, false);
        // Right after a click.
        history.interaction();
        history.observe(&named("abcd"), 2.1, false);
        assert_eq!(history.undo(&named("abcd")), Some(named("abc")));
        assert_eq!(history.undo(&named("abc")), Some(named("ab")));
        assert_eq!(history.undo(&named("ab")), Some(named("a")));
        assert_eq!(history.undo(&named("a")), None);
    }

    #[test]
    fn a_drag_with_pauses_is_one_step() {
        let mut history = History::new(&named("0"));
        history.interaction();
        for (i, value) in ["1", "2", "3"].into_iter().enumerate() {
            history.observe(&named(value), i as f64 * 2.0, true);
        }
        assert_eq!(history.undo(&named("3")), Some(named("0")));
        assert!(!history.can_undo());
    }

    #[test]
    fn unchanged_frames_and_undone_steps_are_not_recorded() {
        let mut history = History::new(&named("a"));
        history.observe(&named("a"), 0.0, false);
        assert!(!history.can_undo());
        history.observe(&named("b"), 1.0, false);
        let back = history.undo(&named("b")).unwrap();
        // Seeing the undone version is not an edit, so redo stays possible.
        history.observe(&back, 1.1, false);
        assert!(history.can_redo());
        // A new edit after an undo starts its own step and drops what was undone.
        history.observe(&named("c"), 1.2, false);
        assert!(!history.can_redo());
        assert_eq!(history.undo(&named("c")), Some(named("a")));
    }

    #[test]
    fn breaking_a_step_and_the_capacity() {
        let mut history = History::new(&named("a"));
        history.observe(&named("b"), 0.0, false);
        history.break_step();
        history.observe(&named("c"), 0.1, false);
        assert_eq!(history.undo(&named("c")), Some(named("b")));

        let mut history = History::new(&named("0"));
        for i in 1..=CAPACITY + 10 {
            history.observe(&named(&i.to_string()), i as f64, false);
        }
        let mut steps = 0;
        let mut current = named(&(CAPACITY + 10).to_string());
        while let Some(previous) = history.undo(&current) {
            current = previous;
            steps += 1;
        }
        assert_eq!(steps, CAPACITY);
        assert_eq!(current, named("10"));
    }
}
