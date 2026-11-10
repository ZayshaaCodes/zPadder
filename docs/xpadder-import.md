# Importing Xpadder files

ZPadder can import both kinds of Xpadder file:

- **`.xpaddercontroller`** (a controller layout): the 512×256 picture and where each control
  sits on it. It becomes a ZPadder layout (`*.layout.toml`) plus a PNG picture.
- **`.xpadderprofile`** (a profile): what each control does, in up to eight sets. It becomes a
  ZPadder profile (`*.toml`), one layer per set.

Xpadder never documented either format. What follows is how ZPadder reads them, based on 239
public files; anything marked **assumption** is a best reading that a copy of Xpadder could
confirm or correct. Nothing is silently lost: everything that cannot be carried over is
reported as a warning, naming the set and control.

## Using it

**In the app:** File > Open accepts `.xpadderprofile` files directly. The profile is converted
when it opens, and a window lists anything worth knowing about the conversion. If the folder
also holds an `.xpaddercontroller` file (as shared Xpadder packs do), its button order is used
and its picture is shown in the editor. File > Save as keeps the result as a ZPadder profile.
To show any profile on an Xpadder picture, use View ▾ > From Xpadder file….

**As a drop-in for Xpadder:** `zpadder.exe "game.xpadderprofile" /M` works the way front-ends
such as LaunchBox call Xpadder, and AutoProfile rules can point straight at `.xpadderprofile`
files. They are converted each time they load, so the originals keep working with Xpadder too.

**On the command line:**

```powershell
zpadder-cli check "p1.xpadderprofile"                 # convert in memory and list the notes
zpadder-cli run "p1.xpadderprofile"                   # map with it, no window
zpadder-cli import "p1.xpadderprofile" [out.toml]     # write a ZPadder profile
zpadder-cli import "Xbox 360.xpaddercontroller" [dir] # write a layout and its PNG picture
zpadder-cli import "p1.xpadderprofile" --buttons dualshock4
```

