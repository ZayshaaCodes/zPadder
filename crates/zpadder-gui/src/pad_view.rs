//! Draws the controller and lets the user pick a control by clicking it. Controls light up
//! when pressed on the real pad, assigned controls are outlined, and labels follow the connected
//! controller's family (Xbox letters, PlayStation shapes, Nintendo letters).

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui,
    Vec2, pos2, vec2,
};
use zpadder_model::profile::{Direction, Layer, Stick, StickMode};
use zpadder_model::{Button, Control};
use zpadder_runtime::PadStatus;

use crate::labels::{self, Chip, Tone};
use crate::picture::Picture;
use crate::theme;

/// Design-space size; everything below is laid out in these units.
const WIDTH: f32 = 1000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
}

/// A selectable part of the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Button(Button),
    Trigger(Side),
    Stick(Side),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    Xbox,
    PlayStation,
    Nintendo,
}

impl Family {
    /// From SDL's controller type string.
    pub(crate) fn from_kind(kind: &str) -> Self {
        if kind.starts_with("ps") {
            Family::PlayStation
        } else if kind.contains("switch") || kind.contains("joycon") {
            Family::Nintendo
        } else {
            Family::Xbox
        }
    }
}

/// What a part is called on this family's controllers.
pub(crate) fn label(family: Family, part: Part) -> String {
    use Family::{Nintendo, PlayStation, Xbox};
    let text = match (part, family) {
        (Part::Button(Button::South), Xbox) => "A",
        (Part::Button(Button::East), Xbox) => "B",
        (Part::Button(Button::West), Xbox) => "X",
        (Part::Button(Button::North), Xbox) => "Y",
        (Part::Button(Button::South), Nintendo) => "B",
        (Part::Button(Button::East), Nintendo) => "A",
        (Part::Button(Button::West), Nintendo) => "Y",
        (Part::Button(Button::North), Nintendo) => "X",
        (Part::Button(Button::South), PlayStation) => "Cross",
        (Part::Button(Button::East), PlayStation) => "Circle",
        (Part::Button(Button::West), PlayStation) => "Square",
        (Part::Button(Button::North), PlayStation) => "Triangle",
        (Part::Button(Button::LeftShoulder), Xbox) => "LB",
        (Part::Button(Button::RightShoulder), Xbox) => "RB",
        (Part::Button(Button::LeftShoulder), PlayStation) => "L1",
        (Part::Button(Button::RightShoulder), PlayStation) => "R1",
        (Part::Button(Button::LeftShoulder), Nintendo) => "L",
        (Part::Button(Button::RightShoulder), Nintendo) => "R",
        (Part::Trigger(Side::Left), Xbox) => "LT",
        (Part::Trigger(Side::Right), Xbox) => "RT",
        (Part::Trigger(Side::Left), PlayStation) => "L2",
        (Part::Trigger(Side::Right), PlayStation) => "R2",
        (Part::Trigger(Side::Left), Nintendo) => "ZL",
        (Part::Trigger(Side::Right), Nintendo) => "ZR",
        (Part::Button(Button::Back), Xbox) => "View",
        (Part::Button(Button::Start), Xbox) => "Menu",
        (Part::Button(Button::Back), PlayStation) => "Share",
        (Part::Button(Button::Start), PlayStation) => "Options",
        (Part::Button(Button::Back), Nintendo) => "Minus",
        (Part::Button(Button::Start), Nintendo) => "Plus",
        (Part::Button(Button::Guide), Xbox) => "Guide",
        (Part::Button(Button::Guide), PlayStation) => "PS",
        (Part::Button(Button::Guide), Nintendo) => "Home",
        (Part::Button(Button::Misc1), Nintendo) => "Capture",
        (Part::Button(Button::Misc1), PlayStation) => "Mic",
        (Part::Button(Button::Misc1), Xbox) => "Share",
        (Part::Button(Button::LeftStick), _) => "Left stick click",
        (Part::Button(Button::RightStick), _) => "Right stick click",
        (Part::Stick(Side::Left), _) => "Left stick",
        (Part::Stick(Side::Right), _) => "Right stick",
        (Part::Button(Button::DPadUp), _) => "D-pad up",
        (Part::Button(Button::DPadDown), _) => "D-pad down",
        (Part::Button(Button::DPadLeft), _) => "D-pad left",
        (Part::Button(Button::DPadRight), _) => "D-pad right",
        (Part::Button(Button::Touchpad), _) => "Touchpad click",
        (Part::Button(Button::LeftPaddle1), _) => "Left paddle 1",
        (Part::Button(Button::LeftPaddle2), _) => "Left paddle 2",
        (Part::Button(Button::RightPaddle1), _) => "Right paddle 1",
        (Part::Button(Button::RightPaddle2), _) => "Right paddle 2",
    };
    text.to_string()
}

/// Short text drawn on the part itself.
pub(crate) fn cap(family: Family, part: Part) -> String {
    match part {
        Part::Button(Button::DPadUp | Button::DPadDown | Button::DPadLeft | Button::DPadRight) => {
            String::new()
        }
        Part::Button(Button::South | Button::East | Button::West | Button::North)
            if family == Family::PlayStation =>
        {
            String::new() // drawn as shapes
        }
        Part::Button(Button::LeftStick) | Part::Stick(Side::Left) => "L".into(),
        Part::Button(Button::RightStick) | Part::Stick(Side::Right) => "R".into(),
        Part::Button(Button::Touchpad) => "Touchpad".into(),
        Part::Button(Button::LeftPaddle1) => "P1".into(),
        Part::Button(Button::LeftPaddle2) => "P2".into(),
        Part::Button(Button::RightPaddle1) => "P3".into(),
        Part::Button(Button::RightPaddle2) => "P4".into(),
        _ => label(family, part),
    }
}

/// The shape a control takes: a circle (centre, radius) or a rounded rectangle (rect, corner
/// radius), in design units or picture pixels.
#[derive(Clone, Copy)]
pub(crate) enum Area {
    Circle(Pos2, f32),
    Rect(Rect, f32),
}

impl Area {
    pub(crate) fn contains(self, p: Pos2) -> bool {
        match self {
            Area::Circle(c, r) => c.distance(p) <= r,
            Area::Rect(rect, _) => rect.contains(p),
        }
    }

    /// The middle of the control.
    pub(crate) fn centre(self) -> Pos2 {
        match self {
            Area::Circle(c, _) => c,
            Area::Rect(r, _) => r.center(),
        }
    }

