//! Editors for assignments, sticks, triggers and layers. Each editor works on the profile in
//! place and returns `true` when it changed something.

use std::time::Duration;

use eframe::egui::{self, ComboBox, DragValue, RichText, Slider, Ui};
use zpadder_model::profile::{
    Assignment, Direction, LayerSelect, MIN_DIRECTION_DEAD_ZONE, Profile, SelectMode, Slot, Stick,
    StickMode,
};

use crate::{keyboard, labels, theme};

/// UI state of the assignment editor that outlives a frame.
#[derive(Debug, Default)]
pub(crate) struct EditorState {
    /// The on-screen keyboard window is open.
    pub(crate) picking: bool,
    /// Waiting for a key on the real keyboard.
    pub(crate) capturing: bool,
    /// Ctrl, Shift and Alt as seen last frame, to notice them being pressed.
    held: egui::Modifiers,
    /// A capture has added Ctrl, Shift or Alt and is waiting for the key that goes with it.
    capture_modifier: bool,
}

/// Edits an optional assignment; an emptied assignment becomes `None`. `xbox` is the control's
/// Xbox counterpart ("RB"), when the profile acts as an Xbox controller.
pub(crate) fn optional_assignment(
    ui: &mut Ui,
    slot: &mut Option<Assignment>,
    layers: &[String],
    state: &mut EditorState,
    xbox: Option<&str>,
) -> bool {
    let mut assignment = slot.clone().unwrap_or_default();
    let changed = assignment_editor(ui, &mut assignment, layers, state, xbox);
    if changed {
        *slot = (assignment != Assignment::default()).then_some(assignment);
    }
    changed
}

