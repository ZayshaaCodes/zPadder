//! ZPadder's icon, drawn in code until there is artwork: a white "Z" on a blue rounded square.
//! `assets/zpadder.ico`, the icon Explorer shows for the exe, is generated from the same drawing.

/// Samples per pixel along each axis, for smooth edges.
const SAMPLES: u32 = 4;

/// RGBA pixels (not premultiplied) of a `size`×`size` icon.
pub(crate) fn rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let n = SAMPLES as f32;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut covered, mut on_z) = (0u32, 0u32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let u = (x as f32 + (sx as f32 + 0.5) / n) / s;
                    let v = (y as f32 + (sy as f32 + 0.5) / n) / s;
                    if in_rounded_square(u, v) {
                        covered += 1;
                        on_z += u32::from(in_z(u, v));
                    }
                }
            }
            if covered == 0 {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            // Blend the white Z over the blue within the covered part, then fade the edge.
            let t = on_z as f32 / covered as f32;
            let mix = |blue: f32| (blue + (255.0 - blue) * t).round() as u8;
            let alpha = (covered as f32 / (n * n) * 255.0).round() as u8;
            pixels.extend_from_slice(&[mix(38.0), mix(110.0), mix(220.0), alpha]);
        }
    }
    pixels
}

fn in_rounded_square(u: f32, v: f32) -> bool {
    let (lo, hi, r) = (0.04, 0.96, 0.22);
    if !(lo..=hi).contains(&u) || !(lo..=hi).contains(&v) {
        return false;
    }
    let cx = u.clamp(lo + r, hi - r);
    let cy = v.clamp(lo + r, hi - r);
    (u - cx).hypot(v - cy) <= r
}

fn in_z(u: f32, v: f32) -> bool {
    let (left, right, top, bottom, bar) = (0.26, 0.74, 0.24, 0.76, 0.12);
    if !(left..=right).contains(&u) {
        return false;
    }
    if (top..=top + bar).contains(&v) || (bottom - bar..=bottom).contains(&v) {
        return true;
    }
    // Diagonal from the top bar's right end to the bottom bar's left end.
    let (x0, y0, x1, y1) = (right, top + bar, left, bottom - bar);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let t = (((u - x0) * dx + (v - y0) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let distance = (u - (x0 + t * dx)).hypot(v - (y0 + t * dy));
    distance <= bar * 0.6
}

/// The icon as a Windows `.ico` file: PNG images at the sizes Explorer and the taskbar use.
#[cfg(test)]
fn ico() -> Vec<u8> {
    const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];
    let images: Vec<Vec<u8>> = SIZES.iter().map(|&size| png(size)).collect();
    let mut file = vec![0, 0, 1, 0];
    file.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (&size, image) in SIZES.iter().zip(&images) {
        // 0 stands for 256 in the one-byte width and height.
        let side = if size >= 256 { 0 } else { size as u8 };
        file.extend_from_slice(&[side, side, 0, 0]);
        file.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        file.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        file.extend_from_slice(&(image.len() as u32).to_le_bytes());
        file.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        file.extend(image);
    }
    file
}

#[cfg(test)]
fn png(size: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("PNG header");
    writer.write_image_data(&rgba(size)).expect("PNG data");
    writer.finish().expect("PNG end");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_icon_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/zpadder.ico");
        let ico = ico();
        if std::env::var_os("ZPADDER_WRITE_ICON").is_some() {
            std::fs::write(path, &ico).expect("cannot write assets/zpadder.ico");
        }
        assert!(
            std::fs::read(path).ok() == Some(ico),
            "assets/zpadder.ico is out of date; regenerate it with\n  \
             ZPADDER_WRITE_ICON=1 cargo test -p zpadder-gui icon"
        );
    }

    #[test]
    fn edges_are_smooth() {
        let pixels = rgba(64);
        let alphas: std::collections::BTreeSet<u8> = pixels.chunks(4).map(|p| p[3]).collect();
        assert!(alphas.len() > 2, "the rounded corners should fade, not step");
    }
}
