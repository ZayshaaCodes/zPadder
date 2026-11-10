//! File helpers: file names Windows accepts, layout paths that keep working when a profile moves
//! to another folder, names for imported files, and opening files and folders the way Explorer
//! would. Files are written with [`zpadder_config::write_atomic`], which never leaves a
//! half-written file.

use std::path::{Component, Path, PathBuf};

use zpadder_config::{ConfigError, Loaded};
use zpadder_model::Profile;

use crate::{builtin, picture};

/// Loads a profile to edit, like [`zpadder_config::load_any`], but also one that loads and
/// reads fine yet has problems (a value out of range, a switch to a missing layer): it opens
/// anyway, with the problems listed, so they can be fixed here. Returns the problems too.
pub(crate) fn load_for_editing(path: &Path) -> Result<(Loaded, Vec<String>), String> {
    let problems = match zpadder_config::load_any(path) {
        Ok(loaded) => return Ok((loaded, Vec::new())),
        Err(ConfigError::Invalid(problems)) => problems,
        Err(e) => return Err(e.to_string()),
    };
    let bytes = read_capped(path)?;
    let loaded = if zpadder_config::is_xpadder_profile(path) {
        // The button order from the pack's controller file, as the first try did.
        let buttons = picture::controller_beside(path)
            .and_then(|controller| {
                let bytes = read_capped(&controller).ok()?;
                let name = controller.file_stem()?.to_string_lossy().into_owned();
                zpadder_import_xpadder::import_controller(&bytes, &name).ok()
            })
            .map(|controller| controller.buttons)
            .unwrap_or_default();
        let imported =
            zpadder_import_xpadder::import_profile(&bytes, &buttons).map_err(|e| e.to_string())?;
        let mut profile = imported.profile;
        if profile.name.trim().is_empty() {
            profile.name =
                path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        }
        Loaded { profile, imported: true, warnings: imported.warnings }
    } else {
        let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
        let (profile, _) = parse_for_editing(&text)?;
        Loaded { profile, imported: false, warnings: Vec::new() }
    };
    Ok((loaded, problems))
}

/// Parses a profile like [`zpadder_config::parse`], but one with problems too; returns them.
pub(crate) fn parse_for_editing(text: &str) -> Result<(Profile, Vec<String>), String> {
    let problems = match zpadder_config::parse(text) {
        Ok(profile) => return Ok((profile, Vec::new())),
        Err(ConfigError::Invalid(problems)) => problems,
        Err(e) => return Err(e.to_string()),
    };
    // It parsed, so it has the right version and fields; only the checks failed.
    let mut table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    table.remove("format_version");
    let profile: Profile = table.try_into().map_err(|e: toml::de::Error| e.to_string())?;
    Ok((profile, problems))
}

/// Reads a whole file, refusing one larger than ZPadder ever reads.
fn read_capped(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let limit = zpadder_config::MAX_FILE_SIZE;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(ConfigError::TooLarge.to_string());
    }
    Ok(bytes)
}

/// Device names Windows reserves, with or without an extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// `name` made into a file name Windows accepts: characters it forbids become `_`, trailing dots
/// and spaces go, reserved device names get a `_`, and an empty name becomes `fallback`.
pub(crate) fn safe_file_stem(name: &str, fallback: &str) -> String {
    let replaced: String = name
        .trim()
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .take(120)
        .collect();
    let stem = replaced.trim_end_matches(['.', ' ']).trim_start();
    if stem.is_empty() {
        return fallback.to_string();
    }
    let device = stem.split('.').next().unwrap_or_default().trim_end();
    if RESERVED.iter().any(|reserved| device.eq_ignore_ascii_case(reserved)) {
        format!("{stem}_")
    } else {
        stem.to_string()
    }
}

/// `path` with a `.toml` extension: kept if it has one, otherwise added.
pub(crate) fn with_toml_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("toml")) {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".toml");
    PathBuf::from(name)
}

/// A profile's `layout` value for the profile saved in `new_dir` instead of `old_dir`. Built-in
/// pictures and absolute paths stay as they are. A relative path is resolved against `old_dir`
/// and written relative to `new_dir` when the layout is inside it, absolute otherwise.
pub(crate) fn rebase_layout(layout: &str, old_dir: Option<&Path>, new_dir: &Path) -> String {
    let path = Path::new(layout);
    if layout.starts_with(builtin::PREFIX) || path.is_absolute() {
        return layout.to_string();
    }
    let Some(old_dir) = old_dir else { return layout.to_string() };
    let old_dir = std::path::absolute(old_dir).unwrap_or_else(|_| old_dir.to_owned());
    let new_dir = std::path::absolute(new_dir).unwrap_or_else(|_| new_dir.to_owned());
    if normalize(&old_dir) == normalize(&new_dir) {
        return layout.to_string();
    }
    let target = normalize(&old_dir.join(path));
    let rebased = match target.strip_prefix(normalize(&new_dir)) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.to_owned(),
        _ => target,
    };
    rebased.to_str().map_or_else(|| layout.to_string(), str::to_string)
}

