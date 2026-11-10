//! The callout sheet: the controller with every label in a column beside it, joined to its
//! control by a line, like a printed manual. Nothing is shortened, and the sheet can be saved as
//! an image.
//!
//! Controls left of the picture's middle get their callouts in the left column and the rest in
//! the right one. Each column keeps its controls' top-to-bottom order and is spread out so no
//! two callouts overlap ([`place`]). The D-pad shares one callout, and so does each stick with
//! its click. Controls that do nothing get no callout: they are dashed rings on the picture,
//! listed under it.

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::text::LayoutJob;
use eframe::egui::{
    self, Align, Color32, ColorImage, CornerRadius, FontId, Galley, Id, Painter, Pos2, Rect, Sense,
    Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use zpadder_model::Button;
use zpadder_model::profile::{Assignment, Direction, Layer, SelectMode, Slot, Stick, StickMode};
use zpadder_runtime::PadStatus;

use crate::labels::{self, Tone};
use crate::pad_view::{self, Area, Colors, Family, Part, Pick, Side, ViewState};
use crate::theme;

/// The picture's room at full size, in points; it keeps its shape inside this box.
const PICTURE_BOX: Vec2 = vec2(440.0, 340.0);
/// How much bigger than [`PICTURE_BOX`] the picture may grow when the window has room, and how
/// much smaller it gets before the text shrinks too.
const MAX_GROWTH: f32 = 1.6;
const MIN_GROWTH: f32 = 0.7;
/// The smallest the sheet shrinks to; below that it scrolls instead.
const MIN_SCALE: f32 = 0.75;
/// A column is as wide as its widest callout, within these bounds; longer text wraps.
const COLUMN_MIN: f32 = 130.0;
const COLUMN_MAX: f32 = 260.0;
/// Space between a column and the picture, for the leader lines.
const LEADER_ROOM: f32 = 40.0;
/// Space between callouts in a column.
const CALLOUT_GAP: f32 = 8.0;
/// Margin inside the sheet's edge.
const MARGIN: f32 = 16.0;
/// Height kept free below the sheet for the save button and the app's status line.
const BELOW: f32 = 56.0;

/// A callout: one control, or a group like the D-pad, and what it does.
struct Callout {
    /// The control's name, e.g. "L2" or "D-pad".
    title: String,
    /// What a click on the title selects.
    pick: Pick,
    /// The parts it covers, to highlight it along with them.
    parts: Vec<Part>,
    /// Where its leader line ends, in picture coordinates.
    anchor: Pos2,
    side: Side,
    rows: Vec<Row>,
}

/// One line of a callout: what one control or stick direction does.
struct Row {
    /// Written before the row: an arrow for a direction, "Click:" for a stick click.
    lead: Option<&'static str>,
    /// The assignment's name, if it has one.
    name: Option<String>,
    items: Vec<Item>,
    pick: Pick,
    /// Stands for its own part or direction, so it is highlighted on its own when selected.
    own: bool,
    /// Held down on the live pad.
    pressed: bool,
    tooltip: String,
}

/// Something in a row, after its name.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    /// A keycap chip.
    Cap(String, Tone),
    /// Words between or after keycaps: "+", "wait 0.1 s", "1200 px/s", "Free".
    Note(String),
    /// A modifier, drawn as a small outlined tag: "Turbo", "Toggle", "Rumble".
    Badge(&'static str),
}

/// The smallest the sheet shrinks to when it is saved as an image, to fit the window whole.
const EXPORT_MIN_SCALE: f32 = 0.3;

/// Shows the sheet and returns the control clicked, if any.
pub(crate) fn show(ui: &mut Ui, state: &ViewState<'_>) -> Option<Pick> {
    let colors = Colors::new(ui);
    let mut save = Save::begin(ui);
    let room = vec2(ui.available_width(), (ui.available_height() - BELOW).max(160.0));
    let cache = Id::unique("callout-sheet-arrangement");
    let mut sheet = Sheet::new(ui, state, &colors, room, MIN_SCALE, Some(cache));
    let mut pick = None;
    egui::ScrollArea::both()
        .id_salt("callout-sheet")
        .auto_shrink([false, true])
        .max_height(room.y)
        .show(ui, |ui| {
            let size = sheet.arrangement.size;
            let width = ui.available_width().max(size.x);
            let (rect, response) = ui.allocate_exact_size(vec2(width, size.y), Sense::click());
            sheet.origin = vec2((rect.center().x - size.x / 2.0).round(), rect.min.y);
            pick = sheet.interact(ui, state, &colors, response);
        });
    if save.asked.is_some() {
        save.rect = Some(draw_for_export(ui, state, &colors));
    }
    save.finish(ui);
    pick
}

/// Draws the whole sheet over the window, scaled to fit it, so a screenshot has all of it even
/// when the sheet scrolls. Returns where it is on screen.
fn draw_for_export(ui: &Ui, state: &ViewState<'_>, colors: &Colors) -> Rect {
    let screen = ui.ctx().content_rect();
    let room = (screen.size() - Vec2::splat(MARGIN * 2.0)).max(Vec2::splat(100.0));
    let mut sheet = Sheet::new(ui, state, colors, room, EXPORT_MIN_SCALE, None);
    let size = sheet.arrangement.size;
    sheet.origin = (screen.center() - size / 2.0).round().to_vec2();
    egui::Area::new(Id::unique("callout-sheet-export"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            let painter = ui.painter_at(screen);
            painter.rect_filled(screen, CornerRadius::ZERO, ui.visuals().panel_fill);
            sheet.draw(&ui.painter_at(sheet.rect()), ui, state, colors, None);
        });
    sheet.rect()
}

/// What the pointer is over.
#[derive(Clone, Copy, PartialEq)]
enum Hover {
    /// A row of a callout: callout and row index.
    Row(usize, usize),
    /// A callout, outside its rows.
    Callout(usize),
    /// A control on the picture.
    Part(Part),
}

/// The sheet for this frame: its callouts, laid out to fit the room there is.
struct Sheet {
    /// The controls on the picture, in picture coordinates.
    spots: Vec<(Part, Area)>,
    /// The part of the picture coordinates that is drawn.
    content: Rect,
    on_picture: bool,
    callouts: Vec<Callout>,
    arrangement: Arc<Arrangement>,
    /// Where the sheet's top-left corner is on screen, once it has a place.
    origin: Vec2,
}

/// A laid-out sheet kept between frames, and what it was laid out from.
#[derive(Clone)]
struct CachedArrangement {
    key: u64,
    arrangement: Arc<Arrangement>,
    /// The pass it was last used in; a sheet not shown for a while is laid out afresh.
    pass: u64,
    /// The font atlas's fill when it was last used. It only drops when the atlas is cleared,
    /// which leaves the old text layouts pointing at glyphs that are gone.
    atlas_fill: f32,
}

impl Sheet {
    /// Lays the sheet out to fit `room`, shrinking the text no further than `min_scale` before
    /// it scrolls. With a `cache` id the layout is kept and reused while nothing it depends on
    /// changes.
    fn new(
        ui: &Ui,
        state: &ViewState<'_>,
        colors: &Colors,
        room: Vec2,
        min_scale: f32,
        cache: Option<Id>,
    ) -> Self {
        let (spots, content) = pad_view::controls(state);
        let content = if content.width() >= 1.0 && content.height() >= 1.0 {
            content
        } else {
            Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0))
        };
        let on_picture = state.picture.is_some();
        let (callouts, free) = callouts(state, &spots, content.center().x, on_picture);
        let footer = (!free.is_empty()).then(|| {
            let lead =
                if state.passthrough { "Free, so they go to the Xbox controller" } else { "Free" };
            format!("{lead}: {}", free.join(", "))
        });
        let lay_out = || {
            Arc::new(arrange_to_fit(
                ui,
                &callouts,
                footer.as_deref(),
                content,
                room,
                min_scale,
                colors,
            ))
        };
        let arrangement = match cache {
            None => lay_out(),
            Some(id) => {
                let ctx = ui.ctx();
                let key = layout_key(ui, &callouts, footer.as_deref(), content, room, min_scale);
                let pass = ctx.cumulative_pass_nr();
                let atlas_fill = ctx.fonts(|fonts| fonts.font_atlas_fill_ratio());
                let cached = ctx
                    .data(|d| d.get_temp::<CachedArrangement>(id))
                    .filter(|c| c.key == key && pass <= c.pass + 1 && atlas_fill >= c.atlas_fill);
                let arrangement = cached.map_or_else(lay_out, |c| c.arrangement);
                let entry = CachedArrangement {
                    key,
                    arrangement: Arc::clone(&arrangement),
                    pass,
                    atlas_fill,
                };
                ctx.data_mut(|d| d.insert_temp(id, entry));
                arrangement
            }
        };
        Self { spots, content, on_picture, callouts, arrangement, origin: Vec2::ZERO }
    }

    /// The whole sheet on screen.
    fn rect(&self) -> Rect {
        Rect::from_min_size(self.origin.to_pos2(), self.arrangement.size)
    }

    /// A callout's box on screen.
    fn callout_rect(&self, i: usize) -> Rect {
        Rect::from_min_size(self.arrangement.tops[i] + self.origin, self.arrangement.laid[i].size)
    }

    /// From picture coordinates to the screen.
    fn to_screen(&self, p: Pos2) -> Pos2 {
        let a = &self.arrangement;
        a.picture.min + self.origin + (p - self.content.min) * a.zoom
    }

    /// From the screen to picture coordinates.
    fn to_picture(&self, p: Pos2) -> Pos2 {
        let a = &self.arrangement;
        self.content.min + (p - self.origin - a.picture.min.to_vec2()).to_vec2() / a.zoom
    }

    /// An area in picture coordinates, on screen.
    fn screen_area(&self, area: Area) -> Area {
        let zoom = self.arrangement.zoom;
        match area {
            Area::Circle(c, r) => Area::Circle(self.to_screen(c), r * zoom),
            Area::Rect(r, rounding) => Area::Rect(
                Rect::from_min_max(self.to_screen(r.min), self.to_screen(r.max)),
                rounding * zoom,
            ),
        }
    }

    /// The callout covering a part.
    fn callout_of(&self, part: Part) -> Option<usize> {
        self.callouts.iter().position(|callout| callout.parts.contains(&part))
    }

    fn hit(&self, pointer: Pos2) -> Option<Hover> {
        if !self.rect().contains(pointer) {
            return None;
        }
        for i in 0..self.callouts.len() {
            let rect = self.callout_rect(i);
            if rect.contains(pointer) {
                let offset = rect.min.to_vec2();
                let row = self.arrangement.laid[i]
                    .rows
                    .iter()
                    .position(|row| row.band.translate(offset).contains(pointer));
                return Some(row.map_or(Hover::Callout(i), |r| Hover::Row(i, r)));
            }
        }
        // Photos mark controls with small rings; give the pointer a little slack.
        let p = self.to_picture(pointer);
        let slack = 6.0 * self.arrangement.style.scale / self.arrangement.zoom;
        self.spots
            .iter()
            .rev()
            .find(|(_, area)| match *area {
                Area::Circle(c, r) => c.distance(p) <= r + slack,
                Area::Rect(r, _) => r.expand(slack).contains(p),
            })
            .map(|&(part, _)| Hover::Part(part))
    }

    /// Draws the sheet and handles the pointer: hover highlights, tooltips and clicks.
    fn interact(
        &self,
        ui: &Ui,
        state: &ViewState<'_>,
        colors: &Colors,
        response: egui::Response,
    ) -> Option<Pick> {
        let hover = response.hover_pos().and_then(|p| self.hit(p));
        if hover.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        self.draw(&ui.painter_at(self.rect()), ui, state, colors, hover);

        let tooltip = match hover {
            Some(Hover::Row(i, r)) => self.callouts[i].rows[r].tooltip.clone(),
            Some(Hover::Callout(i)) => match self.callouts[i].rows.as_slice() {
                [row] => row.tooltip.clone(),
                _ => String::new(),
            },
            Some(Hover::Part(part)) if self.callout_of(part).is_none() => {
                format!("{} is free. Click to assign it.", pad_view::label(state.family, part))
            }
            _ => String::new(),
        };
        let clicked = response.clicked();
        if !tooltip.is_empty() {
            response.on_hover_text_at_pointer(tooltip);
        }
        if !clicked {
            return None;
        }
        match hover? {
            Hover::Row(i, r) => Some(self.callouts[i].rows[r].pick),
            Hover::Callout(i) => Some(self.callouts[i].pick),
            Hover::Part(part) => Some(Pick { part, direction: None }),
        }
    }

    fn draw(
        &self,
        painter: &Painter,
        ui: &Ui,
        state: &ViewState<'_>,
        colors: &Colors,
        hover: Option<Hover>,
    ) {
        let canvas = if self.on_picture {
            theme::picture_canvas_fill(ui.visuals())
        } else {
            theme::canvas_fill(ui.visuals())
        };
        painter.rect_filled(self.rect(), CornerRadius::same(14), canvas);

        // What to highlight: the callout under the pointer and its parts, or the control under
        // the pointer and its callout.
        let (hovered_callout, hovered_parts) = match hover {
            Some(Hover::Row(i, r)) => (Some(i), vec![self.callouts[i].rows[r].pick.part]),
            Some(Hover::Callout(i)) => (Some(i), self.callouts[i].parts.clone()),
            Some(Hover::Part(part)) => (self.callout_of(part), vec![part]),
            None => (None, Vec::new()),
        };

        let picture = Rect::from_min_size(
            self.arrangement.picture.min + self.origin,
            self.arrangement.picture.size(),
        );
        match state.picture {
            Some(photo) => {
                if let Some(texture) = &photo.texture {
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    painter.image(texture.id(), picture, uv, Color32::WHITE);
                }
                self.draw_photo_marks(painter, state, colors, canvas, &hovered_parts);
            }
            None => {
                let to_screen = |p: Pos2| self.to_screen(p);
                pad_view::draw_body(painter, &to_screen, self.arrangement.zoom, colors);
                self.draw_controls(painter, state, colors, &hovered_parts);
            }
        }

        // Leaders, the highlighted ones last so they stay on top.
        let selected_callout = state.selected.and_then(|part| self.callout_of(part));
        let highlighted = |i: usize| hovered_callout == Some(i) || selected_callout == Some(i);
        let line = self.arrangement.style.text.gamma_multiply(0.55);
        for i in (0..self.callouts.len()).filter(|&i| !highlighted(i)) {
            self.draw_leader(painter, i, line, canvas);
        }
        for i in (0..self.callouts.len()).filter(|&i| highlighted(i)) {
            self.draw_leader(painter, i, theme::ACCENT, canvas);
        }

        for i in 0..self.callouts.len() {
            let hovered_row = match hover {
                Some(Hover::Row(c, r)) if c == i => Some(r),
                _ => None,
            };
            self.draw_callout(painter, i, state, colors, hovered_callout == Some(i), hovered_row);
        }

        if let Some((pos, galley)) = &self.arrangement.footer {
            paint_galley(painter, *pos + self.origin, galley.clone(), self.arrangement.style.weak);
        }
    }

    /// The drawn controller's controls: in use ones filled and outlined, free ones as dashed
    /// outlines, lit when pressed on the live pad.
    fn draw_controls(
        &self,
        painter: &Painter,
        state: &ViewState<'_>,
        colors: &Colors,
        hovered: &[Part],
    ) {
        let zoom = self.arrangement.zoom;
        let s = self.arrangement.style.scale;
        for &(part, area) in &self.spots {
            let used = in_use(state, part);
            let pressed = state.live.is_some_and(|pad| pad_view::is_pressed(pad, part));
            let selected = state.selected == Some(part);
            let fill = if pressed {
                colors.pressed
            } else if hovered.contains(&part) {
                colors.hover
            } else if used {
                colors.control
            } else {
                Color32::TRANSPARENT
            };
            let stroke = if selected {
                Some(Stroke::new(3.0 * zoom.max(0.7), theme::ACCENT))
            } else if used {
                Some(Stroke::new(1.5 * zoom.max(0.7), colors.assigned))
            } else {
                None
            };
            let screen = self.screen_area(area);
            match screen {
                Area::Circle(c, r) => {
                    if selected {
                        painter.circle_filled(
                            c,
                            r + 6.0 * zoom,
                            theme::ACCENT.gamma_multiply(0.18),
                        );
                    }
                    painter.circle_filled(c, r, fill);
                    if let Some(stroke) = stroke {
                        painter.circle_stroke(c, r, stroke);
                    }
                    if let Part::Stick(side) = part {
                        pad_view::draw_stick(painter, c, zoom, state, side, colors);
                    }
                }
                Area::Rect(r, rounding) => {
                    if selected {
                        painter.rect_filled(
                            r.expand(5.0 * zoom),
                            CornerRadius::same((rounding + 5.0 * zoom) as u8),
                            theme::ACCENT.gamma_multiply(0.18),
                        );
                    }
                    let radius = CornerRadius::same(rounding as u8);
                    painter.rect_filled(r, radius, fill);
                    if let Some(stroke) = stroke {
                        painter.rect_stroke(r, radius, stroke, StrokeKind::Inside);
                    }
                    if let (Part::Trigger(side), Some(pad)) = (part, state.live) {
                        let value =
                            pad.axes[if side == Side::Left { 4 } else { 5 }].clamp(0.0, 1.0);
                        let mut bar = r.shrink(3.0);
                        bar.set_width(bar.width() * value);
                        painter.rect_filled(
                            bar,
                            CornerRadius::same(4),
                            colors.pressed.gamma_multiply(0.6),
                        );
                    }
                }
            }
            if stroke.is_none() {
                dashed(painter, screen, Stroke::new(1.2, colors.cap.gamma_multiply(0.8)), None, s);
            }
            let text_color = if used { colors.cap } else { colors.cap.gamma_multiply(0.6) };
            let centre = screen.centre();
            let text = pad_view::cap(state.family, part);
            if !text.is_empty() && !matches!(part, Part::Stick(_)) {
                painter.text(
                    centre,
                    egui::Align2::CENTER_CENTER,
                    text,
                    FontId::proportional((13.0 * zoom).max(8.0)),
                    text_color,
                );
            }
            if state.family == Family::PlayStation
                && let Part::Button(
                    button @ (Button::South | Button::East | Button::West | Button::North),
                ) = part
            {
                pad_view::draw_ps_symbol(painter, centre, 9.0 * zoom, button, text_color);
            }
        }
    }

    /// Marks on a photo: dashed rings on free controls, and the selected, hovered and pressed
    /// controls lit.
    fn draw_photo_marks(
        &self,
        painter: &Painter,
        state: &ViewState<'_>,
        colors: &Colors,
        canvas: Color32,
        hovered: &[Part],
    ) {
        let s = self.arrangement.style.scale;
        for &(part, area) in &self.spots {
            let Area::Circle(c, r) = self.screen_area(area) else { continue };
            let r = r.max(7.0 * s);
            let pressed = state.live.is_some_and(|pad| {
                pad_view::is_pressed(pad, part)
                    || matches!(part, Part::Stick(side) if pad.pressed.contains(&click_of(side)))
            });
            if state.selected == Some(part) {
                painter.circle_filled(c, r + 6.0 * s, theme::ACCENT.gamma_multiply(0.18));
                painter.circle_stroke(c, r, Stroke::new(2.5, theme::ACCENT));
            } else if hovered.contains(&part) {
                painter.circle_filled(c, r, colors.hover.gamma_multiply(0.7));
            }
            if pressed {
                painter.circle_filled(c, r, colors.pressed.gamma_multiply(0.7));
            }
            if let (Part::Stick(side), Some(pad)) = (part, state.live) {
                let (x, y) = stick_axes(pad, side);
                if x.hypot(y) > 0.3 {
                    painter.circle_filled(c + vec2(x, y) * r, 4.0 * s, colors.pressed);
                }
            }
            if !in_use(state, part) {
                let stroke = Stroke::new(1.3, self.arrangement.style.text.gamma_multiply(0.8));
                dashed(painter, Area::Circle(c, r), stroke, Some(canvas), s);
            }
        }
    }

    /// The line from a callout to its control: out of the callout level with its title, then
    /// straight to a dot on the control, over a halo of the canvas colour.
    fn draw_leader(&self, painter: &Painter, i: usize, color: Color32, canvas: Color32) {
        let s = self.arrangement.style.scale;
        let rect = self.callout_rect(i);
        let y = rect.min.y + self.arrangement.laid[i].title_mid;
        let (start, elbow) = match self.callouts[i].side {
            Side::Left => (pos2(rect.max.x, y), pos2(rect.max.x + 10.0 * s, y)),
            Side::Right => (pos2(rect.min.x, y), pos2(rect.min.x - 10.0 * s, y)),
        };
        let end = self.to_screen(self.callouts[i].anchor);
        let points = vec![start, elbow, end];
        painter.line(points.clone(), Stroke::new(4.0, canvas));
        painter.line(points, Stroke::new(1.2, color));
        painter.circle_filled(end, 4.6 * s, canvas);
        painter.circle_filled(end, 3.2 * s, color);
    }

    fn draw_callout(
        &self,
        painter: &Painter,
        i: usize,
        state: &ViewState<'_>,
        colors: &Colors,
        hovered: bool,
        hovered_row: Option<usize>,
    ) {
        let callout = &self.callouts[i];
        let laid = &self.arrangement.laid[i];
        let style = &self.arrangement.style;
        let s = style.scale;
        let rect = self.callout_rect(i);
        let offset = rect.min.to_vec2();
        let selected = state.selected.is_some_and(|part| callout.parts.contains(&part));
        let radius = CornerRadius::same((7.0 * s) as u8);
        if selected {
            painter.rect(
                rect,
                radius,
                theme::ACCENT.gamma_multiply(0.08),
                Stroke::new(2.0, theme::ACCENT),
                StrokeKind::Inside,
            );
        } else if hovered {
            painter.rect_filled(rect, radius, style.text.gamma_multiply(0.06));
        }
        let (title_rect, title) = &laid.title;
        paint_galley(painter, title_rect.min + offset, title.clone(), style.weak);

        let band_radius = CornerRadius::same((4.0 * s) as u8);
        for (r, (row, laid_row)) in callout.rows.iter().zip(&laid.rows).enumerate() {
            let band = laid_row.band.translate(offset);
            let row_selected = row.own
                && state.selected == Some(row.pick.part)
                && row.pick.direction.is_none_or(|d| d == state.direction);
            if row.pressed {
                painter.rect_filled(band, band_radius, colors.pressed.gamma_multiply(0.35));
            } else if row_selected {
                painter.rect_filled(band, band_radius, theme::ACCENT.gamma_multiply(0.2));
            } else if hovered_row == Some(r) && callout.rows.len() > 1 {
                painter.rect_filled(band, band_radius, style.text.gamma_multiply(0.06));
            }
            if let Some((lead_rect, lead)) = &laid_row.lead {
                paint_galley(painter, lead_rect.min + offset, lead.clone(), style.weak);
            }
            for piece in &laid_row.pieces {
                let piece_rect = piece.rect.translate(offset);
                match piece.kind {
                    PieceKind::Text => {
                        paint_galley(painter, piece_rect.min, piece.galley.clone(), style.text);
                    }
                    PieceKind::Cap(tone) => {
                        pad_view::draw_chip(
                            painter,
                            piece_rect,
                            piece.galley.clone(),
                            tone,
                            pad_view::ChipLook::default(),
                            s,
                            colors,
                        );
                    }
                    PieceKind::Badge => {
                        painter.rect_stroke(
                            piece_rect,
                            CornerRadius::same((4.0 * s) as u8),
                            Stroke::new(1.0, style.weak.gamma_multiply(0.8)),
                            StrokeKind::Inside,
                        );
                        let pos = piece_rect.center() - piece.galley.size() / 2.0;
                        paint_galley(painter, pos, piece.galley.clone(), style.weak);
                    }
                }
            }
        }
    }
}

