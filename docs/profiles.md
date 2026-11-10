# Editing profile files by hand

You don't need this page if you use the editor: everything on it can be set in ZPadder's window,
which saves the file for you. It's for people who want to write or tweak a profile in a text
editor, or look up exactly how a setting behaves.

On this page:

- [Before you start](#before-you-start)
- [Make a button press a key or a combination](#make-a-button-press-a-key-or-a-combination), such
  as Ctrl+Z
- [Tap or hold, sequences and other timing](#timing-tap-or-hold-sequences-zones-and-cycles)
- [Make a stick move the mouse, press WASD or scroll](#sticks-move-the-mouse-press-wasd-or-scroll)
- [Switch layers](#layers)
- [Act as an Xbox controller](#play-xbox-only-games-act-as-an-xbox-controller)
- Names of [buttons](#button-names) and [keys](#key-mouse-and-wheel-names)
- [How ZPadder behaves](#how-zpadder-behaves) and its [limits](#limits)

## Before you start

A profile is a plain text file ending in `.toml`. TOML is a simple settings format: `name = value`
lines, grouped under headings in square brackets. A complete example is
[`profiles/examples/notepad-test.toml`](../profiles/examples/notepad-test.toml). Every profile
follows this outline:

```toml
format_version = 1              # required
name = "My game"
layout = "builtin:dualshock4"   # optional picture: builtin:dualshock4, builtin:xbox-one,
                                # builtin:xbox-360, builtin:switch-pro, or a .layout.toml file
virtual_pad = "xbox360"         # optional: games also see an Xbox controller (see below)

[[layers]]                 # the first layer is active at start
name = "Main"
trigger_threshold = 0.2    # how far a trigger must be pulled to count as pressed

[layers.buttons.south]     # one table per button
slots = ["space"]

[layers.right_trigger]     # triggers work like buttons
slots = ["mouse_left"]

[layers.left_stick]        # see Sticks below
mode = "directions"

[[layers]]                 # more layers, each complete on its own
name = "Aiming"
```

Without a `layout`, the editor shows a photo of the connected controller, or the drawn one if you
chose "Drawn" in View ▾. A `.layout.toml` path is relative to the profile's folder unless it's a
full path.

**Mistakes are never silently ignored.** If the file can't be read at all (a misspelt key name, an
unknown field, no `format_version`), ZPadder won't open it and the status bar says why. If it reads
but a value is out of range, or a layer switch points to a layer that doesn't exist, it opens with
the problems listed in the status bar, and the controller does nothing until you fix them.

**ZPadder reads a profile when you open it**, so after editing the file, open it again (File >
Open recent). `zpadder-cli run` picks up saved changes while it runs. From a command prompt in
ZPadder's folder:

```powershell
zpadder-cli check my-profile.toml   # find mistakes
zpadder-cli run my-profile.toml     # map without a window (Ctrl+C stops)
```

From the source code, use `cargo run -p zpadder --bin zpadder-cli -- check my-profile.toml`. The
example profile starts with a `#:schema` line pointing to `profile.schema.json`; editors that
understand it complete field and key names as you type.

## Make a button press a key or a combination

```toml
[layers.buttons.south]       # A on Xbox, Cross on PlayStation (see Button names)
slots = ["space"]

[layers.buttons.east]
slots = ["left_ctrl", "z"]   # Ctrl+Z
```

Everything in `slots` is pressed in order when you press the button, and released in reverse when
you let go, so `["left_ctrl", "z"]` is Ctrl+Z. Mouse buttons and the wheel go in the same list:
`["mouse_left"]`, `["wheel_up"]`. The wheel moves one notch per press; add `turbo` to keep it
scrolling. All the names are under [Key, mouse and wheel names](#key-mouse-and-wheel-names).

Triggers work like buttons: use `[layers.left_trigger]` or `[layers.right_trigger]`. A trigger
counts as pressed once it's pulled past its layer's `trigger_threshold` (0.2 unless you change it;
both triggers share it). Stick directions take the same kind of assignment; see
[Sticks](#sticks-move-the-mouse-press-wasd-or-scroll).

### Options for any assignment

| Field | What it does |
|---|---|
| `slots` | What the control presses, and any [timing slots](#timing-tap-or-hold-sequences-zones-and-cycles). Pressed in order, released in reverse |
| `toggle` | `true`: one press holds the outputs down; the next press lets them go |
| `turbo` | Presses per second while held (0.5–60) |
| `rumble` | `true`: the controller vibrates while the assignment is held |
| `layer` | Switches layer: `{ target = "Aiming", mode = "while_held" }`. See [Layers](#layers) |
| `passthrough` | `true`: in a profile that acts as an Xbox controller, the control also works as its Xbox button or trigger while doing the assignment. See [below](#play-xbox-only-games-act-as-an-xbox-controller) |
| `name` | Optional label, such as "Jump". View ▾ > "Show names instead of keys" shows it on the picture |

```toml
[layers.buttons.west]
slots = ["x"]
turbo = 8.0              # hold to type x eight times a second

[layers.buttons.north]
slots = ["left_shift"]
toggle = true            # press once to hold Shift, press again to let go
```

## Timing: tap or hold, sequences, zones and cycles

Timing slots go in the same `slots` list and turn it into a small timeline, as in Xpadder's
Advanced Assignment window. In the editor they're in the **Timing ▾** menu. Xpadder's spellings
(`PAUSE 0.15s`, `HOLD ZONE 0.50s`, `CYCLE STOP`, ...) work too, so you can paste slot lists from
Xpadder files. Write durations as `0.15s`, `150ms` or `1m30s`.

| Slot | What it does |
|---|---|
| `pause 0.2s` | Makes the list a **sequence**: the outputs between pauses are tapped one group at a time. One press starts it and it runs to the end even if you let go; pressing again stops it |
| `tap_time 0.05s` | How long each later tap holds its keys (default 0.1 s). Also separates taps, like a zero-length pause |
| `hold 0.5s` | What follows applies once the control has been held this long. Each hold zone replaces the one before it |
| `release 0.01s` | What follows is tapped **on release**, if the control was held at least this long. With several release zones, the last one reached wins |
| `distance 50%` | For triggers and sticks: what follows applies beyond this deflection. Each zone replaces the one below it |
| `cycle` | Splits the list into alternatives used on successive presses |

All zone times are measured from the press. Recipes:

```toml
# Tap for X, hold half a second for Y. Releasing between 0.25 s and 0.5 s does nothing.
slots = ["hold 0.5s", "y", "release 0.01s", "x", "release 0.25s"]

# A quick combo: tap 1, wait 0.2 s, tap 2 and 3 together.
slots = ["1", "pause 0.2s", "2", "3"]

# Trigger: Ctrl+W when pulled lightly, Shift+W further, W alone near the end.
slots = ["left_ctrl", "w", "distance 40%", "left_shift", "w", "distance 80%", "w"]

# Stick up: walk, and run near full tilt.
up = { slots = ["w", "distance 90%", "left_shift", "w"] }

# Each press picks the next weapon slot.
slots = ["1", "cycle", "2", "cycle", "3"]
```

Rules:

- Distance zones can't be mixed with hold or release zones in one list, and zone times or
  percentages must increase.
- `toggle` can't be combined with timing slots.
- `turbo` repeats whatever the active hold or distance zone is holding.
- Inside a distance zone only the first group of outputs (up to a pause) is used.
- A key held in two zones stays down when one zone hands over to the next.
- On buttons, distance zones always pick the highest zone.

Xpadder never documented how these work; the rules above are how ZPadder pins them down. In
particular, measuring zone times from the press, and not from the previous zone, is inferred from
shared Xpadder profiles and may differ from Xpadder in edge cases.

## Sticks: move the mouse, press WASD or scroll

Each stick has a `mode`: `off` (the default), `mouse`, `directions` or `scroll`. Its `dead_zone`
(default 0.25) is how far from the centre, from 0 to below 1, the stick must move before it
counts. A stick that's `off` does nothing, but its click (`left_stick` or `right_stick` under
`buttons`) can still be assigned.

### Move the mouse

```toml
[layers.right_stick]
mode = "mouse"
dead_zone = 0.15
mouse = { speed = 1200, curve = 1.5 }
```

Inside `mouse = { … }`:

- `speed`: pixels per second at full tilt (default 1000, at most 20000).
- `curve`: the response curve, 0.2–5. 1 (the default) is linear; above 1 gives finer aim near the
  centre and still reaches full speed at full tilt.
- `invert_x = true` or `invert_y = true` reverses an axis.

### Press keys, such as WASD

```toml
[layers.left_stick]
mode = "directions"
dead_zone = 0.25           # at least 0.05 in this mode

[layers.left_stick.directions]
eight_way = false          # false: diagonals press both neighbours (W+D)
diagonal_size = 45         # width of each diagonal sector, in degrees
up = { slots = ["w"] }
down = { slots = ["s"] }
left = { slots = ["a"] }
right = { slots = ["d"] }
# with eight_way = true: up_left, up_right, down_left, down_right
```

Each direction takes a whole assignment, so any field from
[Options for any assignment](#options-for-any-assignment) works there too.

### Scroll like a mouse wheel

```toml
[layers.right_stick]
mode = "scroll"
scroll = { speed = 8, horizontal = true }
```

Inside `scroll = { … }`:

- `speed`: notches per second at full tilt (default 8, at most 100).
- `horizontal = true` also scrolls sideways when you push left or right.
- `invert = true` reverses the scroll direction.

## Layers

Layers are Xpadder's sets: complete sets of assignments you switch between. The first layer is
active when the profile loads, and each layer needs a different `name`.

Each layer is complete on its own: a control that has no assignment in the active layer does
nothing. In a profile that acts as an Xbox controller, such a control works as its Xbox
counterpart instead (see below).

Any assignment can switch layers with `layer`, and can press keys at the same time:

```toml
[layers.buttons.left_shoulder]      # hold L1 / LB for the "Aiming" layer
layer = { target = "Aiming", mode = "while_held" }
```

| `mode` | What it does |
|---|---|
| `one_way` | Switch and stay. The default if you leave `mode` out |
| `two_way` | Switch; the next press switches back |
| `while_held` | Switch only while the control is held |

`toggle` can't go with a `while_held` switch; use `two_way` for a switch that latches.

## Play Xbox-only games: act as an Xbox controller

```toml
format_version = 1
name = "GTA V"
virtual_pad = "xbox360"
```

With `virtual_pad = "xbox360"` (the editor's "Act as an Xbox controller" box), games also see a
virtual Xbox 360 controller for each controller you connect, as they do with Steam Input. It suits
games that only understand Xbox controllers, such as GTA V and FiveM with a DualShock 4. Leave it
out, and ZPadder only sends keys and mouse. It's off by default.

**It needs the ViGEmBus driver**, which is free and installed separately:
<https://github.com/nefarius/ViGEmBus/releases> (version 1.22 works on Windows 10 and 11). ZPadder
works normally without it and says so in the status bar. Once it's installed, ZPadder finds it
within ten seconds, without a restart.

**What goes to the Xbox controller.** Every control with nothing assigned in the pad's active layer
works as its Xbox counterpart. A control with an assignment keeps sending its keys, mouse buttons
or layer switch, and the Xbox controller sees it released, so one profile can mix both: for
example, the touchpad can type `m` while everything else is the Xbox controller.

To have an assigned control do both, add `passthrough = true` to its assignment (the editor's
"Also press the Xbox controller's …" box): R1 can type `i` for a game's own menu and still be RB,
the handbrake. A stick with `passthrough = true` (the editor's "Also move the Xbox controller's
left stick", or right stick) moves the Xbox controller's stick whatever its mode, so it can steer
the mouse and the game at once; in mode `off` it always does.

```toml
[layers.buttons.right_shoulder]
slots = ["i"]
passthrough = true          # types i and is also RB
```

| ZPadder control | Xbox 360 control |
|---|---|
| `south`, `east`, `west`, `north` | A, B, X, Y (by position: Cross is A on a DualShock 4) |
| `left_shoulder`, `right_shoulder` | LB, RB |
| `back`, `start`, `guide` | Back, Start, Guide |
| `left_stick`, `right_stick` (clicks) | LS, RS |
| `dpad_up`, `dpad_down`, `dpad_left`, `dpad_right` | D-pad |
| `touchpad` | Back (pressing it together with `back` is the same as pressing Back) |
| `misc1`, paddles | nothing: an Xbox 360 controller has no such buttons |
| `left_trigger`, `right_trigger` | LT, RT, analog (0–255) |
| `left_stick`, `right_stick` with `mode = "off"` | the sticks, analog, full range |

- **Sticks** pass through in `off` mode, or in any mode with `passthrough = true`. Otherwise a
  stick that moves the mouse, scrolls or presses directions doesn't. No dead zone is added: games
  apply their own.
- **Triggers** pass their whole pull through when they have no assignment.
- **A press keeps its destination.** Whether a button or trigger goes to the Xbox controller is
  decided when you press it, so switching layers while holding it changes nothing until you let
  go. A button pressed with nothing assigned stays pressed on the Xbox controller even if the new
  layer assigns it, and the other way round.
- **Paused means neutral.** While ZPadder has focus, Windows is locked, asleep or showing a secure
  screen (such as an administrator prompt), or you're testing in the Test bench, the Xbox
  controller stays plugged in but reports nothing pressed and the sticks centred. Switching
  profiles also lets go of everything first. Games can drop a controller that disappears, so it
  stays plugged in as long as the profile in use asks for it.
- **It goes away** when the profile in use no longer asks for it, when mapping is switched off,
  when its controller disconnects, and when ZPadder quits.
- **Rumble** a game sends to the Xbox controller reaches your controller, combined with
  assignments that have `rumble = true`.
- **Steam can take it over.** Outside Steam games, Steam applies its desktop layout to every Xbox
  controller it sees, ZPadder's included, so buttons and triggers also click or press keys. Switch
  off "Steam Input for Xbox controllers" in Steam's controller settings; ZPadder reminds you while
  Steam is running.
- **Xbox controllers don't get a second one.** Controllers Windows already treats as Xbox
  controllers (Xbox controllers, and pads that imitate them) are Xbox controllers to games
  already, so a copy would double every press. Their assignments work as usual.
- ZPadder never reads its own virtual controllers. They aren't listed as controllers, and their
  presses aren't mapped again.
- Windows has room for four Xbox controllers, real and virtual together.

Games with anti-cheat may refuse virtual controllers; see [anti-cheat.md](anti-cheat.md).

## Button names

Buttons are named by position, so `south` is the bottom face button on every controller.

| Name in the file | Xbox | PlayStation | Switch Pro |
|---|---|---|---|
| `south`, `east`, `west`, `north` | A, B, X, Y | Cross, Circle, Square, Triangle | B, A, Y, X |
| `left_shoulder`, `right_shoulder` | LB, RB | L1, R1 | L, R |
| `back`, `start` | View, Menu | Share, Options | Minus, Plus |
| `guide` | Guide | PS | Home |
| `misc1` | Share | Mic | Capture |
| `left_stick`, `right_stick` | stick clicks | stick clicks | stick clicks |
| `dpad_up`, `dpad_down`, `dpad_left`, `dpad_right` | D-pad | D-pad | D-pad |
| `touchpad` | – | touchpad click | – |

`left_paddle1`, `right_paddle1`, `left_paddle2` and `right_paddle2` are the paddles some controllers
have. The editor shows them once they're pressed or assigned, or with View ▾ > "Show all buttons".
The triggers aren't buttons: they have their own tables, `left_trigger` and `right_trigger` (LT
and RT, L2 and R2, ZL and ZR).

## Key, mouse and wheel names

Names ignore upper and lower case.

- **Letters, digits and function keys:** `a`–`z`, `0`–`9` (or `digit_0`–`digit_9`), `f1`–`f24`.
- **Modifiers:** `left_shift`, `right_shift`, `left_ctrl`, `right_ctrl`, `left_alt`,
  `right_alt`, `left_gui`, `right_gui` (the Windows keys).
- **Editing and moving:** `space`, `enter`, `escape`, `backspace`, `tab`, `caps_lock`, `up`,
  `down`, `left`, `right`, `insert`, `delete`, `home`, `end`, `page_up`, `page_down`,
  `print_screen`, `scroll_lock`, `pause`, `application` (the menu key).
- **Number pad:** `num_lock`, `numpad_0`–`numpad_9`, `numpad_enter`, `numpad_add`,
  `numpad_subtract`, `numpad_multiply`, `numpad_divide`, `numpad_decimal`, `numpad_equal`.
- **Punctuation:** `minus`, `equal`, `left_bracket`, `right_bracket`, `backslash`, `semicolon`,
  `quote`, `grave`, `comma`, `period`, `slash`, and two keys found on UK and other European
  keyboards: `non_us_backslash` (beside left Shift) and `non_us_hash` (on Windows, the same key as
  `backslash`).
- **Mouse:** `mouse_left`, `mouse_right`, `mouse_middle`, `mouse_x1`, `mouse_x2`.
- **Wheel:** `wheel_up`, `wheel_down`, `wheel_left`, `wheel_right` (one notch per press; add
  `turbo` to keep scrolling).

Names follow the US layout, but each means the key in that position on every layout: `semicolon`
is the key right of L, whatever is printed on it.

## How ZPadder behaves

- **A press owns its release.** If you switch layers while holding a button, releasing it lifts
  what it pressed in the old layer, so keys never get stuck.
- **Shared keys are counted.** If two controls hold the same key, it stays down until both let go.
- **While-held layers stack.** Hold L1 for one layer and R1 for another; release them in any order
  and you end up back where you started.
- **A combination always reaches the game.** If another control already holds `z` when you press a
  Ctrl+Z button, ZPadder lets go of `z` and presses it again after Ctrl, so the game sees Ctrl+Z.
- **Zones hand over cleanly.** When a hold or distance zone takes over from the previous one, keys
  only the old zone used come up before the new zone's keys go down, so `ctrl+c` handing over to
  `x` never sends Ctrl+X.
- **Triggers and sticks keep their layer's settings while pressed.** A trigger or stick direction
  keeps the threshold, dead zone and stick mode of the layer it was pressed in until you let go,
  even if it switched layers itself.
- **Ctrl+C, unplugging a controller and switching profiles release everything.** So does an
  internal error: if mapping stops unexpectedly, every key and button ZPadder held is released.

## Limits

`zpadder-cli check` and the editor report values outside these limits. The editor's sliders cover
the usual range; a file can go further, up to these limits.

| Setting | Allowed |
|---|---|
| `format_version` | `1` |
| `layers` | at least one, each with a different `name` |
| `layer` switches | `target` must be the name of a layer in the profile |
| `trigger_threshold` | at least 0.05 and below 1 (the editor's slider stops at 0.95) |
| `dead_zone` | 0 to below 1; at least 0.05 in `directions` mode |
| `mouse.speed` | 0–20 000 pixels per second |
| `mouse.curve` | 0.2–5 |
| `scroll.speed` | above 0, at most 100 notches per second |
| `diagonal_size` | 0–90 degrees |
| `turbo` | 0.5–60 presses per second |
| `pause`, `hold` and `release` slots | at most 10 minutes; `hold` must be longer than 0 s |
| `tap_time` slots | longer than 0 s, at most 10 s |
| `distance` slots | 0–100 % |
| zones in one list (or one part of a `cycle`) | times or percentages must increase; distance zones can't be mixed with hold or release zones |
| `toggle` | not with timing slots, and not with a `while_held` layer switch (use `two_way`) |
| file size | at most 4 MB |
