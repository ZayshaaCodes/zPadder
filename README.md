# ZPadder

[![CI](https://github.com/ZayshaaCodes/zPadder/actions/workflows/ci.yml/badge.svg)](https://github.com/ZayshaaCodes/zPadder/actions/workflows/ci.yml)
[![Licence: MIT OR Apache-2.0](https://img.shields.io/badge/licence-MIT%20OR%20Apache--2.0-blue)](#licence)
[![Latest release](https://img.shields.io/github/v/release/ZayshaaCodes/zPadder?include_prereleases)](https://github.com/ZayshaaCodes/zPadder/releases)

ZPadder maps game controller input to keyboard and mouse. It is an open-source re-implementation
of [Xpadder](https://xpadder.com/), written in Rust: same picture-first workflow, same idea of
profiles built from slot timelines and switchable sets, and the ability to open existing
`.xpadderprofile` and `.xpaddercontroller` files.

It ships on Windows first. The core (data model, mapping engine, profile format and Xpadder
importer) has no OS dependencies, so Linux and macOS backends can follow.

> **Status: 0.3, ready for everyday use on Windows.** The editor, tray, live mapping,
> AutoProfile, Xpadder file import and the Xpadder-style command line work. Releases are not
> code signed yet, so SmartScreen may warn on first start. See [CHANGELOG.md](CHANGELOG.md)
> and the [roadmap](docs/roadmap.md).

## Getting it

Download the zip from the [latest release](https://github.com/ZayshaaCodes/zPadder/releases/latest), unzip it anywhere and run
`zpadder.exe`. There is no installer, and it needs no runtime. Its `README.txt` covers the
basics, including the optional ViGEmBus driver for "Act as an Xbox controller". To build it
yourself, see [Building](#building).

## Using it

- **See the whole mapping at a glance.** Every control is labelled with what it does, as a
  keycap ("Space", "Shift+X", "LMB"), with stick directions around each stick, like Xpadder.
  Colours tell keys, mouse, layer switches and timed actions apart; hover a label for details.
  View ▾ > "Show names instead of keys" labels controls with assignment names. The **sheet** layout (View ▾ >
  Layout) puts every label in a column beside the controller instead, joined to its control by
  a line, so nothing is shortened; "Save as image…" saves it as a cheat sheet.
- **Photos of real controllers** (DualShock 4, Xbox One/Series, Xbox 360, Switch Pro) are
  built in, with every control already placed. By default the editor shows the one matching
  your pad; View ▾ picks another, the drawn controller, or a picture from an Xpadder file.
- **Starter templates** (File > New from template): Desktop & browsing, First-person shooter,
  Platformer / arcade, a blank DualShock 4 profile, and Xbox controller (all games).
- **Act as an Xbox controller**, like Steam Input: a profile can present each controller to
  games as a virtual Xbox 360 controller, so a DualShock 4 works in games that only understand
  Xbox pads (GTA V, FiveM...). Controls with nothing assigned act as the Xbox controller's,
  assigned ones still send keys. Optional, per profile, and needs the free
  [ViGEmBus](https://github.com/nefarius/ViGEmBus/releases) driver; see
  [docs/profiles.md](docs/profiles.md#acting-as-an-xbox-controller).
- **Edit by clicking or pressing.** Click a control on the drawn controller, or just press it on
  your pad, and the inspector on the right shows what it does. Add keys from the on-screen
  keyboard ("Keyboard & mouse…"), where pressing keys on your real keyboard adds them too, or
  with "Capture key" (hold Ctrl, Shift or Alt first for a combination). Add timing (pauses,
  hold/release zones, distance zones, cycles) from the Timing menu. A plain-English summary
  shows what the assignment will do.
- **Edits apply at once.** The running mapper picks up every valid change; File > Save keeps it.
  Mapping pauses while the ZPadder window has focus, except in the **Test bench** (toolbar): type
  in its box with your pad to try the profile without switching windows. It lists what each press
  sent and why, shows the sticks and triggers live, and warns when Steam, DS4Windows or another
  remapper is running. The mouse stays still there; mouse actions are only listed.
- **Layers** are tabs above the controller. Any control can switch layers (while held, one-way
  or two-way); the layer active on the pad is highlighted.
- **Sticks** can be off, move the mouse (speed, response curve, invert) or act as 4/8
  direction buttons. **Triggers** fire past an adjustable pull.
- **Tray:** closing the window keeps ZPadder running in the notification area, where you can
  switch mapping on and off or quit.
- **Xpadder profiles open directly** (`.xpadderprofile`), converted on the fly, with the
  pack's controller picture when it has one. See [docs/xpadder-import.md](docs/xpadder-import.md).
- **AutoProfile** (Mapping menu) switches profiles when a given program has focus. Program
  names can use `*` and `?` wildcards (`FiveM_*GTAProcess.exe`).
- **Start with Windows** and **Start minimized** are in the Mapping menu.
- If a game runs as administrator, Windows blocks ZPadder's input to it; ZPadder says so and
  offers to restart itself as administrator.

Try [`profiles/examples/notepad-test.toml`](profiles/examples/notepad-test.toml) (File > Open)
with Notepad. Profiles are plain TOML ([docs/profiles.md](docs/profiles.md)), so they can also
be written by hand.

### Command line

`zpadder.exe` accepts Xpadder's command line, so front-ends such as LaunchBox can drive it. A
second launch hands its arguments to the running ZPadder.

```text
zpadder.exe "profile.toml" "Game.lnk" /M    open a profile, start a game, stay in the tray
zpadder.exe /C                              close ZPadder
```

`zpadder-cli.exe` has headless tools: `run` (map without a window, reloading the profile on
save), `check` (find mistakes in a profile), `import` (convert Xpadder files) and `schema`
(JSON schema for editor completion). Wherever a profile is expected, an `.xpadderprofile`
works too.

## Privacy

ZPadder makes no network connections and has no telemetry. To do its job it reads the name and
title of the window in front (for AutoProfile, and to pause in its own window) and the names of
running programs (to warn about other controller software). Nothing leaves your computer.

## Uninstalling

Untick Mapping > Start with Windows and quit ZPadder from the tray, then delete its folder, and
`%APPDATA%\ZPadder` if you don't want to keep your settings. In portable mode (an empty
`portable.txt` next to `zpadder.exe`) everything is in ZPadder's own folder.

## Problems and contributing

Help > Open log folder shows `zpadder.log`; attach it to bug reports. Set `ZPADDER_LOG=debug`
for more detail. Games with anti-cheat may block or flag synthesised input; see
[docs/anti-cheat.md](docs/anti-cheat.md). Contributions are welcome:
[CONTRIBUTING.md](.github/CONTRIBUTING.md). Security problems: [SECURITY.md](.github/SECURITY.md). Everyone
taking part is expected to follow the [code of conduct](.github/CODE_OF_CONDUCT.md). Report bugs and
ideas in the [issue tracker](https://github.com/ZayshaaCodes/zPadder/issues).

## Building

Requirements:

- Rust: the toolchain pinned in `rust-toolchain.toml` (rustup installs it); 1.95 is the
  minimum. Use the MSVC toolchain on Windows
  (`x86_64-pc-windows-msvc`; Visual Studio Build Tools provide the linker).
- Building SDL3 from source needs CMake on `PATH`. Visual Studio Build
  Tools bundle one in `<VS install>\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`.
  The compiler itself does not need to be on `PATH`: CMake and Cargo locate MSVC on their own.

```sh
cargo build                        # debug build of every crate
cargo run -p zpadder               # the app
cargo run -p zpadder --bin zpadder-cli -- run profiles/examples/notepad-test.toml
scripts/package.ps1                # release zip in dist/ (see .github/CONTRIBUTING.md)
```

To run a development copy beside an installed one without touching its settings, set
`ZPADDER_INSTANCE` (a separate single-instance slot) and `ZPADDER_DATA_DIR` (a separate
settings folder):

```powershell
$env:ZPADDER_INSTANCE = "dev"; $env:ZPADDER_DATA_DIR = "$PWD\target\dev-data"
cargo run -p zpadder
cargo test --workspace             # all tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check                   # licences and advisories (cargo install cargo-deny)
```

## Workspace layout

| Crate | Purpose | OS-specific? |
|---|---|---|
| [`zpadder-model`](crates/zpadder-model) | Pads, controls, keys, output actions, profiles | No |
| [`zpadder-engine`](crates/zpadder-engine) | Pure, deterministic mapping engine (`step(events, now)`) | No |
| [`zpadder-config`](crates/zpadder-config) | Native TOML profiles with format migrations | No |
| [`zpadder-import-xpadder`](crates/zpadder-import-xpadder) | Xpadder file importer | No |
| [`zpadder-input`](crates/zpadder-input) | Controller input backends (SDL3) | Yes |
| [`zpadder-output`](crates/zpadder-output) | Keyboard/mouse output sinks (SendInput) | Yes |
| [`zpadder-platform`](crates/zpadder-platform) | Foreground window, elevation, autostart, single instance | Yes |
| [`zpadder-runtime`](crates/zpadder-runtime) | Runs the mapper in the background with live status | Yes |
| [`zpadder-gui`](crates/zpadder-gui) | egui editor and tray | Yes |
| [`zpadder`](crates/zpadder) | `zpadder.exe` (the app) and `zpadder-cli.exe` | Yes |

CI builds the four core crates on Linux so they stay portable.

[`spikes/zpadder-probe`](spikes/zpadder-probe) is a throwaway tool for the Phase 0 hardware
tests; see its [README](spikes/zpadder-probe/README.md).

## Credits

The Xbox One, Xbox 360 and Switch Pro photos are public-domain studio shots by Evan-Amos, from
Wikimedia Commons. The DualShock 4 picture was made for ZPadder by its maintainer. See
[`assets/controllers/CREDITS.md`](assets/controllers/CREDITS.md).

## Background research

[docs/research.md](docs/research.md) covers Xpadder's features, its reverse-engineered file
formats, its technical limitations, modern alternatives and the reasoning behind the stack, with
sources.

## Licence

Dual-licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

ZPadder is not affiliated with Xpadder or its author.
