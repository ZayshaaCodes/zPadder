//! The test bench: a box where mapping stays on even though ZPadder has focus, so a profile can
//! be tried without switching windows, with a log of what each press sent and why, and live
//! readouts of the sticks and triggers.
//!
//! Mapping stays on only while the box has keyboard focus and the window is focused. The mouse
//! stays still meanwhile: the mapper holds mouse output back and the log lists it instead.

use std::collections::VecDeque;
use std::f32::consts::TAU;

use eframe::egui::{
    self, Align, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2,
    vec2,
};
use zpadder_model::profile::{Assignment, Direction, DirectionSettings, Layer, Stick, StickMode};
use zpadder_runtime::{
    ACTIVITY_CAPACITY, Activity, Mapper, Origin, PadInput, PadStatus, Status, VirtualPadStatus,
};

use crate::labels;
use crate::pad_view::{self, Family, Part, Side};
use crate::theme;

const INTRO: &str = "Mapping stays on inside this box, so you can try the profile without \
                     switching windows. The mouse stays still here; mouse actions are only listed.";
/// Width of the live stick and trigger readouts.
const READOUT_WIDTH: f32 = 250.0;
/// Diameter of a stick's circle.
const STICK_SIZE: f32 = 56.0;

/// One line of the log as shown, worked out once when the entry arrives.
#[derive(Debug)]
struct LogRow {
    time: String,
    pad: String,
    sent: String,
    held_back: String,
    why: String,
}

impl LogRow {
    fn new(entry: &Activity, family: Family) -> Self {
        Self {
            time: format!("{:.3}", entry.at.as_secs_f64()),
            pad: pad_text(entry, family),
            sent: entry.sent_text(),
            held_back: entry.held_back_text(),
            why: entry.why.clone(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Bench {
    text: String,
    /// What the mapper was last told: keep mapping on for the bench, or not.
    testing: bool,
    /// The activity read so far, oldest first.
    log: VecDeque<Activity>,
    /// The log's rows as shown, in the same order, for the controller family in `rows_family`.
    rows: VecDeque<LogRow>,
    rows_family: Option<Family>,
    /// Sequence number of the newest entry read from the mapper.
    seen: u64,
}

/// What the bench shows about the profile the pad is using.
pub(crate) struct Running<'a> {
    /// The layer whose dead zones and press points the readouts show.
    pub(crate) layer: Option<&'a Layer>,
    /// Set when the mapper is using another profile than the edited one, to say which.
    pub(crate) other: Option<String>,
}

impl Bench {
    /// Draws the bench. `running` says which layer the pad is on, when it is using the edited
    /// profile.
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        mapper: &Mapper,
        status: &Status,
        running: &Running<'_>,
    ) {
        let pad = status.pads.first();
        let family = pad.map_or(Family::Xbox, |pad| Family::from_kind(&pad.kind));
        self.read_log(mapper, family);

        ui.add_space(4.0);
        ui.heading("Test bench");
        ui.weak(INTRO);
        if matches!(status.virtual_pads, VirtualPadStatus::Active { .. }) {
            ui.weak("The Xbox controller stays neutral here, so games don't see these presses.");
        }
        for name in &status.other_mappers {
            ui.colored_label(theme::warn(ui.visuals()), mapper_warning(name));
        }
        ui.add_space(4.0);

        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            let spacing = ui.spacing().item_spacing.x;
            let box_width = (ui.available_width() * 0.3).clamp(200.0, 380.0);
            column(ui, box_width, height, |ui| self.try_box(ui, mapper, status));
            let log_width = (ui.available_width() - READOUT_WIDTH - spacing).max(200.0);
            column(ui, log_width, height, |ui| self.log_view(ui));
            let rest = ui.available_width();
            column(ui, rest, height, |ui| readouts(ui, pad, running, family));
        });
    }

    /// Called instead of [`Bench::show`] while the bench is hidden.
    pub(crate) fn hidden(&mut self, mapper: &Mapper) {
        self.set_testing(mapper, false);
    }

    /// A new mapper replaced the old one: it is not testing, and its log starts again from the
    /// first entry. What the old one logged stays listed.
    pub(crate) fn mapper_restarted(&mut self) {
        self.testing = false;
        self.seen = 0;
    }

