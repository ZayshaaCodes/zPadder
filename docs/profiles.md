# Profile files

Profiles are made in the app (`zpadder.exe`), but they are plain TOML and can be written or
edited by hand too. `zpadder-cli run` picks up saved changes while it runs; the app reads a
profile when you open it, so reopen it after editing the file by hand. A complete
example is [`profiles/examples/notepad-test.toml`](../profiles/examples/notepad-test.toml).

```powershell
cargo run -p zpadder --bin zpadder-cli -- check my-profile.toml   # find mistakes
cargo run -p zpadder --bin zpadder-cli -- run my-profile.toml     # map headless (Ctrl+C stops)
```

Unknown fields and misspelt key names are errors, not silently ignored.

## Layout

```toml
format_version = 1
name = "My game"
layout = "builtin:dualshock4"   # optional picture: builtin:dualshock4, builtin:xbox-one,
                                # builtin:xbox-360, builtin:switch-pro, or a .layout.toml file
virtual_pad = "xbox360"         # optional: games also see an Xbox controller (see below)

[[layers]]                 # the first layer is active at start
name = "Main"
trigger_threshold = 0.2    # how far a trigger must be pulled to count as pressed (0–1)

[layers.buttons.south]     # one table per button
slots = ["space"]

[layers.right_trigger]     # triggers work like buttons
slots = ["mouse_left"]

[layers.left_stick]        # see Sticks below
mode = "directions"

[[layers]]                 # more layers, each complete on its own
name = "Aiming"
```

Each layer is complete on its own: a control that has no assignment in the active layer does
nothing. This matches Xpadder's sets. In a profile that acts as an Xbox controller, such a
control works as its Xbox counterpart instead (see below).

## Buttons

`south`, `east`, `west`, `north` (A/B/X/Y on Xbox, Cross/Circle/Square/Triangle on PlayStation),
`back`, `guide`, `start`, `left_stick`, `right_stick` (stick clicks), `left_shoulder`,
`right_shoulder`, `dpad_up`, `dpad_down`, `dpad_left`, `dpad_right`, `misc1`, `left_paddle1`,
`right_paddle1`, `left_paddle2`, `right_paddle2`, `touchpad`.

## Assignments

Buttons, triggers and stick directions all take an assignment:

| Field | Meaning |
|---|---|
| `slots` | Outputs, pressed in order and released in reverse. `["left_ctrl", "z"]` is Ctrl+Z |
| `toggle` | `true`: a press latches the outputs on; the next press releases them |
| `turbo` | Presses per second while held (0.5–60) |
| `rumble` | `true`: the controller vibrates while the assignment is held |
| `passthrough` | `true`: in a profile that acts as an Xbox controller, the control also works as its Xbox button or trigger while doing the assignment |
| `layer` | `{ target = "Aiming", mode = "while_held" }`. Modes: `one_way` (switch and stay, the default), `two_way` (the next press switches back), `while_held` |
| `name` | Optional label |

Slot names:

- **Keys:** `a`–`z`, `0`–`9` (or `digit_0`–`digit_9`), `f1`–`f24`, `space`, `enter`, `escape`,
  `backspace`, `tab`, `caps_lock`, `left_shift`, `right_shift`, `left_ctrl`, `right_ctrl`,
  `left_alt`, `right_alt`, `left_gui`, `right_gui` (Windows keys), `up`, `down`, `left`,
  `right`, `insert`, `delete`, `home`, `end`, `page_up`, `page_down`, `print_screen`,
  `scroll_lock`, `pause`, `num_lock`, `numpad_0`–`numpad_9`, `numpad_enter`, `numpad_add`,
  `numpad_subtract`, `numpad_multiply`, `numpad_divide`, `numpad_decimal`, `minus`, `equal`,
  `left_bracket`, `right_bracket`, `backslash`, `semicolon`, `quote`, `grave`, `comma`,
  `period`, `slash`, `application`. Names follow the US layout; the physical key is the same on
  every layout.
- **Mouse:** `mouse_left`, `mouse_right`, `mouse_middle`, `mouse_x1`, `mouse_x2`.
- **Wheel:** `wheel_up`, `wheel_down`, `wheel_left`, `wheel_right` (one notch per press; add
  `turbo` to keep scrolling).

## Timing slots

Timing slots go in the same `slots` list and turn it into a small timeline, as in Xpadder's
Advanced Assignment window. Xpadder's spellings (`PAUSE 0.15s`, `HOLD ZONE 0.50s`,
`CYCLE STOP`, ...) are accepted too, so slot lists can be pasted from Xpadder files. Durations
can be written `0.15s`, `150ms` or `1m30s`.

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
- Distance zones cannot be mixed with hold or release zones in one list, and zone times or
  percentages must increase.
- `toggle` cannot be combined with timing slots.
- `turbo` repeats whatever the active hold or distance zone is holding.
- Inside a distance zone only the first group of outputs (up to a pause) is used.
- A key held in two zones stays down when one zone hands over to the next.
- On buttons, distance zones always pick the highest zone.

These rules are how ZPadder pins down behaviour Xpadder never documented. In particular, the
claim that zone times are measured from the press, and not from the previous zone, is inferred
from shared Xpadder profiles and may differ from Xpadder in edge cases.

## Sticks

