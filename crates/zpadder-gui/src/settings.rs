//! App settings, kept in `settings.toml` in ZPadder's data folder: next to the executable when a
//! `portable.txt` file is there, otherwise `%APPDATA%\ZPadder`.
//!
//! A settings file that cannot be read is not overwritten: it is kept as `settings.toml.bad` and
//! the app starts with the defaults, saying so.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 8;
const FILE_NAME: &str = "settings.toml";
const BAD_FILE_NAME: &str = "settings.toml.bad";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    /// The profile opened at start.
    pub(crate) profile: Option<PathBuf>,
    pub(crate) recent: Vec<PathBuf>,
    /// The folder a profile was last opened from or saved to, where file dialogs start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_folder: Option<PathBuf>,
    pub(crate) enabled: bool,
    /// Closing the window keeps ZPadder running in the tray.
    pub(crate) close_to_tray: bool,
    pub(crate) start_minimized: bool,
    /// Show paddles and other buttons only some controllers have.
    pub(crate) show_all_buttons: bool,
    /// Label controls with assignment names instead of keys.
    pub(crate) show_names: bool,
    /// Show a photo of the connected controller for profiles that do not choose a picture.
    pub(crate) auto_photo: bool,
    /// Show the callout sheet (labels beside the controller) instead of labels on it.
    pub(crate) callouts: bool,
    /// Show the test bench below the controller.
    pub(crate) show_bench: bool,
    /// The user has dealt with Steam taking over ZPadder's Xbox controller, so the reminder about
    /// it stays hidden.
    pub(crate) steam_xbox_hint_hidden: bool,
    pub(crate) auto_profiles: Vec<AutoRule>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profile: None,
            recent: Vec::new(),
            last_folder: None,
            enabled: true,
            close_to_tray: true,
            start_minimized: false,
            show_all_buttons: false,
            show_names: false,
            auto_photo: true,
            callouts: false,
            show_bench: false,
            steam_xbox_hint_hidden: false,
            auto_profiles: Vec::new(),
        }
    }
}

/// Load `profile` while `program` (and optionally a window title) has focus.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AutoRule {
    pub(crate) program: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) title_contains: String,
    pub(crate) profile: PathBuf,
}

/// Settings as loaded, and what the user should hear about how that went.
#[derive(Debug)]
pub(crate) struct Loaded {
    pub(crate) settings: Settings,
    /// Set when the file could not be read and the defaults are used instead.
    pub(crate) notice: Option<String>,
}

impl Settings {
    /// Loads the settings from the data folder, or the defaults when there are none yet.
    pub(crate) fn load() -> Loaded {
        match data_dir() {
            Some(dir) => Self::load_from(&dir),
            None => Loaded { settings: Self::default(), notice: None },
        }
    }

    /// Loads `settings.toml` from `dir`. A file that cannot be read is renamed to
    /// `settings.toml.bad` (replacing an older one), so saving the defaults does not destroy it.
    pub(crate) fn load_from(dir: &Path) -> Loaded {
        let path = dir.join(FILE_NAME);
        let text = match std::fs::read(&path) {
            Ok(bytes) => String::from_utf8(bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Loaded { settings: Self::default(), notice: None };
            }
            Err(e) => Err(e.to_string()),
        };
        let error = match text.and_then(|text| toml::from_str(&text).map_err(|e| e.to_string())) {
            Ok(settings) => return Loaded { settings, notice: None },
            Err(e) => e,
        };
        tracing::warn!("Cannot read {}: {error}", path.display());
        let bad = dir.join(BAD_FILE_NAME);
        let notice = match std::fs::rename(&path, &bad) {
            Ok(()) => format!(
                "Your settings could not be read and were reset. The old file is {BAD_FILE_NAME}."
            ),
            Err(e) => {
                tracing::warn!("Cannot rename {} to {}: {e}", path.display(), bad.display());
                "Your settings could not be read and were reset.".to_string()
            }
        };
        Loaded { settings: Self::default(), notice: Some(notice) }
    }

    /// Saves the settings to the data folder. Returns what went wrong, if anything.
    pub(crate) fn save(&self) -> Result<(), String> {
        match data_dir() {
            Some(dir) => self.save_to(&dir),
            None => Ok(()),
        }
    }

