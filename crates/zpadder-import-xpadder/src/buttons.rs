//! Which ZPadder control each Xpadder button number is.
//!
//! Xpadder reads pads through DirectInput and numbers their buttons `Button1`–`Button32` in the
//! order the driver reports them; ZPadder reads pads through SDL and names buttons by position.
//! The order depends on the pad, so the importer needs a [`ButtonMap`]: one inferred from a
//! controller file's button names, or a preset.

use std::fmt;

use zpadder_model::{Button, Control};

/// How many buttons Xpadder supports.
const MAX: usize = 32;

/// Maps Xpadder button numbers (1–32) to ZPadder controls, each used at most once.
///
/// Most numbers map to a [`Button`]. Some pads also report their triggers as buttons through
/// DirectInput (L2 and R2 on a DualShock 4; LT and RT on Logitech's Dual Action family, whose
/// triggers are digital). Those numbers map to [`Control::LeftTrigger`] and
/// [`Control::RightTrigger`], because SDL, and so ZPadder, reads them as triggers.
#[derive(Clone, PartialEq, Eq)]
pub struct ButtonMap {
    controls: [Option<Control>; MAX],
}

impl ButtonMap {
    /// The names [`ButtonMap::preset`] accepts, one per preset.
    pub const PRESETS: [&'static str; 2] = ["xbox", "dualshock4"];

    /// A map with no buttons.
    pub fn empty() -> Self {
        Self { controls: [None; MAX] }
    }

    /// Xbox 360 and Xbox One pads through DirectInput (Xpadder's usual case): 1 A, 2 B, 3 X,
    /// 4 Y, 5 LB, 6 RB, 7 Back/View, 8 Start/Menu, 9 left stick click, 10 right stick click.
    /// The triggers are an axis, and the guide button is not visible to DirectInput.
    pub fn xbox() -> Self {
        use Button::*;
        Self::from_list(&[
            South,
            East,
            West,
            North,
            LeftShoulder,
            RightShoulder,
            Back,
            Start,
            LeftStick,
            RightStick,
        ])
    }

    /// DualShock 4 through DirectInput: 1 Square, 2 Cross, 3 Circle, 4 Triangle, 5 L1, 6 R1,
    /// 7 L2, 8 R2, 9 Share, 10 Options, 11 L3, 12 R3, 13 PS, 14 touchpad click. Buttons 1–12
    /// follow the same order on Logitech's Dual Action, RumblePad 2 and F310/F710 (in
    /// DirectInput mode) and on most PlayStation-style DirectInput pads.
    pub fn dualshock4() -> Self {
        use Button::*;
        let mut map = Self::from_list(&[West, South, East, North, LeftShoulder, RightShoulder]);
        map.set(7, Some(Control::LeftTrigger));
        map.set(8, Some(Control::RightTrigger));
        for (number, button) in
            [(9, Back), (10, Start), (11, LeftStick), (12, RightStick), (13, Guide), (14, Touchpad)]
        {
            map.set(number, Some(Control::Button(button)));
        }
        map
    }

    /// A preset by name, ignoring case: `xbox` (also `xbox360`, `xboxone`, `xinput`) or
    /// `dualshock4` (also `ds4`, `playstation`, `logitech`, `dualaction`). See
    /// [`ButtonMap::PRESETS`].
    pub fn preset(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "xbox" | "xbox360" | "xboxone" | "xinput" => Some(Self::xbox()),
            "dualshock4" | "ds4" | "playstation" | "logitech" | "dualaction" => {
                Some(Self::dualshock4())
            }
            _ => None,
        }
    }

    fn from_list(buttons: &[Button]) -> Self {
        let mut map = Self::empty();
        for (number, &button) in (1..).zip(buttons) {
            map.set(number, Some(Control::Button(button)));
        }
        map
    }

    /// The ZPadder control for Xpadder's `Button{number}`.
    pub fn get(&self, number: u8) -> Option<Control> {
        *self.controls.get(usize::from(number).checked_sub(1)?)?
    }

