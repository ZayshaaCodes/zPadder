//! Virtual Xbox 360 controllers through the ViGEmBus driver, with the `vigem-client` crate.
//!
//! ViGEmBus is a separate, signed driver the user installs. Without it, connecting fails with
//! [`Fault::DriverMissing`] and nothing else happens. A controller is unplugged when its
//! [`ViGEmTarget`] is dropped; one left behind by a killed process can be removed in Device
//! Manager, but ZPadder unplugs them on every way out it controls, panics included.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use tracing::{debug, warn};
use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};
use zpadder_model::{PadId, XboxState};

use crate::Rumble;
use crate::virtual_pad::{Backend, Fault, Target};

/// The ViGEmBus driver. Rumble that games send to a controller goes to `rumble`, for the
/// physical pad the controller stands in for.
#[derive(Debug)]
pub(crate) struct ViGEm {
    client: Option<Arc<Client>>,
    rumble: Sender<Rumble>,
}

impl ViGEm {
    pub(crate) fn new(rumble: Sender<Rumble>) -> Self {
        Self { client: None, rumble }
    }
}

impl Backend for ViGEm {
    type Target = ViGEmTarget;

    fn connect(&mut self) -> Result<(), Fault> {
        let client = Client::connect().map_err(fault)?;
        self.client = Some(Arc::new(client));
        Ok(())
    }

    fn disconnect(&mut self) {
        self.client = None;
    }

    fn plug(&mut self, pad: PadId) -> Result<ViGEmTarget, Fault> {
        let client = self.client.clone().ok_or(Fault::DriverMissing)?;
        let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
        target.plugin().map_err(fault)?;
        // Dropping `target` on an error unplugs it again.
        target.wait_ready().map_err(fault)?;
        let alive = Arc::new(AtomicBool::new(true));
        match target.request_notification() {
            Ok(request) => {
                let rumble = self.rumble.clone();
                let alive = Arc::clone(&alive);
                // The thread ends when the controller is unplugged, which aborts the request.
                // A notification arriving after that belongs to whatever controller has taken
                // over the driver's serial number, so `alive` keeps it from this pad.
                drop(request.spawn_thread(move |_, notification| {
                    if alive.load(Ordering::SeqCst) {
                        let _ = rumble.send(Rumble::Game {
                            pad,
                            large: notification.large_motor,
                            small: notification.small_motor,
                        });
                    }
                }));
            }
            Err(e) => warn!("Rumble from games will not reach pad {}: {}", pad.0, describe(e)),
        }
        Ok(ViGEmTarget { target, pad, alive, rumble: self.rumble.clone() })
    }
}

/// A virtual Xbox 360 controller that is plugged in.
pub(crate) struct ViGEmTarget {
    target: Xbox360Wired<Arc<Client>>,
    pad: PadId,
    /// Cleared when unplugged, so late rumble notifications are dropped.
    alive: Arc<AtomicBool>,
    rumble: Sender<Rumble>,
}

impl std::fmt::Debug for ViGEmTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViGEmTarget").field("pad", &self.pad).finish_non_exhaustive()
    }
}

impl Target for ViGEmTarget {
    fn user_index(&mut self) -> Option<u8> {
        match self.target.get_user_index() {
            Ok(index) => u8::try_from(index).ok().filter(|&index| index < 4),
            Err(e) => {
                debug!("No XInput user index for pad {} yet: {e}", self.pad.0);
                None
            }
        }
    }

    fn update(&mut self, state: &XboxState) -> Result<(), Fault> {
        self.target.update(&gamepad(state)).map_err(fault)
    }
}

impl Drop for ViGEmTarget {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Err(e) = self.target.unplug() {
            warn!("Unplugging the Xbox controller for pad {} failed: {}", self.pad.0, describe(e));
        }
        // A game may have left it rumbling.
        let _ = self.rumble.send(Rumble::Game { pad: self.pad, large: 0, small: 0 });
    }
}

/// XInput's report, field for field.
fn gamepad(state: &XboxState) -> XGamepad {
    XGamepad {
        buttons: XButtons(state.buttons),
        left_trigger: state.left_trigger,
        right_trigger: state.right_trigger,
        thumb_lx: state.left_x,
        thumb_ly: state.left_y,
        thumb_rx: state.right_x,
        thumb_ry: state.right_y,
    }
}

fn fault(error: vigem_client::Error) -> Fault {
    match error {
        vigem_client::Error::BusNotFound => Fault::DriverMissing,
        other => Fault::Failed(describe(other)),
    }
}

/// A ViGEm error in plain words.
fn describe(error: vigem_client::Error) -> String {
    use vigem_client::Error as E;
    match error {
        E::BusNotFound => "the ViGEmBus driver is not installed".into(),
        E::BusAccessFailed(code) => format!(
            "ZPadder cannot open the ViGEmBus driver (Windows error {code}). Restart Windows if \
             you have just installed it, or reinstall it."
        ),
        E::BusVersionMismatch => {
            "the installed ViGEmBus driver is a version ZPadder cannot use. Install the latest \
             ViGEmBus."
                .into()
        }
        E::NoFreeSlot => "ViGEmBus has no room for another controller.".into(),
        E::TargetNotReady => "the Xbox controller is not ready yet.".into(),
        E::NotPluggedIn => "the Xbox controller is not plugged in.".into(),
        E::WinError(code) => format!("ViGEmBus reported Windows error {code}."),
        other => format!("ViGEmBus reported an error: {other}."),
    }
}
