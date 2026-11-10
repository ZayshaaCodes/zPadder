# Phase 0 spikes

Phase 0 retires the risks in the [roadmap](../../docs/roadmap.md) before real features are built on
top of them. Checks that need controllers or games are run by hand with `zpadder-probe` (in
[`spikes/zpadder-probe`](.)).

**Phase 0 closed on 2026-11-03.** The maintainer judged the hardware checks sufficient to
proceed. Rows still marked "Needs Test" below were not individually recorded and remain open
as watch items; the tests stay here for when one of them needs a closer look.

## Status

| Risk | Status | Finding |
|---|---|---|
| SDL3 builds from source | **Go** | `sdl3-sys` 0.7.2 builds SDL 3.4.18 statically with CMake in ~36 s. Audio, video, GPU, render, camera, dialog and tray are compiled out. No `SDL3.dll` to ship |
| SDL vs Rust C runtime | **Fixed** | At opt-level 0 the `cmake` crate built SDL in Debug, linking the debug CRT. `[profile.dev.package.sdl3-sys] opt-level = 2` builds it RelWithDebInfo; the probe now depends only on the release CRT |
| SDL's default Windows drivers | **Answered** | From SDL 3.4.18 source: XInput **on**, RAWINPUT **off** (`SDL_HINT_JOYSTICK_RAWINPUT` defaults to false), Windows.Gaming.Input **off**, DirectInput and HIDAPI on |
| SDL drops background input | **Low risk** | SDL only discards joystick events when it owns windows and none has focus. With video compiled out it owns no windows. ZPadder sets `SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1` anyway |
| DirectInput exclusivity | **Open** | SDL opens DirectInput devices with `DISCL_EXCLUSIVE \| DISCL_BACKGROUND`. A game asking for exclusive access to the same generic pad could fail. Needs a test with a DirectInput-only pad |
| DualShock 4 support | **Go** | A DS4 (054c:09cc) is detected through SDL's HIDAPI driver with gyro and rumble reported. Every button (including touchpad click and both stick clicks), both sticks and both analog triggers report correctly, and the rumble command is accepted |
| Timer precision (console process) | **Go** | See baseline below. Deadline scheduling holds turbo timing exactly; fixed sleeps drift +0.6% |
| Timer precision (tray app with hidden window) | **Deferred** | Needs the real app's hidden eframe window; re-measure in Phase 1 |
| Background input while a game has focus | **Partly confirmed** | DS4 input kept arriving while another process (VS Code) owned the foreground window. Still needed: a fullscreen game (Test 1, step 4), and whether the rumble was actually felt |
| Analog jitter at thresholds | **Designed for** | The log shows values flickering across a boundary (0.62 ↔ 0.63). The engine gives every analog threshold 0.05 of hysteresis |
| Coexistence with Steam Input / DS4Windows / HidHide | **Needs Test 2** | |
| Scan codes reach games; pointer acceleration | **Needs Test 3** | |
| UIPI silently drops input to elevated windows | **Needs Test 4** | Elevation detection works for normal windows |
| Timer precision while a game runs | **Needs Test 5** | |

### Timer baseline

Measured on the development machine, console process, nothing demanding in the foreground:

| Request | Median | p90 | p99 | Max |
|---|---|---|---|---|
| `sleep(500 µs)` | 1.01 ms | 1.09 ms | 1.42 ms | 1.57 ms |
| `sleep(1 ms)` | 1.52 ms | 1.56 ms | 1.99 ms | 2.42 ms |
| `sleep(4 ms)` | 4.10 ms | 4.44 ms | 4.74 ms | 5.06 ms |

20 Hz turbo for 100 presses (ideal 5.000 s): fixed sleeps 5.030 s (+0.6%); deadline scheduling
5.000 s (+0.0%).

**Consequence for the engine:** sleeps overshoot by about 0.5 ms, so the engine must schedule
against absolute deadlines (as the roadmap planned) and treat "1 ms tick" as roughly 1.5 ms.
That is well under a 240 Hz frame (4.2 ms). Add `spin_sleep` only if Test 5 shows worse numbers.