    /// The smallest rectangle around the control.
    pub(crate) fn bounds(self) -> Rect {
        match self {
            Area::Circle(c, r) => Rect::from_center_size(c, Vec2::splat(r * 2.0)),
            Area::Rect(r, _) => r,
        }
    }
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
}

fn hotspots(family: Family) -> Vec<(Part, Area)> {
    let (left_stick, dpad, right_stick) = match family {
        Family::PlayStation => (pos2(410.0, 360.0), pos2(300.0, 240.0), pos2(590.0, 360.0)),
        _ => (pos2(310.0, 240.0), pos2(410.0, 360.0), pos2(590.0, 360.0)),
    };
    let face = pos2(700.0, 240.0);
    let step = 50.0;
    let face_r = 24.0;
    let pad = 30.0;
    let mut spots = vec![
        (Part::Trigger(Side::Left), Area::Rect(rect(230.0, 10.0, 370.0, 55.0), 12.0)),
        (Part::Trigger(Side::Right), Area::Rect(rect(630.0, 10.0, 770.0, 55.0), 12.0)),
        (Part::Button(Button::LeftShoulder), Area::Rect(rect(210.0, 68.0, 390.0, 100.0), 14.0)),
        (Part::Button(Button::RightShoulder), Area::Rect(rect(610.0, 68.0, 790.0, 100.0), 14.0)),
        (Part::Button(Button::North), Area::Circle(face + vec2(0.0, -step), face_r)),
        (Part::Button(Button::South), Area::Circle(face + vec2(0.0, step), face_r)),
        (Part::Button(Button::West), Area::Circle(face + vec2(-step, 0.0), face_r)),
        (Part::Button(Button::East), Area::Circle(face + vec2(step, 0.0), face_r)),
        (Part::Stick(Side::Left), Area::Circle(left_stick, 50.0)),
        (Part::Stick(Side::Right), Area::Circle(right_stick, 50.0)),
        (
            Part::Button(Button::DPadUp),
            Area::Rect(Rect::from_center_size(dpad + vec2(0.0, -pad), Vec2::splat(30.0)), 4.0),
        ),
        (
            Part::Button(Button::DPadDown),
            Area::Rect(Rect::from_center_size(dpad + vec2(0.0, pad), Vec2::splat(30.0)), 4.0),
        ),
        (
            Part::Button(Button::DPadLeft),
            Area::Rect(Rect::from_center_size(dpad + vec2(-pad, 0.0), Vec2::splat(30.0)), 4.0),
        ),
        (
            Part::Button(Button::DPadRight),
            Area::Rect(Rect::from_center_size(dpad + vec2(pad, 0.0), Vec2::splat(30.0)), 4.0),
        ),
        (Part::Button(Button::Guide), Area::Circle(pos2(500.0, 290.0), 22.0)),
        (Part::Button(Button::Misc1), Area::Rect(rect(470.0, 335.0, 530.0, 355.0), 8.0)),
        (Part::Button(Button::LeftPaddle1), Area::Rect(rect(370.0, 545.0, 420.0, 575.0), 8.0)),
        (Part::Button(Button::LeftPaddle2), Area::Rect(rect(430.0, 545.0, 480.0, 575.0), 8.0)),
        (Part::Button(Button::RightPaddle1), Area::Rect(rect(520.0, 545.0, 570.0, 575.0), 8.0)),
        (Part::Button(Button::RightPaddle2), Area::Rect(rect(580.0, 545.0, 630.0, 575.0), 8.0)),
    ];
    if family == Family::PlayStation {
        spots.push((
            Part::Button(Button::Touchpad),
            Area::Rect(rect(430.0, 140.0, 570.0, 225.0), 14.0),
        ));
        spots
            .push((Part::Button(Button::Back), Area::Rect(rect(370.0, 140.0, 415.0, 165.0), 10.0)));
        spots.push((
            Part::Button(Button::Start),
            Area::Rect(rect(585.0, 140.0, 630.0, 165.0), 10.0),
        ));
    } else {
        spots
            .push((Part::Button(Button::Back), Area::Rect(rect(420.0, 200.0, 475.0, 228.0), 12.0)));
        spots.push((
            Part::Button(Button::Start),
            Area::Rect(rect(525.0, 200.0, 580.0, 228.0), 12.0),
        ));
        spots.push((
            Part::Button(Button::Touchpad),
            Area::Rect(rect(470.0, 140.0, 530.0, 165.0), 8.0),
        ));
    }
    spots
}

/// What the view needs to know about the profile and the live pad.
pub(crate) struct ViewState<'a> {
    pub(crate) family: Family,
    pub(crate) layer: &'a Layer,
    pub(crate) live: Option<&'a PadStatus>,
    pub(crate) selected: Option<Part>,
    /// The stick direction being edited, when a stick is selected.
    pub(crate) direction: Direction,
    /// Show buttons that only some controllers have, even when unused.
    pub(crate) show_all: bool,
    /// Label chips with assignment names instead of keys (Xpadder's "Abc" button).
    pub(crate) show_names: bool,
    /// An imported controller picture to draw instead of the built-in controller.
    pub(crate) picture: Option<&'a Picture>,
    /// The profile acts as an Xbox controller: controls with nothing assigned are labelled with
    /// what they pass through as.
    pub(crate) passthrough: bool,
}

/// What the user clicked: a control and, for a stick's direction chip, the direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pick {
    pub(crate) part: Part,
    pub(crate) direction: Option<Direction>,
}

/// Room around the drawn controller, and around pictures, for label chips.
const MARGIN: Vec2 = vec2(60.0, 44.0);
const PICTURE_MARGIN: Vec2 = vec2(40.0, 30.0);

/// Hotspots from a picture's layout, in the picture's pixel coordinates.
fn picture_spots(picture: &Picture) -> Vec<(Part, Area)> {
    picture
        .layout
        .positions
        .iter()
        .map(|(&control, &[x, y])| {
            let centre = pos2(x, y);
            match control {
                Control::Button(Button::LeftStick) => {
                    (Part::Stick(Side::Left), Area::Circle(centre, 34.0))
                }
                Control::Button(Button::RightStick) => {
                    (Part::Stick(Side::Right), Area::Circle(centre, 34.0))
                }
                Control::Button(button) => (Part::Button(button), Area::Circle(centre, 17.0)),
                Control::LeftTrigger => (Part::Trigger(Side::Left), Area::Circle(centre, 17.0)),
                Control::RightTrigger => (Part::Trigger(Side::Right), Area::Circle(centre, 17.0)),
            }
        })
        .collect()
}

