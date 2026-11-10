//! Xpadder's slot tokens: key names, mouse outputs and timing slots.
//!
//! Xpadder writes keys by name (`Left Shift`, `NumPad Enter`, `Semi-Colon`), not by virtual-key
//! code. Every name maps to the physical key's USB HID usage, so the result does not depend on
//! the keyboard layout. The only earlier table (`xpadder2xml`) went through virtual-key codes and
//! got some keys wrong (`NumPad Enter` became VK_SEPARATOR), so this one is written from scratch.
//!
//! Names marked "observed" occur in the public corpus of Xpadder files. The others are the names
//! Xpadder's on-screen keyboard most likely uses, plus common spellings; Xpadder never documented
//! its names, and an unknown name only costs a warning.

use zpadder_model::profile::{Slot, WheelDirection};
use zpadder_model::{KeyCode, MouseButton};

/// A pointer movement token (`Mouse Move Up`). ZPadder moves the pointer with a stick mode
/// rather than a slot, so these are handled by the stick conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum MouseMove {
    Up,
    Down,
    Left,
    Right,
}

/// What one slot token means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Token {
    Slot(Slot),
    MouseMove(MouseMove),
    /// Xpadder's "NONE" key: an explicitly empty slot.
    Nothing,
}

/// Reads one comma-separated token of a `Slots` value.
pub(crate) fn parse_token(token: &str) -> Option<Token> {
    let name = normalise(token);
    if name == "none" {
        return Some(Token::Nothing);
    }
    if let Some(slot) = timing_slot(token.trim()) {
        return Some(Token::Slot(slot));
    }
    if let Some(slot) = mouse_slot(&name) {
        return Some(slot);
    }
    key_code(&name).map(|key| Token::Slot(Slot::Key(key)))
}

/// Lower case, single spaces, and `num pad` / `keypad` spelt `numpad`.
fn normalise(token: &str) -> String {
    let name = token.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    for prefix in ["num pad", "keypad", "key pad"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return format!("numpad{rest}");
        }
    }
    name
}

/// `PAUSE 0.15s`, `HOLD ZONE 0.30s`, `RELEASE ZONE 0.01s`, `DISTANCE ZONE 50%`,
/// `TAP TIME 0.01s` and `CYCLE STOP` (all observed). The model's parser already reads Xpadder's
/// spelling; this only makes sure a key called `Pause` is not taken for a timing slot.
fn timing_slot(token: &str) -> Option<Slot> {
    let upper = token.to_ascii_uppercase();
    let timing = upper == "CYCLE STOP"
        || ["PAUSE ", "HOLD ZONE ", "RELEASE ZONE ", "DISTANCE ZONE ", "TAP TIME "]
            .iter()
            .any(|prefix| upper.starts_with(prefix));
    timing.then(|| token.parse::<Slot>().ok()).flatten().filter(Slot::is_timing)
}

