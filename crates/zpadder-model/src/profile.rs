//! Profiles: what each control does, organised in switchable layers.
//!
//! A profile is a superset of an Xpadder profile. Xpadder's eight "sets" are ZPadder's layers
//! (any number of them), and an Xpadder assignment's slot list, including its timing slots, is
//! an [`Assignment`]'s `slots`.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::{Button, KeyCode, MouseButton, VirtualPad};

/// The lowest [`Layer::trigger_threshold`]. A trigger resting just above a lower threshold
/// would fire on noise, and its release point would sit at the resting value.
pub const MIN_TRIGGER_THRESHOLD: f32 = 0.05;
/// The lowest [`Stick::dead_zone`] for a stick in `directions` mode, for the same reason: a
/// centred stick's noise must stay inside it.
pub const MIN_DIRECTION_DEAD_ZONE: f32 = 0.05;
/// The highest [`MouseSettings::speed`], in pixels per second.
pub const MAX_MOUSE_SPEED: f32 = 20_000.0;

/// A complete mapping for a controller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default)]
    pub name: String,
    /// Controller picture to show in the editor: `builtin:<id>` for a built-in photo, or a layout
    /// file relative to the profile. Without one the editor shows the connected controller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    /// Presents each controller to games as a virtual controller too (`xbox360`). Controls with
    /// nothing assigned in the active layer act as its buttons, sticks and triggers. Needs the
    /// ViGEmBus driver; `docs/profiles.md` has the details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_pad: Option<VirtualPad>,
    /// The first layer is active when the profile loads.
    #[cfg_attr(feature = "schema", schemars(length(min = 1)))]
    pub layers: Vec<Layer>,
}

/// One complete set of assignments. Only one layer is active at a time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub name: String,
    /// How far a trigger must be pulled (0.05 to below 1) before its assignment fires.
    #[serde(default = "default_trigger_threshold")]
    #[cfg_attr(
        feature = "schema",
        schemars(range(min = 0.05), extend("exclusiveMaximum" = 1.0))
    )]
    pub trigger_threshold: f32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub buttons: BTreeMap<Button, Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_trigger: Option<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_trigger: Option<Assignment>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub left_stick: Stick,
    #[serde(default, skip_serializing_if = "is_default")]
    pub right_stick: Stick,
}

impl Layer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            trigger_threshold: default_trigger_threshold(),
            buttons: BTreeMap::new(),
            left_trigger: None,
            right_trigger: None,
            left_stick: Stick::default(),
            right_stick: Stick::default(),
        }
    }
}

/// What one control does: a list of slots plus modifiers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    /// Optional label, e.g. "Jump".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// What the control does, as a timeline. Plain outputs are pressed in order when the control
    /// is pressed and released in reverse when it is released, so several keys form a
    /// combination such as Shift+X. Timing slots turn the list into sequences, tap-or-hold and
    /// so on; `docs/profiles.md` describes them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<Slot>,
    /// A press latches the outputs on; the next press releases them. Not combinable with
    /// timing slots or a `while_held` layer switch (use `two_way` for a latching switch).
    #[serde(default, skip_serializing_if = "is_false")]
    pub toggle: bool,
    /// Repeats the outputs while held, at this many presses per second (0.5–60).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(range(min = 0.5, max = 60.0)))]
    pub turbo: Option<f32>,
    /// Vibrates the pad while the assignment is held.
    #[serde(default, skip_serializing_if = "is_false")]
    pub rumble: bool,
    /// Switches layers when the control is pressed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerSelect>,
    /// When the profile acts as an Xbox controller (`virtual_pad`), the control also works as
    /// its Xbox counterpart (button or trigger) while doing the assignment. Without it, an
    /// assigned control does only the assignment.
    #[serde(default, skip_serializing_if = "is_false")]
    pub passthrough: bool,
}

