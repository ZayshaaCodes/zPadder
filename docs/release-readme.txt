ZPadder {version}
Play PC games with a controller.
================================================================================

ZPadder is a free, open-source replacement for Xpadder. It shows your
controller, lets you choose what each button, stick and trigger does, and
sends those keys and mouse actions to the game in front. It can also make
games see an Xbox controller, and it opens Xpadder profiles.


GETTING STARTED

1. Run zpadder.exe. Nothing needs installing. If Windows says "Windows
   protected your PC", choose More info, then Run anyway: ZPadder isn't code
   signed yet.
2. Plug in or pair your controller. ZPadder shows a photo of it.
3. Click a control on the picture, or press it on your controller. On the
   right, choose "Keyboard & mouse..." and click the keys it should press, or
   just press them on your real keyboard.
4. Switch to your game. Mapping pauses while ZPadder's own window is in
   front, so it never types into itself. To try a profile without leaving
   ZPadder, click Test bench in the toolbar and type in its box.
5. File > Save keeps the profile. Edits work as soon as you make them.

For a head start, use File > New from template, or open
profiles\notepad-test.toml (File > Open...) and try it in Notepad.

Closing the window keeps ZPadder running in the notification area (tray),
where you can switch mapping on and off, pick a recent profile, or quit.


USEFUL TO KNOW

- Layers are the tabs above the controller, like Xpadder's sets. Any control
  can switch layers while held, one-way, or two-way (press again to go back).
- The Timing menu, next to "Keyboard & mouse...", adds pauses, tap-or-hold,
  actions on release and cycles. "In plain words" sums up what you've set.
- In the View menu (toolbar), "Sheet: labels beside it" puts every label in a
  column beside the controller, and "Save as image..." saves it as a cheat
  sheet.
- Mapping > AutoProfile rules... switches profiles when a given program is in
  front. Program names can use * and ? wildcards, e.g. FiveM_*GTAProcess.exe.
- Mapping > Start with Windows starts ZPadder in the tray when you sign in.
- Edit > Undo (Ctrl+Z) takes back edits. File > Revert to saved goes back to
  the saved file.
- Xpadder profiles (.xpadderprofile) open with File > Open... ZPadder lists
  anything that didn't carry over; docs\xpadder-import.md explains more.


XBOX CONTROLLER MODE (OPTIONAL)

Some games, such as GTA V and FiveM, only understand Xbox controllers.
ZPadder can make games see an Xbox 360 controller for each controller you
connect, as Steam Input does.

1. Install the free ViGEmBus driver. Open
   https://github.com/nefarius/ViGEmBus/releases (the "Get ViGEmBus" button
   in ZPadder's status bar opens it), download the setup program (.exe) of
   the latest release, 1.22.0, and run it.
2. In ZPadder, tick "Act as an Xbox controller". It's in the right-hand panel
   when no control is selected (click the current layer's tab to get there).
   Or use File > New from template > Xbox controller (all games).
3. Within about ten seconds the status bar shows "Xbox controller: on". No
   restart is needed.
4. If Steam is running, switch off "Steam Input for Xbox controllers" in
   Steam's controller settings. Otherwise, outside Steam games, Steam turns
   ZPadder's Xbox controller into keys and clicks too (in FiveM, for example,
   R2 set off push-to-talk).

Controls you haven't assigned act as the Xbox controller's buttons, sticks
and triggers. Assigned controls send their keys instead; tick "Also press the
Xbox controller's ..." in a control's options to have it do both.

ZPadder works without ViGEmBus, and the status bar says when a profile needs
it. Uninstall it from Windows Settings > Apps if you no longer want it. Some
games with anti-cheat refuse virtual controllers; see docs\anti-cheat.md.


IF SOMETHING DOESN'T WORK

"Windows protected your PC" when you start ZPadder
  ZPadder isn't code signed yet. Choose More info, then Run anyway.

The game gets every press twice
  Other controller software is reading your controller too: Steam Input,
  DS4Windows, reWASD, DSX, JoyToKey or AntiMicroX. The status bar shows
  "Also running: ..." when ZPadder sees one. Switch off that program's
  mapping for this controller (in Steam, Steam Input for your type of
  controller), or close it.

It works in Notepad but not in the game
  The game probably runs as administrator. Windows blocks ZPadder from
  controlling games that run as administrator, unless ZPadder does too.
  ZPadder says so in the status bar and offers to restart as administrator.
  If that isn't it, the game's anti-cheat may be blocking ZPadder.

Nothing happens while the ZPadder window is in front
  That's on purpose, so your controller doesn't type into the editor. Use
  the Test bench: mapping stays on inside its box. The mouse stays still
  there; mouse actions are only listed.

Online games and anti-cheat
  ZPadder presses keys the way any keyboard macro tool does. Some games'
  anti-cheat may block or flag that. Read docs\anti-cheat.md before using
  ZPadder in online games.

Reporting a problem
  Help > Open log folder opens the folder with zpadder.log. Attach it to an
  issue at https://github.com/ZayshaaCodes/zPadder/issues (Help > Report a
  problem... opens the page), and say which controller you use and how it's
  connected.


LAUNCHBOX, THE COMMAND LINE AND EDITING FILES BY HAND

zpadder.exe accepts Xpadder's command line, so front-ends such as LaunchBox
can drive it. A second launch hands its arguments to the running ZPadder.

    zpadder.exe "profile.toml" "Game.lnk" /M   open a profile, start a game,
                                               stay in the tray
    zpadder.exe /C                             close ZPadder

An .xpadderprofile works wherever a profile is expected. Given one profile
per controller, as Xpadder allows, ZPadder uses the first for all of them.

zpadder-cli.exe has tools for scripts: run (map without a window), check
(find mistakes in a profile), import (convert Xpadder files) and schema. Run
it without arguments for details. Profiles are plain text (TOML), described
in docs\profiles.md; profile.schema.json gives editors such as VS Code
completion.


WHERE ZPADDER KEEPS THINGS

Settings, imported controller pictures and the log (zpadder.log) are kept in
%APPDATA%\ZPadder. Help > Open log folder opens it.

Portable mode: create an empty file named portable.txt next to zpadder.exe,
and ZPadder keeps everything in this folder instead.

Profiles are wherever you save them.


PRIVACY

ZPadder makes no network connections and collects nothing. To do its job it
reads the name and title of the window in front (for AutoProfile and to pause
in its own window) and the names of running programs (to warn about other
controller software). Nothing leaves your computer.


UNINSTALLING

1. In ZPadder, untick Mapping > Start with Windows, then quit it from the
   tray.
2. Delete this folder.
3. Delete %APPDATA%\ZPadder if you don't want to keep your settings.
4. If you installed ViGEmBus only for ZPadder, uninstall it from Windows
   Settings > Apps.


LICENCES

ZPadder is dual-licensed under MIT or Apache-2.0 (LICENSE-MIT, LICENSE-APACHE).
It includes open-source software listed in THIRD_PARTY_LICENSES.html and the
licenses folder. Controller picture credits are in
docs\controller-pictures-credits.md. ZPadder is not affiliated with Xpadder,
Sony, Microsoft or Nintendo.
