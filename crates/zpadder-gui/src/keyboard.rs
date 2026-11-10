//! The on-screen keyboard and mouse used to pick outputs, as in Xpadder's assignment window,
//! and the mapping from keys pressed on the real keyboard.

use eframe::egui::{self, Key, Modifiers, Ui, Vec2};
use zpadder_model::profile::{Slot, WheelDirection};
use zpadder_model::{KeyCode, MouseButton};

/// One key cap: label, key, width in key units.
type Cap = (&'static str, KeyCode, f32);

const ROWS: [&[Cap]; 6] = [
    &[
        ("Esc", KeyCode::ESCAPE, 1.0),
        ("F1", KeyCode::F1, 1.0),
        ("F2", KeyCode::F2, 1.0),
        ("F3", KeyCode::F3, 1.0),
        ("F4", KeyCode::F4, 1.0),
        ("F5", KeyCode::F5, 1.0),
        ("F6", KeyCode::F6, 1.0),
        ("F7", KeyCode::F7, 1.0),
        ("F8", KeyCode::F8, 1.0),
        ("F9", KeyCode::F9, 1.0),
        ("F10", KeyCode::F10, 1.0),
        ("F11", KeyCode::F11, 1.0),
        ("F12", KeyCode::F12, 1.0),
        ("PrtSc", KeyCode::PRINT_SCREEN, 1.0),
        ("ScrLk", KeyCode::SCROLL_LOCK, 1.0),
        ("Pause", KeyCode::PAUSE, 1.0),
    ],
    &[
        ("`", KeyCode::GRAVE, 1.0),
        ("1", KeyCode::DIGIT_1, 1.0),
        ("2", KeyCode::DIGIT_2, 1.0),
        ("3", KeyCode::DIGIT_3, 1.0),
        ("4", KeyCode::DIGIT_4, 1.0),
        ("5", KeyCode::DIGIT_5, 1.0),
        ("6", KeyCode::DIGIT_6, 1.0),
        ("7", KeyCode::DIGIT_7, 1.0),
        ("8", KeyCode::DIGIT_8, 1.0),
        ("9", KeyCode::DIGIT_9, 1.0),
        ("0", KeyCode::DIGIT_0, 1.0),
        ("-", KeyCode::MINUS, 1.0),
        ("=", KeyCode::EQUAL, 1.0),
        ("Bksp", KeyCode::BACKSPACE, 2.0),
        ("Ins", KeyCode::INSERT, 1.0),
        ("Home", KeyCode::HOME, 1.0),
        ("PgUp", KeyCode::PAGE_UP, 1.0),
    ],
    &[
        ("Tab", KeyCode::TAB, 1.5),
        ("Q", KeyCode::Q, 1.0),
        ("W", KeyCode::W, 1.0),
        ("E", KeyCode::E, 1.0),
        ("R", KeyCode::R, 1.0),
        ("T", KeyCode::T, 1.0),
        ("Y", KeyCode::Y, 1.0),
        ("U", KeyCode::U, 1.0),
        ("I", KeyCode::I, 1.0),
        ("O", KeyCode::O, 1.0),
        ("P", KeyCode::P, 1.0),
        ("[", KeyCode::LEFT_BRACKET, 1.0),
        ("]", KeyCode::RIGHT_BRACKET, 1.0),
        ("\\", KeyCode::BACKSLASH, 1.5),
        ("Del", KeyCode::DELETE, 1.0),
        ("End", KeyCode::END, 1.0),
        ("PgDn", KeyCode::PAGE_DOWN, 1.0),
    ],
    &[
        ("Caps", KeyCode::CAPS_LOCK, 1.75),
        ("A", KeyCode::A, 1.0),
        ("S", KeyCode::S, 1.0),
        ("D", KeyCode::D, 1.0),
        ("F", KeyCode::F, 1.0),
        ("G", KeyCode::G, 1.0),
        ("H", KeyCode::H, 1.0),
        ("J", KeyCode::J, 1.0),
        ("K", KeyCode::K, 1.0),
        ("L", KeyCode::L, 1.0),
        (";", KeyCode::SEMICOLON, 1.0),
        ("'", KeyCode::QUOTE, 1.0),
        ("Enter", KeyCode::ENTER, 2.25),
    ],
    &[
        ("Shift", KeyCode::LEFT_SHIFT, 2.25),
        ("Z", KeyCode::Z, 1.0),
        ("X", KeyCode::X, 1.0),
        ("C", KeyCode::C, 1.0),
        ("V", KeyCode::V, 1.0),
        ("B", KeyCode::B, 1.0),
        ("N", KeyCode::N, 1.0),
        ("M", KeyCode::M, 1.0),
        (",", KeyCode::COMMA, 1.0),
        (".", KeyCode::PERIOD, 1.0),
        ("/", KeyCode::SLASH, 1.0),
        ("Shift", KeyCode::RIGHT_SHIFT, 2.75),
        ("Up", KeyCode::UP, 1.0),
    ],
    &[
        ("Ctrl", KeyCode::LEFT_CTRL, 1.5),
        ("Win", KeyCode::LEFT_GUI, 1.25),
        ("Alt", KeyCode::LEFT_ALT, 1.25),
        ("Space", KeyCode::SPACE, 6.0),
        ("AltGr", KeyCode::RIGHT_ALT, 1.25),
        ("Menu", KeyCode::APPLICATION, 1.25),
        ("Ctrl", KeyCode::RIGHT_CTRL, 1.5),
        ("Left", KeyCode::LEFT, 1.0),
        ("Down", KeyCode::DOWN, 1.0),
        ("Right", KeyCode::RIGHT, 1.0),
    ],
];

const NUMPAD: [&[Cap]; 5] = [
    &[
        ("Num", KeyCode::NUM_LOCK, 1.0),
        ("/", KeyCode::NUMPAD_DIVIDE, 1.0),
        ("*", KeyCode::NUMPAD_MULTIPLY, 1.0),
        ("-", KeyCode::NUMPAD_SUBTRACT, 1.0),
    ],
    &[
        ("7", KeyCode::NUMPAD_7, 1.0),
        ("8", KeyCode::NUMPAD_8, 1.0),
        ("9", KeyCode::NUMPAD_9, 1.0),
        ("+", KeyCode::NUMPAD_ADD, 1.0),
    ],
    &[("4", KeyCode::NUMPAD_4, 1.0), ("5", KeyCode::NUMPAD_5, 1.0), ("6", KeyCode::NUMPAD_6, 1.0)],
    &[
        ("1", KeyCode::NUMPAD_1, 1.0),
        ("2", KeyCode::NUMPAD_2, 1.0),
        ("3", KeyCode::NUMPAD_3, 1.0),
        ("Ent", KeyCode::NUMPAD_ENTER, 1.0),
    ],
    &[("0", KeyCode::NUMPAD_0, 2.0), (".", KeyCode::NUMPAD_DECIMAL, 1.0)],
];

const MOUSE: [(&str, Slot); 9] = [
    ("Left click", Slot::MouseButton(MouseButton::Left)),
    ("Right click", Slot::MouseButton(MouseButton::Right)),
    ("Middle click", Slot::MouseButton(MouseButton::Middle)),
    ("Back (X1)", Slot::MouseButton(MouseButton::X1)),
    ("Forward (X2)", Slot::MouseButton(MouseButton::X2)),
    ("Wheel up", Slot::Wheel(WheelDirection::Up)),
    ("Wheel down", Slot::Wheel(WheelDirection::Down)),
    ("Wheel left", Slot::Wheel(WheelDirection::Left)),
    ("Wheel right", Slot::Wheel(WheelDirection::Right)),
];

/// Shows the on-screen keyboard, mouse buttons and wheel. Returns the clicked output. Keys held
/// on the real keyboard light up.
pub(crate) fn picker(ui: &mut Ui) -> Option<Slot> {
    let unit = 30.0;
    let mut picked = None;
    let down = keys_down(ui);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            for row in ROWS {
                ui.horizontal(|ui| caps(ui, row, unit, &down, &mut picked));
            }
        });
        ui.add_space(unit * 0.5);
        ui.vertical(|ui| {
            for row in NUMPAD {
                ui.horizontal(|ui| caps(ui, row, unit, &down, &mut picked));
            }
        });
    });
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        for (label, slot) in MOUSE {
            if ui.button(label).clicked() {
                picked = Some(slot);
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("More:");
        for key in [KeyCode::F13, KeyCode::F14, KeyCode::F15, KeyCode::F16, KeyCode::RIGHT_GUI] {
            if ui.button(key.to_string()).clicked() {
                picked = Some(Slot::Key(key));
            }
        }
    });
    picked
}