    /// Saves `settings.toml` into `dir`, replacing the old file only once the new one is
    /// complete. Paths that are not valid Unicode cannot be written to TOML; they are left out,
    /// with a warning in the log.
    pub(crate) fn save_to(&self, dir: &Path) -> Result<(), String> {
        let text = toml::to_string_pretty(&self.unicode_only())
            .map_err(|e| format!("Cannot save settings: {e}"))?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Cannot save settings: {e}"))?;
        let path = dir.join(FILE_NAME);
        zpadder_config::write_atomic(&path, text.as_bytes())
            .map_err(|e| format!("Cannot save settings to {}: {e}", path.display()))
    }

    /// A copy without the paths TOML cannot hold.
    fn unicode_only(&self) -> Self {
        let keep = |path: &Path| {
            let ok = path.to_str().is_some();
            if !ok {
                tracing::warn!("Not saving {} in the settings: not valid Unicode", path.display());
            }
            ok
        };
        let mut copy = self.clone();
        copy.profile = copy.profile.filter(|p| keep(p));
        copy.last_folder = copy.last_folder.filter(|p| keep(p));
        copy.recent.retain(|p| keep(p));
        copy.auto_profiles.retain(|rule| keep(&rule.profile));
        copy
    }

    pub(crate) fn add_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_owned());
        self.recent.truncate(MAX_RECENT);
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            self.last_folder = Some(dir.to_owned());
        }
    }
}

/// Where ZPadder keeps its own files: `ZPADDER_DATA_DIR` if set, beside the executable in
/// portable mode, otherwise `%APPDATA%\ZPadder`. See [`zpadder_platform::data_dir`].
pub(crate) fn data_dir() -> Option<PathBuf> {
    zpadder_platform::data_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("zpadder-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn settings_round_trip() {
        let dir = temp_dir("round-trip");
        let loaded = Settings::load_from(&dir);
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.notice.is_none());

        let mut settings = Settings { show_bench: true, ..Settings::default() };
        settings.add_recent(&dir.join("game.toml"));
        settings.save_to(&dir).unwrap();
        let loaded = Settings::load_from(&dir);
        assert_eq!(loaded.settings, settings);
        assert_eq!(loaded.settings.last_folder.as_deref(), Some(dir.as_path()));
        assert!(loaded.notice.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unreadable_settings_are_kept_aside_not_overwritten() {
        let dir = temp_dir("bad");
        std::fs::write(dir.join(FILE_NAME), "enabled = \"not a bool\"").unwrap();
        // An older bad file is replaced.
        std::fs::write(dir.join(BAD_FILE_NAME), "older").unwrap();
        let loaded = Settings::load_from(&dir);
        assert_eq!(loaded.settings, Settings::default());
        let notice = loaded.notice.unwrap();
        assert!(notice.contains("settings.toml.bad"), "{notice}");
        assert!(!dir.join(FILE_NAME).exists());
        let kept = std::fs::read_to_string(dir.join(BAD_FILE_NAME)).unwrap();
        assert_eq!(kept, "enabled = \"not a bool\"");

        // Saving now writes a fresh file and leaves the bad one alone.
        loaded.settings.save_to(&dir).unwrap();
        assert!(Settings::load_from(&dir).notice.is_none());
        assert!(dir.join(BAD_FILE_NAME).exists());

        // Text that is not UTF-8 counts as unreadable too.
        std::fs::write(dir.join(FILE_NAME), [0xff, 0xfe, 0x00]).unwrap();
        assert!(Settings::load_from(&dir).notice.is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn paths_that_are_not_unicode_are_left_out() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        let dir = temp_dir("unicode");
        // An unpaired surrogate: a valid Windows file name, but not Unicode.
        let odd = PathBuf::from(OsString::from_wide(&[0x61, 0xD800, 0x2E, 0x74]));
        let mut settings = Settings::default();
        settings.add_recent(&odd);
        settings.add_recent(&dir.join("fine.toml"));
        settings.profile = Some(odd.clone());
        settings.auto_profiles.push(AutoRule { profile: odd, ..AutoRule::default() });
        settings.save_to(&dir).unwrap();
        let loaded = Settings::load_from(&dir).settings;
        assert_eq!(loaded.recent, [dir.join("fine.toml")]);
        assert_eq!(loaded.profile, None);
        assert!(loaded.auto_profiles.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