/// Lays the sheet out to fit `room`. With full-size text the picture grows when there is room to
/// spare, and shrinks (down to a point) to keep the text readable when there is not. With too
/// little room even then, everything shrinks, down to `min_scale`, and then the sheet scrolls.
fn arrange_to_fit(
    ui: &Ui,
    callouts: &[Callout],
    footer: Option<&str>,
    content: Rect,
    room: Vec2,
    min_scale: f32,
    colors: &Colors,
) -> Arrangement {
    let arrange = |scale: f32, growth: f32| {
        Arrangement::new(ui, callouts, footer, content, scale, growth, colors)
    };
    let natural = arrange(1.0, 1.0);
    let picture = natural.picture.size();
    let other_height = natural.size.y - natural.body_height;
    let by_width = (room.x - (natural.size.x - picture.x)) / picture.x;
    let by_height = if natural.columns_height <= room.y - other_height {
        (room.y - other_height) / picture.y
    } else {
        0.0
    };
    let growth = by_width.min(by_height).min(MAX_GROWTH);
    if (growth - 1.0).abs() < 0.01 {
        natural
    } else if growth >= MIN_GROWTH {
        arrange(1.0, growth)
    } else {
        let small = arrange(1.0, MIN_GROWTH);
        let scale = (room.x / small.size.x).min(room.y / small.size.y);
        arrange(scale.clamp(min_scale, 1.0), MIN_GROWTH)
    }
}

