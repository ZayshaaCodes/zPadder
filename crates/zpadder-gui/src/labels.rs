//! Short labels for the keycap chips drawn on the controller: "Space", "Shift+X", "LMB",
//! "→ Aim". Full detail lives in the inspector and the chip's tooltip.

use zpadder_model::profile::{Assignment, SelectMode, Slot, WheelDirection};
use zpadder_model::{KeyCode, MouseButton};

use crate::inspector;

/// How a chip is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    /// Keyboard keys.
    Key,
    /// Mouse buttons, wheel, pointer and scroll sticks.
    Mouse,
    /// Switches layers.
    Layer,
    /// Uses timing slots: sequences, hold/release zones, cycles.
    Macro,
    /// Nothing assigned: passes through to the profile's virtual Xbox controller.
    Xbox,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Chip {
    pub(crate) text: String,
    pub(crate) tone: Tone,
    /// Small markers drawn after the text: turbo, toggle, rumble.
    pub(crate) badges: Vec<&'static str>,
    /// Shown on hover.
    pub(crate) tooltip: String,
}

impl Chip {
    pub(crate) fn plain(text: &str, tone: Tone, tooltip: &str) -> Self {
        Self { text: text.into(), tone, badges: Vec::new(), tooltip: tooltip.into() }
    }
}

/// The chip for an assignment, or `None` if it does nothing.
pub(crate) fn chip(assignment: &Assignment, show_names: bool) -> Option<Chip> {
    if *assignment == Assignment::default() {
        return None;
    }
    let timing = assignment.slots.iter().any(Slot::is_timing);
    let outputs = first_outputs(&assignment.slots);
    let layer = assignment.layer.as_ref().map(|select| {
        let arrow = match select.mode {
            SelectMode::WhileHeld => "⇧",
            SelectMode::OneWay => "→",
            SelectMode::TwoWay => "⇄",
        };
        format!("{arrow} {}", select.target)
    });

    let mut text = if show_names && !assignment.name.trim().is_empty() {
        assignment.name.trim().to_string()
    } else if !outputs.is_empty() {
        outputs.join("+")
    } else {
        layer.clone().unwrap_or_else(|| "…".into())
    };
    if timing && !outputs.is_empty() {
        text.push_str(" …");
    }
    let tone = if timing {
        Tone::Macro
    } else if outputs.is_empty() && layer.is_some() {
        Tone::Layer
    } else if assignment
        .slots
        .iter()
        .all(|slot| matches!(slot, Slot::MouseButton(_) | Slot::Wheel(_)))
    {
        Tone::Mouse
    } else {
        Tone::Key
    };

    let mut badges = Vec::new();
    if assignment.turbo.is_some() {
        badges.push("T");
    }
    if assignment.toggle {
        badges.push("⇆");
    }
    if assignment.rumble {
        badges.push("~");
    }
    if layer.is_some() && !outputs.is_empty() {
        badges.push("L");
    }
    if assignment.passthrough {
        badges.push("+");
    }
    let mut tooltip = inspector::describe(assignment).join("\n");
    if !assignment.name.trim().is_empty() {
        tooltip = format!("{}\n{tooltip}", assignment.name.trim());
    }
    Some(Chip { text, tone, badges, tooltip })
}

/// The outputs that act first: up to the first timing slot.
fn first_outputs(slots: &[Slot]) -> Vec<String> {
    slots.iter().take_while(|slot| !slot.is_timing()).map(short_slot).collect()
}

/// A slot's keycap legend ("Shift", "LMB", "Wheel ↓").
pub(crate) fn short_slot(slot: &Slot) -> String {
    match *slot {
        Slot::Key(key) => short_key(key),
        Slot::MouseButton(MouseButton::Left) => "LMB".into(),
        Slot::MouseButton(MouseButton::Right) => "RMB".into(),
        Slot::MouseButton(MouseButton::Middle) => "MMB".into(),
        Slot::MouseButton(MouseButton::X1) => "M4".into(),
        Slot::MouseButton(MouseButton::X2) => "M5".into(),
        Slot::Wheel(WheelDirection::Up) => "Wheel ↑".into(),
        Slot::Wheel(WheelDirection::Down) => "Wheel ↓".into(),
        Slot::Wheel(WheelDirection::Left) => "Wheel ←".into(),
        Slot::Wheel(WheelDirection::Right) => "Wheel →".into(),
        _ => String::new(),
    }
}

