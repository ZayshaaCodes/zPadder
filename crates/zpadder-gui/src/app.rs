//! The main window: menu, layer bar, controller view, inspector and status bar.
//!
//! eframe calls [`App::logic`](eframe::App::logic) for every pass, even while the window is
//! minimized or hidden in the tray, and [`App::ui`](eframe::App::ui) only when the window is
//! drawn. So everything that must work from the tray happens in `logic`: closing, commands from
//! the tray and from later launches, handing the profile to the mapper, and keeping the
//! recovery copy. `ui` draws, and does next to nothing while the window is hidden.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText, Ui, ViewportCommand};
use zpadder_model::profile::{Direction, Layer, MIN_TRIGGER_THRESHOLD, Profile, StickMode};
use zpadder_model::{Button, VirtualPad};
use zpadder_platform::Autostart;
use zpadder_runtime::{AutoProfile, Mapper, Power, Status, VirtualPadStatus};

use crate::Launch;
use crate::bench::{Bench, Running};
use crate::builtin;
use crate::callout;
use crate::files;
use crate::history::History;
use crate::inspector::{self, EditorState};
use crate::pad_view::{self, Family, Part, Side, ViewState};
use crate::picture::{self, Picture};
use crate::recovery::{self, Recovered};
use crate::settings::{self, AutoRule, Settings};
use crate::theme;
#[cfg(windows)]
use crate::tray::{Tray, TrayCommand};

/// How often the mapper may wake the app while its window is hidden. It wakes it on every
/// change otherwise, which during play is about 60 times a second.
const HIDDEN_WAKE: Duration = Duration::from_secs(1);
/// How long after the last edit of an AutoProfile rule it is applied.
const RULE_EDIT_DELAY: Duration = Duration::from_millis(600);
/// How many profile problems the status bar lists before summing up the rest.
const MAX_ERRORS_SHOWN: usize = 4;
/// Where ViGEmBus, the driver "Act as an Xbox controller" needs, is downloaded from.
const VIGEMBUS_URL: &str = "https://github.com/nefarius/ViGEmBus/releases";
/// The hover text of "Act as an Xbox controller".
const XBOX_HINT: &str = "Games see an Xbox controller. Controls you haven't assigned in this \
                         layer act as its buttons, sticks and triggers. Needs the ViGEmBus driver.";

/// The running mapper, which can be started again after an internal error stopped it.
struct MapperHandle(Option<Mapper>);

impl MapperHandle {
    /// Starts a mapper that wakes the app on every change while the window is `shown`, and only
    /// now and then while it is not.
    fn start(ctx: &egui::Context, shown: &Arc<AtomicBool>) -> Self {
        Self(Some(Self::new_mapper(ctx, shown)))
    }

    fn new_mapper(ctx: &egui::Context, shown: &Arc<AtomicBool>) -> Mapper {
        let ctx = ctx.clone();
        let shown = Arc::clone(shown);
        Mapper::start(move || {
            if shown.load(Ordering::Relaxed) {
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(HIDDEN_WAKE);
            }
        })
    }

    /// Stops the mapper, then starts a new one. Only one can read the controllers at a time, so
    /// the old one goes first.
    fn restart(&mut self, ctx: &egui::Context, shown: &Arc<AtomicBool>) {
        drop(self.0.take());
        self.0 = Some(Self::new_mapper(ctx, shown));
    }
}

impl std::ops::Deref for MapperHandle {
    type Target = Mapper;

    fn deref(&self) -> &Mapper {
        // Empty only inside `restart`.
        self.0.as_ref().expect("the mapper is only missing while it restarts")
    }
}

/// `arg` quoted for a Windows command line, so it reaches the program as one argument whatever
/// it holds: backslashes before a quote, or at the end, are doubled, and quotes escaped.
fn quote_argument(arg: &str) -> String {
    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            other => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push(other);
                backslashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

/// What to do once unsaved changes have been dealt with.
#[derive(Debug, Clone)]
enum Pending {
    New,
    Open(Option<PathBuf>),
    Quit,
    /// Start a new profile from a built-in template.
    Template(usize),
    /// Restart as administrator, then quit this instance.
    Elevate,
}

impl Pending {
    /// What the unsaved-changes question says is about to happen.
    fn doing(&self) -> &'static str {
        match self {
            Pending::New => "starting a new profile",
            Pending::Open(_) => "opening another profile",
            Pending::Quit => "quitting",
            Pending::Template(_) => "starting from a template",
            Pending::Elevate => "restarting as administrator",
        }
    }
}

pub(crate) struct App {
    settings: Settings,
    mapper: MapperHandle,
    profile: Profile,
    /// The profile as last saved or opened; unsaved changes are the difference.
    saved: Profile,
    path: Option<PathBuf>,
    /// The profile as last checked and (if valid) sent to the mapper.
    applied: Option<Profile>,
    /// The last valid version of the profile, as the mapper has it.
    live: Option<Arc<Profile>>,
    errors: Vec<String>,
    layer: usize,
    selected: Option<Part>,
    direction: Direction,
    editor: EditorState,
    click_editor: EditorState,
    bench: Bench,
    history: History,
    /// When the app started, for timing edits whether or not the window is shown.
    started: Instant,
    pending: Option<Pending>,
    notice: Option<String>,
    /// A problem with the settings file: it could not be read, or not saved.
    settings_notice: Option<String>,
    quitting: bool,
    /// "Don't save" was chosen for quitting: close although there are unsaved changes.
    discard_on_quit: bool,
    title: String,
    show_auto: bool,
    /// The Xpadder file the current profile was imported from (it has no ZPadder file yet).
    imported_from: Option<PathBuf>,
    import_notes: Vec<String>,
    show_import_notes: bool,
    /// Xpadder files whose import notes have opened by themselves this session.
    notes_shown: HashSet<PathBuf>,
    /// The controller picture for the profile's layout, if it has one.
    picture: Option<Picture>,
    /// The layout path a picture failed to load from, so it is not retried every frame.
    picture_failed: Option<PathBuf>,
    auto_errors: Vec<String>,
    /// The AutoProfile rules' profiles as last read from their files.
    auto_loaded: Vec<(PathBuf, Arc<Profile>)>,
    /// When an AutoProfile rule was last edited, while that edit is not applied yet.
    auto_edited: Option<Instant>,
    recent_programs: Vec<String>,
    last_pressed: BTreeSet<Button>,
    /// Command lines handed over by later launches.
    remote: Option<Receiver<Vec<String>>>,
    #[cfg(windows)]
    tray: Option<Tray>,
    /// The window is on screen (not hidden in the tray, not minimized). Shared with the mapper's
    /// callback, which wakes the app less often while it is not.
    shown: Arc<AtomicBool>,
    /// The app hid the window in the tray.
    hidden: bool,
    /// The layer name being typed, committed when the field is left.
    layer_name: String,
    /// The layer `layer_name` belongs to; `None` once it needs reading from the profile again.
    layer_name_for: Option<usize>,
    /// The layer whose deletion is being asked about.
    confirm_delete: Option<usize>,
    /// Where the recovery copy of unsaved edits goes, if there is a data folder.
    recovery_file: Option<PathBuf>,
    /// Unsaved edits from a session that ended without saving them, offered back.
    recovery_offer: Option<Recovered>,
    /// The profile as last written to the recovery copy this session.
    recovery_written: Option<Profile>,
    /// When the recovery copy is next due to be written.
    recovery_due: Option<Instant>,
}

fn new_profile() -> Profile {
    Profile {
        name: "New profile".into(),
        layout: None,
        virtual_pad: None,
        layers: vec![Layer::new("Main")],
    }
}

/// Whether import notes say more than which button order was used, which every import notes.
fn has_real_warnings(notes: &[String]) -> bool {
    notes.iter().any(|note| !note.to_lowercase().contains("button order"))
}

