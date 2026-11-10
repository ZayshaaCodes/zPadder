use std::fmt;

/// A keyboard key, as a USB HID Keyboard/Keypad page (0x07) usage ID.
///
/// HID usages are layout-independent and OS-neutral; each output backend translates them (on
/// Windows, to a set-1 scan code plus the extended-key flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyCode(pub u16);

macro_rules! key_codes {
    ($($name:ident = $usage:literal,)*) => {
        impl KeyCode {
            $(pub const $name: Self = Self($usage);)*
        }

        /// Every named key, by its constant's name.
        const NAMED_KEYS: &[(&str, KeyCode)] = &[$((stringify!($name), KeyCode::$name),)*];
    };
}

// Named keys, using the US layout's labels for layout-dependent positions.
key_codes! {
    A = 0x04, B = 0x05, C = 0x06, D = 0x07, E = 0x08, F = 0x09, G = 0x0A, H = 0x0B, I = 0x0C,
    J = 0x0D, K = 0x0E, L = 0x0F, M = 0x10, N = 0x11, O = 0x12, P = 0x13, Q = 0x14, R = 0x15,
    S = 0x16, T = 0x17, U = 0x18, V = 0x19, W = 0x1A, X = 0x1B, Y = 0x1C, Z = 0x1D,
    DIGIT_1 = 0x1E, DIGIT_2 = 0x1F, DIGIT_3 = 0x20, DIGIT_4 = 0x21, DIGIT_5 = 0x22,
    DIGIT_6 = 0x23, DIGIT_7 = 0x24, DIGIT_8 = 0x25, DIGIT_9 = 0x26, DIGIT_0 = 0x27,
    ENTER = 0x28, ESCAPE = 0x29, BACKSPACE = 0x2A, TAB = 0x2B, SPACE = 0x2C,
    MINUS = 0x2D, EQUAL = 0x2E, LEFT_BRACKET = 0x2F, RIGHT_BRACKET = 0x30, BACKSLASH = 0x31,
    NON_US_HASH = 0x32, SEMICOLON = 0x33, QUOTE = 0x34, GRAVE = 0x35, COMMA = 0x36,
    PERIOD = 0x37, SLASH = 0x38, CAPS_LOCK = 0x39,
    F1 = 0x3A, F2 = 0x3B, F3 = 0x3C, F4 = 0x3D, F5 = 0x3E, F6 = 0x3F,
    F7 = 0x40, F8 = 0x41, F9 = 0x42, F10 = 0x43, F11 = 0x44, F12 = 0x45,
    PRINT_SCREEN = 0x46, SCROLL_LOCK = 0x47, PAUSE = 0x48, INSERT = 0x49, HOME = 0x4A,
    PAGE_UP = 0x4B, DELETE = 0x4C, END = 0x4D, PAGE_DOWN = 0x4E,
    RIGHT = 0x4F, LEFT = 0x50, DOWN = 0x51, UP = 0x52,
    NUM_LOCK = 0x53, NUMPAD_DIVIDE = 0x54, NUMPAD_MULTIPLY = 0x55, NUMPAD_SUBTRACT = 0x56,
    NUMPAD_ADD = 0x57, NUMPAD_ENTER = 0x58, NUMPAD_1 = 0x59, NUMPAD_2 = 0x5A, NUMPAD_3 = 0x5B,
    NUMPAD_4 = 0x5C, NUMPAD_5 = 0x5D, NUMPAD_6 = 0x5E, NUMPAD_7 = 0x5F, NUMPAD_8 = 0x60,
    NUMPAD_9 = 0x61, NUMPAD_0 = 0x62, NUMPAD_DECIMAL = 0x63,
    NON_US_BACKSLASH = 0x64, APPLICATION = 0x65, NUMPAD_EQUAL = 0x67,
    F13 = 0x68, F14 = 0x69, F15 = 0x6A, F16 = 0x6B, F17 = 0x6C, F18 = 0x6D,
    F19 = 0x6E, F20 = 0x6F, F21 = 0x70, F22 = 0x71, F23 = 0x72, F24 = 0x73,
    LEFT_CTRL = 0xE0, LEFT_SHIFT = 0xE1, LEFT_ALT = 0xE2, LEFT_GUI = 0xE3,
    RIGHT_CTRL = 0xE4, RIGHT_SHIFT = 0xE5, RIGHT_ALT = 0xE6, RIGHT_GUI = 0xE7,
}

