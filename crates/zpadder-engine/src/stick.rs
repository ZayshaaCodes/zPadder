use zpadder_model::profile::{
    Direction, DirectionSettings, MAX_MOUSE_SPEED, MIN_DIRECTION_DEAD_ZONE, Stick,
};

use crate::release_point;

/// How far, in degrees, a stick can stray past the edge of the sector it is in before it
/// changes sector, so a stick resting on an edge does not alternate between two directions.
const SECTOR_HYSTERESIS: f32 = 4.0;

/// The directions a stick at `(x, y)` presses (Y positive downwards), given the directions it
/// pressed before. A stick in use lets go a little inside its dead zone and keeps its sector a
/// little past the sector's edges, so a stick resting on either does not chatter.
pub(crate) fn directions(
    x: f32,
    y: f32,
    dead_zone: f32,
    settings: &DirectionSettings,
    previous: &[Direction],
) -> Vec<Direction> {
    // Validation enforces the minimum; it is repeated here so that no profile can leave a
    // centred stick holding a direction.
    let dead_zone = dead_zone.max(MIN_DIRECTION_DEAD_ZONE);
    let threshold = if previous.is_empty() { dead_zone } else { release_point(dead_zone) };
    let magnitude = x.hypot(y);
    // Also true for NaN.
    if magnitude.partial_cmp(&threshold).is_none_or(|order| order.is_lt()) {
        return Vec::new();
    }
    // 0° is right, 90° is up.
    let angle = (-y).atan2(x).to_degrees().rem_euclid(360.0);
    let half_diagonal = settings.diagonal_size.clamp(0.0, 90.0) / 2.0;
    // Sectors are numbered anticlockwise from right in 45° steps; odd ones are diagonals.
    let half_width =
        |sector: usize| if sector % 2 == 1 { half_diagonal } else { 45.0 - half_diagonal };
    let off_centre = |sector: usize| {
        let difference = (angle - 45.0 * sector as f32).rem_euclid(360.0);
        difference.min(360.0 - difference)
    };
    if let Some(sector) = (0..8).find(|&s| sector_directions(s, settings.eight_way) == previous)
        && off_centre(sector) <= half_width(sector) + SECTOR_HYSTERESIS
    {
        return previous.to_vec();
    }
    let diagonal = 2 * ((angle / 90.0) as usize % 4) + 1;
    let sector = if off_centre(diagonal) <= half_diagonal {
        diagonal
    } else {
        2 * (((angle + 45.0) / 90.0) as usize % 4)
    };
    sector_directions(sector, settings.eight_way).to_vec()
}

/// What a stick in `sector` (see [`directions`]) presses. Without eight-way assignments a
/// diagonal presses both neighbours.
fn sector_directions(sector: usize, eight_way: bool) -> &'static [Direction] {
    use Direction::{Down, DownLeft, DownRight, Left, Right, Up, UpLeft, UpRight};
    const FOUR_WAY: [&[Direction]; 8] = [
        &[Right],
        &[Up, Right],
        &[Up],
        &[Up, Left],
        &[Left],
        &[Down, Left],
        &[Down],
        &[Down, Right],
    ];
    const EIGHT_WAY: [&[Direction]; 8] =
        [&[Right], &[UpRight], &[Up], &[UpLeft], &[Left], &[DownLeft], &[Down], &[DownRight]];
    if eight_way { EIGHT_WAY[sector] } else { FOUR_WAY[sector] }
}

/// Pointer velocity in pixels per second for a stick at `(x, y)`, or `None` inside the dead zone.
/// Deflection is rescaled so speed starts at zero at the dead zone's edge.
pub(crate) fn mouse_velocity(x: f32, y: f32, stick: &Stick) -> Option<(f32, f32)> {
    let magnitude = x.hypot(y);
    if magnitude == 0.0 || magnitude <= stick.dead_zone {
        return None;
    }
    let linear = ((magnitude - stick.dead_zone) / (1.0 - stick.dead_zone)).min(1.0);
    let deflection = linear.powf(stick.mouse.curve);
    let scale = deflection / magnitude * stick.mouse.speed.clamp(0.0, MAX_MOUSE_SPEED);
    let sign = |invert: bool| if invert { -1.0 } else { 1.0 };
    Some((x * scale * sign(stick.mouse.invert_x), y * scale * sign(stick.mouse.invert_y)))
}