/// The controls this view draws and their areas, plus the rectangle the controller takes up,
/// for views that lay labels out themselves (the callout sheet). Positions are in the picture's
/// pixels, or in design units for the drawn controller; [`draw_body`] draws the latter.
pub(crate) fn controls(state: &ViewState<'_>) -> (Vec<(Part, Area)>, Rect) {
    match state.picture {
        Some(picture) => {
            let size = Vec2::from(picture.layout.size);
            (picture_spots(picture), Rect::from_min_size(Pos2::ZERO, size))
        }
        None => {
            // The same buttons as `show` draws.
            let spots: Vec<(Part, Area)> = hotspots(state.family)
                .into_iter()
                .filter(|&(part, _)| {
                    state.show_all
                        || !is_optional(state.family, part)
                        || is_assigned(state.layer, part)
                        || state.selected == Some(part)
                        || state.live.is_some_and(|pad| is_pressed(pad, part))
                })
                .collect();
            // The body spans x 175..825 and y 105..535 (grips included), triggers start at 10.
            let body = rect(175.0, 10.0, 825.0, 535.0);
            let bounds = spots.iter().fold(body, |bounds, (_, area)| bounds.union(area.bounds()));
            // Room for the body's shadow and the selection glow.
            (spots, bounds.expand2(vec2(8.0, 12.0)))
        }
    }
}

/// Where a label chip goes: an anchor, the side of it the chip grows towards, and the widest
/// the chip may be, in design units.
#[derive(Clone, Copy)]
struct LabelSpot {
    anchor: Pos2,
    align: Align2,
    max_width: f32,
}

fn spot(x: f32, y: f32, align: Align2, max_width: f32) -> LabelSpot {
    LabelSpot { anchor: pos2(x, y), align, max_width }
}

/// Chip placement on the drawn controller: outwards from each control, so chips sit beside the
/// controls instead of hiding them.
fn label_spot(part: Part, area: Area, family: Family) -> LabelSpot {
    use Align2 as A;
    match (part, area) {
        (Part::Trigger(_), Area::Rect(r, _)) => {
            spot(r.center().x, r.min.y - 6.0, A::CENTER_BOTTOM, 150.0)
        }
        (Part::Button(Button::LeftShoulder), Area::Rect(r, _)) => {
            spot(r.min.x - 6.0, r.center().y, A::RIGHT_CENTER, 140.0)
        }
        (Part::Button(Button::RightShoulder), Area::Rect(r, _)) => {
            spot(r.max.x + 6.0, r.center().y, A::LEFT_CENTER, 140.0)
        }
        (Part::Button(Button::North), Area::Circle(c, r)) => {
            spot(c.x, c.y - r - 5.0, A::CENTER_BOTTOM, 110.0)
        }
        (Part::Button(Button::South), Area::Circle(c, r)) => {
            spot(c.x, c.y + r + 5.0, A::CENTER_TOP, 110.0)
        }
        (Part::Button(Button::West), Area::Circle(c, r)) => {
            spot(c.x - r - 5.0, c.y, A::RIGHT_CENTER, 70.0)
        }
        (Part::Button(Button::East), Area::Circle(c, r)) => {
            spot(c.x + r + 5.0, c.y, A::LEFT_CENTER, 130.0)
        }
        (Part::Button(Button::DPadUp), Area::Rect(r, _)) => {
            spot(r.center().x, r.min.y - 4.0, A::CENTER_BOTTOM, 90.0)
        }
        (Part::Button(Button::DPadDown), Area::Rect(r, _)) => {
            spot(r.center().x, r.max.y + 4.0, A::CENTER_TOP, 90.0)
        }
        (Part::Button(Button::DPadLeft), Area::Rect(r, _)) => {
            spot(r.min.x - 4.0, r.center().y, A::RIGHT_CENTER, 120.0)
        }
        (Part::Button(Button::DPadRight), Area::Rect(r, _)) => {
            spot(r.max.x + 4.0, r.center().y, A::LEFT_CENTER, 60.0)
        }
        (Part::Button(Button::Guide), Area::Circle(c, r)) => {
            spot(c.x, c.y + r + 4.0, A::CENTER_TOP, 90.0)
        }
        (Part::Button(Button::Touchpad), Area::Rect(r, _)) if family == Family::PlayStation => {
            spot(r.center().x, r.center().y, A::CENTER_CENTER, 130.0)
        }
        (
            Part::Button(
                Button::Misc1
                | Button::LeftPaddle1
                | Button::LeftPaddle2
                | Button::RightPaddle1
                | Button::RightPaddle2,
            ),
            Area::Rect(r, _),
        ) => spot(r.center().x, r.max.y + 4.0, A::CENTER_TOP, 80.0),
        (_, Area::Rect(r, _)) => spot(r.center().x, r.min.y - 4.0, A::CENTER_BOTTOM, 100.0),
        (_, Area::Circle(c, _)) => spot(c.x, c.y, A::CENTER_CENTER, 90.0),
    }
}

/// Chip placement around a stick, for each direction.
fn stick_spot(centre: Pos2, radius: f32, direction: Direction) -> LabelSpot {
    use Align2 as A;
    let gap = radius + 6.0;
    let d = gap * 0.75;
    let (x, y) = (centre.x, centre.y);
    match direction {
        Direction::Up => spot(x, y - gap, A::CENTER_BOTTOM, 90.0),
        Direction::Down => spot(x, y + gap, A::CENTER_TOP, 90.0),
        Direction::Left => spot(x - gap, y, A::RIGHT_CENTER, 70.0),
        Direction::Right => spot(x + gap, y, A::LEFT_CENTER, 70.0),
        Direction::UpLeft => spot(x - d, y - d, A::RIGHT_BOTTOM, 70.0),
        Direction::UpRight => spot(x + d, y - d, A::LEFT_BOTTOM, 70.0),
        Direction::DownLeft => spot(x - d, y + d, A::RIGHT_TOP, 70.0),
        Direction::DownRight => spot(x + d, y + d, A::LEFT_TOP, 70.0),
    }
}

const DIRECTIONS: [Direction; 8] = [
    Direction::Up,
    Direction::Down,
    Direction::Left,
    Direction::Right,
    Direction::UpLeft,
    Direction::UpRight,
    Direction::DownLeft,
    Direction::DownRight,
];

