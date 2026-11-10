use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use proptest::prelude::*;
use zpadder_model::xbox::buttons as xb;
use zpadder_model::{
    Axis, Button, InputEvent, KeyCode, MouseButton, OutputAction, PadId, Profile, XboxState,
};

use crate::Engine;

use OutputAction::{KeyDown, KeyUp};

const PAD: PadId = PadId(1);

fn profile(text: &str) -> Arc<Profile> {
    let profile = unvalidated(text);
    profile.validate().unwrap();
    profile
}

/// A profile that skips validation, to check that the engine stays safe with values that
/// validation would refuse.
fn unvalidated(text: &str) -> Arc<Profile> {
    Arc::new(toml::from_str(text).unwrap())
}

/// Drives an engine with a virtual clock.
struct Rig {
    engine: Engine,
    now: Duration,
}

impl Rig {
    fn new(text: &str) -> Self {
        Self::with(profile(text))
    }

    fn with(profile: Arc<Profile>) -> Self {
        let mut engine = Engine::new();
        let mut out = Vec::new();
        engine.set_profile(Some(profile), &mut out);
        assert!(out.is_empty());
        Self { engine, now: Duration::ZERO }
    }

    fn set_profile(&mut self, text: &str) -> Vec<OutputAction> {
        let mut out = Vec::new();
        self.engine.set_profile(Some(profile(text)), &mut out);
        out
    }

    fn at(&mut self, ms: u64) -> &mut Self {
        self.now = Duration::from_millis(ms);
        self
    }

    fn events(&mut self, events: &[InputEvent]) -> Vec<OutputAction> {
        let mut out = Vec::new();
        self.engine.step(events, self.now, &mut out);
        out
    }

    fn press(&mut self, button: Button) -> Vec<OutputAction> {
        self.events(&[InputEvent::Button { pad: PAD, button, pressed: true }])
    }

    fn release(&mut self, button: Button) -> Vec<OutputAction> {
        self.events(&[InputEvent::Button { pad: PAD, button, pressed: false }])
    }

    fn axes(&mut self, values: &[(Axis, f32)]) -> Vec<OutputAction> {
        let events: Vec<_> = values
            .iter()
            .map(|&(axis, value)| InputEvent::Axis { pad: PAD, axis, value })
            .collect();
        self.events(&events)
    }

    fn tick(&mut self) -> Vec<OutputAction> {
        self.events(&[])
    }
}

const BASIC: &str = r#"
    [[layers]]
    name = "Main"
    [layers.buttons.south]
    slots = ["space"]
    [layers.buttons.east]
    slots = ["left_shift", "x"]
    [layers.buttons.west]
    slots = ["space"]
    [layers.buttons.north]
    slots = ["mouse_left"]
    [layers.buttons.dpad_up]
    slots = ["wheel_up"]
"#;

#[test]
fn button_presses_and_releases_its_key() {
    let mut rig = Rig::new(BASIC);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::SPACE)]);
    assert_eq!(rig.release(Button::South), [KeyUp(KeyCode::SPACE)]);
    assert_eq!(rig.release(Button::South), []);
}

#[test]
fn combination_goes_down_in_order_and_up_in_reverse() {
    let mut rig = Rig::new(BASIC);
    assert_eq!(rig.press(Button::East), [KeyDown(KeyCode::LEFT_SHIFT), KeyDown(KeyCode::X)]);
    assert_eq!(rig.release(Button::East), [KeyUp(KeyCode::X), KeyUp(KeyCode::LEFT_SHIFT)]);
}

#[test]
fn shared_key_stays_down_until_the_last_holder_releases() {
    let mut rig = Rig::new(BASIC);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::SPACE)]);
    assert_eq!(rig.press(Button::West), []);
    assert_eq!(rig.release(Button::South), []);
    assert_eq!(rig.release(Button::West), [KeyUp(KeyCode::SPACE)]);
}

#[test]
fn mouse_buttons_and_wheel() {
    let mut rig = Rig::new(BASIC);
    let down = OutputAction::MouseButton { button: MouseButton::Left, pressed: true };
    let up = OutputAction::MouseButton { button: MouseButton::Left, pressed: false };
    assert_eq!(rig.press(Button::North), [down]);
    assert_eq!(rig.release(Button::North), [up]);
    assert_eq!(rig.press(Button::DPadUp), [OutputAction::Wheel { vertical: 120, horizontal: 0 }]);
    assert_eq!(rig.release(Button::DPadUp), []);
}

#[test]
fn toggle_latches_until_the_next_press() {
    let mut rig = Rig::new(
        r#"
        [[layers]]
        name = "Main"
        [layers.buttons.south]
        slots = ["left_shift"]
        toggle = true
    "#,
    );
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::LEFT_SHIFT)]);
    assert_eq!(rig.release(Button::South), []);
    assert_eq!(rig.press(Button::South), [KeyUp(KeyCode::LEFT_SHIFT)]);
    assert_eq!(rig.release(Button::South), []);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::LEFT_SHIFT)]);
}

#[test]
fn turbo_repeats_on_schedule_and_stops_on_release() {
    let mut rig = Rig::new(
        r#"
        [[layers]]
        name = "Main"
        [layers.buttons.south]
        slots = ["x"]
        turbo = 10.0
    "#,
    );
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::X)]);
    assert_eq!(rig.engine.next_wakeup(), Some(Duration::from_millis(50)));
    assert_eq!(rig.at(49).tick(), []);
    assert_eq!(rig.at(50).tick(), [KeyUp(KeyCode::X)]);
    assert_eq!(rig.at(100).tick(), [KeyDown(KeyCode::X)]);
    assert_eq!(rig.at(151).tick(), [KeyUp(KeyCode::X)]);
    // The schedule keeps its deadlines: the next flip is at 200, not 201.
    assert_eq!(rig.engine.next_wakeup(), Some(Duration::from_millis(200)));
    assert_eq!(rig.at(160).release(Button::South), []);
    assert_eq!(rig.engine.next_wakeup(), None);
}

#[test]
fn turbo_skips_missed_flips_instead_of_bursting() {
    let mut rig = Rig::new(
        r#"
        [[layers]]
        name = "Main"
        [layers.buttons.south]
        slots = ["x"]
        turbo = 10.0
    "#,
    );
    rig.press(Button::South);
    assert_eq!(rig.at(1000).tick(), [KeyUp(KeyCode::X)]);
    assert_eq!(rig.engine.next_wakeup(), Some(Duration::from_millis(1050)));
}

const LAYERS: &str = r#"
    [[layers]]
    name = "Main"
    [layers.buttons.south]
    slots = ["a"]
    [layers.buttons.left_shoulder]
    layer = { target = "Shift", mode = "while_held" }
    [layers.buttons.right_shoulder]
    layer = { target = "Other", mode = "while_held" }
    [layers.buttons.start]
    layer = { target = "Other", mode = "two_way" }
    [layers.buttons.back]
    layer = { target = "Shift" }

    [[layers]]
    name = "Shift"
    [layers.buttons.south]
    slots = ["b"]
    [layers.buttons.right_shoulder]
    layer = { target = "Other", mode = "while_held" }
    [layers.buttons.back]
    layer = { target = "Main" }

    [[layers]]
    name = "Other"
    [layers.buttons.south]
    slots = ["c"]
    [layers.buttons.start]
    layer = { target = "Other", mode = "two_way" }
"#;

