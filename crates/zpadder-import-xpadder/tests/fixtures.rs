//! Imports the synthetic fixtures in `tests/fixtures`. They are written for these tests (the
//! public Xpadder files mostly have no licence) and keep Xpadder's bytes exactly: CRLF line
//! endings and Windows-1252 text.

use std::time::Duration;

use zpadder_import_xpadder::{
    ButtonMap, FileKind, ImportError, detect, import_controller, import_profile,
};
use zpadder_model::profile::{LayerSelect, SelectMode, Slot, StickMode, WheelDirection};
use zpadder_model::{Button, Control, KeyCode, MouseButton};

const XBOX: &[u8] = include_bytes!("fixtures/xbox360.xpaddercontroller");
const DS4: &[u8] = include_bytes!("fixtures/dualshock4-legacy.xpaddercontroller");
const SHOOTER: &[u8] = include_bytes!("fixtures/shooter.xpadderprofile");
const LEGACY: &[u8] = include_bytes!("fixtures/legacy-5.6.xpadderprofile");
const GLUED: &[u8] = include_bytes!("fixtures/glued-header.xpadderprofile");

fn key(key: KeyCode) -> Slot {
    Slot::Key(key)
}

#[test]
fn files_are_recognised_by_content() {
    assert_eq!(detect(XBOX), Some(FileKind::Controller));
    assert_eq!(detect(DS4), Some(FileKind::Controller));
    for profile in [SHOOTER, LEGACY, GLUED] {
        assert_eq!(detect(profile), Some(FileKind::Profile));
    }
    let wrong = import_profile(XBOX, &ButtonMap::xbox()).unwrap_err();
    let expected =
        ImportError::WrongKind { expected: FileKind::Profile, found: FileKind::Controller };
    assert_eq!(wrong, expected);
    assert!(import_controller(SHOOTER, "x").is_err());
}

#[test]
fn xbox_controller() {
    let imported = import_controller(XBOX, "Xbox 360").unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.buttons, ButtonMap::xbox());

    let layout = &imported.layout;
    assert_eq!(layout.name, "Xbox 360");
    assert_eq!(layout.image.as_deref(), Some("Xbox 360.png"));
    assert_eq!(layout.size, [512.0, 256.0]);
    // Ten buttons (the stick clicks placed at their sticks), four DPad directions, two
    // triggers.
    assert_eq!(layout.positions.len(), 16);
    let at = |control| layout.positions[&control];
    assert_eq!(at(Control::Button(Button::South)), [391.0, 151.0]);
    assert_eq!(at(Control::Button(Button::DPadLeft)), [156.0, 204.0]);
    assert_eq!(at(Control::Button(Button::LeftStick)), [123.0, 117.0]);
    assert_eq!(at(Control::Button(Button::RightStick)), [326.0, 195.0]);
    assert_eq!(at(Control::RightTrigger), [493.0, 19.0]);

    let png = imported.png.unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
    let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut rgba).unwrap();
    let pixel = |x: usize, y: usize| &rgba[(y * 512 + x) * 4..][..4];
    assert_eq!(pixel(0, 0), [0, 0, 0, 0]); // a missing row
    assert_eq!(pixel(255, 99), [0, 0, 0, 0]); // magenta
    assert_eq!(pixel(256, 99), [0x80, 0x80, 0x80, 0xFF]);
    assert_eq!(pixel(257, 109), [0x40, 0x40, 0x40, 0xFF]);
    assert_eq!(pixel(511, 110), [0, 0, 0, 0xFF]);
}

#[test]
fn legacy_dualshock4_controller() {
    let imported = import_controller(DS4, "DS4").unwrap();
    assert_eq!(imported.buttons, ButtonMap::dualshock4());
    assert_eq!(imported.png, None);
    assert_eq!(imported.layout.image, None);
    // Windows-1252 names come through intact; French face-button names are not recognised, so
    // those buttons follow the DualShock 4 order that the other names point to.
    assert_eq!(
        imported.warnings,
        ["unrecognised button names: 1 (`Carré`), 2 (`Croix`), 3 (`Cercle`), 14 (`Pavé \
          tactile`); buttons 1, 2, 3, 14 follow the DualShock 4 button order"]
    );
    let positions = &imported.layout.positions;
    assert_eq!(positions[&Control::Button(Button::West)], [355.0, 119.0]);
    assert_eq!(positions[&Control::LeftTrigger], [75.0, 19.0]);
    assert_eq!(positions[&Control::Button(Button::Touchpad)], [256.0, 75.0]);
    assert_eq!(positions.len(), 6);
}