/// The label chips a part shows, with where they go, what a click on them selects and whether
/// what they label is pressed on the live pad.
fn part_chips(
    state: &ViewState<'_>,
    part: Part,
    area: Area,
    on_picture: bool,
) -> Vec<(LabelSpot, Chip, Option<Direction>, bool)> {
    let layer = state.layer;
    let part_pressed = state.live.is_some_and(|pad| is_pressed(pad, part));
    let own_spot = |area: Area| {
        if on_picture {
            let c = match area {
                Area::Circle(c, _) => c,
                Area::Rect(r, _) => r.center(),
            };
            spot(c.x, c.y, Align2::CENTER_CENTER, 90.0)
        } else {
            label_spot(part, area, state.family)
        }
    };
    // What a control with nothing assigned passes through as, when the profile acts as an Xbox
    // controller.
    let xbox = |name: Option<&str>| {
        name.filter(|_| state.passthrough).map(|name| {
            let what = match name {
                "Up" => "D-pad up",
                "Down" => "D-pad down",
                "Left" => "D-pad left",
                "Right" => "D-pad right",
                other => other,
            };
            let tip = format!("Nothing assigned, so it acts as the Xbox controller's {what}");
            Chip::plain(name, Tone::Xbox, &tip)
        })
    };
    let single = |assignment: Option<&zpadder_model::profile::Assignment>, name: Option<&str>| {
        assignment
            .and_then(|a| labels::chip(a, state.show_names))
            .or_else(|| xbox(name))
            .map(|chip| vec![(own_spot(area), chip, None, part_pressed)])
            .unwrap_or_default()
    };
    match part {
        Part::Button(button) => {
            single(layer.buttons.get(&button), zpadder_model::xbox::button_name(button))
        }
        Part::Trigger(Side::Left) => single(layer.left_trigger.as_ref(), Some("LT")),
        Part::Trigger(Side::Right) => single(layer.right_trigger.as_ref(), Some("RT")),
        Part::Stick(side) => {
            let Area::Circle(centre, radius) = area else { return Vec::new() };
            // On photos the stick-click label sits on the knob, so keep direction labels clear.
            let radius = if on_picture { radius + 14.0 } else { radius };
            let (stick, click) = match side {
                Side::Left => (&layer.left_stick, Button::LeftStick),
                Side::Right => (&layer.right_stick, Button::RightStick),
            };
            let (x, y) = live_stick(state, side);
            let clicked = state.live.is_some_and(|pad| pad.pressed.contains(&click));
            let mut chips = Vec::new();
            match stick.mode {
                StickMode::Off => {
                    let name = match side {
                        Side::Left => "left stick",
                        Side::Right => "right stick",
                    };
                    if let Some(mut chip) = xbox(Some(name)) {
                        chip.text = "Xbox stick".into();
                        let spot = stick_spot(centre, radius, Direction::Down);
                        chips.push((spot, chip, None, part_pressed));
                    }
                }
                StickMode::Mouse => {
                    let tip = format!("Moves the pointer, up to {} px/s", stick.mouse.speed);
                    let chip = Chip::plain("Mouse", Tone::Mouse, &tip);
                    let spot = stick_spot(centre, radius, Direction::Down);
                    chips.push((spot, chip, None, part_pressed));
                }
                StickMode::Scroll => {
                    let tip = format!("Scrolls, up to {} notches/s", stick.scroll.speed);
                    let chip = Chip::plain("Scroll", Tone::Mouse, &tip);
                    let spot = stick_spot(centre, radius, Direction::Down);
                    chips.push((spot, chip, None, part_pressed));
                }
                StickMode::Directions => {
                    for direction in DIRECTIONS {
                        let diagonal = !matches!(
                            direction,
                            Direction::Up | Direction::Down | Direction::Left | Direction::Right
                        );
                        if diagonal && !stick.directions.eight_way {
                            continue;
                        }
                        if let Some(chip) = stick
                            .directions
                            .get(direction)
                            .and_then(|a| labels::chip(a, state.show_names))
                        {
                            chips.push((
                                stick_spot(centre, radius, direction),
                                chip,
                                Some(direction),
                                direction_active(x, y, stick, direction),
                            ));
                        }
                    }
                }
            }
            if let Some(chip) = layer
                .buttons
                .get(&click)
                .and_then(|a| labels::chip(a, state.show_names))
                .or_else(|| xbox(zpadder_model::xbox::button_name(click)))
            {
                let mut chip = chip;
                chip.tooltip = format!("Stick click\n{}", chip.tooltip);
                // It rides on the knob, so the label shows where the stick is.
                let (_, travel) = stick_reach(on_picture);
                let at = centre + clamp_unit(vec2(x, y)) * travel;
                chips.push((spot(at.x, at.y, Align2::CENTER_CENTER, 120.0), chip, None, clicked));
            }
            chips
        }
    }
}