/// A hash of everything the sheet's layout depends on: what each callout says and where it
/// points, the room, and the theme and text size. Live state such as pressed buttons only
/// changes how the sheet is drawn, so it is left out.
fn layout_key(
    ui: &Ui,
    callouts: &[Callout],
    footer: Option<&str>,
    content: Rect,
    room: Vec2,
    min_scale: f32,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let floats = [content.min.x, content.min.y, content.max.x, content.max.y, room.x, room.y];
    for value in floats.into_iter().chain([min_scale, ui.ctx().pixels_per_point()]) {
        value.to_bits().hash(&mut hasher);
    }
    let visuals = ui.visuals();
    (visuals.dark_mode, visuals.text_color().to_array(), visuals.weak_text_color().to_array())
        .hash(&mut hasher);
    footer.hash(&mut hasher);
    for callout in callouts {
        callout.title.hash(&mut hasher);
        (callout.side == Side::Left).hash(&mut hasher);
        (callout.anchor.x.to_bits(), callout.anchor.y.to_bits()).hash(&mut hasher);
        callout.rows.len().hash(&mut hasher);
        for row in &callout.rows {
            (row.lead, &row.name).hash(&mut hasher);
            for item in &row.items {
                match item {
                    Item::Cap(text, tone) => (0u8, text, *tone as u8).hash(&mut hasher),
                    Item::Note(text) => (1u8, text).hash(&mut hasher),
                    Item::Badge(text) => (2u8, text).hash(&mut hasher),
                }
            }
        }
    }
    hasher.finish()
}

