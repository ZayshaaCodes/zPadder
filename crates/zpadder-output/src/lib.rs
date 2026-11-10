//! Keyboard and mouse output sinks.
//!
//! On Windows, [`SendInputSink`] injects events with `SendInput` using scan codes, one batched
//! call per engine tick. [`TrackingSink`] wraps any sink to keep its own record of the keys and
//! mouse buttons it holds down, so they can always be let go, even if whatever produced the
//! outputs has failed, and so a release that Windows refused is retried rather than lost. Linux
//! (uinput) and macOS (CGEvent) sinks come in Phase 6 of `docs/roadmap.md`.

pub mod scancode;
#[cfg(windows)]
mod sendinput;
mod tracking;

use std::fmt;

#[cfg(windows)]
pub use sendinput::{EXTRA_INFO_TAG, SendInputSink};
use zpadder_model::OutputAction;

pub use crate::tracking::{Held, TrackingSink};

/// A destination for synthesized keyboard and mouse events.
pub trait OutputSink {
    /// Injects `actions` in order, as a single batch where the platform supports it.
    ///
    /// On failure, [`SendError::sent`] says how many of the leading actions went out in full.
    /// None of the others did, except perhaps part of the first one after them (a wheel action
    /// can take two events).
    fn send(&mut self, actions: &[OutputAction]) -> Result<(), SendError>;
}

/// Some or all of a batch did not go out.
#[derive(Debug)]
pub struct SendError {
    /// How many of the batch's leading actions went out in full.
    pub sent: usize,
    pub error: std::io::Error,
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.sent == 0 {
            write!(f, "{}", self.error)
        } else {
            write!(f, "{} (only the first {} actions went out)", self.error, self.sent)
        }
    }
}

impl std::error::Error for SendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Records every action instead of injecting it. Used to test the engine end to end.
#[derive(Debug, Default)]
pub struct RecordingSink {
    pub actions: Vec<OutputAction>,
    /// When set, only this many more actions are accepted; the rest of a batch that goes past it
    /// fails, as if Windows had refused it. For testing what happens when sending fails.
    pub accept: Option<usize>,
}

impl OutputSink for RecordingSink {
    fn send(&mut self, actions: &[OutputAction]) -> Result<(), SendError> {
        let Some(accept) = &mut self.accept else {
            self.actions.extend_from_slice(actions);
            return Ok(());
        };
        let sent = actions.len().min(*accept);
        *accept -= sent;
        self.actions.extend_from_slice(&actions[..sent]);
        if sent == actions.len() {
            Ok(())
        } else {
            let error = std::io::Error::other("refused for the test");
            Err(SendError { sent, error })
        }
    }
}
