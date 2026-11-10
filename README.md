# ZPadder

[![CI](https://github.com/ZayshaaCodes/zPadder/actions/workflows/ci.yml/badge.svg)](https://github.com/ZayshaaCodes/zPadder/actions/workflows/ci.yml)
[![Licence: MIT OR Apache-2.0](https://img.shields.io/badge/licence-MIT%20OR%20Apache--2.0-blue)](#licence)
[![Latest release](https://img.shields.io/github/v/release/ZayshaaCodes/zPadder?include_prereleases)](https://github.com/ZayshaaCodes/zPadder/releases)

ZPadder lets you play PC games with a controller: your buttons and sticks press keys and move the
mouse, and games can see an Xbox controller. It's a free, open-source replacement for
[Xpadder](https://xpadder.com/) on Windows, and it opens your Xpadder profiles.

## Download and get going

1. Download the zip from the [latest release](https://github.com/ZayshaaCodes/zPadder/releases/latest),
   unzip it anywhere and run `zpadder.exe`. There is no installer.
2. If Windows says "Windows protected your PC", choose **More info**, then **Run anyway**. The
   build isn't code signed yet, so SmartScreen doesn't know it.
3. Plug in your controller. Click a button on its picture, or press it. On the right, choose
   **Keyboard & mouse…** and click the keys it should press, or press them on your keyboard.
4. Switch to your game. **File > Save** keeps the profile.

Mapping pauses while ZPadder's window is in front, so it never types into itself; the
**Test bench** (toolbar) lets you try a profile there. For a head start, use
**File > New from template**, or try
[`profiles/examples/notepad-test.toml`](profiles/examples/notepad-test.toml) in Notepad. News is in
the [changelog](CHANGELOG.md), plans in the [roadmap](docs/roadmap.md).

## What it can do

- **See the whole mapping at a glance.** Each control shows what it does ("Space", "Shift+X",
  "LMB"), colour-coded, on a photo of your controller (DualShock 4, Xbox One/Series, Xbox 360,
  Switch Pro). **View ▾** changes the picture, or makes a cheat sheet you can save as an image.
- **Keys, combinations and mouse** (Space, Ctrl+Z, mouse buttons, the scroll wheel), with turbo,
  toggle or rumble.
- **Timing** (**Timing ▾**): sequences, tap or hold, actions on release, light or full trigger
  pull, cycles. "In plain words" sums up what you've set.
- **Sticks** move the mouse, scroll, or press 4 or 8 directions (WASD). **Triggers** count as
  pressed past a pull you set.
- **Layers**, like Xpadder's sets, are the tabs above the controller. Any control can switch
  layer while held, one-way or two-way.
- **AutoProfile** (Mapping > AutoProfile rules…) picks a profile when a game is in front.
  Wildcards such as `FiveM_*GTAProcess.exe` work.
- **Edits work at once.** Closing the window keeps ZPadder in the tray, and Mapping > Start with
  Windows starts it there.
- **Opens Xpadder profiles** ([how well](docs/xpadder-import.md)) and **works with LaunchBox**
  ([how](#use-with-launchbox-and-other-front-ends)).

## Xbox controller mode

Some games, such as GTA V and FiveM, only understand Xbox controllers. ZPadder can make them see
one, as Steam Input does:

1. Install the free [ViGEmBus](https://github.com/nefarius/ViGEmBus/releases) driver: run the
   setup program (.exe) from its latest release, 1.22.0. ZPadder never installs it for you.
2. Tick **Act as an Xbox controller** in the right-hand panel (shown when no control is selected;
   click the current layer's tab to get there), or use File > New from template > Xbox controller
   (all games).
3. Within about ten seconds the status bar shows "Xbox controller: on". No restart is needed.

- Controls you haven't assigned now work as the Xbox controller's buttons, sticks and triggers.
  Assigned ones still send their keys; tick **Also press the Xbox controller's …** to have one do
  both.
- **If Steam is running**, switch off "Steam Input for Xbox controllers" in Steam's controller
  settings. Otherwise Steam also turns ZPadder's Xbox controller into keys and clicks outside
  Steam games: in FiveM, R2 set off push-to-talk.
- Real Xbox controllers don't need this. Some anti-cheat refuses virtual controllers; see
  [anti-cheat](docs/anti-cheat.md) and the
  [details](docs/profiles.md#play-xbox-only-games-act-as-an-xbox-controller).

## If something doesn't work

- **"Windows protected your PC".** ZPadder isn't code signed yet. Choose **More info**, then
  **Run anyway**.
- **The game gets every press twice.** Another program reads your controller too, such as Steam
  Input, DS4Windows or reWASD; ZPadder's status bar shows "Also running: …". Switch off its
  mapping for this controller (in Steam, Steam Input for your type of controller), or close it.
- **It works in Notepad but not in the game.** The game probably runs as administrator. Windows
  then blocks ZPadder from controlling it unless ZPadder runs as administrator too, and the status
  bar offers **Restart ZPadder as administrator**. If not, anti-cheat may be blocking it.
- **Nothing happens while ZPadder's window is in front.** That's on purpose. Try the profile in
  the **Test bench** instead; the mouse stays still there, and mouse actions are only listed.
- **Online games.** ZPadder presses keys the way any keyboard macro tool does, and some anti-cheat
  may block or flag that. Read [docs/anti-cheat.md](docs/anti-cheat.md) first.
- **Anything else.** Help > Open log folder opens the folder with `zpadder.log`. Attach it to a
  [bug report](https://github.com/ZayshaaCodes/zPadder/issues) (Help > Report a problem…), and say
  which controller you use and how it's connected.

## Privacy

ZPadder makes no network connections and has no telemetry. To do its job it reads the name and
title of the window in front (for AutoProfile, and to pause in its own window) and the names of
running programs (to warn about other controller software). Nothing leaves your computer.

## Uninstalling

Untick Mapping > Start with Windows and quit ZPadder from its tray icon. Then delete its folder,
and `%APPDATA%\ZPadder` if you don't want to keep your settings. In portable mode (an empty
`portable.txt` next to `zpadder.exe`) everything is in ZPadder's own folder. If you installed
ViGEmBus only for ZPadder, remove it in Windows Settings > Apps.

## For developers

ZPadder is written in Rust. Its core (data model, mapping engine, profile format, Xpadder
importer) has no OS dependencies, so Linux and macOS backends can follow.
[CONTRIBUTING.md](.github/CONTRIBUTING.md) covers bug reports, building, code, the importer and
releases. Security problems: [SECURITY.md](.github/SECURITY.md). Everyone taking part is expected
to follow the [code of conduct](.github/CODE_OF_CONDUCT.md).

### Use with LaunchBox and other front-ends

`zpadder.exe` accepts Xpadder's command line, so front-ends such as LaunchBox can drive it. An
`.xpadderprofile` works too, and a second launch hands its arguments to the running ZPadder. Given
one profile per controller, as Xpadder allows, ZPadder uses the first for all of them.

```text
zpadder.exe "profile.toml" "Game.lnk" /M    open a profile, start a game, stay in the tray
zpadder.exe /C                              close ZPadder
```

### Building

You need the Rust toolchain pinned in `rust-toolchain.toml` (rustup installs it; the minimum is
`rust-version` in `Cargo.toml`), the MSVC toolchain (`x86_64-pc-windows-msvc`; Visual Studio Build
Tools provide the linker), and CMake on `PATH` to build SDL3. Build Tools bundle CMake in
`<VS install>\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`; the compiler itself needn't
be on `PATH`. Then run `cargo build`. Before `cargo run -p zpadder` beside a ZPadder you use, set
`ZPADDER_INSTANCE` and `ZPADDER_DATA_DIR` so the copy gets its own instance slot and settings.
CONTRIBUTING.md shows how, with the test commands and `scripts/package.ps1` (the release zip).

`ZPADDER_LOG=debug` makes the log more detailed. `zpadder-cli.exe` has tools without a window:
`run` (map with a profile, picking up saved edits), `check` (find mistakes in a profile), `import`
(convert Xpadder files) and `schema` (JSON schema for editor completion). Profiles are plain TOML,
described in [docs/profiles.md](docs/profiles.md).

### Workspace layout

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
[`spikes/zpadder-probe`](spikes/zpadder-probe) is a throwaway tool for the Phase 0 hardware tests
([README](spikes/zpadder-probe/README.md)). [docs/research.md](docs/research.md) covers Xpadder's
features, file formats and limitations, modern alternatives and the reasoning behind the stack.

## Credits

The Xbox One, Xbox 360 and Switch Pro photos are public-domain studio shots by Evan-Amos, from
Wikimedia Commons. The DualShock 4 picture was made for ZPadder by its maintainer. See
[`assets/controllers/CREDITS.md`](assets/controllers/CREDITS.md).

## Licence

Dual-licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
ZPadder is not affiliated with Xpadder or its author.
