use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};
use zpadder_input::InputBackend;
use zpadder_input::sdl::{self, SdlBackend};
use zpadder_model::{Axis, Button, InputEvent, PadId};
use zpadder_platform::{foreground_window, foreground_window_id};

use crate::foreground::describe;
use crate::log::Log;

pub(crate) fn run() -> ExitCode {
    let mut log = Log::create("input");
    log.line(format_args!("SDL {}", sdl::version()));
    let mut backend = match SdlBackend::new() {
        Ok(backend) => backend,
        Err(e) => {
            log.line(format_args!("SDL failed to start: {e}"));
            return ExitCode::FAILURE;
        }
    };
    log.line("Connected pads are listed first. Press buttons and move sticks, then switch to a");
    log.line("game or another window and keep going: lines must keep appearing. North (Y /");
    log.line("Triangle) rumbles the pad. Ctrl+C stops.");

    let (xinput_tx, xinput_rx) = mpsc::channel();
    spawn_xinput_watcher(xinput_tx);

    let mut events = Vec::new();
    let mut axes: HashMap<(PadId, Axis), i8> = HashMap::new();
    let mut foreground = None;
    let mut unmapped: Vec<String> = Vec::new();
    let mut last_unmapped_check = None::<Instant>;
    loop {
        backend.poll(Duration::from_millis(10), &mut events);
        for event in events.drain(..) {
            match event {
                InputEvent::Connected { pad } => {
                    let Some(info) = backend.pad_info(pad) else { continue };
                    log.line(format_args!(
                        "pad {} connected: {} [{}, {:04x}:{:04x}] gyro: {}, rumble: {}, power: {:?}",
                        pad.0,
                        info.name,
                        info.kind,
                        info.vendor_id,
                        info.product_id,
                        yes_no(info.has_gyro),
                        yes_no(info.has_rumble),
                        info.power,
                    ));
                }
                InputEvent::Disconnected { pad } => {
                    log.line(format_args!("pad {} disconnected", pad.0));
                }
                InputEvent::Button { pad, button, pressed } => {
                    let state = if pressed { "down" } else { "up" };
                    log.line(format_args!("pad {} {button:?} {state}", pad.0));
                    if pressed && button == Button::North {
                        let ok = backend.rumble(pad, 0xC000, 0xC000, Duration::from_millis(300));
                        let result = if ok { "sent" } else { "not supported" };
                        log.line(format_args!("pad {} rumble {result}", pad.0));
                    }
                }
                InputEvent::Axis { pad, axis, value } => {
                    // Only report when the value crosses a quarter step, to keep the log readable.
                    let step = (value * 4.0).round() as i8;
                    if axes.insert((pad, axis), step) != Some(step) {
                        log.line(format_args!("pad {} {axis:?} {value:+.2}", pad.0));
                    }
                }
            }
        }

        while let Ok(line) = xinput_rx.try_recv() {
            log.line(line);
        }

        let id = foreground_window_id();
        if id != foreground {
            foreground = id;
            if let Some(window) = foreground_window() {
                log.line(format_args!("foreground: {}", describe(&window)));
            }
        }

        if last_unmapped_check.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
            last_unmapped_check = Some(Instant::now());
            let now = backend.unmapped_joysticks();
            for name in now.iter().filter(|name| !unmapped.contains(name)) {
                log.line(format_args!("joystick without a gamepad mapping (ignored): {name}"));
            }
            unmapped = now;
        }
    }
}

/// Polls XInput directly, independently of SDL, and reports connections and button changes.
/// Only Xbox-compatible pads appear here.
fn spawn_xinput_watcher(tx: Sender<String>) {
    thread::spawn(move || {
        let mut last: [Option<u16>; 4] = [None; 4];
        loop {
            for (slot, last) in (0u32..).zip(last.iter_mut()) {
                let mut state = XINPUT_STATE::default();
                // SAFETY: `state` is a valid out-pointer.
                let connected = unsafe { XInputGetState(slot, &mut state) } == 0;
                let buttons = connected.then_some(state.Gamepad.wButtons.0);
                if buttons == *last {
                    continue;
                }
                let msg = match (buttons, *last) {
                    (None, _) => format!("xinput slot {slot}: disconnected"),
                    (Some(_), None) => format!("xinput slot {slot}: connected"),
                    (Some(b), Some(_)) => format!("xinput slot {slot}: buttons {b:#06x}"),
                };
                *last = buttons;
                if tx.send(msg).is_err() {
                    return;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
    });
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