    fn set_testing(&mut self, mapper: &Mapper, on: bool) {
        if self.testing != on {
            self.testing = on;
            mapper.set_testing(on);
        }
    }

    /// Reads the mapper's new log entries and works out their rows. The rows are redone only
    /// when another kind of controller is connected, since they use its button names.
    fn read_log(&mut self, mapper: &Mapper, family: Family) {
        let new = mapper.activity_since(self.seen);
        if let Some(last) = new.last() {
            self.seen = last.seq;
        }
        if self.rows_family != Some(family) {
            self.rows_family = Some(family);
            self.rows = self.log.iter().map(|entry| LogRow::new(entry, family)).collect();
        }
        self.rows.extend(new.iter().map(|entry| LogRow::new(entry, family)));
        self.log.extend(new);
        while self.log.len() > ACTIVITY_CAPACITY {
            self.log.pop_front();
            self.rows.pop_front();
        }
    }

    /// The state line, the box to type into and its Clear button. Mapping stays on while the box
    /// has keyboard focus and the window is focused.
    fn try_box(&mut self, ui: &mut Ui, mapper: &Mapper, status: &Status) {
        let weak = ui.visuals().weak_text_color();
        let (state, colour) = if !status.enabled {
            ("Mapping is switched off. Switch it on to try your pad here.", weak)
        } else if status.testing {
            ("● Mapping is on in this box", theme::live(ui.visuals()))
        } else {
            ("Click in the box to try your pad here", weak)
        };
        ui.colored_label(colour, state);
        let button_row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let box_height = (ui.available_height() - button_row).max(40.0);
        let response = ui.add_sized(
            vec2(ui.available_width(), box_height),
            egui::TextEdit::multiline(&mut self.text)
                .id_salt("bench-text")
                .hint_text("Type here with your pad")
                .font(egui::TextStyle::Monospace)
                .lock_focus(true),
        );
        // Drawn after the box, so a click here can hand the focus straight back to it.
        if ui.button("Clear").on_hover_text("Empty the box").clicked() {
            self.text.clear();
            response.request_focus();
        }
        // `has_focus` is only true while the window has focus too.
        self.set_testing(mapper, response.has_focus());
    }

    /// The activity log as a table, newest at the bottom. Only the rows in view are drawn, so a
    /// long log costs nothing; text too long for its column is cut short and shown in full on
    /// hover.
    fn log_view(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.strong("What happened");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(!self.log.is_empty(), egui::Button::new("Clear log")).clicked() {
                    self.log.clear();
                    self.rows.clear();
                }
            });
        });
        if self.rows.is_empty() {
            ui.weak("Each press on the pad, what it sent and why will be listed here.");
            return;
        }
        let widths = log_columns(ui.available_width());
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        // Rows touch, striped; `show_rows` works out where they are from this spacing too.
        ui.spacing_mut().item_spacing.y = 0.0;
        log_row(ui, widths, row_height, None, |ui, column| {
            let heading = ["Time", "On the pad", "Sent", "Why"][column];
            ui.add(egui::Label::new(RichText::new(heading).strong()).truncate());
        });
        egui::ScrollArea::vertical()
            .id_salt("bench-log")
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show_rows(ui, row_height, self.rows.len(), |ui, range| {
                for i in range {
                    let row = &self.rows[i];
                    let stripe = (i % 2 == 1).then(|| ui.visuals().faint_bg_color);
                    log_row(ui, widths, row_height, stripe, |ui, column| match column {
                        0 => {
                            ui.add(egui::Label::new(RichText::new(&row.time).monospace()));
                        }
                        1 => {
                            ui.add(egui::Label::new(&row.pad).truncate());
                        }
                        2 => sent_cell(ui, row),
                        _ => {
                            ui.add(egui::Label::new(RichText::new(&row.why).weak()).truncate());
                        }
                    });
                }
            });
    }
}

/// Widths of the log's columns (time, on the pad, sent, why) for a table `width` wide.
fn log_columns(width: f32) -> [f32; 4] {
    let time = 64.0;
    let rest = (width - time).max(120.0);
    [time, rest * 0.3, rest * 0.3, rest * 0.4]
}