pub(crate) fn assignment_editor(
    ui: &mut Ui,
    assignment: &mut Assignment,
    layers: &[String],
    state: &mut EditorState,
    xbox: Option<&str>,
) -> bool {
    let before = assignment.clone();

    theme::card(ui, "Does", |ui| {
        if assignment.slots.is_empty() {
            ui.weak("Nothing yet. Pick keys or mouse buttons, or capture a key.");
        }
        let mut remove = None;
        let mut swap = None;
        let count = assignment.slots.len();
        for (i, slot) in assignment.slots.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.weak(format!("{}", i + 1));
                slot_widget(ui, slot);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::icon_button(ui, true, "✖", "Remove").clicked() {
                        remove = Some(i);
                    }
                    if theme::icon_button(ui, i + 1 < count, "↓", "Move down").clicked() {
                        swap = Some((i, i + 1));
                    }
                    if theme::icon_button(ui, i > 0, "↑", "Move up").clicked() {
                        swap = Some((i - 1, i));
                    }
                });
            });
        }
        if let Some(i) = remove {
            assignment.slots.remove(i);
        }
        if let Some((a, b)) = swap {
            assignment.slots.swap(a, b);
        }

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let pick =
                egui::Button::new(RichText::new("Keyboard & mouse…").color(egui::Color32::WHITE))
                    .fill(theme::ACCENT);
            if ui.add(pick).clicked() {
                state.picking = true;
            }
            let capture_label = if state.capturing { "Press a key…" } else { "Capture key" };
            if ui
                .selectable_label(state.capturing, capture_label)
                .on_hover_text(
                    "Press a key on your keyboard to add it. Hold Ctrl, Shift or Alt first for a \
                     combination.",
                )
                .clicked()
            {
                state.capturing = !state.capturing;
                state.capture_modifier = false;
            }
            ui.menu_button("Timing ▾", |ui| {
                let timing = [
                    ("Pause", Slot::Pause(Duration::from_millis(100))),
                    ("Hold zone", Slot::HoldZone(Duration::from_millis(500))),
                    ("Release zone", Slot::ReleaseZone(Duration::from_millis(10))),
                    ("Distance zone", Slot::DistanceZone(50.0)),
                    ("Tap time", Slot::TapTime(Duration::from_millis(50))),
                    ("Cycle", Slot::CycleStop),
                ];
                for (label, slot) in timing {
                    if ui.button(label).clicked() {
                        assignment.slots.push(slot);
                        ui.close();
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.weak("What each does:");
                    theme::docs_link(ui, "docs/profiles.md", "profiles.md");
                });
            });
        });
    });
    // Keys pressed on the real keyboard. With the keyboard window open each one is added, as if
    // its key had been clicked; "Capture key" adds one key, after any Ctrl, Shift or Alt held
    // with it. Typing in a text box is left alone.
    if (state.picking || state.capturing) && !ui.ctx().text_edit_focused() {
        for key in keyboard::pressed_keys(ui, &mut state.held) {
            assignment.slots.push(Slot::Key(key));
            if keyboard::is_modifier(key) {
                state.capture_modifier = state.capturing;
            } else {
                state.capturing = false;
            }
        }
        // A modifier pressed and let go on its own is the captured key.
        if state.capture_modifier && !state.held.any() {
            state.capturing = false;
        }
        if !state.capturing {
            state.capture_modifier = false;
        }
    } else {
        state.held = ui.input(|input| input.modifiers);
    }
    if state.picking {
        let mut open = true;
        // Each editor has its own window, so two editors on one page never share one.
        egui::Window::new("Keyboard & mouse")
            .id(ui.make_persistent_id("keyboard-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(
                    "Click keys, or press them on your keyboard, to add them. Several keys in a \
                     row make a combination.",
                );
                if let Some(slot) = keyboard::picker(ui) {
                    assignment.slots.push(slot);
                }
            });
        state.picking = open;
    }

    let summary = describe(assignment);
    if !summary.is_empty() {
        ui.add_space(6.0);
        theme::card(ui, "In plain words", |ui| {
            for line in summary {
                ui.label(line);
            }
        });
    }

    ui.add_space(6.0);
    theme::card(ui, "Options", |ui| {
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(&mut assignment.name)
                    .hint_text("optional, e.g. Jump")
                    .desired_width(f32::INFINITY),
            );
        });
        let timing = assignment.slots.iter().any(Slot::is_timing);
        let held_switch =
            assignment.layer.as_ref().is_some_and(|select| select.mode == SelectMode::WhileHeld);
        ui.horizontal(|ui| {
            // Toggle cannot go with timing slots or a while-held layer switch. It stays clickable
            // while ticked, so a profile that has both can be put right.
            let why_not = if timing {
                "Toggle cannot be combined with timing slots."
            } else {
                "A while-held layer switch can't also toggle; use two-way instead."
            };
            let can_toggle = assignment.toggle || (!timing && !held_switch);
            ui.add_enabled(can_toggle, egui::Checkbox::new(&mut assignment.toggle, "Toggle"))
                .on_hover_text("A press latches the outputs on; the next press releases them.")
                .on_disabled_hover_text(why_not);
            let mut turbo = assignment.turbo.is_some();
            if ui.checkbox(&mut turbo, "Turbo").changed() {
                assignment.turbo = turbo.then_some(10.0);
            }
            if let Some(rate) = &mut assignment.turbo {
                ui.add(DragValue::new(rate).range(0.5..=60.0).speed(0.1).suffix(" /s"));
            }
            ui.checkbox(&mut assignment.rumble, "Rumble")
                .on_hover_text("Vibrate the controller while this is held.");
        });
        // Shown while the profile acts as an Xbox controller, and also when it is ticked, so it
        // can always be switched off.
        if let Some(xbox) = xbox.or_else(|| assignment.passthrough.then_some("button")) {
            let label = format!("Also press the Xbox controller's {xbox}");
            ui.checkbox(&mut assignment.passthrough, label).on_hover_text(format!(
                "Does what is set above and works as {xbox} on the Xbox controller at the same \
                 time. Without it, games see {xbox} released while this is assigned."
            ));
        }
        ui.horizontal(|ui| {
            ui.label("Layer switch");
            let current =
                assignment.layer.as_ref().map_or("None".to_string(), |s| s.target.clone());
            ComboBox::from_id_salt("layer-target").selected_text(current).show_ui(ui, |ui| {
                if ui.selectable_label(assignment.layer.is_none(), "None").clicked() {
                    assignment.layer = None;
                }
                for name in layers {
                    let selected = assignment.layer.as_ref().is_some_and(|s| &s.target == name);
                    if ui.selectable_label(selected, name).clicked() {
                        // A toggled assignment latches, so its switch starts out two-way.
                        let first = if assignment.toggle {
                            SelectMode::TwoWay
                        } else {
                            SelectMode::WhileHeld
                        };
                        let mode = assignment.layer.as_ref().map_or(first, |s| s.mode);
                        assignment.layer = Some(LayerSelect { target: name.clone(), mode });
                    }
                }
            });
            let toggle = assignment.toggle;
            if let Some(select) = &mut assignment.layer {
                ComboBox::from_id_salt("layer-mode")
                    .selected_text(mode_label(select.mode))
                    .show_ui(ui, |ui| {
                        for mode in [SelectMode::WhileHeld, SelectMode::OneWay, SelectMode::TwoWay]
                        {
                            let allowed =
                                !(toggle && mode == SelectMode::WhileHeld) || select.mode == mode;
                            ui.add_enabled_ui(allowed, |ui| {
                                ui.selectable_value(&mut select.mode, mode, mode_label(mode))
                                    .on_disabled_hover_text(
                                        "A toggled assignment can't switch layers while held; \
                                         use two-way instead.",
                                    );
                            });
                        }
                    });
            }
        });
    });

    if *assignment != Assignment::default() {
        ui.add_space(4.0);
        let clear =
            egui::Button::new(RichText::new("Clear assignment").color(theme::error(ui.visuals())));
        if ui.add(clear).clicked() {
            *assignment = Assignment::default();
            state.capturing = false;
        }
    }
    *assignment != before
}

