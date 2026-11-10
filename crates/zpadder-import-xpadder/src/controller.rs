//! `.xpaddercontroller` import: the picture, where each control sits, and the button order.

use std::collections::BTreeMap;

use zpadder_model::{Button, Control, Layout};

use crate::buttons::{self, ButtonMap};
use crate::image::{self, HEIGHT, TRANSPARENT, WIDTH};
use crate::text::{KeyReader, shorten};
use crate::{FileKind, ImportError, Warnings};

/// Xpadder stores the top-left corner of each control's 30×30 marker; ZPadder layouts store
/// centres. (Markers sit 30 px apart in evenly spaced layouts, and a trigger at x = 478 ends
/// 4 px from the right edge, mirroring the 4 px margin of the left one.)
const MARKER_HALF: f32 = 15.0;

/// A converted controller file.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedController {
    /// The layout. `image` is a suggested file name for [`png`](Self::png), to be saved next to
    /// the layout file; it is `None` when the file has no picture.
    pub layout: Layout,
    /// The 512×256 controller picture as a PNG, with Xpadder's magenta background (and any rows
    /// the file leaves out) fully transparent.
    pub png: Option<Vec<u8>>,
    /// Which ZPadder button each Xpadder button number is, inferred from the button names. Use
    /// it to import profiles made for this controller.
    pub buttons: ButtonMap,
    /// Everything that could not be carried over, as readable sentences.
    pub warnings: Vec<String>,
}

/// Imports a `.xpaddercontroller` file. `name` names the layout and its picture; the file
/// itself has no name, so pass its file stem.
pub fn import_controller(bytes: &[u8], name: &str) -> Result<ImportedController, ImportError> {
    let doc = crate::parse(bytes)?;
    crate::expect_kind(&doc, FileKind::Controller)?;
    let mut warnings = Warnings::default();
    doc.notes(&mut warnings);

    let mut keys = Keys::default();
    for entry in doc.settings() {
        match parse_key(&entry.key) {
            Some(key) => keys.add(key, &entry.key, &entry.value, &mut warnings),
            None => warnings.push(format!("`{}`: unknown setting; ignored", shorten(&entry.key))),
        }
    }

    let names: Vec<(u8, String)> =
        keys.button_names.iter().map(|(&number, name)| (number, name.clone())).collect();
    let (buttons, notes) = buttons::infer(&names);
    warnings.extend(notes);

    // Later insertions win: a trigger's own marker over a button that reports the same
    // trigger, and a stick's middle over its click button's marker.
    let mut positions = BTreeMap::new();
    for (&number, &position) in &keys.button_locations {
        match buttons.get(number) {
            Some(control) => {
                positions.insert(control, centre(position));
            }
            None => warnings.push(format!(
                "button {number} has no ZPadder button in the button map; its position is not \
                 imported"
            )),
        }
    }
    for (&direction, &position) in &keys.dpad_locations {
        positions.insert(Control::Button(direction.dpad_button()), centre(position));
    }
    // A stick is drawn where its click button is, at the middle of its four direction markers.
    for (&stick, locations) in &keys.stick_locations {
        let button = match stick {
            1 => Button::LeftStick,
            2 => Button::RightStick,
            _ => {
                warnings.push(format!("stick {stick}: ZPadder has two sticks; ignored"));
                continue;
            }
        };
        let count = locations.len() as f32;
        let sum = locations.values().fold([0.0, 0.0], |[x, y], [a, b]| [x + a, y + b]);
        positions.insert(Control::Button(button), centre([sum[0] / count, sum[1] / count]));
    }
    for (&control, &position) in &keys.trigger_locations {
        positions.insert(control, centre(position));
    }

    let png = keys.picture(&mut warnings);
    let layout = Layout {
        name: name.to_string(),
        image: png.is_some().then(|| format!("{}.png", file_stem(name))),
        size: [WIDTH as f32, HEIGHT as f32],
        positions,
    };
    Ok(ImportedController { layout, png, buttons, warnings: warnings.into_vec() })
}

fn centre([x, y]: [f32; 2]) -> [f32; 2] {
    [x + MARKER_HALF, y + MARKER_HALF]
}