/// Draws an area's outline dashed, over a dashed halo when `halo` is given so it shows on any
/// picture.
fn dashed(painter: &Painter, area: Area, stroke: Stroke, halo: Option<Color32>, scale: f32) {
    let points = outline(area);
    let (dash, gap) = (4.0 * scale, 3.0 * scale);
    if let Some(halo) = halo {
        let wide = Stroke::new(stroke.width + 2.5, halo);
        painter.extend(Shape::dashed_line(&points, wide, dash, gap));
    }
    painter.extend(Shape::dashed_line(&points, stroke, dash, gap));
}

/// Points around an area's outline, back to where they started.
fn outline(area: Area) -> Vec<Pos2> {
    match area {
        Area::Circle(c, r) => {
            let n = ((r * TAU / 3.0) as usize).clamp(16, 120);
            (0..=n).map(|i| c + Vec2::angled(i as f32 / n as f32 * TAU) * r).collect()
        }
        Area::Rect(rect, rounding) => {
            let r = rounding.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
            let corners = [
                (pos2(rect.max.x - r, rect.min.y + r), -90.0_f32),
                (pos2(rect.max.x - r, rect.max.y - r), 0.0),
                (pos2(rect.min.x + r, rect.max.y - r), 90.0),
                (pos2(rect.min.x + r, rect.min.y + r), 180.0),
            ];
            let mut points: Vec<Pos2> = corners
                .iter()
                .flat_map(|&(centre, start)| {
                    (0..=6).map(move |k| {
                        centre + Vec2::angled((start + k as f32 * 15.0).to_radians()) * r
                    })
                })
                .collect();
            points.push(points[0]);
            points
        }
    }
}

/// Draws a galley with the top-left corner of its text at `min`, whichever way it is aligned.
fn paint_galley(painter: &Painter, min: Pos2, galley: Arc<Galley>, color: Color32) {
    painter.galley(min - galley.rect.min.to_vec2(), galley, color);
}

// ------------------------------------------------------------------------------------------
// What the callouts say.

/// The assignment, unless it does nothing.
fn active(assignment: Option<&Assignment>) -> Option<&Assignment> {
    assignment.filter(|a| **a != Assignment::default())
}

fn stick_of(layer: &Layer, side: Side) -> &Stick {
    match side {
        Side::Left => &layer.left_stick,
        Side::Right => &layer.right_stick,
    }
}

fn click_of(side: Side) -> Button {
    match side {
        Side::Left => Button::LeftStick,
        Side::Right => Button::RightStick,
    }
}

fn stick_axes(pad: &PadStatus, side: Side) -> (f32, f32) {
    match side {
        Side::Left => (pad.axes[0], pad.axes[1]),
        Side::Right => (pad.axes[2], pad.axes[3]),
    }
}

/// Whether a part does anything in this layer, so it gets a callout.
fn in_use(state: &ViewState<'_>, part: Part) -> bool {
    let layer = state.layer;
    match part {
        Part::Button(button) => active(layer.buttons.get(&button)).is_some(),
        Part::Trigger(Side::Left) => active(layer.left_trigger.as_ref()).is_some(),
        Part::Trigger(Side::Right) => active(layer.right_trigger.as_ref()).is_some(),
        Part::Stick(side) => {
            stick_of(layer, side).mode != StickMode::Off
                || active(layer.buttons.get(&click_of(side))).is_some()
        }
    }
}

const DPAD: [(Button, &str); 4] = [
    (Button::DPadUp, "↑"),
    (Button::DPadRight, "→"),
    (Button::DPadDown, "↓"),
    (Button::DPadLeft, "←"),
];

/// Stick directions in the order their rows are listed: clockwise from up.
const DIRECTIONS: [(Direction, &str); 8] = [
    (Direction::Up, "↑"),
    (Direction::UpRight, "↗"),
    (Direction::Right, "→"),
    (Direction::DownRight, "↘"),
    (Direction::Down, "↓"),
    (Direction::DownLeft, "↙"),
    (Direction::Left, "←"),
    (Direction::UpLeft, "↖"),
];

/// The callouts for the controls in use, and the names of those that do nothing. Controls left
/// of `middle` get their callouts on the left. Leaders end on photos' marks, and at the edge of
/// the drawn controller's controls so the caps printed on them stay readable.
fn callouts(
    state: &ViewState<'_>,
    spots: &[(Part, Area)],
    middle: f32,
    on_picture: bool,
) -> (Vec<Callout>, Vec<String>) {
    let side_of = |x: f32| if x < middle { Side::Left } else { Side::Right };
    let anchor = |area: Area, side: Side| match (on_picture, area, side) {
        (true, _, _) => area.centre(),
        (false, Area::Circle(c, r), Side::Left) => c - vec2(r, 0.0),
        (false, Area::Circle(c, r), Side::Right) => c + vec2(r, 0.0),
        (false, Area::Rect(r, _), Side::Left) => r.left_center(),
        (false, Area::Rect(r, _), Side::Right) => r.right_center(),
    };
    let mut callouts = Vec::new();
    let mut free = Vec::new();
    let mut dpad_done = false;
    for &(part, area) in spots {
        if let Part::Button(button) = part
            && DPAD.iter().any(|&(b, _)| b == button)
        {
            if !dpad_done {
                dpad_done = true;
                match dpad_callout(state, spots, side_of) {
                    Some(callout) => callouts.push(callout),
                    None => free.push("D-pad".to_string()),
                }
            }
            continue;
        }
        if !in_use(state, part) {
            free.push(pad_view::label(state.family, part));
            continue;
        }
        let side = side_of(area.centre().x);
        let pick = Pick { part, direction: None };
        let rows = match part {
            Part::Stick(stick) => stick_rows(state, stick),
            Part::Button(button) => {
                let pressed = state.live.is_some_and(|pad| pad.pressed.contains(&button));
                vec![assignment_row(state.layer.buttons.get(&button), None, pick, false, pressed)]
            }
            Part::Trigger(trigger) => {
                let assignment = match trigger {
                    Side::Left => state.layer.left_trigger.as_ref(),
                    Side::Right => state.layer.right_trigger.as_ref(),
                };
                let pressed = state.live.is_some_and(|pad| pad_view::is_pressed(pad, part));
                vec![assignment_row(assignment, None, pick, false, pressed)]
            }
        };
        callouts.push(Callout {
            title: pad_view::label(state.family, part),
            pick,
            parts: vec![part],
            anchor: anchor(area, side),
            side,
            rows,
        });
    }
    (callouts, free)
}

