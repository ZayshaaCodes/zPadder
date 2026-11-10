//! `zpadder-cli`: headless mapping and profile tools.

mod run;

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use zpadder_import_xpadder::{ButtonMap, FileKind};

const USAGE: &str = "\
ZPadder command-line tools (the app itself is zpadder.exe)

usage:
  zpadder-cli run [--verbose] <profile>       map every connected controller with this profile;
                                              saved edits are picked up while running
  zpadder-cli check <profile>                 check a profile for mistakes without running it
  zpadder-cli import <xpadder file> [<out>] [--buttons xbox|dualshock4|<.xpaddercontroller>]
                                              convert an .xpadderprofile to ZPadder TOML, or an
                                              .xpaddercontroller to a layout and PNG picture
  zpadder-cli schema [<file>]                 write the profile JSON schema (for editors)

<profile> is a ZPadder .toml profile or an Xpadder .xpadderprofile (imported on the fly).";

fn main() -> ExitCode {
    // First, before anything can load a DLL.
    let dll_search = zpadder_platform::restrict_dll_search();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["run", path] => run_with_logging(path, false, dll_search),
        ["run", "--verbose" | "-v", path] => run_with_logging(path, true, dll_search),
        ["check", path] => check(Path::new(path)),
        ["import", rest @ ..] if !rest.is_empty() => import(rest),
        ["schema"] => {
            print!("{}", zpadder_config::json_schema());
            ExitCode::SUCCESS
        }
        ["schema", path] => match std::fs::write(path, zpadder_config::json_schema()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{path}: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            println!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Maps with `path`, logging to the console at INFO, or DEBUG with `--verbose`. `ZPADDER_LOG`, or
/// else `RUST_LOG`, overrides the level in `tracing`'s directive syntax.
fn run_with_logging(path: &str, verbose: bool, dll_search: std::io::Result<()>) -> ExitCode {
    let default = if verbose { LevelFilter::DEBUG } else { LevelFilter::INFO };
    let directives =
        std::env::var("ZPADDER_LOG").or_else(|_| std::env::var("RUST_LOG")).unwrap_or_default();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder().with_default_directive(default.into()).parse_lossy(directives),
        )
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_target(false)
        .init();
    if let Err(e) = dll_search {
        tracing::warn!("Cannot limit where DLLs are loaded from: {e}");
    }
    run::run(Path::new(path))
}

fn check(path: &Path) -> ExitCode {
    match zpadder_config::load_any(path) {
        Ok(loaded) => {
            let layers: Vec<&str> = loaded.profile.layers.iter().map(|l| l.name.as_str()).collect();
            println!("OK: \"{}\" with layers {}", loaded.profile.name, layers.join(", "));
            print_warnings(&loaded.warnings);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            ExitCode::FAILURE
        }
    }
}

fn print_warnings(warnings: &[String]) {
    for warning in warnings {
        println!("  note: {warning}");
    }
}

/// `import <file> [<out>] [--buttons <preset or controller file>]`.
fn import(args: &[&str]) -> ExitCode {
    let mut input = None;
    let mut output = None;
    let mut buttons = None;
    let mut args = args.iter();
    while let Some(&arg) = args.next() {
        match arg {
            "--buttons" => match args.next() {
                Some(&value) => buttons = Some(value),
                None => return usage_error("--buttons needs a value"),
            },
            _ if input.is_none() => input = Some(PathBuf::from(arg)),
            _ if output.is_none() => output = Some(PathBuf::from(arg)),
            _ => return usage_error(&format!("unexpected `{arg}`")),
        }
    }
    let Some(input) = input else { return usage_error("which file?") };
    let bytes = match read_capped(&input) {
        Ok(bytes) => bytes,
        Err(e) => return failure(&input, &e),
    };
    match zpadder_import_xpadder::detect(&bytes) {
        Some(FileKind::Profile) => {
            let (map, mut warnings) = match buttons {
                None => {
                    // Same rule as opening the file: a controller file alongside, else Xbox.
                    match zpadder_config::load_any(&input) {
                        Ok(loaded) => {
                            return write_profile(
                                &input,
                                output,
                                &loaded.profile,
                                &loaded.warnings,
                            );
                        }
                        Err(e) => return failure(&input, &e),
                    }
                }
                Some(name) => match ButtonMap::preset(name) {
                    Some(map) => (map, vec![format!("Button order: {name} preset")]),
                    None => match read_controller_buttons(Path::new(name)) {
                        Ok(map) => (map, vec![format!("Button order taken from {name}")]),
                        Err(e) => return usage_error(&e),
                    },
                },
            };
            match zpadder_import_xpadder::import_profile(&bytes, &map) {
                Ok(imported) => {
                    let mut profile = imported.profile;
                    if profile.name.trim().is_empty() {
                        profile.name = stem(&input);
                    }
                    warnings.extend(imported.warnings);
                    write_profile(&input, output, &profile, &warnings)
                }
                Err(e) => failure(&input, &e),
            }
        }
        Some(FileKind::Controller) => {
            let name = stem(&input);
            let imported = match zpadder_import_xpadder::import_controller(&bytes, &name) {
                Ok(imported) => imported,
                Err(e) => return failure(&input, &e),
            };
            let dir = output.unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")).to_owned());
            let layout_path = dir.join(format!("{name}.layout.toml"));
            if let (Some(png), Some(image)) = (&imported.png, &imported.layout.image)
                && let Err(e) = std::fs::write(dir.join(image), png)
            {
                return failure(&dir.join(image), &e);
            }
            if let Err(e) = zpadder_config::save_layout(&layout_path, &imported.layout) {
                return failure(&layout_path, &e);
            }
            println!("Wrote {}", layout_path.display());
            print_warnings(&imported.warnings);
            ExitCode::SUCCESS
        }
        None => usage_error(&format!("{} is not an Xpadder file", input.display())),
    }
}

fn read_controller_buttons(path: &Path) -> Result<ButtonMap, String> {
    let bytes = read_capped(path).map_err(|e| format!("{}: {e}", path.display()))?;
    zpadder_import_xpadder::import_controller(&bytes, &stem(path))
        .map(|imported| imported.buttons)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn write_profile(
    input: &Path,
    output: Option<PathBuf>,
    profile: &zpadder_model::Profile,
    warnings: &[String],
) -> ExitCode {
    let output = output.unwrap_or_else(|| input.with_extension("toml"));
    if let Err(e) = zpadder_config::save(&output, profile) {
        return failure(&output, &e);
    }
    println!("Wrote {}", output.display());
    print_warnings(warnings);
    ExitCode::SUCCESS
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Reads a file to import, refusing one larger than any ZPadder or Xpadder file can be
/// ([`zpadder_config::MAX_FILE_SIZE`]) before reading it all.
fn read_capped(path: &Path) -> io::Result<Vec<u8>> {
    const MAX: u64 = zpadder_config::MAX_FILE_SIZE;
    let too_large = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "the file is larger than {} MB, too large for a ZPadder or Xpadder file",
                MAX / (1024 * 1024)
            ),
        )
    };
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX {
        return Err(too_large());
    }
    // In case it grows meanwhile.
    let mut bytes = Vec::new();
    file.take(MAX + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX {
        return Err(too_large());
    }
    Ok(bytes)
}

fn failure(path: &Path, error: &dyn std::fmt::Display) -> ExitCode {
    eprintln!("{}: {error}", path.display());
    ExitCode::FAILURE
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("{message}\n\n{USAGE}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_too_large_to_import_are_refused_before_reading() {
        let dir = std::env::temp_dir().join(format!("zpadder-cli-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (small, huge) = (dir.join("small.xpadderprofile"), dir.join("huge.xpadderprofile"));
        std::fs::write(&small, b"DataType=Profile").unwrap();
        File::create(&huge).unwrap().set_len(zpadder_config::MAX_FILE_SIZE + 1).unwrap();
        assert_eq!(read_capped(&small).unwrap(), b"DataType=Profile");
        let error = read_capped(&huge).unwrap_err();
        assert!(error.to_string().contains("too large"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