#[test]
fn while_held_layer_applies_only_while_held() {
    let mut rig = Rig::new(LAYERS);
    rig.press(Button::LeftShoulder);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::B)]);
    assert_eq!(rig.release(Button::South), [KeyUp(KeyCode::B)]);
    rig.release(Button::LeftShoulder);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::A)]);
}

#[test]
fn a_press_owns_its_release_across_layer_changes() {
    let mut rig = Rig::new(LAYERS);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::A)]);
    rig.press(Button::LeftShoulder);
    // South was pressed in Main, so releasing it lifts A, not Shift's B.
    assert_eq!(rig.release(Button::South), [KeyUp(KeyCode::A)]);
}

#[test]
fn while_held_layers_release_in_any_order() {
    // Xpadder's "while held glitch": releasing the first selector before the second used to
    // leave the pad in the wrong set.
    let mut rig = Rig::new(LAYERS);
    rig.press(Button::LeftShoulder); // Main -> Shift
    rig.press(Button::RightShoulder); // Shift -> Other
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::C)]);
    rig.release(Button::South);
    rig.release(Button::LeftShoulder);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::C)]); // Other is still held
    rig.release(Button::South);
    rig.release(Button::RightShoulder);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::A)]); // back to Main
}

#[test]
fn two_way_and_one_way_selectors() {
    let mut rig = Rig::new(LAYERS);
    rig.press(Button::Start);
    rig.release(Button::Start);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::C)]);
    rig.release(Button::South);
    rig.press(Button::Start);
    rig.release(Button::Start);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::A)]);
    rig.release(Button::South);

    rig.press(Button::Back);
    rig.release(Button::Back);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::B)]);
    rig.release(Button::South);
    rig.press(Button::Back);
    rig.release(Button::Back);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::A)]);
}

const STICKS: &str = r#"
    [[layers]]
    name = "Main"
    [layers.left_stick]
    mode = "directions"
    [layers.left_stick.directions]
    up = { slots = ["w"] }
    down = { slots = ["s"] }
    left = { slots = ["a"] }
    right = { slots = ["d"] }
    [layers.right_stick]
    mode = "mouse"
    dead_zone = 0.2
    mouse = { speed = 1000.0 }
    [layers.right_trigger]
    slots = ["mouse_left"]
"#;

#[test]
fn stick_directions_press_and_release_keys() {
    let mut rig = Rig::new(STICKS);
    assert_eq!(rig.axes(&[(Axis::LeftY, -1.0)]), [KeyDown(KeyCode::W)]);
    // Rolling to the up-right diagonal adds D; X and Y arriving together count as one move.
    assert_eq!(rig.axes(&[(Axis::LeftX, 0.7), (Axis::LeftY, -0.7)]), [KeyDown(KeyCode::D)]);
    assert_eq!(rig.axes(&[(Axis::LeftX, 1.0), (Axis::LeftY, 0.0)]), [KeyUp(KeyCode::W)]);
    assert_eq!(rig.axes(&[(Axis::LeftX, 0.0)]), [KeyUp(KeyCode::D)]);
}

#[test]
fn stick_mouse_integrates_speed_over_time_with_sub_pixel_carry() {
    let mut rig = Rig::new(STICKS);
    assert_eq!(rig.axes(&[(Axis::RightX, 1.0)]), []);
    assert_eq!(rig.engine.next_wakeup(), Some(Duration::from_millis(1)));
    let mut total = 0;
    for ms in 1..=1000 {
        for action in rig.at(ms).tick() {
            if let OutputAction::MouseMove { dx, dy } = action {
                assert_eq!(dy, 0);
                total += dx;
            }
        }
    }
    assert!((999..=1000).contains(&total), "moved {total} px in 1 s at 1000 px/s");

    // Barely past the dead zone: well under a pixel per tick, but it still adds up.
    rig.axes(&[(Axis::RightX, 0.21)]);
    let mut total = 0;
    for ms in 1001..=2000 {
        for action in rig.at(ms).tick() {
            if let OutputAction::MouseMove { dx, .. } = action {
                total += dx;
            }
        }
    }
    assert!((11..=13).contains(&total), "moved {total} px");

    rig.axes(&[(Axis::RightX, 0.0)]);
    assert_eq!(rig.engine.next_wakeup(), None);
}

#[test]
fn absurd_mouse_speeds_are_capped() {
    // Validation caps the speed at 20 000 px/s; without it the engine still does.
    for (speed, expected) in [("1e30", 1000), ("inf", 1000), ("nan", 0)] {
        let mut rig = Rig::with(unvalidated(&format!(
            "[[layers]]\nname = \"Main\"\n[layers.right_stick]\nmode = \"mouse\"\n\
             mouse = {{ speed = {speed} }}\n"
        )));
        rig.axes(&[(Axis::RightX, 1.0)]);
        // 50 ms is the longest step integrated at once: 1000 px at 20 000 px/s.
        let moved: i32 = rig
            .at(50)
            .tick()
            .iter()
            .map(|action| match action {
                OutputAction::MouseMove { dx, dy: 0 } => *dx,
                other => panic!("{other:?}"),
            })
            .sum();
        assert_eq!(moved, expected, "at speed {speed}");
    }
}

#[test]
fn trigger_threshold_has_hysteresis() {
    let mut rig = Rig::new(STICKS);
    let down = OutputAction::MouseButton { button: MouseButton::Left, pressed: true };
    let up = OutputAction::MouseButton { button: MouseButton::Left, pressed: false };
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.19)]), []);
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.21)]), [down]);
    // Jitter around the threshold does not release...
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.18)]), []);
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.21)]), []);
    // ...but letting go does.
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.1)]), [up]);
}

#[test]
fn profile_change_and_disconnect_release_everything() {
    let mut rig = Rig::new(BASIC);
    rig.press(Button::East);
    let mut out = Vec::new();
    rig.engine.set_profile(Some(profile(BASIC)), &mut out);
    assert_eq!(out, [KeyUp(KeyCode::X), KeyUp(KeyCode::LEFT_SHIFT)]);
    // The button still held from before the change does nothing when released.
    assert_eq!(rig.release(Button::East), []);

    rig.press(Button::South);
    assert_eq!(rig.events(&[InputEvent::Disconnected { pad: PAD }]), [KeyUp(KeyCode::SPACE)]);
}

#[test]
fn held_stick_takes_effect_under_a_new_profile() {
    let mut rig = Rig::new(BASIC);
    rig.axes(&[(Axis::LeftY, -1.0)]);
    let mut out = Vec::new();
    rig.engine.set_profile(Some(profile(STICKS)), &mut out);
    assert_eq!(out, [KeyDown(KeyCode::W)]);
}

// ---- Timing slots ----

fn one_button(slots: &str) -> Rig {
    let slots: Vec<String> = slots.split(',').map(|s| format!("\"{}\"", s.trim())).collect();
    Rig::new(&format!(
        "[[layers]]\nname = \"Main\"\n[layers.buttons.south]\nslots = [{}]\n",
        slots.join(", ")
    ))
}

#[test]
fn sequence_taps_with_pauses_and_ignores_release() {
    let mut rig = one_button("a, pause 0.2s, b");
    assert_eq!(rig.at(0).press(Button::South), [KeyDown(KeyCode::A)]);
    assert_eq!(rig.at(10).release(Button::South), []);
    assert_eq!(rig.engine.next_wakeup(), Some(Duration::from_millis(100)));
    assert_eq!(rig.at(100).tick(), [KeyUp(KeyCode::A)]);
    assert_eq!(rig.engine.next_wakeup(), Some(Duration::from_millis(300)));
    assert_eq!(rig.at(300).tick(), [KeyDown(KeyCode::B)]);
    assert_eq!(rig.at(400).tick(), [KeyUp(KeyCode::B)]);
    assert_eq!(rig.engine.next_wakeup(), None);
}

