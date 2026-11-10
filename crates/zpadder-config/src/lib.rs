//! Loading and saving ZPadder's native profile files.
//!
//! A profile file is TOML: a `format_version` line followed by the [`Profile`] fields. Files
//! from older format versions will be migrated on load; files from newer versions are refused
//! rather than half-read. Controller layouts (`*.layout.toml`) work the same way, with their own
//! version number.
//!
//! Files are read only up to [`MAX_FILE_SIZE`], and written with [`write_atomic`], so a crash
//! or full disk while saving never leaves half a file behind.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use zpadder_import_xpadder::ImportError;
use zpadder_model::profile::Layer;
use zpadder_model::{Control, Layout, Profile, VirtualPad};

/// The profile format version this build reads and writes.
pub const FORMAT_VERSION: u32 = 1;

/// The controller layout format version this build reads and writes.
pub const LAYOUT_FORMAT_VERSION: u32 = 1;

/// The largest profile, layout or Xpadder file read, in bytes. Real files are far smaller;
/// anything bigger is refused rather than read into memory.
pub const MAX_FILE_SIZE: u64 = zpadder_import_xpadder::MAX_FILE_SIZE as u64;

/// How many `.xpaddercontroller` files beside an Xpadder profile are tried for its button
/// order.
const MAX_NEARBY_CONTROLLERS: usize = 8;

/// Why a profile could not be loaded.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    /// The file is larger than [`MAX_FILE_SIZE`].
    TooLarge,
    /// The file is not valid TOML or does not match the profile format.
    Parse(toml::de::Error),
    MissingVersion,
    /// `format_version` is not a whole number; holds the kind of value found, e.g. `string`.
    VersionNotInteger(&'static str),
    /// The file's format version is one this build cannot read or migrate.
    UnsupportedVersion(i64),
    /// The file was written by a newer ZPadder, in a format version this build does not know.
    NewerVersion(i64),
    /// The file parsed but describes an impossible profile; one message per problem.
    Invalid(Vec<String>),
    /// An Xpadder file could not be imported.
    Import(ImportError),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::TooLarge => write!(
                f,
                "the file is larger than {} MB, too large for a ZPadder or Xpadder file",
                MAX_FILE_SIZE / (1024 * 1024)
            ),
            Self::Parse(e) => write!(f, "{e}"),
            Self::MissingVersion => write!(
                f,
                "missing `format_version` (the first line of a ZPadder file, such as \
                 `format_version = 1`)"
            ),
            Self::VersionNotInteger(kind) => {
                write!(f, "`format_version` must be a whole number such as 1, not a {kind}")
            }
            Self::UnsupportedVersion(v) => {
                write!(f, "format_version {v} is not a format version ZPadder has ever used")
            }
            Self::NewerVersion(v) => write!(
                f,
                "this file was made by a newer ZPadder (format_version {v}); update ZPadder to \
                 open it"
            ),
            Self::Invalid(errors) => write!(f, "{}", errors.join("\n")),
            Self::Import(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
            Self::Import(e) => Some(e),
            _ => None,
        }
    }
}

/// A ZPadder profile: what each controller input does, in switchable layers.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "ZPadder profile")]
struct FileV1 {
    /// Version of the profile file format. Always 1.
    #[allow(dead_code, reason = "checked before the strict parse")]
    #[schemars(range(min = 1, max = 1))]
    format_version: u32,
    /// Display name of the profile.
    #[serde(default)]
    name: String,
    /// Controller picture to show in the editor: a built-in photo (`builtin:dualshock4`,
    /// `builtin:xbox-one`, `builtin:xbox-360`, `builtin:switch-pro`) or a layout file
    /// relative to this profile.
    #[serde(default)]
    layout: Option<String>,
    /// Also presents each controller to games as a virtual Xbox 360 controller (`xbox360`).
    /// Controls with nothing assigned in the active layer act as its buttons, sticks and
    /// triggers. Needs the free ViGEmBus driver; see docs/profiles.md.
    #[serde(default)]
    virtual_pad: Option<VirtualPad>,
    /// The first layer is active when the profile loads.
    #[schemars(length(min = 1))]
    layers: Vec<Layer>,
}