    /// Maps `number` (1–32) to `control`, unmapping any other number that had `control`.
    /// Numbers outside 1–32 are ignored.
    pub fn set(&mut self, number: u8, control: Option<Control>) {
        let Some(index) = usize::from(number).checked_sub(1).filter(|&i| i < MAX) else {
            return;
        };
        if let Some(control) = control {
            self.controls.iter_mut().filter(|c| **c == Some(control)).for_each(|c| *c = None);
        }
        self.controls[index] = control;
    }

    /// The Xpadder number of a ZPadder control.
    pub fn number_of(&self, control: Control) -> Option<u8> {
        self.iter().find(|&(_, c)| c == control).map(|(number, _)| number)
    }

    /// Every mapped number with its control, in number order.
    pub fn iter(&self) -> impl Iterator<Item = (u8, Control)> + '_ {
        (1..).zip(self.controls).filter_map(|(number, control)| Some((number, control?)))
    }
}

impl Default for ButtonMap {
    /// The Xbox preset: by far the most common pad in shared Xpadder files.
    fn default() -> Self {
        Self::xbox()
    }
}

impl fmt::Debug for ButtonMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.iter().map(|(number, control)| (number, control.to_string())))
            .finish()
    }
}

/// Which preset fills in the buttons whose names say nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Xbox,
    PlayStation,
    Either,
}

/// What a button's name says: the control, and which family of pads uses that name.
type Meaning = (Control, Family);

/// Recognises a button name, ignoring case, spaces and punctuation. Only whole names count, so
/// free text such as `Button 3` or `Botón 3` is not recognised.
fn meaning(name: &str) -> Option<Meaning> {
    use Button::*;
    use Family::{Either, PlayStation as Ps, Xbox};
    let key: String =
        name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect();
    let (control, family) = match key.as_str() {
        "a" | "abutton" => (Control::Button(South), Xbox),
        "b" | "bbutton" => (Control::Button(East), Xbox),
        "x" | "xbutton" => (Control::Button(West), Xbox),
        "y" | "ybutton" => (Control::Button(North), Xbox),
        "lb" | "leftbumper" | "lbumper" => (Control::Button(LeftShoulder), Xbox),
        "rb" | "rightbumper" | "rbumper" => (Control::Button(RightShoulder), Xbox),
        "back" | "view" => (Control::Button(Back), Xbox),
        "start" | "menu" => (Control::Button(Start), Xbox),
        "guide" | "xbox" | "xboxbutton" => (Control::Button(Guide), Xbox),
        "lt" => (Control::LeftTrigger, Xbox),
        "rt" => (Control::RightTrigger, Xbox),
        "cross" => (Control::Button(South), Ps),
        "circle" => (Control::Button(East), Ps),
        "square" => (Control::Button(West), Ps),
        "triangle" => (Control::Button(North), Ps),
        "l1" => (Control::Button(LeftShoulder), Ps),
        "r1" => (Control::Button(RightShoulder), Ps),
        "l2" => (Control::LeftTrigger, Ps),
        "r2" => (Control::RightTrigger, Ps),
        "share" | "create" => (Control::Button(Back), Ps),
        "options" => (Control::Button(Start), Ps),
        "ps" | "psbutton" | "playstation" => (Control::Button(Guide), Ps),
        "touchpad" | "touchpadclick" | "touchpadbutton" => (Control::Button(Touchpad), Ps),
        "select" => (Control::Button(Back), Either),
        "home" => (Control::Button(Guide), Either),
        "leftshoulder" | "lshoulder" => (Control::Button(LeftShoulder), Either),
        "rightshoulder" | "rshoulder" => (Control::Button(RightShoulder), Either),
        "lefttrigger" | "ltrigger" | "zl" => (Control::LeftTrigger, Either),
        "righttrigger" | "rtrigger" | "zr" => (Control::RightTrigger, Either),
        "l3" | "ls" | "lsb" | "lclick" | "leftclick" | "leftstick" | "leftstickclick"
        | "leftstickbutton" | "lstick" | "lsclick" | "leftthumb" | "lthumb" => {
            (Control::Button(LeftStick), Either)
        }
        "r3" | "rs" | "rsb" | "rclick" | "rightclick" | "rightstick" | "rightstickclick"
        | "rightstickbutton" | "rstick" | "rsclick" | "rightthumb" | "rthumb" => {
            (Control::Button(RightStick), Either)
        }
        "up" | "dpadup" => (Control::Button(DPadUp), Either),
        "down" | "dpaddown" => (Control::Button(DPadDown), Either),
        "left" | "dpadleft" => (Control::Button(DPadLeft), Either),
        "right" | "dpadright" => (Control::Button(DPadRight), Either),
        _ => return None,
    };
    Some((control, family))
}

