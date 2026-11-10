//! `.xpadderprofile` import.
//!
//! A profile is one flat namespace of `Set{n}{Control}{Property}` keys (plus a few set-level
//! settings). The importer first collects them per set and control, then converts each set to a
//! layer. `docs/xpadder-import.md` explains every conversion and assumption made here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

use zpadder_model::profile::{
    Assignment, DirectionSettings, Layer, LayerSelect, MAX_MOUSE_SPEED, MIN_DIRECTION_DEAD_ZONE,
    MIN_TRIGGER_THRESHOLD, SelectMode, Slot, Stick, StickMode,
};
use zpadder_model::{Button, Control, Profile};

use crate::buttons::ButtonMap;
use crate::keys::{self, MouseMove, Token};
use crate::text::{Document, KeyReader, flag, int, pair, shorten};
use crate::{FileKind, ImportError, Warnings};

/// Pixels per second of pointer speed per unit of Xpadder's `MouseSpeed`. The unit is
/// undocumented; this linear scale makes Xpadder's default of 32 equal ZPadder's default of
/// 1000 px/s at full deflection.
pub(crate) const PIXELS_PER_MOUSE_SPEED: f32 = 1000.0 / 32.0;
/// Xpadder's `MouseSpeed` when a file leaves it out.
const DEFAULT_MOUSE_SPEED: i64 = 32;
/// The highest `MouseSpeed` imported: ZPadder's top speed.
const MAX_XPADDER_MOUSE_SPEED: i64 = (MAX_MOUSE_SPEED / PIXELS_PER_MOUSE_SPEED) as i64;
/// Turbo rate used when a file turns turbo on without a `TurboRate`.
const DEFAULT_TURBO: f32 = 10.0;
/// Xpadder's `KeyTapTime` that matches ZPadder's 0.1 s taps.
const STANDARD_KEY_TAP_TIME: i64 = 10;
/// The highest dead zone or threshold an import produces, so the control still works.
const MAX_DEAD_ZONE: f32 = 0.95;
/// Xpadder has eight sets. Keys for higher sets are ignored, which also bounds the work a
/// crafted file can cause.
const MAX_SETS: u8 = 8;
/// The shortest time an imported hold zone or tap time can have: Xpadder's resolution, a
/// hundredth of a second. A zero-length one would press and release keys in the same instant.
const SHORTEST: Duration = Duration::from_millis(10);

/// A converted profile.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedProfile {
    /// The profile. It always passes [`Profile::validate`]. Its name is the file's `Name=` line,
    /// or empty when there is none (use the file stem then).
    pub profile: Profile,
    /// Everything that could not be carried over, as readable sentences.
    pub warnings: Vec<String>,
}

/// Imports a `.xpadderprofile` file. `buttons` says which ZPadder button each Xpadder button
/// number is: use the map from the controller file the profile was made with
/// ([`ImportedController::buttons`](crate::ImportedController::buttons)), or a preset such as
/// [`ButtonMap::xbox`].
pub fn import_profile(bytes: &[u8], buttons: &ButtonMap) -> Result<ImportedProfile, ImportError> {
    let doc = crate::parse(bytes)?;
    crate::expect_kind(&doc, FileKind::Profile)?;
    let mut warnings = Warnings::default();
    doc.notes(&mut warnings);
    let raw = RawProfile::read(&doc, &mut warnings);
    let mut profile = Converter { raw: &raw, buttons, warnings: &mut warnings }.profile();
    ensure_valid(&mut profile, &mut warnings);
    Ok(ImportedProfile { profile, warnings: warnings.into_vec() })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl Dir {
    /// Longer words first, since `Up` is a prefix of `UpRight`.
    const WORDS: [(&'static str, Dir); 8] = [
        ("UpRight", Dir::UpRight),
        ("UpLeft", Dir::UpLeft),
        ("DownRight", Dir::DownRight),
        ("DownLeft", Dir::DownLeft),
        ("Up", Dir::Up),
        ("Down", Dir::Down),
        ("Left", Dir::Left),
        ("Right", Dir::Right),
    ];

    fn is_diagonal(self) -> bool {
        matches!(self, Dir::UpLeft | Dir::UpRight | Dir::DownLeft | Dir::DownRight)
    }

    fn slot(self, directions: &mut DirectionSettings) -> &mut Option<Assignment> {
        match self {
            Dir::Up => &mut directions.up,
            Dir::Down => &mut directions.down,
            Dir::Left => &mut directions.left,
            Dir::Right => &mut directions.right,
            Dir::UpLeft => &mut directions.up_left,
            Dir::UpRight => &mut directions.up_right,
            Dir::DownLeft => &mut directions.down_left,
            Dir::DownRight => &mut directions.down_right,
        }
    }
}

impl fmt::Display for Dir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Dir::Up => "up",
            Dir::Down => "down",
            Dir::Left => "left",
            Dir::Right => "right",
            Dir::UpLeft => "up-left",
            Dir::UpRight => "up-right",
            Dir::DownLeft => "down-left",
            Dir::DownRight => "down-right",
        })
    }
}

/// A control as Xpadder numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Ctl {
    Button(u8),
    DPad(Dir),
    Stick(u8, Dir),
    TriggerLeft,
    TriggerRight,
}

impl fmt::Display for Ctl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ctl::Button(n) => write!(f, "button {n}"),
            Ctl::DPad(dir) => write!(f, "DPad {dir}"),
            Ctl::Stick(n, dir) => write!(f, "stick {n} {dir}"),
            Ctl::TriggerLeft => f.write_str("left trigger"),
            Ctl::TriggerRight => f.write_str("right trigger"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prop {
    Name,
    Slots,
    Toggle,
    Turbo,
    TurboRate,
    Rumble,
    RumbleStrengths,
    SetSelect,
    MouseSpeed,
    MouseMode,
}

const PROPS: [(&str, Prop); 10] = [
    ("Name", Prop::Name),
    ("Slots", Prop::Slots),
    ("Toggle", Prop::Toggle),
    ("Turbo", Prop::Turbo),
    ("TurboRate", Prop::TurboRate),
    ("Rumble", Prop::Rumble),
    ("RumbleStrengths", Prop::RumbleStrengths),
    ("SetSelect", Prop::SetSelect),
    ("MouseSpeed", Prop::MouseSpeed),
    ("MouseMode", Prop::MouseMode),
];

/// Every key a profile can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    /// `DataType` and `Version`.
    Ignored,
    Name,
    TapTime,
    SetName(u8),
    StickDeadZones(u8),
    TriggerDeadZones(u8),
    StickDiagonalSizes(u8),
    DPadMode(u8),
    StickMode(u8, u8),
    Control(u8, Ctl, Prop),
}

fn parse_key(key: &str) -> Option<Key> {
    let mut k = KeyReader::new(key);
    if let Some(key) = k.rest_is(&[
        ("DataType", Key::Ignored),
        ("Version", Key::Ignored),
        ("Name", Key::Name),
        ("KeyTapTime", Key::TapTime),
    ]) {
        return Some(key);
    }
    if !k.eat("Set") {
        return None;
    }
    let set = k.number()?;
    if let Some(key) = k.rest_is(&[
        ("Name", Key::SetName(set)),
        ("StickDeadZones", Key::StickDeadZones(set)),
        ("TriggerDeadZones", Key::TriggerDeadZones(set)),
        ("StickDiagonalSizes", Key::StickDiagonalSizes(set)),
        ("DPadMode", Key::DPadMode(set)),
    ]) {
        return Some(key);
    }
    let ctl = if k.eat("Button") {
        Ctl::Button(k.number()?)
    } else if k.eat("DPad") {
        Ctl::DPad(k.one_of(&Dir::WORDS)?)
    } else if k.eat("Stick") {
        let stick = k.number()?;
        if k.is("Mode") {
            return Some(Key::StickMode(set, stick));
        }
        Ctl::Stick(stick, k.one_of(&Dir::WORDS)?)
    } else if k.eat("Trigger") {
        k.one_of(&[("Left", Ctl::TriggerLeft), ("Right", Ctl::TriggerRight)])?
    } else {
        return None;
    };
    Some(Key::Control(set, ctl, k.rest_is(&PROPS)?))
}

impl Key {
    /// The set the key belongs to, if any.
    fn set(self) -> Option<u8> {
        match self {
            Key::Ignored | Key::Name | Key::TapTime => None,
            Key::SetName(set)
            | Key::StickDeadZones(set)
            | Key::TriggerDeadZones(set)
            | Key::StickDiagonalSizes(set)
            | Key::DPadMode(set)
            | Key::StickMode(set, _)
            | Key::Control(set, _, _) => Some(set),
        }
    }
}

