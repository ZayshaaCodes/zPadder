//! The virtual Xbox 360 controller a profile can present to games (`virtual_pad = "xbox360"`),
//! in XInput's terms, and which of ZPadder's controls pass through to which of its controls.
//!
//! Only the data lives here; putting a controller on the system is the runtime's job.

use serde::{Deserialize, Serialize};

use crate::Button;

/// A virtual controller a profile presents to games.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VirtualPad {
    /// A wired Xbox 360 controller, which every XInput game understands.
    #[serde(rename = "xbox360")]
    Xbox360,
}

/// XInput's button bits (`XINPUT_GAMEPAD_*`).
pub mod buttons {
    pub const DPAD_UP: u16 = 0x0001;
    pub const DPAD_DOWN: u16 = 0x0002;
    pub const DPAD_LEFT: u16 = 0x0004;
    pub const DPAD_RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LEFT_THUMB: u16 = 0x0040;
    pub const RIGHT_THUMB: u16 = 0x0080;
    pub const LEFT_SHOULDER: u16 = 0x0100;
    pub const RIGHT_SHOULDER: u16 = 0x0200;
    pub const GUIDE: u16 = 0x0400;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;
}

/// Everything an Xbox 360 controller reports, as XInput has it: button bits, triggers from 0
/// to 255 and sticks over the full `i16` range, with Y positive **upwards**.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct XboxState {
    /// [`buttons`] bits.
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
}

impl XboxState {
    /// Nothing pressed, triggers released and sticks centred.
    pub const NEUTRAL: Self = Self {
        buttons: 0,
        left_trigger: 0,
        right_trigger: 0,
        left_x: 0,
        left_y: 0,
        right_x: 0,
        right_y: 0,
    };

    pub fn is_neutral(&self) -> bool {
        *self == Self::NEUTRAL
    }
}

/// The Xbox 360 button a ZPadder button passes through as, by position: `south` is A, `east`
/// is B and so on. The touchpad click is Back (pressing both is the same as pressing Back).
/// Paddles and `misc1` have no Xbox 360 counterpart and pass nothing.
pub fn button_bit(button: Button) -> Option<u16> {
    use buttons::*;
    Some(match button {
        Button::South => A,
        Button::East => B,
        Button::West => X,
        Button::North => Y,
        Button::LeftShoulder => LEFT_SHOULDER,
        Button::RightShoulder => RIGHT_SHOULDER,
        Button::Back | Button::Touchpad => BACK,
        Button::Start => START,
        Button::Guide => GUIDE,
        Button::LeftStick => LEFT_THUMB,
        Button::RightStick => RIGHT_THUMB,
        Button::DPadUp => DPAD_UP,
        Button::DPadDown => DPAD_DOWN,
        Button::DPadLeft => DPAD_LEFT,
        Button::DPadRight => DPAD_RIGHT,
        Button::Misc1
        | Button::LeftPaddle1
        | Button::RightPaddle1
        | Button::LeftPaddle2
        | Button::RightPaddle2 => return None,
    })
}

/// The Xbox 360 name of what [`button_bit`] passes `button` through as, e.g. `A`, `LB`, `Back`.
pub fn button_name(button: Button) -> Option<&'static str> {
    use buttons::*;
    Some(match button_bit(button)? {
        A => "A",
        B => "B",
        X => "X",
        Y => "Y",
        LEFT_SHOULDER => "LB",
        RIGHT_SHOULDER => "RB",
        BACK => "Back",
        START => "Start",
        GUIDE => "Guide",
        LEFT_THUMB => "LS",
        RIGHT_THUMB => "RS",
        DPAD_UP => "Up",
        DPAD_DOWN => "Down",
        DPAD_LEFT => "Left",
        _ => "Right",
    })
}

/// A stick axis (-1 to 1) on XInput's scale: the full `i16` range, saturating. Callers flip Y,
/// since ZPadder's Y is positive downwards and XInput's upwards.
pub fn stick_axis(value: f32) -> i16 {
    // `as` saturates, and turns NaN into 0.
    if value >= 0.0 { (value * 32767.0).round() as i16 } else { (value * 32768.0).round() as i16 }
}

/// A trigger (0 to 1) on XInput's scale, 0 to 255, saturating.
pub fn trigger(value: f32) -> u8 {
    (value * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_buttons_go_by_position() {
        assert_eq!(button_bit(Button::South), Some(buttons::A));
        assert_eq!(button_bit(Button::East), Some(buttons::B));
        assert_eq!(button_bit(Button::West), Some(buttons::X));
        assert_eq!(button_bit(Button::North), Some(buttons::Y));
        assert_eq!(button_bit(Button::Touchpad), button_bit(Button::Back));
        assert_eq!(button_bit(Button::LeftPaddle1), None);
        assert_eq!(button_bit(Button::Misc1), None);
    }

    #[test]
    fn every_passed_button_has_a_name_of_its_own() {
        let mut names: Vec<&str> = Button::ALL
            .into_iter()
            .filter(|&b| b != Button::Touchpad)
            .filter_map(button_name)
            .collect();
        assert_eq!(names.len(), 15);
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 15, "{names:?}");
        assert_eq!(button_name(Button::Touchpad), Some("Back"));
        assert_eq!(button_name(Button::DPadRight), Some("Right"));
    }

    #[test]
    fn axes_use_the_full_range_and_saturate() {
        assert_eq!(stick_axis(0.0), 0);
        assert_eq!(stick_axis(1.0), i16::MAX);
        assert_eq!(stick_axis(-1.0), i16::MIN);
        assert_eq!(stick_axis(5.0), i16::MAX);
        assert_eq!(stick_axis(-5.0), i16::MIN);
        assert_eq!(stick_axis(f32::NAN), 0);
        assert_eq!(stick_axis(0.5), 16384);
        assert_eq!(trigger(0.0), 0);
        assert_eq!(trigger(1.0), 255);
        assert_eq!(trigger(2.0), 255);
        assert_eq!(trigger(-1.0), 0);
        assert_eq!(trigger(f32::NAN), 0);
    }
}
