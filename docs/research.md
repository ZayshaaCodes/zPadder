# Decoding Xpadder to Build Its Rust Successor

Xpadder is a portable, single-executable Windows utility. It reads up to 16 game controllers and turns their buttons, sticks, DPad and triggers into synthesized keyboard and mouse input. Its design rests on two plain-text files. The first is a per-device **controller layout**: a 512×256 picture, hotspot positions and DirectInput axis bindings. The second is a per-game **profile**: eight "sets" of assignments, where each assignment is a slot list that doubles as a small macro language with pause, hold-zone, release-zone, distance-zone, tap-time and cycle-stop tokens. Neither format was ever documented, but both can be fully reverse-engineered. A corpus of 239 public files decodes cleanly, including the run-length-encoded hex image, so a faithful importer is a modest, OS-agnostic job. Xpadder's well-known weaknesses trace almost entirely to its architecture of DirectInput reading plus user-mode injection: silent failure in elevated games, double input in games that already support pads, a fixed mouse curve, caps of eight sets and one DPad, no gyro, and slower sequences on Windows 10. Its author calls these "complicated to resolve using the current generation of Xpadder code." Twenty years after launch it still sells for US$9.99 with lifetime updates, but the public changelog stops at 2017.05.01 and the forum is dormant. Competitors leave a real gap. Steam Input and reWASD are powerful but tied to the Steam client or a subscription. The free GUI mappers are thin: JoyToKey is shareware, AntiMicroX is looking for a maintainer, and JoyShockMapper and most DS4Windows forks have gone quiet. ZPadder's chosen stack (SDL3 input, SendInput output, egui editor) holds up against the 2026 crate ecosystem. The right roadmap first delivers Xpadder parity, including import, on a pure and testable core. After that come the features Xpadder never had: editable curves, unlimited layers, chords, gyro and flick stick, and an optional virtual-controller module.

## Xpadder turns every pad into a picture, a slot list and eight sets

Xpadder's walkthrough spells out the separation everything else depends on: "The controller layout is the controller image and the position of the buttons, sticks, dpads and triggers." A layout is saved once per device. By contrast, "a profile is the actual key and mouse actions that are mapped onto this controller layout" ([Walkthrough 1.2](https://xpadder.com/forum4/viewtopic.php?t=3709)). The user works by clicking pictures. They click the controller picture to pick an input, then a keyboard-and-mouse picture to pick the output. Xpadder dropped its built-in help to shrink the executable and moved the documentation to its forum ([changelog to 2017.05.01](https://xpadder.com/forum4/viewtopic.php?f=26&t=5901)). As a result, the numbered walkthrough posts by developer Jonathan Firth and moderator "Primal Fear" serve as the manual, and they are the main source for this section.

### Layouts are built once per device and reused for identical pads

