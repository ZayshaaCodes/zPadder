# ZPadder and anti-cheat

ZPadder turns controller input into keyboard and mouse input with Windows' `SendInput`, the same
way Xpadder, JoyToKey and AntiMicroX do. This page explains what that means for games with
anti-cheat, so you can decide for yourself.

## What ZPadder does

- It reads your controller through SDL, like many games do.
- It sends keys, mouse buttons, wheel and pointer movement through `SendInput`, the documented
  Windows function for synthesised input.
- It doesn't install drivers, inject code into games, read or write game memory, or hook other
  processes.
- It never hides that its input is synthesised. Windows marks every event ZPadder sends as
  injected (`LLKHF_INJECTED` and `LLMHF_INJECTED`), and any program, anti-cheat included, can see
  that. ZPadder won't try to disguise this. Features that would ("undetectable" input, drivers
  that hide what they are) are out of scope.
- Only if you turn on "Act as an Xbox controller" for a profile, it also drives a virtual Xbox
  controller through ViGEmBus, a separate driver you install yourself (see below).

## What that means for you

Anti-cheat systems are free to treat injected input however they like:

- **Some ignore it.** Many single-player games and many online games work normally.
- **Some block it.** The input just doesn't arrive while the game has focus. ZPadder can't tell
  this apart from a game that ignores keys it doesn't use.
- **Some may flag it.** Terms of service often forbid "macros" or "third-party input software",
  and enforcement varies from game to game and over time.

Nobody can promise that a mapper is "safe" in a given online game. The best-known case: in
February 2017 *For Honor*'s EasyAntiCheat banned players, and many of them had Xpadder installed.
Ubisoft never confirmed that Xpadder was the cause, and players without it were banned too
([Kotaku](https://kotaku.com/for-honors-anti-cheat-system-is-allegedly-banning-innoc-1792679195)).
More recently users have reported trouble with reWASD in competitive shooters; reWASD uses
virtual-device drivers, which ZPadder doesn't.

## Acting as an Xbox controller

A profile can ask ZPadder to present your controller to games as an Xbox 360 controller
(`virtual_pad = "xbox360"`, or "Act as an Xbox controller" in the editor). That needs the
ViGEmBus driver, which Steam-free remappers such as DS4Windows also use. ZPadder never installs
it; you do, from its own download page.

This is the part of ZPadder anti-cheat is most likely to object to:

- **Virtual-controller drivers are what some anti-cheats block.** Some refuse to start while
  ViGEmBus is installed, some ignore virtual controllers, and some treat them like any other
  controller. It can change with an update.
- **It is opt-in per profile** and off by default. Profiles without it never touch the driver,
  so you can keep it for the games that need it. Uninstalling ViGEmBus turns the feature off
  everywhere; ZPadder keeps working without it.
- **It isn't disguised.** The virtual controller is a ViGEmBus device, and anyone who looks can
  see that.

If a game's anti-cheat complains, switch the option off for that game's profile, or uninstall
ViGEmBus while you play it.

## Practical advice

- **Read the game's terms** on macros and input software before using ZPadder online.
- **Prefer plain remapping.** One button, one key is the least controversial use. Turbo,
  sequences and timed macros (hold and release zones, cycles) do things a person can't do by
  hand, and those are what terms of service usually target. ZPadder labels them in the editor,
  and they are only ever on when you turn them on.
- **Use the game's own controller support** where it has one, especially in competitive online
  games.
- **Elevated games:** if a game runs as administrator, Windows blocks ZPadder's input to it
  unless ZPadder runs as administrator too. ZPadder tells you when that happens. That's Windows
  security (UIPI), not anti-cheat.

If a game blocks or flags ZPadder, please tell us in an issue with the game's name, so this page
can list known cases.