/// One control's keys, as written.
#[derive(Debug, Default)]
struct RawAssignment {
    name: Option<String>,
    slots: Option<String>,
    toggle: bool,
    turbo: bool,
    turbo_rate: Option<String>,
    rumble: bool,
    rumble_strengths: Option<String>,
    set_select: Option<String>,
    mouse_speed: Option<String>,
    mouse_mode: Option<String>,
}

#[derive(Debug, Default)]
struct RawSet {
    name: Option<String>,
    stick_dead_zones: Option<String>,
    trigger_dead_zones: Option<String>,
    stick_diagonal_sizes: Option<String>,
    dpad_mode: Option<String>,
    stick_modes: BTreeMap<u8, String>,
    controls: BTreeMap<Ctl, RawAssignment>,
}

#[derive(Debug, Default)]
struct RawProfile {
    name: Option<String>,
    key_tap_time: Option<String>,
    sets: BTreeMap<u8, RawSet>,
}

impl RawProfile {
    fn read(doc: &Document, warnings: &mut Warnings) -> Self {
        let mut raw = RawProfile::default();
        let mut beyond = BTreeSet::new();
        for entry in doc.settings() {
            let Some(key) = parse_key(&entry.key) else {
                warnings.push(format!("`{}`: unknown setting; ignored", shorten(&entry.key)));
                continue;
            };
            if let Some(set) = key.set()
                && set > MAX_SETS
            {
                beyond.insert(set);
                continue;
            }
            let value = Some(entry.value.clone());
            match key {
                Key::Ignored => {}
                Key::Name => raw.name = value,
                Key::TapTime => raw.key_tap_time = value,
                Key::SetName(set) => raw.set(set).name = value,
                Key::StickDeadZones(set) => raw.set(set).stick_dead_zones = value,
                Key::TriggerDeadZones(set) => raw.set(set).trigger_dead_zones = value,
                Key::StickDiagonalSizes(set) => raw.set(set).stick_diagonal_sizes = value,
                Key::DPadMode(set) => raw.set(set).dpad_mode = value,
                Key::StickMode(set, stick) => {
                    raw.set(set).stick_modes.insert(stick, entry.value.clone());
                }
                Key::Control(set, ctl, prop) => {
                    let a = raw.set(set).controls.entry(ctl).or_default();
                    match prop {
                        Prop::Name => a.name = value,
                        Prop::Slots => a.slots = value,
                        Prop::Toggle => a.toggle = flag(&entry.value),
                        Prop::Turbo => a.turbo = flag(&entry.value),
                        Prop::TurboRate => a.turbo_rate = value,
                        Prop::Rumble => a.rumble = flag(&entry.value),
                        Prop::RumbleStrengths => a.rumble_strengths = value,
                        Prop::SetSelect => a.set_select = value,
                        Prop::MouseSpeed => a.mouse_speed = value,
                        Prop::MouseMode => a.mouse_mode = value,
                    }
                }
            }
        }
        if !beyond.is_empty() {
            let list: Vec<String> = beyond.iter().map(u8::to_string).collect();
            warnings.push(format!(
                "Xpadder has {MAX_SETS} sets; settings for higher sets ({}) are ignored",
                shorten(&list.join(", "))
            ));
        }
        raw
    }

    fn set(&mut self, set: u8) -> &mut RawSet {
        self.sets.entry(set).or_default()
    }
}

/// A set selector as written: the control in set `from` switches to set `target`.
#[derive(Debug, Clone, Copy)]
struct Selector {
    target: u8,
    toggle: bool,
}

/// Decides what each set selector becomes. `None` marks a return selector that a while-held
/// layer makes unnecessary.
///
/// Xpadder stores a while-held shift as two selectors on the same control: a forward one on the
/// source set and a mirrored one on the target set pointing back. Which of the pair is forward
/// is decided by distance from set 1 (the set a profile starts in), then by set number. While
/// the control is held, other while-held selectors can take the pad further (set 2, then set 3
/// with a second trigger); a selector on the held control in any of those sets that points back
/// to the source is a return as well. A selector with `Toggle=1` latches: two-way. Anything else
/// is one-way. This reading comes from two public profiles and is an assumption.
///
/// The work grows with the number of selectors times the square of the number of sets, which
/// [`MAX_SETS`] keeps small.
fn resolve_selectors(
    selectors: &BTreeMap<(u8, Ctl), Selector>,
) -> BTreeMap<(u8, Ctl), Option<SelectMode>> {
    // The sets each set's selectors lead to.
    let mut leads_to: BTreeMap<u8, BTreeSet<u8>> = BTreeMap::new();
    for (&(from, _), selector) in selectors {
        leads_to.entry(from).or_default().insert(selector.target);
    }
    // Breadth-first distance of each set from set 1 along any selector.
    let mut depth = BTreeMap::from([(1u8, 0usize)]);
    let mut queue = std::collections::VecDeque::from([1u8]);
    while let Some(set) = queue.pop_front() {
        let next = depth[&set] + 1;
        for &target in leads_to.get(&set).into_iter().flatten() {
            if let std::collections::btree_map::Entry::Vacant(entry) = depth.entry(target) {
                entry.insert(next);
                queue.push_back(target);
            }
        }
    }
    let rank = |set: u8| (depth.get(&set).copied().unwrap_or(usize::MAX), set);

    let forward: BTreeSet<(u8, Ctl)> = selectors
        .iter()
        .filter(|&(&(from, ctl), selector)| {
            !selector.toggle
                && selector.target != from
                && selectors.get(&(selector.target, ctl)).is_some_and(|mirror| {
                    !mirror.toggle && mirror.target == from && rank(from) < rank(selector.target)
                })
        })
        .map(|(&key, _)| key)
        .collect();

    // Forward selectors as moves between sets: from → to → the controls that make the move.
    let mut moves: BTreeMap<u8, BTreeMap<u8, Vec<Ctl>>> = BTreeMap::new();
    for &(from, ctl) in &forward {
        let target = selectors[&(from, ctl)].target;
        moves.entry(from).or_default().entry(target).or_default().push(ctl);
    }

    let mut returns = BTreeSet::new();
    for &(source, ctl) in &forward {
        // Every set reachable while `ctl` stays held: by forward selectors on other controls,
        // never back to the source.
        let mut reached = BTreeSet::from([selectors[&(source, ctl)].target]);
        let mut stack: Vec<u8> = reached.iter().copied().collect();
        while let Some(set) = stack.pop() {
            for (&target, controls) in moves.get(&set).into_iter().flatten() {
                let by_another = controls.iter().any(|&other| other != ctl);
                if by_another && target != source && reached.insert(target) {
                    stack.push(target);
                }
            }
        }
        for set in reached {
            if let Some(selector) = selectors.get(&(set, ctl))
                && !selector.toggle
                && selector.target == source
                && !forward.contains(&(set, ctl))
            {
                returns.insert((set, ctl));
            }
        }
    }

    selectors
        .iter()
        .map(|(&key, selector)| {
            let mode = if forward.contains(&key) {
                Some(SelectMode::WhileHeld)
            } else if returns.contains(&key) {
                None
            } else if selector.toggle {
                Some(SelectMode::TwoWay)
            } else {
                Some(SelectMode::OneWay)
            };
            (key, mode)
        })
        .collect()
}

/// An assignment converted on its own, before its control decides what to do with it.
#[derive(Debug, Default)]
struct Built {
    assignment: Assignment,
    /// `Mouse Move` tokens, which only sticks can use.
    moves: Vec<MouseMove>,
}

impl Built {
    /// Whether the assignment does anything.
    fn is_empty(&self) -> bool {
        let a = &self.assignment;
        a.slots.is_empty() && a.layer.is_none() && !a.rumble
    }
}

struct Converter<'a> {
    raw: &'a RawProfile,
    buttons: &'a ButtonMap,
    warnings: &'a mut Warnings,
}