fn caps(ui: &mut Ui, row: &[Cap], unit: f32, down: &[KeyCode], picked: &mut Option<Slot>) {
    ui.spacing_mut().item_spacing.x = 2.0;
    for &(label, key, width) in row {
        let button = egui::Button::new(label)
            .min_size(Vec2::new(unit * width, unit * 0.9))
            .selected(down.contains(&key));
        if ui.add(button).on_hover_text(key.to_string()).clicked() {
            *picked = Some(Slot::Key(key));
        }
    }
}

/// Ctrl, Shift and Alt: egui reports them only as modifier state, never as key events.
const MODIFIERS: [KeyCode; 3] = [KeyCode::LEFT_CTRL, KeyCode::LEFT_SHIFT, KeyCode::LEFT_ALT];

pub(crate) fn is_modifier(key: KeyCode) -> bool {
    MODIFIERS.contains(&key)
}

/// Keys pressed on the real keyboard this frame, in order, by physical position. `held` is the
/// modifier state seen last time; a modifier that went down since counts as a press.
pub(crate) fn pressed_keys(ui: &Ui, held: &mut Modifiers) -> Vec<KeyCode> {
    ui.input(|input| {
        let now = input.modifiers;
        let mut keys: Vec<KeyCode> =
            [(held.ctrl, now.ctrl), (held.shift, now.shift), (held.alt, now.alt)]
                .into_iter()
                .zip(MODIFIERS)
                .filter(|&((was, is), _)| is && !was)
                .map(|(_, key)| key)
                .collect();
        *held = now;
        keys.extend(input.events.iter().filter_map(|event| match event {
            egui::Event::Key { key, physical_key, pressed: true, repeat: false, .. } => {
                key_code(physical_key.unwrap_or(*key))
            }
            _ => None,
        }));
        keys
    })
}