## Running the probe

```powershell
cargo build -p zpadder-probe
.\target\debug\zpadder-probe.exe help
```

Every command writes `probe-<command>.log` in the current folder. **The logs include window
titles** (chat channels, browser tabs, and so on), so skim them before sharing.

## Test checklist

Report back with the log files and anything you noticed. Tests can be done in any order; skip
the parts that need hardware you don't have.

### Test 1: background input

1. Run `zpadder-probe input` with the DualShock 4 connected (and any other pads you have:
   Xbox, Switch Pro, generic USB).
2. Press buttons and move sticks in the probe's window. Lines appear.
3. Click into another program (e.g. Notepad) and keep pressing. Lines must keep appearing.
4. Start a game. Try it fullscreen and, if the game offers it, exclusive fullscreen. Keep
   pressing buttons for ~10 s, then switch back. The log should show a `foreground:` line naming
   the game, followed by button lines with timestamps from while the game had focus.
5. Press North (Triangle / Y) in the probe window and again while the game has focus. You
   should feel a short rumble both times.
6. Unplug and replug the pad (or turn Bluetooth off and on). `disconnected` / `connected` lines
   should appear.
7. If you have two or more Xbox One / Series pads, repeat steps 3–4 with both. This is the case
   that breaks Xpadder.

Report: whether input continued in step 3 and step 4, whether rumble worked in step 5, and
`probe-input.log`.

### Test 2: coexistence

1. With Steam running, check Steam → Settings → Controller → "Enable Steam Input for
   PlayStation controllers". Run Test 1 steps 1–3 with it **on** and with it **off**.
2. If DS4Windows or HidHide is installed, run Test 1 steps 1–3 with each of them active.
3. Start a game that supports the DS4 natively while `zpadder-probe input` runs. Does the game
   still respond to the pad?

Report: for each case, whether the probe saw input and whether anything else stopped working.

### Test 3: output reaches programs and games

Use two terminals.

1. Terminal A: `zpadder-probe keyhook`.
2. Terminal B: `zpadder-probe output keys`, then click into an empty Notepad window within
   5 seconds. Expected text: `zpadder 123`, a new line, then `Z`.
3. Terminal B: `zpadder-probe output special`, then click into Notepad. Expected: a line
   `axbc` and a line `ok`. A stray `4` or `6` means the arrow keys lost their extended flag.
   The Num Lock light should blink off and on, and Print Screen should open the capture overlay
   or copy a screenshot (press Esc). In terminal A, every event should say `from ZPadder`, and
   Pause should show `vk=0x13`.
4. Pointer acceleration: open Settings → Bluetooth & devices → Mouse → Additional mouse
   settings → Pointer Options and note whether **Enhance pointer precision** is ticked. Run
   `zpadder-probe output mouse` with the pointer mid-screen. Then flip the setting, run it
   again, and put the setting back.
5. In a game where you can walk and look around: `zpadder-probe output game --delay 10`, then
   switch to the game. The character should walk forward for 1 s, jump, turn right and fire.
   If you can, try one older game (DirectInput era) and one recent one.

Report: whether the text in steps 2–3 matched, the `requested … moved …` lines from both runs
in step 4, what happened in each game in step 5, and `probe-keyhook.log`.

### Test 4: elevated windows (UIPI)

1. Run `zpadder-probe foreground`. Within 60 s, focus a normal Notepad, then a Notepad opened
   with right-click → **Run as administrator**. The second should be reported as `elevated`
   with a WARNING.
2. Run `zpadder-probe output keys` and click into the **elevated** Notepad. Expected: nothing
   is typed.
3. Open a terminal **as administrator** and repeat step 2 from there. Expected: the text
   appears.

Report: what was typed in steps 2 and 3, and `probe-foreground.log`.

### Test 5: timing under load

Run `zpadder-probe timer`, switch to a running game within a couple of seconds, play for about
20 seconds, then switch back.

Report: `probe-timer.log`, to compare with the baseline above.