/// Draws the controller with what each control does written beside it, and returns what the
/// user clicked, if anything.
pub(crate) fn show(ui: &mut Ui, state: &ViewState<'_>) -> Option<Pick> {
    let on_picture = state.picture.is_some();
    let spots: Vec<(Part, Area)> = match state.picture {
        Some(picture) => picture_spots(picture),
        // Buttons only some pads have are shown when the pad has them in use or they are
        // assigned.
        None => hotspots(state.family)
            .into_iter()
            .filter(|&(part, _)| {
                state.show_all
                    || !is_optional(state.family, part)
                    || is_assigned(state.layer, part)
                    || state.selected == Some(part)
                    || state.live.is_some_and(|pad| is_pressed(pad, part))
            })
            .collect(),
    };
    let (content, margin) = match state.picture {
        Some(picture) => (Vec2::from(picture.layout.size), PICTURE_MARGIN),
        None => {
            // The grips end at y = 535; paddles below them need room for their chips.
            let bottom = spots
                .iter()
                .map(|(_, area)| match area {
                    Area::Circle(c, r) => c.y + r,
                    Area::Rect(r, _) => r.max.y,
                })
                .fold(535.0_f32, f32::max);
            let room_for_chips = if bottom > 535.0 { 32.0 } else { 0.0 };
            (vec2(WIDTH, bottom + room_for_chips), MARGIN)
        }
    };
    let design = content + margin * 2.0;
    let available = ui.available_size() - vec2(0.0, 48.0); // leave room for the legend
    let max_scale = if on_picture { 3.0 } else { 1.5 };
    let scale = (available.x / design.x).min(available.y / design.y).clamp(0.3, max_scale);
    let (response, painter) = ui.allocate_painter(design * scale, Sense::click());
    let origin = response.rect.min;
    let to_screen = |p: Pos2| origin + (p.to_vec2() + margin) * scale;
    let to_design =
        |p: Pos2| pos2((p.x - origin.x) / scale - margin.x, (p.y - origin.y) / scale - margin.y);
    let colors = Colors::new(ui);

    let canvas = if on_picture {
        theme::picture_canvas_fill(ui.visuals())
    } else {
        theme::canvas_fill(ui.visuals())
    };
    painter.rect_filled(response.rect, CornerRadius::same(14), canvas);
    match state.picture {
        Some(picture) => {
            if let Some(texture) = &picture.texture {
                let rect = Rect::from_min_max(to_screen(Pos2::ZERO), to_screen(content.to_pos2()));
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(texture.id(), rect, uv, Color32::WHITE);
            }
        }
        None => draw_body(&painter, &to_screen, scale, &colors),
    }

    // Label chips, laid out before drawing so they can be hit-tested first (they are on top).
    // Photos are drawn smaller than the drawn controller, so their labels get a larger base.
    let font_size = if on_picture {
        (19.5 * scale).clamp(12.5, 19.0)
    } else {
        (14.5 * scale).clamp(11.0, 17.0)
    };
    let mut chips: Vec<(Rect, Pick, Chip, std::sync::Arc<egui::Galley>, ChipLook)> = Vec::new();
    for &(part, area) in &spots {
        for (label, chip, direction, pressed) in part_chips(state, part, area, on_picture) {
            let selected = state.selected == Some(part)
                && (direction.is_none() || direction == Some(state.direction));
            let galley = chip_galley(
                &painter,
                &chip,
                // Room for a short name like "⇧ Numbers" even when the picture is small.
                (label.max_width * scale).max(110.0),
                font_size,
                &colors,
            );
            let padding = vec2(7.0, 3.0) * scale.max(0.85);
            let rect =
                label.align.anchor_size(to_screen(label.anchor), galley.size() + padding * 2.0);
            let look = ChipLook { selected, pressed, bound: chip.tone != Tone::Xbox };
            chips.push((rect, Pick { part, direction }, chip, galley, look));
        }
    }

    let pointer = response.hover_pos();
    // What the pointer is over: the topmost chip wins over the control under it.
    let hovered_chip: Option<(Pick, String)> = pointer.and_then(|p| {
        chips.iter().rev().find(|c| c.0.contains(p)).map(|c| (c.1, c.2.tooltip.clone()))
    });
    let hovered_part = match &hovered_chip {
        Some((pick, _)) => Some(pick.part),
        None => pointer.map(to_design).and_then(|p| {
            spots.iter().rev().find(|(_, area)| area.contains(p)).map(|&(part, _)| part)
        }),
    };
    if hovered_part.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // On a picture the markers sit over the artwork, so they are drawn translucent.
    let alpha = if on_picture { 0.7 } else { 1.0 };
    for &(part, area) in &spots {
        let pressed = state.live.is_some_and(|pad| is_pressed(pad, part));
        let selected = state.selected == Some(part);
        let fill = if pressed {
            colors.pressed
        } else if hovered_part == Some(part) {
            colors.hover
        } else {
            colors.control
        }
        .gamma_multiply(alpha);
        let stroke = if selected {
            Stroke::new(3.0 * scale.max(0.7), theme::ACCENT)
        } else if is_assigned(state.layer, part) {
            Stroke::new(1.5 * scale.max(0.7), colors.assigned)
        } else {
            Stroke::new(1.0, colors.outline)
        };
        match area {
            Area::Circle(centre, radius) => {
                if selected {
                    painter.circle_filled(
                        to_screen(centre),
                        (radius + 6.0) * scale,
                        theme::ACCENT.gamma_multiply(0.18),
                    );
                }
                painter.circle(to_screen(centre), radius * scale, fill, stroke);
                if let Part::Stick(side) = part {
                    draw_stick(&painter, to_screen(centre), scale, state, side, &colors);
                } else if pressed {
                    painter.circle_stroke(
                        to_screen(centre),
                        (radius + 6.0) * scale,
                        Stroke::new(3.0 * scale.max(0.8), colors.pressed),
                    );
                }
            }
            Area::Rect(r, rounding) => {
                let screen = Rect::from_min_max(to_screen(r.min), to_screen(r.max));
                if selected {
                    painter.rect_filled(
                        screen.expand(5.0 * scale),
                        CornerRadius::same(((rounding + 5.0) * scale) as u8),
                        theme::ACCENT.gamma_multiply(0.18),
                    );
                }
                painter.rect(
                    screen,
                    CornerRadius::same((rounding * scale) as u8),
                    fill,
                    stroke,
                    egui::StrokeKind::Inside,
                );
                if pressed {
                    painter.rect_stroke(
                        screen.expand(5.0 * scale),
                        CornerRadius::same(((rounding + 5.0) * scale) as u8),
                        Stroke::new(3.0 * scale.max(0.8), colors.pressed),
                        egui::StrokeKind::Outside,
                    );
                }
                if let (Part::Trigger(side), Some(pad)) = (part, state.live) {
                    let value = pad.axes[if side == Side::Left { 4 } else { 5 }].clamp(0.0, 1.0);
                    let mut bar = screen.shrink(3.0);
                    bar.set_width(bar.width() * value);
                    painter.rect_filled(
                        bar,
                        CornerRadius::same(4),
                        colors.pressed.gamma_multiply(0.6),
                    );
                }
            }
        }
        let centre = match area {
            Area::Circle(c, _) => c,
            Area::Rect(r, _) => r.center(),
        };
        if !on_picture {
            let text = cap(state.family, part);
            if !text.is_empty() && !matches!(part, Part::Stick(_)) {
                painter.text(
                    to_screen(centre),
                    Align2::CENTER_CENTER,
                    text,
                    FontId::proportional((13.0 * scale).max(8.0)),
                    colors.cap,
                );
            }
            if state.family == Family::PlayStation
                && let Part::Button(
                    button @ (Button::South | Button::East | Button::West | Button::North),
                ) = part
            {
                draw_ps_symbol(&painter, to_screen(centre), 9.0 * scale, button, colors.cap);
            }
        }
    }

    // Chips last: pressed ones above the rest, the selected control's on top.
    chips.sort_by_key(|chip| (chip.4.selected, chip.4.pressed));
    for (rect, _, chip, galley, look) in &chips {
        draw_chip(&painter, *rect, galley.clone(), chip.tone, *look, scale, &colors);
    }

    legend(ui, &colors, state.passthrough);

    let tooltip = hovered_chip.as_ref().map(|(_, tip)| tip.clone()).filter(|t| !t.is_empty());
    let clicked = response.clicked();
    if let Some(tooltip) = tooltip {
        response.on_hover_text_at_pointer(tooltip);
    }
    if !clicked {
        return None;
    }
    match hovered_chip {
        Some((pick, _)) => Some(pick),
        None => hovered_part.map(|part| Pick { part, direction: None }),
    }
}