/// `path` with `.` and `..` resolved without asking the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// A 64-bit FNV-1a hash: stable across Rust versions and runs, unlike the standard hasher, so it
/// can name files.
pub(crate) fn content_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Opens a file with the program Windows associates with it, or a folder in Explorer.
pub(crate) fn open_in_shell(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let opened = zpadder_platform::shell_open(path);
    #[cfg(not(windows))]
    let opened: std::io::Result<()> = Err(std::io::Error::other("not supported here"));
    opened.map_err(|e| format!("Cannot open {}: {e}", path.display()))
}

/// A file from the documentation that ships with ZPadder (`docs\<name>` beside the executable),
/// if it is there. Development builds find the repository's `docs` folder above `target`.
pub(crate) fn docs_file(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).take(4).map(|dir| dir.join("docs").join(name)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_made_safe_for_windows() {
        assert_eq!(
            safe_file_stem("Shooter: Deluxe/Edition?", "Profile"),
            "Shooter_ Deluxe_Edition_"
        );
        assert_eq!(safe_file_stem("  Trailing dots... ", "Profile"), "Trailing dots");
        assert_eq!(safe_file_stem("a\tb", "Profile"), "a_b");
        assert_eq!(safe_file_stem("   ", "Profile"), "Profile");
        assert_eq!(safe_file_stem("...", "Profile"), "Profile");
        assert_eq!(safe_file_stem("con", "Profile"), "con_");
        assert_eq!(safe_file_stem("NUL.backup", "Profile"), "NUL.backup_");
        assert_eq!(safe_file_stem("Console", "Profile"), "Console");
        assert_eq!(safe_file_stem(&"x".repeat(300), "Profile").len(), 120);
    }

    #[test]
    fn saved_profiles_get_a_toml_extension() {
        assert_eq!(with_toml_extension("a/game.toml".into()), PathBuf::from("a/game.toml"));
        assert_eq!(with_toml_extension("a/game.TOML".into()), PathBuf::from("a/game.TOML"));
        assert_eq!(with_toml_extension("a/game".into()), PathBuf::from("a/game.toml"));
        assert_eq!(with_toml_extension("a/game.v2".into()), PathBuf::from("a/game.v2.toml"));
    }

    #[test]
    fn relative_layouts_follow_the_profile_to_its_new_folder() {
        let root = std::env::temp_dir().join("zpadder-rebase");
        let old = root.join("old");
        let new = root.join("new");
        // Moved elsewhere: the layout is written in full.
        let moved = rebase_layout("pads/pad.layout.toml", Some(&old), &new);
        assert_eq!(PathBuf::from(moved), old.join("pads").join("pad.layout.toml"));
        // Saved into a parent folder: still relative, now through the subfolder.
        let up = rebase_layout("pad.layout.toml", Some(&new.join("sub")), &new);
        assert_eq!(PathBuf::from(up), PathBuf::from("sub").join("pad.layout.toml"));
        // `..` is resolved.
        let parent = rebase_layout("../pad.layout.toml", Some(&old), &root);
        assert_eq!(PathBuf::from(parent), PathBuf::from("pad.layout.toml"));
        // The same folder, built-in pictures, absolute paths and unknown origins stay put.
        assert_eq!(rebase_layout("pad.layout.toml", Some(&old), &old), "pad.layout.toml");
        assert_eq!(rebase_layout("builtin:dualshock4", Some(&old), &new), "builtin:dualshock4");
        let absolute = old.join("pad.layout.toml").display().to_string();
        assert_eq!(rebase_layout(&absolute, Some(&root), &new), absolute);
        assert_eq!(rebase_layout("pad.layout.toml", None, &new), "pad.layout.toml");
    }

    #[test]
    fn profiles_with_problems_still_open_for_editing() {
        let text = "format_version = 1\nname = \"Old\"\n[[layers]]\nname = \"Main\"\n\
                    trigger_threshold = 0.01\n";
        let (profile, problems) = parse_for_editing(text).unwrap();
        assert_eq!(profile.name, "Old");
        assert_eq!(profile.layers[0].trigger_threshold, 0.01);
        assert_eq!(problems.len(), 1, "{problems:?}");
        // Files that do not read at all are still refused.
        assert!(parse_for_editing("format_version = 1\n[[layers]]\nnam = \"x\"\n").is_err());
        assert!(parse_for_editing("[[layers]]\nname = \"Main\"\n").is_err());
        let fine = "format_version = 1\n[[layers]]\nname = \"Main\"\n";
        assert_eq!(parse_for_editing(fine).unwrap().1, Vec::<String>::new());
    }

    #[test]
    fn content_hashes_are_stable() {
        assert_eq!(content_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(content_hash(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_ne!(content_hash(b"pad one"), content_hash(b"pad two"));
    }
}