impl Converter<'_> {
    fn warn(&mut self, message: String) {
        self.warnings.push(message);
    }

    fn profile(mut self) -> Profile {
        let raw = self.raw;
        if let Some(value) = &raw.key_tap_time
            && int(value) != Some(STANDARD_KEY_TAP_TIME)
        {
            self.warn(format!(
                "KeyTapTime={value} (Xpadder's standard key tap length, probably in hundredths \
                 of a second) is not imported; ZPadder taps keys for 0.1 s unless a slot list \
                 sets its own tap time"
            ));
        }

        // Set selectors, with targets converted from Xpadder's zero-based numbering.
        let mut selectors = BTreeMap::new();
        for (&set, raw_set) in &raw.sets {
            for (&ctl, a) in &raw_set.controls {
                let Some(value) = &a.set_select else { continue };
                let target = int(value)
                    .and_then(|v| v.checked_add(1))
                    .and_then(|v| u8::try_from(v).ok())
                    .filter(|t| (1..=MAX_SETS).contains(t));
                match target {
                    Some(target) => {
                        selectors.insert((set, ctl), Selector { target, toggle: a.toggle });
                    }
                    None => self.warn(format!(
                        "Set {set}, {ctl}: SetSelect={} is not a set number; ignored",
                        shorten(value)
                    )),
                }
            }
        }
        let modes = resolve_selectors(&selectors);

        // Set 1 is where Xpadder starts, so it is always the first layer. Other sets become
        // layers if they have assignments or a selector leads to them.
        let mut sets: BTreeSet<u8> = BTreeSet::from([1]);
        sets.extend(raw.sets.iter().filter(|(_, s)| !s.controls.is_empty()).map(|(&n, _)| n));
        sets.extend(selectors.values().map(|selector| selector.target));
        let names = layer_names(raw, &sets);

        let layers = sets
            .iter()
            .map(|&set| {
                let empty = RawSet::default();
                let raw_set = raw.sets.get(&set).unwrap_or(&empty);
                let select = |ctl: Ctl| {
                    let target = selectors.get(&(set, ctl))?.target;
                    let mode = modes.get(&(set, ctl)).copied().flatten()?;
                    Some(LayerSelect { target: names[&target].clone(), mode })
                };
                self.layer(set, &names[&set], raw_set, &select)
            })
            .collect();

        Profile {
            name: raw.name.as_deref().unwrap_or("").trim().to_string(),
            layout: None,
            virtual_pad: None,
            layers,
        }
    }

    fn layer(
        &mut self,
        set: u8,
        name: &str,
        raw: &RawSet,
        select: &dyn Fn(Ctl) -> Option<LayerSelect>,
    ) -> Layer {
        let mut layer = Layer::new(name);
        let mut sticks: BTreeMap<u8, BTreeMap<Dir, (Built, &RawAssignment)>> = BTreeMap::new();
        let mut dpad_diagonals = Vec::new();
        // Buttons that the button map says are triggers, applied after the triggers' own keys.
        let mut button_triggers = Vec::new();

        for (&ctl, raw_assignment) in &raw.controls {
            let at = format!("Set {set}, {ctl}");
            let built = self.build(&at, raw_assignment, select(ctl));
            if let Ctl::Stick(stick, dir) = ctl {
                sticks.entry(stick).or_default().insert(dir, (built, raw_assignment));
                continue;
            }
            if !built.moves.is_empty() {
                self.warn(format!(
                    "{at}: pointer movement only works on sticks in ZPadder; `Mouse Move` \
                     slots skipped"
                ));
            }
            if built.is_empty() {
                continue;
            }
            match ctl {
                Ctl::Button(number) => match self.buttons.get(number) {
                    Some(Control::Button(button)) => {
                        layer.buttons.insert(button, built.assignment);
                    }
                    Some(trigger) => button_triggers.push((at, trigger, built.assignment)),
                    None => self.warn(format!(
                        "{at}: no ZPadder button is mapped to Xpadder button {number}; \
                         assignment not imported"
                    )),
                },
                Ctl::DPad(dir) => {
                    let button = match dir {
                        Dir::Up => Button::DPadUp,
                        Dir::Down => Button::DPadDown,
                        Dir::Left => Button::DPadLeft,
                        Dir::Right => Button::DPadRight,
                        _ => {
                            dpad_diagonals.push(dir.to_string());
                            continue;
                        }
                    };
                    layer.buttons.insert(button, built.assignment);
                }
                Ctl::TriggerLeft => layer.left_trigger = Some(built.assignment),
                Ctl::TriggerRight => layer.right_trigger = Some(built.assignment),
                Ctl::Stick(..) => {} // collected above
            }
        }

        for (at, trigger, assignment) in button_triggers {
            let (slot, name) = match trigger {
                Control::LeftTrigger => (&mut layer.left_trigger, "left"),
                _ => (&mut layer.right_trigger, "right"),
            };
            if slot.is_some() {
                self.warn(format!(
                    "{at}: this button is the {name} trigger, which already has an \
                     assignment; not imported"
                ));
            } else {
                *slot = Some(assignment);
            }
        }

        let dpad_mode = raw.dpad_mode.as_deref().unwrap_or("").trim().to_ascii_lowercase();
        if !dpad_diagonals.is_empty() && dpad_mode != "fourway" {
            self.warn(format!(
                "Set {set}: ZPadder's DPad is four buttons, so the DPad diagonal assignments \
                 ({}) are not imported; pressing diagonally presses both neighbours",
                dpad_diagonals.join(", ")
            ));
        }
        if matches!(dpad_mode.as_str(), "horizontal" | "vertical") {
            self.warn(format!(
                "Set {set}: the DPad's {dpad_mode} priority mode has no ZPadder equivalent; \
                 diagonals press both neighbouring directions"
            ));
        }

        if let Some(value) = &raw.trigger_dead_zones {
            let has_triggers = layer.left_trigger.is_some() || layer.right_trigger.is_some();
            let zones = pair(value)
                .map(|zone| zone.map(|z| fraction(z).clamp(MIN_TRIGGER_THRESHOLD, MAX_DEAD_ZONE)));
            match zones {
                [Some(left), Some(right)] => {
                    if has_triggers && left != right {
                        self.warn(format!(
                            "Set {set}: the triggers have different dead zones ({value}); \
                             ZPadder has one threshold for both and uses the larger"
                        ));
                    }
                    layer.trigger_threshold = left.max(right);
                }
                [Some(zone), None] | [None, Some(zone)] => layer.trigger_threshold = zone,
                [None, None] => self.warn(format!(
                    "Set {set}: TriggerDeadZones={} is not two numbers; ignored",
                    shorten(value)
                )),
            }
        }

        for (stick, directions) in sticks {
            let side = match stick {
                1 => &mut layer.left_stick,
                2 => &mut layer.right_stick,
                _ => {
                    self.warn(format!("Set {set}: ZPadder has two sticks; stick {stick} ignored"));
                    continue;
                }
            };
            *side = self.stick(set, stick, raw, directions);
        }
        layer
    }

    /// Converts one control's keys into an assignment, warning about anything it drops.
    fn build(&mut self, at: &str, raw: &RawAssignment, select: Option<LayerSelect>) -> Built {
        let mut built = Built::default();
        let a = &mut built.assignment;
        a.name = raw.name.as_deref().unwrap_or("").trim().to_string();
        for token in raw.slots.as_deref().unwrap_or("").split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            match keys::parse_token(token) {
                Some(Token::Slot(slot)) => a.slots.push(clamp_slot(slot, at, self.warnings)),
                Some(Token::MouseMove(direction)) => built.moves.push(direction),
                Some(Token::Nothing) => {}
                None => self.warnings.push(format!("{at}: unknown slot `{token}`; skipped")),
            }
        }
        // A selector with Toggle=1 is a two-way selector; the flag belongs to it.
        a.toggle = raw.toggle && raw.set_select.is_none();
        a.turbo = raw.turbo.then(|| turbo_rate(raw.turbo_rate.as_deref()));
        a.rumble = raw.rumble;
        a.layer = select;
        if raw.rumble
            && let Some(strengths) = &raw.rumble_strengths
            && pair(strengths) != [Some(255), Some(255)]
        {
            self.warnings.push(format!(
                "{at}: rumble strengths ({strengths}) are not imported; ZPadder rumbles at its \
                 standard strength"
            ));
        }
        if a.toggle && a.slots.iter().any(Slot::is_timing) {
            a.toggle = false;
            self.warnings.push(format!(
                "{at}: toggle cannot be combined with timing slots in ZPadder; toggle dropped"
            ));
        }
        repair_slots(at, &mut a.slots, self.warnings);
        built
    }

    fn stick(
        &mut self,
        set: u8,
        stick: u8,
        raw: &RawSet,
        directions: BTreeMap<Dir, (Built, &RawAssignment)>,
    ) -> Stick {
        let at = format!("Set {set}, stick {stick}");
        let index = usize::from(stick - 1);
        let dead_zone = raw.stick_dead_zones.as_deref().and_then(|v| pair(v)[index]);
        let dead_zone = dead_zone.map(|z| fraction(z).min(MAX_DEAD_ZONE));
        let mut result = Stick::default();
        if let Some(zone) = dead_zone {
            result.dead_zone = zone;
        }

        if directions.values().any(|(built, _)| !built.moves.is_empty()) {
            result.mode = StickMode::Mouse;
            self.mouse_stick(&at, &mut result, &directions);
            return result;
        }

        let assigned: Vec<(Dir, Assignment)> = directions
            .into_iter()
            .filter(|(_, (built, _))| !built.is_empty())
            .map(|(dir, (built, _))| (dir, built.assignment))
            .collect();
        if assigned.is_empty() {
            return Stick::default();
        }
        result.mode = StickMode::Directions;
        if result.dead_zone < MIN_DIRECTION_DEAD_ZONE {
            self.warn(format!(
                "{at}: a dead zone of {:.0}% would press directions on a centred stick's noise; \
                 raised to {:.0}%",
                result.dead_zone * 100.0,
                MIN_DIRECTION_DEAD_ZONE * 100.0
            ));
            result.dead_zone = MIN_DIRECTION_DEAD_ZONE;
        }
        let settings = &mut result.directions;
        let has_diagonals = assigned.iter().any(|(dir, _)| dir.is_diagonal());
        let mode = raw.stick_modes.get(&stick).map(|m| m.trim().to_ascii_lowercase());
        settings.eight_way = match mode.as_deref() {
            Some("eightway") => true,
            Some("fourway") => false,
            // Without a mode, diagonal assignments show what the author meant.
            None => has_diagonals,
            Some(other) => {
                self.warn(format!(
                    "{at}: the `{other}` direction mode has no ZPadder equivalent; imported as \
                     four-way"
                ));
                false
            }
        };
        if let Some(size) = raw.stick_diagonal_sizes.as_deref().and_then(|v| pair(v)[index]) {
            settings.diagonal_size = size.clamp(0, 90) as f32;
        }
        for (dir, assignment) in assigned {
            *dir.slot(settings) = Some(assignment);
        }
        result
    }

    /// Fills in a stick whose directions move the pointer.
    fn mouse_stick(
        &mut self,
        at: &str,
        stick: &mut Stick,
        directions: &BTreeMap<Dir, (Built, &RawAssignment)>,
    ) {
        let (mut x, mut y) = (Axis::default(), Axis::default());
        let mut speeds = Vec::new();
        let mut modes = BTreeSet::new();
        let mut dropped = false;
        for (&dir, (built, raw)) in directions {
            if !built.moves.is_empty() {
                let speed = raw.mouse_speed.as_deref().and_then(int);
                let speed = speed.unwrap_or(DEFAULT_MOUSE_SPEED);
                let clamped = speed.clamp(0, MAX_XPADDER_MOUSE_SPEED);
                if clamped != speed {
                    self.warn(format!(
                        "{at}, {dir}: MouseSpeed={speed} is out of range; changed to {clamped}"
                    ));
                }
                speeds.push(clamped);
                if let Some(mode) = raw.mouse_mode.as_deref().map(str::trim)
                    && !mode.is_empty()
                    && !mode.eq_ignore_ascii_case("normal")
                {
                    modes.insert(mode.to_string());
                }
            }
            for &movement in &built.moves {
                match (dir, movement) {
                    (Dir::Up, MouseMove::Up) | (Dir::Down, MouseMove::Down) => y.normal = true,
                    (Dir::Up, MouseMove::Down) | (Dir::Down, MouseMove::Up) => y.inverted = true,
                    (Dir::Left, MouseMove::Left) | (Dir::Right, MouseMove::Right) => {
                        x.normal = true;
                    }
                    (Dir::Left, MouseMove::Right) | (Dir::Right, MouseMove::Left) => {
                        x.inverted = true;
                    }
                    _ => x.odd = true,
                }
            }
            let a = &built.assignment;
            dropped |= !a.slots.is_empty() || a.layer.is_some() || a.toggle || a.turbo.is_some();
        }

        stick.mouse.invert_x = x.inverted && !x.normal;
        stick.mouse.invert_y = y.inverted && !y.normal;
        if (x.normal && x.inverted) || (y.normal && y.inverted) {
            self.warn(format!(
                "{at}: both directions of one axis move the pointer the same way; imported as \
                 normal movement"
            ));
        }
        if x.odd {
            self.warn(format!(
                "{at}: diagonal or crossed pointer movement has no ZPadder equivalent; the stick \
                 moves the pointer in its own direction"
            ));
        }
        match (x.normal || x.inverted, y.normal || y.inverted) {
            (true, false) => self.warn(format!(
                "{at}: moves the pointer only horizontally in Xpadder; ZPadder's mouse stick \
                 moves it vertically too"
            )),
            (false, true) => self.warn(format!(
                "{at}: moves the pointer only vertically in Xpadder; ZPadder's mouse stick moves \
                 it horizontally too"
            )),
            _ => {}
        }

        let average = speeds.iter().sum::<i64>() as f32 / speeds.len().max(1) as f32;
        stick.mouse.speed = average * PIXELS_PER_MOUSE_SPEED;
        if speeds.iter().any(|&s| s != speeds[0]) {
            let list = speeds.iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
            self.warn(format!(
                "{at}: the directions have different mouse speeds ({list}); ZPadder has one \
                 speed per stick and uses their average"
            ));
        }
        if !modes.is_empty() {
            let list = modes.into_iter().collect::<Vec<_>>().join(", ");
            self.warn(format!(
                "{at}: Xpadder's {list} mouse mode has no ZPadder equivalent yet; imported as \
                 normal (relative) pointer movement"
            ));
        }
        if dropped {
            self.warn(format!(
                "{at}: the stick moves the pointer, so the other outputs, set selectors and \
                 turbo or toggle settings on its directions are not imported"
            ));
        }
    }
}

