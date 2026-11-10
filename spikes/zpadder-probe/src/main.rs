//! Phase 0 probes: small experiments for the `docs/roadmap.md` risks that need real hardware.
//!
//! Windows only. Run `cargo run -p zpadder-probe -- help` for the list.

mod foreground;
mod input;
mod keyhook;
mod log;
mod output;
mod timer;

use std::process::ExitCode;

const HELP: &str = "\
zpadder-probe: Phase 0 experiments (see spikes/zpadder-probe/README.md)

  input             Log gamepad input (SDL3), raw XInput buttons, and the foreground window.
                    After switching to a game, keep pressing buttons: lines must keep coming.
                    North (Y / Triangle) rumbles the pad.
  output <test>     After a countdown, inject input into whichever window has focus.
                    Tests: keys, special, mouse, game.   Option: --delay <seconds> (default 5)
  keyhook           Log every key event Windows receives: scan code, extended flag, and
                    whether it came from hardware or ZPadder. Run it beside `output`.
  timer             Measure sleep and turbo-style timing precision.
  foreground        For 60 s, report the foreground window and whether it is elevated.

Every command also writes probe-<command>.log in the current folder.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("input") => input::run(),
        Some("output") => output::run(rest),
        Some("keyhook") => keyhook::run(),
        Some("timer") => timer::run(),
        Some("foreground") => foreground::run(),
        _ => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
    }
}