/// A key's keycap legend.
pub(crate) fn short_key(key: KeyCode) -> String {
    let fixed = match key {
        KeyCode::SPACE => "Space",
        KeyCode::ENTER => "Enter",
        KeyCode::ESCAPE => "Esc",
        KeyCode::BACKSPACE => "Bksp",
        KeyCode::TAB => "Tab",
        KeyCode::CAPS_LOCK => "Caps",
        KeyCode::LEFT_SHIFT => "Shift",
        KeyCode::RIGHT_SHIFT => "RShift",
        KeyCode::LEFT_CTRL => "Ctrl",
        KeyCode::RIGHT_CTRL => "RCtrl",
        KeyCode::LEFT_ALT => "Alt",
        KeyCode::RIGHT_ALT => "AltGr",
        KeyCode::LEFT_GUI | KeyCode::RIGHT_GUI => "Win",
        KeyCode::APPLICATION => "Menu",
        KeyCode::UP => "↑",
        KeyCode::DOWN => "↓",
        KeyCode::LEFT => "←",
        KeyCode::RIGHT => "→",
        KeyCode::PAGE_UP => "PgUp",
        KeyCode::PAGE_DOWN => "PgDn",
        KeyCode::HOME => "Home",
        KeyCode::END => "End",
        KeyCode::INSERT => "Ins",
        KeyCode::DELETE => "Del",
        KeyCode::PRINT_SCREEN => "PrtSc",
        KeyCode::SCROLL_LOCK => "ScrLk",
        KeyCode::PAUSE => "Pause",
        KeyCode::NUM_LOCK => "NumLk",
        KeyCode::NUMPAD_ENTER => "Num Ent",
        KeyCode::NUMPAD_ADD => "Num +",
        KeyCode::NUMPAD_SUBTRACT => "Num -",
        KeyCode::NUMPAD_MULTIPLY => "Num *",
        KeyCode::NUMPAD_DIVIDE => "Num /",
        KeyCode::NUMPAD_DECIMAL => "Num .",
        KeyCode::MINUS => "-",
        KeyCode::EQUAL => "=",
        KeyCode::LEFT_BRACKET => "[",
        KeyCode::RIGHT_BRACKET => "]",
        KeyCode::BACKSLASH | KeyCode::NON_US_HASH => "\\",
        KeyCode::SEMICOLON => ";",
        KeyCode::QUOTE => "'",
        KeyCode::GRAVE => "`",
        KeyCode::COMMA => ",",
        KeyCode::PERIOD => ".",
        KeyCode::SLASH => "/",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.into();
    }
    let name = key.to_string();
    if let Some(digit) = name.strip_prefix("digit_") {
        return digit.into();
    }
    if let Some(digit) = name.strip_prefix("numpad_") {
        return format!("Num {digit}");
    }
    // Letters and F-keys: "a" -> "A", "f10" -> "F10".
    name.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment(slots: &str) -> Assignment {
        Assignment {
            slots: slots.split(',').map(|s| s.parse().unwrap()).collect(),
            ..Assignment::default()
        }
    }

    #[test]
    fn chips_read_like_keycaps() {
        assert_eq!(chip(&assignment("left_shift, x"), false).unwrap().text, "Shift+X");
        assert_eq!(chip(&assignment("numpad_5"), false).unwrap().text, "Num 5");
        assert_eq!(chip(&assignment("digit_1"), false).unwrap().text, "1");
        let mouse = chip(&assignment("mouse_left"), false).unwrap();
        assert_eq!((mouse.text.as_str(), mouse.tone), ("LMB", Tone::Mouse));
        let combo = chip(&assignment("a, pause 0.1s, b"), false).unwrap();
        assert_eq!((combo.text.as_str(), combo.tone), ("A …", Tone::Macro));
    }

    #[test]
    fn names_layers_and_badges() {
        let mut jump = assignment("space");
        jump.name = "Jump".into();
        jump.turbo = Some(10.0);
        assert_eq!(chip(&jump, true).unwrap().text, "Jump");
        assert_eq!(chip(&jump, false).unwrap().badges, ["T"]);
        let shift = Assignment {
            layer: Some(zpadder_model::profile::LayerSelect {
                target: "Aim".into(),
                mode: SelectMode::WhileHeld,
            }),
            ..Assignment::default()
        };
        let chip = chip(&shift, false).unwrap();
        assert_eq!((chip.text.as_str(), chip.tone), ("⇧ Aim", Tone::Layer));
        assert_eq!(super::chip(&Assignment::default(), false), None);
    }
}