#[test]
fn pressing_again_stops_a_running_sequence() {
    let mut rig = one_button("a, pause 0.2s, b");
    rig.at(0).press(Button::South);
    rig.at(10).release(Button::South);
    assert_eq!(rig.at(50).press(Button::South), [KeyUp(KeyCode::A)]);
    assert_eq!(rig.at(60).release(Button::South), []);
    assert_eq!(rig.at(300).tick(), []);
    assert_eq!(rig.engine.next_wakeup(), None);
    // Once stopped, the next press starts it afresh.
    assert_eq!(rig.at(400).press(Button::South), [KeyDown(KeyCode::A)]);
}

#[test]
fn overdue_sequence_never_presses_and_releases_in_one_batch() {
    let mut rig = one_button("a, pause 0.2s, b");
    rig.at(0).press(Button::South);
    // The engine stalls for a second: A is released, then the pause is restarted from now.
    assert_eq!(rig.at(1000).tick(), [KeyUp(KeyCode::A)]);
    assert_eq!(rig.at(1000).tick(), [KeyDown(KeyCode::B)]);
    assert_eq!(rig.at(1100).tick(), [KeyUp(KeyCode::B)]);
}

#[test]
fn tap_time_sets_tap_length() {
    let mut rig = one_button("tap_time 0.03s, a, b");
    assert_eq!(rig.at(0).press(Button::South), [KeyDown(KeyCode::A), KeyDown(KeyCode::B)]);
    assert_eq!(rig.at(30).tick(), [KeyUp(KeyCode::B), KeyUp(KeyCode::A)]);
}

const TAP_OR_HOLD: &str = "hold 0.5s, y, release 0.01s, x, release 0.25s";

#[test]
fn tap_or_hold_tap() {
    let mut rig = one_button(TAP_OR_HOLD);
    assert_eq!(rig.at(0).press(Button::South), []);
    assert_eq!(rig.at(100).release(Button::South), [KeyDown(KeyCode::X)]);
    assert_eq!(rig.at(200).tick(), [KeyUp(KeyCode::X)]);
}

#[test]
fn tap_or_hold_hold() {
    let mut rig = one_button(TAP_OR_HOLD);
    rig.at(0).press(Button::South);
    assert_eq!(rig.engine.next_wakeup(), Some(Duration::from_millis(500)));
    assert_eq!(rig.at(499).tick(), []);
    assert_eq!(rig.at(500).tick(), [KeyDown(KeyCode::Y)]);
    assert_eq!(rig.at(800).release(Button::South), [KeyUp(KeyCode::Y)]);
    assert_eq!(rig.engine.next_wakeup(), None);
}

#[test]
fn tap_or_hold_in_between_does_nothing() {
    let mut rig = one_button(TAP_OR_HOLD);
    rig.at(0).press(Button::South);
    assert_eq!(rig.at(300).release(Button::South), []);
    assert_eq!(rig.engine.next_wakeup(), None);
}

#[test]
fn hold_zones_replace_each_other() {
    let mut rig = one_button("a, hold 0.5s, left_shift, a");
    assert_eq!(rig.at(0).press(Button::South), [KeyDown(KeyCode::A)]);
    // A stays down across the change because the new zone also holds it.
    assert_eq!(rig.at(500).tick(), [KeyDown(KeyCode::LEFT_SHIFT)]);
    assert_eq!(rig.at(600).release(Button::South), [KeyUp(KeyCode::A), KeyUp(KeyCode::LEFT_SHIFT)]);
}

#[test]
fn cycle_alternates_between_presses() {
    let mut rig = one_button("a, cycle, b, cycle, c");
    for key in [KeyCode::A, KeyCode::B, KeyCode::C, KeyCode::A] {
        assert_eq!(rig.press(Button::South), [KeyDown(key)]);
        assert_eq!(rig.release(Button::South), [KeyUp(key)]);
    }
}

#[test]
fn distance_zones_follow_trigger_pull() {
    let mut rig = Rig::new(
        r#"
        [[layers]]
        name = "Main"
        trigger_threshold = 0.1
        [layers.right_trigger]
        slots = ["left_ctrl", "w", "distance 40%", "left_shift", "w", "distance 80%", "w"]
    "#,
    );
    assert_eq!(
        rig.axes(&[(Axis::RightTrigger, 0.2)]),
        [KeyDown(KeyCode::LEFT_CTRL), KeyDown(KeyCode::W)]
    );
    // W stays down while the modifier changes; Ctrl comes up before Shift goes down.
    assert_eq!(
        rig.axes(&[(Axis::RightTrigger, 0.5)]),
        [KeyUp(KeyCode::LEFT_CTRL), KeyDown(KeyCode::LEFT_SHIFT)]
    );
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.9)]), [KeyUp(KeyCode::LEFT_SHIFT)]);
    // Resting just under a zone edge does not drop back...
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.78)]), []);
    // ...but easing off does.
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.7)]), [KeyDown(KeyCode::LEFT_SHIFT)]);
    assert_eq!(
        rig.axes(&[(Axis::RightTrigger, 0.0)]),
        [KeyUp(KeyCode::W), KeyUp(KeyCode::LEFT_SHIFT)]
    );
}

#[test]
fn distance_zones_on_a_stick_walk_then_run() {
    let mut rig = Rig::new(
        r#"
        [[layers]]
        name = "Main"
        [layers.left_stick]
        mode = "directions"
        [layers.left_stick.directions]
        up = { slots = ["w", "distance 90%", "left_shift", "w"] }
    "#,
    );
    assert_eq!(rig.axes(&[(Axis::LeftY, -0.5)]), [KeyDown(KeyCode::W)]);
    assert_eq!(rig.axes(&[(Axis::LeftY, -1.0)]), [KeyDown(KeyCode::LEFT_SHIFT)]);
    assert_eq!(rig.axes(&[(Axis::LeftY, 0.0)]), [KeyUp(KeyCode::W), KeyUp(KeyCode::LEFT_SHIFT)]);
}

#[test]
fn scroll_stick_scrolls_smoothly() {
    let mut rig = Rig::new(
        r#"
        [[layers]]
        name = "Main"
        [layers.right_stick]
        mode = "scroll"
        dead_zone = 0.2
        scroll = { speed = 10.0 }
    "#,
    );
    rig.axes(&[(Axis::RightY, -1.0)]);
    let mut total = 0;
    for ms in 1..=1000 {
        for action in rig.at(ms).tick() {
            match action {
                OutputAction::Wheel { vertical, horizontal } => {
                    assert!(vertical >= 40 && horizontal == 0, "{action:?}");
                    total += vertical;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }
    // 10 notches of 120 in one second, give or take a step still accumulating.
    assert!((1160..=1200).contains(&total), "scrolled {total}");
    rig.axes(&[(Axis::RightY, 0.0)]);
    assert_eq!(rig.engine.next_wakeup(), None);
}

#[test]
fn rumble_follows_held_assignments() {
    let mut rig = Rig::new(
        r#"
        [[layers]]
        name = "Main"
        [layers.buttons.south]
        slots = ["space"]
        rumble = true
        [layers.buttons.east]
        slots = ["x"]
        rumble = true
        toggle = true
    "#,
    );
    rig.press(Button::South);
    assert_eq!(rig.engine.take_rumble_changes(), [(PAD, true)]);
    rig.press(Button::East);
    rig.release(Button::South);
    // The latched toggle keeps the rumble going.
    rig.release(Button::East);
    assert_eq!(rig.engine.take_rumble_changes(), []);
    rig.press(Button::East);
    assert_eq!(rig.engine.take_rumble_changes(), [(PAD, false)]);
    assert_eq!(rig.engine.active_layer(PAD), Some(0));
}

// ---- Analog controls at rest, and layers selected by analog controls ----

#[test]
fn small_trigger_thresholds_still_let_go_at_rest() {
    let text = |threshold: f32| {
        format!(
            "[[layers]]\nname = \"Main\"\ntrigger_threshold = {threshold}\n\
             [layers.right_trigger]\nslots = [\"x\"]\n"
        )
    };
    // At the smallest threshold validation allows, a trigger lets go at half of it, not at 0.
    let mut rig = Rig::new(&text(0.05));
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 1.0)]), [KeyDown(KeyCode::X)]);
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.03)]), []);
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.01)]), [KeyUp(KeyCode::X)]);
    // Below it (only possible without validation), a trigger at rest still lets go.
    let mut rig = Rig::with(unvalidated(&text(0.03)));
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 1.0)]), [KeyDown(KeyCode::X)]);
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.002)]), [KeyUp(KeyCode::X)]);
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.01)]), []);
}