fn mouse_slot(name: &str) -> Option<Token> {
    let wheel = |direction| Some(Token::Slot(Slot::Wheel(direction)));
    let button = |button| Some(Token::Slot(Slot::MouseButton(button)));
    let movement = |direction| Some(Token::MouseMove(direction));
    match name {
        // Observed.
        "mouse wheel up" => wheel(WheelDirection::Up),
        "mouse wheel down" => wheel(WheelDirection::Down),
        "mouse wheel left" => wheel(WheelDirection::Left),
        "mouse wheel right" => wheel(WheelDirection::Right),
        "mouse move up" => movement(MouseMove::Up),
        "mouse move down" => movement(MouseMove::Down),
        "mouse move left" => movement(MouseMove::Left),
        "mouse move right" => movement(MouseMove::Right),
        // Observed: `Left Mouse Button (1)`, `Right Mouse Button (2)`, `Middle Mouse Button (3)`.
        // Buttons 4 and 5 were never seen; their number in brackets is the likely spelling.
        _ if name.contains("mouse button") => {
            let numbered = |n: &str| name.ends_with(&format!("({n})"));
            if numbered("1") || name.starts_with("left mouse button") {
                button(MouseButton::Left)
            } else if numbered("2") || name.starts_with("right mouse button") {
                button(MouseButton::Right)
            } else if numbered("3") || name.starts_with("middle mouse button") {
                button(MouseButton::Middle)
            } else if numbered("4") || name.ends_with("mouse button 4") {
                button(MouseButton::X1)
            } else if numbered("5") || name.ends_with("mouse button 5") {
                button(MouseButton::X2)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Maps a normalised Xpadder key name to its physical key.
fn key_code(name: &str) -> Option<KeyCode> {
    // Letters and the number row (observed).
    if let [c] = name.as_bytes() {
        return match c {
            b'a'..=b'z' => Some(KeyCode(KeyCode::A.0 + u16::from(c - b'a'))),
            b'0' => Some(KeyCode::DIGIT_0),
            b'1'..=b'9' => Some(KeyCode(KeyCode::DIGIT_1.0 + u16::from(c - b'1'))),
            _ => None,
        };
    }
    // F1–F12 observed; F13–F24 for completeness.
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
        return match n {
            1..=12 => Some(KeyCode(KeyCode::F1.0 + n - 1)),
            13..=24 => Some(KeyCode(KeyCode::F13.0 + n - 13)),
            _ => None,
        };
    }
    // Keypad digits (observed: `NumPad 0`–`NumPad 9`).
    if let Some(rest) = name.strip_prefix("numpad")
        && let [digit @ b'0'..=b'9'] = rest.trim().as_bytes()
    {
        return Some(if *digit == b'0' {
            KeyCode::NUMPAD_0
        } else {
            KeyCode(KeyCode::NUMPAD_1.0 + u16::from(digit - b'1'))
        });
    }
    NAMES.iter().find(|(n, _)| *n == name).map(|&(_, key)| key)
}

/// Every other key name, lower case with single spaces.
const NAMES: &[(&str, KeyCode)] = &[
    // Observed.
    ("escape", KeyCode::ESCAPE),
    ("enter", KeyCode::ENTER),
    ("space", KeyCode::SPACE),
    ("tab", KeyCode::TAB),
    ("backspace", KeyCode::BACKSPACE),
    ("delete", KeyCode::DELETE),
    ("insert", KeyCode::INSERT),
    ("home", KeyCode::HOME),
    ("end", KeyCode::END),
    ("page up", KeyCode::PAGE_UP),
    ("page down", KeyCode::PAGE_DOWN),
    ("arrow up", KeyCode::UP),
    ("arrow down", KeyCode::DOWN),
    ("arrow left", KeyCode::LEFT),
    ("arrow right", KeyCode::RIGHT),
    ("left shift", KeyCode::LEFT_SHIFT),
    ("right shift", KeyCode::RIGHT_SHIFT),
    ("left control", KeyCode::LEFT_CTRL),
    ("right control", KeyCode::RIGHT_CTRL),
    // Xpadder's only Alt key is the left one; Windows' generic VK_MENU sends left Alt.
    ("alt", KeyCode::LEFT_ALT),
    ("caps lock", KeyCode::CAPS_LOCK),
    ("scroll lock", KeyCode::SCROLL_LOCK),
    ("numpad /", KeyCode::NUMPAD_DIVIDE),
    ("numpad +", KeyCode::NUMPAD_ADD),
    // The keypad's own Enter key, not the main Enter and not VK_SEPARATOR.
    ("numpad enter", KeyCode::NUMPAD_ENTER),
    ("back-tick", KeyCode::GRAVE),
    ("minus", KeyCode::MINUS),
    ("equals", KeyCode::EQUAL),
    ("left square bracket", KeyCode::LEFT_BRACKET),
    ("right square bracket", KeyCode::RIGHT_BRACKET),
    ("semi-colon", KeyCode::SEMICOLON),
    ("apostrophe", KeyCode::QUOTE),
    ("comma", KeyCode::COMMA),
    ("dot", KeyCode::PERIOD),
    ("forward-slash", KeyCode::SLASH),
    // Named in xpadder2xml's table, so written by some Xpadder version. The key above Enter is
    // `\` on US keyboards and `#` on UK ones: the same physical key and scan code.
    ("num lock", KeyCode::NUM_LOCK),
    ("numpad *", KeyCode::NUMPAD_MULTIPLY),
    ("numpad -", KeyCode::NUMPAD_SUBTRACT),
    ("numpad dot", KeyCode::NUMPAD_DECIMAL),
    ("back-slash[us]/hash[uk]", KeyCode::BACKSLASH),
    // From the Xpadder forum.
    ("windows logo left", KeyCode::LEFT_GUI),
    ("windows logo right", KeyCode::RIGHT_GUI),
    // Not seen anywhere; likely spellings and common aliases.
    ("windows logo", KeyCode::LEFT_GUI),
    ("left windows", KeyCode::LEFT_GUI),
    ("right windows", KeyCode::RIGHT_GUI),
    ("left alt", KeyCode::LEFT_ALT),
    ("right alt", KeyCode::RIGHT_ALT),
    ("alt gr", KeyCode::RIGHT_ALT),
    ("altgr", KeyCode::RIGHT_ALT),
    ("shift", KeyCode::LEFT_SHIFT),
    ("control", KeyCode::LEFT_CTRL),
    ("ctrl", KeyCode::LEFT_CTRL),
    ("left ctrl", KeyCode::LEFT_CTRL),
    ("right ctrl", KeyCode::RIGHT_CTRL),
    ("esc", KeyCode::ESCAPE),
    ("return", KeyCode::ENTER),
    ("spacebar", KeyCode::SPACE),
    ("back space", KeyCode::BACKSPACE),
    ("del", KeyCode::DELETE),
    ("ins", KeyCode::INSERT),
    ("pageup", KeyCode::PAGE_UP),
    ("pagedown", KeyCode::PAGE_DOWN),
    ("pgup", KeyCode::PAGE_UP),
    ("pgdn", KeyCode::PAGE_DOWN),
    ("up", KeyCode::UP),
    ("down", KeyCode::DOWN),
    ("left", KeyCode::LEFT),
    ("right", KeyCode::RIGHT),
    ("up arrow", KeyCode::UP),
    ("down arrow", KeyCode::DOWN),
    ("left arrow", KeyCode::LEFT),
    ("right arrow", KeyCode::RIGHT),
    ("capslock", KeyCode::CAPS_LOCK),
    ("scrolllock", KeyCode::SCROLL_LOCK),
    ("numlock", KeyCode::NUM_LOCK),
    ("print screen", KeyCode::PRINT_SCREEN),
    ("printscreen", KeyCode::PRINT_SCREEN),
    ("print scrn", KeyCode::PRINT_SCREEN),
    ("prt sc", KeyCode::PRINT_SCREEN),
    ("sys rq", KeyCode::PRINT_SCREEN),
    ("pause", KeyCode::PAUSE),
    ("pause/break", KeyCode::PAUSE),
    ("pause break", KeyCode::PAUSE),
    ("break", KeyCode::PAUSE),
    ("application", KeyCode::APPLICATION),
    ("apps", KeyCode::APPLICATION),
    ("context menu", KeyCode::APPLICATION),
    ("numpad .", KeyCode::NUMPAD_DECIMAL),
    ("numpad decimal", KeyCode::NUMPAD_DECIMAL),
    ("numpad del", KeyCode::NUMPAD_DECIMAL),
    ("numpad delete", KeyCode::NUMPAD_DECIMAL),
    ("numpad multiply", KeyCode::NUMPAD_MULTIPLY),
    ("numpad minus", KeyCode::NUMPAD_SUBTRACT),
    ("numpad subtract", KeyCode::NUMPAD_SUBTRACT),
    ("numpad plus", KeyCode::NUMPAD_ADD),
    ("numpad add", KeyCode::NUMPAD_ADD),
    ("numpad divide", KeyCode::NUMPAD_DIVIDE),
    ("numpad slash", KeyCode::NUMPAD_DIVIDE),
    ("numpad forward-slash", KeyCode::NUMPAD_DIVIDE),
    ("numpad =", KeyCode::NUMPAD_EQUAL),
    ("numpad equals", KeyCode::NUMPAD_EQUAL),
    ("backtick", KeyCode::GRAVE),
    ("back tick", KeyCode::GRAVE),
    ("grave", KeyCode::GRAVE),
    ("equal", KeyCode::EQUAL),
    ("hyphen", KeyCode::MINUS),
    ("semicolon", KeyCode::SEMICOLON),
    ("quote", KeyCode::QUOTE),
    ("period", KeyCode::PERIOD),
    ("full stop", KeyCode::PERIOD),
    ("forward slash", KeyCode::SLASH),
    ("slash", KeyCode::SLASH),
    ("back-slash", KeyCode::BACKSLASH),
    ("back slash", KeyCode::BACKSLASH),
    ("backslash", KeyCode::BACKSLASH),
    ("hash", KeyCode::BACKSLASH),
    // The extra key left of Z on ISO (UK and European) keyboards.
    ("back-slash[uk]", KeyCode::NON_US_BACKSLASH),
    ("left bracket", KeyCode::LEFT_BRACKET),
    ("right bracket", KeyCode::RIGHT_BRACKET),
    ("left square-bracket", KeyCode::LEFT_BRACKET),
    ("right square-bracket", KeyCode::RIGHT_BRACKET),
];

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn key(name: &str) -> Option<KeyCode> {
        match parse_token(name)? {
            Token::Slot(Slot::Key(key)) => Some(key),
            _ => None,
        }
    }

    #[test]
    fn every_observed_key_name_maps() {
        let observed = [
            ("A", KeyCode::A),
            ("Z", KeyCode::Z),
            ("0", KeyCode::DIGIT_0),
            ("1", KeyCode::DIGIT_1),
            ("9", KeyCode::DIGIT_9),
            ("F1", KeyCode::F1),
            ("F12", KeyCode::F12),
            ("Escape", KeyCode::ESCAPE),
            ("Enter", KeyCode::ENTER),
            ("Space", KeyCode::SPACE),
            ("Tab", KeyCode::TAB),
            ("Backspace", KeyCode::BACKSPACE),
            ("Delete", KeyCode::DELETE),
            ("Insert", KeyCode::INSERT),
            ("Home", KeyCode::HOME),
            ("End", KeyCode::END),
            ("Page Up", KeyCode::PAGE_UP),
            ("Page Down", KeyCode::PAGE_DOWN),
            ("Arrow Up", KeyCode::UP),
            ("Arrow Down", KeyCode::DOWN),
            ("Arrow Left", KeyCode::LEFT),
            ("Arrow Right", KeyCode::RIGHT),
            ("Left Shift", KeyCode::LEFT_SHIFT),
            ("Right Shift", KeyCode::RIGHT_SHIFT),
            ("Left Control", KeyCode::LEFT_CTRL),
            ("Right Control", KeyCode::RIGHT_CTRL),
            ("Alt", KeyCode::LEFT_ALT),
            ("Caps Lock", KeyCode::CAPS_LOCK),
            ("Scroll Lock", KeyCode::SCROLL_LOCK),
            ("NumPad 0", KeyCode::NUMPAD_0),
            ("NumPad 1", KeyCode::NUMPAD_1),
            ("NumPad 9", KeyCode::NUMPAD_9),
            ("NumPad /", KeyCode::NUMPAD_DIVIDE),
            ("NumPad +", KeyCode::NUMPAD_ADD),
            ("NumPad Enter", KeyCode::NUMPAD_ENTER),
            ("Back-Tick", KeyCode::GRAVE),
            ("Minus", KeyCode::MINUS),
            ("Equals", KeyCode::EQUAL),
            ("Left Square Bracket", KeyCode::LEFT_BRACKET),
            ("Right Square Bracket", KeyCode::RIGHT_BRACKET),
            ("Semi-Colon", KeyCode::SEMICOLON),
            ("Apostrophe", KeyCode::QUOTE),
            ("Comma", KeyCode::COMMA),
            ("Dot", KeyCode::PERIOD),
            ("Forward-Slash", KeyCode::SLASH),
            ("Windows Logo Left", KeyCode::LEFT_GUI),
        ];
        for (name, expected) in observed {
            assert_eq!(key(name), Some(expected), "{name}");
        }
    }

    #[test]
    fn keys_map_by_physical_key_not_virtual_key() {
        // xpadder2xml sent NumPad Enter as VK_SEPARATOR (108).
        assert_eq!(key("NumPad Enter"), Some(KeyCode::NUMPAD_ENTER));
        assert_ne!(key("NumPad Enter"), key("Enter"));
        assert_eq!(key("NumPad -"), Some(KeyCode::NUMPAD_SUBTRACT));
        assert_eq!(key("NumPad Dot"), Some(KeyCode::NUMPAD_DECIMAL));
        assert_eq!(key("Back-Slash[US]/Hash[UK]"), Some(KeyCode::BACKSLASH));
    }

    #[test]
    fn names_are_tolerant_of_case_and_spacing() {
        assert_eq!(key("  left   SHIFT "), Some(KeyCode::LEFT_SHIFT));
        assert_eq!(key("num pad 5"), Some(KeyCode::NUMPAD_5));
        assert_eq!(key("Keypad Enter"), Some(KeyCode::NUMPAD_ENTER));
        assert_eq!(key("f24"), Some(KeyCode::F24));
        assert_eq!(key("F25"), None);
        assert_eq!(key("NumPad 10"), None);
        assert_eq!(parse_token("Shfit"), None);
        assert_eq!(parse_token(""), None);
    }

    #[test]
    fn mouse_tokens() {
        let button = |b| Some(Token::Slot(Slot::MouseButton(b)));
        assert_eq!(parse_token("Left Mouse Button (1)"), button(MouseButton::Left));
        assert_eq!(parse_token("Right Mouse Button (2)"), button(MouseButton::Right));
        assert_eq!(parse_token("Middle Mouse Button (3)"), button(MouseButton::Middle));
        assert_eq!(parse_token("Mouse Button 4 (4)"), button(MouseButton::X1));
        assert_eq!(parse_token("Mouse Button 5"), button(MouseButton::X2));
        assert_eq!(parse_token("Mouse Button 6 (6)"), None);
        assert_eq!(
            parse_token("Mouse Wheel Down"),
            Some(Token::Slot(Slot::Wheel(WheelDirection::Down)))
        );
        assert_eq!(parse_token("Mouse Move Left"), Some(Token::MouseMove(MouseMove::Left)));
        assert_eq!(parse_token("NONE"), Some(Token::Nothing));
    }

    #[test]
    fn timing_tokens() {
        let ms = Duration::from_millis;
        let slot = |text| match parse_token(text) {
            Some(Token::Slot(slot)) => Some(slot),
            _ => None,
        };
        assert_eq!(slot("PAUSE 0.15s"), Some(Slot::Pause(ms(150))));
        assert_eq!(slot("PAUSE 10m00.00s"), Some(Slot::Pause(ms(600_000))));
        assert_eq!(slot("HOLD ZONE 0.30s"), Some(Slot::HoldZone(ms(300))));
        assert_eq!(slot("RELEASE ZONE 0.01s"), Some(Slot::ReleaseZone(ms(10))));
        assert_eq!(slot("DISTANCE ZONE 50%"), Some(Slot::DistanceZone(50.0)));
        assert_eq!(slot("TAP TIME 0.25s"), Some(Slot::TapTime(ms(250))));
        assert_eq!(slot("CYCLE STOP"), Some(Slot::CycleStop));
        // The Pause key is not a pause slot, and a broken duration is not a key.
        assert_eq!(slot("Pause"), Some(Slot::Key(KeyCode::PAUSE)));
        assert_eq!(slot("PAUSE soon"), None);
        // ZPadder's own spellings are not Xpadder tokens.
        assert_eq!(slot("hold 0.5s"), None);
    }
}
