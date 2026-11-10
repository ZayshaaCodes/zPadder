# ZPadder roadmap

The goal is a free, maintained, Steam-independent controller mapper that opens existing Xpadder
profiles on day one, then adds what Xpadder never had: editable curves, unlimited layers, chords,
gyro aiming and flick stick.

The plan comes from the research in
[research.md](research.md).
Section references below (§) point into that report.

## Decisions

| Area | Choice | Why |
|---|---|---|
| Language | Rust stable, MSRV 1.95 | Native, low-latency, single exe; MSRV set by egui |
| Platforms | Windows first; OS-agnostic core | Core crates are built on Linux CI from day one |
| Input | SDL3 through a thin in-house wrapper over `sdl3-sys` (Zlib) | Only option covering XInput, DirectInput, DS4, DualSense and Switch Pro, plus gyro, touchpad, rumble and battery. The safe `sdl3` crate is pre-1.0. `gilrs` is ruled out: on Windows it needs a focused window |
| Output | `SendInput` via `windows` 0.62: scan codes plus extended flag, one batch per tick | Reaches DirectInput games, layout-independent, no kernel driver. `enigo` calls itself early alpha |
| GUI | egui/eframe 0.37, plus `tray-icon` and `muda` | Fits the picture-first editor, MIT/Apache, AccessKit by default. Slint's licence would block free signing |
| Profiles | TOML with `format_version` and migrations | Hand-editable; keeping comments on save (`toml_edit`) is planned. Xpadder files are import-only |
| Licence | MIT OR Apache-2.0 | Lets us reuse MIT code (JoyShockMapper, GamepadMotionHelpers, VIIPER clients), and keeps us eligible for SignPath's free code signing. GPL projects are for study only |

## Principles

1. **Xpadder's behaviour is the spec.** Its slot timelines, set selectors and stick modes have
   been tested by users for 20 years. The data model starts as a strict superset of Xpadder's,
   so import stays faithful.
2. **The engine is pure.** `Engine::step(events, now, out)` never reads the clock or touches the
   OS, so every timing behaviour can be tested deterministically with a virtual clock.
3. **No stuck keys, ever.** Track every held output, and release them all on profile switch,
   disable, unplug, crash-recovery and exit.
4. **Never disguise injected input.** Turbo and macros are opt-in and clearly labelled. Virtual
   pad output is an optional, removable module. ZPadder does not try to evade anti-cheat.
5. **Trust is a feature.** Signed builds, one canonical download site and more than one
   maintainer. Xpadder's name has been used for malware mirrors, and DS4Windows split into
   competing forks; ZPadder should avoid both.
6. **The core stays portable.** `zpadder-model`, `zpadder-engine`, `zpadder-config` and
   `zpadder-import-xpadder` never depend on OS crates.

## Phase 0: Spikes (retire the big risks)