impl KeyCode {
    /// Every named key's name, as written in profile files.
    pub fn names() -> impl Iterator<Item = String> {
        NAMED_KEYS.iter().map(|(name, _)| name.to_ascii_lowercase())
    }

    /// Whether this is Ctrl, Shift, Alt or the Windows key, on either side.
    pub fn is_modifier(self) -> bool {
        (Self::LEFT_CTRL.0..=Self::RIGHT_GUI.0).contains(&self.0)
    }

    /// The key's name in profile files, e.g. `left_shift`, or `None` for an unnamed usage.
    pub fn name(self) -> Option<String> {
        NAMED_KEYS.iter().find(|&&(_, key)| key == self).map(|(name, _)| name.to_ascii_lowercase())
    }

    /// Parses a key name, ignoring case. Single digits are accepted for the number row (`1` is
    /// `digit_1`), and `0x..` gives a raw usage ID.
    pub fn from_name(name: &str) -> Option<Self> {
        if let [digit @ b'0'..=b'9'] = name.as_bytes() {
            return Some(if *digit == b'0' {
                Self::DIGIT_0
            } else {
                Self(Self::DIGIT_1.0 + u16::from(digit - b'1'))
            });
        }
        if let Some(hex) = name.strip_prefix("0x") {
            return u16::from_str_radix(hex, 16).ok().map(Self);
        }
        NAMED_KEYS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|&(_, key)| key)
    }
}

impl fmt::Display for KeyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(&name),
            None => write!(f, "0x{:02x}", self.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

/// One synthesized keyboard or mouse event produced by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputAction {
    KeyDown(KeyCode),
    KeyUp(KeyCode),
    MouseButton {
        button: MouseButton,
        pressed: bool,
    },
    /// Relative pointer movement in whole pixels; the engine carries sub-pixel remainders.
    MouseMove {
        dx: i32,
        dy: i32,
    },
    /// Wheel movement in 1/120ths of a notch (the Windows `WHEEL_DELTA` convention). Positive
    /// `vertical` scrolls up (away from the user); positive `horizontal` scrolls right.
    Wheel {
        vertical: i32,
        horizontal: i32,
    },
}

/// Short readable form for logs, e.g. `left_shift down` or `wheel -120,+0`.
impl fmt::Display for OutputAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = |pressed: bool| if pressed { "down" } else { "up" };
        match self {
            Self::KeyDown(key) => write!(f, "{key} down"),
            Self::KeyUp(key) => write!(f, "{key} up"),
            Self::MouseButton { button, pressed } => {
                let button = format!("{button:?}").to_lowercase();
                write!(f, "mouse_{button} {}", state(*pressed))
            }
            Self::MouseMove { dx, dy } => write!(f, "move {dx:+},{dy:+}"),
            Self::Wheel { vertical, horizontal } => write!(f, "wheel {vertical:+},{horizontal:+}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_round_trip() {
        for &(_, key) in NAMED_KEYS {
            assert_eq!(KeyCode::from_name(&key.name().unwrap()), Some(key));
        }
        assert_eq!(KeyCode::LEFT_SHIFT.name().as_deref(), Some("left_shift"));
    }

    #[test]
    fn key_name_aliases() {
        assert_eq!(KeyCode::from_name("Left_Shift"), Some(KeyCode::LEFT_SHIFT));
        assert_eq!(KeyCode::from_name("1"), Some(KeyCode::DIGIT_1));
        assert_eq!(KeyCode::from_name("0"), Some(KeyCode::DIGIT_0));
        assert_eq!(KeyCode::from_name("0x2c"), Some(KeyCode::SPACE));
        assert_eq!(KeyCode::from_name("nope"), None);
    }

    #[test]
    fn modifiers() {
        assert!(KeyCode::LEFT_CTRL.is_modifier() && KeyCode::RIGHT_GUI.is_modifier());
        assert!(!KeyCode::Z.is_modifier() && !KeyCode::CAPS_LOCK.is_modifier());
    }
}
