//! Controller pictures: a layout (positions of controls on an image) plus the image, as imported
//! from Xpadder `.xpaddercontroller` files.

use std::path::{Path, PathBuf};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use zpadder_model::Layout;

use crate::{builtin, files};

/// The largest picture side ZPadder decodes, in pixels.
const MAX_SIDE: u32 = 8192;
/// The most memory a decoded picture may take, in bytes of RGBA.
const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// A layout ready to draw.
pub(crate) struct Picture {
    /// The layout file it came from, to know when it needs reloading.
    pub(crate) source: PathBuf,
    pub(crate) layout: Layout,
    pub(crate) texture: Option<TextureHandle>,
}

impl Picture {
    /// Loads a layout file, or a built-in picture when `path` is `builtin:<id>`. Pictures larger
    /// than the graphics card takes are scaled down.
    pub(crate) fn load(ctx: &egui::Context, path: &Path) -> Result<Self, String> {
        let max_side = ctx.input(|i| i.max_texture_side);
        if let Some(builtin) = path.to_str().and_then(builtin::picture) {
            let layout = zpadder_config::parse_layout(builtin.layout).map_err(|e| e.to_string())?;
            let pixels = decode_png(builtin.png, builtin.name, max_side)?;
            let texture =
                ctx.load_texture(format!("builtin-{}", builtin.id), pixels, TextureOptions::LINEAR);
            return Ok(Self { source: path.to_owned(), layout, texture: Some(texture) });
        }
        if let Some(id) = path.to_str().and_then(|p| p.strip_prefix(builtin::PREFIX)) {
            return Err(format!("this version of ZPadder has no built-in picture \"{id}\""));
        }
        let layout = zpadder_config::load_layout(path).map_err(|e| e.to_string())?;
        let texture = match &layout.image {
            Some(image) => {
                let image_path = path.parent().unwrap_or(Path::new(".")).join(image);
                let bytes = read_capped(&image_path, MAX_BYTES)?;
                let pixels = decode_png(&bytes, &image_path.display().to_string(), max_side)?;
                Some(ctx.load_texture(
                    image_path.display().to_string(),
                    pixels,
                    TextureOptions::LINEAR,
                ))
            }
            None => None,
        };
        Ok(Self { source: path.to_owned(), layout, texture })
    }
}

/// Decodes a PNG into RGBA, treating magenta (#FF00FF) as transparent like Xpadder does.
/// Pictures too large to decode safely are refused before anything is allocated, and pictures
/// with a side longer than `max_side` are scaled down to fit.
fn decode_png(bytes: &[u8], what: &str, max_side: usize) -> Result<ColorImage, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("{what}: {e}"))?;
    let (width, height) = (reader.info().width, reader.info().height);
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) * 4 > MAX_BYTES
    {
        return Err(format!(
            "{what}: the picture is {width} by {height} pixels, too large to show. Pictures up to \
             {MAX_SIDE} pixels a side and {} megapixels work.",
            MAX_BYTES / 4 / 1_000_000
        ));
    }
    let size = reader.output_buffer_size().ok_or_else(|| format!("{what}: picture too large"))?;
    if size as u64 > MAX_BYTES * 2 {
        return Err(format!("{what}: picture too large"));
    }
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("{what}: {e}"))?;
    let bytes = &buf[..info.buffer_size()];
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(bytes),
        png::ColorType::Rgb => bytes
            .as_chunks::<3>()
            .0
            .iter()
            .for_each(|&[r, g, b]| rgba.extend_from_slice(&[r, g, b, 255])),
        png::ColorType::GrayscaleAlpha => bytes
            .as_chunks::<2>()
            .0
            .iter()
            .for_each(|&[v, a]| rgba.extend_from_slice(&[v, v, v, a])),
        png::ColorType::Grayscale => {
            bytes.iter().for_each(|&g| rgba.extend_from_slice(&[g, g, g, 255]))
        }
        png::ColorType::Indexed => {
            return Err(format!("{what}: unsupported palette picture"));
        }
    }
    for pixel in rgba.as_chunks_mut::<4>().0 {
        if pixel[..3] == [255, 0, 255] {
            pixel[3] = 0;
        }
    }
    let (mut width, mut height) = (info.width as usize, info.height as usize);
    let longest = width.max(height);
    if longest > max_side.max(1) {
        let factor = longest.div_ceil(max_side.max(1));
        (rgba, width, height) = downscale(&rgba, width, height, factor);
    }
    Ok(ColorImage::from_rgba_unmultiplied([width, height], &rgba))
}