impl Assignment {
    /// Whether the assignment has nothing in it, and so does nothing. A control with an empty
    /// assignment counts as unassigned.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// One entry in an assignment's slot list, written in profiles as a string.
///
/// Outputs are a key name (`left_shift`, `a`, `f1`, `numpad_enter`),
/// `mouse_left`/`right`/`middle`/`x1`/`x2` or `wheel_up`/`down`/`left`/`right`. Timing slots
/// are `pause 0.15s`, `hold 0.5s`, `release 0.01s`, `distance 50%`, `tap_time 0.05s` and
/// `cycle`; Xpadder's spellings (`PAUSE 0.15s`, `HOLD ZONE 0.50s`, `CYCLE STOP`, ...) are
/// accepted too. Durations may be written `0.15s`, `150ms` or `1m30s`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Slot {
    Key(KeyCode),
    MouseButton(MouseButton),
    /// One wheel notch per press (repeated by turbo).
    Wheel(WheelDirection),
    /// Turns the list into a sequence of taps separated by pauses.
    Pause(Duration),
    /// Later slots apply once the control has been held this long (measured from the press).
    HoldZone(Duration),
    /// Later slots fire on release, if the control was held at least this long.
    ReleaseZone(Duration),
    /// For sticks and triggers: later slots apply beyond this deflection, in percent.
    DistanceZone(f32),
    /// How long later taps hold their keys down (default 0.1 s). Also separates taps.
    TapTime(Duration),
    /// Splits the list into alternatives used on successive presses.
    CycleStop,
}

impl Slot {
    /// Whether this is a timing slot rather than an output.
    pub fn is_timing(&self) -> bool {
        !matches!(self, Slot::Key(_) | Slot::MouseButton(_) | Slot::Wheel(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelDirection {
    Up,
    Down,
    Left,
    Right,
}

const MOUSE_SLOTS: [(&str, Slot); 9] = [
    ("mouse_left", Slot::MouseButton(MouseButton::Left)),
    ("mouse_right", Slot::MouseButton(MouseButton::Right)),
    ("mouse_middle", Slot::MouseButton(MouseButton::Middle)),
    ("mouse_x1", Slot::MouseButton(MouseButton::X1)),
    ("mouse_x2", Slot::MouseButton(MouseButton::X2)),
    ("wheel_up", Slot::Wheel(WheelDirection::Up)),
    ("wheel_down", Slot::Wheel(WheelDirection::Down)),
    ("wheel_left", Slot::Wheel(WheelDirection::Left)),
    ("wheel_right", Slot::Wheel(WheelDirection::Right)),
];

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Slot::Key(key) => match key.name() {
                Some(name) => f.write_str(&name),
                None => write!(f, "0x{:02x}", key.0),
            },
            Slot::MouseButton(button) => f.write_str(match button {
                MouseButton::Left => "mouse_left",
                MouseButton::Right => "mouse_right",
                MouseButton::Middle => "mouse_middle",
                MouseButton::X1 => "mouse_x1",
                MouseButton::X2 => "mouse_x2",
            }),
            Slot::Wheel(direction) => f.write_str(match direction {
                WheelDirection::Up => "wheel_up",
                WheelDirection::Down => "wheel_down",
                WheelDirection::Left => "wheel_left",
                WheelDirection::Right => "wheel_right",
            }),
            Slot::Pause(d) => write!(f, "pause {}", format_duration(*d)),
            Slot::HoldZone(d) => write!(f, "hold {}", format_duration(*d)),
            Slot::ReleaseZone(d) => write!(f, "release {}", format_duration(*d)),
            Slot::DistanceZone(percent) => write!(f, "distance {percent}%"),
            Slot::TapTime(d) => write!(f, "tap_time {}", format_duration(*d)),
            Slot::CycleStop => f.write_str("cycle"),
        }
    }
}

impl FromStr for Slot {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if let Some(&(_, slot)) = MOUSE_SLOTS.iter().find(|(name, _)| name.eq_ignore_ascii_case(s))
        {
            return Ok(slot);
        }
        if let Some(key) = KeyCode::from_name(s) {
            return Ok(Slot::Key(key));
        }
        // Timing slots, in ZPadder's or Xpadder's spelling.
        let lower = s.to_ascii_lowercase().replace(" zone", "").replace("tap time", "tap_time");
        if lower == "cycle" || lower == "cycle stop" {
            return Ok(Slot::CycleStop);
        }
        let duration = |text: &str| {
            parse_duration(text).ok_or_else(|| format!("`{s}`: cannot read the duration `{text}`"))
        };
        match lower.split_once(' ') {
            Some(("pause", arg)) => Ok(Slot::Pause(duration(arg)?)),
            Some(("hold", arg)) => Ok(Slot::HoldZone(duration(arg)?)),
            Some(("release", arg)) => Ok(Slot::ReleaseZone(duration(arg)?)),
            Some(("tap_time", arg)) => Ok(Slot::TapTime(duration(arg)?)),
            Some(("distance", arg)) => arg
                .trim()
                .trim_end_matches('%')
                .parse::<f32>()
                .map(Slot::DistanceZone)
                .map_err(|_| format!("`{s}`: cannot read the percentage `{arg}`")),
            _ => Err(format!("unknown key or slot `{s}`")),
        }
    }
}

