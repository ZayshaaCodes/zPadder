//! The notification-area (tray) icon. Its menu works while the window is hidden: handlers
//! queue a command and wake the app, whose `logic` step runs even when nothing is shown.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use eframe::egui;
use tray_icon::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::icon;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TrayCommand {
    Show,
    ToggleEnabled,
    Open(PathBuf),
    Quit,
}

/// The profile submenu's items and the files they open, shared with the menu event handler.
type ProfileItems = Arc<Mutex<Vec<(MenuId, PathBuf)>>>;

pub(crate) struct Tray {
    _icon: TrayIcon,
    enabled: CheckMenuItem,
    profiles: Submenu,
    profile_items: ProfileItems,
    /// What the profile submenu currently shows, to rebuild it only on change.
    shown: (Vec<PathBuf>, Option<PathBuf>),
    commands: Receiver<TrayCommand>,
}

impl Tray {
    pub(crate) fn new(ctx: &egui::Context, enabled: bool) -> Result<Self, String> {
        let show = MenuItem::new("Open ZPadder", true, None);
        let toggle = CheckMenuItem::new("Mapping on/off", true, enabled, None);
        let profiles = Submenu::new("Profile", true);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        menu.append_items(&[&show, &toggle, &profiles, &PredefinedMenuItem::separator(), &quit])
            .map_err(|e| e.to_string())?;

        let (tx, commands) = mpsc::channel();
        let profile_items: ProfileItems = Arc::default();
        let ids = [show.id().clone(), toggle.id().clone(), quit.id().clone()];
        let menu_tx = tx.clone();
        let menu_ctx = ctx.clone();
        let menu_profiles = Arc::clone(&profile_items);
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let command = match ids.iter().position(|id| *id == event.id) {
                Some(0) => TrayCommand::Show,
                Some(1) => TrayCommand::ToggleEnabled,
                Some(2) => TrayCommand::Quit,
                _ => {
                    let items = menu_profiles.lock().unwrap_or_else(|e| e.into_inner());
                    match items.iter().find(|(id, _)| *id == event.id) {
                        Some((_, path)) => TrayCommand::Open(path.clone()),
                        None => return,
                    }
                }
            };
            let _ = menu_tx.send(command);
            menu_ctx.request_repaint();
        }));
        let icon_ctx = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = event {
                let _ = tx.send(TrayCommand::Show);
                icon_ctx.request_repaint();
            }
        }));

        let image =
            tray_icon::Icon::from_rgba(icon::rgba(32), 32, 32).map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("ZPadder")
            .with_icon(image)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            _icon: tray,
            enabled: toggle,
            profiles,
            profile_items,
            shown: (Vec::new(), None),
            commands,
        })
    }

    pub(crate) fn commands(&self) -> Vec<TrayCommand> {
        self.commands.try_iter().collect()
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.set_checked(enabled);
    }

    /// Makes the next [`Tray::set_profiles`] rebuild the Profile submenu. Clicking a profile
    /// ticks it at once, before the app has tried to open it; if that is cancelled or fails, the
    /// rebuild puts the tick back where it belongs.
    pub(crate) fn refresh_profiles(&mut self) {
        self.shown = (Vec::new(), Some(PathBuf::new()));
    }

    /// Lists `recent` profiles in the Profile submenu, ticking `current`.
    pub(crate) fn set_profiles(&mut self, recent: &[PathBuf], current: Option<&Path>) {
        let wanted = (recent.to_vec(), current.map(Path::to_path_buf));
        if wanted == self.shown {
            return;
        }
        while self.profiles.remove_at(0).is_some() {}
        let mut items = self.profile_items.lock().unwrap_or_else(|e| e.into_inner());
        items.clear();
        if recent.is_empty() {
            let _ = self.profiles.append(&MenuItem::new("No recent profiles", false, None));
        }
        for path in recent {
            let name = path.file_stem().map_or_else(
                || path.display().to_string(),
                |stem| stem.to_string_lossy().into_owned(),
            );
            let item = CheckMenuItem::new(name, true, Some(path.as_path()) == current, None);
            items.push((item.id().clone(), path.clone()));
            let _ = self.profiles.append(&item);
        }
        self.shown = wanted;
    }
}