#[test]
fn shooter_profile() {
    let imported = import_profile(SHOOTER, &ButtonMap::xbox()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let profile = imported.profile;
    assert_eq!(profile.validate(), Ok(()));
    assert_eq!(profile.name, "Synthetic Shooter");
    let names: Vec<&str> = profile.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["On foot", "Menus"]);

    let foot = &profile.layers[0];
    let button = |b| &foot.buttons[&b];
    assert_eq!(button(Button::South).name, "Jump");
    assert_eq!(button(Button::South).slots, [key(KeyCode::SPACE)]);
    assert_eq!(button(Button::East).slots.len(), 5);
    assert_eq!(button(Button::North).slots.iter().filter(|s| **s == Slot::CycleStop).count(), 2);
    assert_eq!(button(Button::LeftShoulder).turbo, Some(5.0));
    assert!(button(Button::RightShoulder).toggle);
    assert_eq!(
        button(Button::RightStick).slots,
        [key(KeyCode::V), Slot::Pause(Duration::from_millis(150)), key(KeyCode::SEMICOLON)]
    );
    assert_eq!(button(Button::DPadUp).slots, [key(KeyCode::LEFT_ALT), key(KeyCode::F4)]);
    assert_eq!(button(Button::DPadDown).slots, [Slot::Wheel(WheelDirection::Down)]);
    assert_eq!(button(Button::DPadDown).turbo, Some(10.0));
    assert_eq!(button(Button::DPadLeft).slots, [key(KeyCode::NUMPAD_ENTER)]);
    assert_eq!(button(Button::DPadRight).slots, [key(KeyCode::GRAVE)]);
    assert_eq!(foot.buttons.len(), 14);

    assert_eq!(foot.left_stick.mode, StickMode::Directions);
    assert!(!foot.left_stick.directions.eight_way);
    assert_eq!(foot.left_stick.dead_zone, 64.0 / 255.0);
    assert_eq!(foot.left_stick.directions.up.as_ref().unwrap().slots, [key(KeyCode::W)]);
    assert_eq!(foot.right_stick.mode, StickMode::Mouse);
    assert_eq!(foot.right_stick.mouse.speed, 1500.0);
    assert!(!foot.right_stick.mouse.invert_y);
    assert_eq!(foot.right_stick.dead_zone, 39.0 / 255.0);

    let aim = foot.left_trigger.as_ref().unwrap();
    assert_eq!(aim.name, "Aim / menus");
    assert_eq!(aim.slots, [Slot::MouseButton(MouseButton::Right)]);
    assert_eq!(
        aim.layer,
        Some(LayerSelect { target: "Menus".into(), mode: SelectMode::WhileHeld })
    );
    let fire = foot.right_trigger.as_ref().unwrap();
    assert!(fire.rumble);
    assert_eq!(fire.slots[1], Slot::DistanceZone(90.0));
    assert_eq!(foot.trigger_threshold, 28.0 / 255.0);

    let menus = &profile.layers[1];
    assert_eq!(menus.buttons[&Button::South].slots, [key(KeyCode::ENTER)]);
    let start = menus.buttons[&Button::Start].layer.as_ref().unwrap();
    assert_eq!((start.target.as_str(), start.mode), ("On foot", SelectMode::TwoWay));
    // The mirrored return selector on the left trigger is folded into the while-held selector.
    assert!(menus.left_trigger.is_none());
    let directions = &menus.left_stick.directions;
    assert!(directions.eight_way);
    assert_eq!(directions.up_right.as_ref().unwrap().slots, [key(KeyCode::PAGE_UP)]);
}

#[test]
fn legacy_profile_without_sections() {
    let imported = import_profile(LEGACY, &ButtonMap::xbox()).unwrap();
    let profile = &imported.profile;
    assert_eq!(profile.validate(), Ok(()));
    assert_eq!(profile.name, "Old Shooter");
    let layer = &profile.layers[0];
    assert_eq!(layer.name, "Set 1");
    assert_eq!(layer.buttons[&Button::South].name, "Jump/Stand");
    assert_eq!(layer.buttons[&Button::East].slots.len(), 4);
    assert!(layer.buttons[&Button::LeftShoulder].rumble);
    assert_eq!(layer.buttons[&Button::West].slots, [key(KeyCode::F)]);
    assert_eq!(layer.left_stick.mode, StickMode::Directions);
    assert!(layer.right_trigger.is_some());
    assert_eq!(imported.warnings.len(), 2, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("unknown slot `Turbo Boost`"));
    assert!(imported.warnings[1].contains("DPad diagonal"));
}

#[test]
fn glued_header_profile() {
    let imported = import_profile(GLUED, &ButtonMap::xbox()).unwrap();
    let layers = &imported.profile.layers;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name, "Glued");
    assert_eq!(layers[0].buttons[&Button::South].slots, [key(KeyCode::Q)]);
    // The second file's header lines repeat the first's.
    assert_eq!(imported.warnings.len(), 2, "{:?}", imported.warnings);
}

/// Truncated and corrupted fixtures must never panic, and profiles must stay valid.
#[test]
fn damaged_files_never_panic() {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for fixture in [XBOX, DS4, SHOOTER, LEGACY, GLUED] {
        let mut inputs: Vec<Vec<u8>> =
            (0..fixture.len()).step_by(31).map(|n| fixture[..n].to_vec()).collect();
        for _ in 0..100 {
            let mut damaged = fixture.to_vec();
            for _ in 0..1 + random() % 8 {
                let at = (random() % damaged.len() as u64) as usize;
                damaged[at] = match random() % 4 {
                    0 => b',',
                    1 => b'=',
                    2 => b'\n',
                    _ => random() as u8,
                };
            }
            inputs.push(damaged);
        }
        for input in inputs {
            if let Ok(imported) = import_profile(&input, &ButtonMap::dualshock4()) {
                assert_eq!(imported.profile.validate(), Ok(()));
            }
            let _ = import_controller(&input, "damaged");
        }
    }
}