/// Shrinks an RGBA picture `factor` times in each direction, averaging each block of pixels.
/// Colours are weighted by their opacity, so transparent pixels do not tint the edges.
fn downscale(rgba: &[u8], width: usize, height: usize, factor: usize) -> (Vec<u8>, usize, usize) {
    let (new_width, new_height) = (width.div_ceil(factor), height.div_ceil(factor));
    let mut out = Vec::with_capacity(new_width * new_height * 4);
    for y in 0..new_height {
        for x in 0..new_width {
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for sy in y * factor..((y + 1) * factor).min(height) {
                for sx in x * factor..((x + 1) * factor).min(width) {
                    let i = (sy * width + sx) * 4;
                    let alpha = u64::from(rgba[i + 3]);
                    for c in 0..3 {
                        sum[c] += u64::from(rgba[i + c]) * alpha;
                    }
                    sum[3] += alpha;
                    count += 1;
                }
            }
            let colour = |c: usize| sum[c].checked_div(sum[3]).unwrap_or(0) as u8;
            let alpha = sum[3].checked_div(count).unwrap_or(0) as u8;
            out.extend_from_slice(&[colour(0), colour(1), colour(2), alpha]);
        }
    }
    (out, new_width, new_height)
}

/// Imports an Xpadder controller file into ZPadder's `layouts` folder (layout plus PNG) and
/// returns the layout's path and the import notes.
pub(crate) fn import_controller(controller: &Path) -> Result<(PathBuf, Vec<String>), String> {
    let dir = crate::settings::data_dir().ok_or("no folder for ZPadder's files")?.join("layouts");
    import_controller_into(controller, &dir)
}

/// The file name, without extension, an imported controller gets: its own name plus a hash of
/// its contents, so controllers with the same name from different packs do not overwrite each
/// other, and importing the same file again finds the earlier import.
fn import_name(stem: &str, bytes: &[u8]) -> String {
    let hash = files::content_hash(bytes);
    format!("{}-{:012x}", files::safe_file_stem(stem, "controller"), hash >> 16)
}