/// What the chip colours mean.
fn legend(ui: &mut Ui, colors: &Colors, passthrough: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let xbox = passthrough.then_some((Tone::Xbox, "Xbox controller"));
        for (tone, text) in [
            (Tone::Key, "Keys"),
            (Tone::Mouse, "Mouse"),
            (Tone::Layer, "Layer switch"),
            (Tone::Macro, "Timed (sequence, hold, release…)"),
        ]
        .into_iter()
        .chain(xbox)
        {
            let (fill, _) = colors.chip(tone);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(3), fill);
            ui.weak(text);
            ui.add_space(6.0);
        }
        ui.weak(if passthrough {
            "T turbo   ⇆ toggle   ~ rumble   L also switches layer   + also the Xbox button"
        } else {
            "T turbo   ⇆ toggle   ~ rumble   L also switches layer"
        });
        ui.weak("Hover a label for details, click to edit.");
    });
}

/// The drawn controller's body: a rounded slab with two grips, with a soft shadow.
pub(crate) fn draw_body(
    painter: &Painter,
    to_screen: &dyn Fn(Pos2) -> Pos2,
    scale: f32,
    colors: &Colors,
) {
    let slab = Rect::from_min_max(to_screen(pos2(190.0, 105.0)), to_screen(pos2(810.0, 430.0)));
    let grips = [to_screen(pos2(290.0, 420.0)), to_screen(pos2(710.0, 420.0))];
    let radius = CornerRadius::same((110.0 * scale) as u8);
    let drop = vec2(0.0, 10.0 * scale);
    let shadow = Color32::from_black_alpha(70);
    painter.rect_filled(slab.translate(drop), radius, shadow);
    for grip in grips {
        painter.circle_filled(grip + drop, 115.0 * scale, shadow);
    }
    for grip in grips {
        painter.circle(grip, 115.0 * scale, colors.body, Stroke::new(1.0, colors.body_edge));
    }
    painter.rect(
        slab,
        radius,
        colors.body,
        Stroke::new(1.0, colors.body_edge),
        egui::StrokeKind::Inside,
    );
    // Cover the grip outlines where they meet the slab.
    painter.rect_filled(slab.shrink(2.0), radius, colors.body);
}

/// Lays out a chip's text and badges, shortened with "…" to fit `max_width` pixels.
fn chip_galley(
    painter: &Painter,
    chip: &Chip,
    max_width: f32,
    font_size: f32,
    colors: &Colors,
) -> std::sync::Arc<egui::Galley> {
    let (_, text_color) = colors.chip(chip.tone);
    let font = FontId::proportional(font_size);
    let badge_font = FontId::proportional(font_size * 0.8);
    let padding = 12.0;
    let build = |text: &str| {
        let mut job = LayoutJob::default();
        job.append(text, 0.0, TextFormat::simple(font.clone(), text_color));
        if !chip.badges.is_empty() {
            job.append(
                &chip.badges.join(" "),
                4.0,
                TextFormat::simple(badge_font.clone(), text_color.gamma_multiply(0.6)),
            );
        }
        painter.layout_job(job)
    };
    let mut text: Vec<char> = chip.text.chars().collect();
    let mut galley = build(&chip.text);
    while galley.size().x + padding > max_width && text.len() > 1 {
        text.pop();
        let shortened: String = text.iter().collect::<String>() + "…";
        galley = build(&shortened);
    }
    galley
}

/// How a label chip is drawn besides its tone.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ChipLook {
    /// The control is selected for editing.
    pub(crate) selected: bool,
    /// What the chip labels is pressed on the live pad.
    pub(crate) pressed: bool,
    /// The chip shows an assignment, not a control that only passes through.
    pub(crate) bound: bool,
}

/// Draws a label chip: a rounded box in its tone's colours with a soft shadow, and its text. A
/// pressed chip lights up in the press colour, a little larger and with a glow, so a press shows
/// from across the room; a bound one carries a thin outline in the assignment colour.
pub(crate) fn draw_chip(
    painter: &Painter,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    tone: Tone,
    look: ChipLook,
    scale: f32,
    colors: &Colors,
) {
    let (background, text) = colors.chip(tone);
    let grow = 2.0 * scale.max(0.8);
    let rect = if look.pressed { rect.expand(grow) } else { rect };
    let corner = 6.0 * scale.max(0.8);
    let radius = CornerRadius::same(corner as u8);
    if look.pressed {
        painter.rect_filled(
            rect.expand(2.0 * grow),
            CornerRadius::same((corner + 2.0 * grow) as u8),
            colors.pressed.gamma_multiply(0.35),
        );
    }
    painter.rect_filled(rect.translate(vec2(0.0, 1.5)), radius, Color32::from_black_alpha(60));
    let stroke = if look.selected {
        Stroke::new(2.0, theme::ACCENT)
    } else if look.pressed {
        Stroke::new(1.5, Color32::from_white_alpha(220))
    } else if look.bound {
        Stroke::new(1.25, colors.assigned)
    } else {
        Stroke::new(1.0, colors.chip_border)
    };
    let fill = if look.pressed { colors.pressed } else { background };
    painter.rect(rect, radius, fill, stroke, egui::StrokeKind::Inside);
    let pos = rect.center() - galley.size() / 2.0;
    if look.pressed {
        painter.galley_with_override_text_color(pos, galley, Color32::from_rgb(20, 22, 28));
    } else {
        painter.galley(pos, galley, text);
    }
}

/// How big a stick's knob is drawn and how far it moves at full tilt, in design units. Photos
/// are drawn smaller than the drawn controller, so their knobs move further.
fn stick_reach(on_picture: bool) -> (f32, f32) {
    if on_picture { (30.0, 46.0) } else { (30.0, 34.0) }
}