/// Which way one stick axis moves the pointer. `odd` collects diagonal or cross-axis moves.
#[derive(Debug, Default)]
struct Axis {
    normal: bool,
    inverted: bool,
    odd: bool,
}

/// Layer names: `Set{n}Name`, or `Set {n}`, made unique.
fn layer_names(raw: &RawProfile, sets: &BTreeSet<u8>) -> BTreeMap<u8, String> {
    let mut names = BTreeMap::new();
    let mut used = BTreeSet::new();
    for &set in sets {
        let given = raw.sets.get(&set).and_then(|s| s.name.as_deref()).map(str::trim);
        let base = match given {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => format!("Set {set}"),
        };
        let mut name = base.clone();
        let mut attempt = 1;
        while used.contains(&name) {
            name = if attempt == 1 {
                format!("{base} (Set {set})")
            } else {
                format!("{base} (Set {set}, {attempt})")
            };
            attempt += 1;
        }
        used.insert(name.clone());
        names.insert(set, name);
    }
    names
}

/// An Xpadder dead zone, 0–255, as a fraction of full deflection. Out-of-range values are
/// clamped first.
fn fraction(value: i64) -> f32 {
    value.clamp(0, 255) as f32 / 255.0
}

/// Makes sure the imported profile passes [`Profile::validate`]. The conversions above keep
/// every value within ZPadder's limits, so this does nothing unless one of them has a gap; then
/// whatever fails is dropped or reset, with a warning, rather than returning a profile that
/// cannot be used.
fn ensure_valid(profile: &mut Profile, warnings: &mut Warnings) {
    let Err(errors) = profile.validate() else { return };
    for error in errors {
        warnings.push(format!("{error}; dropped or reset to the default"));
    }
    let names: Vec<String> = profile.layers.iter().map(|layer| layer.name.clone()).collect();
    let valid = |assignment: &Assignment| {
        let mut layers: Vec<Layer> = names.iter().map(Layer::new).collect();
        layers[0].buttons.insert(Button::South, assignment.clone());
        Profile { name: String::new(), layout: None, virtual_pad: None, layers }.validate().is_ok()
    };
    for layer in &mut profile.layers {
        let settings = Layer {
            buttons: BTreeMap::new(),
            left_trigger: None,
            right_trigger: None,
            left_stick: without_assignments(&layer.left_stick),
            right_stick: without_assignments(&layer.right_stick),
            ..layer.clone()
        };
        let alone = Profile {
            name: String::new(),
            layout: None,
            virtual_pad: None,
            layers: vec![settings],
        };
        if alone.validate().is_err() {
            layer.trigger_threshold = Layer::new("").trigger_threshold;
            for stick in [&mut layer.left_stick, &mut layer.right_stick] {
                let directions = std::mem::take(&mut stick.directions);
                *stick = Stick { mode: stick.mode, ..Stick::default() };
                if stick.mode == StickMode::Directions {
                    stick.directions = DirectionSettings {
                        diagonal_size: DirectionSettings::default().diagonal_size,
                        ..directions
                    };
                }
            }
        }
        layer.buttons.retain(|_, assignment| valid(assignment));
        for trigger in [&mut layer.left_trigger, &mut layer.right_trigger] {
            if trigger.as_ref().is_some_and(|assignment| !valid(assignment)) {
                *trigger = None;
            }
        }
        for stick in [&mut layer.left_stick, &mut layer.right_stick] {
            for dir in Dir::WORDS.map(|(_, dir)| dir) {
                let slot = dir.slot(&mut stick.directions);
                if slot.as_ref().is_some_and(|assignment| !valid(assignment)) {
                    *slot = None;
                }
            }
        }
    }
    if profile.validate().is_err() {
        warnings.push(
            "the profile could not be converted into a valid ZPadder profile; only an empty \
             layer is imported"
                .to_string(),
        );
        profile.layers = vec![Layer::new("Set 1")];
    }
}