fn mode_label(mode: SelectMode) -> &'static str {
    match mode {
        SelectMode::WhileHeld => "while held",
        SelectMode::OneWay => "one-way",
        SelectMode::TwoWay => "two-way",
    }
}

/// One slot: outputs as a label, timing slots with their value editable in place.
fn slot_widget(ui: &mut Ui, slot: &mut Slot) {
    let seconds = |ui: &mut Ui, label: &str, duration: &mut Duration, hint: &str| {
        let mut value = duration.as_secs_f64();
        ui.label(RichText::new(label).italics()).on_hover_text(hint);
        let response = ui.add(
            DragValue::new(&mut value)
                .range(0.0..=600.0)
                .speed(0.01)
                .fixed_decimals(2)
                .suffix(" s"),
        );
        if response.changed() {
            *duration = Duration::from_millis((value * 1000.0).round() as u64);
        }
    };
    match slot {
        Slot::Pause(d) => seconds(ui, "pause", d, "Wait, then tap what follows (makes a sequence)"),
        Slot::HoldZone(d) => {
            seconds(ui, "hold zone", d, "What follows applies after holding this long")
        }
        Slot::ReleaseZone(d) => {
            seconds(ui, "release zone", d, "What follows is tapped on release, if held this long")
        }
        Slot::TapTime(d) => seconds(ui, "tap time", d, "How long later taps hold their keys"),
        Slot::DistanceZone(percent) => {
            ui.label(RichText::new("distance zone").italics()).on_hover_text(
                "For sticks and triggers: what follows applies beyond this deflection",
            );
            ui.add(DragValue::new(percent).range(0.0..=100.0).speed(0.5).suffix(" %"));
        }
        Slot::CycleStop => {
            ui.label(RichText::new("cycle: next press uses what follows").italics());
        }
        other => {
            let dark = ui.visuals().dark_mode;
            let (fill, text) = if dark {
                (egui::Color32::from_rgb(236, 239, 244), egui::Color32::from_rgb(20, 22, 28))
            } else {
                (egui::Color32::from_rgb(232, 235, 241), egui::Color32::from_rgb(20, 22, 28))
            };
            egui::Frame::new()
                .fill(fill)
                .corner_radius(egui::CornerRadius::same(5))
                .inner_margin(egui::Margin::symmetric(7, 1))
                .show(ui, |ui| {
                    ui.label(RichText::new(labels::short_slot(other)).strong().color(text));
                });
            ui.weak(other.to_string());
        }
    }
}

