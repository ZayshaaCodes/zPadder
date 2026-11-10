# ZPadder and anti-cheat

ZPadder presses keys the way any keyboard macro tool does. Some online games' anti-cheat may
block or flag that; you decide.

## What ZPadder does and doesn't do

- It reads your controller the way many games do, and presses keys, clicks and moves the mouse
  through the standard Windows feature for this, like Xpadder, JoyToKey and AntiMicroX.
- It doesn't install drivers, inject code into games, read or change game memory, or hook into
  other programs.
- It doesn't hide what it does. Windows marks every key and mouse action ZPadder sends as coming
  from software, and any program, anti-cheat included, can see that. ZPadder won't disguise it,
  and won't add "undetectable" input or drivers that hide what they are.
- Only if you turn on "Act as an Xbox controller" in a profile does it also drive a virtual Xbox
  controller, through ViGEmBus, a separate driver you install yourself (see below).

## What that means for you

Each game's anti-cheat decides what to do with input that comes from software:

- **Some ignore it.** Many single-player games and many online games work normally.
- **Some block it.** The keys just don't arrive while the game is in front. ZPadder can't tell
  this apart from a game that ignores keys it doesn't use.
- **Some may flag it.** Terms of service often forbid "macros" or "third-party input software",
  and enforcement varies from game to game and over time.

Nobody can promise that a mapper is "safe" in a given online game. The best-known case: in
February 2017 *For Honor*'s EasyAntiCheat banned players, and many of them had Xpadder installed.
Ubisoft never confirmed that Xpadder was the cause, and players without it were banned too
([Kotaku](https://kotaku.com/for-honors-anti-cheat-system-is-allegedly-banning-innoc-1792679195)).
More recently, players have reported trouble with reWASD in competitive shooters. reWASD adds
virtual devices with drivers of its own; ZPadder's key and mouse mapping uses no driver.

## Xbox controller mode and anti-cheat

"Act as an Xbox controller" makes games see your controller as an Xbox 360 controller. It needs
the ViGEmBus driver, which DS4Windows and other remappers also use, and which you install
yourself. This is the part of ZPadder anti-cheat is most likely to object to:

- **Some anti-cheat blocks virtual-controller drivers.** Some games refuse to start while
  ViGEmBus is installed, some ignore virtual controllers, and some treat them like any other
  controller. That can change with an update.
- **It's off unless you turn it on, profile by profile.** Profiles without it never touch the
  driver. If a game's anti-cheat complains, switch it off in that game's profile, or uninstall
  ViGEmBus while you play; the rest of ZPadder keeps working without it.
- **It isn't disguised.** The virtual controller is a ViGEmBus device, and anyone who looks can
  see that.

## Practical advice

- **Read the game's rules** on macros and input software before using ZPadder online.
- **Keep it simple.** One button, one key is the least controversial use. Turbo, sequences and
  timed actions (hold and release zones, cycles) do things a person can't do by hand, which is
  what rules usually target. They're only on when you set them up, and the editor marks them.
- **Use the game's own controller support** where it has one, especially in competitive games.
- **Games that run as administrator** don't get ZPadder's input unless ZPadder runs as
  administrator too. ZPadder tells you when that happens. That's Windows security, not anti-cheat.

If a game blocks or flags ZPadder, please tell us in an
[issue](https://github.com/ZayshaaCodes/zPadder/issues) with its name, so this page can list it.

## Details

ZPadder reads controllers through SDL3 and sends input with Windows' `SendInput`, which sets
`LLKHF_INJECTED` or `LLMHF_INJECTED` on every event. The profile setting for Xbox controller mode
is `virtual_pad = "xbox360"`.
