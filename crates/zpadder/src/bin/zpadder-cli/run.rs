//! `zpadder run`: the headless mapper. Ctrl+C, Ctrl+Break and closing the console window all
//! release every held key before exiting.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tracing::{error, info, warn};
use zpadder_model::Profile;
use zpadder_platform::console::console_stop;
use zpadder_platform::instance;
use zpadder_runtime::Mapper;

/// How long closing the console window waits for every key to be released. Windows ends the
/// process after five seconds whatever happens.
const CLOSE_GRACE: Duration = Duration::from_secs(4);

pub(crate) fn run(path: &Path) -> ExitCode {
    let profile = match zpadder_config::load_any(path) {
        Ok(loaded) => {
            for warning in &loaded.warnings {
                info!("Import note: {warning}");
            }
            loaded.profile
        }
        Err(e) => {
            error!("{}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    if instance::is_running() {
        warn!(
            "ZPadder is running too and maps the controller as well, so programs may get each \
             press twice. Switch its mapping off, or quit it, while zpadder-cli run is in use."
        );
    }
    info!("Profile \"{}\" loaded. Press Ctrl+C to stop.", profile.name);

    let stop = match console_stop(CLOSE_GRACE) {
        Ok(stop) => stop,
        Err(e) => {
            error!("Cannot handle Ctrl+C: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mapper = Mapper::start(|| {});
    mapper.set_profile(Some(Arc::new(profile)));
    let mut watch = ProfileWatch::new(path);
    let code = loop {
        if stop.wait(ProfileWatch::INTERVAL) {
            break ExitCode::SUCCESS;
        }
        if let Some(error) = mapper.status().error {
            error!("{error}");
            break ExitCode::FAILURE;
        }
        if let Some(profile) = watch.reload() {
            info!("Profile \"{}\" reloaded", profile.name);
            mapper.set_profile(Some(Arc::new(profile)));
        }
    };
    mapper.shutdown();
    // Every key is released: a console window being closed may now let the process end.
    stop.finished();
    code
}

/// Reloads the profile when its file changes. A change is only acted on once the modification
/// time has stayed the same across two checks, so a half-written file is not read.
struct ProfileWatch {
    path: PathBuf,
    loaded: Option<SystemTime>,
    pending: Option<SystemTime>,
    last_check: Instant,
}

impl ProfileWatch {
    const INTERVAL: Duration = Duration::from_millis(300);

    fn new(path: &Path) -> Self {
        let modified = modified(path);
        Self {
            path: path.to_owned(),
            loaded: modified,
            pending: modified,
            last_check: Instant::now(),
        }
    }

    fn reload(&mut self) -> Option<Profile> {
        if self.last_check.elapsed() < Self::INTERVAL {
            return None;
        }
        self.last_check = Instant::now();
        let now = modified(&self.path);
        if now == self.loaded || now != std::mem::replace(&mut self.pending, now) {
            return None;
        }
        self.loaded = now;
        match zpadder_config::load_any(&self.path) {
            Ok(loaded) => Some(loaded.profile),
            Err(e) => {
                warn!("Not reloading {}: {e}", self.path.display());
                None
            }
        }
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}
