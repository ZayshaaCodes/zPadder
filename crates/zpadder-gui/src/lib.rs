//! ZPadder's window and tray icon.
//!
//! The editor follows Xpadder's picture-first workflow: click a control on the controller (or
//! press it on the real pad), then choose what it does from an on-screen keyboard and mouse.
//! Edits apply to the running mapper as soon as they are valid.

mod app;
mod bench;
mod builtin;
mod callout;
mod files;
mod history;
mod icon;
mod inspector;
mod keyboard;
mod labels;
mod pad_view;
mod picture;
mod recovery;
mod settings;
mod theme;
#[cfg(windows)]
mod tray;

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Vec2, vec2};

/// How the app was started.
#[derive(Debug, Default)]
pub struct Launch {
    /// Profile to open instead of the last one used.
    pub profile: Option<PathBuf>,
    /// Start hidden in the tray.
    pub minimized: bool,
    /// Command lines handed over by later launches (Xpadder-style: profile paths, `/M`, `/C`).
    /// An empty one means "show the window".
    pub remote: Option<std::sync::mpsc::Receiver<Vec<String>>>,
}

/// The window's size when there is room for it, in points.
const PREFERRED_SIZE: Vec2 = vec2(1180.0, 760.0);
/// The smallest the window can be made: small enough for 1366 by 768 at 125% scaling.
const MIN_SIZE: Vec2 = vec2(720.0, 440.0);

/// Runs the app until the user quits.
pub fn run(launch: Launch) -> Result<(), String> {
    let loaded = settings::Settings::load();
    let hidden = launch.minimized || loaded.settings.start_minimized;
    let icon = egui::IconData { rgba: icon::rgba(64), width: 64, height: 64 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ZPadder")
            .with_inner_size(initial_size())
            .with_min_inner_size(MIN_SIZE)
            .with_icon(Arc::new(icon))
            .with_visible(!hidden),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "ZPadder",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, launch, loaded, hidden)))
        }),
    )
    .map_err(|e| e.to_string())
}

/// The window's first size: the preferred size, or less to fit the screen's work area (the
/// screen without the taskbar) with room for the title bar.
fn initial_size() -> Vec2 {
    match work_area() {
        Some(area) => fit(PREFERRED_SIZE, area),
        None => PREFERRED_SIZE,
    }
}

/// `preferred`, made to fit in 90% of a work area `area` big, title bar included.
fn fit(preferred: Vec2, area: Vec2) -> Vec2 {
    const TITLE_BAR: f32 = 32.0;
    let room = vec2(area.x * 0.9, area.y * 0.9 - TITLE_BAR);
    preferred.min(room).max(MIN_SIZE)
}

/// The primary screen's work area in points.
#[cfg(windows)]
fn work_area() -> Option<Vec2> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut rect = RECT::default();
    // SAFETY: SPI_GETWORKAREA writes one RECT to the pointer, which points at `rect`.
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut rect).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .ok()?;
    // SAFETY: no preconditions. Both values are in the same units, whether or not the process
    // is DPI aware yet.
    let dpi = unsafe { GetDpiForSystem() };
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    let size = vec2((rect.right - rect.left) as f32, (rect.bottom - rect.top) as f32) / scale;
    (size.x > 0.0 && size.y > 0.0).then_some(size)
}

#[cfg(not(windows))]
fn work_area() -> Option<Vec2> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_fits_small_screens() {
        // 1920 by 1080 at 150%, with the taskbar: 1280 by 696 points.
        let size = fit(PREFERRED_SIZE, vec2(1280.0, 696.0));
        assert!(size.x <= 1152.0 && size.y + 32.0 <= 696.0 * 0.9 + 0.01, "{size:?}");
        // 1366 by 768 at 125%: the window can still shrink to fit.
        let small = vec2(1093.0, 582.0);
        assert!(MIN_SIZE.x <= small.x && MIN_SIZE.y + 32.0 <= small.y);
        // Big screens get the preferred size.
        assert_eq!(fit(PREFERRED_SIZE, vec2(2560.0, 1400.0)), PREFERRED_SIZE);
    }
}
