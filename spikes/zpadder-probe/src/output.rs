use std::io::Write;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use zpadder_model::{KeyCode, MouseButton, OutputAction};
use zpadder_output::{OutputSink, SendInputSink};
use zpadder_platform::foreground_window;

use crate::foreground::describe;
use crate::log::Log;

const USAGE: &str = "usage: output <keys|special|mouse|game> [--delay <seconds>]";

enum Step {
    Send(Vec<OutputAction>),
    Wait(Duration),
    /// Log the pointer's movement since the previous mark, against what was requested.
    Mark {
        label: &'static str,
        expected: (i32, i32),
    },
}

/// A sequence of output steps, built by chaining.
#[derive(Default)]
struct Script {
    steps: Vec<Step>,
}

impl Script {
    fn send(mut self, actions: impl Into<Vec<OutputAction>>) -> Self {
        self.steps.push(Step::Send(actions.into()));
        self
    }

    fn wait(mut self, ms: u64) -> Self {
        self.steps.push(Step::Wait(Duration::from_millis(ms)));
        self
    }

    fn mark(mut self, label: &'static str, expected: (i32, i32)) -> Self {
        self.steps.push(Step::Mark { label, expected });
        self
    }

    /// Presses and releases a key, holding it long enough for games that poll once per frame.
    fn tap(self, key: KeyCode) -> Self {
        self.send([OutputAction::KeyDown(key)]).wait(40).send([OutputAction::KeyUp(key)]).wait(40)
    }

    /// Holds `modifier` while tapping `key`.
    fn chord(self, modifier: KeyCode, key: KeyCode) -> Self {
        self.send([OutputAction::KeyDown(modifier)])
            .wait(20)
            .tap(key)
            .send([OutputAction::KeyUp(modifier)])
            .wait(40)
    }

    /// Types lowercase letters, digits and spaces.
    fn text(self, text: &str) -> Self {
        text.bytes().fold(self, |script, c| {
            let key = match c {
                b'a'..=b'z' => KeyCode(KeyCode::A.0 + u16::from(c - b'a')),
                b'1'..=b'9' => KeyCode(KeyCode::DIGIT_1.0 + u16::from(c - b'1')),
                b'0' => KeyCode::DIGIT_0,
                b' ' => KeyCode::SPACE,
                _ => unreachable!("unsupported character {c}"),
            };
            script.tap(key)
        })
    }

    fn mouse_moves(self, count: usize, dx: i32, dy: i32, interval_ms: u64) -> Self {
        (0..count).fold(self, |s, _| s.send([OutputAction::MouseMove { dx, dy }]).wait(interval_ms))
    }

    fn wheel(self, count: usize, vertical: i32) -> Self {
        (0..count)
            .fold(self, |s, _| s.send([OutputAction::Wheel { vertical, horizontal: 0 }]).wait(150))
    }
}