To create a layout, the user picks Controller menu → "New...". This opens a Controller Settings window with Image, Sticks, DPad, Buttons and Triggers pages. For each element the user ticks "Enabled", presses or moves the element on the real pad so Xpadder detects it, drags its marker onto the picture and names it. For the DPad, Xpadder asks for each direction in turn ([Walkthrough 1.2](https://xpadder.com/forum4/viewtopic.php?t=3709); [Racketboy tutorial](https://racketboy.com/retro/xpadder-use-your-pc-gamepad-instead-of-keyboard)). The picture is a **512×256 BMP that uses magenta (255,0,255) for transparency**. Xpadder opens only uncompressed 256-colour or 16M-colour BMPs, but an image in any format can be pasted from the clipboard ([How to create Controller Images](https://xpadder.com/forum4/viewtopic.php?t=6092)). Limits are hard-coded: **32 buttons** ([t=6139](https://xpadder.com/forum4/viewtopic.php?t=6139)) and **one DPad** ([t=2149](https://xpadder.com/forum4/viewtopic.php?t=2149)), plus two sticks and two triggers, which the file keys imply. Xpadder runs **16 controllers at once**, each with its own tab and profile. It reuses a saved layout automatically for an identical device (the user can override this) and picks up reconnected pads "instantly" ([Controller questions](https://xpadder.com/forum4/viewtopic.php?t=2149); [What can Xpadder do](https://xpadder.com/forum4/viewtopic.php?t=2422)).

Xpadder has no calibrator of its own; for stick drift, the official advice is to use Windows' joy.cpl. The Stick Settings window shows the live stick position as a dot inside a red dead-zone area. A slider sets the dead zone per profile and per set, with a **default of 25%**, and triggers have their own dead-zone trackbar ([Mouse tips and troubleshooting](https://xpadder.com/forum4/viewtopic.php?t=5323); [changelog](https://xpadder.com/forum4/viewtopic.php?f=26&t=5901)). Clicking a controller tab shows its raw DirectInput and XInput data ([How to display raw data](https://xpadder.com/forum4/viewtopic.php?t=5983)). The changelog also mentions a "Virtual Controller panel" whose capacity grew from 1 to 16. No source explains what it is for; editing without hardware attached is the likely purpose. There is no in-app library. Layouts and profiles are shared through the forum, which has **343 Controller Images topics and 580 Game Profiles topics** ([forum index](https://xpadder.com/forum4/)).

### Assignments are slot lists, and six special slots make them a macro language

Left-clicking an element opens the Assignment window. It shows an on-screen keyboard, a mouse picture, a large "NONE" key and a row of **slots**. Clicking a key, or pressing it on the real keyboard, fills the current slot. Each element has unlimited slots, and keys in consecutive slots are held together as a combination such as Shift+X ([Walkthrough 2.1](https://xpadder.com/forum4/viewtopic.php?t=3714); [Racketboy tutorial](https://racketboy.com/retro/xpadder-use-your-pc-gamepad-instead-of-keyboard)). Along the bottom are three checkboxes (**Toggle**, **Turbo** and **Rumble**), an "Abc" field for naming the assignment, and an "Advanced..." button ([bottom three checkboxes](https://xpadder.com/forum4/viewtopic.php?t=6019)). Outputs cover "the most common keyboard keys, mouse movements, up to 5 mouse buttons and mouse wheel scrolling/tilting." A spanner icon next to each stick or DPad auto-assigns a WSAD, arrow, mouse or numpad group ([What can Xpadder do](https://xpadder.com/forum4/viewtopic.php?t=2422)).

The Advanced Assignment window exposes the engine itself. It contains a slot bar, Clear/Insert/Delete buttons, colour-coded special items and preset time and distance buttons. A "schedule text" pane describes the assignment in plain English and links to a test editor ([Advanced Assignments tutorial](https://xpadder.com/forum4/viewtopic.php?t=3137)). The six special slot types are the core semantics ZPadder must reproduce:

| Special slot (UI colour) | File token | Behaviour |
|---|---|---|
| Pause (blue) | `PAUSE 0.15s` | Turns the list into a sequence. One press starts it, a second press stops and resets it, and a finished sequence does not repeat while held. 0.00 s is allowed; the maximum is `PAUSE 10m00.00s` |
| Hold Zone (green) | `HOLD ZONE 0.30s` | Later slots fire only once the button has been held this long. Zones accumulate, and every list starts with an implicit 0.01 s zone |
| Release Zone (yellow) | `RELEASE ZONE 0.01s` | Later slots fire on release, selected by how long the button was held. Cannot be nested with Hold Zones |
| Distance Zone (magenta) | `DISTANCE ZONE 50%` | For sticks and triggers: later slots fire beyond this deflection. A sequence inside a zone fires only its first item |
| Tap Time (light blue) | `TAP TIME 0.01s` | Changes the key-tap duration (default 0.10 s) for all later slots and acts as a 0 s pause |
| Cycle Stop (red) | `CYCLE STOP` | Splits the list into alternatives that advance on each press and wrap around |

Sources: [Walkthrough 2.1](https://xpadder.com/forum4/viewtopic.php?t=3714); [Advanced Assignments tutorial](https://xpadder.com/forum4/viewtopic.php?t=3137); token inventory from the [namwent/hyperspin-xpadder](https://github.com/namwent/hyperspin-xpadder) corpus.

These primitives combine. The standard tap-or-hold recipe is `HOLD ZONE 0.50s, Y, RELEASE ZONE 0.01s, X, RELEASE ZONE 0.25s`: a tap sends X and a hold sends Y ([Walkthrough 2.1](https://xpadder.com/forum4/viewtopic.php?t=3714)). Stacked Distance Zones give analog gradation, for example Ctrl+W up to 40% deflection, Shift+W up to 80% and W beyond that ([Advanced tutorial](https://xpadder.com/forum4/viewtopic.php?t=3137)). The missing pieces are just as telling. Xpadder has **no built-in double-tap or two-button chord**, so users emulate them with sets and pauses ([Set Selector Tricks](https://xpadder.com/forum4/viewtopic.php?t=4457)). Toggle sends a brief tap, pauses, and only then holds the key ([Toggle behavior](https://xpadder.com/forum4/viewtopic.php?t=5924)). Turbo takes an integer rate whose unit nobody documents ([temporary turbo](https://xpadder.com/forum4/viewtopic.php?t=6023)). Rumble is only a per-assignment flag that vibrates the pad "(if possible) while you hold it down" ([What can Xpadder do](https://xpadder.com/forum4/viewtopic.php?t=2422)). No source mentions battery display or LED control. Asked about gyro support, the forum answered "In short, no" ([About gyroscope support](https://xpadder.com/forum4/viewtopic.php?t=5926)).

### Eight sets and three selector modes substitute for shift layers

"Each profile contains 8 Sets of assignments - the default is Set 1," and sets are selected from numbered buttons at the bottom right of the window ([Walkthrough 2.0](https://xpadder.com/forum4/viewtopic.php?t=3713)). Right-clicking any button, trigger or stick direction turns it into a **Set Selector** with one of three modes. **One-way** switches on press-and-release and stays. **Two-way** switches, and a second press returns to the previous set. **While held** applies the set only while the control is held ([Walkthrough 2.0](https://xpadder.com/forum4/viewtopic.php?t=3713)). Later versions added set names, a Set Settings window and set clearing. Mouse speed and dead zones are stored per set, so the standard aim-down-sights trick is a while-held selector on the left trigger that switches to a set with a slower mouse ([changelog](https://xpadder.com/forum4/viewtopic.php?f=26&t=5901); [Mouse tips](https://xpadder.com/forum4/viewtopic.php?t=5323)).

Three defects show where ZPadder must improve. A two-way selector switches only on *release*. If two while-held selectors are released in the wrong order, the user is left in the wrong set (the "while held glitch"). A button on one controller cannot change sets on another ([Set Selector Tricks](https://xpadder.com/forum4/viewtopic.php?t=4457); [While-held glitch workaround](https://xpadder.com/forum4/viewtopic.php?t=1282); [t=4220](https://xpadder.com/forum4/viewtopic.php?t=4220)). Users have also asked repeatedly for more than eight sets ([More than 8 sets?](https://xpadder.com/forum4/viewtopic.php?t=4690)).

### Sticks and triggers are flexible, but the mouse curve is fixed

Each stick is stored as four direction "buttons", and each direction can take any assignment. Stick and DPad settings choose a direction mode ("4-standard, 4-way diagonal, 8 way or vertical/horizonal priority"). A **Diagonal Size** slider sets the width of the diagonal sectors. In the status image, white areas move the mouse straight along one axis and grey areas move it fully analog ([What can Xpadder do](https://xpadder.com/forum4/viewtopic.php?t=2422); [Mouse tips](https://xpadder.com/forum4/viewtopic.php?t=5323)). Mouse emulation has three modes: **Normal** (camera/look, relative velocity), **Cursor** and **Spring** (absolute positioning relative to the centre). Other options are speed per direction, invert, and a "Move Together" checkbox ([Mouse Spring thread](https://xpadder.com/forum4/viewtopic.php?t=5853); [changelog](https://xpadder.com/forum4/viewtopic.php?f=26&t=5901)).

The response curve cannot be edited. After years of requests, Jonathan's answer was "Not yet. It's one of the more complex features to add while maintaining backwards-compatibility." One frustrated user called the program "completely useless without decent control of the mouse via the right stick" ([Edit the Mouse-Emulation curve](https://xpadder.com/forum4/viewtopic.php?t=3703)). Spring mode also fails to return to centre reliably after a fast release, and its radius and pivot cannot be configured ([Mouse tips](https://xpadder.com/forum4/viewtopic.php?t=5323); [Mouse Spring thread](https://xpadder.com/forum4/viewtopic.php?t=5853)). Xbox 360-class triggers read independently because Xpadder can read XInput. Under DirectInput the two triggers share one Z axis, and a settings option turns XInput off entirely ([Game compatibility questions](https://xpadder.com/forum4/viewtopic.php?t=2158); [t=5991](https://xpadder.com/forum4/viewtopic.php?t=5991)).

### AutoProfile, a command line and the tray complete the workflow

Profiles are saved as `.xpadderprofile` files next to the portable executable. They open from a Profile menu with a Recent list, and unsaved changes are marked with a yellow highlight ([Walkthrough 1.3](https://xpadder.com/forum4/viewtopic.php?t=3710); [changelog](https://xpadder.com/forum4/viewtopic.php?f=26&t=5901)). **AutoProfile** watches the focused window and loads a profile for each controller. A rule picks a program by executable ("Select Program") or from recently focused windows, and it can match the whole program or a single window title. With several controllers, consecutive entries go to controllers 1, 2 and so on. A `>none< >default<` entry provides a fallback profile, and `>Ignore Default< Xpadder.exe` stops the fallback from loading while Xpadder itself has focus ([3.1 AutoProfiles](https://xpadder.com/forum4/viewtopic.php?t=1944)).

The **command line** is `Xpadder "Profile1" "Profile2" "Profile3" "Shortcut.lnk" /M /C /?`. Profiles open onto controllers in order, or onto a controller when it connects. The shortcut launches only after all controllers are connected and every profile has loaded. `/M` starts minimised and `/C` closes a running instance. Running Xpadder with no arguments while it is already running restores or minimises the existing window ([3.0 Command Line Options](https://xpadder.com/forum4/viewtopic.php?t=3715)). Front-ends such as LaunchBox, HyperSpin, GameEx and RocketLauncher drive Xpadder through this interface. RocketLauncher stores per-game `p1.xpadderprofile` files and supports "ONLY Xpadder (2012 versions)" ([RocketLauncher wiki](https://rlauncher.com/wiki/index.php/Keymapper); [LaunchBox forum](https://forums.launchbox-app.com/topic/34644-how-to-toggle-between-xpadder-sets-with-a-two-button-press-or-timed-press/)).

Xpadder runs from the system tray and blocks Windows shutdown if there are unsaved changes ([Walkthrough 1.6](https://xpadder.com/forum4/viewtopic.php?t=3712)). It keeps Xpadder.ini beside the exe ([Issues and Disclaimers](https://xpadder.com/forum4/viewtopic.php?t=6065)). The executable is **under 2 MB** and has "no online functions" ([How safe and efficient is Xpadder?](https://xpadder.com/forum4/viewtopic.php?t=2425)). It includes 12 UI languages and accepts community theme backgrounds ([xpadder.com](https://xpadder.com/); [Theme Images forum](https://xpadder.com/forum4/viewforum.php?f=13)). No source documents an in-app hotkey or controller chord for switching profiles.

## Both file formats are plain-text INI, and a 239-file corpus decodes them

Xpadder never published a format specification. The research team therefore parsed **239 real files (220 profiles and 19 controller layouts) from 14 public GitHub repositories** and cross-checked them against examples posted on the forum. [namwent/hyperspin-xpadder](https://github.com/namwent/hyperspin-xpadder) alone supplies 167 of the profiles. Anything below marked as inferred comes from this corpus rather than from documentation.

### Nineteen version stamps share one container format

Both file types are INI-like text with **CRLF line endings, no BOM and Windows-1252 encoding**. The only non-ASCII file in the corpus contains `Bot\xF3n` ("Botón"), which is not valid UTF-8 ([exitcas/XpadderPS3](https://github.com/exitcas/XpadderPS3)). Modern files start with a comment banner followed by `DataType=` and `Version=`. Legacy 5.x files use a boxed comment and leave out `DataType`, and one 5.6 profile variant has no `[Section]` headers at all ([mobiwolf/xpadder2xml](https://github.com/mobiwolf/xpadder2xml); [namwent COD4 profile](https://github.com/namwent/hyperspin-xpadder)). The banner also varies: a controller file posted on the forum in 2014 begins `;--- Xpadder Controller Data ---`, while repository files begin `;--- Xpadder Controller File ---`. An importer should therefore identify files by `DataType` and key shapes, not by the comment ([Troubleshooting controller layouts](https://xpadder.com/forum4/viewtopic.php?t=5057); [gist lotherk](https://gist.github.com/lotherk/e3ce62a2bc4ac746ad16)).

Profile versions in the corpus run from `5.6` to `2019.05.01`; `2012.12.31` is the most common, with 113 files. Controller versions run from `5.3` to `2020.05.01`. The changelog entry "Removed old binary recognition for controller and profile files" shows that an older binary format once existed, but no circulating file uses it ([changelog](https://xpadder.com/forum4/viewtopic.php?f=26&t=5901)). Because the files are plain text, users routinely share them by copying and pasting through the clipboard ([sliekens/gw2-xpadder](https://github.com/sliekens/gw2-xpadder)).

### Controller files hold geometry, DirectInput bindings and a run-length-encoded picture

A `.xpaddercontroller` file has four sections. Here is an abbreviated 2020.05.01 file for an XInput pad ([serafdev/xpadder-config](https://github.com/serafdev/xpadder-config)):

```ini
;--- Xpadder Controller File ---
DataType=Controller
Version=2020.05.01

[Button Locations]
Button1Location=376,136
DPadUpLocation=173,157
Stick1UpLocation=108,70
Stick1RightLocation=140,102
Stick1DownLocation=108,134
Stick1LeftLocation=76,102
TriggerLeftLocation=4,4
TriggerRightLocation=478,4

[Access]
DPadUpAccess=POV
Stick2XAccess=Axis RX
Stick2YAccess=Axis RY
TriggerLeftAccess=Axis Z
TriggerRightAccess=Axis Z -

[Names]
Button1Name=Button 1
DPadName=DPad
Stick1Name=Stick 1
StickUpName=Up
TriggerLeftName=Trigger Left

[Image]
ImageLine1=1,1,FF00FF,...
```

`Location` values are pixel coordinates on the 512×256 canvas, most likely the top-left corner of a label box. Each stick has four direction locations. `[Names]` holds free user text: a German file uses `DPadUpName=Hoch` ([gist lotherk](https://gist.github.com/lotherk/e3ce62a2bc4ac746ad16)). The direction labels `StickUpName` and the like are shared by all sticks.

The `[Access]` vocabulary is small. Across the whole corpus it consists of `POV` (68 uses), `Axis X/Y/Z/RX/RY/RZ`, `Axis Z -` (7), `Axis Z Full`, `Axis RZ Full`, `Axis RY - Full` and `Slider 1`. These names match the fields of DirectInput's `DIJOYSTATE` structure (lX, lY, lZ, lRx, lRy, lRz, rglSlider, rgdwPOV), not WinMM's naming. The likely semantics, inferred from the files: a trailing ` -` selects the negative half of an axis, which is how `Axis Z`/`Axis Z -` split an Xbox pad's combined DirectInput trigger axis into LT and RT. A trailing ` Full` maps the whole range to 0–100% for pedal-style axes that rest at one end. For example, an Xbox One layout binds its triggers separately as `Axis Z Full` and `Axis RZ Full` ([sliekens/gw2-xpadder](https://github.com/sliekens/gw2-xpadder)), and a SideWinder joystick file uses `Slider 1` ([CareFudundae/Xpadder_profiles](https://github.com/CareFudundae/Xpadder_profiles)). Buttons have no Access key, so button *N* is presumably DirectInput button *N*−1. When `Stick1XAccess`/`Stick1YAccess` lines are missing, Xpadder evidently defaults to `Axis X`/`Axis Y`. Both of these points are inferred.

The picture is stored in up to 256 `ImageLine{y}=` rows. The research team decoded the encoding and verified it against every file that contains image data. The first token of each row is an **escape value** (0, 1 or 2 in the corpus) that never appears as a literal pixel in that row. After that, a token equal to the escape value starts a run `ESC,RRGGBB,COUNT`, where the count is in **hexadecimal**. Any other token is a single literal pixel in hex RGB with leading zeros dropped, so `0` is black and `10101` is #010101. The first row of an Xbox 360 layout adds up to 0x18 + 2 + 0x1CB + 6 + 0x15 = 512 pixels. All 17 image-bearing files decode to exactly 512 pixels per row, and re-rendering one produced the expected pad on a magenta background ([CareFudundae/Xpadder_profiles](https://github.com/CareFudundae/Xpadder_profiles)). Rows can be missing, since some files start at `ImageLine27`, and some files have an empty `[Image]` section ([gkytrash/xpadder-configs](https://github.com/gkytrash/xpadder-configs); [ParadiseAigo/xpadder-files](https://github.com/ParadiseAigo/xpadder-files)). The verified reference decoder ports directly to Rust:

```python
def decode_line(v):
    t = v.split(','); esc = t[0]; i = 1; px = []
    while i < len(t):
        if t[i] == esc:
            color = int(t[i+1], 16); n = int(t[i+2], 16); px += [color]*n; i += 3
        else:
            px.append(int(t[i], 16)); i += 1
    return px   # always len == 512 in corpus
```

### Profiles flatten everything into Set–Control–Property keys

A `.xpadderprofile` has up to three sections (`[Profile Settings]`, `[Set Settings]` and `[Assignments]`) and a single flat key namespace of the form `Set{n}{Control}{Property}`. A 2011 Call of Duty profile shows most of the grammar ([namwent COD MW3 profile](https://github.com/namwent/hyperspin-xpadder/blob/master/Xpadder%20Xbox360/PC%20Games/Call%20of%20Duty%206%20Modern%20Warfare%203/p1.xpadderprofile)):

```ini
;--- Xpadder Profile File ---
DataType=Profile
Version=2011.06.24
Name= COD Modern Warfare 3

[Set Settings]
Set1Name=Main Action
Set1StickDeadZones=64,53
Set2Name=MultiPlayer

[Assignments]
Set1Button1Name=Jump/Stand
Set1Button1Slots=Space,Enter
Set1Button2Slots=RELEASE ZONE 0.01s,C,RELEASE ZONE 0.30s,HOLD ZONE 0.30s,Left Control
Set1Button5Toggle=1
Set1Button5Turbo=1
Set1Button5TurboRate=2
Set1Button5SetSelect=1
Set1Stick2UpSlots=Mouse Move Up
Set1Stick2UpMouseSpeed=75
Set1Stick2UpMouseMode=Cursor
Set1TriggerRightSlots=Left Mouse Button (1)
Set1TriggerRightRumbleStrengths=255,255
Set2Button5SetSelect=0
```

The full set of key families found in the corpus, with the confidence of each interpretation:

| Key family | Values observed | Meaning |
|---|---|---|
| `KeyTapTime` (Profile Settings) | 5, 11, 23 | Standard key-tap time; unit unknown |
| `Set#Name` | free text | Set name (documented) |
| `Set#StickDeadZones=a,b` | 39,39 (most common); 64,53; 220,187 | Per-stick dead zones. Inferred to use a 0–255 scale, because 64/255 ≈ the documented 25% default |
| `Set#TriggerDeadZones=a,b` | 28,28; 5,5; 65,65 | Left and right trigger dead zones (inferred) |
| `Set#StickDiagonalSizes=a,b` | 45,80; 67,45; 25,80; 45,89 | Diagonal sector width per stick (inferred) |
| `Set#Stick#Mode`, `Set#DPadMode` | FourWay, EightWay, Horizontal, Vertical | Direction mode. The UI's "4-way diagonal" token never appears |
| `…Slots` | comma-separated tokens | The assignment timeline (documented) |
| `…Name` | free text | Assignment label (documented) |
| `…Toggle`, `…Turbo`, `…Rumble` | 1 | Checkbox flags (documented) |
| `…TurboRate` | 2–41 (2 most common) | Turbo speed; unit unknown |
| `…RumbleStrengths=a,b` | 255,255 | Two motor strengths (inferred) |
| `…SetSelect` | 0–7 | **Zero-based** target set (documented) |
| `…MouseSpeed` | 12–215 (32 most common, 146 uses) | Mouse speed in an undocumented unit |
| `…MouseMode` | Cursor; Spring | Mouse mode. When absent, presumably Normal |

Sources: corpus analysis of the repositories cited above; [Mouse tips](https://xpadder.com/forum4/viewtopic.php?t=5323) for `MouseMode=Spring` and the 25% default; [Set Selector Tricks](https://xpadder.com/forum4/viewtopic.php?t=4457) for zero-based `SetSelect`.

The controls are `Button1`–`Button32`, `DPadUp/Right/Down/Left` plus four diagonals (`DPadUpRight` and so on), `Stick#Up/Right/Down/Left` plus four diagonals, and `TriggerLeft`/`TriggerRight`. Slot tokens are human-readable key names, not virtual-key codes. They include letters, digits, `F1`–`F12`, `Left Shift`, `Right Control`, `Alt`, `NumPad 0`–`NumPad 9`, `NumPad Enter`, `Arrow Up`, `Page Up`, `Back-Tick`, `Semi-Colon`, `Forward-Slash` and `Dot`. The mouse tokens are `Left/Right/Middle Mouse Button (1/2/3)`, `Mouse Move Up/Down/Left/Right` and `Mouse Wheel Up/Down/Left/Right`. Then come the six timing tokens. A forum post also uses `Windows Logo Left` ([Advanced tutorial](https://xpadder.com/forum4/viewtopic.php?t=3137)). Most assignments are a single slot (1,579 cases), a few run to eight, and one pathological macro holds **493 slots** ([namwent corpus](https://github.com/namwent/hyperspin-xpadder)).

Set selectors need the most care. One Guild Wars 2 profile's README describes "Set 2: left trigger (while held); Set 3: set 2 + right trigger (while held)". The file itself contains `Set1TriggerLeftSetSelect=1`, `Set2TriggerLeftSetSelect=0`, `Set2TriggerRightSetSelect=2` and `Set3TriggerRightSetSelect=1` ([sliekens/gw2-xpadder](https://github.com/sliekens/gw2-xpadder)). The pattern suggests that a while-held shift is stored as two entries: a forward selector on the source set, and a mirrored "return" selector on the same control in the target set. `Toggle=1` next to `SetSelect` appears to make the switch latch, as in two-way mode. This reading comes from only two independent profiles. It needs confirming against a live copy of Xpadder before the importer depends on it.

### Eight traps an importer must survive

Real files break naive parsers in eight predictable ways:

1. **Encoding.** Decode as Windows-1252, never as UTF-8.
2. **Whitespace.** Trim every line, because one large HyperSpin export indents each line by four spaces.
3. **Legacy structure.** Accept files with no sections, and lines glued onto a section header such as `[Assignments];--- Xpadder Profile File ---` ([namwent corpus](https://github.com/namwent/hyperspin-xpadder)).
4. **Missing images and user errors.** Tolerate missing image rows, and files that bind both `Stick1XAccess=Axis Y` and `Stick1YAccess=Axis Y` ([gkytrash/xpadder-configs](https://github.com/gkytrash/xpadder-configs)).
5. **Off-by-one indexing.** Sets and controls are numbered from 1, but `SetSelect` counts from 0.
6. **Unknown keys.** Keep any keys the importer doesn't recognise, for diagnostics and round-tripping, because no corpus covers every case.
7. **Key table.** Don't reuse the only existing key table. `xpadder2xml` maps `NumPad Enter` to 108, which is VK_SEPARATOR, and maps `Alt` to the generic VK_MENU ([mobiwolf/xpadder2xml](https://github.com/mobiwolf/xpadder2xml)). Map names to **scan codes plus an extended-key flag** instead.
8. **Test fixtures.** Most corpus repositories have no licence, so regression fixtures should be synthetic or cleared with the authors.

A tolerant pattern for assignment keys is:

```
^Set(\d+)(Button(\d+)|DPad(Up|Down|Left|Right|UpRight|UpLeft|DownRight|DownLeft)|Stick(\d+)(Up|Down|Left|Right|UpRight|UpLeft|DownRight|DownLeft)|Trigger(Left|Right))(Name|Slots|Toggle|Turbo|TurboRate|SetSelect|Rumble|RumbleStrengths|MouseSpeed|MouseMode)$
```

There is almost no prior art. The only open-source reader is `xpadder2xml`, a 2013-era Ruby script that reads only the `Slots` values. QPadder, a "cross-platform analogue" in C++/Qt last pushed in 2022 with two stars, contains no Xpadder parser. No converter from Xpadder to AntiMicroX, JoyToKey, Steam Input or DS4Windows was found ([QPadder](https://github.com/3dproger/QPadder); [AlternativeTo](https://www.alternativeto.net/software/xpadder/)). ZPadder would ship the first serious importer.

Several things remain unknown. The units of `MouseSpeed`, `TurboRate` and `KeyTapTime` are undocumented. So are the keys in Xpadder.ini, where AutoProfile rules probably live. Tokens for Right Alt, Print Screen, Pause, mouse buttons 4 and 5, and media keys were never observed. Turning `MouseSpeed=32` into pixels per second has no formula. The cheapest way to settle all of this is to buy one licence and use it as a reference oracle.

## DirectInput reading and user-mode injection explain most of Xpadder's failures

Xpadder can only read controllers. In Jonathan's words it cannot "simulate game controller buttons or axis (Xpadder can only read them)" and cannot remap axes, and it "is completely dependant on a controller driver" ([What can Xpadder do](https://xpadder.com/forum4/viewtopic.php?t=2422); [Controller questions](https://xpadder.com/forum4/viewtopic.php?t=2149)). Three pieces of evidence point to a **DirectInput core**:

- The binding names match DirectInput's `DIJOYSTATE` structure.
- Xpadder supports 16 controllers, whereas XInput is capped at four ([RocketLauncher wiki](https://rlauncher.com/wiki/index.php/Keymapper)).
- Even 2020-format files for XInput pads bind the Xbox triggers as two halves of the Z axis.

On top of that core sits **XInput**. Xpadder "can read XInput" and offers a DirectInput-only mode ([Game compatibility questions](https://xpadder.com/forum4/viewtopic.php?t=2158)), and the changelog mentions a "dual raw data" read path ([changelog](https://xpadder.com/forum4/viewtopic.php?f=26&t=5901)). The likely reading is that the layout file records the DirectInput view of the pad, and Xpadder substitutes XInput data at runtime for Xbox pads. That is an inference.

This hybrid design must match each XInput device to its DirectInput twin. Microsoft notes that the two can only be told apart with a WMI scan for `IG_` in the device IDs, because "this information cannot be found by just using DirectInput." Under DirectInput alone, Xbox triggers "act as a single button" and vibration is unavailable ([Microsoft Learn: XInput and DirectInput](https://learn.microsoft.com/en-us/windows/win32/xinput/xinput-and-directinput)). The matching broke when two or more Xbox One pads were connected on Windows 10: "DirectInput in Windows 10 does not allow XBox One controllers to be accessed in the background… the GUI needs to be in focus." Jonathan called this "one of many issues that are complicated to resolve using the current generation of Xpadder code" ([t=5991](https://xpadder.com/forum4/viewtopic.php?t=5991)).

On the output side, Xpadder synthesizes input in user mode and runs straight into **UIPI**. The official diagnostic is a Notepad test: "If the Notepad test does work then the game may be running in admin mode… Xpadder is currently unable to send any keys to it." The fix is to run Xpadder as administrator. Doing that permanently breaks "Start with Windows", so users fall back on a Task Scheduler entry with highest privileges ([Walkthrough 1.5](https://xpadder.com/forum4/viewtopic.php?t=3711); [3.2 Works in Windows but not in game](https://xpadder.com/forum4/viewtopic.php?t=1574)). Microsoft confirms both failure modes. SendInput fails under UIPI and "neither GetLastError nor the return value will indicate the failure," and it "does not reset the keyboard's current state" ([Microsoft Learn: SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)).

Nobody knows whether Xpadder sends scan codes or virtual-key codes. This matters because DirectInput-based games ignore input injected as virtual keys only ([gamedev.net](https://www.gamedev.net/topic/581515-simulating-keyboard-with-sendinput-api-in-directinput-applications/)). Relative mouse moves are multiplied "by up to four times" by Windows pointer settings ([Microsoft Learn: MOUSEINPUT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-mouseinput)). That explains why the forum's standing advice is to disable "Enhance pointer precision" ([Mouse tips](https://xpadder.com/forum4/viewtopic.php?t=5323)).

Injected events are also easy to identify. Low-level hooks see `LLKHF_INJECTED` ([Microsoft Learn: KBDLLHOOKSTRUCT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-kbdllhookstruct)). Xpadder's own anti-cheat list rated GameGuard and HackShield as "probably blocks" and PunkBuster, VAC and Warden as "probably allows" ([Official Anti-Cheat Topic](https://xpadder.com/forum4/viewtopic.php?t=2157)). The one well-known incident came in February 2017, when EasyAntiCheat banned *For Honor* players. GameRevolution noticed that many banned users had Xpadder installed, but Ubisoft never confirmed the cause, and some players without Xpadder were banned too ([Kotaku](https://kotaku.com/for-honors-anti-cheat-system-is-allegedly-banning-innoc-1792679195); [Twinfinite](https://twinfinite.net/news/player-gets-banned-from-for-honor-without-even-owning-the-game/)).

The complaints users raise most often map cleanly onto these root causes. That mapping tells ZPadder what a modern stack fixes for free and what it must handle deliberately:

| Limitation | Evidence | Root cause | ZPadder response |
|---|---|---|---|
| Game reacts twice in titles with native pad support | A whole help subforum; workarounds include dummy `xinput1_3.dll` files ([t=2510](https://xpadder.com/forum4/viewtopic.php?t=2510)) | Read-only design cannot hide the physical pad | Integrate HidHide; offer optional virtual-pad output |
| Silent failure in elevated games | [Walkthrough 1.5](https://xpadder.com/forum4/viewtopic.php?t=3711) | UIPI | Detect an elevated foreground process, warn, offer elevated restart or Task Scheduler |
| No input on the lock screen, UAC secure desktop or many Metro apps | [t=5716](https://xpadder.com/forum4/viewtopic.php?t=5716); [t=5842](https://xpadder.com/forum4/viewtopic.php?t=5842) | User-mode injection | Document as out of scope |
| Toggle+turbo test gave 21 keys on Windows 7 but 13–14 on Windows 10 | [t=6017](https://xpadder.com/forum4/viewtopic.php?t=6017) | Undiagnosed; consistent with Windows timer changes | Deadline scheduling on high-resolution timers |
| Two or more Xbox One pads need focus on Windows 10 | [t=5991](https://xpadder.com/forum4/viewtopic.php?t=5991) | DirectInput/XInput matching | SDL3 background events and XInput correlation |
| No PS button, touchpad, Guide, Elite paddles or gyro | [t=5881](https://xpadder.com/forum4/viewtopic.php?t=5881); [t=5926](https://xpadder.com/forum4/viewtopic.php?t=5926) | Driver-dependent DirectInput | SDL3 HIDAPI drivers |
| 32 buttons, 1 DPad, 8 sets, no cross-controller sets | [t=6139](https://xpadder.com/forum4/viewtopic.php?t=6139); [t=4690](https://xpadder.com/forum4/viewtopic.php?t=4690); [t=4220](https://xpadder.com/forum4/viewtopic.php?t=4220) | Fixed data model | Unbounded model, global layers |
| Fixed mouse curve, inaccurate spring mode | [t=3703](https://xpadder.com/forum4/viewtopic.php?t=3703); [t=5853](https://xpadder.com/forum4/viewtopic.php?t=5853) | Engine | Editable curves; spring radius and pivot |
| While-held glitch, tap-before-hold toggle, slow key auto-repeat | [t=4457](https://xpadder.com/forum4/viewtopic.php?t=4457); [t=5924](https://xpadder.com/forum4/viewtopic.php?t=5924); [t=5658](https://xpadder.com/forum4/viewtopic.php?t=5658) | Engine | Layer stack, clean toggle, configurable repeat |
| PrintScreen and Pause unsupported | [it.wikipedia](https://it.wikipedia.org/wiki/Xpadder) | Key table | Scan-code table with special cases |
| First-run dialogs repeat when the exe folder is unwritable | [t=6065](https://xpadder.com/forum4/viewtopic.php?t=6065) | Portable-only storage | Portable flag, otherwise `%APPDATA%` |
| Windows only | "There are no plans to create Linux or Mac versions" ([t=2139](https://xpadder.com/forum4/viewtopic.php?t=2139)) | Win32 design | OS-agnostic core |

Nobody has diagnosed the Windows 10 slowdown, which is roughly a third fewer keys in the same 20 seconds. Microsoft documents that `timeBeginPeriod` stopped affecting global timer resolution in Windows 10 version 2004 ([Microsoft Learn: timeBeginPeriod](https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timebeginperiod)). That fits the symptom but does not prove the cause. Either way, ZPadder should not depend on the system timer resolution.

## Xpadder still sells for $9.99, but development visibly stopped between 2017 and 2020

| Date | Milestone | Source |
|---|---|---|
| c. 2006 | Jonathan Firth announces Xpadder "to a small group of gamers in the UK"; the site footer still reads "2006-2026" | [xpadder.com](https://xpadder.com/?c=US&x=hi); [Demo/trial FAQ](https://xpadder.com/forum4/viewtopic.php?t=2460) |
| 2008 | Version 5.3 (dated 4 Sep by Softpedia, 12 Dec by FileHorse). On 9 Oct the free line is closed at 5.3 | [Softpedia](https://www.softpedia.com/progChangelog/Xpadder-Changelog-45445.html); [FileHorse](https://www.filehorse.com/download-xpadder/old-versions/); [it.wikipedia](https://it.wikipedia.org/wiki/Xpadder) |
| Mid-2009 | Version 5.4 relaunches Xpadder as a commercial product | [it.wikipedia](https://it.wikipedia.org/wiki/Xpadder) |
| 18 Mar 2010 | Buyers get all future versions free; older versions except the newest become free downloads; YYYY.MM.DD versioning begins | [it.wikipedia](https://it.wikipedia.org/wiki/Xpadder); [Buy once](https://xpadder.com/forum4/viewtopic.php?t=2400) |
| 2010–2012 | 5.7 circulates as the "last freeware version"; 2012.12.31 becomes the most common profile version | [MajorGeeks](https://www.majorgeeks.com/files/details/xpadder_(last_freeware_version).html); [namwent corpus](https://github.com/namwent/hyperspin-xpadder) |
| 2014–2017 | Releases 2014.01.01, 2015.01.01, 2016.05.01 and 2017.05.01; the cumulative changelog ends at 2017.05.01 | [changelog](https://xpadder.com/forum4/viewtopic.php?f=26&t=5901); [Mouse Spring thread](https://xpadder.com/forum4/viewtopic.php?t=5853) |
| Feb 2017 | *For Honor* EasyAntiCheat bans associated with Xpadder | [Kotaku](https://kotaku.com/for-honors-anti-cheat-system-is-allegedly-banning-innoc-1792679195) |
| Apr 2017 | "Xpadder is still selling well… The new forum and help system… is taking most of my time" | [Project update](https://xpadder.com/forum4/viewtopic.php?t=5899) |
| 2020 | Release 2020.05.01; June update: "I'm busy coding the future" | [t=6017](https://xpadder.com/forum4/viewtopic.php?t=6017); [Project update](https://xpadder.com/forum4/viewtopic.php?t=5962) |
| 2023 | A TechSpot download title names 2023.05.01 (search snippet only, unverified) | [TechSpot](https://www.techspot.com/downloads/5869-xpadder.html) |
| Nov 2026 | On sale at **US$9.99 / £6.99** via PayPal; "designed for Windows 11, 10, 8.1, 8, 7, Vista and XP"; latest buyer review dated 2026-10-18 | [xpadder.com US](https://xpadder.com/?c=US&x=hi); [xpadder.com GB](https://xpadder.com/?c=GB&x=hi) |

Xpadder's business model has stayed the same for sixteen years. It is a single purchase with all future updates included, there is no demo or trial, and a donation page was added at long-time fans' request ([Demo/trial FAQ](https://xpadder.com/forum4/viewtopic.php?t=2460); [Donate](https://xpadder.com/?x=donate&c=US)). The site says "I work on Xpadder full-time." A 2026 review marvels that "I bought this 12+ years ago and the fact that I can STILL download a working, updated, installer is just astounding" ([xpadder.com](https://xpadder.com/?c=US&x=hi)). In an undated FAQ post, probably from around 2011, Jonathan claimed "millions of users" ([Demo/trial FAQ](https://xpadder.com/forum4/viewtopic.php?t=2460)), and in a later post he noted that "not a single advert has been made to promote Xpadder" ([Xpadder is now 15 years old!](https://xpadder.com/forum4/viewtopic.php?t=6144)).

Technically, though, Xpadder is frozen. No public changelog exists after 2017.05.01, so whatever changed in 2020.05.01 or any later build is unknown. Feature requests such as an editable curve, gyro and more than eight sets have gone unanswered for years. The forum holds 17,846 posts across 4,204 topics and 49,671 members. Its pre-sales subforum is locked, and users describe it as "practically dead" ([forum index](https://xpadder.com/forum4/); [secure desktop thread](https://xpadder.com/forum4/viewtopic.php?t=5842)). The brand also sits inside a piracy ecosystem: "cracked" Google Sites pages, look-alike domains such as xpadder.org and xpadder.net, and a forum subforum titled "Xpadder is reported as a virus or unsafe" ([xpadder.org](https://www.xpadder.org/); [Google Sites crack page](https://sites.google.com/view/xpadder-windows-10-cracked-95); [changelog page jumpbox](https://xpadder.com/forum4/viewtopic.php?f=26&t=5901)). GitHub repositories named "Xpadder Full Version / Unlocked" follow the pattern of malware lures ([GitHub search](https://api.github.com/search/repositories?q=xpadder)). What survives is a large, loyal user base and an archive of community profiles and layouts in a format that can be decoded. That archive is the asset a successor can inherit.

## Rivals are either powerful but tethered or free but fading

| Tool | Licence / price | Status, Nov 2026 | What it has that Xpadder lacks |
|---|---|---|---|
| Steam Input | Free, closed; needs the Steam client | Active; 2nd-gen Steam Controller shipped 4 May 2026 | Regular, Double, Long, Start, Release and Chorded activators; fire delays; cycle bindings; per-activator haptics ([Steamworks](https://partner.steamgames.com/doc/features/steam_controller/activators?language=english)) |
| reWASD | Subscription since 8.0: $29/$39/$49 per year; lifetime tier | Version 9.5.0 | 10 shift layouts, single/double/triple/start/release press, response curves, virtual gyro, radial menu, 6 virtual controller types, mobile app ([reWASD](https://www.rewasd.com/advanced-controller-mapping); [blog](https://www.rewasd.com/blog/post/meet-new-era)) |
| JoyToKey | Shareware | 7.4, 4 Oct 2026; frequent releases | Profile switching by exe or window title, shift layers, absolute and relative mouse, macros, media keys, vibration patterns ([JoyToKey](https://joytokey.net/en/)) |
| AntiMicroX | GPL-3.0, Qt + SDL2 | 3.6.1, May 2026; **looking for a maintainer** | Sets, macros, running scripts, calibration UI, Linux support; auto-profile does not work on Wayland ([README](https://github.com/AntiMicroX/antimicrox)) |
| DS4Windows 5 (hbashton) | GPL-3.0 | **Only unsigned pre-releases** (VIIPERRC4.6.7, Oct) | PlayStation and Switch pads, gyro aiming, flick stick, touchpad, adaptive triggers, virtual Xbox/DS output ([README](https://github.com/hbashton/DS4Windows)) |
| JoyShockMapper | MIT | **Dormant**: v3.6.1, Feb 2024 | Gyro and flick stick, Simultaneous and Diagonal Press, text configuration ([releases](https://github.com/Electronicks/JoyShockMapper/releases)) |
| Joystick Gremlin | GPL-3.0, Python | R16, Sep 2026 | HOTAS modes, response curves, macro recording, vJoy output, about 1 ms input ([releases](https://github.com/WhiteMagic/JoystickGremlin/releases)) |
| Handheld Companion | CC BY-NC-SA 4.0 | 1.3.3.1, Oct 2026 | Motion profiles for handheld PCs, overlay, VIIPER + HidHide ([README](https://github.com/Valkirie/HandheldCompanion)) |
| x360ce | No licence file | 4.28, Oct 2026 | Controller-to-controller mapping only; no keyboard/mouse output ([README](https://github.com/x360ce/x360ce)) |

The rest of the field is dead or stale. Gopher360's last release was in 2017. InputMapper is discontinued, and users report that Controller Companion's developer has abandoned it. The original antimicro is archived. GlosSI, the "Steam Input everywhere" workaround, was archived in November 2025 because "without ViGEm and HidHide GlosSI cannot function" ([Gopher360](https://github.com/Tylemagne/Gopher360); [Vaporlens](https://vaporlens.app/app/367670/controller_companion); [GlosSI](https://github.com/Alia5/GlosSI)). The niche for Rust mappers is almost empty. XialloControl (Rust with Tauri and gilrs, GPL-3.0) has 13 stars, and gyromouse has no licence at all, so its code cannot be reused ([XialloControl](https://github.com/Hotakus/XialloControl); [gyromouse](https://github.com/Yamakaky/gyromouse)).

### Table stakes have moved past Xpadder; the frontier is precision aim

Each feature below appears in at least three active competitors, so any 2026 mapper is expected to have it: layers, tap/hold/double/release activators, turbo and toggle, timed macros, per-app auto profiles, a mouse stick with dead zones and curves, a scroll stick, rumble, and support for PlayStation and Switch pads. Xpadder covers most of this list but misses double-tap, editable curves, a dedicated scroll stick and native PlayStation and Switch support.

Competition now happens in areas Xpadder never reached. **Gyro-to-mouse and flick stick** are the clearest example. They are JoyShockMapper's signature features and now ship in DS4Windows 5. Calibration can come from **GamepadMotionHelpers**, a header-only MIT library with stillness-detection and sensor-fusion auto-calibration that JoyShockMapper and Godot already use ([GamepadMotionHelpers](https://github.com/JibbSmart/GamepadMotionHelpers)). The other frontier features are Steam-style activators with fire delays, radial menus, on-screen keyboards and virtual controller output.

### Users complain about tethers, subscriptions, bans and abandonment

Complaints follow a clear pattern. Steam Input works well only inside Steam: adding GOG or Epic titles as non-Steam games is described as "clunky" ([Notebookcheck](https://www.notebookcheck.net/Steam-Controller-The-current-solution-to-the-99-gamepad-s-biggest-problem.1285432.0.html)). Even after an SDL patch, the 2026 Steam Controller still needs Steam Input for its gyro, back buttons and touchpads ([Notebookcheck](https://www.notebookcheck.net/Steam-Controller-now-works-without-Steam.1298686.0.html)).

reWASD draws two kinds of complaints. The first is its move to subscriptions in 2024. The second is anti-cheat trouble: users report bans in Battlefield 6 and from Activision, and EA forum threads report reWASD being blocked even when it is not running ([reWASD forum](https://forum.rewasd.com/forum/rewasd/technical-questions-aa/246616-rewasd-causing-bans-in-battlefield-6-even-with-compatibility-mode-turned-on); [EA forums](https://forums.ea.com/discussions/battlefield-2042-technical-issues-en/rewasd-error-but-no-rewasd-on-computer/7055345)).

DS4Windows has fragmented into a succession of forks. The nefarius-maintained line was discontinued over burnout and "disillusionment" with the quality of open-source contributions. Its successor offers only unsigned pre-releases, and look-alike download domains have multiplied ([nefarius/DS4Windows-schmaldeo](https://github.com/nefarius/DS4Windows-schmaldeo); [hbashton releases](https://github.com/hbashton/DS4Windows/releases)).

The lesson for ZPadder is that **governance is a feature**. Signed releases, one canonical download site and a bench of more than one maintainer set it apart as much as gyro does. Keyboard and mouse output through SendInput, with no kernel driver, also avoids the virtual-device blocks that hit reWASD.

### Virtual controllers have moved to VIIPER, and permissive code is enough

The virtual-controller landscape changed in 2025–2026:

- **ViGEmBus** was archived in November 2023 at v1.22.0. It still works, but it has no future ([ViGEmBus](https://github.com/nefarius/ViGEmBus)).
- **VIIPER** is the open successor. Its core is GPL-3.0 and its client libraries are **MIT, including the Rust crate `viiper-client`**. It emulates Xbox 360, DS4, DualSense, Switch 2 Pro and HID keyboard and mouse through the already-signed usbip-win2 driver, and claims latency "typically well below 1 millisecond." DS4Windows 5 and Handheld Companion have adopted it ([VIIPER](https://github.com/Alia5/VIIPER)).
- **LizardByte's driver** costs $14.99 a year and is source-available rather than open source ([LizardByte](https://app.lizardbyte.dev/2026-08-16-introducing-libvirtualhid-and-virtual-hid-driver/?lng=en-US)).
- **HidHide** (MIT) is still maintained and remains the standard way to hide a physical pad ([HidHide](https://github.com/nefarius/HidHide)).
- **Microsoft GameInput** gained motion and haptics support in v2.2, but no public API for creating virtual devices was found ([GameInput v2.2](https://developer.microsoft.com/en-us/games/articles/2025/09/gameinput-pc-update-adds-haptics-motion-and-more/)).

The code ZPadder would want to reuse is all under permissive licences: JoyShockMapper's semantics (MIT), GamepadMotionHelpers (MIT), the VIIPER clients (MIT), HidHide (MIT), ViGEmBus (BSD-3), BetterJoy (MIT) and DualSenseY-v2 (MIT). The GPL projects mainly offer controller HID protocols that SDL3 already implements. Those projects are AntiMicroX, DS4Windows, Joystick Gremlin, InputPlumber and makima ([JoyShockMapper licence](https://github.com/JibbSmart/JoyShockMapper); [DualSenseY-v2](https://github.com/WujekFoliarz/DualSenseY-v2)). That argues for licensing ZPadder **MIT OR Apache-2.0**.

## ZPadder should reach Xpadder parity on a pure core, then leapfrog it

### SDL3, SendInput and egui hold up, with three caveats

**SDL3 is the only input option that covers the whole matrix.** It handles XInput and generic DirectInput pads, and its HIDAPI drivers cover DS4, DualSense and Switch Pro. It also exposes gyro and accelerometer data, touchpads, rumble including trigger rumble, LEDs, battery level and the community mapping database ([SDL3 wiki: CategoryGamepad](https://wiki.libsdl.org/SDL3/CategoryGamepad)). A background mapper must set `SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1` before initialising SDL, because the default is off ([SDL3 wiki](https://wiki.libsdl.org/SDL3/SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS)). SDL's RAWINPUT driver correlates XInput and WGI data "for better trigger axes, guide button presses, and rumble support" and allows more than four Xbox pads ([mankier](https://www.mankier.com/3/SDL_HINT_JOYSTICK_RAWINPUT_CORRELATE_XINPUT)). This directly fixes Xpadder's Xbox One focus bug. The pure-Rust alternative, gilrs, is ruled out as the primary backend because its default Windows backend "requires an in focus window" ([docs.rs gilrs](https://docs.rs/gilrs/latest/gilrs/)).

The SDL3 plan comes with three caveats:

1. **The Rust bindings are immature.** The safe `sdl3` 0.22 wrapper is pre-1.0, and its README warns "Expect some bugs and missing features" ([sdl3-rs](https://github.com/vhspace/sdl3-rs)). ZPadder should instead write a thin in-house wrapper over **`sdl3-sys` 0.7.2** (Zlib licence, bundling SDL 3.4.18), and use its feature flags to disable video, audio, GPU, render and camera ([crates.io sdl3-sys](https://crates.io/crates/sdl3-sys)). Building from source needs CMake and a C compiler ([docs.rs sdl3-sys](https://docs.rs/crate/sdl3-sys/latest)). A local check of the development machine found Rust 1.99.1 and Visual Studio Build Tools 2022, with MSVC and a bundled CMake, already installed but not on PATH. The CI image also ships CMake 3.31.6 ([runner image](https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md)).
2. **Two behaviours are unverified.** Nobody has confirmed that background reading works on Windows 11, or whether SDL's RAWINPUT driver is on by default; the commit history conflicts ([SDL discourse](https://discourse.libsdl.org/t/sdl-disable-the-rawinput-joystick-driver-by-default/39395)).
3. **Coexistence with other mappers is untested.** It is unknown how SDL behaves alongside Steam Input, DS4Windows and HidHide. An Xpadder thread shows Steam Input silently taking over a Bluetooth PS5 pad ([t=6087](https://xpadder.com/forum4/viewtopic.php?t=6087)).

For DirectInput-only devices such as wheels, HOTAS sticks and dance mats, SDL's joystick layer (the `sdl-joystick` feature) exposes raw axes and hats. That layer is the natural target for imported Xpadder `Access` bindings.

**For output, call SendInput directly through `windows` 0.62, not through enigo.** Enigo describes itself as "early alpha" and would pull in a second copy of `windows` ([docs.rs enigo](https://docs.rs/enigo/latest/enigo/)). The output path should follow these rules:

- Send keys as `KEYEVENTF_SCANCODE` plus `KEYEVENTF_EXTENDEDKEY` for E0-prefixed keys, so input is independent of keyboard layout and reaches DirectInput games ([Microsoft Learn: KEYBDINPUT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-keybdinput)). AntiMicroX's Windows backend is a good reference for the special cases: VK_PAUSE needs a hand-coded scan code, and NumPad Enter must be told apart from Enter ([AntiMicroX winextras.cpp](https://github.com/AntiMicroX/antimicrox/blob/master/src/winextras.cpp)).
- Batch each engine tick into a single SendInput call, because events within one call are inserted serially without interleaving ([SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)).
- Carry sub-pixel remainders on mouse movement, so slow stick deflection still moves the cursor.
- Send wheel input in 120-unit notches, or in smaller steps for smooth scrolling ([MOUSEINPUT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-mouseinput); [WM_MOUSEWHEEL](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel)).
- Tag every event with a `dwExtraInfo` marker.
- Keep a record of held keys and release them all on profile switch, unplug or exit.

A `uiAccess` build is possible, but only as a signed binary installed under Program Files, and Microsoft reserves `uiAccess` for assistive technology ([Microsoft Learn: UIAccess security](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview)). The default response to elevated games should therefore be to detect them and warn the user.

**egui/eframe 0.37** fits Xpadder's picture-first UX closely. Its painter can draw the controller image, and hotspots that respond to click-and-drag map onto Xpadder's drag-to-place layout editor. It is licensed MIT OR Apache-2.0, has AccessKit on by default and needs Rust 1.95 or newer ([crates.io eframe](https://crates.io/crates/eframe)). The alternatives fall short:

- **iced 0.14** has had no release since December 2025, and its release notes never mention accessibility ([iced releases](https://github.com/iced-rs/iced/releases)).
- **Slint**'s royalty-free licence requires attribution and is not OSI-approved. SignPath Foundation, the free signing service, requires an "OSI-approved Open Source license without commercial dual-licensing" ([Slint terms](https://slint.dev/terms-and-conditions); [SignPath terms](https://signpath.org/terms)).
- **Tauri** brings in WebView2 and a JavaScript toolchain.

One upcoming breaking change matters: winit 0.31 is in beta ([crates.io winit](https://crates.io/crates/winit)).

The rest of the stack is uncontroversial:

| Concern | Choice (Nov 2026) | Why |
|---|---|---|
| Tray and menus | `tray-icon` 0.26.1 + `muda` 0.21.2 | Maintained by the Tauri project; MIT/Apache ([crates.io](https://crates.io/crates/tray-icon)) |
| AutoProfile | `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` → `QueryFullProcessImageNameW` | Event-driven, no polling, works on elevated targets ([Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)) |
| Single instance and CLI forwarding | `Local\` named mutex + `interprocess` 2.4.7 | `single-instance` has been unmaintained since 2021 ([crates.io interprocess](https://crates.io/crates/interprocess)) |
| Autostart | `auto-launch` 0.6 (HKCU Run); Task Scheduler for elevated mode | The HKCU Run key cannot elevate ([crates.io auto-launch](https://crates.io/crates/auto-launch)) |
| Native profiles | serde + `toml` 1.2 / `toml_edit` 0.25, `format_version` migrations, `schemars` 1.2 | Hand-editable, preserves comments, schema for editors ([crates.io toml](https://crates.io/crates/toml)) |
| Xpadder import | `rust-ini` 0.21.3 (MIT) | Keeps key order and arbitrary sections ([crates.io rust-ini](https://crates.io/crates/rust-ini)) |
| Packaging | `cargo-dist` 0.34 (zip + WiX v3 MSI), `winresource`, `embed-manifest` | Release automation; MSI supports WiX v3 only ([cargo-dist book](https://axodotdev.github.io/cargo-dist/book/installers/msi.html)) |
| Signing | SignPath Foundation (free for OSI licences); Azure Artifact Signing ($9.99/month) as fallback | Avoids the "reported as a virus" problem Xpadder had ([SignPath](https://signpath.org/); [Microsoft Learn](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)) |
| Testing | `insta` 1.49, `proptest` 1.12, `cargo-nextest` | Snapshot and property tests for a pure engine ([crates.io insta](https://crates.io/crates/insta)) |

### A thread-per-concern engine keeps idle CPU near zero and turbo accurate

Timing needs explicit design, because a tray app is exactly the kind of process Windows deprioritises. Since Windows 10 version 2004, `timeBeginPeriod` has been per-process. On Windows 11, a window-owning process that is "fully occluded, minimized, or otherwise invisible" gets no guarantee of higher timer resolution ([Microsoft Learn: timeBeginPeriod](https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timebeginperiod)). Rust's `std::thread::sleep` already uses `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`, which gives about 0.5 ms granularity ([rust-lang PR #116461](https://github.com/rust-lang/rust/pull/116461/)). `spin_sleep` can cover the sub-millisecond remainder ([docs.rs spin_sleep](https://docs.rs/crate/spin_sleep/latest/source/src/windows.rs)).

The recommended design uses four threads:

1. **Input thread.** Owns SDL and pumps events with a 1 ms timeout.
2. **Engine thread.** A pure state machine, `step(state, events, now) -> Vec<OutputAction>`. It blocks indefinitely when idle. While turbo, macros or stick-to-mouse are active, it ticks every 1 ms and emits one batched SendInput per tick.
3. **UI thread.** Runs eframe and the tray, and publishes edited profiles to the engine as `Arc<Profile>` through `arc-swap` ([crates.io arc-swap](https://crates.io/crates/arc-swap)).
4. **OS-events thread.** Runs a Win32 message loop for the foreground hook, device notifications and hotkeys.

Because the engine takes `now` as a parameter instead of reading the clock, it can be tested deterministically. Scripted input timelines run against a virtual clock, and `insta` snapshots record the resulting output streams. `proptest` checks invariants: every key-down has a key-up, switching profiles releases held outputs, and turbo never exceeds its rate.

The workspace should keep platform code out of the core so Linux CI can enforce portability from day one:

```
zpadder/
  crates/
    zpadder-model/           # pad IDs, HID-usage KeyCode, Action enum, Profile/Layer structs (serde)
    zpadder-engine/          # pure state machine; Clock trait; no OS deps
    zpadder-config/          # TOML load/save, format_version migrations, schema export
    zpadder-import-xpadder/  # rust-ini parser, image RLE decoder, key-name -> scan-code table
    zpadder-input/           # trait InputBackend; sdl3 (default), xinput fallback, replay (tests)
    zpadder-output/          # trait OutputSink; SendInput, later uinput / CGEvent, recording (tests)
    zpadder-platform/        # ForegroundWatcher, Autostart, SingleInstance, ElevationInfo
    zpadder-gui/             # eframe editor, on-screen keyboard popup, live overlay, tray
    zpadder/                 # binary: wiring, threads, CLI, logging
```

### Seven phases take ZPadder from spike to cross-platform

This roadmap changes the research notes' sketch in one deliberate way: **Xpadder import moves from the fourth milestone to the third phase (Phase 2), directly after the core.** The importer is OS-agnostic and cheap to write, since it is only INI parsing. More importantly, Xpadder's semantics are a battle-tested specification, and its files are a ready-made test corpus. If ZPadder's data model starts as a strict superset of Xpadder's, import-compatibility bugs cannot accumulate unnoticed.

| Phase | Scope | Exit criterion |
|---|---|---|
| **0. Spikes** | Build SDL3 statically, locally and in CI. Confirm background input while a game has focus on Windows 11. Confirm scan codes reach a fullscreen game. Test UIPI against an elevated target. Check timer precision while minimised. Test an XInput fallback | A written go/no-go for each risk |
| **1. Headless core (0.1)** | Data model as a superset of Xpadder's: N layers; typed timeline steps (Pause, Hold, Release, Distance, TapTime, Cycle); toggle, turbo and rumble modifiers; set selectors that can switch on press. Pure engine. TOML v1. SendInput sink. Button→key, stick→mouse with sub-pixel accumulation, stick→4/8-way keys, dead zones. Tray with enable/disable. Portable zip | The Notepad test and one game work from a hand-written profile; core crates pass on Ubuntu CI; no-stuck-keys property holds |
| **2. Xpadder import (0.2)** | Parse both file types; decode images to PNG; map key names to scan codes; map `Access` strings to SDL joystick bindings; split `Axis Z`/`Axis Z -` into independent triggers; paste-from-clipboard import | Every corpus file parses without panic; selected profiles match a licensed Xpadder's output on recorded timing tests |
| **3. Visual editor (0.3)** | egui controller canvas (imported 512×256 magenta-keyed art plus new PNG/SVG); drag-placed hotspots; assignment popup with on-screen keyboard and mouse picture; slot bar with special items, schedule text and test zone; set bar; stick settings with live dead-zone overlay; live pressed-button overlay | A new user builds a layout and profile without touching text |
| **4. Daily driver → signed 1.0** | AutoProfile (exe or title rules, per controller, default, ignore-self); Xpadder-compatible command line (`"P1" "P2" "Shortcut.lnk" /M /C`); single-instance forwarding; autostart; elevated-target warning; rumble; battery; SignPath signing, MSI and portable zip; docs on anti-cheat and the one official download site | Drop-in replacement for Xpadder in LaunchBox/RocketLauncher setups |
| **5. Beyond Xpadder (1.x)** | Editable response curves, outer dead zone, time-based acceleration; configurable spring radius and pivot; unlimited and cross-controller layers held on a stack (fixes the while-held glitch); double, long, start, release and chord activators with fire delays; gyro→mouse via a GamepadMotionHelpers port; flick stick; touchpad; scroll stick; multiple DPads; AccessKit labels on the canvas | Core aim features match JoyShockMapper and Steam Input on DS4, DualSense and Switch Pro |
| **6. Optional modules and platforms** | Opt-in VIIPER virtual-pad output per profile plus HidHide integration; Linux backend (evdev uinput, udev rule); macOS backend (CGEvent, Accessibility onboarding) | Fixes double input in games with native pad support; core runs unchanged on three OSes |

The highest risks should be retired during Phase 0:

- whether background input is reliable on Windows 11;
- whether the SDL3 bindings are mature enough;
- whether UIPI or anti-cheat silently drops injected input;
- whether Windows 11 throttles timers for a process that lives in the tray.

Licensing also has to be settled early, because it decides which GUI framework and signing route are available. The anti-cheat position should be explicit. ZPadder should **never try to disguise its input**. Turbo and macros should be opt-in and clearly labelled, since "macro" programs were the stated terms-of-service concern in the *For Honor* case ([GameRevolution](https://www.gamerevolution.com/news/13690-report-ubisoft-bans-for-honor-players-for-trying-to-remap-their-controller-xpadder)). Virtual-pad output should stay an uninstallable, per-profile module, because virtual-device drivers are what anti-cheat systems now block in reWASD's case.

## Conclusion

Xpadder's lasting value is not its code but two things it leaves behind: a coherent interaction model, and a large archive of profiles in a format that can now be decoded completely. Its successor gains the most by inheriting both. The model is a picture-first editor built on slot timelines and switchable sets. The archive is reachable through a faithful importer that is checked against a licensed copy of Xpadder as a timing oracle. Most of Xpadder's famous defects come from its 2000s architecture rather than its design, so SDL3, high-resolution timers and an unbounded data model fix them almost automatically. These include combined triggers, the Xbox One focus bug, the eight-set and one-DPad caps, and the drift in Windows 10 timing. Two limits are different. UIPI and anti-cheat visibility are inherent to any SendInput mapper, so ZPadder can only handle them through honest detection and documentation, not engineering. Treating them otherwise would turn ZPadder into the kind of cheat tooling that gets mappers banned.

Market timing matters as well. In 2026 the field polarised between tools tied to a platform or a subscription and free tools that have lost their maintainers, and much of the gap ZPadder can fill is about trust rather than features. The DS4Windows fork churn, GlosSI's death when ViGEm was retired, and the malware-laden mirrors around Xpadder's own name all show that signed builds, one canonical site, a permissive licence that keeps SignPath eligibility, and a bench of more than one maintainer are part of the product. A free, Steam-independent mapper that opens old Xpadder profiles on day one and adds gyro and editable curves later would be the first credible open-source successor in this space.