/// The D-pad's callout: a row per direction, or `None` if none of them does anything.
fn dpad_callout(
    state: &ViewState<'_>,
    spots: &[(Part, Area)],
    side_of: impl Fn(f32) -> Side,
) -> Option<Callout> {
    let found: Vec<(Button, &'static str, Area)> = DPAD
        .iter()
        .filter_map(|&(button, arrow)| {
            let area = spots.iter().find(|(part, _)| *part == Part::Button(button))?.1;
            Some((button, arrow, area))
        })
        .collect();
    // A click on the title picks the first direction in use.
    let &(first, _, _) =
        found.iter().find(|&&(button, _, _)| in_use(state, Part::Button(button)))?;
    let anchor = found.iter().fold(Vec2::ZERO, |sum, (_, _, area)| sum + area.centre().to_vec2())
        / found.len() as f32;
    let rows: Vec<Row> = found
        .iter()
        .map(|&(button, arrow, _)| {
            let pick = Pick { part: Part::Button(button), direction: None };
            let pressed = state.live.is_some_and(|pad| pad.pressed.contains(&button));
            assignment_row(state.layer.buttons.get(&button), Some(arrow), pick, true, pressed)
        })
        .collect();
    Some(Callout {
        title: "D-pad".into(),
        pick: Pick { part: Part::Button(first), direction: None },
        parts: found.iter().map(|&(button, _, _)| Part::Button(button)).collect(),
        anchor: anchor.to_pos2(),
        side: side_of(anchor.x),
        rows,
    })
}

/// A stick's rows: what its directions do (or its pointer or scroll settings), its dead zone,
/// and its click.
fn stick_rows(state: &ViewState<'_>, side: Side) -> Vec<Row> {
    let stick = stick_of(state.layer, side);
    let part = Part::Stick(side);
    let pick = Pick { part, direction: None };
    let axes = state.live.map(|pad| stick_axes(pad, side));
    let pushed = axes.is_some_and(|(x, y)| x.hypot(y) > stick.dead_zone.max(0.3));
    let mut rows = Vec::new();
    let summary = |items: Vec<Item>, tooltip: String| Row {
        lead: None,
        name: None,
        items,
        pick,
        own: false,
        pressed: pushed,
        tooltip,
    };
    match stick.mode {
        StickMode::Off => {}
        StickMode::Mouse => {
            let mouse = &stick.mouse;
            let mut details = vec![format!("{} px/s", mouse.speed)];
            if mouse.curve != 1.0 {
                details.push(format!("curve {}", mouse.curve));
            }
            match (mouse.invert_x, mouse.invert_y) {
                (true, true) => details.push("inverted".into()),
                (true, false) => details.push("X inverted".into()),
                (false, true) => details.push("Y inverted".into()),
                (false, false) => {}
            }
            let items =
                vec![Item::Cap("Mouse".into(), Tone::Mouse), Item::Note(details.join(", "))];
            rows.push(summary(items, format!("Moves the pointer, up to {} px/s", mouse.speed)));
        }
        StickMode::Scroll => {
            let scroll = &stick.scroll;
            let mut details = vec![format!("{} notches/s", scroll.speed)];
            if scroll.horizontal {
                details.push("sideways too".into());
            }
            if scroll.invert {
                details.push("inverted".into());
            }
            let items =
                vec![Item::Cap("Scroll".into(), Tone::Mouse), Item::Note(details.join(", "))];
            rows.push(summary(items, format!("Scrolls, up to {} notches/s", scroll.speed)));
        }
        StickMode::Directions => {
            let held = axes.map_or_else(Vec::new, |(x, y)| stick_directions(x, y, stick));
            for (direction, arrow) in DIRECTIONS {
                let diagonal = !matches!(
                    direction,
                    Direction::Up | Direction::Down | Direction::Left | Direction::Right
                );
                if diagonal && !stick.directions.eight_way {
                    continue;
                }
                let pick = Pick { part, direction: Some(direction) };
                let pressed = held.contains(&direction);
                let assignment = stick.directions.get(direction);
                rows.push(assignment_row(assignment, Some(arrow), pick, true, pressed));
            }
        }
    }
    if stick.mode != StickMode::Off {
        let dead_zone = format!("Dead zone {}%", (stick.dead_zone * 100.0).round());
        let mut row = summary(vec![Item::Note(dead_zone)], String::new());
        row.pressed = false;
        rows.push(row);
    }
    let click = click_of(side);
    let pressed = state.live.is_some_and(|pad| pad.pressed.contains(&click));
    let mut row =
        assignment_row(state.layer.buttons.get(&click), Some("Click:"), pick, false, pressed);
    if !row.tooltip.is_empty() {
        row.tooltip = format!("Stick click\n{}", row.tooltip);
    }
    rows.push(row);
    rows
}

/// A row for one assignment: its name, keycaps and modifiers, or "Free" when it does nothing.
/// The tooltip is the one the controller view's chip has.
fn assignment_row(
    assignment: Option<&Assignment>,
    lead: Option<&'static str>,
    pick: Pick,
    own: bool,
    pressed: bool,
) -> Row {
    let assignment = active(assignment);
    let name =
        assignment.map(|a| a.name.trim()).filter(|name| !name.is_empty()).map(str::to_string);
    let items = assignment.map_or_else(|| vec![Item::Note("Free".into())], assignment_items);
    let tooltip = assignment
        .and_then(|a| labels::chip(a, false))
        .map(|chip| chip.tooltip)
        .unwrap_or_default();
    Row { lead, name, items, pick, own, pressed, tooltip }
}

/// An assignment's keycaps in order, with the timing between them in words, then its layer
/// switch and its modifiers. Keycaps use the controller view's chip colours: keys, mouse, layer
/// switch, or timed when the assignment has timing slots.
fn assignment_items(assignment: &Assignment) -> Vec<Item> {
    let timed = assignment.slots.iter().any(Slot::is_timing);
    let mut items = Vec::new();
    let mut after_output = false;
    for slot in &assignment.slots {
        let words = match *slot {
            Slot::Key(_) | Slot::MouseButton(_) | Slot::Wheel(_) => None,
            Slot::Pause(d) => Some(format!("wait {}", secs(d))),
            Slot::HoldZone(d) => Some(format!("held {}:", secs(d))),
            Slot::ReleaseZone(d) => Some(format!("released after {}:", secs(d))),
            Slot::DistanceZone(percent) => Some(format!("beyond {percent}%:")),
            Slot::TapTime(d) => Some(format!("taps last {}", secs(d))),
            Slot::CycleStop => Some("next press:".into()),
        };
        match words {
            Some(words) => {
                items.push(Item::Note(words));
                after_output = false;
            }
            None => {
                if after_output {
                    items.push(Item::Note("+".into()));
                }
                let tone = if timed {
                    Tone::Macro
                } else if matches!(slot, Slot::MouseButton(_) | Slot::Wheel(_)) {
                    Tone::Mouse
                } else {
                    Tone::Key
                };
                items.push(Item::Cap(labels::short_slot(slot), tone));
                after_output = true;
            }
        }
    }
    if let Some(select) = &assignment.layer {
        let arrow = match select.mode {
            SelectMode::WhileHeld => "⇧",
            SelectMode::OneWay => "→",
            SelectMode::TwoWay => "⇄",
        };
        items.push(Item::Cap(format!("{arrow} {}", select.target), Tone::Layer));
    }
    if assignment.turbo.is_some() {
        items.push(Item::Badge("Turbo"));
    }
    if assignment.toggle {
        items.push(Item::Badge("Toggle"));
    }
    if assignment.rumble {
        items.push(Item::Badge("Rumble"));
    }
    // Words that start the row start with a capital.
    if let Some(Item::Note(first)) = items.first_mut()
        && let Some(c) = first.chars().next()
    {
        *first = c.to_uppercase().chain(first.chars().skip(1)).collect();
    }
    items
}

fn secs(d: Duration) -> String {
    format!("{} s", d.as_millis() as f64 / 1000.0)
}

/// The directions a stick pushed to `(x, y)` (y down) presses, read the way the engine does.
fn stick_directions(x: f32, y: f32, stick: &Stick) -> Vec<Direction> {
    let magnitude = x.hypot(y);
    if magnitude == 0.0 || magnitude < stick.dead_zone {
        return Vec::new();
    }
    // 0° is right, 90° is up.
    let angle = (-y).atan2(x).to_degrees().rem_euclid(360.0);
    let half_diagonal = stick.directions.diagonal_size.clamp(0.0, 90.0) / 2.0;
    let diagonals = [
        (45.0, Direction::UpRight, [Direction::Up, Direction::Right]),
        (135.0, Direction::UpLeft, [Direction::Up, Direction::Left]),
        (225.0, Direction::DownLeft, [Direction::Down, Direction::Left]),
        (315.0, Direction::DownRight, [Direction::Down, Direction::Right]),
    ];
    for (centre, diagonal, pair) in diagonals {
        if (angle - centre).abs() <= half_diagonal {
            return if stick.directions.eight_way { vec![diagonal] } else { pair.to_vec() };
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

// ------------------------------------------------------------------------------------------
// Layout.

/// Fonts and colours for one size of the sheet.
struct Style {
    /// Points per point at full size.
    scale: f32,
    title: FontId,
    lead: FontId,
    name: FontId,
    cap: FontId,
    note: FontId,
    badge: FontId,
    text: Color32,
    weak: Color32,
}

impl Style {
    fn new(ui: &Ui, scale: f32) -> Self {
        let font = |size: f32| FontId::proportional(size * scale);
        Self {
            scale,
            title: font(12.0),
            lead: font(13.0),
            name: font(13.5),
            cap: font(13.0),
            note: font(12.0),
            badge: font(10.5),
            text: ui.visuals().text_color(),
            weak: ui.visuals().weak_text_color(),
        }
    }
}

/// How a laid-out piece of a row is drawn.
#[derive(Clone, Copy)]
enum PieceKind {
    /// A name or words, in the galley's own colour.
    Text,
    /// A keycap chip.
    Cap(Tone),
    /// An outlined tag.
    Badge,
}

struct Piece {
    kind: PieceKind,
    galley: Arc<Galley>,
    /// Relative to the callout's top-left corner (to the row's while it is being laid out).
    rect: Rect,
}

struct LaidRow {
    /// The row's full width, for highlights and hit-testing, relative to the callout.
    band: Rect,
    lead: Option<(Rect, Arc<Galley>)>,
    pieces: Vec<Piece>,
}

/// A callout's text laid out, relative to its top-left corner.
struct Laid {
    size: Vec2,
    title: (Rect, Arc<Galley>),
    /// Middle of the title line, where the leader line leaves.
    title_mid: f32,
    rows: Vec<LaidRow>,
}

/// Lays text out at most `wrap` wide, aligned left or right.
fn text_galley(
    painter: &Painter,
    text: &str,
    font: FontId,
    color: Color32,
    wrap: f32,
    align: Align,
) -> Arc<Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), font, color, wrap);
    job.halign = align;
    painter.layout_job(job)
}

impl Laid {
    /// Lays a callout out: its title over its rows, each row's lead first, then its pieces
    /// flowed into lines no wider than a column allows. In the left column text is flush right
    /// and keycaps come after the name, so in both columns they are nearest the picture.
    fn new(painter: &Painter, callout: &Callout, style: &Style, colors: &Colors) -> Self {
        let s = style.scale;
        let padding = vec2(8.0, 5.0) * s;
        let flush_right = callout.side == Side::Left;
        let align = if flush_right { Align::RIGHT } else { Align::LEFT };
        let max_inner = COLUMN_MAX * s - padding.x * 2.0;
        let title =
            text_galley(painter, &callout.title, style.title.clone(), style.weak, max_inner, align);
        let leads: Vec<Option<Arc<Galley>>> = callout
            .rows
            .iter()
            .map(|row| {
                row.lead.map(|lead| {
                    let wrap = f32::INFINITY;
                    text_galley(painter, lead, style.lead.clone(), style.weak, wrap, Align::LEFT)
                })
            })
            .collect();
        let lead_width = leads.iter().flatten().map(|g| g.size().x).fold(0.0, f32::max);
        let lead_room = if lead_width > 0.0 { lead_width + 8.0 * s } else { 0.0 };
        let content_max = (max_inner - lead_room).max(40.0 * s);

        // Each row's pieces flowed into lines, relative to the row's content.
        let mut flowed = Vec::new();
        for row in &callout.rows {
            let mut pieces: Vec<Piece> = row
                .items
                .iter()
                .map(|item| Piece::new(painter, item, style, colors, content_max, align))
                .collect();
            if let Some(name) = &row.name {
                let galley =
                    text_galley(painter, name, style.name.clone(), style.text, content_max, align);
                let rect = Rect::from_min_size(Pos2::ZERO, galley.size());
                let name = Piece { kind: PieceKind::Text, galley, rect };
                if flush_right {
                    pieces.insert(0, name);
                } else {
                    pieces.push(name);
                }
            }
            let sizes: Vec<Vec2> = pieces.iter().map(|piece| piece.rect.size()).collect();
            let (corners, size, first_line) =
                flow(&sizes, content_max, 5.0 * s, 3.0 * s, flush_right);
            for (piece, corner) in pieces.iter_mut().zip(corners) {
                piece.rect = Rect::from_min_size(corner, piece.rect.size());
            }
            flowed.push((pieces, size, first_line));
        }
        let inner_width =
            flowed.iter().map(|(_, size, _)| lead_room + size.x).fold(title.size().x, f32::max);

        let title_x =
            if flush_right { padding.x + inner_width - title.size().x } else { padding.x };
        let title_rect = Rect::from_min_size(pos2(title_x, padding.y), title.size());
        let mut y = title_rect.max.y + 2.0 * s;
        let mut bottom = title_rect.max.y;
        let mut rows = Vec::new();
        for ((pieces, size, first_line), lead) in flowed.into_iter().zip(leads) {
            let x =
                if flush_right { padding.x + inner_width - size.x } else { padding.x + lead_room };
            let pieces = pieces
                .into_iter()
                .map(|mut piece| {
                    piece.rect = piece.rect.translate(vec2(x, y));
                    piece
                })
                .collect();
            let lead_height = lead.as_ref().map_or(0.0, |g| g.size().y);
            let lead = lead.map(|g| {
                let top = y + (first_line - lead_height).max(0.0) / 2.0;
                (Rect::from_min_size(pos2(padding.x, top), g.size()), g)
            });
            let height = size.y.max(lead_height);
            let band = Rect::from_min_max(
                pos2(padding.x - 4.0 * s, y - 1.5 * s),
                pos2(padding.x + inner_width + 4.0 * s, y + height + 1.5 * s),
            );
            rows.push(LaidRow { band, lead, pieces });
            bottom = y + height;
            y = bottom + 3.0 * s;
        }
        Self {
            size: vec2(inner_width + padding.x * 2.0, bottom + padding.y),
            title_mid: title_rect.center().y,
            title: (title_rect, title),
            rows,
        }
    }
}

impl Piece {
    /// A row item at its size, placed at the origin.
    fn new(
        painter: &Painter,
        item: &Item,
        style: &Style,
        colors: &Colors,
        max_width: f32,
        align: Align,
    ) -> Self {
        let s = style.scale;
        let (kind, galley, padding) = match item {
            Item::Cap(text, tone) => {
                let padding = vec2(6.0, 2.5) * s.max(0.8);
                let (_, color) = colors.chip(*tone);
                let wrap = (max_width - padding.x * 2.0).max(10.0);
                let galley =
                    text_galley(painter, text, style.cap.clone(), color, wrap, Align::LEFT);
                (PieceKind::Cap(*tone), galley, padding)
            }
            Item::Note(text) => {
                let galley =
                    text_galley(painter, text, style.note.clone(), style.weak, max_width, align);
                (PieceKind::Text, galley, Vec2::ZERO)
            }
            Item::Badge(text) => {
                let padding = vec2(4.0, 1.0) * s;
                let galley = text_galley(
                    painter,
                    text,
                    style.badge.clone(),
                    style.weak,
                    f32::INFINITY,
                    Align::LEFT,
                );
                (PieceKind::Badge, galley, padding)
            }
        };
        let rect = Rect::from_min_size(Pos2::ZERO, galley.size() + padding * 2.0);
        Self { kind, galley, rect }
    }
}

/// The sheet laid out at one size, relative to its top-left corner.
struct Arrangement {
    style: Style,
    /// The callouts' text, in the same order as the callouts.
    laid: Vec<Laid>,
    /// Each callout's top-left corner.
    tops: Vec<Pos2>,
    /// Where the picture goes.
    picture: Rect,
    /// Points per picture unit.
    zoom: f32,
    /// Height of the picture and columns, without margins and footer.
    body_height: f32,
    /// Height of the taller column.
    columns_height: f32,
    /// The list of free controls under everything.
    footer: Option<(Pos2, Arc<Galley>)>,
    size: Vec2,
}

impl Arrangement {
    /// Lays the sheet out with text at `scale` and the picture `growth` times its usual size.
    fn new(
        ui: &Ui,
        callouts: &[Callout],
        footer: Option<&str>,
        content: Rect,
        scale: f32,
        growth: f32,
        colors: &Colors,
    ) -> Self {
        let style = Style::new(ui, scale);
        let painter = ui.painter();
        let laid: Vec<Laid> =
            callouts.iter().map(|callout| Laid::new(painter, callout, &style, colors)).collect();
        let (column, room) = if callouts.is_empty() {
            (0.0, 0.0)
        } else {
            let widest = laid.iter().map(|l| l.size.x).fold(COLUMN_MIN * scale, f32::max);
            (widest, LEADER_ROOM * scale)
        };
        let fit = PICTURE_BOX * scale * growth;
        let zoom = (fit.x / content.width()).min(fit.y / content.height());
        let picture_size = content.size() * zoom;
        let gap = CALLOUT_GAP * scale;
        let column_height = |side: Side| {
            let heights: Vec<f32> = callouts
                .iter()
                .zip(&laid)
                .filter(|(callout, _)| callout.side == side)
                .map(|(_, l)| l.size.y)
                .collect();
            heights.iter().sum::<f32>() + gap * heights.len().saturating_sub(1) as f32
        };
        let columns_height = column_height(Side::Left).max(column_height(Side::Right));
        let body_height = picture_size.y.max(columns_height);
        let margin = MARGIN * scale;
        let width = margin * 2.0 + column * 2.0 + room * 2.0 + picture_size.x;
        let picture = Rect::from_min_size(
            pos2(margin + column + room, margin + (body_height - picture_size.y) / 2.0),
            picture_size,
        );

        // Each column in its controls' order from the top, spread out so nothing overlaps.
        let mut tops = vec![Pos2::ZERO; callouts.len()];
        let anchor_y = |i: usize| picture.min.y + (callouts[i].anchor.y - content.min.y) * zoom;
        for side in [Side::Left, Side::Right] {
            let mut order: Vec<usize> =
                (0..callouts.len()).filter(|&i| callouts[i].side == side).collect();
            // Level controls: the one nearer the column first, so the far one's line passes
            // under it rather than through it.
            let nearness = |i: usize| match side {
                Side::Left => callouts[i].anchor.x,
                Side::Right => -callouts[i].anchor.x,
            };
            order.sort_by(|&a, &b| {
                anchor_y(a).total_cmp(&anchor_y(b)).then(nearness(a).total_cmp(&nearness(b)))
            });
            let heights: Vec<f32> = order.iter().map(|&i| laid[i].size.y).collect();
            // Title lines level with their controls where there is room.
            let desired: Vec<f32> = order
                .iter()
                .map(|&i| anchor_y(i) - laid[i].title_mid + laid[i].size.y / 2.0)
                .collect();
            let centres = place(&desired, &heights, gap, margin, margin + body_height);
            for (k, &i) in order.iter().enumerate() {
                let x = match side {
                    Side::Left => margin + column - laid[i].size.x,
                    Side::Right => picture.max.x + room,
                };
                tops[i] = pos2(x, (centres[k] - heights[k] / 2.0).round());
            }
        }

        let mut height = margin * 2.0 + body_height;
        let footer = footer.map(|text| {
            let wrap = width - margin * 2.0;
            let galley =
                text_galley(painter, text, style.note.clone(), style.weak, wrap, Align::LEFT);
            let pos = pos2(margin, margin + body_height + gap * 1.5);
            height += gap * 1.5 + galley.size().y;
            (pos, galley)
        });
        Self {
            style,
            laid,
            tops,
            picture,
            zoom,
            body_height,
            columns_height,
            footer,
            size: vec2(width, height),
        }
    }
}

/// Places boxes down a column. Each box wants its centre at `desired[i]` (given from the top
/// down) and is `heights[i]` tall. Boxes keep their order and at least `gap` between them:
/// a box that would overlap the one above is pushed down, and if the last one then runs past
/// `bottom` the column is pushed back up, no higher than `top`. Returns the centres. A column
/// too tall for `top..bottom` starts at `top` and runs past `bottom`.
fn place(desired: &[f32], heights: &[f32], gap: f32, top: f32, bottom: f32) -> Vec<f32> {
    let heights = &heights[..desired.len().min(heights.len())];
    // Down: each box at its place, or just below the one above.
    let mut floor = top;
    let mut tops: Vec<f32> = desired
        .iter()
        .zip(heights)
        .map(|(&centre, &height)| {
            let box_top = (centre - height / 2.0).max(floor);
            floor = box_top + height + gap;
            box_top
        })
        .collect();
    // Up: from the bottom, each box above the one below it.
    let mut ceiling = bottom;
    for (box_top, &height) in tops.iter_mut().zip(heights).rev() {
        *box_top = box_top.min(ceiling - height);
        ceiling = *box_top - gap;
    }
    // Down again, in case that pushed the first ones above `top`.
    let mut floor = top;
    for (box_top, &height) in tops.iter_mut().zip(heights) {
        *box_top = box_top.max(floor);
        floor = *box_top + height + gap;
    }
    tops.iter().zip(heights).map(|(box_top, height)| box_top + height / 2.0).collect()
}

/// Lays boxes of the given sizes out in lines no wider than `max_width` (a box wider than that
/// gets a line of its own), `spacing` apart, with `line_gap` between lines, each line flush
/// right when `right`. Returns each box's top-left corner, the size of the whole, and the
/// height of the first line.
fn flow(
    sizes: &[Vec2],
    max_width: f32,
    spacing: f32,
    line_gap: f32,
    right: bool,
) -> (Vec<Pos2>, Vec2, f32) {
    // Lines as (first box, end, width, height).
    let mut lines: Vec<(usize, usize, f32, f32)> = Vec::new();
    for (i, size) in sizes.iter().enumerate() {
        match lines.last_mut() {
            Some(line) if line.2 + spacing + size.x <= max_width => {
                line.1 = i + 1;
                line.2 += spacing + size.x;
                line.3 = line.3.max(size.y);
            }
            _ => lines.push((i, i + 1, size.x, size.y)),
        }
    }
    let width = lines.iter().map(|line| line.2).fold(0.0, f32::max);
    let mut corners = vec![Pos2::ZERO; sizes.len()];
    let mut y = 0.0;
    for &(start, end, line_width, line_height) in &lines {
        let mut x = if right { width - line_width } else { 0.0 };
        for (corner, size) in corners[start..end].iter_mut().zip(&sizes[start..end]) {
            *corner = pos2(x, y + (line_height - size.y) / 2.0);
            x += size.x + spacing;
        }
        y += line_height + line_gap;
    }
    let height = (y - line_gap).max(0.0);
    let first_line = lines.first().map_or(0.0, |line| line.3);
    (corners, vec2(width, height), first_line)
}

// ------------------------------------------------------------------------------------------
// Saving the sheet as an image.

/// Saving's progress, kept between frames. While a save is under way the whole sheet is drawn
/// over the window ([`draw_for_export`]); the window delivers its screenshot to a callback a
/// frame or two after it is asked for, and the next frame asks where to save the picture, once
/// the window shows the normal view again.
#[derive(Clone, Default)]
struct Save {
    /// The screenshot, once it has arrived.
    shot: Arc<Mutex<Option<Arc<ColorImage>>>>,
    /// When saving started, while the screenshot has not arrived.
    asked: Option<f64>,
    /// The screenshot has been asked for. That waits for a frame with the whole sheet drawn.
    requested: bool,
    /// Where the whole sheet is drawn on screen, in points.
    rect: Option<Rect>,
    /// The sheet cut out of the screenshot, waiting for a file name.
    picture: Option<Arc<ColorImage>>,
    /// How the last save went: where the image went, or what went wrong.
    outcome: Option<Result<PathBuf, String>>,
}

impl Save {
    const ID: &'static str = "callout-sheet-save";

    /// Picks the save up where the last frame left it: takes a screenshot that has arrived, or
    /// asks where to save the picture cut from the last one.
    fn begin(ui: &Ui) -> Self {
        let mut save: Self =
            ui.ctx().data(|d| d.get_temp(Id::unique(Self::ID))).unwrap_or_default();
        let now = ui.input(|i| i.time);
        if let Some(picture) = save.picture.take() {
            if let Some(outcome) = save_image(&picture) {
                save.outcome = Some(outcome);
            }
            return save;
        }
        let shot = save.shot.lock().ok().and_then(|mut shot| shot.take());
        match (shot, save.asked) {
            (Some(image), Some(_)) => {
                save.asked = None;
                save.requested = false;
                match save.rect.map(|rect| crop(&image, rect, ui.ctx().pixels_per_point())) {
                    Some(Ok(picture)) => {
                        save.picture = Some(Arc::new(picture));
                        // Ask for the file name next frame, over the normal view.
                        ui.ctx().request_repaint();
                    }
                    Some(Err(e)) => save.outcome = Some(Err(e)),
                    None => save.outcome = Some(Err("The sheet was not on screen.".into())),
                }
            }
            (None, Some(asked)) if now - asked > 3.0 => {
                save.asked = None;
                save.requested = false;
                save.outcome = Some(Err("The window did not send a picture. Try again.".into()));
            }
            // Nothing asked for: a screenshot arriving after the wait gave up is dropped.
            _ => {}
        }
        save
    }

    /// The "Save as image…" button under the sheet, and asking for the screenshot once the
    /// whole sheet is on screen.
    fn finish(mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        if self.asked.is_some() && self.rect.is_some() && !self.requested {
            self.requested = true;
            let slot = self.shot.clone();
            let ctx = ui.ctx().clone();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::ScreenshotCallback::new(move |image| {
                    if let Ok(mut shot) = slot.lock() {
                        *shot = Some(image);
                    }
                    ctx.request_repaint();
                }),
            ));
        }
        ui.horizontal_wrapped(|ui| {
            let button = egui::Button::new("Save as image…").small();
            let idle = self.asked.is_none() && self.picture.is_none();
            if ui
                .add_enabled(idle, button)
                .on_hover_text(
                    "Saves the whole sheet as a PNG picture, even the parts scrolled out of view.",
                )
                .clicked()
            {
                self.asked = Some(now);
                self.requested = false;
                self.rect = None;
                self.outcome = None;
            }
            match &self.outcome {
                Some(Ok(path)) => {
                    ui.weak(format!("Saved as {}", path.display()));
                }
                Some(Err(e)) => {
                    ui.colored_label(theme::error(ui.visuals()), e);
                }
                None => {}
            }
        });
        if self.asked.is_some() {
            // Keep frames coming until the screenshot arrives or the wait gives up.
            ui.ctx().request_repaint();
        }
        ui.ctx().data_mut(|d| d.insert_temp(Id::unique(Self::ID), self));
    }
}