/// JSON schema for profile files, used by editors that validate and complete TOML (for example
/// Even Better TOML in VS Code, through a `#:schema` comment).
pub fn json_schema() -> String {
    let schema = schemars::schema_for!(FileV1);
    let mut schema = serde_json::to_value(schema).expect("schemas always serialize");
    tidy_floats(&mut schema);
    serde_json::to_string_pretty(&schema).expect("schemas always serialize") + "\n"
}

/// Writes numbers that started out as `f32`, such as the default threshold 0.2 (widened to
/// 0.20000000298023224), as they are written in profiles.
fn tidy_floats(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Number(number) if number.is_f64() => {
            let wide = number.as_f64().expect("is_f64");
            let narrow = wide as f32;
            if f64::from(narrow) == wide
                && let Ok(tidy) = narrow.to_string().parse::<f64>()
                && let Some(tidy) = serde_json::Number::from_f64(tidy)
            {
                *number = tidy;
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(tidy_floats),
        serde_json::Value::Object(fields) => fields.values_mut().for_each(tidy_floats),
        _ => {}
    }
}

#[derive(Serialize)]
struct FileRef<'a> {
    format_version: u32,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    layout: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    virtual_pad: Option<VirtualPad>,
    layers: &'a [Layer],
}

/// Checks `format_version` before the strict parse, so files without one, or from another
/// format version, get a clear message instead of field errors. `current` is the version this
/// build reads.
fn check_version(text: &str, current: u32) -> Result<(), ConfigError> {
    let table: toml::Table = text.parse().map_err(ConfigError::Parse)?;
    let version = match table.get("format_version") {
        None => return Err(ConfigError::MissingVersion),
        Some(toml::Value::Integer(version)) => *version,
        Some(other) => return Err(ConfigError::VersionNotInteger(other.type_str())),
    };
    match version {
        v if v == i64::from(current) => Ok(()),
        v if v > i64::from(current) => Err(ConfigError::NewerVersion(v)),
        // Older formats are migrated here, once there are any. Version 1 is the first.
        v => Err(ConfigError::UnsupportedVersion(v)),
    }
}

/// Parses and validates a profile from TOML text.
pub fn parse(text: &str) -> Result<Profile, ConfigError> {
    check_version(text, FORMAT_VERSION)?;
    let file: FileV1 = toml::from_str(text).map_err(ConfigError::Parse)?;
    let profile = Profile {
        name: file.name,
        layout: file.layout,
        virtual_pad: file.virtual_pad,
        layers: file.layers,
    };
    profile.validate().map_err(ConfigError::Invalid)?;
    Ok(profile)
}

/// Formats a profile as TOML.
pub fn to_string(profile: &Profile) -> String {
    let file = FileRef {
        format_version: FORMAT_VERSION,
        name: &profile.name,
        layout: profile.layout.as_deref(),
        virtual_pad: profile.virtual_pad,
        layers: &profile.layers,
    };
    toml::to_string_pretty(&file).expect("profiles always serialize")
}

pub fn load(path: &Path) -> Result<Profile, ConfigError> {
    parse(&read_text(path)?)
}

/// Reads a whole file, refusing one larger than [`MAX_FILE_SIZE`].
fn read_capped(path: &Path) -> Result<Vec<u8>, ConfigError> {
    let file = std::fs::File::open(path).map_err(ConfigError::Io)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_SIZE + 1).read_to_end(&mut bytes).map_err(ConfigError::Io)?;
    if bytes.len() as u64 > MAX_FILE_SIZE {
        return Err(ConfigError::TooLarge);
    }
    Ok(bytes)
}

/// Reads a whole UTF-8 file, refusing one larger than [`MAX_FILE_SIZE`].
fn read_text(path: &Path) -> Result<String, ConfigError> {
    String::from_utf8(read_capped(path)?)
        .map_err(|e| ConfigError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))
}

