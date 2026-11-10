//! SDL3 gamepad backend.
//!
//! SDL covers XInput, DirectInput and (through its HIDAPI drivers) PlayStation and Switch pads,
//! plus gyro, rumble and battery. Only gamepads SDL can map are reported; joysticks without a
//! mapping are listed by [`SdlBackend::unmapped_joysticks`]. A [`DeviceFilter`] can keep
//! gamepads out before they are opened, such as the virtual controllers ZPadder itself creates.

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, c_char, c_int};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sdl3_sys::error::SDL_GetError;
use sdl3_sys::events::{
    SDL_EVENT_GAMEPAD_ADDED, SDL_EVENT_GAMEPAD_AXIS_MOTION, SDL_EVENT_GAMEPAD_BUTTON_DOWN,
    SDL_EVENT_GAMEPAD_BUTTON_UP, SDL_EVENT_GAMEPAD_REMOVED, SDL_Event, SDL_PollEvent,
    SDL_WaitEventTimeout,
};
use sdl3_sys::gamepad::{
    SDL_CloseGamepad, SDL_GAMEPAD_AXIS_LEFT_TRIGGER, SDL_GAMEPAD_AXIS_LEFTX,
    SDL_GAMEPAD_AXIS_LEFTY, SDL_GAMEPAD_AXIS_RIGHT_TRIGGER, SDL_GAMEPAD_AXIS_RIGHTX,
    SDL_GAMEPAD_AXIS_RIGHTY, SDL_GAMEPAD_BUTTON_BACK, SDL_GAMEPAD_BUTTON_DPAD_DOWN,
    SDL_GAMEPAD_BUTTON_DPAD_LEFT, SDL_GAMEPAD_BUTTON_DPAD_RIGHT, SDL_GAMEPAD_BUTTON_DPAD_UP,
    SDL_GAMEPAD_BUTTON_EAST, SDL_GAMEPAD_BUTTON_GUIDE, SDL_GAMEPAD_BUTTON_LEFT_PADDLE1,
    SDL_GAMEPAD_BUTTON_LEFT_PADDLE2, SDL_GAMEPAD_BUTTON_LEFT_SHOULDER,
    SDL_GAMEPAD_BUTTON_LEFT_STICK, SDL_GAMEPAD_BUTTON_MISC1, SDL_GAMEPAD_BUTTON_NORTH,
    SDL_GAMEPAD_BUTTON_RIGHT_PADDLE1, SDL_GAMEPAD_BUTTON_RIGHT_PADDLE2,
    SDL_GAMEPAD_BUTTON_RIGHT_SHOULDER, SDL_GAMEPAD_BUTTON_RIGHT_STICK, SDL_GAMEPAD_BUTTON_SOUTH,
    SDL_GAMEPAD_BUTTON_START, SDL_GAMEPAD_BUTTON_TOUCHPAD, SDL_GAMEPAD_BUTTON_WEST,
    SDL_GamepadAxis, SDL_GamepadButton, SDL_GamepadHasSensor, SDL_GetGamepadName,
    SDL_GetGamepadPath, SDL_GetGamepadPathForID, SDL_GetGamepadPowerInfo, SDL_GetGamepadProduct,
    SDL_GetGamepadProductForID, SDL_GetGamepadProperties, SDL_GetGamepadStringForType,
    SDL_GetGamepadType, SDL_GetGamepadVendor, SDL_GetGamepadVendorForID, SDL_IsGamepad,
    SDL_OpenGamepad, SDL_PROP_GAMEPAD_CAP_RUMBLE_BOOLEAN, SDL_RumbleGamepad,
};
use sdl3_sys::hints::{
    SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS, SDL_HINT_JOYSTICK_ENHANCED_REPORTS, SDL_SetHint,
};
use sdl3_sys::init::{SDL_INIT_GAMEPAD, SDL_INIT_HAPTIC, SDL_INIT_SENSOR, SDL_Init, SDL_Quit};
use sdl3_sys::joystick::SDL_JoystickID;
use sdl3_sys::joystick::{SDL_GetJoystickNameForID, SDL_GetJoysticks};
use sdl3_sys::power::{
    SDL_POWERSTATE_CHARGED, SDL_POWERSTATE_CHARGING, SDL_POWERSTATE_NO_BATTERY,
    SDL_POWERSTATE_ON_BATTERY,
};
use sdl3_sys::properties::SDL_GetBooleanProperty;
use sdl3_sys::sensor::SDL_SENSOR_GYRO;
use sdl3_sys::stdinc::SDL_free;
use sdl3_sys::version::{
    SDL_GetVersion, SDL_VERSIONNUM_MAJOR, SDL_VERSIONNUM_MICRO, SDL_VERSIONNUM_MINOR,
};
use zpadder_model::{Axis, Button, InputEvent, PadId};

