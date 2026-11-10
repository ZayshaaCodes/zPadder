//! Core data model shared by every ZPadder crate.
//!
//! This crate must stay free of OS dependencies: it is built and tested on Linux CI to keep the
//! core portable.

mod input;
pub mod layout;
mod output;
pub mod profile;
pub mod xbox;

pub use input::{Axis, Button, InputEvent, PadId};
pub use layout::{Control, Layout};
pub use output::{KeyCode, MouseButton, OutputAction};
pub use profile::Profile;
pub use xbox::{VirtualPad, XboxState};