/// A stick's live position, or the centre without a pad.
fn live_stick(state: &ViewState<'_>, side: Side) -> (f32, f32) {
    state.live.map_or((0.0, 0.0), |pad| match side {
        Side::Left => (pad.axes[0], pad.axes[1]),
        Side::Right => (pad.axes[2], pad.axes[3]),
    })
}

/// `v`, shortened to length 1 if longer: a stick's corners reach a little past its circle.
fn clamp_unit(v: Vec2) -> Vec2 {
    if v.length() > 1.0 { v.normalized() } else { v }
}

/// Whether a stick at `(x, y)` presses `direction`, roughly as the mapper decides: past the
/// dead zone and within that direction's sector. Four-way sticks press both neighbours on a
/// diagonal, so their sectors are wider.
fn direction_active(x: f32, y: f32, stick: &Stick, direction: Direction) -> bool {
    if x.hypot(y) < stick.dead_zone.max(0.05) {
        return false;
    }
    // Degrees anticlockwise from right; ZPadder's y is positive downwards.
    let angle = (-y).atan2(x).to_degrees();
    let target: f32 = match direction {
        Direction::Right => 0.0,
        Direction::UpRight => 45.0,
        Direction::Up => 90.0,
        Direction::UpLeft => 135.0,
        Direction::Left => 180.0,
        Direction::DownLeft => -135.0,
        Direction::Down => -90.0,
        Direction::DownRight => -45.0,
    };
    let off = ((angle - target + 540.0) % 360.0 - 180.0).abs();
    off <= if stick.directions.eight_way { 22.5 } else { 67.5 }
}

/// Draws a stick's knob where the live pad has it: outlined in the press colour while pushed,
/// filled with it while clicked, inside a faint ring showing how far it can go.
pub(crate) fn draw_stick(
    painter: &Painter,
    centre: Pos2,
    scale: f32,
    state: &ViewState<'_>,
    side: Side,
    colors: &Colors,
) {
    let on_picture = state.picture.is_some();
    let (knob_radius, travel) = stick_reach(on_picture);
    let (x, y) = live_stick(state, side);
    if state.live.is_some() {
        painter.circle_stroke(
            centre,
            (travel + knob_radius) * scale,
            Stroke::new(1.0, colors.cap.gamma_multiply(0.35)),
        );
    }
    let knob = centre + clamp_unit(vec2(x, y)) * travel * scale;
    let clicked = state.live.is_some_and(|pad| {
        pad.pressed.contains(if side == Side::Left {
            &Button::LeftStick
        } else {
            &Button::RightStick
        })
    });
    let pushed = x.hypot(y) > 0.3;
    // On photos the knob is see-through, so the stick under it still shows.
    let fill = if clicked {
        colors.pressed
    } else if on_picture {
        colors.knob.gamma_multiply(0.75)
    } else {
        colors.knob
    };
    let stroke = if pushed || clicked {
        Stroke::new(2.5 * scale.max(0.8), colors.pressed)
    } else {
        Stroke::new(1.0, colors.outline)
    };
    painter.circle(knob, knob_radius * scale, fill, stroke);
    let text = if side == Side::Left { "L" } else { "R" };
    painter.text(
        knob,
        Align2::CENTER_CENTER,
        text,
        FontId::proportional((13.0 * scale).max(8.0)),
        colors.cap,
    );
}

/// Draws a PlayStation face button's shape (cross, circle, square, triangle).
pub(crate) fn draw_ps_symbol(painter: &Painter, c: Pos2, r: f32, button: Button, color: Color32) {
    let stroke = Stroke::new(2.0, color);
    match button {
        Button::South => {
            painter.line_segment([c + vec2(-r, -r), c + vec2(r, r)], stroke);
            painter.line_segment([c + vec2(-r, r), c + vec2(r, -r)], stroke);
        }
        Button::East => {
            painter.circle_stroke(c, r, stroke);
        }
        Button::West => {
            painter.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(r * 1.8)),
                CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        _ => {
            let points = vec![c + vec2(0.0, -r), c + vec2(r, r * 0.8), c + vec2(-r, r * 0.8)];
            painter.add(Shape::closed_line(points, stroke));
        }
    }
}

/// Buttons that many controllers lack.
fn is_optional(family: Family, part: Part) -> bool {
    match part {
        Part::Button(Button::Touchpad) => family != Family::PlayStation,
        Part::Button(
            Button::Misc1
            | Button::LeftPaddle1
            | Button::LeftPaddle2
            | Button::RightPaddle1
            | Button::RightPaddle2,
        ) => true,
        _ => false,
    }
}

/// Whether the live pad has this part pressed (or pushed, for sticks and triggers).
pub(crate) fn is_pressed(pad: &PadStatus, part: Part) -> bool {
    match part {
        Part::Button(button) => pad.pressed.contains(&button),
        Part::Trigger(Side::Left) => pad.axes[4] > 0.2,
        Part::Trigger(Side::Right) => pad.axes[5] > 0.2,
        Part::Stick(Side::Left) => pad.axes[0].hypot(pad.axes[1]) > 0.3,
        Part::Stick(Side::Right) => pad.axes[2].hypot(pad.axes[3]) > 0.3,
    }
}

pub(crate) fn is_assigned(layer: &Layer, part: Part) -> bool {
    match part {
        Part::Button(button) => layer.buttons.contains_key(&button),
        Part::Trigger(Side::Left) => layer.left_trigger.is_some(),
        Part::Trigger(Side::Right) => layer.right_trigger.is_some(),
        Part::Stick(Side::Left) => {
            layer.left_stick.mode != StickMode::Off
                || layer.buttons.contains_key(&Button::LeftStick)
        }
        Part::Stick(Side::Right) => {
            layer.right_stick.mode != StickMode::Off
                || layer.buttons.contains_key(&Button::RightStick)
        }
    }
}

/// The controller's and the chips' colours for the current theme.
pub(crate) struct Colors {
    pub(crate) body: Color32,
    pub(crate) body_edge: Color32,
    pub(crate) control: Color32,
    pub(crate) knob: Color32,
    pub(crate) hover: Color32,
    pub(crate) pressed: Color32,
    pub(crate) assigned: Color32,
    pub(crate) outline: Color32,
    /// Text printed on the controls themselves.
    pub(crate) cap: Color32,
    pub(crate) chip_border: Color32,
    pub(crate) dark: bool,
}