/// One row of the log: a band `height` tall, striped when `stripe` is given, with each column
/// drawn by `cell` in its own space.
fn log_row(
    ui: &mut Ui,
    widths: [f32; 4],
    height: f32,
    stripe: Option<egui::Color32>,
    mut cell: impl FnMut(&mut Ui, usize),
) {
    let width = ui.available_width().max(widths.iter().sum());
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    if let Some(fill) = stripe {
        ui.painter().rect_filled(rect, 3.0, fill);
    }
    let mut x = rect.left() + 4.0;
    for (column, &column_width) in widths.iter().enumerate() {
        let inner = (column_width - 10.0).max(10.0);
        let cell_rect = Rect::from_min_size(pos2(x, rect.top()), vec2(inner, height));
        let builder =
            egui::UiBuilder::new().max_rect(cell_rect).layout(Layout::left_to_right(Align::Center));
        ui.scope_builder(builder, |ui| {
            ui.set_clip_rect(cell_rect.intersect(ui.clip_rect()));
            cell(ui, column);
        });
        x += column_width;
    }
}

/// A fixed-size column of the bench, framed as a card.
fn column(ui: &mut Ui, width: f32, height: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(width, height), Layout::top_down(Align::Min), |ui| {
        ui.set_width(width);
        ui.set_height(height);
        theme::card(ui, "", |ui| {
            ui.set_min_height(ui.available_height());
            add(ui);
        });
    });
}

/// The warning shown while another program that reads controllers is running.
fn mapper_warning(name: &str) -> String {
    if name == "Steam" {
        "Steam is running. If Steam Input is on for this controller, games may get each press \
         twice."
            .to_string()
    } else {
        format!(
            "{name} is running and may also be reading this controller, so games may get each \
             press twice."
        )
    }
}

/// The "On the pad" cell: what happened, in the controller's own names.
fn pad_text(entry: &Activity, family: Family) -> String {
    match entry.origin {
        Origin::Timer => "Timer".to_string(),
        Origin::Mapping => "Mapping changed".to_string(),
        Origin::Pad => entry
            .input
            .iter()
            .map(|&input| input_text(input, family))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn input_text(input: PadInput, family: Family) -> String {
    match input {
        PadInput::Button { button, pressed, .. } => {
            let state = if pressed { "down" } else { "up" };
            format!("{} {state}", pad_view::label(family, Part::Button(button)))
        }
        PadInput::Trigger { side, value } => {
            let name = pad_view::label(family, Part::Trigger(view_side(side)));
            format!("{name} {:.0}%", value * 100.0)
        }
        PadInput::Stick { side, .. } => pad_view::label(family, Part::Stick(view_side(side))),
        PadInput::Connected => "Controller connected".to_string(),
        PadInput::Disconnected => "Controller disconnected".to_string(),
    }
}

/// The "Sent" cell. Outputs held back are struck through; the "Why" cell says why.
fn sent_cell(ui: &mut Ui, row: &LogRow) {
    if row.sent.is_empty() && row.held_back.is_empty() {
        ui.weak("Nothing");
    }
    if !row.sent.is_empty() {
        ui.add(egui::Label::new(RichText::new(&row.sent).monospace()).truncate());
    }
    if !row.held_back.is_empty() {
        let text = RichText::new(&row.held_back).monospace().strikethrough().weak();
        ui.add(egui::Label::new(text).truncate())
            .on_hover_text("Not sent: the mouse stays still while testing in ZPadder");
    }
}

fn view_side(side: zpadder_runtime::Side) -> Side {
    match side {
        zpadder_runtime::Side::Left => Side::Left,
        zpadder_runtime::Side::Right => Side::Right,
    }
}

/// Live sticks and triggers of the first pad, with the dead zones and press points of the layer
/// active on it.
fn readouts(ui: &mut Ui, pad: Option<&PadStatus>, running: &Running<'_>, family: Family) {
    let Some(pad) = pad else {
        ui.weak("No controller connected");
        return;
    };
    if let Some(other) = &running.other {
        ui.add(egui::Label::new(RichText::new(other).small().weak()).wrap());
    }
    let layer = running.layer;
    ui.horizontal_top(|ui| {
        let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
        for (side, x, y) in
            [(Side::Left, pad.axes[0], pad.axes[1]), (Side::Right, pad.axes[2], pad.axes[3])]
        {
            let stick = layer.map(|layer| stick_of(layer, side));
            ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Center), |ui| {
                ui.set_width(width);
                stick_circle(ui, stick.map(|stick| stick.dead_zone), x, y)
                    .on_hover_text(format!("x {x:+.2}, y {y:+.2}"));
                let name = pad_view::label(family, Part::Stick(side));
                let doing =
                    stick.map_or_else(|| format!("{x:+.2}, {y:+.2}"), |s| stick_doing(s, x, y));
                ui.add(egui::Label::new(RichText::new(format!("{name}: {doing}")).small()).wrap());
            });
        }
    });
    ui.add_space(4.0);
    let threshold = layer.map(|layer| layer.trigger_threshold);
    for (side, value) in [(Side::Left, pad.axes[4]), (Side::Right, pad.axes[5])] {
        trigger_bar(ui, &pad_view::label(family, Part::Trigger(side)), value, threshold);
    }
}

