//! Controller input backends.
//!
//! The default backend, [`sdl::SdlBackend`], wraps SDL3 with background events enabled, so input
//! keeps flowing while a game has focus. A replay backend for tests is planned (Phase 1 in
//! `docs/roadmap.md`).

pub mod sdl;

use std::time::Duration;

use zpadder_model::InputEvent;

/// A source of controller input events.
pub trait InputBackend {
    /// Waits up to `timeout` for input, then appends any events received to `out`. With a zero
    /// `timeout` it only collects what has already arrived.
    fn poll(&mut self, timeout: Duration, out: &mut Vec<InputEvent>);
}