/// Cuts the sheet at `rect` (in points) out of the window's screenshot.
fn crop(image: &ColorImage, rect: Rect, pixels_per_point: f32) -> Result<ColorImage, String> {
    let [width, height] = image.size;
    let px = |v: f32, max: usize| ((v * pixels_per_point).round().max(0.0) as usize).min(max);
    let (x0, y0) = (px(rect.min.x, width), px(rect.min.y, height));
    let (x1, y1) = (px(rect.max.x, width), px(rect.max.y, height));
    if x1 <= x0 || y1 <= y0 {
        return Err("The sheet was not on screen.".into());
    }
    Ok(image.region_by_pixels([x0, y0], [x1 - x0, y1 - y0]))
}

/// Asks where to save the sheet's picture and writes it as a PNG. Returns where it went or what
/// went wrong, or `None` if the user cancelled.
fn save_image(picture: &ColorImage) -> Option<Result<PathBuf, String>> {
    let path = rfd::FileDialog::new()
        .add_filter("PNG image", &["png"])
        .set_file_name("Controller sheet.png")
        .save_file()?;
    let path = if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("png")) {
        path
    } else {
        let mut name = path.into_os_string();
        name.push(".png");
        PathBuf::from(name)
    };
    Some(write_png(&path, picture).map(|()| path))
}