#[test]
fn small_dead_zones_still_let_go_at_rest() {
    let text = |dead_zone: f32| {
        format!(
            "[[layers]]\nname = \"Main\"\n[layers.left_stick]\nmode = \"directions\"\n\
             dead_zone = {dead_zone}\n[layers.left_stick.directions]\n\
             right = {{ slots = [\"d\"] }}\n"
        )
    };
    let mut rig = Rig::new(&text(0.05));
    assert_eq!(rig.axes(&[(Axis::LeftX, 1.0)]), [KeyDown(KeyCode::D)]);
    assert_eq!(rig.axes(&[(Axis::LeftX, 0.01)]), [KeyUp(KeyCode::D)]);
    // A dead zone of 0 (refused by validation) would press on noise and never let go.
    let mut rig = Rig::with(unvalidated(&text(0.0)));
    assert_eq!(rig.axes(&[(Axis::LeftX, 0.01)]), []);
    assert_eq!(rig.axes(&[(Axis::LeftX, 1.0)]), [KeyDown(KeyCode::D)]);
    assert_eq!(rig.axes(&[(Axis::LeftX, 0.01), (Axis::LeftY, 0.005)]), [KeyUp(KeyCode::D)]);
}

#[test]
fn low_distance_zones_release_proportionally() {
    let mut rig = Rig::new(
        r#"
        [[layers]]
        name = "Main"
        trigger_threshold = 0.05
        [layers.right_trigger]
        slots = ["a", "distance 8%", "b"]
    "#,
    );
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.5)]), [KeyDown(KeyCode::B)]);
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.05)]), []);
    // The zone lets go at half its 8% edge; a fixed 5% margin would hold it down to 3%.
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.035)]), [KeyUp(KeyCode::B), KeyDown(KeyCode::A)]);
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.0)]), [KeyUp(KeyCode::A)]);
}

const TRIGGER_SHIFT: &str = r#"
    [[layers]]
    name = "Main"
    [layers.buttons.south]
    slots = ["a"]
    [layers.buttons.left_shoulder]
    layer = { target = "Shift", mode = "while_held" }
    [layers.left_trigger]
    layer = { target = "Shift", mode = "while_held" }
    [layers.right_trigger]
    slots = ["mouse_left"]

    [[layers]]
    name = "Shift"
    trigger_threshold = 0.6
    [layers.buttons.south]
    slots = ["b"]
"#;

#[test]
fn a_trigger_holding_a_layer_keeps_the_threshold_it_was_pulled_past() {
    let mut rig = Rig::new(TRIGGER_SHIFT);
    // Past Main's threshold (0.2) but not Shift's (0.6): Shift must not release it.
    rig.axes(&[(Axis::LeftTrigger, 0.4)]);
    for value in [0.41, 0.39, 0.4, 0.2] {
        assert_eq!(rig.axes(&[(Axis::LeftTrigger, value)]), []);
        assert_eq!(rig.engine.active_layer(PAD), Some(1), "at {value}");
    }
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::B)]);
    rig.release(Button::South);
    rig.axes(&[(Axis::LeftTrigger, 0.0)]);
    assert_eq!(rig.engine.active_layer(PAD), Some(0));
}

#[test]
fn a_held_trigger_survives_a_layer_change() {
    let mut rig = Rig::new(TRIGGER_SHIFT);
    let down = OutputAction::MouseButton { button: MouseButton::Left, pressed: true };
    let up = OutputAction::MouseButton { button: MouseButton::Left, pressed: false };
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.3)]), [down]);
    // Shift's threshold is higher, but the trigger was pulled in Main: no release, no re-click.
    assert_eq!(rig.press(Button::LeftShoulder), []);
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.31)]), []);
    assert_eq!(rig.release(Button::LeftShoulder), []);
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.0)]), [up]);
}

#[test]
fn a_stick_holding_a_layer_keeps_the_settings_it_was_pushed_with() {
    let mut rig = Rig::new(
        r#"
        [[layers]]
        name = "Main"
        [layers.left_stick]
        mode = "directions"
        [layers.left_stick.directions]
        up = { layer = { target = "Shift", mode = "while_held" } }
        [layers.buttons.south]
        slots = ["a"]

        [[layers]]
        name = "Shift"
        [layers.left_stick]
        mode = "mouse"
        dead_zone = 0.9
        [layers.buttons.south]
        slots = ["b"]
    "#,
    );
    rig.axes(&[(Axis::LeftY, -1.0)]);
    // In Shift the stick moves the pointer, but while it holds Shift it still counts as pushed
    // up (with Main's dead zone), so it neither drops the layer nor moves the pointer.
    for (ms, value) in [(10, -0.98), (20, -1.0), (30, -0.5)] {
        assert_eq!(rig.at(ms).axes(&[(Axis::LeftY, value)]), []);
        assert_eq!(rig.engine.active_layer(PAD), Some(1), "at {value}");
    }
    assert_eq!(rig.engine.next_wakeup(), None);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::B)]);
    rig.release(Button::South);
    assert_eq!(rig.at(40).axes(&[(Axis::LeftY, 0.0)]), []);
    assert_eq!(rig.engine.active_layer(PAD), Some(0));
    assert_eq!(rig.at(50).tick(), []);
}

#[test]
fn stick_jitter_on_a_sector_edge_does_not_alternate_keys() {
    let mut rig = Rig::new(STICKS);
    // The up/up-right edge is at 67.5°.
    let at = |degrees: f32| {
        let radians = degrees.to_radians();
        [(Axis::LeftX, radians.cos()), (Axis::LeftY, -radians.sin())]
    };
    assert_eq!(rig.axes(&at(69.0)), [KeyDown(KeyCode::W)]);
    for degrees in [66.0, 68.0, 65.0, 69.0] {
        assert_eq!(rig.axes(&at(degrees)), [], "at {degrees}°");
    }
    assert_eq!(rig.axes(&at(60.0)), [KeyDown(KeyCode::D)]);
}

// ---- Combinations and zone hand-over ----