/// A profile loaded by [`load_any`].
#[derive(Debug, Clone)]
pub struct Loaded {
    pub profile: Profile,
    /// Imported from Xpadder: saving it means writing a new ZPadder (`.toml`) file.
    pub imported: bool,
    /// What an import could not carry over, and which button order it assumed.
    pub warnings: Vec<String>,
}

/// Whether `path` names an Xpadder profile.
pub fn is_xpadder_profile(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("xpadderprofile"))
}

/// Loads a ZPadder profile, or imports an Xpadder `.xpadderprofile`. An Xpadder profile takes
/// its button order from a `.xpaddercontroller` in the same folder when there is one (as in
/// shared Xpadder packs), otherwise the Xbox order is assumed. Either way the profile returned
/// has passed [`Profile::validate`].
pub fn load_any(path: &Path) -> Result<Loaded, ConfigError> {
    if !is_xpadder_profile(path) {
        return Ok(Loaded { profile: load(path)?, imported: false, warnings: Vec::new() });
    }
    let bytes = read_capped(path)?;
    let (buttons, mut warnings) = nearby_button_order(path);
    let imported =
        zpadder_import_xpadder::import_profile(&bytes, &buttons).map_err(ConfigError::Import)?;
    let mut profile = imported.profile;
    if profile.name.trim().is_empty() {
        profile.name =
            path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    }
    profile.validate().map_err(ConfigError::Invalid)?;
    warnings.extend(imported.warnings);
    Ok(Loaded { profile, imported: true, warnings })
}

/// The button order from the first readable `.xpaddercontroller` beside `profile` (trying at
/// most a few), or the Xbox order.
fn nearby_button_order(profile: &Path) -> (zpadder_import_xpadder::ButtonMap, Vec<String>) {
    let dir = profile.parent().unwrap_or(Path::new("."));
    let mut controllers: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("xpaddercontroller")))
        .collect();
    controllers.sort();
    for controller in controllers.into_iter().take(MAX_NEARBY_CONTROLLERS) {
        let name =
            controller.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let Ok(bytes) = read_capped(&controller) else { continue };
        if let Ok(imported) = zpadder_import_xpadder::import_controller(&bytes, &name) {
            let note = format!("Button order taken from {}", controller.display());
            return (imported.buttons, vec![note]);
        }
    }
    let note = "No .xpaddercontroller next to the profile, so the Xbox button order was assumed"
        .to_string();
    (zpadder_import_xpadder::ButtonMap::default(), vec![note])
}

pub fn save(path: &Path, profile: &Profile) -> Result<(), ConfigError> {
    write_atomic(path, to_string(profile).as_bytes()).map_err(ConfigError::Io)
}