Each risk ends in a written go/no-go in [the probe tool's README](../spikes/zpadder-probe/README.md).
Checks that need real controllers or games are run by hand by the maintainer, using
`zpadder-probe` and the checklist in that doc.

- [x] Build SDL3 through `sdl3-sys` (bundled, static, gamepad/joystick/sensor/haptic only)
      locally. On `windows-latest` it is still to be confirmed once a remote exists.
- [ ] Confirm SDL3 keeps delivering input while a fullscreen game has focus on Windows 11, with
      `SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1`. Test Xbox (XInput), DualShock 4, DualSense,
      Switch Pro and a generic DirectInput pad. *(Test 1)*
- [x] Find out whether SDL's RAWINPUT driver is on by default: it is **off** in SDL 3.4.18, and
      Xbox pads go through XInput.
- [ ] Check that two or more Xbox One pads work in the background, which breaks Xpadder
      (§ DirectInput reading). *(Test 1, step 7)*
- [ ] Confirm scan-code `SendInput` reaches a DirectInput or raw-input fullscreen game. Measure
      mouse movement with "Enhance pointer precision" on and off. *(Test 3)*
- [ ] Reproduce UIPI's silent drop against an elevated target *(Test 4)*. Elevation detection
      is prototyped in `zpadder-platform`.
- [x] Measure sleep and timer jitter for a console process: ~0.5 ms overshoot, and deadline
      scheduling holds turbo timing exactly. Under game load: *(Test 5)*. For a tray app with a
      hidden window: re-measure in Phase 1.
- [ ] Check how SDL3 coexists with Steam Input, DS4Windows and HidHide, and whether SDL's
      exclusive DirectInput access blocks games. *(Test 2)*
- [x] XInput-only fallback: not needed as a separate backend, because SDL already reads Xbox
      pads through XInput. The probe polls XInput directly as a cross-check.

**Exit:** a written go/no-go for every risk.

**Closed on 2026-11-03.** The maintainer judged the hardware checks sufficient to proceed. The
unticked items above were not individually recorded and stay open as watch items: if one of
them shows up as a problem later, its probe test in `spikes/zpadder-probe` is the place to
start.

## Phase 1: Headless core (v0.1)

**Model** (`zpadder-model`)
- [x] Profile → unbounded list of layers → assignment per control.
- [x] Assignment timeline steps:
  - [x] outputs: keys, mouse buttons, wheel (pointer movement is a stick mode);
  - [x] timing: Pause, Hold Zone, Release Zone, Distance Zone, Tap Time, Cycle Stop. Xpadder's
        own spellings are accepted, and the semantics are pinned down in `docs/profiles.md`.
- [x] Modifiers: Toggle and Turbo (with rate). Rumble is stored; the engine acts on it in
      Phase 4.
- [x] Layer selectors: one-way, two-way and while-held. They switch on press, and while-held
      layers stack.

**Engine** (`zpadder-engine`)
- [x] Button → key or mouse button. Outputs are reference-counted, and a press owns its release.
- [x] Stick → mouse (relative "Normal" mode): dead zone, speed, invert, sub-pixel accumulation.
- [x] Stick → 4-way or 8-way keys, with Diagonal Size and dead-zone hysteresis. (The DPad is
      four ordinary buttons.)
- [x] Trigger → key, with a threshold and hysteresis.
- [x] Full timeline execution: sequences, tap-or-hold, hold zones, release zones, distance-zone
      gradation, cycles. Overdue sequences never put a tap's press and release in one batch.
- [x] Tests: scenario tests on a virtual clock, plus a `proptest` property (including timing
      slots): no key goes down twice or up while up, everything is up after `release_all`, and
      the engine goes idle.

**Platform and binary**
- [x] SDL3 input backend (`zpadder-input`).
- [x] `SendInput` sink with a HID-usage → set-1 scan code table (`zpadder-output`).
- [x] Input and engine threads connected by `std::sync::mpsc`.
- [x] Load and save TOML v1 profiles (`zpadder-cli run`, `zpadder-cli check`). The running profile
      reloads when its file is saved, and a broken save keeps the old profile running.
- [x] JSON schema (`zpadder-cli schema`, committed as `schema/profile.schema.json`), so editors can
      complete key names and flag typos. A test fails if the committed schema is stale.
- [x] Logging with `tracing`; `--verbose` shows each button press and the keys it produced.
- [x] Windows manifest: asInvoker, per-monitor-v2 DPI, UTF-8, long paths.
- [x] Portable zip build (`scripts/package.ps1`).
- *Moved to Phase 3:* tray icon and its UI/OS-events threads. The tray shares the editor's
  event loop, so building it on a separate loop now would mean building it twice.
- *Moved to Phase 3:* the application icon, once there is artwork.
- *Moved to Phase 3:* keeping comments when ZPadder saves a profile (`toml_edit`). Only the
  editor writes profiles.

**Exit:** a hand-written profile drives Notepad and one real game (your test). The core passes
on Linux CI (pending a GitHub remote), and the no-stuck-keys property holds.

## Phase 2: Xpadder import (v0.2)

Import comes early on purpose. The Xpadder file corpus becomes our test suite, and it stops the
data model drifting away from Xpadder's (§ file formats). Details and every assumption:
[`docs/xpadder-import.md`](xpadder-import.md).

- [x] Decode controller images (`ImageLine` run-length encoding) to PNG.
- [x] Tolerant reader: Windows-1252 (with BOM and UTF-8 re-saves honoured), any line endings,
      indented lines, section-less 5.x files, glued headers, repeated and unknown keys; files
      identified by `DataType` and key shapes. Never panics.
- [x] `.xpaddercontroller` → layout (positions) + PNG picture + button order, inferred from the
      `[Names]` section or a preset (Xbox, DualShock 4).
- [x] `.xpadderprofile` → profile: sets to layers, every slot token, zero-based `SetSelect`,
      while-held pairs, two-way toggles, mouse sticks, dead zones, diagonals, triggers, turbo,
      rumble. Anything not representable becomes a note naming the set and control.
- [x] Key-name table built by physical key.
- [x] Corpus check: all 239 public files (19 controllers, 220 profiles) import with no panics
      and no validation failures; 174 profiles import with no notes. Fixtures in the repo are
      synthetic.
- [x] Opening `.xpadderprofile` anywhere a profile is accepted: the app (with the pack's
      picture), `zpadder.exe` arguments, AutoProfile rules, and `zpadder-cli run/check/import`.
- [ ] Import by pasting from the clipboard.
- [ ] Presets for PS2 adapters and PS3 DirectInput drivers.
- [ ] Confirm the assumptions (zone timing, TurboRate/MouseSpeed units, selector pairing,
      marker offset) against behaviour reported by Xpadder users.

**Exit:**
- every corpus file parses without a panic (done);
- selected corpus profiles behave the way their READMEs and the Xpadder forum walkthroughs
  describe, confirmed by hand with a real controller.

## Phase 3: Visual editor (v0.3)

- [ ] Controller canvas:
  - [x] a drawn controller whose labels follow the connected pad (Xbox letters, PlayStation
        shapes, Nintendo letters), with extra buttons (paddles, Misc) shown once used;
  - [ ] imported 512×256 magenta-keyed Xpadder art (with Phase 2), or new PNG/SVG art;
  - [ ] hotspots placed by dragging, for custom layouts;
  - [x] press a control on the pad to select it.
- [x] Assignment editor:
  - on-screen keyboard and mouse;
  - capture a real key;
  - slot list with reordering;
  - Toggle, Turbo and Rumble, a name field, and layer switching.
- [x] Advanced assignment: timing slots added from a menu and edited in place, and a
      plain-English summary (Xpadder's "schedule text"). Testing happens live in another window,
      since mapping pauses while ZPadder has focus.
- [x] Layer bar (add, rename, reorder, delete; renames and deletes update selectors); stick
      settings (mode, dead zone, speed, curve, invert, 4/8-way, diagonal size, per-direction
      assignments); trigger threshold; live pressed-control overlay, stick positions and trigger
      pull.
- [x] Live dead-zone overlay on the stick settings.
- [x] Built-in controller photos with every control placed: DualShock 4, Xbox One/Series,
      Xbox 360, Switch Pro (Xbox and Switch photos public domain by Evan-Amos). The one matching
      the connected pad is shown by default.
- [x] DualShock 4 picture made for ZPadder by its maintainer, so every built-in picture has a
      confirmed licence (`assets/controllers/CREDITS.md`).
- [ ] A DualSense (PS5) photo: no freely licensed one was found yet.
- [x] Starter profile templates: Desktop & browsing, First-person shooter, Platformer / arcade,
      Blank DualShock 4.
- [x] Callout sheet: every label in a column beside the controller, joined by a line, never
      shortened; saved as a PNG cheat sheet.
- [x] Test bench: mapping stays on in a box inside ZPadder (keys only; the mouse stays still),
      with a log of what each press sent and why, live sticks and triggers, and a warning when
      another remapper (Steam, DS4Windows, reWASD…) is running.
- [x] Keys pressed on the real keyboard are added while the on-screen keyboard is open; "Capture
      key" takes Ctrl, Shift or Alt combinations.
- [x] Unsaved-changes marker in the title, Recent profiles, Ctrl+S, and a prompt before
      losing changes.
- [ ] Saving keeps the comments and layout of hand-edited profiles (`toml_edit`).
- [x] Tray icon sharing the editor's event loop: Open, Mapping enabled, Quit. Closing the window
      leaves ZPadder in the tray.
- [ ] Profile picker in the tray menu.
- [x] Application icon, drawn in code until there is artwork; the same drawing is the exe's
      icon (`assets/zpadder.ico`), with version information.

**Exit:** a new user builds a layout and a profile without touching text.

## Phase 4: Daily driver → signed 1.0

- [x] AutoProfile: rules match a program (and optionally a window title); the open profile is
      the fallback; ZPadder's own window is ignored; rules are edited in the app. Program names
      take `*` and `?` wildcards.
- [ ] AutoProfile driven by `SetWinEventHook` instead of the input thread's cheap polling of
      the foreground window; rules per controller.
- [x] Xpadder-compatible command line: `zpadder "P1" "Shortcut.lnk" /M /C`, with
      single-instance forwarding over a named pipe, so a second launch drives the running one.
- [ ] Per-controller profiles (`"P1" "P2"`); today the first profile applies to every pad.
- [x] Start with Windows through the HKCU Run key (starts minimized).
- [ ] Elevated autostart through Task Scheduler.
- [x] Warn when the foreground app is elevated and ZPadder isn't (UIPI), with a one-click
      restart as administrator.
- [x] Rumble per assignment; battery display.
- [x] Settings next to the exe when `portable.txt` is present, otherwise in `%APPDATA%\ZPadder`.
- [x] Production hardening (0.3 audit):
  - keys released if mapping crashes, on lock screens and sleep, and when a key-up fails;
  - single instance that survives an elevated restart, with a pipe locked to the user;
  - DLL search restricted to System32;
  - a log file, a panic hook and error boxes;
  - atomic saves, unsaved-change recovery and undo;
  - size and range limits on untrusted files.
- [x] Release engineering:
  - static C runtime, version information and an exe icon;
  - third-party licence notices;
  - a zip with checksums;
  - CI with an MSRV job;
  - a tag-triggered draft-release workflow;
  - Dependabot;
  - CHANGELOG, SECURITY, CONTRIBUTING and issue templates;
  - privacy and uninstall notes;
  - [docs/anti-cheat.md](anti-cheat.md) and the [release checklist](../.github/CONTRIBUTING.md#releasing).
- [ ] Release pipeline, remaining (needs a GitHub remote and the maintainer):
  - SignPath Foundation signing (the step is marked in `release.yml`);
  - an MSI or per-user installer, if wanted beside the portable zip;
  - one canonical download page.

**Exit:** ZPadder replaces Xpadder in an existing LaunchBox or RocketLauncher setup without
other changes.

## Phase 5: Beyond Xpadder (1.x)

- [x] Editable response curve for stick-to-mouse (Xpadder's most requested missing feature).
- [ ] Outer dead zone, time-based acceleration.
- [x] Scroll stick (a stick that scrolls like a wheel).
- [ ] Per-axis mouse speed (also covers horizontal-only mouse sticks), and sticks that move the
      mouse on one axis while pressing keys on the other. Both appear in imported profiles.
- [ ] Per-trigger thresholds, and a profile-wide default tap time (Xpadder's `KeyTapTime`).
- [ ] Absolute "Cursor" and "Spring" mouse modes.
- [ ] Spring mode with configurable radius and pivot, plus reliable return to centre.
- [x] While-held layers form a stack, which fixes Xpadder's while-held glitch.
- [ ] Layers shared across controllers.
- [ ] Activators: double-tap, long press, start, release and chord, with fire delays.
- [ ] Gyro → mouse, using a port of GamepadMotionHelpers (MIT) for calibration; flick stick;
      touchpad regions.
- [ ] Multiple DPads; DirectInput devices such as wheels, HOTAS and dance mats, through SDL's
      joystick layer.
- [ ] Radial menus and an on-screen keyboard overlay.
- [ ] AccessKit labels for every canvas hotspot.

**Exit:** the core aiming features match JoyShockMapper and Steam Input on DualShock 4,
DualSense and Switch Pro.

## Phase 6: Optional modules and other platforms

- [x] Virtual Xbox 360 controller per pad through ViGEmBus (MIT `vigem-client` crate), opt-in
      per profile (`virtual_pad = "xbox360"`): unassigned controls pass through, assigned ones
      send keys, rumble comes back from games, and ZPadder never reads its own virtual pads.
      Needs the user-installed ViGEmBus driver.
- [ ] Hide the physical pad from games (HidHide), so a game that reads both sees only the
      virtual one; a DualShock 4 virtual target (ViGEm has one).
- [ ] Virtual-pad output through VIIPER (MIT `viiper-client` crate), for when ViGEmBus, now
      retired upstream, stops working. Get a legal review of running VIIPER's GPL-3.0 core as
      a separate process.
- [ ] Linux: `evdev`/uinput output, a udev rule, foreground detection (X11 first; Wayland is
      limited).
- [ ] macOS: CGEvent output and onboarding for the Accessibility permission.

**Exit:** no double input in games with native pad support, and the core runs unchanged on
three operating systems.

## Risks

| Risk | Impact | Mitigation | Retired in |
|---|---|---|---|
| Background input unreliable on Windows 11 | Fatal for a mapper | Spike SDL3 hints, then an XInput fallback | Phase 0 |
| `sdl3-sys` build or binding problems | Blocks input | Thin wrapper over a small API surface; fall back to the prebuilt `SDL3.dll` | Phase 0 |
| UIPI silently drops input to elevated games | "Doesn't work" reports | Detect, warn, offer elevated restart or Task Scheduler | Phase 0 / 4 |
| Timer throttling for tray-only processes | Turbo and macros drift | Deadline scheduling on high-resolution waitable timers; measure | Phase 0 |
| Anti-cheat flags injected input | Users get banned | No disguising; opt-in macros; clear docs; no driver by default | Phase 4 |
| Xpadder semantics guessed wrong (no Xpadder copy to compare against) | Imported profiles misbehave | Infer from the corpus and forum docs; record every assumption; make uncertain values (e.g. mouse speed scale) adjustable after import; hand-test with real hardware | Phase 2 |
| Single-maintainer burnout | Project dies like its rivals | Second maintainer before 1.0; contributor docs | Phase 4 |

## Open questions

There is no working Xpadder install to check these against. Each one is settled by inference
from the file corpus and the forum documentation. The choice is written down in the importer
next to the code that depends on it, and the value is made user-adjustable where getting it
wrong would be noticeable:

- What units are `MouseSpeed`, `TurboRate` and `KeyTapTime` in, and how does `MouseSpeed=32`
  translate to pixels per second?
- Which keys does `Xpadder.ini` contain? AutoProfile rules probably live there.
- Exactly how are while-held and two-way set selectors encoded (`SetSelect` plus `Toggle`)?
- Which slot tokens are used for Right Alt, Print Screen, Pause, mouse buttons 4 and 5, and media
  keys?
- How exactly do `HOLD ZONE`, `RELEASE ZONE` and `CYCLE STOP` interact in edge cases?

## Out of scope

- Input on the lock screen or the UAC secure desktop, which user-mode injection cannot reach.
- Hiding injected input from games or anti-cheat.
- Writing files in Xpadder's format. Import is one-way.