/// Edits a stick's mode and settings, and which direction is selected for editing. `live` is
/// the stick's current position, drawn over the dead zone. Returns `true` when the stick changed.
/// `xbox` names the Xbox controller's matching stick ("left stick") when the profile acts as
/// one.
pub(crate) fn stick_editor(
    ui: &mut Ui,
    stick: &mut Stick,
    direction: &mut Direction,
    live: Option<(f32, f32)>,
    xbox: Option<&str>,
) -> bool {
    let before = stick.clone();
    ui.horizontal(|ui| {
        ui.label("Mode");
        ui.selectable_value(&mut stick.mode, StickMode::Off, "Off");
        ui.selectable_value(&mut stick.mode, StickMode::Mouse, "Mouse");
        ui.selectable_value(&mut stick.mode, StickMode::Directions, "Directions");
        ui.selectable_value(&mut stick.mode, StickMode::Scroll, "Scroll");
    });
    // In mode Off the stick always goes to the Xbox controller, so the option only matters in
    // the other modes (or to switch it off).
    if let Some(xbox) = xbox
        && (stick.mode != StickMode::Off || stick.passthrough)
    {
        ui.checkbox(&mut stick.passthrough, format!("Also move the Xbox controller's {xbox}"))
            .on_hover_text(format!(
                "Does what the mode says and moves the Xbox controller's {xbox} at the same time."
            ));
    }
    // Directions need a dead zone, or a centred stick's noise would press them.
    let min_dead_zone =
        if stick.mode == StickMode::Directions { MIN_DIRECTION_DEAD_ZONE } else { 0.0 };
    if stick.mode != before.mode && stick.dead_zone < min_dead_zone {
        stick.dead_zone = min_dead_zone;
    }
    if stick.mode != StickMode::Off {
        ui.horizontal(|ui| {
            dead_zone_diagram(ui, stick.dead_zone, live);
            ui.vertical(|ui| {
                ui.add(Slider::new(&mut stick.dead_zone, min_dead_zone..=0.9).text("dead zone"));
                ui.weak("Inside the grey circle the stick counts as centred.");
            });
        });
    }
    match stick.mode {
        StickMode::Scroll => {
            ui.add(Slider::new(&mut stick.scroll.speed, 1.0..=60.0).text("notches per second"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut stick.scroll.horizontal, "Also scroll sideways");
                ui.checkbox(&mut stick.scroll.invert, "Invert");
            });
        }
        StickMode::Off => {
            ui.weak("The stick does nothing. Its click can still be assigned below.");
        }
        StickMode::Mouse => {
            ui.add(
                Slider::new(&mut stick.mouse.speed, 50.0..=5000.0)
                    .logarithmic(true)
                    .text("speed (px/s)"),
            );
            ui.add(Slider::new(&mut stick.mouse.curve, 0.5..=3.0).text("curve"))
                .on_hover_text("1 is linear. Higher values give finer aim near the centre and keep full speed at full tilt.");
            ui.horizontal(|ui| {
                ui.checkbox(&mut stick.mouse.invert_x, "Invert X");
                ui.checkbox(&mut stick.mouse.invert_y, "Invert Y");
            });
        }
        StickMode::Directions => {
            ui.horizontal(|ui| {
                ui.checkbox(&mut stick.directions.eight_way, "8-way").on_hover_text(
                    "Diagonals get their own assignments instead of pressing two directions.",
                );
                ui.add(
                    Slider::new(&mut stick.directions.diagonal_size, 0.0..=90.0).text("diagonal °"),
                );
            });
            if !stick.directions.eight_way && is_diagonal(*direction) {
                *direction = Direction::Up;
            }
            egui::Grid::new("directions").show(ui, |ui| {
                let grid = [
                    [Some(Direction::UpLeft), Some(Direction::Up), Some(Direction::UpRight)],
                    [Some(Direction::Left), None, Some(Direction::Right)],
                    [Some(Direction::DownLeft), Some(Direction::Down), Some(Direction::DownRight)],
                ];
                for row in grid {
                    for cell in row {
                        match cell {
                            Some(d) => {
                                let enabled = stick.directions.eight_way || !is_diagonal(d);
                                let assigned = stick.directions.get(d).is_some();
                                let text = if assigned {
                                    RichText::new(direction_label(d)).strong()
                                } else {
                                    RichText::new(direction_label(d))
                                };
                                let button = egui::Button::selectable(*direction == d, text);
                                if ui.add_enabled(enabled, button).clicked() {
                                    *direction = d;
                                }
                            }
                            None => {
                                ui.label("");
                            }
                        }
                    }
                    ui.end_row();
                }
            });
        }
    }
    *stick != before
}