/// `stick`'s settings without its direction assignments.
fn without_assignments(stick: &Stick) -> Stick {
    let directions = DirectionSettings {
        eight_way: stick.directions.eight_way,
        diagonal_size: stick.directions.diagonal_size,
        ..DirectionSettings::default()
    };
    Stick { directions, ..stick.clone() }
}

/// Converts `TurboRate`, whose unit Xpadder never documented, to presses per second.
///
/// The reading used: the time between presses in hundredths of a second, so `TurboRate=25` is
/// four presses a second. It is the one reading under which shared profiles make sense: menu
/// sets repeat arrow keys at 20 (5 per second), a weapon-cycling set scrolls the wheel at 40
/// (2.5 per second), and an Alt+Tab assignment repeats at 25 next to a 0.25 s tap time. The
/// result is kept within ZPadder's 0.5–60 range.
fn turbo_rate(value: Option<&str>) -> f32 {
    match value.and_then(int) {
        Some(hundredths) if hundredths > 0 => (100.0 / hundredths as f32).clamp(0.5, 60.0),
        Some(_) => 60.0,
        None => DEFAULT_TURBO,
    }
}

/// Brings a timing slot within ZPadder's limits (which match Xpadder's).
fn clamp_slot(slot: Slot, at: &str, warnings: &mut Warnings) -> Slot {
    const LONGEST: Duration = Duration::from_secs(600);
    const LONGEST_TAP: Duration = Duration::from_secs(10);
    let clamped = match slot {
        Slot::Pause(d) if d > LONGEST => Slot::Pause(LONGEST),
        Slot::HoldZone(d) if d > LONGEST => Slot::HoldZone(LONGEST),
        Slot::HoldZone(d) if d.is_zero() => Slot::HoldZone(SHORTEST),
        Slot::ReleaseZone(d) if d > LONGEST => Slot::ReleaseZone(LONGEST),
        Slot::TapTime(d) if d > LONGEST_TAP => Slot::TapTime(LONGEST_TAP),
        Slot::TapTime(d) if d.is_zero() => Slot::TapTime(SHORTEST),
        Slot::DistanceZone(p) if !(0.0..=100.0).contains(&p) => {
            Slot::DistanceZone(if p.is_nan() { 0.0 } else { p.clamp(0.0, 100.0) })
        }
        other => other,
    };
    if clamped != slot {
        warnings.push(format!("{at}: `{slot}` is out of range; changed to `{clamped}`"));
    }
    clamped
}

/// Makes a slot list valid for ZPadder, or empties it, warning about any change.
///
/// Two patterns in real files break ZPadder's rules, and both have a likely meaning:
/// - a hold or release zone shorter than the one before it, as in `HOLD ZONE 0.30s, F2,
///   HOLD ZONE 0.10s`. The Xpadder tutorial says zones accumulate, so the later zone is read as
///   relative to the one before (`hold 0.4s`): F2 is tapped for 0.1 s after a 0.3 s hold;
/// - a hold or release zone with no outputs after it, in a list that also has distance zones.
///   It does nothing useful, so it is dropped.
fn repair_slots(at: &str, slots: &mut Vec<Slot>, warnings: &mut Warnings) {
    let problems = slot_problems(slots);
    if problems.is_empty() {
        return;
    }
    let relative = relative_zones(slots);
    let candidates = [
        (relative.clone(), "zones that go back in time were read as relative to the zone before"),
        (drop_empty_zones(slots), "zones with no outputs after them were dropped"),
        (drop_empty_zones(&relative), "zones were read as relative and empty ones dropped"),
    ];
    for (candidate, change) in candidates {
        if candidate != *slots && slot_problems(&candidate).is_empty() {
            warnings.push(format!("{at}: {change}: `{}`", shorten(&list(&candidate))));
            *slots = candidate;
            return;
        }
    }
    warnings.push(format!(
        "{at}: the slots `{}` cannot be represented in ZPadder ({}); not imported",
        shorten(&list(slots)),
        problems.join("; ")
    ));
    slots.clear();
}

fn list(slots: &[Slot]) -> String {
    slots.iter().map(Slot::to_string).collect::<Vec<_>>().join(", ")
}

/// Reads each hold or release zone that is not later than the previous zone of its kind (in
/// the same cycle alternative) as relative to that zone.
fn relative_zones(slots: &[Slot]) -> Vec<Slot> {
    const LONGEST: Duration = Duration::from_secs(600);
    let (mut hold, mut release): (Option<Duration>, Option<Duration>) = (None, None);
    let relative = |last: &mut Option<Duration>, time: Duration| {
        let time = match *last {
            Some(previous) if time <= previous => (previous + time).min(LONGEST),
            _ => time,
        };
        *last = Some(time);
        time
    };
    slots
        .iter()
        .map(|&slot| match slot {
            Slot::HoldZone(time) => Slot::HoldZone(relative(&mut hold, time)),
            Slot::ReleaseZone(time) => Slot::ReleaseZone(relative(&mut release, time)),
            Slot::CycleStop => {
                (hold, release) = (None, None);
                slot
            }
            _ => slot,
        })
        .collect()
}

/// Drops hold, release and distance zones that no output follows in their cycle alternative.
fn drop_empty_zones(slots: &[Slot]) -> Vec<Slot> {
    let mut kept = Vec::with_capacity(slots.len());
    for alternative in slots.split_inclusive(|slot| matches!(slot, Slot::CycleStop)) {
        let last_output = alternative.iter().rposition(|slot| !slot.is_timing());
        kept.extend(alternative.iter().enumerate().filter_map(|(i, &slot)| {
            let zone =
                matches!(slot, Slot::HoldZone(_) | Slot::ReleaseZone(_) | Slot::DistanceZone(_));
            (!zone || last_output.is_some_and(|last| i < last)).then_some(slot)
        }));
    }
    kept
}