#[test]
fn zone_hand_over_never_combines_the_two_zones() {
    let mut rig = one_button("left_ctrl, c, hold 0.5s, x");
    assert_eq!(rig.at(0).press(Button::South), [KeyDown(KeyCode::LEFT_CTRL), KeyDown(KeyCode::C)]);
    // Ctrl+C comes up before X goes down, so nothing sees Ctrl+X.
    assert_eq!(
        rig.at(500).tick(),
        [KeyUp(KeyCode::C), KeyUp(KeyCode::LEFT_CTRL), KeyDown(KeyCode::X)]
    );
    assert_eq!(rig.at(600).release(Button::South), [KeyUp(KeyCode::X)]);
}

#[test]
fn a_hold_zone_due_in_the_same_step_as_the_release_still_counts() {
    let mut rig = one_button(TAP_OR_HOLD);
    rig.at(0).press(Button::South);
    // The engine did not run at 500 ms, when Y was due; the release arrives at 520 ms.
    assert_eq!(rig.at(520).release(Button::South), [KeyDown(KeyCode::Y), KeyUp(KeyCode::Y)]);
    assert_eq!(rig.engine.next_wakeup(), None);
}

const CHORDS: &str = r#"
    [[layers]]
    name = "Main"
    [layers.buttons.south]
    slots = ["z"]
    [layers.buttons.east]
    slots = ["left_ctrl", "z"]
    [layers.buttons.west]
    slots = ["left_ctrl", "z", "pause 0.1s", "a"]
    [layers.buttons.north]
    slots = ["z", "left_ctrl"]
"#;

#[test]
fn a_combination_retaps_a_key_another_control_holds() {
    let mut rig = Rig::new(CHORDS);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::Z)]);
    // Z is already down, so it is tapped again after Ctrl for the application to see Ctrl+Z.
    assert_eq!(
        rig.press(Button::East),
        [KeyDown(KeyCode::LEFT_CTRL), KeyUp(KeyCode::Z), KeyDown(KeyCode::Z)]
    );
    // South still holds Z.
    assert_eq!(rig.release(Button::East), [KeyUp(KeyCode::LEFT_CTRL)]);
    assert_eq!(rig.release(Button::South), [KeyUp(KeyCode::Z)]);

    // The taps of a sequence do the same.
    rig.at(0).press(Button::South);
    assert_eq!(
        rig.at(0).press(Button::West),
        [KeyDown(KeyCode::LEFT_CTRL), KeyUp(KeyCode::Z), KeyDown(KeyCode::Z)]
    );
    rig.release(Button::West);
    assert_eq!(rig.at(100).tick(), [KeyUp(KeyCode::LEFT_CTRL)]);
    assert_eq!(rig.at(200).tick(), [KeyDown(KeyCode::A)]);
    assert_eq!(rig.at(300).tick(), [KeyUp(KeyCode::A)]);

    // A key before the modifier is not part of a combination, shared or not.
    assert_eq!(rig.press(Button::North), [KeyDown(KeyCode::LEFT_CTRL)]);
    assert_eq!(rig.release(Button::North), [KeyUp(KeyCode::LEFT_CTRL)]);
    assert_eq!(rig.release(Button::South), [KeyUp(KeyCode::Z)]);
}

// ---- Profile changes mid-press ----

const SWAP: &str = r#"
    [[layers]]
    name = "Main"
    [layers.buttons.south]
    slots = ["a", "pause 0.2s", "b"]
    [layers.buttons.east]
    slots = ["left_shift"]
    toggle = true
    rumble = true
    [layers.buttons.west]
    slots = ["x"]
    turbo = 10.0
    [layers.buttons.north]
    layer = { target = "Shift", mode = "while_held" }

    [[layers]]
    name = "Shift"
    [layers.buttons.south]
    slots = ["c"]
"#;

#[test]
fn profile_change_mid_press_releases_everything_once() {
    let mut rig = Rig::new(SWAP);
    let mut keyboard = Keyboard::default();
    let mut check = |out: Vec<OutputAction>| {
        keyboard.apply(&out).unwrap();
        out
    };
    check(rig.at(0).press(Button::West)); // turbo
    check(rig.press(Button::South)); // a running sequence
    check(rig.press(Button::East)); // a toggle, latched on release
    check(rig.release(Button::East));
    check(rig.press(Button::North)); // a while-held layer, still held
    assert_eq!(check(rig.at(50).tick()), [KeyUp(KeyCode::X)]); // turbo's up phase
    assert_eq!(rig.engine.active_layer(PAD), Some(1));
    assert_eq!(rig.engine.take_rumble_changes(), [(PAD, true)]);

    let out = check(rig.at(60).set_profile(SWAP));
    assert_eq!(out, [KeyUp(KeyCode::LEFT_SHIFT), KeyUp(KeyCode::A)]);
    assert!(keyboard.down.is_empty(), "{:?}", keyboard.down);
    assert_eq!(rig.engine.active_layer(PAD), Some(0));
    assert_eq!(rig.engine.take_rumble_changes(), [(PAD, false)]);
    assert_eq!(rig.engine.next_wakeup(), None);

    // The controls let go later: nothing was pressed under the new profile, so nothing happens.
    for button in [Button::West, Button::South, Button::North] {
        assert_eq!(rig.release(button), [], "{button:?}");
    }
    assert_eq!(rig.at(300).tick(), []);
    assert_eq!(rig.at(1000).tick(), []);
    assert_eq!(rig.engine.take_rumble_changes(), []);
}

#[test]
fn a_held_trigger_rumbles_at_once_under_a_new_profile() {
    let mut rig = Rig::new(BASIC);
    rig.axes(&[(Axis::RightTrigger, 1.0)]);
    let out = rig.set_profile(
        "[[layers]]\nname = \"Main\"\n[layers.right_trigger]\nslots = [\"x\"]\nrumble = true\n",
    );
    assert_eq!(out, [KeyDown(KeyCode::X)]);
    assert_eq!(rig.engine.take_rumble_changes(), [(PAD, true)]);
}

// ---- Property tests: whatever the input, outputs stay consistent. ----

// ---- Virtual Xbox controller ----

/// South types space and the left shoulder holds layer "Aim", where east types e and the left
/// stick presses directions. North has an empty assignment. The right trigger clicks and the
/// right stick moves the pointer.
const XBOX: &str = r#"
    virtual_pad = "xbox360"
    [[layers]]
    name = "Main"
    [layers.buttons.south]
    slots = ["space"]
    [layers.buttons.north]
    [layers.buttons.left_shoulder]
    layer = { target = "Aim", mode = "while_held" }
    [layers.right_trigger]
    slots = ["mouse_left"]
    [layers.right_stick]
    mode = "mouse"

    [[layers]]
    name = "Aim"
    [layers.buttons.east]
    slots = ["e"]
    [layers.left_stick]
    mode = "directions"
    [layers.left_stick.directions]
    up = { slots = ["w"] }
"#;

/// An Xbox state with only these buttons pressed.
fn pressed(buttons: u16) -> XboxState {
    XboxState { buttons, ..XboxState::NEUTRAL }
}

fn stick(value: f32) -> i16 {
    zpadder_model::xbox::stick_axis(value)
}

impl Rig {
    fn xbox(&mut self) -> Vec<(PadId, XboxState)> {
        self.engine.take_virtual_pad_changes()
    }
}