/// Writes an image as an opaque 8-bit RGBA PNG.
fn write_png(path: &Path, image: &ColorImage) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("Could not save {}: {e}", path.display());
    let file = std::fs::File::create(path).map_err(|e| error(&e))?;
    let [width, height] = image.size;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| error(&e))?;
    // The window's pixels are opaque; its alpha channel is not always.
    let data: Vec<u8> = image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b(), 255]).collect();
    writer.write_image_data(&data).map_err(|e| error(&e))?;
    writer.finish().map_err(|e| error(&e))
}

#[cfg(test)]
mod tests {
    use zpadder_model::profile::LayerSelect;

    use super::*;

    fn assignment(slots: &str) -> Assignment {
        Assignment {
            slots: slots.split(',').map(|s| s.parse().unwrap()).collect(),
            ..Assignment::default()
        }
    }

    fn cap(text: &str, tone: Tone) -> Item {
        Item::Cap(text.into(), tone)
    }

    fn note(text: &str) -> Item {
        Item::Note(text.into())
    }

    #[test]
    fn callouts_that_fit_sit_level_with_their_controls() {
        assert_eq!(place(&[50.0, 150.0], &[20.0, 20.0], 10.0, 0.0, 300.0), [50.0, 150.0]);
        assert_eq!(place(&[], &[], 10.0, 0.0, 300.0), Vec::<f32>::new());
    }