use crate::InputBackend;

/// SDL keeps global state, so only one backend may exist at a time.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// SDL could not be started.
#[derive(Debug, Clone)]
pub struct SdlError(String);

impl fmt::Display for SdlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SdlError {}

/// Battery state of a pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    Unknown,
    Wired,
    OnBattery { percent: Option<u8> },
    Charging { percent: Option<u8> },
    Charged,
}

/// Descriptive details about a connected pad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PadInfo {
    pub name: String,
    /// SDL's controller type, e.g. `xbox360`, `ps5`, `switchpro`.
    pub kind: String,
    /// SDL's device path: a HID path, or `XInput#<user index>` for pads read through XInput.
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub has_gyro: bool,
    pub has_rumble: bool,
    pub power: Power,
}

/// What is known about a gamepad before it is opened, for a [`DeviceFilter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    /// SDL's instance id: unique to this connection of the device, never reused.
    pub instance: u32,
    /// SDL's device path; see [`PadInfo::path`].
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
}

/// What a [`DeviceFilter`] decides about a gamepad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Open it and report it.
    Admit,
    /// Never open it, for as long as it stays connected.
    Ignore,
    /// Not known yet: ask again on the next [`InputBackend::poll`].
    Defer,
}

/// Decides which gamepads [`SdlBackend`] reads.
pub trait DeviceFilter {
    /// Called when a gamepad appears, and again on every poll while the answer is
    /// [`Verdict::Defer`].
    fn check(&mut self, device: &DeviceIdentity) -> Verdict;
    /// A gamepad that was ignored or deferred has gone away.
    fn gone(&mut self, instance: u32);
}

/// The XInput user index (0–3) in an SDL device path of the form `XInput#<index>`, which SDL
/// gives every pad it reads through XInput (`SDL_xinputjoystick.c`). `None` for other pads.
pub fn xinput_slot(path: &str) -> Option<u8> {
    let index = path.strip_prefix("XInput#")?;
    if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    index.parse().ok()
}

/// The SDL version linked into this build, e.g. `3.4.18`.
pub fn version() -> String {
    let v = SDL_GetVersion();
    format!("{}.{}.{}", SDL_VERSIONNUM_MAJOR(v), SDL_VERSIONNUM_MINOR(v), SDL_VERSIONNUM_MICRO(v))
}

/// Reads gamepads through SDL3. Must stay on the thread that created it.
pub struct SdlBackend {
    pads: HashMap<u32, *mut sdl3_sys::gamepad::SDL_Gamepad>,
    filter: Option<Box<dyn DeviceFilter>>,
    /// Gamepads the filter said to ignore, until they go away.
    ignored: HashSet<u32>,
    /// Gamepads the filter has not decided about yet, in the order they appeared.
    deferred: Vec<u32>,
}

impl fmt::Debug for SdlBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SdlBackend")
            .field("pads", &self.pads.keys().collect::<Vec<_>>())
            .field("ignored", &self.ignored)
            .field("deferred", &self.deferred)
            .finish_non_exhaustive()
    }
}

impl SdlBackend {
    /// Starts SDL's gamepad subsystem with background input enabled.
    pub fn new() -> Result<Self, SdlError> {
        if ACTIVE.swap(true, Ordering::SeqCst) {
            return Err(SdlError("an SDL backend is already running".into()));
        }
        // SAFETY: all arguments are NUL-terminated strings; hints must be set before SDL_Init.
        unsafe {
            SDL_SetHint(SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS, c"1".as_ptr());
            // Bluetooth PlayStation pads stay in their simple report mode, which other programs
            // (and DirectInput) understand, unless rumble or the gyro is actually used. Once
            // switched, a pad keeps the enhanced mode until it is turned off.
            SDL_SetHint(SDL_HINT_JOYSTICK_ENHANCED_REPORTS, c"auto".as_ptr());
        }
        // SAFETY: SDL_Init has no preconditions beyond being called from one thread at a time,
        // which ACTIVE guarantees.
        if !unsafe { SDL_Init(SDL_INIT_GAMEPAD | SDL_INIT_SENSOR | SDL_INIT_HAPTIC) } {
            ACTIVE.store(false, Ordering::SeqCst);
            return Err(last_error());
        }
        Ok(Self {
            pads: HashMap::new(),
            filter: None,
            ignored: HashSet::new(),
            deferred: Vec::new(),
        })
    }