/// Reads `0.15s`, `150ms` or `1m30s` (and Xpadder's `10m00.00s`), to the millisecond.
fn parse_duration(text: &str) -> Option<Duration> {
    let text = text.trim();
    let seconds = if let Some(ms) = text.strip_suffix("ms") {
        ms.parse::<f64>().ok()? / 1000.0
    } else {
        let text = text.strip_suffix('s')?;
        match text.split_once('m') {
            Some((minutes, seconds)) => {
                minutes.parse::<f64>().ok()? * 60.0 + seconds.parse::<f64>().ok()?
            }
            None => text.parse::<f64>().ok()?,
        }
    };
    (seconds.is_finite() && seconds >= 0.0)
        .then(|| Duration::from_millis((seconds * 1000.0).round() as u64))
}

fn format_duration(d: Duration) -> String {
    format!("{}s", d.as_millis() as f64 / 1000.0)
}

/// Slots are strings: editors get every output name as a completion, and timing slots by
/// pattern, in both ZPadder's and Xpadder's spelling.
#[cfg(feature = "schema")]
impl schemars::JsonSchema for Slot {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Slot".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut names: Vec<String> = KeyCode::names().collect();
        names.extend(MOUSE_SLOTS.iter().map(|(name, _)| name.to_string()));
        names.extend((0..=9).map(|digit| digit.to_string()));
        // Timing slots are read ignoring case, but JSON Schema patterns have no flag for that.
        let duration = format!(
            r"([0-9]+{m}[0-9]*\.?[0-9]+{s}|[0-9]*\.?[0-9]+({s}|{ms}))",
            m = any_case("m"),
            s = any_case("s"),
            ms = any_case("ms"),
        );
        let zone = any_case("zone");
        let timed = format!(
            "^({pause}|{hold}( {zone})?|{release}( {zone})?|{tap}[ _]{time}) {duration}$",
            pause = any_case("pause"),
            hold = any_case("hold"),
            release = any_case("release"),
            tap = any_case("tap"),
            time = any_case("time"),
        );
        let distance = format!(r"^{}( {zone})? [0-9]*\.?[0-9]+%?$", any_case("distance"));
        let other =
            format!("^({}( {})?|0x[0-9a-fA-F]{{1,4}})$", any_case("cycle"), any_case("stop"));
        schemars::json_schema!({
            "description": "A key, mouse button or wheel output, or a timing slot \
                            (pause, hold, release, distance, tap_time, cycle). \
                            See docs/profiles.md.",
            "anyOf": [
                { "enum": names },
                { "type": "string", "pattern": timed },
                { "type": "string", "pattern": distance },
                { "type": "string", "pattern": other }
            ]
        })
    }
}

/// A regular expression matching `word` in any mix of upper and lower case.
#[cfg(feature = "schema")]
fn any_case(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            'a'..='z' => format!("[{c}{}]", c.to_ascii_uppercase()),
            _ => c.to_string(),
        })
        .collect()
}

impl Serialize for Slot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Slot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?.parse().map_err(de::Error::custom)
    }
}

/// A layer switch attached to an assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LayerSelect {
    /// Name of the layer to switch to.
    pub target: String,
    #[serde(default)]
    pub mode: SelectMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SelectMode {
    /// Switch and stay.
    #[default]
    OneWay,
    /// Switch; the next press switches back.
    TwoWay,
    /// Switch only while the control is held.
    WhileHeld,
}