/// A stick's range with its dead zone as a dashed ring and a dot where the stick is.
fn stick_circle(ui: &mut Ui, dead_zone: Option<f32>, x: f32, y: f32) -> egui::Response {
    let (response, painter) = ui.allocate_painter(Vec2::splat(STICK_SIZE), Sense::hover());
    let centre = response.rect.center();
    let radius = STICK_SIZE / 2.0 - 2.0;
    let visuals = ui.visuals();
    let line = Stroke::new(1.0, visuals.weak_text_color());
    painter.circle_filled(centre, radius, visuals.extreme_bg_color);
    painter.circle_stroke(centre, radius, line);
    if let Some(dead_zone) = dead_zone.filter(|&d| d > 0.0) {
        let ring: Vec<Pos2> = (0..=48)
            .map(|i| centre + Vec2::angled(i as f32 / 48.0 * TAU) * radius * dead_zone)
            .collect();
        painter.extend(Shape::dashed_line(&ring, line, 3.0, 3.0));
    }
    let mut offset = vec2(x, y);
    if offset.length() > 1.0 {
        offset = offset.normalized();
    }
    let outside = dead_zone.is_some_and(|d| x.hypot(y) > d);
    let colour = if outside { theme::ACCENT } else { visuals.weak_text_color() };
    painter.circle_filled(centre + offset * radius, 4.0, colour);
    response
}

/// A trigger's pull as a bar, with a mark at the press point.
fn trigger_bar(ui: &mut Ui, name: &str, value: f32, threshold: Option<f32>) {
    ui.horizontal(|ui| {
        ui.add_sized([32.0, 18.0], egui::Label::new(name));
        let (rect, response) = ui.allocate_exact_size(vec2(130.0, 10.0), Sense::hover());
        let visuals = ui.visuals();
        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, visuals.extreme_bg_color);
        painter.rect_stroke(
            rect,
            3.0,
            Stroke::new(1.0, visuals.weak_text_color()),
            StrokeKind::Inside,
        );
        let pressed = threshold.is_some_and(|t| value >= t);
        let fill = Rect::from_min_size(
            rect.min,
            vec2(rect.width() * value.clamp(0.0, 1.0), rect.height()),
        );
        painter.rect_filled(
            fill,
            3.0,
            if pressed { theme::ACCENT } else { visuals.weak_text_color() },
        );
        if let Some(threshold) = threshold {
            let x = rect.left() + rect.width() * threshold.clamp(0.0, 1.0);
            painter.line_segment(
                [pos2(x, rect.top() - 3.0), pos2(x, rect.bottom() + 3.0)],
                Stroke::new(2.0, visuals.strong_text_color()),
            );
            response.on_hover_text(format!("Fires past the mark, at {:.0}%", threshold * 100.0));
        }
        ui.monospace(format!("{:>3.0}%", value * 100.0));
    });
}

fn stick_of(layer: &Layer, side: Side) -> &Stick {
    match side {
        Side::Left => &layer.left_stick,
        Side::Right => &layer.right_stick,
    }
}