/// `name` made safe as a file name on Windows.
fn file_stem(name: &str) -> String {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect();
    let stem = stem.trim_end_matches(['.', ' ']);
    if stem.is_empty() { "controller".to_string() } else { stem.to_string() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Direction {
    Up,
    Right,
    Down,
    Left,
}

impl Direction {
    const WORDS: [(&'static str, Direction); 4] = [
        ("Up", Direction::Up),
        ("Right", Direction::Right),
        ("Down", Direction::Down),
        ("Left", Direction::Left),
    ];

    fn dpad_button(self) -> Button {
        match self {
            Direction::Up => Button::DPadUp,
            Direction::Right => Button::DPadRight,
            Direction::Down => Button::DPadDown,
            Direction::Left => Button::DPadLeft,
        }
    }
}

/// Every key a controller file can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    /// `DataType`, `Version`, display names other than buttons', and `[Access]` lines for
    /// sticks and the DPad: understood, but nothing in ZPadder uses them.
    Ignored,
    ButtonLocation(u8),
    ButtonName(u8),
    DPadLocation(Direction),
    StickLocation(u8, Direction),
    /// `Stick{n}XAccess` / `Stick{n}YAccess`: which DirectInput axis the stick reads.
    StickAccess(u8, bool),
    TriggerLocation(Control),
    Access,
    ImageLine(u32),
}

fn parse_key(key: &str) -> Option<Key> {
    let mut k = KeyReader::new(key);
    if k.is("DataType") || k.is("Version") {
        return Some(Key::Ignored);
    }
    if k.eat("ImageLine") {
        return k.rest().parse().ok().map(Key::ImageLine);
    }
    if k.eat("Button") {
        let number = k.number()?;
        return k.rest_is(&[
            ("Location", Key::ButtonLocation(number)),
            ("Name", Key::ButtonName(number)),
        ]);
    }
    if k.eat("DPad") {
        if k.is("Name") {
            return Some(Key::Ignored);
        }
        let direction = k.one_of(&Direction::WORDS)?;
        return k.rest_is(&[
            ("Location", Key::DPadLocation(direction)),
            ("Access", Key::Access),
            ("Name", Key::Ignored),
        ]);
    }
    if k.eat("Stick") {
        let Some(stick) = k.number() else {
            // `StickUpName` and friends: direction labels shared by all sticks.
            k.one_of(&Direction::WORDS)?;
            return k.is("Name").then_some(Key::Ignored);
        };
        if let Some(key) = k.rest_is(&[
            ("Name", Key::Ignored),
            ("XAccess", Key::StickAccess(stick, true)),
            ("YAccess", Key::StickAccess(stick, false)),
        ]) {
            return Some(key);
        }
        let direction = k.one_of(&Direction::WORDS)?;
        return k.is("Location").then_some(Key::StickLocation(stick, direction));
    }
    if k.eat("Trigger") {
        let control =
            k.one_of(&[("Left", Control::LeftTrigger), ("Right", Control::RightTrigger)])?;
        return k.rest_is(&[
            ("Location", Key::TriggerLocation(control)),
            ("Access", Key::Access),
            ("Name", Key::Ignored),
        ]);
    }
    None
}

/// The parts of a controller file the importer uses.
#[derive(Default)]
struct Keys {
    button_locations: BTreeMap<u8, [f32; 2]>,
    button_names: BTreeMap<u8, String>,
    dpad_locations: BTreeMap<Direction, [f32; 2]>,
    stick_locations: BTreeMap<u8, BTreeMap<Direction, [f32; 2]>>,
    stick_access: BTreeMap<(u8, bool), String>,
    trigger_locations: BTreeMap<Control, [f32; 2]>,
    image_lines: BTreeMap<u32, String>,
}

impl Keys {
    fn add(&mut self, key: Key, raw_key: &str, value: &str, warnings: &mut Warnings) {
        let mut location = || {
            let mut parts = value.split(',').map(|part| part.trim().parse::<f32>());
            match (parts.next(), parts.next(), parts.next()) {
                (Some(Ok(x)), Some(Ok(y)), None) if x.is_finite() && y.is_finite() => Some([x, y]),
                _ => {
                    warnings.push(format!(
                        "`{raw_key}`: `{}` is not an x,y position; ignored",
                        shorten(value)
                    ));
                    None
                }
            }
        };
        match key {
            Key::Ignored => {}
            Key::ButtonLocation(n) => {
                if let Some(at) = location() {
                    self.button_locations.insert(n, at);
                }
            }
            Key::ButtonName(n) => {
                self.button_names.insert(n, value.to_string());
            }
            Key::DPadLocation(direction) => {
                if let Some(at) = location() {
                    self.dpad_locations.insert(direction, at);
                }
            }
            Key::StickLocation(stick, direction) => {
                if let Some(at) = location() {
                    self.stick_locations.entry(stick).or_default().insert(direction, at);
                }
            }
            Key::StickAccess(stick, x) => {
                check_access(raw_key, value, warnings);
                if let Some(other) = self.stick_access.get(&(stick, !x))
                    && other.eq_ignore_ascii_case(value)
                {
                    warnings.push(format!(
                        "stick {stick} reads both directions from `{value}`, which looks like \
                         a mistake in the file; it does not matter to ZPadder, which reads sticks \
                         through SDL's gamepad mapping"
                    ));
                }
                self.stick_access.insert((stick, x), value.to_string());
            }
            Key::TriggerLocation(control) => {
                if let Some(at) = location() {
                    self.trigger_locations.insert(control, at);
                }
            }
            Key::Access => check_access(raw_key, value, warnings),
            Key::ImageLine(row) => {
                self.image_lines.insert(row, value.to_string());
            }
        }
    }

    /// Decodes the `[Image]` rows into a PNG. Missing or broken rows stay transparent.
    fn picture(&self, warnings: &mut Warnings) -> Option<Vec<u8>> {
        if self.image_lines.is_empty() {
            return None;
        }
        let mut rgba = vec![0u8; WIDTH * HEIGHT * 4];
        let (mut broken, mut short, mut outside) = (Vec::new(), 0, Vec::new());
        for (&row, value) in &self.image_lines {
            let y = match usize::try_from(row) {
                Ok(y @ 1..=HEIGHT) => y - 1,
                _ => {
                    outside.push(row.to_string());
                    continue;
                }
            };
            match image::decode_line(value) {
                Ok(pixels) => {
                    if pixels.len() < WIDTH {
                        short += 1;
                    }
                    let start = y * WIDTH * 4;
                    for (pixel, out) in pixels.iter().zip(rgba[start..].as_chunks_mut::<4>().0) {
                        if *pixel != TRANSPARENT {
                            let [_, r, g, b] = pixel.to_be_bytes();
                            out.copy_from_slice(&[r, g, b, 0xFF]);
                        }
                    }
                }
                Err(error) => broken.push(format!("ImageLine{row} ({error})")),
            }
        }
        if !broken.is_empty() {
            warnings.push(format!(
                "{} picture rows could not be decoded and are left transparent: {}",
                broken.len(),
                shorten(&broken.join(", "))
            ));
        }
        if short > 0 {
            warnings.push(format!(
                "{short} picture rows are narrower than {WIDTH} pixels; the rest is transparent"
            ));
        }
        if !outside.is_empty() {
            warnings.push(format!(
                "picture rows outside 1 to {HEIGHT} ignored: {}",
                shorten(&outside.join(", "))
            ));
        }
        match encode_png(&rgba) {
            Ok(png) => Some(png),
            Err(error) => {
                warnings.push(format!("the picture could not be converted to PNG: {error}"));
                None
            }
        }
    }
}

/// Warns about `[Access]` values outside the vocabulary seen in real files: `POV`,
/// `Axis X/Y/Z/RX/RY/RZ` optionally followed by ` -` (the negative half) and/or ` Full`, and
/// `Slider 1`/`Slider 2`. ZPadder reads controllers through SDL's gamepad mapping, so the
/// bindings themselves are not imported.
fn check_access(key: &str, value: &str, warnings: &mut Warnings) {
    let upper = value.trim().to_ascii_uppercase();
    let known = upper.starts_with("POV")
        || upper.starts_with("SLIDER")
        || upper.strip_prefix("AXIS ").is_some_and(|rest| {
            let rest = rest.trim_end_matches(" FULL").trim_end_matches(" -");
            ["X", "Y", "Z", "RX", "RY", "RZ"].contains(&rest)
        });
    if !known {
        warnings.push(format!("`{key}`: unknown input binding `{}`; ignored", shorten(value)));
    }
}

fn encode_png(rgba: &[u8]) -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, WIDTH as u32, HEIGHT as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XBOX: &str = "\
;--- Xpadder Controller File ---\r
DataType=Controller\r
Version=2020.05.01\r
\r
[Button Locations]\r
Button1Location=376,136\r
Button9Location=100,100\r
Button11Location=1,1\r
DPadUpLocation=173,157\r
Stick1UpLocation=108,70\r
Stick1RightLocation=140,102\r
Stick1DownLocation=108,134\r
Stick1LeftLocation=76,102\r
Stick3UpLocation=0,0\r
TriggerLeftLocation=4,4\r
TriggerRightLocation=478,4\r
\r
[Access]\r
DPadUpAccess=POV\r
Stick1XAccess=Axis Y\r
Stick1YAccess=Axis Y\r
TriggerLeftAccess=Axis Z\r
TriggerRightAccess=Axis Z -\r
\r
[Names]\r
Button1Name=A\r
Button2Name=B\r
DPadName=DPad\r
StickUpName=Up\r
TriggerLeftName=LT\r
\r
[Image]\r
ImageLine2=1,1,FF00FF,1FE,FF0000,0\r
";

    fn import(text: &str) -> ImportedController {
        import_controller(text.as_bytes(), "Xbox 360: pad").unwrap()
    }

    #[test]
    fn positions_are_centres_keyed_by_control() {
        let imported = import(XBOX);
        let at = |control| imported.layout.positions.get(&control).copied();
        assert_eq!(at(Control::Button(Button::South)), Some([391.0, 151.0]));
        assert_eq!(at(Control::Button(Button::DPadUp)), Some([188.0, 172.0]));
        // The left stick is the middle of its four markers, not its click button's marker.
        assert_eq!(at(Control::Button(Button::LeftStick)), Some([123.0, 117.0]));
        assert_eq!(at(Control::LeftTrigger), Some([19.0, 19.0]));
        assert_eq!(at(Control::RightTrigger), Some([493.0, 19.0]));
        assert_eq!(imported.layout.positions.len(), 5);
        assert_eq!(imported.layout.size, [512.0, 256.0]);
        assert_eq!(imported.layout.name, "Xbox 360: pad");
        assert_eq!(imported.layout.image.as_deref(), Some("Xbox 360_ pad.png"));
        assert_eq!(imported.buttons, ButtonMap::xbox());
        let warnings = imported.warnings.join("\n");
        assert!(warnings.contains("button 11"), "{warnings}");
        assert!(warnings.contains("stick 3"), "{warnings}");
        assert!(warnings.contains("both directions from `Axis Y`"), "{warnings}");
        assert_eq!(imported.warnings.len(), 3, "{warnings}");
    }

    #[test]
    fn picture_becomes_a_transparent_png() {
        let imported = import(XBOX);
        let png = imported.png.expect("the file has a picture");
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut rgba).unwrap();
        assert_eq!((info.width, info.height), (512, 256));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let pixel = |x: usize, y: usize| &rgba[(y * 512 + x) * 4..][..4];
        assert_eq!(pixel(0, 0), [0, 0, 0, 0]); // row 1 is missing
        assert_eq!(pixel(0, 1), [0, 0, 0, 0]); // magenta
        assert_eq!(pixel(510, 1), [255, 0, 0, 255]);
        assert_eq!(pixel(511, 1), [0, 0, 0, 255]);
    }

    #[test]
    fn broken_and_missing_pictures() {
        let imported = import(
            "DataType=Controller\r\n[Image]\r\nImageLine1=1,ZZ\r\nImageLine3=0,FF\r\n\
             ImageLine999=0\r\n",
        );
        assert!(imported.png.is_some());
        let warnings = imported.warnings.join("\n");
        assert!(warnings.contains("ImageLine1 (token 1"), "{warnings}");
        assert!(warnings.contains("1 picture rows are narrower"), "{warnings}");
        assert!(warnings.contains("999"), "{warnings}");

        let empty = import("DataType=Controller\r\n[Image]\r\n");
        assert_eq!(empty.png, None);
        assert_eq!(empty.layout.image, None);
        assert!(empty.warnings.is_empty(), "{:?}", empty.warnings);
    }

    #[test]
    fn legacy_and_odd_files() {
        // Legacy 5.3: boxed banner, no DataType; generic names fall back to the Xbox order.
        let imported = import(
            ";-------------------------\r\n; Xpadder Controller File\r\n\
             ;-------------------------\r\n\r\nVersion=5.3\r\n\r\n[Button Locations]\r\n\
             Button1Location=1,2\r\nButton2Location=oops\r\n[Names]\r\nButton1Name=Button 1\r\n\
             StickUpName=Hoch\r\nMystery=1\r\nTriggerLeftAccess=Wheel 9\r\n",
        );
        assert_eq!(imported.layout.positions.len(), 1);
        assert_eq!(imported.buttons, ButtonMap::xbox());
        let warnings = imported.warnings.join("\n");
        assert!(warnings.contains("`Button2Location`: `oops`"), "{warnings}");
        assert!(warnings.contains("`Mystery`"), "{warnings}");
        assert!(warnings.contains("`Wheel 9`"), "{warnings}");
        assert!(warnings.contains("Xbox button order"), "{warnings}");
    }

    #[test]
    fn file_stems() {
        assert_eq!(file_stem("Xbox 360"), "Xbox 360");
        assert_eq!(file_stem(r#"a<b>c:d"e/f\g|h?i*"#), "a_b_c_d_e_f_g_h_i_");
        assert_eq!(file_stem(" . "), "controller");
        assert_eq!(file_stem("pad."), "pad");
    }

    #[test]
    fn access_vocabulary() {
        let mut warnings = Warnings::default();
        for value in ["POV", "Axis Z -", "Axis RZ Full", "Axis RY - Full", "Slider 1", "Axis X"] {
            check_access("k", value, &mut warnings);
        }
        assert_eq!(warnings.len(), 0, "{warnings:?}");
        check_access("k", "Axis Q", &mut warnings);
        assert_eq!(warnings.len(), 1);
    }
}