    /// Lets `filter` decide which gamepads are opened and reported from now on. Pads already
    /// open stay open.
    pub fn set_filter(&mut self, filter: Box<dyn DeviceFilter>) {
        self.filter = Some(filter);
    }

    /// How many pads are connected (and mapped by SDL).
    pub fn pad_count(&self) -> usize {
        self.pads.len()
    }

    /// Whether `pad` is connected and open.
    pub fn has_pad(&self, pad: PadId) -> bool {
        self.pads.contains_key(&pad.0)
    }

    /// Details about a connected pad, or `None` if it is not connected.
    pub fn pad_info(&self, pad: PadId) -> Option<PadInfo> {
        let gamepad = *self.pads.get(&pad.0)?;
        // SAFETY: `gamepad` came from SDL_OpenGamepad and is closed only when removed from
        // `pads`. Returned strings are owned by SDL and copied immediately.
        unsafe {
            let mut percent: c_int = -1;
            let state = SDL_GetGamepadPowerInfo(gamepad, &mut percent);
            let percent = u8::try_from(percent).ok();
            let power = match state {
                SDL_POWERSTATE_NO_BATTERY => Power::Wired,
                SDL_POWERSTATE_ON_BATTERY => Power::OnBattery { percent },
                SDL_POWERSTATE_CHARGING => Power::Charging { percent },
                SDL_POWERSTATE_CHARGED => Power::Charged,
                _ => Power::Unknown,
            };
            Some(PadInfo {
                name: c_string(SDL_GetGamepadName(gamepad)),
                kind: c_string(SDL_GetGamepadStringForType(SDL_GetGamepadType(gamepad))),
                path: c_string(SDL_GetGamepadPath(gamepad)),
                vendor_id: SDL_GetGamepadVendor(gamepad),
                product_id: SDL_GetGamepadProduct(gamepad),
                has_gyro: SDL_GamepadHasSensor(gamepad, SDL_SENSOR_GYRO),
                has_rumble: SDL_GetBooleanProperty(
                    SDL_GetGamepadProperties(gamepad),
                    SDL_PROP_GAMEPAD_CAP_RUMBLE_BOOLEAN,
                    false,
                ),
                power,
            })
        }
    }

    /// Names of connected joysticks that SDL has no gamepad mapping for (and so ignores).
    pub fn unmapped_joysticks(&self) -> Vec<String> {
        let mut names = Vec::new();
        // SAFETY: SDL_GetJoysticks returns an SDL-allocated array of `count` ids (or null), which
        // is freed with SDL_free after use.
        unsafe {
            let mut count: c_int = 0;
            let ids = SDL_GetJoysticks(&mut count);
            if ids.is_null() {
                return names;
            }
            for &id in std::slice::from_raw_parts(ids, usize::try_from(count).unwrap_or(0)) {
                if !SDL_IsGamepad(id) {
                    names.push(c_string(SDL_GetJoystickNameForID(id)));
                }
            }
            SDL_free(ids.cast());
        }
        names
    }

