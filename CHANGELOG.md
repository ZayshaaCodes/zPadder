# Changelog

All notable changes to ZPadder are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and ZPadder uses
[semantic versioning](https://semver.org/).

## [Unreleased]

## [0.3.0]

The first public release: a usable replacement for Xpadder on Windows. Builds are not yet code
signed, so Windows SmartScreen may warn before the first start.

### Acting as an Xbox controller

- **Act as an Xbox controller.** A profile can present each controller to games as a virtual
  Xbox 360 controller, as Steam Input does, so a DualShock 4 works in games that only
  understand Xbox pads, such as GTA V and FiveM started outside Steam. Controls with nothing
  assigned in the active layer act as the Xbox controller's buttons, sticks and triggers;
  assigned ones keep sending their keys. Rumble from games reaches the controller. It is off by
  default (`virtual_pad = "xbox360"` in profile files, a checkbox in the editor) and needs the
  free ViGEmBus driver, which ZPadder finds by itself once installed. The format version stays 1.
- Template "Xbox controller (all games)": nothing assigned, everything goes to the Xbox
  controller.
- "Also press the Xbox controller's …" (`passthrough = true`): an assigned button or trigger
  sends its keys and also works as its Xbox counterpart; a stick can move the mouse and the Xbox
  controller's stick at once. Such labels carry a "+" badge.
- AutoProfile program names can use `*` and `?` wildcards, so a rule such as
  `FiveM_*GTAProcess.exe` survives game updates.
- While Steam runs, a reminder that Steam applies its desktop layout to ZPadder's Xbox
  controller outside Steam games, and how to switch that off.

### Mapping

- Buttons, sticks and triggers map to keys, key combinations, mouse buttons, the wheel and
  pointer movement.
- Each assignment is a timeline of slots, as in Xpadder: pauses, hold and release zones,
  distance zones for triggers, tap times and cycles. It also offers turbo, toggle and rumble.
- Sticks can move the mouse (speed, response curve, invert), scroll, or act as 4- or 8-way
  buttons, with a dead zone.
- Layers can be switched while held, one way, or back and forth.
- Mapping uses SDL 3, so it keeps working in the background, and sends input through
  `SendInput` with scan codes.
- It pauses while ZPadder has focus and warns when a game runs as administrator.
- Held keys are released when a controller disconnects, when the profile changes, when mapping
  stops, and if mapping fails.

### Editor

- The editor shows a photo of your controller: DualShock 4, Xbox One/Series, Xbox 360 or Switch
  Pro. Every control is labelled with what it does, or you can use a drawn controller or a
  picture from an Xpadder file.
- The sheet layout puts every label beside the controller and saves it as an image.
- Pressing a control on your pad selects it. Keys can be picked on the on-screen keyboard or by
  pressing them on your real keyboard.
- The Test bench tries the profile inside ZPadder. It lists what each press sent and why, shows
  live sticks and triggers, and warns about other controller software.
- Starter templates: Desktop & browsing, First-person shooter, Platformer / arcade, and Blank
  DualShock 4.
- Undo, Revert to saved and a prompt before losing changes.
- AutoProfile rules choose a profile by program and window title.
- Tray icon, Start with Windows, and start minimised.

### Compatibility

- Xpadder profiles (`.xpadderprofile`) open directly, and `.xpaddercontroller` pictures can be
  imported.
- Xpadder's command line works (`"profile" "game.lnk" /M`, `/C`), so front-ends such as LaunchBox
  can drive ZPadder.
- `zpadder-cli` can map without a window, check profiles, convert Xpadder files and write the
  profile JSON schema.

### Packaging

- A portable zip that needs no Visual C++ runtime, includes third-party licence notices and comes
  with SHA-256 checksums. `portable.txt` keeps settings next to the exe.
- A log file (`zpadder.log`) for bug reports.

[Unreleased]: https://github.com/ZayshaaCodes/zPadder/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/ZayshaaCodes/zPadder/releases/tag/v0.3.0