/// [`import_controller`] into `dir`. An earlier import of the same file is reused when it is
/// still intact; otherwise the layout and picture are written again.
fn import_controller_into(controller: &Path, dir: &Path) -> Result<(PathBuf, Vec<String>), String> {
    let bytes = read_capped(controller, zpadder_config::MAX_FILE_SIZE)?;
    let stem = controller.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut imported = zpadder_import_xpadder::import_controller(&bytes, &stem)
        .map_err(|e| format!("{}: {e}", controller.display()))?;
    let base = import_name(&stem, &bytes);
    imported.layout.image = imported.png.as_ref().map(|_| format!("{base}.png"));
    let path = dir.join(format!("{base}.layout.toml"));

    let picture_intact = match (&imported.png, &imported.layout.image) {
        (Some(png), Some(image)) => std::fs::read(dir.join(image)).is_ok_and(|old| old == *png),
        _ => true,
    };
    let layout_intact = zpadder_config::load_layout(&path).is_ok_and(|old| old == imported.layout);
    if picture_intact && layout_intact {
        return Ok((path, imported.warnings));
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if let (Some(png), Some(image)) = (&imported.png, &imported.layout.image) {
        zpadder_config::write_atomic(&dir.join(image), png).map_err(|e| format!("{image}: {e}"))?;
    }
    zpadder_config::save_layout(&path, &imported.layout)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((path, imported.warnings))
}

/// Reads a whole file of at most `limit` bytes. A larger one is refused with a clear message
/// before anything is read; one that grows while it is read is cut off and refused too.
fn read_capped(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let too_large = || {
        format!("{}: the file is larger than {} MB, too large to use", path.display(), limit >> 20)
    };
    let error = |e: std::io::Error| format!("{}: {e}", path.display());
    let file = std::fs::File::open(path).map_err(error)?;
    if file.metadata().map_err(error)?.len() > limit {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(error)?;
    if bytes.len() as u64 > limit {
        return Err(too_large());
    }
    Ok(bytes)
}

/// The `.xpaddercontroller` beside an Xpadder profile, if there is one.
pub(crate) fn controller_beside(profile: &Path) -> Option<PathBuf> {
    let dir = profile.parent()?;
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("xpaddercontroller")))
        .collect();
    found.sort();
    found.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG of the given size, all one colour.
    fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let data: Vec<u8> = (0..width * height).flat_map(|_| rgba).collect();
        writer.write_image_data(&data).unwrap();
        writer.finish().unwrap();
        out
    }

    #[test]
    fn large_pictures_are_scaled_to_fit_the_graphics_card() {
        let image = decode_png(&png(300, 100, [10, 20, 30, 255]), "test", 128).unwrap();
        assert_eq!(image.size, [100, 34]);
        assert_eq!(image.pixels[0], egui::Color32::from_rgb(10, 20, 30));
        let small = decode_png(&png(64, 32, [10, 20, 30, 255]), "test", 128).unwrap();
        assert_eq!(small.size, [64, 32]);
    }

    /// A one-pixel PNG whose header claims another size, as a hostile or broken file might.
    fn claiming(width: u32, height: u32) -> Vec<u8> {
        fn crc32(data: &[u8]) -> u32 {
            let mut crc = 0xFFFF_FFFFu32;
            for &byte in data {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
                }
            }
            !crc
        }
        let mut bytes = png(1, 1, [0, 0, 0, 255]);
        // Signature (8), IHDR length (4) and type (4), then width and height.
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        let crc = crc32(&bytes[12..29]);
        bytes[29..33].copy_from_slice(&crc.to_be_bytes());
        bytes
    }

    #[test]
    fn huge_pictures_are_refused_before_decoding() {
        let error = decode_png(&claiming(9000, 10), "test", 2048).unwrap_err();
        assert!(error.contains("too large"), "{error}");
        assert!(decode_png(&claiming(4100, 4100), "test", 2048).is_err());
        assert!(decode_png(&claiming(u32::MAX / 2, u32::MAX / 2), "test", 2048).is_err());
    }

    #[test]
    fn transparent_pixels_do_not_tint_a_scaled_picture() {
        // Half magenta (transparent), half white: the average is white at half opacity.
        let mut rgba = vec![];
        rgba.extend_from_slice(&[255, 0, 255, 0]);
        rgba.extend_from_slice(&[255, 255, 255, 255]);
        let (out, width, height) = downscale(&rgba, 2, 1, 2);
        assert_eq!((width, height), (1, 1));
        assert_eq!(out, [255, 255, 255, 127]);
    }

    #[test]
    fn import_names_are_unique_per_file_and_stable() {
        let a = import_name("Xbox: pad", b"one");
        assert_eq!(a, import_name("Xbox: pad", b"one"));
        assert!(a.starts_with("Xbox_ pad-"), "{a}");
        assert_eq!(a.len(), "Xbox_ pad-".len() + 12);
        assert_ne!(a, import_name("Xbox: pad", b"two"));
        assert!(import_name("", b"one").starts_with("controller-"));
    }

    #[test]
    fn importing_the_same_controller_again_reuses_the_files() {
        let root = std::env::temp_dir().join(format!("zpadder-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let packs = [root.join("pack one"), root.join("pack two")];
        let layouts = root.join("layouts");
        let contents = [
            "DataType=Controller\r\nButton1Location=100,100\r\n[Image]\r\nImageLine1=0,FF\r\n",
            "DataType=Controller\r\nButton1Location=200,50\r\n",
        ];
        let mut paths = Vec::new();
        for (pack, text) in packs.iter().zip(contents) {
            std::fs::create_dir_all(pack).unwrap();
            let controller = pack.join("Pad.xpaddercontroller");
            std::fs::write(&controller, text).unwrap();
            paths.push(import_controller_into(&controller, &layouts).unwrap().0);
        }
        // Same name, different packs: two layouts.
        assert_ne!(paths[0], paths[1]);
        assert!(paths.iter().all(|p| p.exists()));

        // Importing again finds the same file and leaves it alone.
        let before = std::fs::metadata(&paths[0]).unwrap().modified().unwrap();
        let again = import_controller_into(&packs[0].join("Pad.xpaddercontroller"), &layouts);
        assert_eq!(again.unwrap().0, paths[0]);
        assert_eq!(std::fs::metadata(&paths[0]).unwrap().modified().unwrap(), before);

        // A damaged earlier import is written again.
        std::fs::write(&paths[0], "damaged").unwrap();
        import_controller_into(&packs[0].join("Pad.xpaddercontroller"), &layouts).unwrap();
        assert!(zpadder_config::load_layout(&paths[0]).is_ok());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