/// Keys held on the real keyboard right now, for lighting up the on-screen ones.
fn keys_down(ui: &Ui) -> Vec<KeyCode> {
    ui.input(|input| {
        let m = input.modifiers;
        let modifiers = [
            (m.ctrl, [KeyCode::LEFT_CTRL, KeyCode::RIGHT_CTRL]),
            (m.shift, [KeyCode::LEFT_SHIFT, KeyCode::RIGHT_SHIFT]),
            (m.alt, [KeyCode::LEFT_ALT, KeyCode::RIGHT_ALT]),
        ];
        let held = modifiers.into_iter().filter(|(on, _)| *on).flat_map(|(_, keys)| keys);
        input.keys_down.iter().filter_map(|&key| key_code(key)).chain(held).collect()
    })
}

fn key_code(key: Key) -> Option<KeyCode> {
    let letters = [
        (Key::A, KeyCode::A),
        (Key::B, KeyCode::B),
        (Key::C, KeyCode::C),
        (Key::D, KeyCode::D),
        (Key::E, KeyCode::E),
        (Key::F, KeyCode::F),
        (Key::G, KeyCode::G),
        (Key::H, KeyCode::H),
        (Key::I, KeyCode::I),
        (Key::J, KeyCode::J),
        (Key::K, KeyCode::K),
        (Key::L, KeyCode::L),
        (Key::M, KeyCode::M),
        (Key::N, KeyCode::N),
        (Key::O, KeyCode::O),
        (Key::P, KeyCode::P),
        (Key::Q, KeyCode::Q),
        (Key::R, KeyCode::R),
        (Key::S, KeyCode::S),
        (Key::T, KeyCode::T),
        (Key::U, KeyCode::U),
        (Key::V, KeyCode::V),
        (Key::W, KeyCode::W),
        (Key::X, KeyCode::X),
        (Key::Y, KeyCode::Y),
        (Key::Z, KeyCode::Z),
    ];
    let others = [
        (Key::Num0, KeyCode::DIGIT_0),
        (Key::Num1, KeyCode::DIGIT_1),
        (Key::Num2, KeyCode::DIGIT_2),
        (Key::Num3, KeyCode::DIGIT_3),
        (Key::Num4, KeyCode::DIGIT_4),
        (Key::Num5, KeyCode::DIGIT_5),
        (Key::Num6, KeyCode::DIGIT_6),
        (Key::Num7, KeyCode::DIGIT_7),
        (Key::Num8, KeyCode::DIGIT_8),
        (Key::Num9, KeyCode::DIGIT_9),
        (Key::F1, KeyCode::F1),
        (Key::F2, KeyCode::F2),
        (Key::F3, KeyCode::F3),
        (Key::F4, KeyCode::F4),
        (Key::F5, KeyCode::F5),
        (Key::F6, KeyCode::F6),
        (Key::F7, KeyCode::F7),
        (Key::F8, KeyCode::F8),
        (Key::F9, KeyCode::F9),
        (Key::F10, KeyCode::F10),
        (Key::F11, KeyCode::F11),
        (Key::F12, KeyCode::F12),
        (Key::ArrowUp, KeyCode::UP),
        (Key::ArrowDown, KeyCode::DOWN),
        (Key::ArrowLeft, KeyCode::LEFT),
        (Key::ArrowRight, KeyCode::RIGHT),
        (Key::Escape, KeyCode::ESCAPE),
        (Key::Tab, KeyCode::TAB),
        (Key::Backspace, KeyCode::BACKSPACE),
        (Key::Enter, KeyCode::ENTER),
        (Key::Space, KeyCode::SPACE),
        (Key::Insert, KeyCode::INSERT),
        (Key::Delete, KeyCode::DELETE),
        (Key::Home, KeyCode::HOME),
        (Key::End, KeyCode::END),
        (Key::PageUp, KeyCode::PAGE_UP),
        (Key::PageDown, KeyCode::PAGE_DOWN),
        (Key::Minus, KeyCode::MINUS),
        (Key::Equals, KeyCode::EQUAL),
        (Key::Plus, KeyCode::EQUAL),
        (Key::OpenBracket, KeyCode::LEFT_BRACKET),
        (Key::CloseBracket, KeyCode::RIGHT_BRACKET),
        (Key::Backslash, KeyCode::BACKSLASH),
        (Key::Semicolon, KeyCode::SEMICOLON),
        (Key::Quote, KeyCode::QUOTE),
        (Key::Backtick, KeyCode::GRAVE),
        (Key::Comma, KeyCode::COMMA),
        (Key::Period, KeyCode::PERIOD),
        (Key::Slash, KeyCode::SLASH),
    ];
    letters.into_iter().chain(others).find(|&(k, _)| k == key).map(|(_, code)| code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, pressed: bool, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers }
    }

    /// Runs one frame with `events` and returns what `pressed_keys` saw.
    fn frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        modifiers: Modifiers,
        held: &mut Modifiers,
    ) -> Vec<KeyCode> {
        let mut keys = Vec::new();
        let events = [vec![egui::Event::ModifiersChanged(modifiers)], events].concat();
        let input = egui::RawInput { events, ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| keys = pressed_keys(ui, held));
        output.textures_delta.clear();
        keys
    }

    #[test]
    fn ctrl_then_z_is_a_combination() {
        let ctx = egui::Context::default();
        let mut held = Modifiers::NONE;
        assert_eq!(frame(&ctx, Vec::new(), Modifiers::CTRL, &mut held), [KeyCode::LEFT_CTRL]);
        let z = key(Key::Z, true, Modifiers::CTRL);
        assert_eq!(frame(&ctx, vec![z], Modifiers::CTRL, &mut held), [KeyCode::Z]);
        // Still holding Ctrl adds nothing more.
        assert!(frame(&ctx, Vec::new(), Modifiers::CTRL, &mut held).is_empty());
    }

    #[test]
    fn releases_and_held_keys_add_nothing() {
        let ctx = egui::Context::default();
        let mut held = Modifiers::NONE;
        let a = || key(Key::A, true, Modifiers::NONE);
        assert_eq!(frame(&ctx, vec![a()], Modifiers::NONE, &mut held), [KeyCode::A]);
        // The key is still down, so egui reports this as a repeat.
        assert!(frame(&ctx, vec![a()], Modifiers::NONE, &mut held).is_empty());
        let up = key(Key::A, false, Modifiers::NONE);
        assert!(frame(&ctx, vec![up], Modifiers::NONE, &mut held).is_empty());
    }
}