impl Colors {
    pub(crate) fn new(ui: &Ui) -> Self {
        if ui.visuals().dark_mode {
            Self {
                body: Color32::from_rgb(40, 44, 52),
                body_edge: Color32::from_rgb(58, 63, 74),
                control: Color32::from_rgb(58, 63, 74),
                knob: Color32::from_rgb(74, 80, 94),
                hover: Color32::from_rgb(78, 86, 104),
                pressed: Color32::from_rgb(240, 160, 60),
                assigned: Color32::from_rgb(120, 160, 255),
                outline: Color32::from_rgb(28, 30, 36),
                cap: Color32::from_rgb(170, 176, 190),
                chip_border: Color32::from_black_alpha(0),
                dark: true,
            }
        } else {
            Self {
                body: Color32::from_rgb(208, 213, 222),
                body_edge: Color32::from_rgb(186, 192, 204),
                control: Color32::from_rgb(244, 246, 250),
                knob: Color32::from_rgb(228, 232, 240),
                hover: Color32::from_rgb(222, 230, 246),
                pressed: Color32::from_rgb(245, 165, 60),
                assigned: Color32::from_rgb(60, 110, 230),
                outline: Color32::from_rgb(160, 166, 178),
                cap: Color32::from_rgb(90, 96, 110),
                chip_border: Color32::from_rgb(200, 205, 215),
                dark: false,
            }
        }
    }

    /// Background and text colour of a chip.
    pub(crate) fn chip(&self, tone: Tone) -> (Color32, Color32) {
        match (tone, self.dark) {
            (Tone::Key, true) => (Color32::from_rgb(236, 239, 244), Color32::from_rgb(20, 22, 28)),
            (Tone::Key, false) => (Color32::WHITE, Color32::from_rgb(20, 22, 28)),
            (Tone::Mouse, _) => (Color32::from_rgb(214, 228, 255), Color32::from_rgb(16, 36, 80)),
            (Tone::Layer, _) => (theme::ACCENT, Color32::WHITE),
            (Tone::Macro, _) => (Color32::from_rgb(255, 228, 178), Color32::from_rgb(60, 38, 0)),
            // Quiet: the control is free, and only passes through.
            (Tone::Xbox, true) => (self.control, self.cap),
            (Tone::Xbox, false) => (Color32::from_rgb(232, 235, 241), self.cap),
        }
    }
}

#[cfg(test)]
mod tests {
    use zpadder_model::profile::Assignment;

    use super::*;

    #[test]
    fn stick_directions_light_up_where_the_stick_points() {
        let mut stick = Stick { dead_zone: 0.25, ..Stick::default() };
        // Up and a little right (ZPadder's y is negative upwards).
        let (x, y) = (0.2, -0.9);
        assert!(direction_active(x, y, &stick, Direction::Up));
        assert!(!direction_active(x, y, &stick, Direction::Right));
        // Inside the dead zone nothing is pressed.
        assert!(!direction_active(0.1, -0.1, &stick, Direction::Up));
        // On a diagonal a four-way stick presses both neighbours, an eight-way one the diagonal.
        let (x, y) = (0.7, -0.7);
        assert!(direction_active(x, y, &stick, Direction::Up));
        assert!(direction_active(x, y, &stick, Direction::Right));
        stick.directions.eight_way = true;
        assert!(direction_active(x, y, &stick, Direction::UpRight));
        assert!(!direction_active(x, y, &stick, Direction::Up));
        // Left wraps around the +/-180 degree seam.
        assert!(direction_active(-1.0, 0.01, &stick, Direction::Left));
        assert!(direction_active(-1.0, -0.01, &stick, Direction::Left));
    }

    /// The chips a part shows on the drawn PlayStation controller, as (text, tone, tooltip).
    fn chips(layer: &Layer, part: Part, passthrough: bool) -> Vec<(String, Tone, String)> {
        let state = ViewState {
            family: Family::PlayStation,
            layer,
            live: None,
            selected: None,
            direction: Direction::Up,
            show_all: true,
            show_names: false,
            picture: None,
            passthrough,
        };
        let (_, area) = hotspots(Family::PlayStation)
            .into_iter()
            .find(|&(p, _)| p == part)
            .expect("the part is drawn");
        part_chips(&state, part, area, false)
            .into_iter()
            .map(|(_, chip, _, _)| (chip.text, chip.tone, chip.tooltip))
            .collect()
    }

    #[test]
    fn free_controls_show_what_they_pass_through_as() {
        let mut layer = Layer::new("Main");
        let space = Assignment { slots: vec!["space".parse().unwrap()], ..Assignment::default() };
        layer.buttons.insert(Button::East, space);
        layer.buttons.insert(Button::North, Assignment::default());

        // Cross is A by position; an empty assignment counts as free.
        let south = chips(&layer, Part::Button(Button::South), true);
        assert_eq!((south[0].0.as_str(), south[0].1), ("A", Tone::Xbox));
        assert_eq!(chips(&layer, Part::Button(Button::North), true)[0].0, "Y");
        // Assigned controls show their assignment, as always.
        let east = chips(&layer, Part::Button(Button::East), true);
        assert_eq!((east[0].0.as_str(), east[0].1), ("Space", Tone::Key));
        let up = chips(&layer, Part::Button(Button::DPadUp), true);
        assert!(up[0].2.ends_with("D-pad up"), "{up:?}");
        let trigger = chips(&layer, Part::Trigger(Side::Right), true);
        assert_eq!(trigger[0].0, "RT");
        // A stick in "off" mode passes through, and so does its click.
        let stick: Vec<String> =
            chips(&layer, Part::Stick(Side::Left), true).into_iter().map(|c| c.0).collect();
        assert_eq!(stick, ["Xbox stick", "LS"]);

        // Without an Xbox controller, free controls stay unlabelled.
        assert!(chips(&layer, Part::Button(Button::South), false).is_empty());
        assert!(chips(&layer, Part::Stick(Side::Left), false).is_empty());
    }

    #[test]
    fn controls_with_no_xbox_counterpart_stay_unlabelled() {
        let mut layer = Layer::new("Main");
        layer.left_stick.mode = StickMode::Mouse;
        assert!(chips(&layer, Part::Button(Button::LeftPaddle1), true).is_empty());
        let stick: Vec<String> =
            chips(&layer, Part::Stick(Side::Left), true).into_iter().map(|c| c.0).collect();
        assert_eq!(stick, ["Mouse", "LS"], "a mouse stick does not pass through");
    }
}