Without `--buttons`, the button order comes from an `.xpaddercontroller` in the profile's
folder, or the Xbox order is assumed (and the notes say so). `--buttons` takes a preset name
(`xbox`, `dualshock4`) or a controller file; see [Button presets](#button-presets).

For development there is also an example converter:
`cargo run -p zpadder-import-xpadder --example xpadder-import -- FILE [OUT_DIR] [--buttons ...]`.

## Reading the files

Real files break naive parsers in several ways; all of these are handled:

| Trap | What the importer does |
|---|---|
| Text is Windows-1252, not UTF-8 (`Botón`) | Decoded as Windows-1252. A UTF-8 or UTF-16 BOM, or text that is valid UTF-8, means another editor re-saved the file, and is honoured |
| CRLF line endings, or LF / lone CR after copy and paste | All accepted |
| Indented lines (one HyperSpin export indents every line) | Every line is trimmed |
| Legacy 5.x files with no `[Section]` headers | Sections are not needed; keys are recognised by their shape |
| Text glued after a header: `[Assignments];--- Xpadder Profile File ---` | The rest of the line is read as a line of its own |
| Different banners (`Xpadder Controller File`, `... Data`, boxed 5.x comments) | Ignored. Files are identified by `DataType=`, or by their keys when it is missing |
| A key written twice (for example two files pasted together) | The last value wins, with a warning |
| Unknown keys, or lines that are not `key=value` | Ignored, each with a warning |
| Missing or broken picture rows | Left transparent, with a warning |

## Controller files

| Xpadder | ZPadder layout |
|---|---|
| `Button{n}Location` | The position of the button that `Button{n}` is in the [button map](#button-presets) |
| `DPad{Up,Right,Down,Left}Location` | `dpad_up` ... `dpad_right` |
| `Stick1...` / `Stick2...` four direction locations | The middle of the four, as the position of `left_stick` / `right_stick` (sticks are placed by their click buttons). This replaces the click button's own marker |
| `Trigger{Left,Right}Location` | `left_trigger`, `right_trigger` |
| `[Image]` `ImageLine1`–`ImageLine256` | A 512×256 PNG. Magenta (#FF00FF) and missing rows are fully transparent |
| `Button{n}Name` | Used to work out the button map, then dropped |
| `[Access]` | Not imported: ZPadder reads pads through SDL's gamepad mapping, so DirectInput axis bindings do not apply. `Axis Z` / `Axis Z -` (an Xbox pad's shared trigger axis) is simply ZPadder's two independent triggers. Unknown bindings, and a stick reading both directions from one axis, get a warning |

**Assumption:** Xpadder stores the top-left corner of each control's 30×30 marker, and ZPadder
layouts store centres, so 15 px is added to both coordinates. Evenly spaced layouts put
markers exactly 30 px apart, and a right trigger at x = 478 ends 4 px from the edge, mirroring
the left trigger's 4 px margin.

The layout is named after the file (controller files have no name inside), and its picture gets
the same name with `.png`.

## Profiles

### Sets become layers

- Set 1 is always the first layer, because Xpadder starts in set 1.
- Other sets become layers if they have assignments or a set selector leads to them; empty,
  unreachable sets are left out. Layers stay in set order.
- Layers are named from `Set{n}Name`, or `Set {n}`. If two sets share a name, the later one
  becomes `Name (Set {n})`.
- The profile's name comes from `Name=`; when there is none, name it after the file.
- Xpadder has at most 8 sets; sets numbered above 8 are ignored, with a warning.

### Assignments

| Xpadder | ZPadder |
|---|---|
| `...Slots` | `slots`. Keys by name (table below), mouse buttons, wheel, and the timing slots `PAUSE`, `HOLD ZONE`, `RELEASE ZONE`, `DISTANCE ZONE`, `TAP TIME`, `CYCLE STOP` |
| `...Name` | `name` |
| `...Toggle=1` | `toggle = true` (on a set selector it means two-way, see below) |
| `...Turbo=1` with `...TurboRate` | `turbo`, in presses per second (see [Turbo](#turbo)) |
| `...Rumble=1` | `rumble = true`. `RumbleStrengths` other than `255,255` get a warning |
| `...SetSelect` | `layer`, see [Set selectors](#set-selectors) |
| Buttons `Button1`–`Button32` | Through the [button map](#button-presets). A number with no ZPadder button is reported and skipped. A number the map says is a trigger (L2/R2 on a DualShock 4) fills that trigger, unless the profile's own `TriggerLeft`/`TriggerRight` assignment already does |
| `DPad{Up,Down,Left,Right}` | `dpad_up` ... `dpad_right` |
| `DPad` diagonals | **Not imported**: ZPadder's DPad is four buttons, so a diagonal presses both neighbours |
| `TriggerLeft`, `TriggerRight` | `left_trigger`, `right_trigger` |

Unknown slot tokens are skipped with a warning; Xpadder's `NONE` is an empty slot.

ZPadder's rules for slot lists are stricter than Xpadder's in two places, and real files hit
both:

- **Toggle with timing slots.** ZPadder cannot toggle a timeline, so `toggle` is dropped (with a
  warning) and the timeline kept.
- **Zones that go back in time.** Seven assignments in two corpus profiles end with a shorter
  hold zone:
  `HOLD ZONE 0.30s, F2, HOLD ZONE 0.10s`. ZPadder measures zones from the press, so this
  would be invalid. **Assumption:** the Xpadder tutorial says zones accumulate, so such a zone is
  read as relative to the one before: `hold 0.3s, f2, hold 0.4s`, which taps F2 for 0.1 s once
  the button has been held 0.3 s. Every other multi-zone list in the corpus already reads
  correctly with times measured from the press.
- An empty zone at the end of a list that mixes distance and hold zones (which ZPadder does not
  allow) is dropped. Any other list ZPadder cannot represent is not imported, with a warning.

Out-of-range values are clamped to ZPadder's limits (which match Xpadder's): pauses and zones up
to 10 minutes, tap times up to 10 s, distances 0–100 %.

### Key names

Xpadder writes keys by name. Each name maps to the **physical key** (a USB HID usage), never
through Windows virtual-key codes: the only earlier converter did that and sent `NumPad Enter`
as VK_SEPARATOR.

| Xpadder | ZPadder | | Xpadder | ZPadder |
|---|---|---|---|---|
| `A`–`Z`, `0`–`9`, `F1`–`F24` | `a`–`z`, `0`–`9`, `f1`–`f24` | | `Back-Tick` | `grave` |
| `Left Shift`, `Right Shift` | `left_shift`, `right_shift` | | `Minus`, `Equals` | `minus`, `equal` |
| `Left Control`, `Right Control` | `left_ctrl`, `right_ctrl` | | `Left/Right Square Bracket` | `left_bracket`, `right_bracket` |
| `Alt` | `left_alt` | | `Semi-Colon`, `Apostrophe` | `semicolon`, `quote` |
| `Windows Logo Left` / `Right` | `left_gui`, `right_gui` | | `Comma`, `Dot`, `Forward-Slash` | `comma`, `period`, `slash` |
| `Arrow Up/Down/Left/Right` | `up`, `down`, `left`, `right` | | `Back-Slash[US]/Hash[UK]` | `backslash` (the same key) |
| `Page Up`, `Page Down` | `page_up`, `page_down` | | `NumPad 0`–`NumPad 9` | `numpad_0`–`numpad_9` |
| `Escape`, `Enter`, `Space`, `Tab`, `Backspace` | same, lower case | | `NumPad Enter` | `numpad_enter` |
| `Insert`, `Delete`, `Home`, `End` | same, lower case | | `NumPad /`, `*`, `-`, `+`, `Dot` | `numpad_divide`, `_multiply`, `_subtract`, `_add`, `_decimal` |
| `Caps Lock`, `Scroll Lock`, `Num Lock` | `caps_lock`, `scroll_lock`, `num_lock` | | `Left/Right/Middle Mouse Button (1/2/3)` | `mouse_left`, `mouse_right`, `mouse_middle` |
| `Mouse Wheel Up/Down/Left/Right` | `wheel_up` ... `wheel_right` | | `Mouse Move ...` | a mouse stick, see below |

Matching ignores case and extra spaces. Names never seen in a real file (Right Alt, Print
Screen, Pause, the Windows menu key, mouse buttons 4 and 5) are accepted in their likely
spellings and common variants, such as `Right Alt`, `Print Screen`, `Pause`,
`Mouse Button 4 (4)`.

### Set selectors

**Assumption, from two public profiles and their READMEs:**

- `SetSelect` counts sets from **zero**: `SetSelect=1` switches to set 2.
- A **while-held** shift is stored as a pair: a selector on set A pointing to set B, and a
  mirrored selector on the same control in set B pointing back to A. The pair becomes one
  `while_held` selector on A; the return selector in B is dropped (the control's own slots in B
  are kept). Which side is "forward" is decided by distance from set 1, then by set number.
- While that control is held, other while-held selectors can take the pad further (hold LT for
  set 2, then RT for set 3). A selector on the held control in those deeper sets that points
  back to A is a return selector too, and is dropped. ZPadder's while-held layers stack, so
  releasing the controls in any order lands back in the right set; Xpadder's "while held glitch"
  does not carry over.
- `Toggle=1` on a selector makes it **two-way**: the switch latches, and the next press returns.
- Any other selector is **one-way**.

### Sticks

A stick whose directions carry `Mouse Move` slots becomes a **mouse stick**:

- `invert_y` / `invert_x` when the directions are swapped (`Mouse Move Up` on the stick's down
  direction).
- **Assumption:** `MouseSpeed` is linear, and Xpadder's default of 32 equals ZPadder's default of
  1000 pixels per second at full tilt: speed = `MouseSpeed` × 31.25 px/s. ZPadder has one speed
  per stick, so different speeds per direction are averaged, with a warning. The response curve
  stays linear (`curve = 1`). `MouseSpeed` is capped at 640 (20 000 px/s, ZPadder's maximum),
  with a warning.
- `MouseMode=Cursor` or `Spring` (absolute positioning) has no ZPadder equivalent yet; the stick
  is imported as normal relative movement, with a warning.
- Keys, selectors, turbo or toggle on a mouse stick's directions are not imported (warning).
  A stick that moves the pointer on only one axis in Xpadder moves it on both in ZPadder
  (warning). Five corpus profiles do this, for example turning with the stick while its up and
  down directions press keys.

Any other stick with assignments becomes a **directions stick**:

- `Set{n}Stick{m}Mode`: `EightWay` → `eight_way = true`, `FourWay` → `false`. `Horizontal` and
  `Vertical` priority have no equivalent and import as four-way (warning). With no mode, the
  stick is eight-way if it has diagonal assignments.
- `Set{n}StickDiagonalSizes=a,b` (stick 1, stick 2) → `diagonal_size` in degrees, clamped to
  0–90. **Assumption:** the values (25–89 in the corpus, 45 most common) are degrees.

For both kinds, **assumption:** `Set{n}StickDeadZones=a,b` uses a 0–255 scale, because 64 / 255
is Xpadder's documented 25 % default. `dead_zone` = value / 255, at most 0.95. A directions
stick needs at least 5 % so it lets go at rest, so lower values are raised to 5 % (warning).
Sticks with nothing assigned stay off.

### Triggers

**Assumption:** `Set{n}TriggerDeadZones=a,b` (left, right) is on the same 0–255 scale, and a
trigger's dead zone is where it starts to count as pressed. It becomes the layer's
`trigger_threshold`, clamped to 0.05–0.95 (lower values would fire on noise). ZPadder has one
threshold for both triggers; if they differ, the larger is used, with a warning.

### Turbo

**Assumption:** `TurboRate` is the time between presses in hundredths of a second, so turbo =
100 / `TurboRate` presses per second, kept within ZPadder's 0.5–60. It is the one reading under
which shared profiles make sense: menu sets repeat arrow keys at 20 (5 per second), a
weapon-switching set scrolls the wheel at 40 (2.5 per second), and an Alt+Tab assignment
repeats at 25 next to a 0.25 s tap time. The most common value, 2, gives 50 per second. A file
that turns turbo on without a `TurboRate` gets 10 presses per second. `TurboRate` without
`Turbo=1` is ignored, as in Xpadder.

### Not imported

- `KeyTapTime` (profile-wide tap length; 5, 11 and 23 in the corpus, probably hundredths of a
  second). ZPadder taps keys for 0.1 s unless a slot list has its own `tap_time`; a value other
  than 10 gets a warning.
- DPad diagonals and the DPad's horizontal/vertical priority modes.
- `Mouse Move` on buttons, triggers or the DPad (ZPadder moves the pointer only with sticks).
- Mouse `Cursor`/`Spring` modes, per-direction mouse speeds and one-axis mouse sticks (see
  above).
- Sticks beyond the second.
- Rumble strengths.

### Damaged and unusual files

The importer accepts what Xpadder wrote and skips what it can't read, with a warning for each
problem. After 50 warnings it adds "…and N more warnings" instead of listing them all. Values
outside what ZPadder allows are clamped (for example a 0 s hold zone or tap time becomes
0.01 s), and assignments that still don't make sense are dropped, so an imported profile always
loads. Files larger than 4 MB are refused; real Xpadder files are a few kilobytes.

## Button presets

Xpadder numbers buttons as DirectInput reports them, and the order depends on the pad. ZPadder
names buttons by position (`south` is A on Xbox and Cross on PlayStation), so importing a
profile needs a **button map**.

- **Import the controller file first** whenever you have it. Its button names usually say which
  button is which (`A`, `LB`, `Start`, `Cross`, `L1`, `Options`, `L3`, `R-Click` and other common
  spellings are recognised). Recognised names are used directly, and the other buttons follow
  the preset their names point to.
- If the names are generic (`Button 1`, `Botón 1`), the Xbox order is assumed, with a warning.
- Otherwise choose a preset:

| Preset | Order | Use for |
|---|---|---|
| `xbox` (default) | 1 A, 2 B, 3 X, 4 Y, 5 LB, 6 RB, 7 Back, 8 Start, 9 LS, 10 RS | Xbox 360 and Xbox One pads, and most XInput pads. The guide button is invisible to DirectInput; the triggers are axes |
| `dualshock4` | 1 Square, 2 Cross, 3 Circle, 4 Triangle, 5 L1, 6 R1, 7 L2, 8 R2, 9 Share, 10 Options, 11 L3, 12 R3, 13 PS, 14 touchpad | DualShock 4, and also Logitech Dual Action, RumblePad 2 and F310/F710 in DirectInput mode, and most PlayStation-style DirectInput pads (buttons 1–12 match) |

Buttons 7 and 8 in the `dualshock4` preset are the **triggers**: those pads report L2/R2 (or
LT/RT) as buttons, and ZPadder reads them as triggers, so their assignments go to
`left_trigger` / `right_trigger`.

Signs that the preset is wrong: warnings that buttons 11 and up have no ZPadder button, or
face buttons that do the wrong thing in game. PS2 adapters and the PS3's DirectInput drivers use
yet other orders; for those, import a controller file with recognisable names, or remap the
buttons in the editor afterwards.