fn script(name: &str) -> Option<(&'static str, Script)> {
    let s = Script::default();
    Some(match name {
        "keys" => (
            "Focus Notepad. Expected text: 'zpadder 123', then a new line with a capital 'Z'.",
            s.text("zpadder 123").tap(KeyCode::ENTER).chord(KeyCode::LEFT_SHIFT, KeyCode::Z),
        ),
        "special" => (
            "Focus Notepad. Expected: a line 'axbc' and a line 'ok' (a '4' or '6' means arrow keys \
             lost their extended flag), the Num Lock light toggles off and on, then Print \
             Screen opens the capture overlay or copies a screenshot (press Esc). Pause has no \
             visible effect: check it with `keyhook`.",
            s.text("abc")
                .tap(KeyCode::LEFT)
                .tap(KeyCode::LEFT)
                .text("x")
                .tap(KeyCode::END)
                .tap(KeyCode::NUMPAD_ENTER)
                .text("ok")
                .wait(300)
                .tap(KeyCode::NUM_LOCK)
                .wait(700)
                .tap(KeyCode::NUM_LOCK)
                .wait(300)
                .tap(KeyCode::PAUSE)
                .wait(300)
                .tap(KeyCode::PRINT_SCREEN),
        ),
        "mouse" => (
            "Start with the pointer near the middle of the screen. Draws a 200 px square, then \
             makes 200 single-pixel moves right, then scrolls down 3 and up 3 notches. The log \
             compares requested and actual movement; a mismatch means Windows pointer \
             acceleration ('Enhance pointer precision') is scaling injected movement.",
            s.mark("start", (0, 0))
                .mouse_moves(20, 10, 0, 10)
                .mouse_moves(20, 0, 10, 10)
                .mouse_moves(20, -10, 0, 10)
                .mouse_moves(20, 0, -10, 10)
                .mark("200 px square (fast 10 px steps)", (0, 0))
                .mouse_moves(200, 1, 0, 5)
                .mark("200 x 1 px steps (slow)", (200, 0))
                .mouse_moves(10, 40, 0, 2)
                .mark("10 x 40 px steps (fast)", (400, 0))
                .wheel(3, -120)
                .wheel(3, 120),
        ),
        "game" => (
            "Focus a game where you can walk and look around. Holds W for 1 s, taps Space, \
             turns the camera right with 30 small mouse moves, then clicks the left button.",
            s.send([OutputAction::KeyDown(KeyCode::W)])
                .wait(1000)
                .send([OutputAction::KeyUp(KeyCode::W)])
                .wait(300)
                .send([OutputAction::KeyDown(KeyCode::SPACE)])
                .wait(100)
                .send([OutputAction::KeyUp(KeyCode::SPACE)])
                .wait(500)
                .mouse_moves(30, 10, 0, 16)
                .wait(300)
                .send([OutputAction::MouseButton { button: MouseButton::Left, pressed: true }])
                .wait(80)
                .send([OutputAction::MouseButton { button: MouseButton::Left, pressed: false }]),
        ),
        _ => return None,
    })
}

pub(crate) fn run(args: &[String]) -> ExitCode {
    let mut name = None;
    let mut delay = 5;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--delay" {
            match args.next().and_then(|v| v.parse().ok()) {
                Some(seconds) => delay = seconds,
                None => {
                    eprintln!("{USAGE}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            name = Some(arg.as_str());
        }
    }
    let Some((name, (description, script))) = name.and_then(|n| Some((n, script(n)?))) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    let mut log = Log::create("output");
    log.line(format_args!("Test '{name}'. {description}"));
    print!("Switch to the target window now. Starting in");
    for remaining in (1..=delay).rev() {
        print!(" {remaining}...");
        let _ = std::io::stdout().flush();
        thread::sleep(Duration::from_secs(1));
    }
    println!();
    match foreground_window() {
        Some(window) => log.line(format_args!("Target: {}", describe(&window))),
        None => log.line("Target: no foreground window"),
    }

    let mut sink = SendInputSink;
    let mut last_mark = cursor();
    for step in script.steps {
        match step {
            Step::Send(actions) => {
                if let Err(e) = sink.send(&actions) {
                    log.line(format_args!("SendInput failed: {e}"));
                }
            }
            Step::Wait(duration) => thread::sleep(duration),
            Step::Mark { label, expected } => {
                let now = cursor();
                let moved = (now.0 - last_mark.0, now.1 - last_mark.1);
                if label != "start" {
                    log.line(format_args!(
                        "{label}: requested {expected:?}, pointer moved {moved:?}"
                    ));
                }
                last_mark = now;
            }
        }
    }
    log.line("Done.");
    ExitCode::SUCCESS
}

fn cursor() -> (i32, i32) {
    let mut point = POINT::default();
    // SAFETY: `point` is a valid out-pointer.
    let _ = unsafe { GetCursorPos(&mut point) };
    (point.x, point.y)
}
