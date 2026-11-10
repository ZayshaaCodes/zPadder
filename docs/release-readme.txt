ZPadder {version}
Map game controller input to keyboard and mouse.
================================================================================

ZPadder is a free, open-source replacement for Xpadder. It shows your controller,
lets you choose what each button, stick and trigger does, and sends those keys and
mouse actions to whatever program is in front. It opens Xpadder profiles directly.


GETTING STARTED

1. Unzip this folder anywhere and run zpadder.exe. No installation is needed.
2. Plug in or pair your controller. The editor shows a photo of it.
3. Click a control on the picture, or press it on your controller, then choose what
   it does on the right: click keys on the on-screen keyboard, or just press them on
   your real keyboard.
4. Try it in the Test bench (toolbar), or switch to Notepad or your game.
   Mapping pauses while the ZPadder window itself has focus, except in the Test
   bench's box.
5. File > Save keeps the profile. Edits apply as soon as you make them.

Starting points are under File > New from template, and in the profiles folder
(try profiles\notepad-test.toml with Notepad).

Closing the window keeps ZPadder running in the notification area (tray), where you
can switch mapping on and off or quit.


USEFUL TO KNOW

- Layers are the tabs above the controller. Any control can switch layers while
  held, one way, or back and forth.
- View > Layout > "Sheet" puts every label beside the controller instead of on it,
  and "Save as image" saves that as a cheat sheet.
- Mapping > AutoProfile switches profiles when a given program is in front.
  Program names can use * and ? wildcards, e.g. FiveM_*GTAProcess.exe.
- Mapping > Start with Windows starts ZPadder in the tray when you sign in.
- Edit > Undo (Ctrl+Z) takes back edits; File > Revert goes back to the saved file.
- If a game runs as administrator, Windows blocks ZPadder's input to it. ZPadder
  says so and offers to restart itself as administrator.
- Other controller software (Steam Input, DS4Windows, reWASD...) can make games get
  every press twice. ZPadder warns when it sees one running.
- Games with anti-cheat may block or flag synthesised input. Read
  docs\anti-cheat.md before using ZPadder in online games.


ACTING AS AN XBOX CONTROLLER (OPTIONAL)

Some games only understand Xbox controllers. Tick "Act as an Xbox controller" in
a profile (or start from File > New from template > Xbox controller), and games
see an Xbox 360 controller for each controller you connect, as with Steam Input.
Controls you haven't assigned act as its buttons, sticks and triggers.

This needs the free ViGEmBus driver. ZPadder works without it, and tells you in
the status bar when a profile needs it. To install it:

1. Open https://github.com/nefarius/ViGEmBus/releases (the "Get ViGEmBus"
   button in ZPadder opens it).
2. Download the setup program (.exe) of the latest release, 1.22.0, and run it.
3. Back in ZPadder, wait a few seconds: the status bar shows "Xbox controller:
   on". No restart is needed.

If Steam is running, switch off "Steam Input for Xbox controllers" in Steam's
controller settings. Otherwise, outside Steam games, Steam applies its desktop
layout to ZPadder's Xbox controller too, and the triggers and buttons also
click or press keys (in FiveM, for example, R2 set off push-to-talk).

Uninstall it from Windows Settings > Apps if you no longer want it. Some games
with anti-cheat refuse virtual controllers; see docs\anti-cheat.md.


COMMAND LINE

zpadder.exe accepts Xpadder's command line, so front-ends such as LaunchBox can
drive it. A second launch hands its arguments to the running ZPadder.

    zpadder.exe "profile.toml" "Game.lnk" /M   open a profile, start a game,
                                               stay in the tray
    zpadder.exe /C                             close ZPadder

zpadder-cli.exe has tools for scripts: run (map without a window), check (find
mistakes in a profile), import (convert Xpadder files) and schema. Run it
without arguments for details. Profiles are plain text (TOML), described in
docs\profiles.md; profile.schema.json gives editors such as VS Code completion.


WHERE ZPADDER KEEPS THINGS

Settings, imported controller pictures and the log (zpadder.log) are kept in
%APPDATA%\ZPadder. Help > Open log folder opens it.

Portable mode: create an empty file named portable.txt next to zpadder.exe, and
ZPadder keeps everything in this folder instead.

Profiles are wherever you save them.


PRIVACY

ZPadder makes no network connections and collects nothing. To do its job it reads
the name and title of the window in front (for AutoProfile and to pause in its own
window) and the names of running programs (to warn about other controller
software). Nothing leaves your computer.


UNINSTALLING

1. In ZPadder, untick Mapping > Start with Windows, then quit it from the tray.
2. Delete this folder.
3. Delete %APPDATA%\ZPadder if you don't want to keep your settings.


PROBLEMS AND LICENCES

To report a problem, attach zpadder.log (Help > Open log folder) and say which
controller you use and how it's connected.

ZPadder is dual-licensed under MIT or Apache-2.0 (LICENSE-MIT, LICENSE-APACHE).
It includes open-source software listed in THIRD_PARTY_LICENSES.html and the
licenses folder. Controller picture credits: docs\controller-pictures-credits.md.
ZPadder is not affiliated with Xpadder, Sony, Microsoft or Nintendo.
