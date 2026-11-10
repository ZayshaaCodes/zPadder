use serde::{Deserialize, Serialize};

/// Identifies one physical controller for as long as it stays connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PadId(pub u32);

/// Gamepad buttons, using SDL3's positional names (`South` is A on Xbox, Cross on PlayStation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Button {
    South,
    East,
    West,
    North,
    Back,
    Guide,
    Start,
    LeftStick,
    RightStick,
    LeftShoulder,
    RightShoulder,
    #[serde(rename = "dpad_up")]
    DPadUp,
    #[serde(rename = "dpad_down")]
    DPadDown,
    #[serde(rename = "dpad_left")]
    DPadLeft,
    #[serde(rename = "dpad_right")]
    DPadRight,
    Misc1,
    RightPaddle1,
    LeftPaddle1,
    RightPaddle2,
    LeftPaddle2,
    Touchpad,
}

impl Button {
    pub const ALL: [Button; 21] = [
        Button::South,
        Button::East,
        Button::West,
        Button::North,
        Button::Back,
        Button::Guide,
        Button::Start,
        Button::LeftStick,
        Button::RightStick,
        Button::LeftShoulder,
        Button::RightShoulder,
        Button::DPadUp,
        Button::DPadDown,
        Button::DPadLeft,
        Button::DPadRight,
        Button::Misc1,
        Button::RightPaddle1,
        Button::LeftPaddle1,
        Button::RightPaddle2,
        Button::LeftPaddle2,
        Button::Touchpad,
    ];

    /// The button's name in profile files, e.g. `dpad_up`.
    pub fn name(self) -> &'static str {
        match self {
            Button::South => "south",
            Button::East => "east",
            Button::West => "west",
            Button::North => "north",
            Button::Back => "back",
            Button::Guide => "guide",
            Button::Start => "start",
            Button::LeftStick => "left_stick",
            Button::RightStick => "right_stick",
            Button::LeftShoulder => "left_shoulder",
            Button::RightShoulder => "right_shoulder",
            Button::DPadUp => "dpad_up",
            Button::DPadDown => "dpad_down",
            Button::DPadLeft => "dpad_left",
            Button::DPadRight => "dpad_right",
            Button::Misc1 => "misc1",
            Button::RightPaddle1 => "right_paddle1",
            Button::LeftPaddle1 => "left_paddle1",
            Button::RightPaddle2 => "right_paddle2",
            Button::LeftPaddle2 => "left_paddle2",
            Button::Touchpad => "touchpad",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|button| button.name() == name)
    }
}

/// Analog gamepad axes. Sticks are normalised to `-1.0..=1.0` (Y is positive downwards),
/// triggers to `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    LeftTrigger,
    RightTrigger,
}

/// A change reported by an input backend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    Connected { pad: PadId },
    Disconnected { pad: PadId },
    Button { pad: PadId, button: Button, pressed: bool },
    Axis { pad: PadId, axis: Axis, value: f32 },
}
