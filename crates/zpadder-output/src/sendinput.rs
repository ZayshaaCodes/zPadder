//! Windows output through `SendInput`.

use std::io;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSE_EVENT_FLAGS,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, SendInput, VIRTUAL_KEY,
    VK_PAUSE,
};
use zpadder_model::{KeyCode, MouseButton, OutputAction};

use crate::{OutputSink, SendError, scancode};

/// Value placed in `dwExtraInfo` of every injected event ("ZPAD"), so hooks can tell ZPadder's
/// events apart from real hardware.
pub const EXTRA_INFO_TAG: usize = 0x5A50_4144;

const XBUTTON1: u32 = 0x0001;
const XBUTTON2: u32 = 0x0002;

/// Injects keyboard and mouse input with `SendInput`.
///
/// Keys are sent as scan codes, so they work regardless of keyboard layout and reach games that
/// read DirectInput. Windows silently drops injected input aimed at a window with a higher
/// integrity level (an elevated game) — `send` still reports success in that case.
#[derive(Debug, Default)]
pub struct SendInputSink;

impl OutputSink for SendInputSink {
    fn send(&mut self, actions: &[OutputAction]) -> Result<(), SendError> {
        let mut inputs = Vec::with_capacity(actions.len());
        // How many INPUTs there are once each action is added, to tell which actions went out
        // when SendInput inserts only some of them.
        let mut ends = Vec::with_capacity(actions.len());
        for action in actions {
            push_inputs(*action, &mut inputs);
            ends.push(inputs.len());
        }
        if inputs.is_empty() {
            return Ok(());
        }
        // SAFETY: `inputs` is a slice of fully initialised INPUT values and `cbsize` is the size
        // of one element, as SendInput requires.
        let inserted = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) } as usize;
        if inserted == inputs.len() {
            return Ok(());
        }
        // Read right away, before anything else can overwrite the thread's last error.
        let error = match inserted {
            0 => io::Error::last_os_error(),
            n => io::Error::other(format!("SendInput inserted {n} of {} events", inputs.len())),
        };
        Err(SendError { sent: fully_sent(&ends, inserted), error })
    }
}

/// How many leading actions went out in full when SendInput inserted `inserted` events, given
/// how many events there are once each action is added.
fn fully_sent(ends: &[usize], inserted: usize) -> usize {
    ends.partition_point(|&end| end <= inserted)
}

/// Appends the `INPUT`s for `action`. Keys with no Windows mapping are skipped; profiles are
/// validated on load, so none should reach this point.
fn push_inputs(action: OutputAction, out: &mut Vec<INPUT>) {
    match action {
        OutputAction::KeyDown(key) => out.extend(key_input(key, false)),
        OutputAction::KeyUp(key) => out.extend(key_input(key, true)),
        OutputAction::MouseButton { button, pressed } => {
            let (flags, data) = match (button, pressed) {
                (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
                (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
                (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
                (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
                (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
                (MouseButton::X1, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
                (MouseButton::X1, false) => (MOUSEEVENTF_XUP, XBUTTON1),
                (MouseButton::X2, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
                (MouseButton::X2, false) => (MOUSEEVENTF_XUP, XBUTTON2),
            };
            out.push(mouse_input(0, 0, data, flags));
        }
        OutputAction::MouseMove { dx, dy } => out.push(mouse_input(dx, dy, 0, MOUSEEVENTF_MOVE)),
        OutputAction::Wheel { vertical, horizontal } => {
            // mouseData is a DWORD holding a signed delta.
            if vertical != 0 {
                out.push(mouse_input(0, 0, vertical as u32, MOUSEEVENTF_WHEEL));
            }
            if horizontal != 0 {
                out.push(mouse_input(0, 0, horizontal as u32, MOUSEEVENTF_HWHEEL));
            }
        }
    }
}

fn key_input(key: KeyCode, up: bool) -> Option<INPUT> {
    // Pause has no single scan code (E1 1D 45), so it goes by virtual key. Everything else goes by
    // scan code, which Windows translates to a virtual key using the active layout.
    let (vk, code, mut flags) = if key == KeyCode::PAUSE {
        (VK_PAUSE, 0x45, KEYBD_EVENT_FLAGS(0))
    } else {
        (VIRTUAL_KEY(0), scancode::set1(key)?, KEYEVENTF_SCANCODE)
    };
    if code & 0xFF00 == 0xE000 {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    Some(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: code & 0xFF,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: EXTRA_INFO_TAG,
            },
        },
    })
}

fn mouse_input(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: EXTRA_INFO_TAG,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_sends_count_whole_actions_only() {
        // A key (1 event), a key with no mapping (0), a two-way wheel (2) and a key (1).
        let ends = [1, 1, 3, 4];
        assert_eq!(fully_sent(&ends, 0), 0);
        // The unmapped key had nothing to send, so it went out along with the first.
        assert_eq!(fully_sent(&ends, 1), 2);
        // Half the wheel is not the whole wheel.
        assert_eq!(fully_sent(&ends, 2), 2);
        assert_eq!(fully_sent(&ends, 3), 3);
        assert_eq!(fully_sent(&ends, 4), 4);
    }

    #[test]
    fn every_key_and_button_is_one_event() {
        // The tracking sink relies on this: a key or button either went out or did not.
        let mut inputs = Vec::new();
        push_inputs(OutputAction::KeyDown(KeyCode::A), &mut inputs);
        push_inputs(OutputAction::KeyUp(KeyCode::PAUSE), &mut inputs);
        push_inputs(
            OutputAction::MouseButton { button: MouseButton::X2, pressed: true },
            &mut inputs,
        );
        assert_eq!(inputs.len(), 3);
    }
}