/// The problems ZPadder's validation finds in a slot list, without the location prefix.
fn slot_problems(slots: &[Slot]) -> Vec<String> {
    let mut layer = Layer::new("check");
    let check = Assignment { slots: slots.to_vec(), ..Assignment::default() };
    layer.buttons.insert(Button::South, check);
    let profile =
        Profile { name: String::new(), layout: None, virtual_pad: None, layers: vec![layer] };
    match profile.validate() {
        Ok(()) => Vec::new(),
        Err(errors) => errors
            .into_iter()
            .map(|e| match e.split_once(": ") {
                Some((_, rest)) => rest.to_string(),
                None => e,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use zpadder_model::KeyCode;

    use super::*;

    fn import(text: &str) -> ImportedProfile {
        let imported = import_profile(text.as_bytes(), &ButtonMap::xbox()).unwrap();
        assert_eq!(imported.profile.validate(), Ok(()));
        imported
    }

    fn select(pairs: &[((u8, Ctl), u8, bool)]) -> BTreeMap<(u8, Ctl), Option<SelectMode>> {
        let selectors =
            pairs.iter().map(|&(key, target, toggle)| (key, Selector { target, toggle })).collect();
        resolve_selectors(&selectors)
    }

    const LT: Ctl = Ctl::TriggerLeft;
    const RT: Ctl = Ctl::TriggerRight;

    #[test]
    fn keys_parse() {
        let control = |set, ctl, prop| Some(Key::Control(set, ctl, prop));
        assert_eq!(parse_key("Set1Button12Slots"), control(1, Ctl::Button(12), Prop::Slots));
        assert_eq!(
            parse_key("Set8DPadUpRightTurboRate"),
            control(8, Ctl::DPad(Dir::UpRight), Prop::TurboRate)
        );
        assert_eq!(
            parse_key("Set2Stick2DownLeftMouseMode"),
            control(2, Ctl::Stick(2, Dir::DownLeft), Prop::MouseMode)
        );
        assert_eq!(parse_key("Set3TriggerRightSetSelect"), control(3, RT, Prop::SetSelect));
        assert_eq!(parse_key("Set1Stick2Mode"), Some(Key::StickMode(1, 2)));
        assert_eq!(parse_key("Set1DPadMode"), Some(Key::DPadMode(1)));
        assert_eq!(parse_key("Set4StickDeadZones"), Some(Key::StickDeadZones(4)));
        assert_eq!(parse_key("set1name"), Some(Key::SetName(1)));
        assert_eq!(parse_key("Name"), Some(Key::Name));
        for unknown in ["Set1Button1Colour", "Set0Button1Slots", "Set1Stick1Slots", "Foo", "Set"] {
            assert_eq!(parse_key(unknown), None, "{unknown}");
        }
    }

    #[test]
    fn while_held_pairs_become_one_while_held_selector() {
        // Guild Wars 2: "Set 2: left trigger (while held); Set 3: set 2 + right trigger".
        let modes = select(&[
            ((1, LT), 2, false),
            ((2, LT), 1, false),
            ((2, RT), 3, false),
            ((3, LT), 1, false),
            ((3, RT), 2, false),
        ]);
        assert_eq!(modes[&(1, LT)], Some(SelectMode::WhileHeld));
        assert_eq!(modes[&(2, LT)], None);
        assert_eq!(modes[&(2, RT)], Some(SelectMode::WhileHeld));
        assert_eq!(modes[&(3, RT)], None);
        // Releasing the left trigger in set 3 returns to set 1 in Xpadder: also a return.
        assert_eq!(modes[&(3, LT)], None);
    }

    #[test]
    fn toggled_and_unpaired_selectors() {
        // Call of Duty MW3: Toggle=1 on both sides of the pair latches.
        let modes = select(&[((1, Ctl::Button(5)), 2, true), ((2, Ctl::Button(5)), 1, true)]);
        assert!(modes.values().all(|&m| m == Some(SelectMode::TwoWay)));

        let modes = select(&[((1, Ctl::Button(1)), 3, false), ((3, Ctl::Button(2)), 1, false)]);
        assert!(modes.values().all(|&m| m == Some(SelectMode::OneWay)));

        // A pair between two sets that set 1 cannot reach: the lower set is the source.
        let modes = select(&[((5, LT), 4, false), ((4, LT), 5, false)]);
        assert_eq!(modes[&(4, LT)], Some(SelectMode::WhileHeld));
        assert_eq!(modes[&(5, LT)], None);

        // Pointing at its own set is harmless and stays as written.
        assert_eq!(select(&[((1, LT), 1, false)])[&(1, LT)], Some(SelectMode::OneWay));
    }

    #[test]
    fn set_selectors_in_a_profile() {
        let imported = import(
            "DataType=Profile\r\n[Set Settings]\r\nSet1Name=Main\r\nSet2Name=Menus\r\n\
             [Assignments]\r\nSet1TriggerLeftSetSelect=1\r\nSet2TriggerLeftSetSelect=0\r\n\
             Set2TriggerLeftSlots=Escape\r\nSet1Button8SetSelect=3\r\nSet1Button8Toggle=1\r\n\
             Set1Button7SetSelect=nope\r\n",
        );
        let layers = &imported.profile.layers;
        let names: Vec<&str> = layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Main", "Menus", "Set 4"]);
        let lt = layers[0].left_trigger.as_ref().unwrap();
        assert_eq!(
            lt.layer,
            Some(LayerSelect { target: "Menus".into(), mode: SelectMode::WhileHeld })
        );
        // The return selector is dropped, but the trigger's own output stays.
        let back = layers[1].left_trigger.as_ref().unwrap();
        assert_eq!((back.layer.as_ref(), back.slots.len()), (None, 1));
        let start = &layers[0].buttons[&Button::Start];
        assert_eq!(start.layer.as_ref().unwrap().mode, SelectMode::TwoWay);
        assert!(!start.toggle);
        assert!(imported.warnings.iter().any(|w| w.contains("SetSelect=nope")));
    }

    #[test]
    fn layer_names_are_unique() {
        let imported = import(
            "DataType=Profile\r\nSet1Name=Main\r\nSet2Name=Main\r\nSet3Name= \r\n\
             Set2Button1Slots=A\r\nSet3Button1Slots=B\r\nSet7Name=Unused\r\n",
        );
        let names: Vec<&str> = imported.profile.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Main", "Main (Set 2)", "Set 3"]);
    }

    #[test]
    fn assignments_and_flags() {
        let imported = import(
            "DataType=Profile\r\nName= Shooter \r\nKeyTapTime=11\r\n[Assignments]\r\n\
             Set1Button1Name=Jump\r\nSet1Button1Slots=Space,Enter\r\n\
             Set1Button2Slots=RELEASE ZONE 0.01s,C,RELEASE ZONE 0.30s,HOLD ZONE 0.30s,Left Control\r\n\
             Set1Button3Slots=X\r\nSet1Button3Turbo=1\r\nSet1Button3TurboRate=25\r\n\
             Set1Button4Slots=Y\r\nSet1Button4Turbo=1\r\nSet1Button4Rumble=1\r\n\
             Set1Button4RumbleStrengths=51,0\r\n\
             Set1Button5Slots=RELEASE ZONE 0.01s,Q,RELEASE ZONE 0.30s\r\nSet1Button5Toggle=1\r\n\
             Set1Button6Slots=Q,Frobnicate,,NONE\r\nSet1Button6TurboRate=2\r\n\
             Set1Button9Slots=Mouse Move Up,W\r\nSet1Button20Slots=Z\r\n\
             Set1Button10Slots=HOLD ZONE 0.30s,F2,HOLD ZONE 0.10s\r\n\
             Set1Button7Slots=DISTANCE ZONE 50%,A,HOLD ZONE 0.20s,B\r\n\
             Set1Button7Name=Only a name survives\r\n",
        );
        let profile = &imported.profile;
        assert_eq!(profile.name, "Shooter");
        let buttons = &profile.layers[0].buttons;
        let south = &buttons[&Button::South];
        assert_eq!(south.name, "Jump");
        assert_eq!(south.slots, [Slot::Key(KeyCode::SPACE), Slot::Key(KeyCode::ENTER)]);
        assert_eq!(buttons[&Button::East].slots.len(), 5);
        assert_eq!(buttons[&Button::West].turbo, Some(4.0));
        let north = &buttons[&Button::North];
        assert_eq!((north.turbo, north.rumble), (Some(DEFAULT_TURBO), true));
        assert!(!buttons[&Button::LeftShoulder].toggle);
        let rb = &buttons[&Button::RightShoulder];
        assert_eq!((rb.slots.len(), rb.turbo), (1, None));
        assert_eq!(buttons[&Button::LeftStick].slots, [Slot::Key(KeyCode::W)]);
        let ms = Duration::from_millis;
        assert_eq!(
            buttons[&Button::RightStick].slots,
            [Slot::HoldZone(ms(300)), Slot::Key(KeyCode::F2), Slot::HoldZone(ms(400))]
        );
        assert!(!buttons.contains_key(&Button::Back));

        let warnings = imported.warnings.join("\n");
        for expected in [
            "KeyTapTime=11",
            "rumble strengths (51,0)",
            "Set 1, button 5: toggle cannot be combined",
            "unknown slot `Frobnicate`",
            "Set 1, button 9: pointer movement only works on sticks",
            "Xpadder button 20",
            "Set 1, button 10: zones that go back in time were read as relative",
            "Set 1, button 7: the slots `distance 50%, a, hold 0.2s, b` cannot be represented",
        ] {
            assert!(warnings.contains(expected), "missing `{expected}` in\n{warnings}");
        }
        assert_eq!(imported.warnings.len(), 8, "{warnings}");
    }

    #[test]
    fn mouse_sticks() {
        let imported = import(
            "DataType=Profile\r\nSet1StickDeadZones=64,39\r\n\
             Set1Stick2UpSlots=Mouse Move Down\r\nSet1Stick2DownSlots=Mouse Move Up\r\n\
             Set1Stick2LeftSlots=Mouse Move Left\r\nSet1Stick2RightSlots=Mouse Move Right\r\n\
             Set1Stick2UpMouseSpeed=64\r\nSet1Stick2DownMouseSpeed=64\r\n\
             Set1Stick2LeftMouseSpeed=64\r\nSet1Stick2RightMouseSpeed=64\r\n\
             Set2Stick1LeftSlots=Mouse Move Left\r\nSet2Stick1RightSlots=Mouse Move Right,A\r\n\
             Set2Stick1LeftMouseSpeed=40\r\nSet2Stick1RightMouseSpeed=60\r\n\
             Set2Stick1UpSlots=W\r\nSet2Stick1LeftMouseMode=Cursor\r\n",
        );
        let layers = &imported.profile.layers;
        let right = &layers[0].right_stick;
        assert_eq!(right.mode, StickMode::Mouse);
        assert_eq!(right.mouse.speed, 2000.0);
        assert!(right.mouse.invert_y && !right.mouse.invert_x);
        assert_eq!(right.dead_zone, 39.0 / 255.0);
        // The left stick is not used in set 1, so it keeps the defaults.
        assert_eq!(layers[0].left_stick, Stick::default());

        let left = &layers[1].left_stick;
        assert_eq!(left.mode, StickMode::Mouse);
        assert_eq!(left.mouse.speed, 50.0 * PIXELS_PER_MOUSE_SPEED);
        assert_eq!(left.dead_zone, Stick::default().dead_zone);
        let warnings = imported.warnings.join("\n");
        for expected in [
            "Set 2, stick 1: moves the pointer only horizontally",
            "different mouse speeds (40, 60)",
            "Cursor mouse mode",
            "other outputs",
        ] {
            assert!(warnings.contains(expected), "missing `{expected}` in\n{warnings}");
        }
        assert_eq!(imported.warnings.len(), 4, "{warnings}");
    }

    #[test]
    fn direction_sticks_triggers_and_dpad() {
        let imported = import(
            "DataType=Profile\r\n\
             Set1StickDeadZones=0,255\r\nSet1StickDiagonalSizes=67,120\r\nSet1TriggerDeadZones=28,65\r\n\
             Set1Stick1Mode=EightWay\r\nSet1Stick2Mode=Horizontal\r\nSet1DPadMode=EightWay\r\n\
             Set1Stick1UpSlots=W\r\nSet1Stick1UpRightSlots=W,D\r\nSet1Stick1DownSlots=S\r\n\
             Set1Stick2UpSlots=Arrow Up\r\nSet1Stick3UpSlots=Q\r\n\
             Set1TriggerLeftSlots=Right Mouse Button (2)\r\n\
             Set1TriggerRightSlots=Left Mouse Button (1),DISTANCE ZONE 50%,F\r\n\
             Set1DPadUpSlots=1\r\nSet1DPadUpLeftSlots=2\r\n\
             Set2Stick1UpSlots=W\r\nSet2Stick1UpLeftSlots=Q\r\nSet2TriggerDeadZones=1,1\r\n",
        );
        let [one, two] = &imported.profile.layers[..] else { panic!("two layers") };
        let left = &one.left_stick;
        assert_eq!(left.mode, StickMode::Directions);
        // A dead zone of 0 would press directions on noise.
        assert_eq!(left.dead_zone, MIN_DIRECTION_DEAD_ZONE);
        assert!(left.directions.eight_way);
        assert_eq!(left.directions.diagonal_size, 67.0);
        assert!(left.directions.up_right.is_some() && left.directions.left.is_none());
        let right = &one.right_stick;
        assert_eq!((right.dead_zone, right.directions.diagonal_size), (MAX_DEAD_ZONE, 90.0));
        assert!(!right.directions.eight_way);
        assert_eq!(one.trigger_threshold, 65.0 / 255.0);
        assert!(one.left_trigger.is_some());
        assert_eq!(one.right_trigger.as_ref().unwrap().slots.len(), 3);
        assert_eq!(one.buttons[&Button::DPadUp].slots, [Slot::Key(KeyCode::DIGIT_1)]);
        // No mode, but diagonal assignments: eight-way.
        assert!(two.left_stick.directions.eight_way);
        assert_eq!(two.trigger_threshold, MIN_TRIGGER_THRESHOLD);

        let warnings = imported.warnings.join("\n");
        for expected in [
            "`horizontal` direction mode",
            "stick 3 ignored",
            "different dead zones (28,65)",
            "DPad diagonal assignments (up-left)",
            "Set 1, stick 1: a dead zone of 0% would press directions",
        ] {
            assert!(warnings.contains(expected), "missing `{expected}` in\n{warnings}");
        }
        assert_eq!(imported.warnings.len(), 5, "{warnings}");
    }

    fn slots(text: &str) -> Vec<Slot> {
        text.split(',')
            .map(|token| match keys::parse_token(token) {
                Some(Token::Slot(slot)) => slot,
                other => panic!("`{token}` is {other:?}"),
            })
            .collect()
    }

    #[test]
    fn slot_repairs() {
        let repaired = |text: &str| {
            let mut list = slots(text);
            let mut warnings = Warnings::default();
            repair_slots("x", &mut list, &mut warnings);
            (list, warnings.len())
        };
        // Valid lists are left alone.
        let fine = "HOLD ZONE 0.30s,C,RELEASE ZONE 0.01s,6,RELEASE ZONE 0.29s";
        assert_eq!(repaired(fine), (slots(fine), 0));
        // A zone that goes back in time is relative to the one before.
        assert_eq!(
            repaired(
                "RELEASE ZONE 0.01s,Esc,RELEASE ZONE 2.00s,HOLD ZONE 1.50s,F4,HOLD ZONE 0.20s"
            ),
            (
                slots(
                    "RELEASE ZONE 0.01s,Esc,RELEASE ZONE 2.00s,HOLD ZONE 1.50s,F4,HOLD ZONE 1.70s"
                ),
                1
            )
        );
        // Each cycle alternative starts afresh.
        let cycles = "HOLD ZONE 0.50s,A,CYCLE STOP,HOLD ZONE 0.20s,B";
        assert_eq!(repaired(cycles), (slots(cycles), 0));
        // An empty zone mixed with distance zones is dropped.
        assert_eq!(
            repaired("W,DISTANCE ZONE 70%,Left Shift,W,HOLD ZONE 0.30s"),
            (slots("W,DISTANCE ZONE 70%,Left Shift,W"), 1)
        );
        // Anything else is not imported.
        assert_eq!(repaired("DISTANCE ZONE 50%,A,HOLD ZONE 0.20s,B"), (vec![], 1));
    }

    #[test]
    fn buttons_that_are_triggers() {
        let text = "DataType=Profile\r\nSet1Button7Slots=Q\r\nSet1Button8Slots=E\r\n\
                    Set1TriggerRightSlots=Left Mouse Button (1)\r\nSet1Button13Slots=Escape\r\n";
        let imported = import_profile(text.as_bytes(), &ButtonMap::dualshock4()).unwrap();
        let layer = &imported.profile.layers[0];
        assert_eq!(layer.left_trigger.as_ref().unwrap().slots, [Slot::Key(KeyCode::Q)]);
        assert_eq!(layer.right_trigger.as_ref().unwrap().slots.len(), 1);
        assert_eq!(layer.buttons[&Button::Guide].slots, [Slot::Key(KeyCode::ESCAPE)]);
        assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
        assert!(imported.warnings[0].contains("button 8: this button is the right trigger"));
    }

    #[test]
    fn turbo_conversion() {
        assert_eq!(turbo_rate(Some("25")), 4.0);
        assert_eq!(turbo_rate(Some("2")), 50.0);
        assert_eq!(turbo_rate(Some("1")), 60.0);
        assert_eq!(turbo_rate(Some("0")), 60.0);
        assert_eq!(turbo_rate(Some("1000")), 0.5);
        assert_eq!(turbo_rate(Some("fast")), DEFAULT_TURBO);
        assert_eq!(turbo_rate(None), DEFAULT_TURBO);
    }

    #[test]
    fn out_of_range_slots_are_clamped() {
        let mut warnings = Warnings::default();
        let long = Slot::Pause(Duration::from_secs(700));
        assert_eq!(clamp_slot(long, "x", &mut warnings), Slot::Pause(Duration::from_secs(600)));
        let tap = Slot::TapTime(Duration::from_secs(11));
        assert_eq!(clamp_slot(tap, "x", &mut warnings), Slot::TapTime(Duration::from_secs(10)));
        assert_eq!(
            clamp_slot(Slot::DistanceZone(150.0), "x", &mut warnings),
            Slot::DistanceZone(100.0)
        );
        // Zero-length hold zones and taps would press and release in the same instant.
        let ms = Duration::from_millis;
        assert_eq!(clamp_slot(Slot::HoldZone(ms(0)), "x", &mut warnings), Slot::HoldZone(ms(10)));
        assert_eq!(clamp_slot(Slot::TapTime(ms(0)), "x", &mut warnings), Slot::TapTime(ms(10)));
        assert_eq!(
            clamp_slot(Slot::ReleaseZone(ms(0)), "x", &mut warnings),
            Slot::ReleaseZone(ms(0))
        );
        assert_eq!(warnings.len(), 5);
    }

    #[test]
    fn numeric_extremes_are_clamped_not_panicked_on() {
        let max = i64::MAX;
        let min = i64::MIN;
        let text = format!(
            "DataType=Profile\r\nKeyTapTime={max}\r\n\
             Set1StickDeadZones={max},{min}\r\nSet1TriggerDeadZones={min},{max}\r\n\
             Set1StickDiagonalSizes={max},{min}\r\n\
             Set1Button1SetSelect={max}\r\nSet1Button2SetSelect={min}\r\n\
             Set1Button3SetSelect=-1\r\nSet1Button4SetSelect=8\r\nSet1Button5SetSelect=7\r\n\
             Set1Button6Slots=X\r\nSet1Button6Turbo=1\r\nSet1Button6TurboRate={max}\r\n\
             Set1Button7Slots=X\r\nSet1Button7Turbo=1\r\nSet1Button7TurboRate={min}\r\n\
             Set1Button8Slots=PAUSE 99999999999999999999m00.00s,A,HOLD ZONE 1e300s,B,\
             TAP TIME 0.00s,C,DISTANCE ZONE NaN%,D\r\n\
             Set1Button9Slots=HOLD ZONE 0.00s,A,HOLD ZONE 0.00s,B\r\n\
             Set1Stick1UpSlots=W\r\nSet1Stick1DownSlots=S\r\n\
             Set1Stick2UpSlots=Mouse Move Up\r\nSet1Stick2DownSlots=Mouse Move Down\r\n\
             Set1Stick2UpMouseSpeed={max}\r\nSet1Stick2DownMouseSpeed={max}\r\n\
             Set2Stick2UpSlots=Mouse Move Up\r\nSet2Stick2UpMouseSpeed={min}\r\n\
             Set9Button1Slots=A\r\nSet255Name=Far\r\nSet256Button1Slots=A\r\n"
        );
        let imported = import(&text);
        let layers = &imported.profile.layers;
        let names: Vec<&str> = layers.iter().map(|l| l.name.as_str()).collect();
        // Set 8 (SetSelect=7) is kept; SetSelect=8 would be a ninth set.
        assert_eq!(names, ["Set 1", "Set 2", "Set 8"]);
        let one = &layers[0];
        assert_eq!(one.left_stick.dead_zone, MAX_DEAD_ZONE);
        assert_eq!(one.right_stick.dead_zone, 0.0);
        assert_eq!(one.left_stick.directions.diagonal_size, 90.0);
        assert_eq!(one.right_stick.mouse.speed, MAX_MOUSE_SPEED);
        assert_eq!(layers[1].right_stick.mouse.speed, 0.0);
        assert_eq!(one.trigger_threshold, MAX_DEAD_ZONE);
        assert_eq!(one.buttons[&Button::RightShoulder].turbo, Some(0.5));
        assert_eq!(one.buttons[&Button::Back].turbo, Some(60.0));
        let warnings = imported.warnings.join("\n");
        for expected in [
            "SetSelect=9223372036854775807 is not a set number",
            "SetSelect=-9223372036854775808 is not a set number",
            "SetSelect=8 is not a set number",
            "MouseSpeed=9223372036854775807 is out of range; changed to 640",
            "MouseSpeed=-9223372036854775808 is out of range; changed to 0",
            "`tap_time 0s` is out of range; changed to `tap_time 0.01s`",
            "Xpadder has 8 sets; settings for higher sets (9, 255) are ignored",
            "`Set256Button1Slots`: unknown setting",
        ] {
            assert!(warnings.contains(expected), "missing `{expected}` in\n{warnings}");
        }
    }

    #[test]
    fn thousands_of_selectors_import_quickly() {
        // Every control of every set selects a neighbouring set and is selected back, so
        // nearly every selector is half of a while-held pair: about 18 000 selectors.
        let mut text = String::from("DataType=Profile\r\n");
        let mut controls: Vec<String> = (1..=255).map(|n| format!("Button{n}")).collect();
        for (word, _) in Dir::WORDS {
            controls.push(format!("DPad{word}"));
            controls.extend((1..=255).map(|n| format!("Stick{n}{word}")));
        }
        controls.extend(["TriggerLeft".to_string(), "TriggerRight".to_string()]);
        for set in 1..=MAX_SETS {
            for (c, control) in controls.iter().enumerate() {
                let partner = if (usize::from(set) + c) % 2 == 0 { set + 1 } else { set - 1 };
                let partner = partner.clamp(1, MAX_SETS);
                // SetSelect counts from zero.
                text.push_str(&format!("Set{set}{control}SetSelect={}\r\n", partner - 1));
            }
        }
        let started = std::time::Instant::now();
        let imported = import(&text);
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
        assert_eq!(imported.profile.layers.len(), usize::from(MAX_SETS));
        assert_eq!(imported.warnings.len(), crate::MAX_WARNINGS + 1);
    }

    #[test]
    fn invalid_conversions_are_repaired_or_dropped() {
        let mut layer = Layer::new("Set 1");
        layer.trigger_threshold = f32::NAN;
        layer.left_stick.mode = StickMode::Directions;
        layer.left_stick.dead_zone = 0.0;
        layer.left_stick.directions.up = Some(Assignment::default());
        layer.right_stick.mouse.speed = f32::INFINITY;
        let bad = Assignment { turbo: Some(1000.0), ..Assignment::default() };
        layer.buttons.insert(Button::South, bad.clone());
        layer.buttons.insert(Button::East, Assignment::default());
        layer.left_stick.directions.down = Some(bad.clone());
        layer.right_trigger = Some(bad);
        let mut profile =
            Profile { name: "P".into(), layout: None, virtual_pad: None, layers: vec![layer] };
        let mut warnings = Warnings::default();
        ensure_valid(&mut profile, &mut warnings);
        assert_eq!(profile.validate(), Ok(()));
        assert_eq!(warnings.len(), 6);
        let layer = &profile.layers[0];
        assert_eq!(layer.buttons.keys().collect::<Vec<_>>(), [&Button::East]);
        assert!(layer.right_trigger.is_none() && layer.left_stick.directions.down.is_none());
        // The stick keeps its mode and valid assignments, with default settings.
        assert_eq!(layer.left_stick.mode, StickMode::Directions);
        assert!(layer.left_stick.directions.up.is_some());
        assert_eq!(layer.right_stick.mouse.speed, Stick::default().mouse.speed);

        // What cannot be repaired piece by piece falls back to an empty layer.
        let mut profile = Profile {
            name: "P".into(),
            layout: None,
            virtual_pad: None,
            layers: vec![Layer::new("Same"), Layer::new("Same")],
        };
        let mut warnings = Warnings::default();
        ensure_valid(&mut profile, &mut warnings);
        assert_eq!(profile.validate(), Ok(()));
        assert_eq!(profile.layers, [Layer::new("Set 1")]);
        assert_eq!(profile.name, "P");
    }

    #[test]
    fn empty_and_minimal_profiles() {
        // RocketLauncher's blank profile: sections only.
        let imported = import(
            ";--- Xpadder Profile File ---\r\nDataType=Profile\r\nVersion=2012.12.31\r\n\r\n\
             [Profile Settings]\r\n\r\n[Set Settings]\r\n\r\n[Assignments]\r\n",
        );
        assert_eq!(imported.profile.layers.len(), 1);
        assert_eq!(imported.profile.layers[0], Layer::new("Set 1"));
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    }
}