```toml
[layers.left_stick]
mode = "directions"        # "off", "mouse", "directions" or "scroll"
dead_zone = 0.25           # 0–1

[layers.left_stick.directions]
eight_way = false          # false: diagonals press both neighbours (W+D)
diagonal_size = 45         # width of each diagonal sector, in degrees
up = { slots = ["w"] }
down = { slots = ["s"] }
left = { slots = ["a"] }
right = { slots = ["d"] }
# with eight_way = true: up_left, up_right, down_left, down_right

[layers.right_stick]
mode = "mouse"
dead_zone = 0.15
mouse = { speed = 1200, curve = 1.5 }   # pixels per second at full tilt (at most 20000);
                                        # curve > 1 gives finer aim near the centre
                                        # (0.2–5; 1 = linear, the default)
```

Add `invert_x = true` or `invert_y = true` inside `mouse = { … }` to reverse an axis.

A stick can also scroll like a mouse wheel:

```toml
[layers.right_stick]
mode = "scroll"
scroll = { speed = 8, horizontal = true }   # notches per second at full tilt (at most 100);
                                            # horizontal also scrolls sideways
```

`invert = true` inside `scroll = { … }` reverses the scroll direction.

## Acting as an Xbox controller

```toml
format_version = 1
name = "GTA V"
virtual_pad = "xbox360"
```

With `virtual_pad = "xbox360"` (the editor's "Act as an Xbox controller" box), games also see a
virtual Xbox 360 controller for each controller you connect, as they do with Steam Input. It
suits games that only understand Xbox controllers, such as GTA V and FiveM with a DualShock 4.
Leave it out, and ZPadder only sends keys and mouse, as before. It is off by default.

**It needs the ViGEmBus driver**, which is free and installed separately:
<https://github.com/nefarius/ViGEmBus/releases> (version 1.22 works on Windows 10 and 11).
ZPadder works normally without it and says so in the status bar. Once it is installed, ZPadder
finds it within ten seconds, without a restart.

**What passes through.** Every control with nothing assigned in the pad's active layer works as
its Xbox counterpart. A control with an assignment keeps sending its keys, mouse buttons or
layer switch, and the Xbox controller sees it released, so one profile can mix both: for
example, the touchpad can type `m` while everything else is the Xbox controller.

To have an assigned control do both, add `passthrough = true` to its assignment (the editor's
"Also press the Xbox controller's …" box): R1 can type `i` for a game's own menu and still be
RB, the handbrake. A stick with `passthrough = true` moves the Xbox controller's stick whatever
its mode, so it can steer the mouse and the game at once; in mode `off` it always does.

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

- **Sticks** pass through only in `off` mode; a stick that moves the mouse, scrolls or presses
  directions does not. No dead zone is added: games apply their own.
- **Triggers** pass their whole pull through when they have no assignment.
- **A press keeps its destination.** Whether a button or trigger goes to the Xbox controller
  is decided when it is pressed, so switching layers while holding it changes nothing until
  you let go. A button pressed with nothing assigned stays pressed on the Xbox controller even
  if the new layer assigns it, and vice versa.
- **Paused means neutral.** While ZPadder has focus, Windows is locked or asleep, or you test in
  the Test bench, the Xbox controller stays plugged in but reports nothing pressed and the sticks
  centred. Switching profiles also lets go of everything first. Games can drop a controller that
  disappears, so it stays plugged in as long as the profile in use asks for it.
- **It goes away** when the profile in use no longer asks for it, when mapping is switched off,
  when its controller disconnects, and when ZPadder quits.
- **Rumble** a game sends to the Xbox controller reaches your controller, combined with
  assignments that have `rumble = true`.
- **Steam can take it over.** Outside Steam games, Steam applies its desktop layout to every
  Xbox controller it sees, ZPadder's included, so buttons and triggers also click or press keys.
  Switch off "Steam Input for Xbox controllers" in Steam's controller settings; ZPadder reminds
  you while Steam is running.
- **Xbox controllers get no second one.** Pads Windows reads through XInput (Xbox controllers
  and pads that imitate them) are already Xbox controllers to games, so a copy would double
  every press. Their assignments work as usual.
- ZPadder never reads its own virtual controllers. They are not listed as controllers, and
  their presses are not mapped again.
- XInput has room for four controllers, real and virtual together.

Games with anti-cheat may refuse virtual controllers; see [anti-cheat.md](anti-cheat.md).

## Behaviour worth knowing

- **A press owns its release.** If you switch layers while holding a button, releasing it lifts
  what it pressed in the old layer, so keys never get stuck.
- **Shared keys are counted.** If two controls hold the same key, it stays down until both let
  go.
- **While-held layers stack.** Hold L1 for one layer and R1 for another; release them in any
  order and you end up back where you started.
- **A combination always reaches the game.** If another control already holds `z` when you
  press a Ctrl+Z button, ZPadder lets go of `z` and presses it again after Ctrl, so the game
  sees Ctrl+Z.
- **Zones hand over cleanly.** When a hold or distance zone takes over from the previous one,
  keys only the old zone used come up before the new zone's keys go down, so `ctrl+c` handing
  over to `x` never sends Ctrl+X.
- **Analog controls keep their layer's settings while pressed.** A trigger or stick direction
  keeps the threshold, dead zone and stick mode of the layer it was pressed in until you let go,
  even if it switched layers itself.
- **Ctrl+C, unplugging a pad and switching profiles release everything.** So does an internal
  error: if mapping stops unexpectedly, every key and button ZPadder held is released.

## Limits

`zpadder-cli check` and the editor report values outside these limits:

| Setting | Allowed |
|---|---|
| `trigger_threshold` | 0.05–0.95 |
| `dead_zone` in `directions` mode | at least 0.05 (0 is fine for `mouse` and `scroll`) |
| `mouse.speed` | 0–20 000 pixels per second |
| `hold` and `tap_time` slots | longer than 0 s |
| `toggle` | not together with a `while_held` layer switch (use `two_way`) |
| file size | at most 4 MB |