/// The stick's range, its dead zone and where the stick is now.
fn dead_zone_diagram(ui: &mut Ui, dead_zone: f32, live: Option<(f32, f32)>) {
    let size = 72.0;
    let (rect, painter) = ui.allocate_painter(egui::Vec2::splat(size), egui::Sense::hover());
    let centre = rect.rect.center();
    let radius = size / 2.0 - 3.0;
    let visuals = ui.visuals();
    painter.circle_stroke(centre, radius, egui::Stroke::new(1.0, visuals.weak_text_color()));
    painter.circle_filled(centre, radius * dead_zone, visuals.widgets.inactive.bg_fill);
    if let Some((x, y)) = live {
        let point = centre + egui::vec2(x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)) * radius;
        let inside = x.hypot(y) <= dead_zone;
        let colour =
            if inside { visuals.weak_text_color() } else { egui::Color32::from_rgb(240, 160, 60) };
        painter.circle_filled(point, 4.0, colour);
    }
}

pub(crate) fn direction_slot(stick: &mut Stick, direction: Direction) -> &mut Option<Assignment> {
    let d = &mut stick.directions;
    match direction {
        Direction::Up => &mut d.up,
        Direction::Down => &mut d.down,
        Direction::Left => &mut d.left,
        Direction::Right => &mut d.right,
        Direction::UpLeft => &mut d.up_left,
        Direction::UpRight => &mut d.up_right,
        Direction::DownLeft => &mut d.down_left,
        Direction::DownRight => &mut d.down_right,
    }
}

pub(crate) fn direction_label(direction: Direction) -> &'static str {
    match direction {
        Direction::Up => "Up",
        Direction::Down => "Down",
        Direction::Left => "Left",
        Direction::Right => "Right",
        Direction::UpLeft => "Up-left",
        Direction::UpRight => "Up-right",
        Direction::DownLeft => "Down-left",
        Direction::DownRight => "Down-right",
    }
}

fn is_diagonal(direction: Direction) -> bool {
    matches!(
        direction,
        Direction::UpLeft | Direction::UpRight | Direction::DownLeft | Direction::DownRight
    )
}

/// Why a layer cannot be called `name`, if it cannot: names must be there and be unique, since
/// layer switches find their layer by name.
pub(crate) fn layer_name_problem(
    profile: &Profile,
    index: usize,
    name: &str,
) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        Some("A layer needs a name.")
    } else if profile.layers.iter().enumerate().any(|(i, layer)| i != index && layer.name == name) {
        Some("Another layer already has this name.")
    } else {
        None
    }
}