/// Settings for one analog stick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Stick {
    #[serde(default)]
    pub mode: StickMode,
    /// Deflection (0 to below 1) below which the stick counts as centred. At least 0.05 in
    /// `directions` mode.
    #[serde(default = "default_dead_zone")]
    #[cfg_attr(feature = "schema", schemars(range(min = 0.0), extend("exclusiveMaximum" = 1.0)))]
    pub dead_zone: f32,
    /// Used when `mode` is `mouse`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub mouse: MouseSettings,
    /// Used when `mode` is `directions`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub directions: DirectionSettings,
    /// Used when `mode` is `scroll`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub scroll: ScrollSettings,
    /// When the profile acts as an Xbox controller (`virtual_pad`), the stick also moves the
    /// Xbox controller's stick, whatever its mode. In `off` mode it always does.
    #[serde(default, skip_serializing_if = "is_false")]
    pub passthrough: bool,
}

impl Default for Stick {
    fn default() -> Self {
        Self {
            mode: StickMode::Off,
            dead_zone: default_dead_zone(),
            mouse: MouseSettings::default(),
            directions: DirectionSettings::default(),
            scroll: ScrollSettings::default(),
            passthrough: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StickMode {
    #[default]
    Off,
    /// Moves the pointer (Xpadder's "Normal" mouse mode).
    Mouse,
    /// Acts as four or eight direction buttons.
    Directions,
    /// Scrolls like a mouse wheel: up/down scrolls vertically, left/right horizontally.
    Scroll,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ScrollSettings {
    /// Wheel notches per second at full deflection (above 0, at most 100).
    #[serde(default = "default_scroll_speed")]
    #[cfg_attr(feature = "schema", schemars(range(max = 100.0), extend("exclusiveMinimum" = 0.0)))]
    pub speed: f32,
    /// Also scroll sideways when the stick moves left or right.
    #[serde(default, skip_serializing_if = "is_false")]
    pub horizontal: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
}

impl Default for ScrollSettings {
    fn default() -> Self {
        Self { speed: default_scroll_speed(), horizontal: false, invert: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MouseSettings {
    /// Pointer speed at full deflection, in pixels per second (0–20000).
    #[serde(default = "default_mouse_speed")]
    #[cfg_attr(feature = "schema", schemars(range(min = 0.0, max = 20000.0)))]
    pub speed: f32,
    /// Response curve exponent (0.2–5): 1 is linear; above 1 gives finer control near the
    /// centre while keeping full speed at full tilt.
    #[serde(default = "default_curve", skip_serializing_if = "is_linear")]
    #[cfg_attr(feature = "schema", schemars(range(min = 0.2, max = 5.0)))]
    pub curve: f32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert_x: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert_y: bool,
}

impl Default for MouseSettings {
    fn default() -> Self {
        Self {
            speed: default_mouse_speed(),
            curve: default_curve(),
            invert_x: false,
            invert_y: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DirectionSettings {
    /// Diagonals have their own assignments. Otherwise a diagonal presses both neighbouring
    /// directions (up + right), like holding W and D.
    #[serde(default, skip_serializing_if = "is_false")]
    pub eight_way: bool,
    /// Width of each diagonal sector in degrees (0–90).
    #[serde(default = "default_diagonal_size")]
    #[cfg_attr(feature = "schema", schemars(range(min = 0.0, max = 90.0)))]
    pub diagonal_size: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up: Option<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down: Option<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_left: Option<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_right: Option<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down_left: Option<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down_right: Option<Assignment>,
}

impl Default for DirectionSettings {
    fn default() -> Self {
        Self {
            eight_way: false,
            diagonal_size: default_diagonal_size(),
            up: None,
            down: None,
            left: None,
            right: None,
            up_left: None,
            up_right: None,
            down_left: None,
            down_right: None,
        }
    }
}

/// A stick direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl DirectionSettings {
    pub fn get(&self, direction: Direction) -> Option<&Assignment> {
        match direction {
            Direction::Up => self.up.as_ref(),
            Direction::Down => self.down.as_ref(),
            Direction::Left => self.left.as_ref(),
            Direction::Right => self.right.as_ref(),
            Direction::UpLeft => self.up_left.as_ref(),
            Direction::UpRight => self.up_right.as_ref(),
            Direction::DownLeft => self.down_left.as_ref(),
            Direction::DownRight => self.down_right.as_ref(),
        }
    }

    /// Every assignment, with its direction's name in profile files.
    fn assignments(&self) -> impl Iterator<Item = (&'static str, &Assignment)> {
        [
            ("up", &self.up),
            ("down", &self.down),
            ("left", &self.left),
            ("right", &self.right),
            ("up_left", &self.up_left),
            ("up_right", &self.up_right),
            ("down_left", &self.down_left),
            ("down_right", &self.down_right),
        ]
        .into_iter()
        .filter_map(|(name, assignment)| Some((name, assignment.as_ref()?)))
    }
}

impl Profile {
    /// Index of the layer called `name`.
    pub fn layer_index(&self, name: &str) -> Option<usize> {
        self.layers.iter().position(|layer| layer.name == name)
    }

    /// Checks everything serde cannot: layer references, ranges and duplicates. Returns every
    /// problem found, each as a readable sentence.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.layers.is_empty() {
            errors.push("the profile needs at least one layer".to_string());
        }
        for (i, layer) in self.layers.iter().enumerate() {
            let at = format!("layer `{}`", layer.name);
            if self.layers[..i].iter().any(|other| other.name == layer.name) {
                errors.push(format!("{at}: another layer has the same name"));
            }
            if !(MIN_TRIGGER_THRESHOLD..1.0).contains(&layer.trigger_threshold) {
                errors.push(format!(
                    "{at}: trigger_threshold must be at least {MIN_TRIGGER_THRESHOLD} and below 1"
                ));
            }
            for (side, stick) in
                [("left_stick", &layer.left_stick), ("right_stick", &layer.right_stick)]
            {
                if !(0.0..1.0).contains(&stick.dead_zone) {
                    errors.push(format!("{at}, {side}: dead_zone must be at least 0 and below 1"));
                } else if stick.mode == StickMode::Directions
                    && stick.dead_zone < MIN_DIRECTION_DEAD_ZONE
                {
                    errors.push(format!(
                        "{at}, {side}: dead_zone must be at least {MIN_DIRECTION_DEAD_ZONE} in \
                         directions mode, or a centred stick's noise could press directions"
                    ));
                }
                if !(0.0..=90.0).contains(&stick.directions.diagonal_size) {
                    errors.push(format!(
                        "{at}, {side}: directions.diagonal_size must be 0 to 90 degrees"
                    ));
                }
                if !(stick.scroll.speed > 0.0 && stick.scroll.speed <= 100.0) {
                    errors.push(format!(
                        "{at}, {side}: scroll.speed must be above 0 and at most 100"
                    ));
                }
                if !(0.2..=5.0).contains(&stick.mouse.curve) {
                    errors.push(format!("{at}, {side}: mouse.curve must be 0.2 to 5"));
                }
                if !(0.0..=MAX_MOUSE_SPEED).contains(&stick.mouse.speed) {
                    errors.push(format!(
                        "{at}, {side}: mouse.speed must be 0 to {MAX_MOUSE_SPEED} pixels per second"
                    ));
                }
            }
            for (control, assignment) in layer.assignments() {
                self.check_assignment(&format!("{at}, {control}"), assignment, &mut errors);
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    fn check_assignment(&self, at: &str, assignment: &Assignment, errors: &mut Vec<String>) {
        if let Some(rate) = assignment.turbo
            && !(0.5..=60.0).contains(&rate)
        {
            errors.push(format!("{at}: turbo must be 0.5 to 60 presses per second"));
        }
        if let Some(select) = &assignment.layer
            && self.layer_index(&select.target).is_none()
        {
            errors.push(format!("{at}: no layer is called `{}`", select.target));
        }
        if assignment.toggle && assignment.slots.iter().any(Slot::is_timing) {
            errors.push(format!("{at}: toggle cannot be combined with timing slots"));
        }
        if assignment.toggle
            && assignment.layer.as_ref().is_some_and(|select| select.mode == SelectMode::WhileHeld)
        {
            errors.push(format!(
                "{at}: toggle cannot be combined with a while_held layer switch; for a switch \
                 that latches, use mode = \"two_way\""
            ));
        }
        check_slots(at, &assignment.slots, errors);
    }
}

/// Checks timing slots: ranges, zone order, and zone kinds that cannot be mixed.
fn check_slots(at: &str, slots: &[Slot], errors: &mut Vec<String>) {
    const LONGEST: Duration = Duration::from_secs(600);
    for slot in slots {
        let problem = match *slot {
            Slot::Pause(d) | Slot::HoldZone(d) | Slot::ReleaseZone(d) if d > LONGEST => {
                Some("pauses and zones can be at most 10 minutes")
            }
            // Either would press and release keys in the same instant, which games can miss.
            Slot::HoldZone(d) if d.is_zero() => Some("hold zones must start after 0 s"),
            Slot::TapTime(d) if d.is_zero() => Some("tap_time must be longer than 0 s"),
            Slot::TapTime(d) if d > Duration::from_secs(10) => Some("tap_time can be at most 10 s"),
            Slot::DistanceZone(p) if !(0.0..=100.0).contains(&p) => {
                Some("distance zones must be 0% to 100%")
            }
            _ => None,
        };
        if let Some(problem) = problem {
            errors.push(format!("{at}: `{slot}`: {problem}"));
        }
    }
    for alternative in slots.split(|slot| matches!(slot, Slot::CycleStop)) {
        let holds: Vec<Duration> = alternative
            .iter()
            .filter_map(|s| if let Slot::HoldZone(d) = s { Some(*d) } else { None })
            .collect();
        let releases: Vec<Duration> = alternative
            .iter()
            .filter_map(|s| if let Slot::ReleaseZone(d) = s { Some(*d) } else { None })
            .collect();
        let distances: Vec<f32> = alternative
            .iter()
            .filter_map(|s| if let Slot::DistanceZone(p) = s { Some(*p) } else { None })
            .collect();
        if !holds.is_sorted_by(|a, b| a < b) {
            errors.push(format!("{at}: hold zones must be in increasing order of time"));
        }
        if !releases.is_sorted_by(|a, b| a < b) {
            errors.push(format!("{at}: release zones must be in increasing order of time"));
        }
        if !distances.is_sorted_by(|a, b| a < b) {
            errors.push(format!("{at}: distance zones must be in increasing order"));
        }
        if !distances.is_empty() && (!holds.is_empty() || !releases.is_empty()) {
            errors.push(format!("{at}: distance zones cannot be mixed with hold or release zones"));
        }
    }
}

impl Layer {
    /// Every assignment in the layer, labelled with its place in a profile file, such as
    /// `buttons.south` or `left_stick.directions.up`.
    fn assignments(&self) -> impl Iterator<Item = (String, &Assignment)> {
        let buttons =
            self.buttons.iter().map(|(button, a)| (format!("buttons.{}", button.name()), a));
        let triggers =
            [("left_trigger", &self.left_trigger), ("right_trigger", &self.right_trigger)]
                .into_iter()
                .filter_map(|(name, a)| Some((name.to_string(), a.as_ref()?)));
        let sticks = [("left_stick", &self.left_stick), ("right_stick", &self.right_stick)]
            .into_iter()
            .flat_map(|(side, stick)| {
                stick
                    .directions
                    .assignments()
                    .map(move |(direction, a)| (format!("{side}.directions.{direction}"), a))
            });
        buttons.chain(triggers).chain(sticks)
    }
}

fn default_trigger_threshold() -> f32 {
    0.2
}

fn default_dead_zone() -> f32 {
    0.25
}

fn default_mouse_speed() -> f32 {
    1000.0
}

fn default_scroll_speed() -> f32 {
    8.0
}

fn default_curve() -> f32 {
    1.0
}

fn is_linear(curve: &f32) -> bool {
    *curve == 1.0
}

fn default_diagonal_size() -> f32 {
    45.0
}

fn is_false(value: &bool) -> bool {
    !value
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_parse_and_print() {
        for text in ["left_shift", "a", "numpad_enter", "mouse_left", "wheel_down", "0x66"] {
            let slot: Slot = text.parse().unwrap();
            assert_eq!(slot.to_string(), text);
        }
        assert_eq!("1".parse::<Slot>(), Ok(Slot::Key(KeyCode::DIGIT_1)));
        assert!("left_shfit".parse::<Slot>().is_err());
    }

    #[test]
    fn timing_slots_parse_in_both_spellings() {
        let ms = Duration::from_millis;
        let cases = [
            ("pause 0.15s", "PAUSE 0.15s", Slot::Pause(ms(150))),
            ("hold 0.5s", "HOLD ZONE 0.50s", Slot::HoldZone(ms(500))),
            ("release 0.01s", "RELEASE ZONE 0.01s", Slot::ReleaseZone(ms(10))),
            ("distance 50%", "DISTANCE ZONE 50%", Slot::DistanceZone(50.0)),
            ("tap_time 0.01s", "TAP TIME 0.01s", Slot::TapTime(ms(10))),
            ("cycle", "CYCLE STOP", Slot::CycleStop),
            ("pause 600s", "PAUSE 10m00.00s", Slot::Pause(ms(600_000))),
        ];
        for (ours, xpadders, slot) in cases {
            assert_eq!(ours.parse::<Slot>(), Ok(slot), "{ours}");
            assert_eq!(xpadders.parse::<Slot>(), Ok(slot), "{xpadders}");
            assert_eq!(slot.to_string(), ours);
        }
        assert_eq!("pause 150ms".parse::<Slot>(), Ok(Slot::Pause(ms(150))));
        assert_eq!("pause".parse::<Slot>(), Ok(Slot::Key(KeyCode::PAUSE)));
        assert!("pause soon".parse::<Slot>().is_err());
    }

    #[test]
    fn timing_slot_rules() {
        let check = |slots: &str, toggle: bool| {
            let slots = slots.split(',').map(|s| s.parse().unwrap()).collect();
            let mut layer = Layer::new("Main");
            layer
                .buttons
                .insert(Button::South, Assignment { slots, toggle, ..Assignment::default() });
            Profile { name: String::new(), layout: None, virtual_pad: None, layers: vec![layer] }
                .validate()
        };
        assert!(check("hold 0.5s, y, release 0.01s, x, release 0.25s", false).is_ok());
        assert!(check("a, cycle, hold 0.2s, b, hold 0.1s, c", false).is_err());
        assert!(check("a, distance 40%, b, distance 80%, c", false).is_ok());
        assert!(check("a, distance 80%, b, distance 40%, c", false).is_err());
        assert!(check("a, distance 50%, b, release 0.1s, c", false).is_err());
        assert!(check("a, pause 0.1s, b", true).is_err());
        assert!(check("a, pause 11m0s", false).is_err());
    }

    #[test]
    fn validation_reports_every_problem() {
        let mut layer = Layer::new("Main");
        layer.buttons.insert(
            Button::South,
            Assignment {
                turbo: Some(100.0),
                layer: Some(LayerSelect { target: "Missing".into(), mode: SelectMode::OneWay }),
                ..Assignment::default()
            },
        );
        layer.left_stick.dead_zone = 1.5;
        let profile = Profile {
            name: String::new(),
            layout: None,
            virtual_pad: None,
            layers: vec![layer.clone(), layer],
        };
        let errors = profile.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("same name")), "{errors:?}");
        assert!(errors.iter().any(|e| e.contains("turbo")), "{errors:?}");
        assert!(errors.iter().any(|e| e.contains("`Missing`")), "{errors:?}");
        assert!(errors.iter().any(|e| e.contains("dead_zone")), "{errors:?}");
    }

    #[test]
    fn every_mouse_slot_prints_as_it_parses() {
        for (name, slot) in MOUSE_SLOTS {
            assert_eq!(slot.to_string(), name);
            assert_eq!(name.to_ascii_uppercase().parse::<Slot>(), Ok(slot));
        }
    }

    fn one_layer(layer: Layer) -> Result<(), Vec<String>> {
        Profile { name: String::new(), layout: None, virtual_pad: None, layers: vec![layer] }
            .validate()
    }

    #[test]
    fn thresholds_and_dead_zones_have_working_minimums() {
        let mut layer = Layer::new("Main");
        layer.trigger_threshold = 0.03;
        assert!(one_layer(layer.clone()).unwrap_err()[0].contains("trigger_threshold"));
        layer.trigger_threshold = MIN_TRIGGER_THRESHOLD;
        assert!(one_layer(layer.clone()).is_ok());

        // A dead zone of 0 suits a mouse stick, but would press directions on noise.
        layer.left_stick.dead_zone = 0.0;
        layer.left_stick.mode = StickMode::Mouse;
        assert!(one_layer(layer.clone()).is_ok());
        layer.left_stick.mode = StickMode::Directions;
        let errors = one_layer(layer.clone()).unwrap_err();
        assert!(errors[0].contains("left_stick: dead_zone must be at least 0.05"), "{errors:?}");
        layer.left_stick.dead_zone = MIN_DIRECTION_DEAD_ZONE;
        assert!(one_layer(layer).is_ok());
    }

    #[test]
    fn mouse_speed_is_capped() {
        let mut layer = Layer::new("Main");
        layer.right_stick.mouse.speed = MAX_MOUSE_SPEED;
        assert!(one_layer(layer.clone()).is_ok());
        for speed in [MAX_MOUSE_SPEED + 1.0, f32::INFINITY, f32::NAN, -1.0] {
            layer.right_stick.mouse.speed = speed;
            let errors = one_layer(layer.clone()).unwrap_err();
            assert!(errors[0].contains("right_stick: mouse.speed"), "{speed}: {errors:?}");
        }
    }

    #[test]
    fn zero_length_taps_are_rejected() {
        let check = |slots: &str| {
            let slots = slots.split(',').map(|s| s.parse().unwrap()).collect();
            let mut layer = Layer::new("Main");
            layer.buttons.insert(Button::South, Assignment { slots, ..Assignment::default() });
            one_layer(layer)
        };
        assert!(
            check("tap_time 0s, a")
                .unwrap_err()
                .iter()
                .any(|e| e.contains("`tap_time 0s`: tap_time must be longer than 0 s"))
        );
        assert!(check("a, hold 0s, b").unwrap_err()[0].contains("hold zones must start after"));
        assert!(check("tap_time 1ms, a, hold 1ms, b").is_ok());
        // A release zone at 0 fires a normal tap on release, and a pause of 0 separates taps.
        assert!(check("release 0s, a, pause 0s, b").is_ok());
    }

    #[test]
    fn toggle_cannot_latch_a_while_held_layer() {
        let mut layer = Layer::new("Main");
        let select = |mode| Some(LayerSelect { target: "Main".into(), mode });
        let mut assignment = Assignment {
            toggle: true,
            layer: select(SelectMode::WhileHeld),
            ..Assignment::default()
        };
        layer.buttons.insert(Button::LeftShoulder, assignment.clone());
        let errors = one_layer(layer.clone()).unwrap_err();
        assert!(errors[0].contains("two_way"), "{errors:?}");
        assignment.layer = select(SelectMode::TwoWay);
        layer.buttons.insert(Button::LeftShoulder, assignment);
        assert!(one_layer(layer).is_ok());
    }

    #[test]
    fn messages_name_controls_as_profile_files_do() {
        let bad = Assignment { turbo: Some(0.0), ..Assignment::default() };
        let mut layer = Layer::new("Main");
        layer.buttons.insert(Button::DPadUp, bad.clone());
        layer.left_trigger = Some(bad.clone());
        layer.right_stick.directions.up_left = Some(bad);
        let errors = one_layer(layer).unwrap_err();
        let at: Vec<&str> = errors.iter().map(|e| e.split_once(": ").unwrap().0).collect();
        assert_eq!(
            at,
            [
                "layer `Main`, buttons.dpad_up",
                "layer `Main`, left_trigger",
                "layer `Main`, right_stick.directions.up_left"
            ]
        );
    }
}