/// Wheel speed in `WHEEL_DELTA` units per second as `(vertical, horizontal)` for a stick at
/// `(x, y)`, or `None` inside the dead zone. Pushing the stick up scrolls up.
pub(crate) fn scroll_velocity(x: f32, y: f32, stick: &Stick) -> Option<(f32, f32)> {
    let magnitude = x.hypot(y);
    if magnitude == 0.0 || magnitude <= stick.dead_zone {
        return None;
    }
    let deflection = ((magnitude - stick.dead_zone) / (1.0 - stick.dead_zone)).min(1.0);
    let sign = if stick.scroll.invert { -1.0 } else { 1.0 };
    let scale = deflection / magnitude * stick.scroll.speed * 120.0 * sign;
    let horizontal = if stick.scroll.horizontal { x * scale } else { 0.0 };
    Some((-y * scale, horizontal))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn four_way() -> DirectionSettings {
        DirectionSettings::default()
    }

    use Direction::{Down, DownLeft, Left, Right, Up, UpLeft};

    /// A stick at full tilt, `degrees` anticlockwise from right.
    fn at(degrees: f32) -> (f32, f32) {
        (degrees.to_radians().cos(), -degrees.to_radians().sin())
    }

    #[test]
    fn cardinals_and_diagonals() {
        let s = four_way();
        assert_eq!(directions(0.0, -1.0, 0.25, &s, &[]), [Up]);
        assert_eq!(directions(1.0, 0.0, 0.25, &s, &[]), [Right]);
        assert_eq!(directions(0.0, 1.0, 0.25, &s, &[]), [Down]);
        assert_eq!(directions(-1.0, 0.0, 0.25, &s, &[]), [Left]);
        assert_eq!(directions(0.7, -0.7, 0.25, &s, &[]), [Up, Right]);
        assert_eq!(directions(-0.7, -0.7, 0.25, &s, &[]), [Up, Left]);
        // Just below the X axis: the angle wraps to almost 360°.
        assert_eq!(directions(1.0, 1e-7, 0.25, &s, &[]), [Right]);
        let eight = DirectionSettings { eight_way: true, ..four_way() };
        assert_eq!(directions(-0.7, 0.7, 0.25, &eight, &[]), [DownLeft]);
    }

    #[test]
    fn diagonal_size_sets_sector_width() {
        // 30° above the X axis: inside a 45° diagonal sector (22.5°–67.5°), outside a 20° one.
        let (x, y) = at(30.0);
        let wide = four_way();
        let narrow = DirectionSettings { diagonal_size: 20.0, ..four_way() };
        assert_eq!(directions(x, y, 0.25, &wide, &[]), [Up, Right]);
        assert_eq!(directions(x, y, 0.25, &narrow, &[]), [Right]);
        // No diagonal sectors at all, or nothing but diagonals.
        let none = DirectionSettings { diagonal_size: 0.0, ..four_way() };
        let all = DirectionSettings { diagonal_size: 90.0, ..four_way() };
        let (x, y) = at(130.0);
        assert_eq!(directions(x, y, 0.25, &none, &[]), [Up]);
        assert_eq!(directions(x, y, 0.25, &all, &[]), [Up, Left]);
    }

    #[test]
    fn dead_zone_has_hysteresis() {
        let s = four_way();
        assert!(directions(0.23, 0.0, 0.25, &s, &[]).is_empty());
        assert_eq!(directions(0.23, 0.0, 0.25, &s, &[Right]), [Right]);
        assert!(directions(0.19, 0.0, 0.25, &s, &[Right]).is_empty());
    }

    #[test]
    fn small_dead_zones_still_let_go_at_rest() {
        // A held stick lets go at half a small dead zone, never at zero: noise of 0.01 at rest
        // releases it.
        let s = four_way();
        assert_eq!(directions(0.06, 0.0, 0.05, &s, &[]), [Right]);
        assert_eq!(directions(0.03, 0.0, 0.05, &s, &[Right]), [Right]);
        assert!(directions(0.01, 0.0, 0.05, &s, &[Right]).is_empty());
        // Below the minimum (only possible without validation), the minimum applies.
        assert!(directions(0.01, 0.0, 0.0, &s, &[]).is_empty());
        assert!(directions(0.01, 0.0, 0.04, &s, &[Right]).is_empty());
        assert!(directions(f32::NAN, 0.0, 0.25, &s, &[Right]).is_empty());
    }

    #[test]
    fn sector_edges_have_hysteresis() {
        let s = four_way(); // the right/up-right edge is at 22.5°
        let (x, y) = at(23.0);
        assert_eq!(directions(x, y, 0.25, &s, &[]), [Up, Right]);
        // Jitter back across the edge keeps the diagonal...
        let (x, y) = at(21.0);
        assert_eq!(directions(x, y, 0.25, &s, &[Up, Right]), [Up, Right]);
        assert_eq!(directions(x, y, 0.25, &s, &[]), [Right]);
        // ...and keeps the cardinal coming the other way.
        let (x, y) = at(25.0);
        assert_eq!(directions(x, y, 0.25, &s, &[Right]), [Right]);
        // Moving well past the edge changes sector.
        let (x, y) = at(18.0);
        assert_eq!(directions(x, y, 0.25, &s, &[Up, Right]), [Right]);
        let (x, y) = at(27.0);
        assert_eq!(directions(x, y, 0.25, &s, &[Right]), [Up, Right]);
        // Across 0°/360° too.
        let eight = DirectionSettings { eight_way: true, ..four_way() };
        let (x, y) = at(-24.0);
        assert_eq!(directions(x, y, 0.25, &eight, &[Right]), [Right]);
        let (x, y) = at(158.0);
        assert_eq!(directions(x, y, 0.25, &eight, &[UpLeft]), [UpLeft]);
        assert_eq!(directions(x, y, 0.25, &eight, &[]), [Left]);
        // A sector from another mode (four-way pair, eight-way settings) is not kept.
        let (x, y) = at(21.0);
        assert_eq!(directions(x, y, 0.25, &eight, &[Up, Right]), [Right]);
    }

    #[test]
    fn curve_slows_small_deflections_but_not_full_tilt() {
        let mut stick = Stick::default();
        stick.mouse.curve = 2.0;
        assert_eq!(mouse_velocity(1.0, 0.0, &stick), Some((1000.0, 0.0)));
        let (vx, _) = mouse_velocity(0.625, 0.0, &stick).unwrap(); // half-way: 0.5^2
        assert!((vx - 250.0).abs() < 0.01, "{vx}");
    }

    #[test]
    fn mouse_speed_scales_from_the_dead_zone_edge() {
        let stick = Stick::default(); // dead zone 0.25, 1000 px/s
        assert_eq!(mouse_velocity(0.2, 0.0, &stick), None);
        assert_eq!(mouse_velocity(1.0, 0.0, &stick), Some((1000.0, 0.0)));
        let (vx, _) = mouse_velocity(0.625, 0.0, &stick).unwrap();
        assert!((vx - 500.0).abs() < 0.01, "{vx}");
    }
}