    /// Rumbles a pad at the given low- and high-frequency strengths. Returns `false` if the pad
    /// is not connected or cannot rumble.
    pub fn rumble(&mut self, pad: PadId, low: u16, high: u16, duration: Duration) -> bool {
        let Some(&gamepad) = self.pads.get(&pad.0) else { return false };
        let ms = u32::try_from(duration.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: `gamepad` is a live handle from SDL_OpenGamepad.
        unsafe { SDL_RumbleGamepad(gamepad, low, high, ms) }
    }

    /// What the filter says about gamepad `id`; [`Verdict::Admit`] without a filter.
    fn check(&mut self, id: SDL_JoystickID) -> Verdict {
        let Some(filter) = &mut self.filter else { return Verdict::Admit };
        // SAFETY: SDL is initialised; the *ForID functions accept any id and return null or 0
        // for one that is gone. The path is owned by SDL and copied immediately.
        let device = unsafe {
            DeviceIdentity {
                instance: id.0,
                path: c_string(SDL_GetGamepadPathForID(id)),
                vendor_id: SDL_GetGamepadVendorForID(id),
                product_id: SDL_GetGamepadProductForID(id),
            }
        };
        filter.check(&device)
    }

    /// Opens gamepad `id` and reports it.
    fn open(&mut self, id: SDL_JoystickID, out: &mut Vec<InputEvent>) {
        // SAFETY: opening any id is allowed (null for one that is gone); a handle replaced in
        // `pads` is live and closed once.
        unsafe {
            let gamepad = SDL_OpenGamepad(id);
            if !gamepad.is_null() {
                if let Some(old) = self.pads.insert(id.0, gamepad) {
                    SDL_CloseGamepad(old);
                }
                out.push(InputEvent::Connected { pad: PadId(id.0) });
            }
        }
    }

    /// A gamepad appeared: open it, unless the filter keeps it out or decides later.
    fn added(&mut self, id: SDL_JoystickID, out: &mut Vec<InputEvent>) {
        match self.check(id) {
            Verdict::Admit => self.open(id, out),
            Verdict::Ignore => {
                self.ignored.insert(id.0);
            }
            Verdict::Defer => self.deferred.push(id.0),
        }
    }

    /// Asks the filter again about gamepads it deferred.
    fn recheck_deferred(&mut self, out: &mut Vec<InputEvent>) {
        for id in std::mem::take(&mut self.deferred) {
            self.added(SDL_JoystickID(id), out);
        }
    }

    fn handle(&mut self, event: &SDL_Event, out: &mut Vec<InputEvent>) {
        // SAFETY: each arm reads the union field that SDL documents for that event type.
        unsafe {
            match event.event_type() {
                SDL_EVENT_GAMEPAD_ADDED => self.added(event.gdevice.which, out),
                SDL_EVENT_GAMEPAD_REMOVED => {
                    let id = event.gdevice.which;
                    if let Some(gamepad) = self.pads.remove(&id.0) {
                        SDL_CloseGamepad(gamepad);
                        out.push(InputEvent::Disconnected { pad: PadId(id.0) });
                    } else if self.ignored.remove(&id.0) || self.deferred.contains(&id.0) {
                        self.deferred.retain(|&deferred| deferred != id.0);
                        if let Some(filter) = &mut self.filter {
                            filter.gone(id.0);
                        }
                    }
                }
                SDL_EVENT_GAMEPAD_BUTTON_DOWN | SDL_EVENT_GAMEPAD_BUTTON_UP => {
                    let e = event.gbutton;
                    if let Some(button) = button(SDL_GamepadButton(c_int::from(e.button))) {
                        let pad = PadId(e.which.0);
                        out.push(InputEvent::Button { pad, button, pressed: e.down });
                    }
                }
                SDL_EVENT_GAMEPAD_AXIS_MOTION => {
                    let e = event.gaxis;
                    if let Some(axis) = axis(SDL_GamepadAxis(c_int::from(e.axis))) {
                        let value = (f32::from(e.value) / 32767.0).clamp(-1.0, 1.0);
                        out.push(InputEvent::Axis { pad: PadId(e.which.0), axis, value });
                    }
                }
                _ => {}
            }
        }
    }
}

impl InputBackend for SdlBackend {
    /// While waiting, SDL checks for input every millisecond. A zero `timeout` only collects what
    /// is there, without waiting, which lets an idle caller sleep for longer between checks.
    fn poll(&mut self, timeout: Duration, out: &mut Vec<InputEvent>) {
        self.recheck_deferred(out);
        let mut event = SDL_Event::default();
        let ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        let got = if ms == 0 {
            // SAFETY: `event` is a valid SDL_Event for SDL to fill in.
            unsafe { SDL_PollEvent(&mut event) }
        } else {
            // SAFETY: as above.
            unsafe { SDL_WaitEventTimeout(&mut event, ms) }
        };
        if !got {
            return;
        }
        loop {
            self.handle(&event, out);
            // SAFETY: as above.
            if !unsafe { SDL_PollEvent(&mut event) } {
                break;
            }
        }
    }
}

impl Drop for SdlBackend {
    fn drop(&mut self) {
        // SAFETY: every handle in `pads` is live; SDL_Quit is called once, after them.
        unsafe {
            for (_, gamepad) in self.pads.drain() {
                SDL_CloseGamepad(gamepad);
            }
            SDL_Quit();
        }
        ACTIVE.store(false, Ordering::SeqCst);
    }
}

fn button(b: SDL_GamepadButton) -> Option<Button> {
    Some(match b {
        SDL_GAMEPAD_BUTTON_SOUTH => Button::South,
        SDL_GAMEPAD_BUTTON_EAST => Button::East,
        SDL_GAMEPAD_BUTTON_WEST => Button::West,
        SDL_GAMEPAD_BUTTON_NORTH => Button::North,
        SDL_GAMEPAD_BUTTON_BACK => Button::Back,
        SDL_GAMEPAD_BUTTON_GUIDE => Button::Guide,
        SDL_GAMEPAD_BUTTON_START => Button::Start,
        SDL_GAMEPAD_BUTTON_LEFT_STICK => Button::LeftStick,
        SDL_GAMEPAD_BUTTON_RIGHT_STICK => Button::RightStick,
        SDL_GAMEPAD_BUTTON_LEFT_SHOULDER => Button::LeftShoulder,
        SDL_GAMEPAD_BUTTON_RIGHT_SHOULDER => Button::RightShoulder,
        SDL_GAMEPAD_BUTTON_DPAD_UP => Button::DPadUp,
        SDL_GAMEPAD_BUTTON_DPAD_DOWN => Button::DPadDown,
        SDL_GAMEPAD_BUTTON_DPAD_LEFT => Button::DPadLeft,
        SDL_GAMEPAD_BUTTON_DPAD_RIGHT => Button::DPadRight,
        SDL_GAMEPAD_BUTTON_MISC1 => Button::Misc1,
        SDL_GAMEPAD_BUTTON_RIGHT_PADDLE1 => Button::RightPaddle1,
        SDL_GAMEPAD_BUTTON_LEFT_PADDLE1 => Button::LeftPaddle1,
        SDL_GAMEPAD_BUTTON_RIGHT_PADDLE2 => Button::RightPaddle2,
        SDL_GAMEPAD_BUTTON_LEFT_PADDLE2 => Button::LeftPaddle2,
        SDL_GAMEPAD_BUTTON_TOUCHPAD => Button::Touchpad,
        _ => return None,
    })
}

fn axis(a: SDL_GamepadAxis) -> Option<Axis> {
    Some(match a {
        SDL_GAMEPAD_AXIS_LEFTX => Axis::LeftX,
        SDL_GAMEPAD_AXIS_LEFTY => Axis::LeftY,
        SDL_GAMEPAD_AXIS_RIGHTX => Axis::RightX,
        SDL_GAMEPAD_AXIS_RIGHTY => Axis::RightY,
        SDL_GAMEPAD_AXIS_LEFT_TRIGGER => Axis::LeftTrigger,
        SDL_GAMEPAD_AXIS_RIGHT_TRIGGER => Axis::RightTrigger,
        _ => return None,
    })
}

fn last_error() -> SdlError {
    // SAFETY: SDL_GetError always returns a valid NUL-terminated string.
    SdlError(unsafe { c_string(SDL_GetError()) })
}

/// Copies an SDL-owned C string. Null becomes an empty string.
///
/// # Safety
/// `ptr` must be null or point to a NUL-terminated string valid for the duration of the call.
unsafe fn c_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: guaranteed by the caller.
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xinput_paths_name_their_user_index() {
        assert_eq!(xinput_slot("XInput#0"), Some(0));
        assert_eq!(xinput_slot("XInput#3"), Some(3));
        assert_eq!(xinput_slot(r"\\?\HID#VID_054C&PID_09CC#7&1"), None);
        assert_eq!(xinput_slot("XInput#"), None);
        assert_eq!(xinput_slot("XInput#-1"), None);
        assert_eq!(xinput_slot("XInput#1x"), None);
        assert_eq!(xinput_slot("xinput#1"), None);
        assert_eq!(xinput_slot(""), None);
    }
}