/// Replaces the file at `path` with `bytes` all at once: they are written to a temporary file
/// in the same folder, flushed to disk, then renamed over `path` (which replaces it on Windows
/// too). Readers see the old file or the new one, never a mix, and a failed write leaves the
/// old file untouched and no temporary file behind.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "the path has no file name")
    })?;
    let unique = format!(
        ".{}.{}-{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let temporary: PathBuf = path.with_file_name(unique);
    let written = (|| {
        let mut file = std::fs::File::create_new(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

/// A controller layout file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutFileV1 {
    #[allow(dead_code, reason = "checked before the strict parse")]
    format_version: u32,
    name: String,
    #[serde(default)]
    image: Option<String>,
    size: [f32; 2],
    #[serde(default)]
    positions: BTreeMap<Control, [f32; 2]>,
}

/// Parses and validates a controller layout (`*.layout.toml`) from TOML text.
pub fn parse_layout(text: &str) -> Result<Layout, ConfigError> {
    check_version(text, LAYOUT_FORMAT_VERSION)?;
    let file: LayoutFileV1 = toml::from_str(text).map_err(ConfigError::Parse)?;
    let layout =
        Layout { name: file.name, image: file.image, size: file.size, positions: file.positions };
    layout.validate().map_err(ConfigError::Invalid)?;
    Ok(layout)
}

/// Formats a controller layout as TOML.
pub fn layout_to_string(layout: &Layout) -> String {
    let mut table = toml::Table::try_from(layout).expect("layouts always serialize");
    table.insert("format_version".into(), toml::Value::Integer(LAYOUT_FORMAT_VERSION.into()));
    toml::to_string_pretty(&table).expect("layouts always serialize")
}

pub fn load_layout(path: &Path) -> Result<Layout, ConfigError> {
    parse_layout(&read_text(path)?)
}

pub fn save_layout(path: &Path, layout: &Layout) -> Result<(), ConfigError> {
    write_atomic(path, layout_to_string(layout).as_bytes()).map_err(ConfigError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../profiles/examples/notepad-test.toml");

    /// A fresh folder under the system temp folder, removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("zpadder-config-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn example_profile_loads_and_round_trips() {
        let profile = parse(EXAMPLE).unwrap();
        assert_eq!(profile.layers.len(), 2);
        assert_eq!(parse(&to_string(&profile)).unwrap(), profile);
    }

    #[test]
    fn virtual_pad_round_trips_and_is_off_by_default() {
        let body = "[[layers]]
name = \"Main\"
";
        let plain = parse(&format!(
            "format_version = 1
{body}"
        ))
        .unwrap();
        assert_eq!(plain.virtual_pad, None);
        assert!(!to_string(&plain).contains("virtual_pad"));

        let text = format!(
            "format_version = 1
virtual_pad = \"xbox360\"
{body}"
        );
        let profile = parse(&text).unwrap();
        assert_eq!(profile.virtual_pad, Some(VirtualPad::Xbox360));
        let saved = to_string(&profile);
        assert!(saved.contains("virtual_pad = \"xbox360\""), "{saved}");
        assert_eq!(parse(&saved).unwrap(), profile);

        // Only known controllers are accepted, and the format version stays 1.
        let error = parse(&format!(
            "format_version = 1
virtual_pad = \"ds4\"
{body}"
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("xbox360"), "{error}");
        assert!(
            saved.starts_with(
                "format_version = 1
"
            ),
            "{saved}"
        );
    }

    #[test]
    fn xpadder_profiles_import_through_load_any() {
        let dir = TempDir::new("load-any");
        let path = dir.0.join("Game.xpadderprofile");
        let text = ";--- Xpadder Profile File ---
DataType=Profile
Version=2012.12.31
                    [Assignments]
Set1Button1Slots=Space
";
        std::fs::write(&path, text).unwrap();
        let loaded = load_any(&path).unwrap();
        assert!(loaded.imported);
        assert_eq!(loaded.profile.validate(), Ok(()));
        assert_eq!(loaded.profile.name, "Game");
        let south = &loaded.profile.layers[0].buttons[&zpadder_model::Button::South];
        assert_eq!(south.slots, ["space".parse().unwrap()]);
        assert!(loaded.warnings.iter().any(|w| w.contains("Xbox")), "{:?}", loaded.warnings);
    }

    #[test]
    fn import_errors_keep_their_type() {
        let dir = TempDir::new("import-error");
        let path = dir.0.join("Pad.xpadderprofile");
        std::fs::write(&path, "DataType=Controller\r\nButton1Location=1,2\r\n").unwrap();
        let error = load_any(&path).unwrap_err();
        assert!(matches!(error, ConfigError::Import(ImportError::WrongKind { .. })), "{error:?}");
        let source = std::error::Error::source(&error).unwrap();
        assert_eq!(source.to_string(), error.to_string());
    }

    #[test]
    fn oversized_files_are_refused() {
        let dir = TempDir::new("too-large");
        let big = vec![b' '; MAX_FILE_SIZE as usize + 1];
        for name in ["big.toml", "big.xpadderprofile", "big.layout.toml"] {
            std::fs::write(dir.0.join(name), &big).unwrap();
        }
        assert!(matches!(load(&dir.0.join("big.toml")), Err(ConfigError::TooLarge)));
        assert!(matches!(load_any(&dir.0.join("big.xpadderprofile")), Err(ConfigError::TooLarge)));
        assert!(matches!(load_layout(&dir.0.join("big.layout.toml")), Err(ConfigError::TooLarge)));
    }

    #[test]
    fn only_a_few_nearby_controllers_are_tried() {
        let dir = TempDir::new("nearby");
        // Sorted first, and none of them a controller file.
        for i in 0..MAX_NEARBY_CONTROLLERS {
            std::fs::write(dir.0.join(format!("{i}.xpaddercontroller")), "junk").unwrap();
        }
        let good = "DataType=Controller\r\nButton1Name=A\r\nButton1Location=1,2\r\n";
        std::fs::write(dir.0.join("z.xpaddercontroller"), good).unwrap();
        let path = dir.0.join("Game.xpadderprofile");
        std::fs::write(&path, "DataType=Profile\r\nSet1Button1Slots=Space\r\n").unwrap();
        let loaded = load_any(&path).unwrap();
        assert!(loaded.warnings[0].contains("Xbox button order"), "{:?}", loaded.warnings);
        std::fs::remove_file(dir.0.join("0.xpaddercontroller")).unwrap();
        let loaded = load_any(&path).unwrap();
        assert!(loaded.warnings[0].contains("z.xpaddercontroller"), "{:?}", loaded.warnings);
    }

    #[test]
    fn saving_replaces_the_file_atomically() {
        let dir = TempDir::new("save");
        let path = dir.0.join("profile.toml");
        let profile = parse(EXAMPLE).unwrap();
        std::fs::write(&path, "old").unwrap();
        save(&path, &profile).unwrap();
        assert_eq!(load(&path).unwrap(), profile);
        write_atomic(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        // Only the file itself is left: no temporary files.
        let names: Vec<_> =
            std::fs::read_dir(&dir.0).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, ["profile.toml"]);

        // A failed write leaves nothing behind: here the folder does not exist.
        let missing = dir.0.join("missing").join("profile.toml");
        assert!(write_atomic(&missing, b"x").is_err());
        assert!(write_atomic(Path::new(""), b"x").is_err());
        let names: Vec<_> =
            std::fs::read_dir(&dir.0).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, ["profile.toml"]);
    }

    #[test]
    fn layouts_round_trip() {
        let text = "format_version = 1\nname = \"Pad\"\nimage = \"pad.png\"\nsize = [512.0, 256.0]\n\
                    [positions]\nsouth = [376.0, 136.0]\nright_trigger = [478.0, 4.0]\n";
        let layout = parse_layout(text).unwrap();
        assert_eq!(layout.positions.len(), 2);
        assert_eq!(parse_layout(&layout_to_string(&layout)).unwrap(), layout);
        assert!(parse_layout("format_version = 1\nname = \"x\"\nsize = [1.0, 1.0]\n[positions]\nnope = [0.0, 0.0]\n").is_err());
    }

    #[test]
    fn layout_errors_say_where_and_what() {
        let text =
            "format_version = 1\nname = \"x\"\nsize = [1.0, 1.0]\n[positions]\nnope = [0.0, 0.0]\n";
        let error = parse_layout(text).unwrap_err().to_string();
        assert!(error.contains("line 5") && error.contains("unknown control `nope`"), "{error}");

        let invalid =
            |body: &str| match parse_layout(&format!("format_version = 1\nname = \"x\"\n{body}")) {
                Err(ConfigError::Invalid(errors)) => errors,
                other => panic!("{other:?}"),
            };
        assert!(invalid("size = [0.0, 1.0]\n")[0].contains("size"));
        assert!(invalid("size = [1.0, nan]\n")[0].contains("size"));
        assert!(invalid("image = \"../x.png\"\nsize = [1.0, 1.0]\n")[0].contains("image"));
        assert!(invalid("image = \"C:\\\\x.png\"\nsize = [1.0, 1.0]\n")[0].contains("image"));
        assert!(
            invalid("size = [1.0, 1.0]\n[positions]\nsouth = [inf, 0.0]\n")[0]
                .contains("positions.south")
        );
    }

    #[test]
    fn committed_schema_is_current() {
        let committed = include_str!("../../../schema/profile.schema.json");
        assert!(
            committed.replace("\r\n", "\n") == json_schema(),
            "schema/profile.schema.json is out of date; regenerate it with\n  \
             cargo run -p zpadder --bin zpadder-cli -- schema schema/profile.schema.json"
        );
    }

    #[test]
    fn schema_defaults_are_written_as_in_profiles() {
        let schema = json_schema();
        assert!(schema.contains("\"default\": 0.2,"), "{schema}");
        assert!(!schema.contains("0000000"), "{schema}");
        assert!(schema.contains("\"minItems\": 1"), "{schema}");
    }

    #[test]
    fn version_is_required_and_checked() {
        let body = "[[layers]]\nname = \"Main\"\n";
        assert!(matches!(parse(body), Err(ConfigError::MissingVersion)));
        let newer = format!("format_version = 99\n{body}");
        assert!(matches!(parse(&newer), Err(ConfigError::NewerVersion(99))));
        assert!(parse(&newer).unwrap_err().to_string().contains("update ZPadder"));
        let older = format!("format_version = 0\n{body}");
        assert!(matches!(parse(&older), Err(ConfigError::UnsupportedVersion(0))));
        let text = format!("format_version = \"1\"\n{body}");
        assert!(matches!(parse(&text), Err(ConfigError::VersionNotInteger("string"))));
        assert!(parse(&format!("format_version = 1\n{body}")).is_ok());
        // Layouts have a version of their own.
        assert!(matches!(
            parse_layout("format_version = 2\nname = \"x\"\nsize = [1.0, 1.0]\n"),
            Err(ConfigError::NewerVersion(2))
        ));
    }

    #[test]
    fn typos_are_reported_not_ignored() {
        let text = "format_version = 1\n[[layers]]\nname = \"Main\"\ntrigger_treshold = 0.3\n";
        let error = parse(text).unwrap_err().to_string();
        assert!(error.contains("trigger_treshold"), "{error}");

        let text = "format_version = 1\n[[layers]]\nname = \"Main\"\n\
                    [layers.buttons.south]\nslots = [\"left_shfit\"]\n";
        let error = parse(text).unwrap_err().to_string();
        assert!(error.contains("left_shfit"), "{error}");
    }

    #[test]
    fn invalid_profiles_list_every_problem() {
        let text = "format_version = 1\n[[layers]]\nname = \"Main\"\n\
                    [layers.buttons.south]\nturbo = 500.0\nlayer = { target = \"Nope\" }\n";
        let ConfigError::Invalid(errors) = parse(text).unwrap_err() else { panic!() };
        assert_eq!(errors.len(), 2, "{errors:?}");
    }

    #[test]
    fn templates_and_examples_are_valid() {
        for text in [
            EXAMPLE,
            include_str!("../../../profiles/templates/blank-dualshock4.toml"),
            include_str!("../../../profiles/templates/desktop.toml"),
            include_str!("../../../profiles/templates/fps.toml"),
            include_str!("../../../profiles/templates/platformer.toml"),
            include_str!("../../../profiles/templates/xbox-controller.toml"),
        ] {
            parse(text).unwrap();
        }
        for text in [
            include_str!("../../../assets/controllers/dualshock4.layout.toml"),
            include_str!("../../../assets/controllers/switch-pro.layout.toml"),
            include_str!("../../../assets/controllers/xbox-360.layout.toml"),
            include_str!("../../../assets/controllers/xbox-one.layout.toml"),
        ] {
            parse_layout(text).unwrap();
        }
    }
}