/// What a stick at `(x, y)` does under `stick`'s settings, e.g. "pressing D".
fn stick_doing(stick: &Stick, x: f32, y: f32) -> String {
    let magnitude = x.hypot(y);
    if stick.mode == StickMode::Off {
        return "off".to_string();
    }
    if magnitude == 0.0 || magnitude < stick.dead_zone {
        return "centred".to_string();
    }
    match stick.mode {
        StickMode::Off => "off".to_string(),
        StickMode::Mouse => "moves the pointer".to_string(),
        StickMode::Scroll => "scrolls".to_string(),
        StickMode::Directions => {
            let directions = directions(x, y, &stick.directions);
            let keys: Vec<String> = directions
                .iter()
                .filter_map(|&d| stick.directions.get(d))
                .map(outputs_text)
                .filter(|text| !text.is_empty())
                .collect();
            if keys.is_empty() {
                let names: Vec<&str> = directions.iter().map(|&d| direction_name(d)).collect();
                format!("pushed {}", names.join(" and "))
            } else {
                format!("pressing {}", keys.join(" and "))
            }
        }
    }
}

/// The keycaps an assignment presses first, e.g. "Shift+X".
fn outputs_text(assignment: &Assignment) -> String {
    let outputs = assignment.slots.iter().take_while(|slot| !slot.is_timing());
    outputs.map(labels::short_slot).collect::<Vec<_>>().join("+")
}

/// The directions a stick outside its dead zone presses. Mirrors the engine's choice, without its
/// hysteresis, which only matters at the very edge of a sector.
fn directions(x: f32, y: f32, settings: &DirectionSettings) -> Vec<Direction> {
    // 0° is right, 90° is up.
    let angle = (-y).atan2(x).to_degrees().rem_euclid(360.0);
    let half_diagonal = settings.diagonal_size.clamp(0.0, 90.0) / 2.0;
    let diagonals = [
        (45.0, Direction::UpRight, [Direction::Up, Direction::Right]),
        (135.0, Direction::UpLeft, [Direction::Up, Direction::Left]),
        (225.0, Direction::DownLeft, [Direction::Down, Direction::Left]),
        (315.0, Direction::DownRight, [Direction::Down, Direction::Right]),
    ];
    for (centre, diagonal, pair) in diagonals {
        if (angle - centre).abs() <= half_diagonal {
            return if settings.eight_way { vec![diagonal] } else { pair.to_vec() };
        }
    }
    let cardinal = match ((angle + 45.0) / 90.0) as u32 % 4 {
        0 => Direction::Right,
        1 => Direction::Up,
        2 => Direction::Left,
        _ => Direction::Down,
    };
    vec![cardinal]
}

fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Up => "up",
        Direction::Down => "down",
        Direction::Left => "left",
        Direction::Right => "right",
        Direction::UpLeft => "up-left",
        Direction::UpRight => "up-right",
        Direction::DownLeft => "down-left",
        Direction::DownRight => "down-right",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasd() -> Stick {
        let key = |slots: &str| {
            Some(Assignment { slots: vec![slots.parse().unwrap()], ..Assignment::default() })
        };
        let mut stick = Stick { mode: StickMode::Directions, ..Stick::default() };
        stick.directions.up = key("w");
        stick.directions.left = key("a");
        stick.directions.down = key("s");
        stick.directions.right = key("d");
        stick
    }

    #[test]
    fn stick_captions_say_what_the_stick_presses() {
        let stick = wasd();
        assert_eq!(stick_doing(&stick, 0.1, 0.0), "centred");
        assert_eq!(stick_doing(&stick, 1.0, 0.0), "pressing D");
        assert_eq!(stick_doing(&stick, 0.7, -0.7), "pressing W and D");
        let mut eight = wasd();
        eight.directions.eight_way = true;
        assert_eq!(stick_doing(&eight, -0.7, 0.7), "pushed down-left");
        let mouse = Stick { mode: StickMode::Mouse, ..Stick::default() };
        assert_eq!(stick_doing(&mouse, 0.0, 1.0), "moves the pointer");
        assert_eq!(stick_doing(&Stick::default(), 1.0, 0.0), "off");
    }

    #[test]
    fn steam_gets_its_own_warning() {
        assert!(mapper_warning("Steam").contains("Steam Input"));
        assert_eq!(
            mapper_warning("DS4Windows"),
            "DS4Windows is running and may also be reading this controller, so games may get \
             each press twice."
        );
    }
}