#[test]
fn unassigned_buttons_pass_through_by_position() {
    let mut rig = Rig::new(XBOX);
    assert_eq!(rig.press(Button::South), [KeyDown(KeyCode::SPACE)]);
    assert_eq!(rig.xbox(), [], "assigned: the virtual pad sees it released");
    assert_eq!(rig.press(Button::East), []);
    assert_eq!(rig.xbox(), [(PAD, pressed(xb::B))]);
    rig.press(Button::DPadUp);
    rig.press(Button::RightStick);
    assert_eq!(
        rig.xbox(),
        [
            (PAD, pressed(xb::B | xb::DPAD_UP)),
            (PAD, pressed(xb::B | xb::DPAD_UP | xb::RIGHT_THUMB))
        ]
    );
    // Paddles have no Xbox 360 counterpart.
    rig.press(Button::LeftPaddle1);
    assert_eq!(rig.xbox(), []);
    rig.events(&[
        InputEvent::Button { pad: PAD, button: Button::East, pressed: false },
        InputEvent::Button { pad: PAD, button: Button::DPadUp, pressed: false },
        InputEvent::Button { pad: PAD, button: Button::RightStick, pressed: false },
        InputEvent::Button { pad: PAD, button: Button::LeftPaddle1, pressed: false },
    ]);
    assert_eq!(rig.xbox(), [(PAD, XboxState::NEUTRAL)], "one report per step");
    assert_eq!(rig.release(Button::South), [KeyUp(KeyCode::SPACE)]);
    assert_eq!(rig.xbox(), []);
}

#[test]
fn a_passthrough_assignment_does_both() {
    let mut rig = Rig::new(
        r#"
        virtual_pad = "xbox360"
        [[layers]]
        name = "Main"
        [layers.buttons.right_shoulder]
        slots = ["i"]
        passthrough = true
        [layers.right_trigger]
        slots = ["mouse_left"]
        passthrough = true
        [layers.right_stick]
        mode = "mouse"
        passthrough = true
        "#,
    );
    // The key goes down and the Xbox controller's RB is pressed too.
    assert_eq!(rig.press(Button::RightShoulder), [KeyDown(KeyCode::I)]);
    assert_eq!(rig.xbox(), [(PAD, pressed(xb::RIGHT_SHOULDER))]);
    assert_eq!(rig.release(Button::RightShoulder), [KeyUp(KeyCode::I)]);
    assert_eq!(rig.xbox(), [(PAD, XboxState::NEUTRAL)]);
    // The trigger clicks and passes its pull.
    let click = OutputAction::MouseButton { button: MouseButton::Left, pressed: true };
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.6)]), [click]);
    assert_eq!(rig.xbox(), [(PAD, XboxState { right_trigger: 153, ..XboxState::NEUTRAL })]);
    rig.axes(&[(Axis::RightTrigger, 0.0)]);
    rig.xbox();
    // The stick moves the pointer and the Xbox controller's stick.
    rig.axes(&[(Axis::RightX, 1.0)]);
    assert_eq!(rig.xbox(), [(PAD, XboxState { right_x: i16::MAX, ..XboxState::NEUTRAL })]);
}

#[test]
fn an_empty_assignment_counts_as_unassigned() {
    let mut rig = Rig::new(XBOX);
    rig.press(Button::North);
    assert_eq!(rig.xbox(), [(PAD, pressed(xb::Y))]);
}

#[test]
fn the_touchpad_is_back_and_pressing_both_is_back() {
    let mut rig = Rig::new(XBOX);
    rig.press(Button::Touchpad);
    assert_eq!(rig.xbox(), [(PAD, pressed(xb::BACK))]);
    rig.press(Button::Back);
    rig.release(Button::Touchpad);
    assert_eq!(rig.xbox(), [], "Back stays down while either is held");
    rig.release(Button::Back);
    assert_eq!(rig.xbox(), [(PAD, XboxState::NEUTRAL)]);
}

#[test]
fn a_press_keeps_its_destination_across_layer_switches() {
    let mut rig = Rig::new(XBOX);
    // East has nothing assigned in Main, so it goes to the virtual pad...
    rig.press(Button::East);
    assert_eq!(rig.xbox(), [(PAD, pressed(xb::B))]);
    // ...and stays there when Aim, where east types e, comes up. The shoulder holding Aim is
    // assigned, so it is not LB.
    assert_eq!(rig.press(Button::LeftShoulder), []);
    assert_eq!(rig.xbox(), []);
    assert_eq!(rig.release(Button::East), [], "no e to let go of");
    assert_eq!(rig.xbox(), [(PAD, XboxState::NEUTRAL)]);
    // Pressed in Aim, east types e, and keeps doing so after Aim goes.
    assert_eq!(rig.press(Button::East), [KeyDown(KeyCode::E)]);
    rig.release(Button::LeftShoulder);
    assert_eq!(rig.release(Button::East), [KeyUp(KeyCode::E)]);
    assert_eq!(rig.xbox(), []);
}

#[test]
fn sticks_pass_through_in_mode_off_with_y_up() {
    let mut rig = Rig::new(XBOX);
    // Up and to the right: ZPadder's Y is negative upwards, XInput's positive.
    rig.axes(&[(Axis::LeftX, 0.5), (Axis::LeftY, -1.0)]);
    let expected = XboxState { left_x: 16384, left_y: i16::MAX, ..XboxState::NEUTRAL };
    assert_eq!(rig.xbox(), [(PAD, expected)]);
    rig.axes(&[(Axis::LeftX, -1.0), (Axis::LeftY, 1.0)]);
    let expected = XboxState { left_x: i16::MIN, left_y: i16::MIN, ..XboxState::NEUTRAL };
    assert_eq!(rig.xbox(), [(PAD, expected)]);
    // No dead zone: games apply their own.
    rig.axes(&[(Axis::LeftX, 0.01), (Axis::LeftY, 0.0)]);
    assert_eq!(rig.xbox(), [(PAD, XboxState { left_x: 328, ..XboxState::NEUTRAL })]);
    // The right stick moves the pointer, so the virtual pad sees it centred.
    rig.axes(&[(Axis::RightX, 1.0)]);
    assert_eq!(rig.xbox(), []);
}

#[test]
fn stick_values_saturate() {
    let mut rig = Rig::with(unvalidated(XBOX));
    rig.axes(&[(Axis::LeftX, 3.0), (Axis::LeftY, -3.0)]);
    let expected = XboxState { left_x: i16::MAX, left_y: i16::MAX, ..XboxState::NEUTRAL };
    assert_eq!(rig.xbox(), [(PAD, expected)]);
    rig.axes(&[(Axis::LeftX, f32::NAN), (Axis::LeftY, f32::INFINITY)]);
    let expected = XboxState { left_x: 0, left_y: i16::MIN, ..XboxState::NEUTRAL };
    assert_eq!(rig.xbox(), [(PAD, expected)]);
}

#[test]
fn a_stick_pressing_directions_passes_nothing_until_it_centres() {
    let mut rig = Rig::new(XBOX);
    rig.press(Button::LeftShoulder);
    assert_eq!(rig.axes(&[(Axis::LeftY, -1.0)]), [KeyDown(KeyCode::W)]);
    assert_eq!(rig.xbox(), []);
    // Aim goes, but the stick keeps Aim's directions until it centres.
    rig.release(Button::LeftShoulder);
    assert_eq!(rig.xbox(), []);
    assert_eq!(rig.axes(&[(Axis::LeftY, 0.0)]), [KeyUp(KeyCode::W)]);
    rig.axes(&[(Axis::LeftY, -0.5)]);
    assert_eq!(rig.xbox(), [(PAD, XboxState { left_y: 16384, ..XboxState::NEUTRAL })]);
}