/// Renames a layer and every selector that points at it. A name that is empty or taken by
/// another layer is refused, so selectors of other layers are never repointed. Returns whether
/// the layer was renamed.
pub(crate) fn rename_layer(profile: &mut Profile, index: usize, new_name: &str) -> bool {
    let new_name = new_name.trim();
    if index >= profile.layers.len() || layer_name_problem(profile, index, new_name).is_some() {
        return false;
    }
    let old = std::mem::replace(&mut profile.layers[index].name, new_name.to_string());
    if old == new_name {
        return false;
    }
    // Names are unique, so the old name pointed at this layer and nothing else.
    for_each_assignment(profile, |assignment| {
        if let Some(select) = &mut assignment.layer
            && select.target == old
        {
            select.target = new_name.to_string();
        }
    });
    true
}

/// Removes a layer and every selector that points at it.
pub(crate) fn remove_layer(profile: &mut Profile, index: usize) {
    let name = profile.layers.remove(index).name;
    for_each_assignment(profile, |assignment| {
        if assignment.layer.as_ref().is_some_and(|select| select.target == name) {
            assignment.layer = None;
        }
    });
}

fn for_each_assignment(profile: &mut Profile, mut f: impl FnMut(&mut Assignment)) {
    for layer in &mut profile.layers {
        layer.buttons.values_mut().for_each(&mut f);
        layer.left_trigger.iter_mut().for_each(&mut f);
        layer.right_trigger.iter_mut().for_each(&mut f);
        for stick in [&mut layer.left_stick, &mut layer.right_stick] {
            for direction in [
                Direction::Up,
                Direction::Down,
                Direction::Left,
                Direction::Right,
                Direction::UpLeft,
                Direction::UpRight,
                Direction::DownLeft,
                Direction::DownRight,
            ] {
                direction_slot(stick, direction).iter_mut().for_each(&mut f);
            }
        }
    }
}

/// Plain-English summary of what an assignment does, like Xpadder's "schedule text".
pub(crate) fn describe(assignment: &Assignment) -> Vec<String> {
    let mut lines = Vec::new();
    let alternatives: Vec<&[Slot]> =
        assignment.slots.split(|slot| matches!(slot, Slot::CycleStop)).collect();
    for (i, alternative) in alternatives.iter().enumerate() {
        let prefix = if alternatives.len() > 1 {
            format!("Press {} of {}: ", i + 1, alternatives.len())
        } else {
            String::new()
        };
        let releases: Vec<Duration> = alternative
            .iter()
            .filter_map(|slot| if let Slot::ReleaseZone(d) = slot { Some(*d) } else { None })
            .collect();
        let mut segments: Vec<(String, bool, Vec<Slot>)> =
            vec![("On press".to_string(), false, Vec::new())];
        for slot in *alternative {
            match *slot {
                Slot::HoldZone(d) => {
                    segments.push((format!("After holding {}", secs(d)), false, Vec::new()));
                }
                Slot::ReleaseZone(d) => {
                    let label = match releases.iter().find(|&&next| next > d) {
                        Some(&next) => format!("On release after {}–{}", secs(d), secs(next)),
                        None => format!("On release after {} or longer", secs(d)),
                    };
                    segments.push((label, true, Vec::new()));
                }
                Slot::DistanceZone(percent) => {
                    segments.push((format!("Pushed beyond {percent}%"), false, Vec::new()));
                }
                other => segments.last_mut().expect("never empty").2.push(other),
            }
        }
        for (label, on_release, slots) in segments {
            if slots.iter().any(|slot| !slot.is_timing()) {
                lines.push(format!("{prefix}{label}: {}", body(&slots, on_release)));
            }
        }
    }
    if assignment.toggle {
        lines.push("Toggle: stays pressed until the next press.".into());
    }
    if let Some(rate) = assignment.turbo {
        lines.push(format!("Turbo: repeats {rate} times a second while held."));
    }
    if assignment.rumble {
        lines.push("Rumbles while held.".into());
    }
    if let Some(select) = &assignment.layer {
        let how = match select.mode {
            SelectMode::WhileHeld => "while held",
            SelectMode::OneWay => "and stays there",
            SelectMode::TwoWay => "; press again to switch back",
        };
        lines.push(format!("Switches to layer \"{}\" {how}.", select.target));
    }
    lines
}

