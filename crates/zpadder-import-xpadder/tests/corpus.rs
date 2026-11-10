//! Imports every Xpadder file in a local corpus. Opt-in, because the public corpus repositories
//! mostly have no licence and cannot be committed:
//!
//! ```text
//! $env:XPADDER_CORPUS = "C:\path\to\xpadder-files"
//! cargo test -p zpadder-import-xpadder --test corpus -- --ignored --nocapture
//! ```
//!
//! Profiles are imported with the button map of a controller file in the same folder, if there
//! is one. Otherwise the folder names choose a preset, as a user would: the DualShock 4 order for
//! Logitech and PlayStation-style pads, and the Xbox order for everything else. Set `XPADDER_CORPUS_REPORT` to a file path to also get
//! every warning, file by file.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::panic::catch_unwind;
use std::path::{Path, PathBuf};

use zpadder_import_xpadder::{ButtonMap, import_controller, import_profile};

#[test]
#[ignore = "needs XPADDER_CORPUS pointing at a folder of Xpadder files"]
fn corpus() {
    let root = std::env::var_os("XPADDER_CORPUS").expect("set XPADDER_CORPUS to a folder");
    let mut files = Vec::new();
    collect(Path::new(&root), &mut files);
    let extension = |path: &Path| {
        path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default()
    };
    let (controllers, profiles): (Vec<PathBuf>, Vec<PathBuf>) = files
        .into_iter()
        .filter(|p| matches!(extension(p).as_str(), "xpaddercontroller" | "xpadderprofile"))
        .partition(|p| extension(p) == "xpaddercontroller");

    let mut report = String::new();
    let mut warnings: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut maps: HashMap<PathBuf, ButtonMap> = HashMap::new();
    let (mut controllers_ok, mut pictures) = (0, 0);
    for path in &controllers {
        let bytes = std::fs::read(path).unwrap();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        match catch_unwind(|| import_controller(&bytes, &name)) {
            Err(_) => failures.push(format!("PANIC {}", path.display())),
            Ok(Err(error)) => failures.push(format!("{}: {error}", path.display())),
            Ok(Ok(imported)) => {
                controllers_ok += 1;
                pictures += usize::from(imported.png.is_some());
                record(&mut report, &mut warnings, path, &imported.warnings);
                let _ = writeln!(report, "  buttons: {:?}", imported.buttons);
                maps.entry(path.parent().unwrap().to_path_buf()).or_insert(imported.buttons);
            }
        }
    }

    let (mut profiles_ok, mut invalid, mut layers, mut clean) = (0, 0, 0, 0);
    for path in &profiles {
        let bytes = std::fs::read(path).unwrap();
        let buttons = maps.get(path.parent().unwrap()).cloned().unwrap_or_else(|| guess(path));
        match catch_unwind(|| import_profile(&bytes, &buttons)) {
            Err(_) => failures.push(format!("PANIC {}", path.display())),
            Ok(Err(error)) => failures.push(format!("{}: {error}", path.display())),
            Ok(Ok(imported)) => {
                profiles_ok += 1;
                layers += imported.profile.layers.len();
                clean += usize::from(imported.warnings.is_empty());
                if let Err(errors) = imported.profile.validate() {
                    invalid += 1;
                    failures.push(format!("INVALID {}: {errors:?}", path.display()));
                }
                record(&mut report, &mut warnings, path, &imported.warnings);
            }
        }
    }

    println!("files found: {} controllers, {} profiles", controllers.len(), profiles.len());
    println!("controllers imported: {controllers_ok} ({pictures} with a picture)");
    println!(
        "profiles imported: {profiles_ok} ({layers} layers, {clean} without warnings), \
         {invalid} failed validation"
    );
    println!("failures: {}", failures.len());
    for failure in &failures {
        println!("  {failure}");
    }
    let mut common: Vec<(&String, &usize)> = warnings.iter().collect();
    common.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("most common warnings ({} kinds):", common.len());
    for (warning, count) in common.iter().take(30) {
        println!("  {count:5}  {warning}");
    }
    if let Some(path) = std::env::var_os("XPADDER_CORPUS_REPORT") {
        std::fs::write(path, report).unwrap();
    }

    assert!(!failures.iter().any(|f| f.starts_with("PANIC")), "the importer panicked");
    assert_eq!(invalid, 0, "every imported profile must pass validation");
}

fn guess(path: &Path) -> ButtonMap {
    let path = path.to_string_lossy().to_ascii_lowercase();
    let dual_action = ["rumblepad", "dual action", "f310", "f710", "ps2", "ps3", "playstation"];
    if dual_action.iter().any(|name| path.contains(name)) {
        ButtonMap::dualshock4()
    } else {
        ButtonMap::xbox()
    }
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != ".git") {
                collect(&path, files);
            }
        } else {
            files.push(path);
        }
    }
}

/// Adds a file's warnings to the report and to the tally, with numbers and quoted text blanked
/// out so similar warnings count together.
fn record(report: &mut String, tally: &mut BTreeMap<String, usize>, path: &Path, all: &[String]) {
    let _ = writeln!(report, "{}", path.display());
    for warning in all {
        let _ = writeln!(report, "  {warning}");
        let mut shape = String::new();
        let mut quoted = false;
        for c in warning.chars() {
            match c {
                '`' => {
                    quoted = !quoted;
                    if quoted {
                        shape.push_str("`…`");
                    }
                }
                _ if quoted => {}
                '0'..='9' => {
                    if !shape.ends_with('#') {
                        shape.push('#');
                    }
                }
                _ => shape.push(c),
            }
        }
        *tally.entry(shape).or_default() += 1;
    }
}