#[test]
fn triggers_pass_their_pull_when_unassigned() {
    let mut rig = Rig::new(XBOX);
    assert_eq!(rig.axes(&[(Axis::LeftTrigger, 0.5)]), []);
    assert_eq!(rig.xbox(), [(PAD, XboxState { left_trigger: 128, ..XboxState::NEUTRAL })]);
    // The right trigger clicks, so the virtual pad sees it released.
    let click = OutputAction::MouseButton { button: MouseButton::Left, pressed: true };
    assert_eq!(rig.axes(&[(Axis::RightTrigger, 0.6)]), [click]);
    assert_eq!(rig.axes(&[(Axis::LeftTrigger, 1.0)]), []);
    assert_eq!(rig.xbox(), [(PAD, XboxState { left_trigger: 255, ..XboxState::NEUTRAL })]);
    rig.axes(&[(Axis::LeftTrigger, 0.0)]);
    assert_eq!(rig.xbox(), [(PAD, XboxState::NEUTRAL)]);
}

#[test]
fn a_pulled_trigger_keeps_its_destination_across_layer_switches() {
    let mut rig = Rig::new(
        r#"
        virtual_pad = "xbox360"
        [[layers]]
        name = "Main"
        [layers.buttons.left_shoulder]
        layer = { target = "Fire", mode = "while_held" }
        [[layers]]
        name = "Fire"
        [layers.left_trigger]
        slots = ["f"]
        "#,
    );
    rig.axes(&[(Axis::LeftTrigger, 0.8)]);
    assert_eq!(rig.xbox(), [(PAD, XboxState { left_trigger: 204, ..XboxState::NEUTRAL })]);
    // Pulled while unassigned, it stays with the virtual pad when "Fire" comes up.
    assert_eq!(rig.press(Button::LeftShoulder), []);
    assert_eq!(rig.xbox(), []);
    rig.axes(&[(Axis::LeftTrigger, 0.0)]);
    assert_eq!(rig.xbox(), [(PAD, XboxState::NEUTRAL)]);
    // Pulled in "Fire", it presses f instead.
    assert_eq!(rig.axes(&[(Axis::LeftTrigger, 0.8)]), [KeyDown(KeyCode::F)]);
    assert_eq!(rig.xbox(), []);
}

#[test]
fn release_all_reports_neutral_until_the_pad_moves_again() {
    let mut rig = Rig::new(XBOX);
    rig.press(Button::East);
    rig.axes(&[(Axis::LeftX, 1.0), (Axis::LeftTrigger, 1.0)]);
    rig.xbox();
    let mut out = Vec::new();
    rig.engine.release_all(&mut out);
    assert_eq!(rig.xbox(), [(PAD, XboxState::NEUTRAL)]);
    rig.at(10).tick();
    assert_eq!(rig.xbox(), [], "still at rest");
    rig.axes(&[(Axis::LeftX, 0.9)]);
    let expected = XboxState { left_x: stick(0.9), left_trigger: 255, ..XboxState::NEUTRAL };
    assert_eq!(rig.xbox(), [(PAD, expected)], "east must be pressed again");
}

#[test]
fn profile_changes_report_neutral_first() {
    let mut rig = Rig::new(XBOX);
    rig.press(Button::East);
    rig.axes(&[(Axis::LeftX, 1.0)]);
    rig.xbox();
    // Another version of the profile: everything is let go, then the stick that is still
    // pushed passes through again. East must be pressed again.
    assert_eq!(rig.set_profile(XBOX), []);
    let pushed = XboxState { left_x: i16::MAX, ..XboxState::NEUTRAL };
    assert_eq!(rig.xbox(), [(PAD, XboxState::NEUTRAL), (PAD, pushed)]);
    // A profile without a virtual pad, and no profile, leave it neutral.
    rig.set_profile(BASIC);
    assert_eq!(rig.xbox(), [(PAD, XboxState::NEUTRAL)]);
    let mut out = Vec::new();
    rig.engine.set_profile(None, &mut out);
    rig.axes(&[(Axis::LeftX, -1.0)]);
    rig.press(Button::East);
    assert_eq!(rig.xbox(), []);
}

#[test]
fn profiles_without_a_virtual_pad_report_nothing() {
    let mut rig = Rig::new(BASIC);
    rig.press(Button::Start);
    rig.axes(&[(Axis::LeftX, 1.0), (Axis::LeftTrigger, 1.0)]);
    assert_eq!(rig.xbox(), []);
}

#[test]
fn each_pad_has_its_own_virtual_pad() {
    let mut rig = Rig::new(XBOX);
    let two = PadId(2);
    let button = |pad, button, pressed| InputEvent::Button { pad, button, pressed };
    // Pad 1 holds Aim, where east types e; pad 2 stays on Main, where east is B.
    rig.events(&[button(PAD, Button::LeftShoulder, true)]);
    assert_eq!(rig.events(&[button(PAD, Button::East, true)]), [KeyDown(KeyCode::E)]);
    assert_eq!(rig.events(&[button(two, Button::East, true)]), []);
    assert_eq!(rig.xbox(), [(two, pressed(xb::B))]);
    rig.events(&[InputEvent::Axis { pad: two, axis: Axis::LeftTrigger, value: 1.0 }]);
    let both = XboxState { left_trigger: 255, ..pressed(xb::B) };
    assert_eq!(rig.xbox(), [(two, both)]);
    // A pad that goes away is forgotten without a report.
    rig.events(&[InputEvent::Disconnected { pad: two }]);
    assert_eq!(rig.xbox(), []);
}

const FUZZ: &str = r#"
    virtual_pad = "xbox360"
    [[layers]]
    name = "Base"
    [layers.buttons.south]
    slots = ["a", "left_shift"]
    [layers.buttons.east]
    slots = ["a"]
    turbo = 15.0
    [layers.buttons.west]
    slots = ["b", "mouse_left"]
    toggle = true
    [layers.buttons.north]
    layer = { target = "Second", mode = "while_held" }
    [layers.left_stick]
    mode = "directions"
    [layers.left_stick.directions]
    up = { slots = ["w"] }
    down = { slots = ["s"] }
    left = { slots = ["a"] }
    right = { slots = ["d"] }
    [layers.right_stick]
    mode = "mouse"
    [layers.right_trigger]
    slots = ["a", "distance 50%", "a", "mouse_left", "distance 90%", "e"]
    [layers.buttons.start]
    slots = ["a", "pause 0.05s", "b", "tap_time 0.02s", "c", "d"]
    [layers.buttons.back]
    slots = ["hold 0.1s", "e", "release 0.01s", "a", "pause 0.03s", "b", "release 0.1s"]
    [layers.buttons.left_shoulder]
    slots = ["b", "hold 0.08s", "a", "b", "cycle", "c", "pause 0.02s", "a"]

    [[layers]]
    name = "Second"
    [layers.buttons.south]
    slots = ["c"]
    turbo = 30.0
    toggle = true
    [layers.buttons.east]
    layer = { target = "Base", mode = "two_way" }
    [layers.buttons.west]
    layer = { target = "Second", mode = "while_held" }
    [layers.left_stick]
    mode = "directions"
    [layers.left_stick.directions]
    eight_way = true
    up = { slots = ["a"] }
    up_right = { slots = ["e"] }
    [layers.left_trigger]
    slots = ["d"]
"#;