    #[test]
    fn overlapping_callouts_are_pushed_down() {
        // The second wants to start at 90 but the first ends at 110: it starts at 120.
        assert_eq!(place(&[100.0, 100.0], &[20.0, 20.0], 10.0, 0.0, 300.0), [100.0, 130.0]);
        // Pushes carry on down the column.
        let placed = place(&[100.0, 100.0, 105.0], &[20.0, 40.0, 20.0], 10.0, 0.0, 300.0);
        assert_eq!(placed, [100.0, 140.0, 180.0]);
    }

    #[test]
    fn a_column_past_the_bottom_is_shifted_back_up() {
        let placed = place(&[50.0, 190.0, 195.0], &[20.0, 20.0, 20.0], 10.0, 0.0, 200.0);
        // The last ends at the bottom, the one above it a gap higher, the first stays put.
        assert_eq!(placed, [50.0, 160.0, 190.0]);
        // Above the top is not allowed either.
        assert_eq!(place(&[0.0], &[40.0], 10.0, 5.0, 200.0), [25.0]);
    }

    #[test]
    fn a_column_too_tall_starts_at_the_top() {
        let placed = place(&[100.0, 100.0, 100.0], &[40.0, 40.0, 40.0], 10.0, 0.0, 100.0);
        assert_eq!(placed, [20.0, 70.0, 120.0]);
    }

    #[test]
    fn flow_wraps_and_aligns() {
        let sizes = [vec2(30.0, 10.0), vec2(30.0, 20.0), vec2(30.0, 10.0)];
        let (corners, size, first) = flow(&sizes, 70.0, 5.0, 2.0, false);
        assert_eq!(corners, [pos2(0.0, 5.0), pos2(35.0, 0.0), pos2(0.0, 22.0)]);
        assert_eq!((size, first), (vec2(65.0, 32.0), 20.0));
        // Flush right, the short last line moves over.
        let (corners, _, _) = flow(&sizes, 70.0, 5.0, 2.0, true);
        assert_eq!(corners[2], pos2(35.0, 22.0));
    }

    #[test]
    fn keycaps_are_listed_in_full() {
        assert_eq!(
            assignment_items(&assignment("left_shift, mouse_left")),
            [cap("Shift", Tone::Key), note("+"), cap("LMB", Tone::Mouse)]
        );
        assert_eq!(
            assignment_items(&assignment("a, pause 0.1s, b, cycle, c")),
            [
                cap("A", Tone::Macro),
                note("wait 0.1 s"),
                cap("B", Tone::Macro),
                note("next press:"),
                cap("C", Tone::Macro)
            ]
        );
        assert_eq!(
            assignment_items(&assignment("hold 0.5s, space")),
            [note("Held 0.5 s:"), cap("Space", Tone::Macro)]
        );
        let aim = Assignment {
            name: "Aim".into(),
            turbo: Some(10.0),
            toggle: true,
            layer: Some(LayerSelect { target: "Scope".into(), mode: SelectMode::WhileHeld }),
            ..assignment("mouse_right")
        };
        assert_eq!(
            assignment_items(&aim),
            [
                cap("RMB", Tone::Mouse),
                cap("⇧ Scope", Tone::Layer),
                Item::Badge("Turbo"),
                Item::Badge("Toggle")
            ]
        );
    }

    #[test]
    fn stick_directions_match_the_engine() {
        let mut stick = Stick::default();
        assert_eq!(stick_directions(0.0, -1.0, &stick), [Direction::Up]);
        assert_eq!(stick_directions(0.7, 0.7, &stick), [Direction::Down, Direction::Right]);
        assert!(stick_directions(0.1, 0.0, &stick).is_empty());
        stick.directions.eight_way = true;
        assert_eq!(stick_directions(-0.7, -0.7, &stick), [Direction::UpLeft]);
    }

    #[test]
    fn related_controls_share_a_callout_and_free_ones_are_listed() {
        let mut layer = Layer::new("Main");
        let mut jump = assignment("space");
        jump.name = "Jump".into();
        layer.buttons.insert(Button::South, jump);
        layer.buttons.insert(Button::DPadUp, assignment("up"));
        layer.buttons.insert(Button::RightStick, assignment("v"));
        // Does nothing, so it counts as free.
        layer.buttons.insert(Button::North, Assignment::default());
        layer.left_stick.mode = StickMode::Directions;
        layer.left_stick.directions.up = Some(assignment("w"));
        let state = ViewState {
            family: Family::Xbox,
            layer: &layer,
            live: None,
            selected: None,
            direction: Direction::Up,
            show_all: false,
            show_names: false,
            picture: None,
            passthrough: false,
        };
        let (spots, content) = pad_view::controls(&state);
        let (callouts, free) = callouts(&state, &spots, content.center().x, false);
        let titles: Vec<&str> = callouts.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["A", "Left stick", "Right stick", "D-pad"]);

        let jump = &callouts[0];
        assert_eq!(jump.side, Side::Right);
        assert_eq!(jump.rows[0].name.as_deref(), Some("Jump"));
        assert_eq!(jump.rows[0].items, [cap("Space", Tone::Key)]);

        // Four directions, the dead zone and the click.
        let left = &callouts[1];
        assert_eq!(left.side, Side::Left);
        let leads: Vec<Option<&str>> = left.rows.iter().map(|row| row.lead).collect();
        assert_eq!(leads, [Some("↑"), Some("→"), Some("↓"), Some("←"), None, Some("Click:")]);
        assert_eq!(left.rows[0].pick.direction, Some(Direction::Up));
        assert_eq!(left.rows[1].items, [note("Free")]);
        assert_eq!(left.rows[4].items, [note("Dead zone 25%")]);

        // A stick that only clicks: just the click.
        assert_eq!(callouts[2].rows.len(), 1);
        assert_eq!(callouts[2].rows[0].items, [cap("V", Tone::Key)]);

        let dpad = &callouts[3];
        assert_eq!(dpad.parts.len(), 4);
        assert_eq!(dpad.rows[0].pick.part, Part::Button(Button::DPadUp));
        assert_eq!(dpad.rows[2].items, [note("Free")]);

        assert!(free.contains(&"Y".to_string()));
        assert!(free.contains(&"LT".to_string()));
        assert!(!free.iter().any(|name| name.starts_with("D-pad")));
        // Paddles are hidden unless asked for, as in the controller view.
        assert!(!free.iter().any(|name| name.contains("paddle")));
    }

    /// Draws the sheet headlessly for every built-in photo and the drawn controller, with each
    /// starter template, at a roomy and a cramped size: it must not panic, and clicking where
    /// nothing is must pick nothing.
    #[test]
    fn sheet_draws_for_every_picture_and_template() {
        use eframe::egui::{Rect, pos2, vec2};

        use crate::builtin::{PICTURES, TEMPLATES};
        use crate::pad_view::Family;
        use crate::picture::Picture;

        let pictures: Vec<Option<Picture>> = std::iter::once(None)
            .chain(PICTURES.iter().map(|builtin| {
                Some(Picture {
                    source: builtin.id.into(),
                    layout: zpadder_config::parse_layout(builtin.layout).unwrap(),
                    texture: None,
                })
            }))
            .collect();
        for template in &TEMPLATES {
            let profile = zpadder_config::parse(template.toml).unwrap();
            for picture in &pictures {
                // The family only changes the drawn controller's labels.
                let families: &[Family] = if picture.is_some() {
                    &[Family::PlayStation]
                } else {
                    &[Family::PlayStation, Family::Xbox, Family::Nintendo]
                };
                for &family in families {
                    for size in [vec2(1180.0, 760.0), vec2(700.0, 420.0)] {
                        let state = ViewState {
                            family,
                            layer: &profile.layers[0],
                            live: None,
                            selected: Some(Part::Button(zpadder_model::Button::South)),
                            direction: Direction::Up,
                            show_all: true,
                            show_names: true,
                            picture: picture.as_ref(),
                            passthrough: true,
                        };
                        let ctx = egui::Context::default();
                        let input = egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)),
                            ..Default::default()
                        };
                        let mut picked = None;
                        let mut output = ctx.run_ui(input, |ui| picked = show(ui, &state));
                        output.textures_delta.clear();
                        assert_eq!(picked, None, "{} {family:?} {size:?}", template.name);
                    }
                }
            }
        }
    }
}