/// Builds a button map from a controller file's `Button{n}Name` values, plus notes for the user.
///
/// Recognised names are mapped directly. If none are recognised the Xbox preset is used.
/// Otherwise the remaining numbers come from the preset of the family the recognised names
/// belong to (Xbox, or DualShock 4 for PlayStation names), where that does not clash.
pub(crate) fn infer(names: &[(u8, String)]) -> (ButtonMap, Vec<String>) {
    let mut notes = Vec::new();
    let meanings: Vec<(u8, &str, Option<Meaning>)> = names
        .iter()
        .filter(|(number, _)| (1..=MAX).contains(&usize::from(*number)))
        .map(|(number, name)| (*number, name.as_str(), meaning(name)))
        .collect();
    if meanings.iter().all(|(_, _, meaning)| meaning.is_none()) {
        if !names.is_empty() {
            let mut note = "the button names do not say which button is which, so the Xbox \
                            button order is assumed (1 A, 2 B, 3 X, 4 Y, 5 LB, 6 RB, 7 Back, \
                            8 Start, 9 and 10 the stick clicks); choose another button preset \
                            if this is a different pad"
                .to_string();
            let most = names.iter().map(|(number, _)| *number).max().unwrap_or(0);
            if most > 10 {
                note += &format!(
                    ". This pad has {most} buttons, more than an Xbox pad: the DualShock 4 \
                     preset fits Logitech's Dual Action family and many PlayStation-style pads"
                );
            }
            notes.push(note);
        }
        return (ButtonMap::xbox(), notes);
    }

    let count =
        |family| meanings.iter().filter(|(_, _, m)| m.is_some_and(|m| m.1 == family)).count();
    let (preset, preset_name) = if count(Family::PlayStation) > count(Family::Xbox) {
        (ButtonMap::dualshock4(), "DualShock 4")
    } else {
        (ButtonMap::xbox(), "Xbox")
    };

    let mut map = ButtonMap::empty();
    let mut decided = [false; MAX];
    for &(number, name, meaning) in &meanings {
        let Some((control, _)) = meaning else { continue };
        if let Some(other) = map.number_of(control) {
            notes.push(format!(
                "buttons {other} and {number} are both named like `{control}`; button {number} \
                 (`{name}`) is not imported"
            ));
        } else {
            map.set(number, Some(control));
        }
        decided[usize::from(number) - 1] = true;
    }

    let mut filled = Vec::new();
    for (number, control) in preset.iter() {
        if !decided[usize::from(number) - 1] && map.number_of(control).is_none() {
            map.set(number, Some(control));
            filled.push(number.to_string());
        }
    }
    let unrecognised: Vec<String> = meanings
        .iter()
        .filter(|(_, _, meaning)| meaning.is_none())
        .map(|(number, name, _)| format!("{number} (`{name}`)"))
        .collect();
    if !unrecognised.is_empty() {
        notes.push(format!(
            "unrecognised button names: {}; buttons {} follow the {preset_name} button order",
            unrecognised.join(", "),
            if filled.is_empty() { "none".to_string() } else { filled.join(", ") },
        ));
    }
    (map, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<(u8, String)> {
        (1..).zip(list).map(|(n, name)| (n, name.to_string())).collect()
    }

    fn button(b: Button) -> Option<Control> {
        Some(Control::Button(b))
    }

    #[test]
    fn presets() {
        let xbox = ButtonMap::xbox();
        assert_eq!(xbox.get(1), button(Button::South));
        assert_eq!(xbox.get(8), button(Button::Start));
        assert_eq!(xbox.get(10), button(Button::RightStick));
        assert_eq!(xbox.get(11), None);
        assert_eq!(xbox.get(0), None);
        assert_eq!(xbox.get(200), None);

        let ds4 = ButtonMap::dualshock4();
        assert_eq!(ds4.get(1), button(Button::West));
        assert_eq!(ds4.get(2), button(Button::South));
        assert_eq!(ds4.get(7), Some(Control::LeftTrigger));
        assert_eq!(ds4.get(8), Some(Control::RightTrigger));
        assert_eq!(ds4.get(13), button(Button::Guide));
        assert_eq!(ds4.get(14), button(Button::Touchpad));
        assert_eq!(ds4.iter().count(), 14);

        assert_eq!(ButtonMap::preset("DS4"), Some(ds4.clone()));
        assert_eq!(ButtonMap::preset("logitech"), Some(ds4));
        assert_eq!(ButtonMap::preset("Xbox"), Some(xbox.clone()));
        assert_eq!(ButtonMap::default(), xbox);
        assert!(ButtonMap::PRESETS.iter().all(|name| ButtonMap::preset(name).is_some()));
        assert_eq!(ButtonMap::preset("n64"), None);
        assert_eq!(format!("{:?}", ButtonMap::dualshock4().get(7).unwrap()), "LeftTrigger");
    }

    #[test]
    fn set_keeps_controls_unique() {
        let mut map = ButtonMap::xbox();
        map.set(12, button(Button::South));
        assert_eq!(map.get(1), None);
        assert_eq!(map.number_of(Control::Button(Button::South)), Some(12));
        map.set(33, button(Button::Guide));
        map.set(0, button(Button::Guide));
        assert_eq!(map.number_of(Control::Button(Button::Guide)), None);
    }

    #[test]
    fn xbox_names() {
        // From a real Xbox 360 layout, typo included ("LR" for RB).
        let (map, notes) =
            infer(&names(&["A", "B", "X", "Y", "LB", "LR", "Back", "Start", "L-Click", "R-Click"]));
        assert_eq!(map, ButtonMap::xbox());
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("6 (`LR`)"), "{notes:?}");

        let (map, _) =
            infer(&names(&["A", "B", "X", "Y", "LB", "RB", "View", "Menu", "L", "Button 10"]));
        assert_eq!(map, ButtonMap::xbox());
    }

    #[test]
    fn playstation_names() {
        let (map, notes) = infer(&names(&[
            "Square", "Cross", "Circle", "Triangle", "L1", "R1", "L2", "R2", "Share", "Options",
            "L3", "R3", "PS", "Touchpad",
        ]));
        assert_eq!(map, ButtonMap::dualshock4());
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn generic_names_fall_back_to_xbox() {
        let (map, notes) = infer(&names(&["Button 1", "Bot\u{f3}n 2", "3Button 3"]));
        assert_eq!(map, ButtonMap::xbox());
        assert_eq!(notes.len(), 1);
        assert!(!notes[0].contains("DualShock"), "{notes:?}");
        let twelve: Vec<(u8, String)> = (1..=12).map(|n| (n, format!("Button {n}"))).collect();
        assert!(infer(&twelve).1[0].contains("This pad has 12 buttons"));
        assert_eq!(infer(&[]), (ButtonMap::xbox(), vec![]));
    }

    #[test]
    fn duplicate_names_keep_the_first() {
        let (map, notes) = infer(&names(&["A", "Cross"]));
        assert_eq!(map.get(1), button(Button::South));
        assert_eq!(map.get(2), None);
        assert!(notes.iter().any(|n| n.contains("both")), "{notes:?}");
    }
}