/// A second profile for the property test, switched to mid-press: other thresholds, layers
/// held by a trigger and a stick direction, combinations and zone hand-overs.
const FUZZ_OTHER: &str = r#"
    [[layers]]
    name = "One"
    trigger_threshold = 0.3
    [layers.buttons.south]
    slots = ["left_ctrl", "a", "hold 0.05s", "b"]
    [layers.buttons.east]
    slots = ["a"]
    [layers.buttons.west]
    slots = ["tap_time 0.01s", "left_ctrl", "a", "pause 0.02s", "b"]
    [layers.buttons.north]
    slots = ["mouse_left"]
    rumble = true
    [layers.left_trigger]
    slots = ["left_shift"]
    layer = { target = "Two", mode = "while_held" }
    [layers.right_trigger]
    slots = ["left_ctrl", "a", "distance 60%", "b"]
    [layers.left_stick]
    mode = "directions"
    dead_zone = 0.05
    [layers.left_stick.directions]
    up = { layer = { target = "Two", mode = "while_held" } }
    down = { slots = ["s", "left_alt"] }
    left = { slots = ["a"], turbo = 20.0 }

    [[layers]]
    name = "Two"
    trigger_threshold = 0.8
    [layers.buttons.east]
    slots = ["left_ctrl", "a"]
    [layers.buttons.south]
    layer = { target = "One" }
    [layers.left_stick]
    mode = "mouse"
    [layers.right_stick]
    mode = "directions"
    [layers.right_stick.directions]
    right = { slots = ["d"] }
"#;

#[derive(Debug, Clone)]
enum Op {
    Button(u32, usize, bool),
    Axis(u32, usize, f32),
    Wait(u64),
    Unplug(u32),
    /// Switches to `FUZZ`, `FUZZ_OTHER` or no profile.
    SetProfile(usize),
    ReleaseAll,
}

fn op() -> impl Strategy<Value = Op> {
    // Two pads share the profile, so they share keys too.
    let pad = 1..=2u32;
    prop_oneof![
        8 => (pad.clone(), 0..7usize, any::<bool>()).prop_map(|(p, b, on)| Op::Button(p, b, on)),
        6 => (pad.clone(), 0..6usize, -1.0f32..=1.0).prop_map(|(p, a, v)| Op::Axis(p, a, v)),
        4 => (0..120u64).prop_map(Op::Wait),
        1 => pad.prop_map(Op::Unplug),
        1 => (0..3usize).prop_map(Op::SetProfile),
        1 => Just(Op::ReleaseAll),
    ]
}

/// Checks the output stream as an OS would see it: no key goes down twice or up while up.
#[derive(Default)]
struct Keyboard {
    down: HashSet<String>,
}

impl Keyboard {
    fn apply(&mut self, actions: &[OutputAction]) -> Result<(), String> {
        for action in actions {
            let (name, pressed) = match action {
                KeyDown(key) => (format!("{key:?}"), true),
                KeyUp(key) => (format!("{key:?}"), false),
                OutputAction::MouseButton { button, pressed } => (format!("{button:?}"), *pressed),
                OutputAction::MouseMove { .. } | OutputAction::Wheel { .. } => continue,
            };
            let changed =
                if pressed { self.down.insert(name.clone()) } else { self.down.remove(&name) };
            if !changed {
                return Err(format!(
                    "{action:?} while {name} was already {}",
                    if pressed { "down" } else { "up" }
                ));
            }
        }
        Ok(())
    }
}

/// Checks rumble changes: each one flips its pad's state.
#[derive(Default)]
struct Rumble {
    on: HashSet<PadId>,
}

impl Rumble {
    fn apply(&mut self, changes: &[(PadId, bool)]) -> Result<(), String> {
        for &(pad, on) in changes {
            let changed = if on { self.on.insert(pad) } else { self.on.remove(&pad) };
            if !changed {
                return Err(format!("rumble {on} on {pad:?} twice"));
            }
        }
        Ok(())
    }
}

/// Checks virtual controller reports: each one is a change.
#[derive(Default)]
struct Xbox {
    reported: HashMap<PadId, XboxState>,
}

impl Xbox {
    fn apply(&mut self, changes: &[(PadId, XboxState)]) -> Result<(), String> {
        for &(pad, state) in changes {
            let before = self.reported.insert(pad, state).unwrap_or_default();
            if before == state {
                return Err(format!("{pad:?} reported {state:?} twice"));
            }
        }
        Ok(())
    }

    fn neutral(&self) -> bool {
        self.reported.values().all(XboxState::is_neutral)
    }
}

proptest! {
    #[test]
    fn outputs_stay_balanced(ops in proptest::collection::vec(op(), 1..300)) {
        const BUTTONS: [Button; 7] = [
            Button::South,
            Button::East,
            Button::West,
            Button::North,
            Button::Start,
            Button::Back,
            Button::LeftShoulder,
        ];
        const AXES: [Axis; 6] =
            [Axis::LeftX, Axis::LeftY, Axis::RightX, Axis::RightY, Axis::LeftTrigger, Axis::RightTrigger];
        let profiles = [Some(profile(FUZZ)), Some(profile(FUZZ_OTHER)), None];
        let mut rig = Rig::new(FUZZ);
        let mut keyboard = Keyboard::default();
        let mut rumble = Rumble::default();
        let mut xbox = Xbox::default();
        let mut now = 0;
        for op in ops {
            let released = matches!(op, Op::ReleaseAll);
            let out = match op {
                Op::Button(pad, b, pressed) => rig.events(&[InputEvent::Button {
                    pad: PadId(pad),
                    button: BUTTONS[b],
                    pressed,
                }]),
                Op::Axis(pad, a, value) => {
                    rig.events(&[InputEvent::Axis { pad: PadId(pad), axis: AXES[a], value }])
                }
                Op::Wait(ms) => {
                    now += ms;
                    rig.at(now).tick()
                }
                Op::Unplug(pad) => {
                    // A pad that is gone stops rumbling without being told, and its virtual
                    // controller goes with it.
                    rumble.on.remove(&PadId(pad));
                    xbox.reported.remove(&PadId(pad));
                    rig.events(&[InputEvent::Disconnected { pad: PadId(pad) }])
                }
                Op::SetProfile(i) => {
                    let mut out = Vec::new();
                    rig.engine.set_profile(profiles[i].clone(), &mut out);
                    out
                }
                Op::ReleaseAll => {
                    let mut out = Vec::new();
                    rig.engine.release_all(&mut out);
                    out
                }
            };
            keyboard.apply(&out).map_err(TestCaseError::fail)?;
            rumble.apply(&rig.engine.take_rumble_changes()).map_err(TestCaseError::fail)?;
            xbox.apply(&rig.engine.take_virtual_pad_changes()).map_err(TestCaseError::fail)?;
            if released {
                prop_assert!(keyboard.down.is_empty(), "down after release_all: {:?}", keyboard.down);
                prop_assert!(rumble.on.is_empty(), "rumbling after release_all: {:?}", rumble.on);
                prop_assert!(xbox.neutral(), "virtual pad busy after release_all: {:?}", xbox.reported);
            }
        }
        let mut out = Vec::new();
        rig.engine.release_all(&mut out);
        keyboard.apply(&out).map_err(TestCaseError::fail)?;
        rumble.apply(&rig.engine.take_rumble_changes()).map_err(TestCaseError::fail)?;
        xbox.apply(&rig.engine.take_virtual_pad_changes()).map_err(TestCaseError::fail)?;
        prop_assert!(xbox.neutral(), "virtual pad busy after release_all: {:?}", xbox.reported);
        prop_assert!(keyboard.down.is_empty(), "still down after release_all: {:?}", keyboard.down);
        prop_assert!(rumble.on.is_empty(), "still rumbling after release_all: {:?}", rumble.on);
        prop_assert_eq!(rig.engine.next_wakeup(), None);
    }
}