impl App {
    pub(crate) fn new(
        cc: &eframe::CreationContext<'_>,
        launch: Launch,
        loaded: settings::Loaded,
        hidden: bool,
    ) -> Self {
        let shown = Arc::new(AtomicBool::new(!hidden));
        let mapper = MapperHandle::start(&cc.egui_ctx, &shown);
        let settings = loaded.settings;
        mapper.set_enabled(settings.enabled);
        let mut app = Self {
            mapper,
            profile: new_profile(),
            saved: new_profile(),
            path: None,
            applied: None,
            live: None,
            errors: Vec::new(),
            layer: 0,
            selected: None,
            direction: Direction::Up,
            editor: EditorState::default(),
            click_editor: EditorState::default(),
            bench: Bench::default(),
            history: History::new(&new_profile()),
            started: Instant::now(),
            pending: None,
            notice: None,
            settings_notice: loaded.notice,
            quitting: false,
            discard_on_quit: false,
            title: String::new(),
            show_auto: false,
            imported_from: None,
            import_notes: Vec::new(),
            show_import_notes: false,
            notes_shown: HashSet::new(),
            picture: None,
            picture_failed: None,
            auto_errors: Vec::new(),
            auto_loaded: Vec::new(),
            auto_edited: None,
            recent_programs: Vec::new(),
            last_pressed: BTreeSet::new(),
            remote: None,
            #[cfg(windows)]
            tray: None,
            shown,
            hidden: false,
            layer_name: String::new(),
            layer_name_for: None,
            confirm_delete: None,
            recovery_file: settings::data_dir().map(|dir| recovery::file_in(&dir)),
            recovery_offer: None,
            recovery_written: None,
            recovery_due: None,
            settings,
        };
        // Edits a previous session did not save. Read before opening anything, which would
        // otherwise count as dealing with them.
        if let Some(file) = &app.recovery_file {
            match recovery::read(file) {
                Some(Ok(recovered)) => app.recovery_offer = Some(recovered),
                Some(Err(e)) => {
                    tracing::warn!("Ignoring unreadable {}: {e}", file.display());
                    recovery::remove(file);
                }
                None => {}
            }
        }
        if let Some(path) = launch.profile.or_else(|| app.settings.profile.clone()) {
            app.open_path(&path);
        }
        if let Some(commands) = launch.remote {
            // Forward handed-over command lines and wake the app, even while it is hidden.
            let (tx, rx) = mpsc::channel();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                for command in commands {
                    if tx.send(command).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
            app.remote = Some(rx);
        }
        app.load_auto_profiles();
        // Development aid: preselect a control, e.g. ZPADDER_DEBUG_SELECT=south, to look at the
        // inspector without clicking.
        #[cfg(debug_assertions)]
        if let Some(button) =
            std::env::var("ZPADDER_DEBUG_SELECT").ok().as_deref().and_then(Button::from_name)
        {
            app.selected = Some(Part::Button(button));
        }
        #[cfg(windows)]
        {
            app.tray = match Tray::new(&cc.egui_ctx, app.settings.enabled) {
                Ok(tray) => Some(tray),
                Err(e) => {
                    tracing::warn!("No tray icon: {e}");
                    None
                }
            };
        }
        if hidden {
            if app.has_tray() {
                // eframe shows the window once it has drawn its first frame, whatever it was
                // built with; this hides it again straight after.
                app.hide_window(&cc.egui_ctx);
            } else {
                cc.egui_ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            }
        }
        app
    }

    fn has_tray(&self) -> bool {
        #[cfg(windows)]
        return self.tray.is_some();
        #[cfg(not(windows))]
        false
    }

    fn dirty(&self) -> bool {
        self.profile != self.saved
    }

    /// Seconds since the app started, for the undo history.
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn save_settings(&mut self) {
        match self.settings.save() {
            Ok(()) => {
                if self.settings_notice.as_deref().is_some_and(|n| n.starts_with("Cannot save")) {
                    self.settings_notice = None;
                }
            }
            Err(e) => {
                tracing::warn!("{e}");
                self.settings_notice = Some(e);
            }
        }
    }

    // ---- Selection ----

    /// Changes what the inspector edits. Every change goes through here, so an editor that was
    /// picking or capturing keys for the old selection stops.
    fn set_selected(&mut self, part: Option<Part>) {
        if self.selected != part {
            self.commit_layer_name();
            self.selected = part;
            self.reset_editors();
        }
    }

    fn select(&mut self, part: Part) {
        self.set_selected(Some(part));
    }

    /// Changes which stick direction the inspector edits.
    fn set_direction(&mut self, direction: Direction) {
        if self.direction != direction {
            self.direction = direction;
            self.editor = EditorState::default();
        }
    }

    fn set_layer(&mut self, layer: usize) {
        if self.layer != layer {
            self.commit_layer_name();
            self.layer = layer;
            self.reset_editors();
        }
    }

    fn reset_editors(&mut self) {
        self.editor = EditorState::default();
        self.click_editor = EditorState::default();
    }

    /// Renames the layer to the name being typed, if that is a name it can have. Called when
    /// the name field is left, and before anything that could take the field away.
    fn commit_layer_name(&mut self) {
        let Some(index) = self.layer_name_for.take() else { return };
        let Some(layer) = self.profile.layers.get(index) else { return };
        if self.layer_name.trim() == layer.name {
            return;
        }
        match inspector::layer_name_problem(&self.profile, index, &self.layer_name) {
            None => {
                self.history.break_step();
                inspector::rename_layer(&mut self.profile, index, &self.layer_name);
            }
            Some(problem) => {
                self.notice = Some(format!("Layer \"{}\" keeps its name. {problem}", layer.name));
            }
        }
    }

    /// Moves the current layer to `to`, next to it.
    fn move_layer(&mut self, to: usize) {
        self.commit_layer_name();
        self.profile.layers.swap(self.layer, to);
        self.layer = to;
    }

    // ---- Files ----

    /// Opens a profile, or imports an Xpadder one. Returns whether it opened; if not, the
    /// profile being edited stays as it was.
    fn open_path(&mut self, path: &Path) -> bool {
        // A path from the command line may be relative to wherever ZPadder was started from.
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        // A profile with problems opens too, so they can be fixed; the status bar lists them.
        let (loaded, problems) = match files::load_for_editing(&path) {
            Ok(loaded) => loaded,
            Err(e) => {
                self.notice = Some(format!("Cannot open {}: {e}", path.display()));
                return false;
            }
        };
        let mut profile = loaded.profile;
        let mut notes = loaded.warnings;
        let (file, imported_from) = if loaded.imported {
            // Show the Xpadder controller picture when the pack has one.
            if let Some(controller) = picture::controller_beside(&path) {
                match picture::import_controller(&controller) {
                    Ok((layout, more)) => {
                        profile.layout = Some(layout.display().to_string());
                        notes.extend(more);
                    }
                    Err(e) => notes.push(format!("Picture not imported: {e}")),
                }
            }
            (None, Some(path.clone()))
        } else {
            (Some(path.clone()), None)
        };
        // The notes open by themselves only when they say something worth reading, and only
        // the first time a file is opened.
        let show_notes =
            loaded.imported && has_real_warnings(&notes) && self.notes_shown.insert(path.clone());
        self.replace_profile(profile, file, imported_from);
        self.import_notes = notes;
        self.show_import_notes = show_notes;
        self.settings.profile = Some(path.clone());
        self.settings.add_recent(&path);
        self.save_settings();
        self.notice = (!problems.is_empty()).then(|| {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            format!(
                "{name} has problems, listed above. The pad does nothing until they are fixed; \
                 fix them here and save."
            )
        });
        true
    }

    /// Makes `profile` the one being edited, as just opened or created: it counts as saved, the
    /// undo history starts afresh and the previous profile's recovery copy goes.
    fn replace_profile(
        &mut self,
        profile: Profile,
        path: Option<PathBuf>,
        imported_from: Option<PathBuf>,
    ) {
        // A layer name being typed belonged to the old profile.
        self.layer_name_for = None;
        self.saved = profile.clone();
        self.history.reset(&profile);
        self.profile = profile;
        self.path = path;
        self.imported_from = imported_from;
        self.import_notes.clear();
        self.show_import_notes = false;
        self.layer = 0;
        self.set_selected(None);
        self.reset_editors();
        self.confirm_delete = None;
        self.drop_recovery_copy();
        // The mapper gets the new profile now, or nothing if it has problems: the previous
        // profile's last valid version must not stay in use under the new one's name.
        self.applied = None;
        self.live = None;
        self.apply();
        if self.live.is_none() {
            self.mapper.set_profile(None);
        }
        self.push_auto_rules();
    }

    /// Saves to the current file, asking for one if needed. Returns whether it saved.
    fn save(&mut self) -> bool {
        self.commit_layer_name();
        let Some(path) = self.path.clone() else { return self.save_as() };
        self.save_to(&path, self.profile.clone())
    }

    /// Writes `profile` to `path` and makes it the saved version of the profile being edited.
    fn save_to(&mut self, path: &Path, profile: Profile) -> bool {
        if let Err(e) = zpadder_config::save(path, &profile) {
            self.notice = Some(format!("Cannot save {}: {e}", path.display()));
            return false;
        }
        if profile != self.profile {
            // Only the layout path moved with the file; that is not an edit to undo.
            self.history.reset(&profile);
        }
        self.profile = profile;
        self.saved = self.profile.clone();
        self.path = Some(path.to_owned());
        self.imported_from = None;
        self.settings.profile = Some(path.to_owned());
        self.settings.add_recent(path);
        self.save_settings();
        self.notice = None;
        self.drop_recovery_copy();
        // Rules that use this file get the saved version.
        self.load_auto_profiles();
        true
    }

    /// Asks for a file and saves there. A relative layout path is rewritten so it still finds
    /// the layout from the new folder.
    fn save_as(&mut self) -> bool {
        let name = format!("{}.toml", files::safe_file_stem(&self.profile.name, "Profile"));
        let mut dialog =
            rfd::FileDialog::new().add_filter("ZPadder profile", &["toml"]).set_file_name(name);
        if let Some(folder) = self.dialog_folder() {
            dialog = dialog.set_directory(folder);
        }
        let Some(chosen) = dialog.save_file() else { return false };
        let path = files::with_toml_extension(chosen);
        let mut profile = self.profile.clone();
        if let (Some(layout), Some(new_dir)) = (&profile.layout, path.parent()) {
            let old_dir =
                self.path.as_deref().or(self.imported_from.as_deref()).and_then(Path::parent);
            profile.layout = Some(files::rebase_layout(layout, old_dir, new_dir));
        }
        self.save_to(&path, profile)
    }

    /// Where file dialogs start: the current file's folder, or the one used last.
    fn dialog_folder(&self) -> Option<PathBuf> {
        self.path
            .as_deref()
            .or(self.imported_from.as_deref())
            .and_then(Path::parent)
            .map(Path::to_owned)
            .or_else(|| self.settings.last_folder.clone())
            .filter(|folder| folder.is_dir())
    }

    /// Goes back to the profile as last saved. Undo brings the edits back.
    fn revert(&mut self) {
        self.history.break_step();
        self.profile = self.saved.clone();
        self.after_profile_swap();
    }

    fn undo(&mut self) {
        if let Some(previous) = self.history.undo(&self.profile) {
            self.profile = previous;
            self.after_profile_swap();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.history.redo(&self.profile) {
            self.profile = next;
            self.after_profile_swap();
        }
    }

    /// Keeps the view valid after the profile was swapped for another version of itself.
    fn after_profile_swap(&mut self) {
        self.layer_name_for = None;
        if self.layer >= self.profile.layers.len() {
            self.set_layer(0);
        }
        self.confirm_delete = None;
    }

    fn request(&mut self, ctx: &egui::Context, action: Pending) {
        if self.dirty() {
            self.ask(ctx, action);
        } else {
            self.perform(ctx, action, false);
        }
    }

    /// Asks what to do about unsaved changes before `action`, showing the window if needed.
    fn ask(&mut self, ctx: &egui::Context, action: Pending) {
        self.show_window(ctx);
        self.pending = Some(action);
        // Keys go to the question: Enter saves, Escape cancels.
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }

    /// Does `action`. `discard` means the user chose not to save: a quit then goes ahead with
    /// the changes unsaved. Opening, new profiles and templates replace the profile only once
    /// they have succeeded, so a cancelled or failed open keeps the edits.
    fn perform(&mut self, ctx: &egui::Context, action: Pending, discard: bool) {
        match action {
            Pending::New => {
                self.replace_profile(new_profile(), None, None);
                self.settings.profile = None;
                self.save_settings();
            }
            Pending::Open(Some(path)) => {
                self.open_path(&path);
            }
            Pending::Template(index) => {
                let template = &builtin::TEMPLATES[index];
                match zpadder_config::parse(template.toml) {
                    Ok(profile) => self.replace_profile(profile, None, None),
                    Err(e) => self.notice = Some(format!("Template {}: {e}", template.name)),
                }
            }
            Pending::Open(None) => {
                let mut dialog =
                    rfd::FileDialog::new().add_filter("Profiles", &["toml", "xpadderprofile"]);
                if let Some(folder) = self.dialog_folder() {
                    dialog = dialog.set_directory(folder);
                }
                if let Some(path) = dialog.pick_file() {
                    self.open_path(&path);
                }
            }
            Pending::Quit => {
                self.quitting = true;
                self.discard_on_quit = discard;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Pending::Elevate => {
                let mut args = "--after-restart".to_string();
                if let Some(path) = self.path.as_deref().and_then(Path::to_str) {
                    args.push(' ');
                    args.push_str(&quote_argument(path));
                }
                match zpadder_platform::restart_elevated(&args) {
                    Ok(()) => self.perform(ctx, Pending::Quit, discard),
                    Err(e) => self.notice = Some(format!("Cannot restart as administrator: {e}")),
                }
            }
        }
    }

    /// Acts on a command line handed over by a later launch, the way Xpadder does: profile paths
    /// open, `/C` quits, `/M` keeps the window hidden, and nothing at all shows the window.
    fn handle_remote(&mut self, ctx: &egui::Context, args: Vec<String>) {
        let flag = |name: &str| args.iter().any(|arg| arg.eq_ignore_ascii_case(name));
        if flag("/C") {
            self.request(ctx, Pending::Quit);
            return;
        }
        let profile = args.iter().find(|arg| !arg.starts_with('/')).map(PathBuf::from);
        if !flag("/M") {
            self.show_window(ctx);
        }
        if let Some(path) = profile {
            self.request(ctx, Pending::Open(Some(path)));
        }
    }

    /// Checks the edited profile and, when it is valid, hands it to the mapper so edits take
    /// effect at once.
    fn apply(&mut self) {
        if self.applied.as_ref() == Some(&self.profile) {
            return;
        }
        match self.profile.validate() {
            Ok(()) => {
                let profile = Arc::new(self.profile.clone());
                self.mapper.set_profile(Some(Arc::clone(&profile)));
                self.live = Some(profile);
                self.errors.clear();
                // A rule for the file being edited uses the edits too.
                if self
                    .settings
                    .auto_profiles
                    .iter()
                    .any(|rule| Some(&rule.profile) == self.path.as_ref())
                {
                    self.push_auto_rules();
                }
            }
            Err(errors) => self.errors = errors,
        }
        self.applied = Some(self.profile.clone());
    }

    /// Reads the AutoProfile rules' profiles from their files and hands the rules to the
    /// mapper.
    fn load_auto_profiles(&mut self) {
        self.auto_errors.clear();
        self.auto_loaded.clear();
        for rule in &self.settings.auto_profiles {
            let path = &rule.profile;
            if rule.program.trim().is_empty()
                || path.as_os_str().is_empty()
                || self.auto_loaded.iter().any(|(loaded, _)| loaded == path)
            {
                continue;
            }
            match zpadder_config::load_any(path) {
                Ok(loaded) => self.auto_loaded.push((path.clone(), Arc::new(loaded.profile))),
                Err(e) => self.auto_errors.push(format!("{}: {e}", path.display())),
            }
        }
        self.push_auto_rules();
    }

    /// Hands the rules to the mapper. A rule for the file open in the editor uses the profile
    /// as edited (its last valid version), so edits apply there at once as well.
    fn push_auto_rules(&mut self) {
        let rules = self
            .settings
            .auto_profiles
            .iter()
            .filter(|rule| !rule.program.trim().is_empty())
            .filter_map(|rule| {
                let edited = (self.path.as_ref() == Some(&rule.profile))
                    .then(|| self.live.clone())
                    .flatten();
                let from_file = || {
                    self.auto_loaded
                        .iter()
                        .find(|(path, _)| *path == rule.profile)
                        .map(|(_, profile)| Arc::clone(profile))
                };
                Some(AutoProfile {
                    program: rule.program.trim().to_string(),
                    title_contains: Some(rule.title_contains.trim().to_string())
                        .filter(|text| !text.is_empty()),
                    profile: edited.or_else(from_file)?,
                })
            })
            .collect();
        self.mapper.set_auto_profiles(rules);
    }

    /// Saves and applies AutoProfile rule edits.
    fn apply_rule_edits(&mut self) {
        self.auto_edited = None;
        self.save_settings();
        self.load_auto_profiles();
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.settings.enabled = enabled;
        self.save_settings();
        self.mapper.set_enabled(enabled);
        #[cfg(windows)]
        if let Some(tray) = &self.tray {
            tray.set_enabled(enabled);
        }
    }

    /// Starts mapping again after an internal error stopped it, with the same profile, rules
    /// and switch.
    fn restart_mapping(&mut self, ctx: &egui::Context) {
        self.mapper.restart(ctx, &self.shown);
        self.bench.mapper_restarted();
        self.mapper.set_enabled(self.settings.enabled);
        self.applied = None;
        self.live = None;
        self.apply();
        self.push_auto_rules();
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        self.hidden = false;
        self.shown.store(true, Ordering::Relaxed);
    }

    /// Hides the window in the tray.
    fn hide_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        self.hidden = true;
        self.shown.store(false, Ordering::Relaxed);
        // No more edits until it is shown again: bring the recovery copy up to date now.
        if self.recovery_due.is_some() {
            self.recovery_due = Some(Instant::now());
            self.update_recovery(ctx);
        }
    }

    /// Notes whether the window is on screen, for the mapper's callback.
    fn track_shown(&mut self, ctx: &egui::Context) {
        let minimized = ctx.input(|i| i.viewport().minimized) == Some(true);
        self.shown.store(!self.hidden && !minimized, Ordering::Relaxed);
    }

    // ---- Recovery ----

    /// Keeps the recovery copy in step with unsaved edits: written a moment after an edit,
    /// removed once there is nothing unsaved.
    fn update_recovery(&mut self, ctx: &egui::Context) {
        let Some(file) = self.recovery_file.clone() else { return };
        if self.recovery_offer.is_some() {
            // The copy on disk is the previous session's until the user decides about it.
            return;
        }
        if !self.dirty() {
            if self.recovery_written.take().is_some() {
                recovery::remove(&file);
            }
            self.recovery_due = None;
            return;
        }
        if self.recovery_written.as_ref() == Some(&self.profile) {
            self.recovery_due = None;
            return;
        }
        let now = Instant::now();
        let delay = Duration::from_secs_f64(recovery::DELAY_SECONDS);
        let due = *self.recovery_due.get_or_insert(now + delay);
        if now < due {
            ctx.request_repaint_after(due - now);
            return;
        }
        self.recovery_due = None;
        let recovered = Recovered {
            profile: self.profile.clone(),
            path: self.path.clone(),
            imported_from: self.imported_from.clone(),
        };
        match recovery::write(&file, &recovered) {
            Ok(()) => self.recovery_written = Some(recovered.profile),
            Err(e) => tracing::warn!("Cannot write the recovery copy: {e}"),
        }
    }

    /// Removes this session's recovery copy: the edits were saved or deliberately dropped.
    fn drop_recovery_copy(&mut self) {
        self.recovery_due = None;
        if self.recovery_offer.is_none()
            && self.recovery_written.take().is_some()
            && let Some(file) = &self.recovery_file
        {
            recovery::remove(file);
        }
    }

    /// Puts back edits a previous session did not save: opens the file they belong to, so Save
    /// goes there, and then applies them as unsaved changes.
    fn restore(&mut self, recovered: Recovered) {
        let source = recovered.path.as_ref().or(recovered.imported_from.as_ref());
        let opened = source.is_some_and(|source| self.open_path(source));
        if !opened {
            self.replace_profile(Profile { name: String::new(), ..new_profile() }, None, None);
        }
        self.history.break_step();
        self.profile = recovered.profile;
        self.after_profile_swap();
        // Write this session's copy straight away.
        self.recovery_due = Some(Instant::now());
    }

    // ---- Pictures ----

    /// Where the profile's layout file is: as written if absolute, otherwise beside the profile.
    fn layout_path(&self) -> Option<PathBuf> {
        let layout_value = self.profile.layout.as_ref()?;
        if layout_value.starts_with(builtin::PREFIX) {
            return Some(PathBuf::from(layout_value));
        }
        let layout = PathBuf::from(layout_value);
        if layout.is_absolute() {
            return Some(layout);
        }
        let base = self.path.as_deref().or(self.imported_from.as_deref())?.parent()?;
        Some(base.join(layout))
    }

    /// Loads the profile's controller picture when its layout changes. Profiles without one
    /// show a photo of the connected controller, unless that is switched off.
    fn update_picture(&mut self, ctx: &egui::Context, family: Family) {
        let wanted = self.layout_path().or_else(|| {
            self.settings.auto_photo.then(|| {
                PathBuf::from(format!("{}{}", builtin::PREFIX, builtin::picture_for(family).id))
            })
        });
        let current = self.picture.as_ref().map(|p| p.source.clone());
        if wanted == current || (wanted.is_some() && wanted == self.picture_failed) {
            return;
        }
        self.picture = None;
        let Some(path) = wanted else { return };
        match Picture::load(ctx, &path) {
            Ok(picture) => {
                self.picture = Some(picture);
                self.picture_failed = None;
            }
            Err(e) => {
                self.notice = Some(format!("Controller picture not shown: {e}"));
                self.picture_failed = Some(path);
            }
        }
    }

    /// The View menu: how the controller is shown (a photo of the connected pad, the drawn
    /// controller, a specific built-in photo, or an Xpadder picture) and how it is labelled.
    fn view_menu(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Controller picture").strong());
        let layout = self.profile.layout.clone();
        if ui
            .selectable_label(layout.is_none() && self.settings.auto_photo, "Photo of my pad")
            .on_hover_text("A photo matching the connected controller")
            .clicked()
        {
            self.profile.layout = None;
            self.settings.auto_photo = true;
            self.save_settings();
        }
        if ui.selectable_label(layout.is_none() && !self.settings.auto_photo, "Drawn").clicked() {
            self.profile.layout = None;
            self.settings.auto_photo = false;
            self.save_settings();
        }
        for picture in &builtin::PICTURES {
            let id = format!("{}{}", builtin::PREFIX, picture.id);
            if ui
                .selectable_label(layout.as_deref() == Some(id.as_str()), picture.name)
                .on_hover_text("Saved with the profile")
                .clicked()
            {
                self.profile.layout = Some(id);
            }
        }
        let xpadder = layout.as_deref().is_some_and(|l| builtin::picture(l).is_none());
        if ui
            .selectable_label(xpadder, "From Xpadder file…")
            .on_hover_text("Use the picture from an .xpaddercontroller file")
            .clicked()
        {
            self.choose_picture();
            ui.close();
        }
        ui.separator();
        ui.label(RichText::new("Labels").strong());
        let mut changed = false;
        changed |=
            ui.checkbox(&mut self.settings.show_names, "Show names instead of keys").changed();
        changed |= ui
            .checkbox(&mut self.settings.show_all_buttons, "Show all buttons")
            .on_hover_text("Paddles and other buttons only some controllers have. They also appear once pressed or assigned.")
            .changed();
        ui.separator();
        ui.label(RichText::new("Layout").strong());
        changed |= ui
            .radio_value(&mut self.settings.callouts, false, "Labels on the controller")
            .changed();
        changed |= ui
            .radio_value(&mut self.settings.callouts, true, "Sheet: labels beside it")
            .on_hover_text(
                "Every label in a column beside the controller, joined to its control by a line. \
                 Nothing is shortened, and the sheet can be saved as an image.",
            )
            .changed();
        if changed {
            self.save_settings();
        }
    }

    fn choose_picture(&mut self) {
        let Some(controller) = rfd::FileDialog::new()
            .add_filter("Xpadder controller", &["xpaddercontroller"])
            .pick_file()
        else {
            return;
        };
        match picture::import_controller(&controller) {
            Ok((layout, notes)) => {
                self.profile.layout = Some(layout.display().to_string());
                if !notes.is_empty() {
                    self.import_notes = notes;
                    self.show_import_notes = true;
                }
            }
            Err(e) => self.notice = Some(e),
        }
    }

    // ---- Frame ----

    /// Handles the window being closed: into the tray, or quitting once unsaved changes are
    /// dealt with. Runs in `logic`, so it works while the window is minimized too.
    fn handle_close(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.quitting {
            if self.dirty() && !self.discard_on_quit {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.quitting = false;
                self.ask(ctx, Pending::Quit);
            }
        } else if self.settings.close_to_tray && self.has_tray() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_window(ctx);
        } else if self.dirty() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.ask(ctx, Pending::Quit);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = |path: &Path| path.file_name().map(|n| n.to_string_lossy().into_owned());
        let file = match (&self.path, &self.imported_from) {
            (Some(path), _) => name(path),
            (None, Some(source)) => name(source).map(|n| format!("{n} (imported from Xpadder)")),
            (None, None) => None,
        };
        let title = format!(
            "{}{} - ZPadder",
            if self.dirty() { "* " } else { "" },
            file.unwrap_or_else(|| "Unsaved profile".into()),
        );
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// What every pass ends with, drawn or not: the mapper gets the profile, the title and tray
    /// follow it, the undo history sees edits and the recovery copy keeps up.
    fn sync(&mut self, ctx: &egui::Context) {
        let held = ctx.input(|i| i.pointer.any_down());
        self.history.observe(&self.profile, self.now(), held && !self.hidden);
        self.apply();
        self.update_title(ctx);
        self.update_recovery(ctx);
        #[cfg(windows)]
        if let Some(tray) = &mut self.tray {
            let current = self.path.as_deref().or(self.imported_from.as_deref());
            tray.set_profiles(&self.settings.recent, current);
        }
    }

    /// Selects whatever the user presses on the real controller, so editing a control is as
    /// quick as pressing it.
    fn follow_controller(&mut self, status: &Status) {
        let Some(pad) = status.pads.first() else { return };
        if self.editor.capturing || self.click_editor.capturing {
            return;
        }
        let new: Vec<Button> = pad.pressed.difference(&self.last_pressed).copied().collect();
        self.last_pressed = pad.pressed.clone();
        for button in new {
            self.select(match button {
                Button::LeftStick => Part::Stick(Side::Left),
                Button::RightStick => Part::Stick(Side::Right),
                other => Part::Button(other),
            });
        }
    }

    fn track_programs(&mut self, status: &Status) {
        if let Some((program, _)) = &status.foreground
            && !program.is_empty()
            && !program.eq_ignore_ascii_case("zpadder.exe")
            && self.recent_programs.first() != Some(program)
        {
            self.recent_programs.retain(|p| p != program);
            self.recent_programs.insert(0, program.clone());
            self.recent_programs.truncate(12);
        }
    }

    /// Whether the pad is using the profile being edited, as edited: not one AutoProfile chose,
    /// and not an older valid version kept while the edited one has problems.
    fn running_edited(&self, status: &Status) -> bool {
        status.auto_program.is_none()
            && self.errors.is_empty()
            && status.profile.as_deref() == Some(self.profile.name.as_str())
    }

    /// The layer active on the pad, when the pad is using the profile being edited.
    fn live_layer(&self, status: &Status) -> Option<usize> {
        let pad = status.pads.first()?;
        (self.running_edited(status) && pad.layer < self.profile.layers.len()).then_some(pad.layer)
    }

    /// The profile the pad is using, as far as the editor knows it.
    fn running_profile(&self, status: &Status) -> Option<Arc<Profile>> {
        match &status.auto_program {
            Some(program) => {
                let rule = self
                    .settings
                    .auto_profiles
                    .iter()
                    .find(|rule| rule.program.trim().eq_ignore_ascii_case(program))?;
                if self.path.as_ref() == Some(&rule.profile) {
                    return self.live.clone();
                }
                let (_, profile) =
                    self.auto_loaded.iter().find(|(path, _)| *path == rule.profile)?;
                Some(Arc::clone(profile))
            }
            None => self.live.clone(),
        }
    }

    // ---- Panels ----

    fn menu_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New profile").clicked() {
                    self.request(&ctx, Pending::New);
                    ui.close();
                }
                ui.menu_button("New from template", |ui| {
                    for (index, template) in builtin::TEMPLATES.iter().enumerate() {
                        if ui.button(template.name).on_hover_text(template.description).clicked() {
                            self.request(&ctx, Pending::Template(index));
                            ui.close();
                        }
                    }
                });
                if ui.button("Open…").clicked() {
                    self.request(&ctx, Pending::Open(None));
                    ui.close();
                }
                ui.add_enabled_ui(!self.settings.recent.is_empty(), |ui| {
                    ui.menu_button("Open recent", |ui| {
                        for path in self.settings.recent.clone() {
                            if ui.button(path.display().to_string()).clicked() {
                                self.request(&ctx, Pending::Open(Some(path)));
                                ui.close();
                            }
                        }
                    });
                });
                ui.separator();
                if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                    self.save();
                    ui.close();
                }
                if ui.button("Save as…").clicked() {
                    self.save_as();
                    ui.close();
                }
                if ui
                    .add_enabled(self.dirty(), egui::Button::new("Revert to saved"))
                    .on_hover_text("Drop the unsaved changes. Undo brings them back.")
                    .clicked()
                {
                    self.revert();
                    ui.close();
                }
                if !self.import_notes.is_empty() && ui.button("Import notes…").clicked() {
                    self.show_import_notes = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    self.request(&ctx, Pending::Quit);
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                let undo = egui::Button::new("Undo").shortcut_text("Ctrl+Z");
                if ui.add_enabled(self.history.can_undo(), undo).clicked() {
                    self.undo();
                    ui.close();
                }
                let redo = egui::Button::new("Redo").shortcut_text("Ctrl+Y");
                if ui.add_enabled(self.history.can_redo(), redo).clicked() {
                    self.redo();
                    ui.close();
                }
            });
            ui.menu_button("Mapping", |ui| {
                let mut enabled = self.settings.enabled;
                if ui
                    .checkbox(&mut enabled, "Mapping on/off")
                    .on_hover_text("Switch mapping on or off")
                    .changed()
                {
                    self.set_enabled(enabled);
                }
                if ui.button("AutoProfile rules…").clicked() {
                    self.show_auto = true;
                    ui.close();
                }
                ui.separator();
                let mut changed = false;
                if self.has_tray() {
                    changed |=
                        ui.checkbox(&mut self.settings.close_to_tray, "Close to tray").changed();
                    changed |= ui
                        .checkbox(&mut self.settings.start_minimized, "Start minimized to tray")
                        .changed();
                }
                self.autostart_menu(ui);
                if changed {
                    self.save_settings();
                }
            });
            ui.menu_button("Help", |ui| {
                ui.label(format!("ZPadder {}", env!("CARGO_PKG_VERSION")));
                ui.label("Click a control on the controller, or press it on your pad, to edit it.");
                ui.label("Mapping pauses while this window has focus.");
                ui.horizontal(|ui| {
                    ui.label("Profile format and timing slots:");
                    theme::docs_link(ui, "docs/profiles.md", "profiles.md");
                });
                ui.separator();
                let repository = env!("CARGO_PKG_REPOSITORY");
                for (label, url) in [
                    ("Report a problem…", format!("{repository}/issues")),
                    ("ZPadder on GitHub", repository.to_string()),
                ] {
                    if ui.button(label).on_hover_text(&url).clicked() {
                        if let Err(e) = zpadder_platform::open_url(&url) {
                            self.notice = Some(format!("Cannot open {url}: {e}"));
                        }
                        ui.close();
                    }
                }
                if let Some(folder) =
                    zpadder_platform::log_path().and_then(|log| log.parent().map(Path::to_owned))
                    && ui
                        .button("Open log folder")
                        .on_hover_text(folder.display().to_string())
                        .clicked()
                {
                    if let Err(e) = files::open_in_shell(&folder) {
                        self.notice = Some(e);
                    }
                    ui.close();
                }
            });
        });
    }

    /// "Start with Windows", and a way to repair an entry that starts another copy of ZPadder,
    /// such as one that has since been moved.
    fn autostart_menu(&mut self, ui: &mut Ui) {
        let status = zpadder_platform::autostart_status();
        let mut on = status == Autostart::On;
        if ui.checkbox(&mut on, "Start with Windows").changed()
            && let Err(e) = zpadder_platform::set_autostart(on)
        {
            self.notice = Some(format!("Cannot change Start with Windows: {e}"));
        }
        if let Autostart::Elsewhere { command } = &status {
            let detail = if command.is_empty() {
                "The start-up entry for ZPadder does not start this copy.".to_string()
            } else {
                format!("It runs {command}")
            };
            ui.colored_label(
                theme::warn(ui.visuals()),
                "Start with Windows points to another copy of ZPadder.",
            )
            .on_hover_text(detail);
            if ui
                .button("Use this copy")
                .on_hover_text("Make Windows start this copy of ZPadder instead")
                .clicked()
                && let Err(e) = zpadder_platform::set_autostart(true)
            {
                self.notice = Some(format!("Cannot change Start with Windows: {e}"));
            }
        }
    }

    /// The bar above the controller, after Xpadder's: profile switcher, layers, view toggles and
    /// the mapping switch. The layers scroll sideways when there are more than fit.
    fn toolbar(&mut self, ui: &mut Ui, live_layer: Option<usize>) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            // The right-hand controls first, so the layers get the room that is left.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut enabled = self.settings.enabled;
                let label = if enabled {
                    RichText::new("● Mapping on").color(theme::live(ui.visuals()))
                } else {
                    RichText::new("○ Mapping off")
                };
                if ui
                    .toggle_value(&mut enabled, label)
                    .on_hover_text("Mapping on/off: switch mapping on or off")
                    .changed()
                {
                    self.set_enabled(enabled);
                }
                ui.separator();
                ui.menu_button("View ▾", |ui| self.view_menu(ui))
                    .response
                    .on_hover_text("Controller picture and labels");
                if ui
                    .toggle_value(&mut self.settings.show_bench, "Test bench")
                    .on_hover_text("Try the profile here, without switching to another window")
                    .changed()
                {
                    self.save_settings();
                }
                ui.separator();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    self.profile_switcher(ui, &ctx);
                    ui.separator();
                    egui::ScrollArea::horizontal().id_salt("layer-pills").show(ui, |ui| {
                        ui.horizontal(|ui| self.layer_pills(ui, live_layer));
                    });
                });
            });
        });
    }

    fn profile_switcher(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let dirty = self.dirty();
        let title =
            if dirty { format!("● {}", self.profile.name) } else { self.profile.name.clone() };
        let current = self.path.clone().or_else(|| self.imported_from.clone());
        // Narrower when space is short, so the layer tabs keep their room.
        let width = (ui.available_width() * 0.3).clamp(120.0, 200.0);
        egui::ComboBox::from_id_salt("profile-switcher")
            .width(width)
            .selected_text(RichText::new(title).strong())
            .show_ui(ui, |ui| {
                for path in self.settings.recent.clone() {
                    let name = path.file_stem().map_or_else(
                        || path.display().to_string(),
                        |s| s.to_string_lossy().into_owned(),
                    );
                    if ui
                        .selectable_label(current.as_ref() == Some(&path), name)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        self.request(ctx, Pending::Open(Some(path)));
                    }
                }
                if !self.settings.recent.is_empty() {
                    ui.separator();
                }
                if ui.button("Open…").clicked() {
                    self.request(ctx, Pending::Open(None));
                }
                if ui.button("New profile").clicked() {
                    self.request(ctx, Pending::New);
                }
            })
            .response
            .on_hover_text("Switch profile");
        if ui.add_enabled(dirty, egui::Button::new("Save")).on_hover_text("Ctrl+S").clicked() {
            self.save();
        }
    }

    /// Layers as pills; the one active on the pad is green. Clicking the current one opens its
    /// settings.
    fn layer_pills(&mut self, ui: &mut Ui, live_layer: Option<usize>) {
        let mut clicked = None;
        for (i, layer) in self.profile.layers.iter().enumerate() {
            let live = live_layer == Some(i);
            let mut text = RichText::new(if live {
                format!("● {}", layer.name)
            } else {
                layer.name.clone()
            });
            if live && self.layer != i {
                text = text.color(theme::live(ui.visuals()));
            }
            let hint = if live {
                "Active on the controller now. Click the selected layer for its settings."
            } else {
                "Click the selected layer for its settings."
            };
            if ui.selectable_label(self.layer == i, text).on_hover_text(hint).clicked() {
                clicked = Some(i);
            }
        }
        if let Some(i) = clicked {
            if self.layer == i {
                self.set_selected(None);
            } else {
                self.set_layer(i);
            }
        }
        if theme::icon_button(ui, true, "+", "Add a layer").clicked() {
            let mut n = self.profile.layers.len() + 1;
            while self.profile.layer_index(&format!("Layer {n}")).is_some() {
                n += 1;
            }
            self.profile.layers.push(Layer::new(format!("Layer {n}")));
            self.set_layer(self.profile.layers.len() - 1);
            self.set_selected(None);
        }
    }

    fn pad(&mut self, ui: &mut Ui, status: &Status) {
        let live = status.pads.first();
        let family = live.map_or(Family::Xbox, |pad| Family::from_kind(&pad.kind));
        self.update_picture(ui.ctx(), family);
        let state = ViewState {
            family,
            layer: &self.profile.layers[self.layer],
            live,
            selected: self.selected,
            direction: self.direction,
            show_all: self.settings.show_all_buttons,
            show_names: self.settings.show_names,
            picture: self.picture.as_ref(),
            passthrough: self.profile.virtual_pad.is_some(),
        };
        let pick = if self.settings.callouts {
            callout::show(ui, &state)
        } else {
            ui.vertical_centered(|ui| pad_view::show(ui, &state)).inner
        };
        if let Some(pick) = pick {
            self.select(pick.part);
            if let Some(direction) = pick.direction {
                self.set_direction(direction);
            }
        }
        if live.is_none() {
            ui.weak("No controller connected. Plug one in to see it live.");
        }
    }

    fn inspector(&mut self, ui: &mut Ui, status: &Status) {
        let family = status.pads.first().map_or(Family::Xbox, |pad| Family::from_kind(&pad.kind));
        let layer_names: Vec<String> = self.profile.layers.iter().map(|l| l.name.clone()).collect();
        let Some(part) = self.selected else {
            self.layer_settings(ui);
            return;
        };
        let mut back = false;
        ui.horizontal(|ui| {
            ui.heading(pad_view::label(family, part));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                back = theme::icon_button(ui, true, "✖", "Back to layer settings").clicked();
            });
        });
        if back {
            self.set_selected(None);
            return;
        }
        let armed_before = (armed(&self.editor), armed(&self.click_editor));
        // A control's Xbox counterpart, when the profile acts as an Xbox controller.
        let xbox_on = self.profile.virtual_pad.is_some();
        let xbox_button =
            |button| xbox_on.then(|| zpadder_model::xbox::button_name(button)).flatten();
        let layer = &mut self.profile.layers[self.layer];
        ui.weak(format!("Layer \"{}\"", layer.name));
        ui.add_space(6.0);
        match part {
            Part::Button(button) => {
                ui.push_id("button", |ui| {
                    let xbox = xbox_button(button);
                    edit_button(ui, layer, button, &layer_names, &mut self.editor, xbox);
                });
            }
            Part::Trigger(side) => {
                theme::card(ui, "Trigger", |ui| {
                    ui.add(
                        egui::Slider::new(
                            &mut layer.trigger_threshold,
                            MIN_TRIGGER_THRESHOLD..=0.95,
                        )
                        .text("pull to press"),
                    )
                    .on_hover_text("Shared by both triggers in this layer.");
                });
                ui.add_space(6.0);
                let slot = match side {
                    Side::Left => &mut layer.left_trigger,
                    Side::Right => &mut layer.right_trigger,
                };
                let xbox = xbox_on.then_some(if side == Side::Left { "LT" } else { "RT" });
                ui.push_id("trigger", |ui| {
                    inspector::optional_assignment(ui, slot, &layer_names, &mut self.editor, xbox);
                });
            }
            Part::Stick(side) => {
                let stick = match side {
                    Side::Left => &mut layer.left_stick,
                    Side::Right => &mut layer.right_stick,
                };
                let live = status.pads.first().map(|pad| match side {
                    Side::Left => (pad.axes[0], pad.axes[1]),
                    Side::Right => (pad.axes[2], pad.axes[3]),
                });
                let direction_before = self.direction;
                theme::card(ui, "Stick", |ui| {
                    let xbox = xbox_on.then_some(if side == Side::Left {
                        "left stick"
                    } else {
                        "right stick"
                    });
                    inspector::stick_editor(ui, stick, &mut self.direction, live, xbox);
                });
                if self.direction != direction_before {
                    // Keys being picked were for the other direction.
                    self.editor = EditorState::default();
                }
                if stick.mode == StickMode::Directions {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!(
                            "Direction: {}",
                            inspector::direction_label(self.direction)
                        ))
                        .strong(),
                    );
                    let slot = inspector::direction_slot(stick, self.direction);
                    ui.push_id("direction", |ui| {
                        inspector::optional_assignment(
                            ui,
                            slot,
                            &layer_names,
                            &mut self.editor,
                            None,
                        );
                    });
                }
                ui.add_space(6.0);
                let click = if side == Side::Left { Button::LeftStick } else { Button::RightStick };
                egui::CollapsingHeader::new("Stick click")
                    .default_open(layer.buttons.contains_key(&click))
                    .show(ui, |ui| {
                        ui.push_id("click", |ui| {
                            let xbox = xbox_button(click);
                            edit_button(
                                ui,
                                layer,
                                click,
                                &layer_names,
                                &mut self.click_editor,
                                xbox,
                            );
                        });
                    });
            }
        }
        // Only one editor picks keys at a time: arming one disarms the other.
        if armed(&self.editor) && armed(&self.click_editor) {
            if armed_before.0 {
                self.editor = EditorState::default();
            } else {
                self.click_editor = EditorState::default();
            }
        }
    }

    fn layer_settings(&mut self, ui: &mut Ui) {
        ui.weak("Click a control on the controller, or press it on your pad, to edit it.");
        ui.add_space(6.0);
        theme::card(ui, "Profile", |ui| {
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(
                    egui::TextEdit::singleline(&mut self.profile.name).desired_width(f32::INFINITY),
                );
            });
            let mut xbox = self.profile.virtual_pad.is_some();
            if ui
                .checkbox(&mut xbox, "Act as an Xbox controller")
                .on_hover_text(XBOX_HINT)
                .changed()
            {
                self.profile.virtual_pad = xbox.then_some(VirtualPad::Xbox360);
            }
        });
        ui.add_space(6.0);
        let count = self.profile.layers.len();
        let index = self.layer;
        if self.layer_name_for != Some(index) {
            self.layer_name.clone_from(&self.profile.layers[index].name);
            self.layer_name_for = Some(index);
        }
        theme::card(ui, "Layer", |ui| {
            ui.horizontal(|ui| {
                ui.label("Name");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.layer_name)
                        .id_salt(("layer-name", index))
                        .desired_width(f32::INFINITY),
                );
                // The name is committed when the field is left, and only if no other layer has
                // it: layer switches find their layer by name. Escape gives up.
                if response.lost_focus() {
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        self.layer_name_for = None;
                    } else {
                        self.commit_layer_name();
                    }
                } else if !response.has_focus()
                    && self.layer_name != self.profile.layers[index].name
                {
                    // Not being typed in: follow the profile, as after an undo.
                    self.layer_name_for = None;
                }
            });
            if let Some(problem) =
                inspector::layer_name_problem(&self.profile, index, &self.layer_name)
            {
                ui.colored_label(theme::warn(ui.visuals()), problem);
            }
            let layer = &mut self.profile.layers[index];
            ui.add(
                egui::Slider::new(&mut layer.trigger_threshold, MIN_TRIGGER_THRESHOLD..=0.95)
                    .text("trigger pull to press"),
            );
            ui.horizontal(|ui| {
                if ui.add_enabled(index > 0, egui::Button::new("Move left")).clicked() {
                    self.move_layer(index - 1);
                }
                if ui.add_enabled(index + 1 < count, egui::Button::new("Move right")).clicked() {
                    self.move_layer(index + 1);
                }
                let delete = egui::Button::new(
                    RichText::new("Delete layer…").color(theme::error(ui.visuals())),
                );
                if ui
                    .add_enabled(count > 1, delete)
                    .on_hover_text(
                        "Asks first. Layer switches that go to this layer are removed too.",
                    )
                    .clicked()
                {
                    self.confirm_delete = Some(index);
                }
            });
            if index == 0 {
                ui.weak("The first layer is active when the profile loads.");
            }
            ui.weak(if self.profile.virtual_pad.is_some() {
                "Controls not assigned in this layer act as Xbox controller buttons, sticks and \
                 triggers while it is active."
            } else {
                "Controls not assigned in this layer do nothing while it is active."
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui, status: &Status) {
        let error_colour = theme::error(ui.visuals());
        let warn_colour = theme::warn(ui.visuals());
        if !self.errors.is_empty() {
            let headline = if self.live.is_some() {
                "The profile has problems; the last valid version is still in use:"
            } else {
                "The profile has problems, so the pad does nothing until they are fixed:"
            };
            ui.colored_label(error_colour, headline);
            for error in self.errors.iter().take(MAX_ERRORS_SHOWN) {
                ui.colored_label(error_colour, format!("• {error}"));
            }
            if self.errors.len() > MAX_ERRORS_SHOWN {
                ui.colored_label(
                    error_colour,
                    format!("and {} more", self.errors.len() - MAX_ERRORS_SHOWN),
                );
            }
            ui.separator();
        }
        if let Some(error) = &status.error {
            // The mapper has stopped; a new one is needed to map again.
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(error_colour, error);
                if ui
                    .button("Restart mapping")
                    .on_hover_text("Start mapping again with the same profile")
                    .clicked()
                {
                    let ctx = ui.ctx().clone();
                    self.restart_mapping(&ctx);
                }
                if let Some(folder) =
                    zpadder_platform::log_path().and_then(|log| log.parent().map(Path::to_owned))
                    && ui.button("Open log folder").clicked()
                    && let Err(e) = files::open_in_shell(&folder)
                {
                    self.notice = Some(e);
                }
            });
        }
        if let Some(title) = &status.blocked_window {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    warn_colour,
                    format!(
                        "\"{title}\" runs as administrator, so Windows blocks ZPadder's input to it."
                    ),
                );
                if ui.button("Restart ZPadder as administrator").clicked() {
                    let ctx = ui.ctx().clone();
                    self.request(&ctx, Pending::Elevate);
                }
            });
        }
        self.xbox_notice(ui, status);
        if let Some(notice) = self.settings_notice.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(warn_colour, notice);
                if theme::icon_button(ui, true, "✖", "Dismiss").clicked() {
                    self.settings_notice = None;
                }
            });
        }
        if !self.auto_errors.is_empty() {
            ui.horizontal_wrapped(|ui| {
                let text = match self.auto_errors.len() {
                    1 => "An AutoProfile rule's profile cannot be loaded, so the rule does nothing."
                        .to_string(),
                    n => format!(
                        "{n} AutoProfile rules' profiles cannot be loaded, so those rules do nothing."
                    ),
                };
                ui.colored_label(warn_colour, text)
                    .on_hover_text(self.auto_errors.join("\n"));
                if ui.small_button("Show rules").clicked() {
                    self.show_auto = true;
                }
            });
        }
        if let Some(notice) = self.notice.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(warn_colour, notice);
                if theme::icon_button(ui, true, "✖", "Dismiss").clicked() {
                    self.notice = None;
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            let (state, colour) = if !status.enabled {
                ("Mapping off", ui.visuals().weak_text_color())
            } else if status.locked {
                ("Mapping paused while Windows is locked", theme::paused(ui.visuals()))
            } else if status.testing {
                ("Mapping on in the test bench; the mouse stays still", theme::live(ui.visuals()))
            } else if status.paused {
                ("Mapping paused while ZPadder has focus", theme::paused(ui.visuals()))
            } else {
                ("Mapping on", theme::live(ui.visuals()))
            };
            ui.colored_label(colour, state);
            if let VirtualPadStatus::Active { count } = status.virtual_pads {
                ui.separator();
                let hint = match count {
                    0 => "Games will see an Xbox controller for each controller you connect."
                        .to_string(),
                    1 => "Games see an Xbox controller in place of your controller.".to_string(),
                    n => format!("Games see {n} Xbox controllers, one for each of yours."),
                };
                ui.colored_label(theme::live(ui.visuals()), "Xbox controller: on")
                    .on_hover_text(hint);
            }
            ui.separator();
            match (&status.profile, &status.auto_program) {
                (Some(name), Some(program)) => {
                    ui.label(format!("Profile: {name}, chosen by AutoProfile for {program}"));
                }
                (Some(name), None) => {
                    ui.label(format!("Profile: {name}"));
                }
                (None, _) => {
                    ui.weak("No profile");
                }
            }
            let running = self.running_profile(status);
            for pad in &status.pads {
                ui.separator();
                ui.label(&pad.name);
                let power = power_text(pad.power);
                if !power.is_empty() {
                    ui.separator();
                    ui.label(power);
                }
                // The layer of the profile the pad is using, which may not be the edited one.
                if let Some(layer) = running.as_ref().and_then(|p| p.layers.get(pad.layer)) {
                    ui.separator();
                    ui.label(format!("Layer: {}", layer.name));
                }
            }
            if status.pads.is_empty() {
                ui.separator();
                ui.weak("No controller connected");
            }
            if !status.other_mappers.is_empty() {
                ui.separator();
                ui.colored_label(
                    warn_colour,
                    format!("Also running: {}", status.other_mappers.join(", ")),
                )
                .on_hover_text(
                    "Other controller software may also read this pad, so games could get each \
                     press twice. The test bench says more.",
                );
            }
        });
    }

    /// Says when "Act as an Xbox controller" cannot work, and how to fix it.
    fn xbox_notice(&mut self, ui: &mut Ui, status: &Status) {
        let warn_colour = theme::warn(ui.visuals());
        match &status.virtual_pads {
            VirtualPadStatus::DriverMissing => {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        warn_colour,
                        "Act as an Xbox controller needs the free ViGEmBus driver.",
                    )
                    .on_hover_text(
                        "Everything else works without it. Once it is installed, ZPadder finds it \
                         within a few seconds; no restart needed.",
                    );
                    if ui
                        .button("Get ViGEmBus")
                        .on_hover_text(format!("Opens {VIGEMBUS_URL} in your browser"))
                        .clicked()
                        && let Err(e) = zpadder_platform::open_url(VIGEMBUS_URL)
                    {
                        self.notice = Some(format!("Cannot open {VIGEMBUS_URL}: {e}"));
                    }
                });
            }
            VirtualPadStatus::Failed(message) => {
                ui.colored_label(
                    warn_colour,
                    format!("Act as an Xbox controller isn't working: {message}"),
                );
            }
            // Steam applies its desktop layout to every Xbox controller it sees, ZPadder's
            // included, outside Steam games: the triggers become mouse clicks, buttons become
            // keys, and games get both.
            VirtualPadStatus::Active { .. }
                if !self.settings.steam_xbox_hint_hidden
                    && status.other_mappers.iter().any(|name| name == "Steam") =>
            {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        warn_colour,
                        "Steam may also be using ZPadder's Xbox controller. If buttons or \
                         triggers press keys or click in games, switch off Steam Input for Xbox \
                         controllers in Steam's controller settings.",
                    )
                    .on_hover_text(
                        "Outside Steam games, Steam applies its desktop layout to every Xbox \
                         controller it sees, so the game gets both the controller and Steam's \
                         keys and clicks. Steam's PlayStation setting is separate and can stay \
                         as it is.",
                    );
                    if ui.button("Don't show again").clicked() {
                        self.settings.steam_xbox_hint_hidden = true;
                        self.save_settings();
                    }
                });
            }
            VirtualPadStatus::Off | VirtualPadStatus::Active { .. } => {}
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.pending.clone() {
            self.unsaved_dialog(ctx, action);
        } else if let Some(index) = self.confirm_delete {
            self.delete_dialog(ctx, index);
        } else if let Some(recovered) = self.recovery_offer.clone() {
            self.recovery_dialog(ctx, recovered);
        }

        if self.show_import_notes {
            let mut open = true;
            egui::Window::new("Imported from Xpadder")
                .open(&mut open)
                .default_width(520.0)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .show(ctx, |ui| {
                    ui.label(
                        "The profile was converted. Anything Xpadder did differently, or that \
                         ZPadder cannot do yet, is listed here. File > Save as keeps it as a \
                         ZPadder profile.",
                    );
                    ui.separator();
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for note in &self.import_notes {
                            ui.label(format!("• {note}"));
                        }
                    });
                });
            self.show_import_notes = open;
        }

        if self.show_auto {
            let mut open = true;
            egui::Window::new("AutoProfile")
                .open(&mut open)
                .default_width(620.0)
                .show(ctx, |ui| self.auto_profile_ui(ui));
            self.show_auto = open;
            if !open && self.auto_edited.is_some() {
                self.apply_rule_edits();
            }
        }
    }

    /// Asks whether to save before `action`. Enter saves, Escape cancels.
    fn unsaved_dialog(&mut self, ctx: &egui::Context, action: Pending) {
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::unique("unsaved")).show(ctx, |ui| {
            ui.heading("Save changes?");
            ui.label(format!(
                "\"{}\" has unsaved changes. Save them before {}?",
                self.profile.name,
                action.doing()
            ));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Save").on_hover_text("Enter").clicked() {
                    choice = Some(0);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(1);
                }
                if ui.button("Cancel").on_hover_text("Escape").clicked() {
                    choice = Some(2);
                }
            });
        });
        // Enter saves, unless a button has keyboard focus: then Enter presses that button.
        let button_focused = ctx.memory(|m| m.focused().is_some());
        if choice.is_none()
            && !button_focused
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
        {
            choice = Some(0);
        }
        if choice.is_none() && modal.should_close() {
            choice = Some(2);
        }
        match choice {
            Some(0) => {
                if self.save() {
                    self.pending = None;
                    self.perform(ctx, action, false);
                }
            }
            Some(1) => {
                self.pending = None;
                self.drop_recovery_copy_for_discard(&action);
                self.perform(ctx, action, true);
            }
            Some(_) => self.pending = None,
            None => {}
        }
    }

    /// "Don't save" before quitting means the edits are meant to go; any other action keeps
    /// the copy until it has replaced the profile.
    fn drop_recovery_copy_for_discard(&mut self, action: &Pending) {
        if matches!(action, Pending::Quit | Pending::Elevate) {
            self.drop_recovery_copy();
        }
    }

    fn delete_dialog(&mut self, ctx: &egui::Context, index: usize) {
        let Some(name) = self.profile.layers.get(index).map(|l| l.name.clone()) else {
            self.confirm_delete = None;
            return;
        };
        let mut delete = None;
        let modal = egui::Modal::new(egui::Id::unique("delete-layer")).show(ctx, |ui| {
            ui.heading("Delete layer?");
            ui.label(format!(
                "\"{name}\" and everything assigned in it will be deleted, along with the layer \
                 switches that go to it. Undo (Ctrl+Z) brings it back."
            ));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let button =
                    egui::Button::new(RichText::new("Delete").color(theme::error(ui.visuals())));
                if ui.add(button).clicked() {
                    delete = Some(true);
                }
                if ui.button("Cancel").on_hover_text("Escape").clicked() {
                    delete = Some(false);
                }
            });
        });
        if delete.is_none() && modal.should_close() {
            delete = Some(false);
        }
        match delete {
            Some(true) => {
                self.confirm_delete = None;
                if self.profile.layers.len() > 1 {
                    self.commit_layer_name();
                    self.history.break_step();
                    inspector::remove_layer(&mut self.profile, index);
                    self.set_layer(self.layer.min(self.profile.layers.len() - 1));
                    self.set_selected(None);
                }
            }
            Some(false) => self.confirm_delete = None,
            None => {}
        }
    }

    fn recovery_dialog(&mut self, ctx: &egui::Context, recovered: Recovered) {
        let mut restore = None;
        egui::Modal::new(egui::Id::unique("recovery")).show(ctx, |ui| {
            ui.heading("Restore unsaved changes?");
            let name = if recovered.profile.name.trim().is_empty() {
                "a profile".to_string()
            } else {
                format!("\"{}\"", recovered.profile.name)
            };
            ui.label(format!("ZPadder closed before changes to {name} were saved."));
            if let Some(source) = recovered.path.as_ref().or(recovered.imported_from.as_ref()) {
                ui.weak(format!("From {}", source.display()));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Restore").on_hover_text("Open them as unsaved changes").clicked() {
                    restore = Some(true);
                }
                if ui.button("Discard").clicked() {
                    restore = Some(false);
                }
            });
        });
        match restore {
            Some(true) => {
                self.recovery_offer = None;
                self.restore(recovered);
            }
            Some(false) => {
                self.recovery_offer = None;
                if let Some(file) = &self.recovery_file {
                    recovery::remove(file);
                }
            }
            None => {}
        }
    }

    fn auto_profile_ui(&mut self, ui: &mut Ui) {
        ui.label(
            "When one of these programs has focus, its profile is used instead of the open \
             profile. The first matching rule wins.",
        );
        ui.weak(
            "Program names can use * for any run of characters and ? for any one, so \
             FiveM_*GTAProcess.exe keeps working when FiveM updates.",
        );
        ui.add_space(4.0);
        // Typing applies a moment after it stops; anything else at once.
        let mut typed = false;
        let mut changed = false;
        let mut remove = None;
        egui::Grid::new("auto-rules").striped(true).show(ui, |ui| {
            ui.strong("Program (e.g. game.exe)");
            ui.strong("Window title contains");
            ui.strong("Profile");
            ui.end_row();
            for (i, rule) in self.settings.auto_profiles.iter_mut().enumerate() {
                for text in [&mut rule.program, &mut rule.title_contains] {
                    let response = ui.text_edit_singleline(text);
                    typed |= response.changed();
                    changed |= response.lost_focus();
                }
                ui.horizontal(|ui| {
                    let name = rule
                        .profile
                        .file_name()
                        .map_or("(choose)".into(), |n| n.to_string_lossy().into_owned());
                    if ui.button(name).on_hover_text(rule.profile.display().to_string()).clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Profiles", &["toml", "xpadderprofile"])
                            .pick_file()
                    {
                        rule.profile = path;
                        changed = true;
                    }
                    if theme::icon_button(ui, true, "✖", "Remove rule").clicked() {
                        remove = Some(i);
                    }
                });
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.settings.auto_profiles.remove(i);
            changed = true;
        }
        ui.horizontal(|ui| {
            if ui.button("Add rule").clicked() {
                self.settings.auto_profiles.push(AutoRule {
                    profile: self.path.clone().unwrap_or_default(),
                    ..AutoRule::default()
                });
                changed = true;
            }
            ui.add_enabled_ui(!self.recent_programs.is_empty(), |ui| {
                ui.menu_button("Add recent program ▾", |ui| {
                    for program in self.recent_programs.clone() {
                        if ui.button(&program).clicked() {
                            self.settings.auto_profiles.push(AutoRule {
                                program,
                                title_contains: String::new(),
                                profile: self.path.clone().unwrap_or_default(),
                            });
                            changed = true;
                            ui.close();
                        }
                    }
                });
            });
        });
        for error in &self.auto_errors {
            ui.colored_label(theme::error(ui.visuals()), error);
        }
        if changed {
            self.apply_rule_edits();
        } else if typed {
            self.auto_edited = Some(Instant::now());
            ui.ctx().request_repaint_after(RULE_EDIT_DELAY);
        }
    }
}

/// Whether an editor is picking or capturing keys.
fn armed(editor: &EditorState) -> bool {
    editor.picking || editor.capturing
}

fn edit_button(
    ui: &mut Ui,
    layer: &mut Layer,
    button: Button,
    layer_names: &[String],
    editor: &mut EditorState,
    xbox: Option<&str>,
) {
    let mut slot = layer.buttons.get(&button).cloned();
    if inspector::optional_assignment(ui, &mut slot, layer_names, editor, xbox) {
        match slot {
            Some(assignment) => {
                layer.buttons.insert(button, assignment);
            }
            None => {
                layer.buttons.remove(&button);
            }
        }
    }
}

fn power_text(power: Power) -> String {
    match power {
        Power::Wired => "Wired".into(),
        Power::Charged => "Charged".into(),
        Power::Charging { percent: Some(p) } => format!("Charging {p}%"),
        Power::Charging { percent: None } => "Charging".into(),
        Power::OnBattery { percent: Some(p) } => format!("Battery {p}%"),
        Power::OnBattery { percent: None } => "On battery".into(),
        Power::Unknown => String::new(),
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.track_shown(ctx);
        self.handle_close(ctx);
        let remote: Vec<Vec<String>> =
            self.remote.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        for args in remote {
            self.handle_remote(ctx, args);
        }
        #[cfg(windows)]
        if let Some(commands) = self.tray.as_ref().map(Tray::commands) {
            for command in commands {
                match command {
                    TrayCommand::Show => self.show_window(ctx),
                    TrayCommand::ToggleEnabled => self.set_enabled(!self.settings.enabled),
                    TrayCommand::Quit => self.request(ctx, Pending::Quit),
                    TrayCommand::Open(path) => {
                        self.request(ctx, Pending::Open(Some(path)));
                        // The menu ticked what was clicked; put the tick back where it belongs
                        // if that open was cancelled or failed.
                        if let Some(tray) = &mut self.tray {
                            tray.refresh_profiles();
                        }
                    }
                }
            }
        }
        // A question waiting for an answer needs the window.
        if (self.pending.is_some() || self.confirm_delete.is_some()) && self.hidden {
            self.show_window(ctx);
        }
        let status = self.mapper.status();
        self.track_programs(&status);
        if let Some(edited) = self.auto_edited
            && edited.elapsed() >= RULE_EDIT_DELAY
        {
            self.apply_rule_edits();
        }
        self.sync(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.hidden {
            // Hidden in the tray: nothing to draw. `logic` does what still needs doing.
            self.bench.hidden(&self.mapper);
            return;
        }
        if ctx.input(|i| i.pointer.any_pressed()) {
            // Each click or drag is an undo step of its own.
            self.history.interaction();
        }
        // While keys are being picked, every key is one to bind: Ctrl+S is not Save, and Space or
        // Enter must not press the button that was clicked last (egui gives it keyboard focus).
        let picking_keys = armed(&self.editor) || armed(&self.click_editor);
        if picking_keys && !ctx.text_edit_focused() {
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
        let dialog_open = self.pending.is_some() || self.confirm_delete.is_some();
        if !picking_keys && !dialog_open {
            let shortcut = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
            if ctx.input_mut(|i| i.consume_shortcut(&shortcut(Key::S))) {
                self.save();
            }
            // Text fields have their own undo.
            if !ctx.text_edit_focused() {
                let redo_shift =
                    KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
                if ctx.input_mut(|i| {
                    i.consume_shortcut(&redo_shift) || i.consume_shortcut(&shortcut(Key::Y))
                }) {
                    self.redo();
                } else if ctx.input_mut(|i| i.consume_shortcut(&shortcut(Key::Z))) {
                    self.undo();
                }
            }
        }
        let status = self.mapper.status();
        // Pressing pad buttons selects them, except while typing with the pad in the test bench.
        if ctx.input(|i| i.focused) && !status.testing {
            self.follow_controller(&status);
        }
        if self.layer >= self.profile.layers.len() {
            self.set_layer(0);
        }
        let live_layer = self.live_layer(&status);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui, &status));
        if self.settings.show_bench {
            // At most half the window, so the controller and status bar stay in view.
            let max = (ctx.content_rect().height() * 0.45).max(120.0);
            let running_profile = self.running_profile(&status);
            let running = Running {
                layer: match live_layer {
                    Some(i) => self.profile.layers.get(i),
                    None => running_profile
                        .as_ref()
                        .zip(status.pads.first())
                        .and_then(|(profile, pad)| profile.layers.get(pad.layer)),
                },
                other: (!self.running_edited(&status)).then(|| match &status.profile {
                    Some(name) => match &status.auto_program {
                        Some(program) => format!(
                            "The pad is using \"{name}\", chosen by AutoProfile for {program}."
                        ),
                        None => format!(
                            "The pad is using the last valid version of \"{name}\" until the \
                             problems below are fixed."
                        ),
                    },
                    None => "The pad is not using a profile.".to_string(),
                }),
            };
            egui::Panel::bottom("bench")
                .resizable(true)
                .default_size(260.0_f32.min(max))
                .min_size(120.0_f32.min(max))
                .max_size(max)
                .show(ui, |ui| self.bench.show(ui, &self.mapper, &status, &running));
        } else {
            self.bench.hidden(&self.mapper);
        }
        egui::Panel::right("inspector").resizable(true).default_size(430.0).min_size(300.0).show(
            ui,
            |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.inspector(ui, &status));
            },
        );
        egui::CentralPanel::default().show(ui, |ui| {
            self.toolbar(ui, live_layer);
            ui.add_space(6.0);
            self.pad(ui, &status);
        });
        self.dialogs(&ctx);
        self.sync(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Err(e) = self.settings.save() {
            tracing::warn!("{e}");
        }
        // Unsaved edits left on purpose go; any others stay in the recovery copy.
        if self.discard_on_quit || !self.dirty() {
            self.drop_recovery_copy();
        } else if let (Some(file), None) = (&self.recovery_file, &self.recovery_offer) {
            let recovered = Recovered {
                profile: self.profile.clone(),
                path: self.path.clone(),
                imported_from: self.imported_from.clone(),
            };
            if let Err(e) = recovery::write(file, &recovered) {
                tracing::warn!("Cannot write the recovery copy: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_import_problems_open_the_notes() {
        let routine = [
            "Button order taken from C:\\pack\\pad.xpaddercontroller".to_string(),
            "No .xpaddercontroller next to the profile, so the Xbox button order was assumed"
                .to_string(),
        ];
        assert!(!has_real_warnings(&routine));
        assert!(!has_real_warnings(&[]));
        let mut notes = routine.to_vec();
        notes.push("Set1Button3Slots: unknown key `Foo`; ignored".into());
        assert!(has_real_warnings(&notes));
    }

    #[test]
    fn arguments_are_quoted_for_the_command_line() {
        assert_eq!(quote_argument(r"C:\Games\pad.toml"), r#""C:\Games\pad.toml""#);
        // A trailing backslash would otherwise escape the closing quote.
        assert_eq!(quote_argument(r"C:\Games\"), r#""C:\Games\\""#);
        assert_eq!(quote_argument(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_argument(r#"a\"b"#), r#""a\\\"b""#);
        assert_eq!(quote_argument(""), r#""""#);
    }

    #[test]
    fn power_is_written_with_a_capital() {
        assert_eq!(power_text(Power::Wired), "Wired");
        assert_eq!(power_text(Power::OnBattery { percent: Some(50) }), "Battery 50%");
        assert_eq!(power_text(Power::Charging { percent: None }), "Charging");
        assert_eq!(power_text(Power::Unknown), "");
    }
}
