//! HID keyboard usage → PC/AT set-1 scan code.
//!
//! Set-1 scan codes are what Windows `SendInput` takes with `KEYEVENTF_SCANCODE`, and Linux evdev
//! key codes are derived from them, so this table is OS-neutral data.

use zpadder_model::KeyCode;

/// Set-1 make code for `key`. Codes of the form `0xE0xx` need the E0 prefix, which Windows
/// expresses as `KEYEVENTF_EXTENDEDKEY`. `None` for keys without a plain scan code (Pause sends
/// E1 1D 45 and must go through its virtual-key code) and for unmapped usages.
pub fn set1(key: KeyCode) -> Option<u16> {
    Some(match key {
        KeyCode::A => 0x1E,
        KeyCode::B => 0x30,
        KeyCode::C => 0x2E,
        KeyCode::D => 0x20,
        KeyCode::E => 0x12,
        KeyCode::F => 0x21,
        KeyCode::G => 0x22,
        KeyCode::H => 0x23,
        KeyCode::I => 0x17,
        KeyCode::J => 0x24,
        KeyCode::K => 0x25,
        KeyCode::L => 0x26,
        KeyCode::M => 0x32,
        KeyCode::N => 0x31,
        KeyCode::O => 0x18,
        KeyCode::P => 0x19,
        KeyCode::Q => 0x10,
        KeyCode::R => 0x13,
        KeyCode::S => 0x1F,
        KeyCode::T => 0x14,
        KeyCode::U => 0x16,
        KeyCode::V => 0x2F,
        KeyCode::W => 0x11,
        KeyCode::X => 0x2D,
        KeyCode::Y => 0x15,
        KeyCode::Z => 0x2C,
        KeyCode::DIGIT_1 => 0x02,
        KeyCode::DIGIT_2 => 0x03,
        KeyCode::DIGIT_3 => 0x04,
        KeyCode::DIGIT_4 => 0x05,
        KeyCode::DIGIT_5 => 0x06,
        KeyCode::DIGIT_6 => 0x07,
        KeyCode::DIGIT_7 => 0x08,
        KeyCode::DIGIT_8 => 0x09,
        KeyCode::DIGIT_9 => 0x0A,
        KeyCode::DIGIT_0 => 0x0B,
        KeyCode::ENTER => 0x1C,
        KeyCode::ESCAPE => 0x01,
        KeyCode::BACKSPACE => 0x0E,
        KeyCode::TAB => 0x0F,
        KeyCode::SPACE => 0x39,
        KeyCode::MINUS => 0x0C,
        KeyCode::EQUAL => 0x0D,
        KeyCode::LEFT_BRACKET => 0x1A,
        KeyCode::RIGHT_BRACKET => 0x1B,
        KeyCode::BACKSLASH | KeyCode::NON_US_HASH => 0x2B,
        KeyCode::SEMICOLON => 0x27,
        KeyCode::QUOTE => 0x28,
        KeyCode::GRAVE => 0x29,
        KeyCode::COMMA => 0x33,
        KeyCode::PERIOD => 0x34,
        KeyCode::SLASH => 0x35,
        KeyCode::CAPS_LOCK => 0x3A,
        KeyCode::F1 => 0x3B,
        KeyCode::F2 => 0x3C,
        KeyCode::F3 => 0x3D,
        KeyCode::F4 => 0x3E,
        KeyCode::F5 => 0x3F,
        KeyCode::F6 => 0x40,
        KeyCode::F7 => 0x41,
        KeyCode::F8 => 0x42,
        KeyCode::F9 => 0x43,
        KeyCode::F10 => 0x44,
        KeyCode::F11 => 0x57,
        KeyCode::F12 => 0x58,
        KeyCode::PRINT_SCREEN => 0xE037,
        KeyCode::SCROLL_LOCK => 0x46,
        KeyCode::INSERT => 0xE052,
        KeyCode::HOME => 0xE047,
        KeyCode::PAGE_UP => 0xE049,
        KeyCode::DELETE => 0xE053,
        KeyCode::END => 0xE04F,
        KeyCode::PAGE_DOWN => 0xE051,
        KeyCode::RIGHT => 0xE04D,
        KeyCode::LEFT => 0xE04B,
        KeyCode::DOWN => 0xE050,
        KeyCode::UP => 0xE048,
        // Windows reports Num Lock with the extended flag; plain 0x45 is Pause's last byte.
        KeyCode::NUM_LOCK => 0xE045,
        KeyCode::NUMPAD_DIVIDE => 0xE035,
        KeyCode::NUMPAD_MULTIPLY => 0x37,
        KeyCode::NUMPAD_SUBTRACT => 0x4A,
        KeyCode::NUMPAD_ADD => 0x4E,
        KeyCode::NUMPAD_ENTER => 0xE01C,
        KeyCode::NUMPAD_1 => 0x4F,
        KeyCode::NUMPAD_2 => 0x50,
        KeyCode::NUMPAD_3 => 0x51,
        KeyCode::NUMPAD_4 => 0x4B,
        KeyCode::NUMPAD_5 => 0x4C,
        KeyCode::NUMPAD_6 => 0x4D,
        KeyCode::NUMPAD_7 => 0x47,
        KeyCode::NUMPAD_8 => 0x48,
        KeyCode::NUMPAD_9 => 0x49,
        KeyCode::NUMPAD_0 => 0x52,
        KeyCode::NUMPAD_DECIMAL => 0x53,
        KeyCode::NON_US_BACKSLASH => 0x56,
        KeyCode::APPLICATION => 0xE05D,
        KeyCode::NUMPAD_EQUAL => 0x59,
        KeyCode::F13 => 0x64,
        KeyCode::F14 => 0x65,
        KeyCode::F15 => 0x66,
        KeyCode::F16 => 0x67,
        KeyCode::F17 => 0x68,
        KeyCode::F18 => 0x69,
        KeyCode::F19 => 0x6A,
        KeyCode::F20 => 0x6B,
        KeyCode::F21 => 0x6C,
        KeyCode::F22 => 0x6D,
        KeyCode::F23 => 0x6E,
        KeyCode::F24 => 0x76,
        KeyCode::LEFT_CTRL => 0x1D,
        KeyCode::LEFT_SHIFT => 0x2A,
        KeyCode::LEFT_ALT => 0x38,
        KeyCode::LEFT_GUI => 0xE05B,
        KeyCode::RIGHT_CTRL => 0xE01D,
        KeyCode::RIGHT_SHIFT => 0x36,
        KeyCode::RIGHT_ALT => 0xE038,
        KeyCode::RIGHT_GUI => 0xE05C,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn scan_codes_are_unique_except_documented_aliases() {
        let mut seen: HashMap<u16, KeyCode> = HashMap::new();
        for usage in 0..=0xFF {
            let key = KeyCode(usage);
            let Some(code) = set1(key) else { continue };
            if key == KeyCode::NON_US_HASH {
                continue; // Same physical position as Backslash on ANSI boards.
            }
            if let Some(other) = seen.insert(code, key) {
                panic!("{key:?} and {other:?} both map to {code:#06x}");
            }
        }
    }

    #[test]
    fn extended_keys_use_the_e0_prefix() {
        for key in [KeyCode::NUMPAD_ENTER, KeyCode::RIGHT_ALT, KeyCode::UP, KeyCode::DELETE] {
            assert_eq!(set1(key).unwrap() & 0xFF00, 0xE000, "{key:?}");
        }
        assert_eq!(set1(KeyCode::ENTER), Some(0x1C));
        assert_eq!(set1(KeyCode::PAUSE), None);
    }
}
