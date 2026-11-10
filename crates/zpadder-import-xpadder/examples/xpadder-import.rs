//! Converts an Xpadder file to ZPadder's TOML, for trying imports by hand.
//!
//! ```text
//! cargo run -p zpadder-import-xpadder --example xpadder-import -- FILE [OUT_DIR] [--buttons B]
//! ```
//!
//! `FILE` is a `.xpadderprofile` or `.xpaddercontroller`. Without `OUT_DIR` the TOML is printed;
//! with it, a profile is written as `<name>.toml` (ready for `zpadder run`), and a controller as
//! `<name>.layout.toml` plus its PNG picture. `B` is the button map for a profile: `xbox`
//! (the default), `dualshock4`, or the `.xpaddercontroller` file the profile was made with.
//! Warnings go to stderr.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use zpadder_import_xpadder::{ButtonMap, FileKind, detect, import_controller, import_profile};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut file, mut out, mut buttons) = (None, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--buttons" => buttons = Some(args.next().ok_or("--buttons needs a value")?),
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ if out.is_none() => out = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument `{arg}`")),
        }
    }
    let file = file.ok_or("usage: xpadder-import FILE [OUT_DIR] [--buttons B]")?;
    let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

    match detect(&bytes) {
        Some(FileKind::Controller) => {
            let imported = import_controller(&bytes, &stem).map_err(|e| e.to_string())?;
            warn(&imported.warnings);
            eprintln!("button map: {:?}", imported.buttons);
            let mut table = toml::Table::try_from(&imported.layout).map_err(|e| e.to_string())?;
            table.insert("format_version".into(), toml::Value::Integer(1));
            let text = toml::to_string_pretty(&table).map_err(|e| e.to_string())?;
            let picture = imported.layout.image.as_deref().zip(imported.png.as_deref());
            match &out {
                Some(dir) => {
                    write(dir, &format!("{stem}.layout.toml"), text.as_bytes())?;
                    if let Some((name, png)) = picture {
                        write(dir, name, png)?;
                    }
                }
                None => print!("{text}"),
            }
        }
        Some(FileKind::Profile) => {
            let buttons = button_map(buttons.as_deref())?;
            let mut imported = import_profile(&bytes, &buttons).map_err(|e| e.to_string())?;
            warn(&imported.warnings);
            if imported.profile.name.is_empty() {
                imported.profile.name = stem.clone();
            }
            let body = toml::to_string_pretty(&imported.profile).map_err(|e| e.to_string())?;
            let text = format!("format_version = 1\n{body}");
            match &out {
                Some(dir) => write(dir, &format!("{stem}.toml"), text.as_bytes())?,
                None => print!("{text}"),
            }
        }
        None => return Err(format!("{} is not an Xpadder file", file.display())),
    }
    Ok(())
}

fn button_map(choice: Option<&str>) -> Result<ButtonMap, String> {
    let Some(choice) = choice else { return Ok(ButtonMap::default()) };
    if let Some(preset) = ButtonMap::preset(choice) {
        return Ok(preset);
    }
    let bytes = std::fs::read(choice).map_err(|e| {
        format!(
            "`{choice}` is not a preset ({}) or a readable file: {e}",
            ButtonMap::PRESETS.join(", ")
        )
    })?;
    let controller = import_controller(&bytes, "buttons").map_err(|e| e.to_string())?;
    Ok(controller.buttons)
}

fn warn(warnings: &[String]) {
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    eprintln!("wrote {}", path.display());
    Ok(())
}