/// "hold a + b", or for sequences "tap a, wait 0.2 s, tap b + c".
fn body(slots: &[Slot], on_release: bool) -> String {
    let sequence =
        on_release || slots.iter().any(|slot| matches!(slot, Slot::Pause(_) | Slot::TapTime(_)));
    let mut parts = Vec::new();
    let mut group: Vec<String> = Vec::new();
    let flush = |group: &mut Vec<String>, parts: &mut Vec<String>| {
        if !group.is_empty() {
            let verb = if sequence { "tap" } else { "hold" };
            parts.push(format!("{verb} {}", group.join(" + ")));
            group.clear();
        }
    };
    for slot in slots {
        match *slot {
            Slot::Pause(d) => {
                flush(&mut group, &mut parts);
                parts.push(format!("wait {}", secs(d)));
            }
            Slot::TapTime(d) => {
                flush(&mut group, &mut parts);
                parts.push(format!("taps last {}", secs(d)));
            }
            other => group.push(other.to_string()),
        }
    }
    flush(&mut group, &mut parts);
    parts.join(", then ")
}

fn secs(d: Duration) -> String {
    format!("{} s", d.as_millis() as f64 / 1000.0)
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
    fn describes_tap_or_hold() {
        let lines = describe(&assignment("hold 0.5s, y, release 0.01s, x, release 0.25s"));
        assert_eq!(lines, ["After holding 0.5 s: hold y", "On release after 0.01 s–0.25 s: tap x"]);
    }

    #[test]
    fn renaming_a_layer_never_takes_another_layers_name() {
        use zpadder_model::profile::Layer;

        let switch = |target: &str| Assignment {
            layer: Some(LayerSelect { target: target.into(), mode: SelectMode::WhileHeld }),
            ..Assignment::default()
        };
        let mut main = Layer::new("Main");
        main.buttons.insert(zpadder_model::Button::South, switch("Menu"));
        main.buttons.insert(zpadder_model::Button::East, switch("Map"));
        let mut profile = Profile {
            name: "Test".into(),
            layout: None,
            virtual_pad: None,
            layers: vec![main, Layer::new("Menu"), Layer::new("Map")],
        };
        let target = |profile: &Profile, button| {
            profile.layers[0].buttons[&button].layer.as_ref().unwrap().target.clone()
        };
        // "Menu" cannot become "Map" or nothing; selectors stay where they were.
        assert!(!rename_layer(&mut profile, 1, "Map"));
        assert!(!rename_layer(&mut profile, 1, "  "));
        assert_eq!(
            layer_name_problem(&profile, 1, "Map"),
            Some("Another layer already has this name.")
        );
        assert_eq!(target(&profile, zpadder_model::Button::South), "Menu");
        // A free name renames the layer and its selectors only.
        assert!(rename_layer(&mut profile, 1, " Pause menu "));
        assert_eq!(profile.layers[1].name, "Pause menu");
        assert_eq!(target(&profile, zpadder_model::Button::South), "Pause menu");
        assert_eq!(target(&profile, zpadder_model::Button::East), "Map");
        assert_eq!(layer_name_problem(&profile, 1, "Pause menu"), None);
    }

    #[test]
    fn describes_sequences_cycles_and_modifiers() {
        let mut a = assignment("a, pause 0.2s, b, c, cycle, left_shift");
        a.turbo = Some(10.0);
        let lines = describe(&a);
        assert_eq!(lines[0], "Press 1 of 2: On press: tap a, then wait 0.2 s, then tap b + c");
        assert_eq!(lines[1], "Press 2 of 2: On press: hold left_shift");
        assert_eq!(lines[2], "Turbo: repeats 10 times a second while held.");
    }
}
