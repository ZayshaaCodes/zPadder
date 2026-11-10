//! A recovery copy of unsaved edits, so they survive Windows shutting down, a crash or a killed
//! process.
//!
//! While the profile has unsaved changes the app writes it to `recovery\unsaved.toml` in the
//! data folder, a moment after each edit. Saving, discarding or opening another profile removes
//! the copy; one still there at the next start means edits were lost, and the app offers them
//! back.
//!
//! winit, under eframe, does not pass on Windows' end-of-session messages, so catching shutdown
//! itself would mean subclassing the window. A copy kept up to date covers shutdown too, and
//! crashes and power cuts as well, with no window plumbing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zpadder_model::Profile;

/// Seconds after the last edit before the copy is written.
pub(crate) const DELAY_SECONDS: f64 = 1.5;

/// What is kept: the edited profile and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Recovered {
    pub(crate) profile: Profile,
    /// The ZPadder file being edited, if it had one.
    pub(crate) path: Option<PathBuf>,
    /// The Xpadder file it was imported from, if it was.
    pub(crate) imported_from: Option<PathBuf>,
}

/// The file as stored: the profile in its usual TOML form, as text, beside the paths.
#[derive(Serialize, Deserialize)]
struct Stored {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    imported_from: Option<PathBuf>,
    profile: String,
}

/// Where the copy goes inside the data folder.
pub(crate) fn file_in(data_dir: &Path) -> PathBuf {
    data_dir.join("recovery").join("unsaved.toml")
}

/// Writes the copy. Paths that are not valid Unicode are left out rather than failing.
pub(crate) fn write(file: &Path, recovered: &Recovered) -> Result<(), String> {
    let unicode = |path: &Option<PathBuf>| path.clone().filter(|p| p.to_str().is_some());
    let stored = Stored {
        path: unicode(&recovered.path),
        imported_from: unicode(&recovered.imported_from),
        profile: zpadder_config::to_string(&recovered.profile),
    };
    let text = toml::to_string_pretty(&stored).map_err(|e| e.to_string())?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    zpadder_config::write_atomic(file, text.as_bytes())
        .map_err(|e| format!("{}: {e}", file.display()))
}

/// Reads the copy, if there is one. A copy that cannot be read is reported as an error.
pub(crate) fn read(file: &Path) -> Option<Result<Recovered, String>> {
    let bytes = std::fs::read(file).ok()?;
    let parsed = String::from_utf8(bytes)
        .map_err(|e| e.to_string())
        .and_then(|text| toml::from_str::<Stored>(&text).map_err(|e| e.to_string()))
        .and_then(|stored| {
            // Unsaved edits can be half done: problems are fine, they are fixed after restoring.
            let (profile, _) = crate::files::parse_for_editing(&stored.profile)?;
            Ok(Recovered { profile, path: stored.path, imported_from: stored.imported_from })
        });
    Some(parsed)
}

/// Removes the copy, if there is one.
pub(crate) fn remove(file: &Path) {
    match std::fs::remove_file(file) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("Cannot remove {}: {e}", file.display()),
    }
}

#[cfg(test)]
mod tests {
    use zpadder_model::profile::Layer;

    use super::*;

    #[test]
    fn a_copy_round_trips_and_can_be_removed() {
        let dir = std::env::temp_dir().join(format!("zpadder-recovery-{}", std::process::id()));
        let file = file_in(&dir);
        assert!(read(&file).is_none());
        let recovered = Recovered {
            profile: Profile {
                name: "Racing".into(),
                layout: None,
                virtual_pad: None,
                layers: vec![Layer::new("Main")],
            },
            path: Some(dir.join("racing.toml")),
            imported_from: None,
        };
        write(&file, &recovered).unwrap();
        assert_eq!(read(&file), Some(Ok(recovered)));
        remove(&file);
        assert!(read(&file).is_none());
        // Removing it twice is fine.
        remove(&file);
        std::fs::write(&file, "not a recovery file").unwrap();
        assert!(matches!(read(&file), Some(Err(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
