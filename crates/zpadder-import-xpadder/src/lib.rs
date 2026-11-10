//! Importer for Xpadder `.xpaddercontroller` and `.xpadderprofile` files.
//!
//! Both formats are undocumented INI-like text. Everything known about them is in
//! `docs/research.md`, and `docs/xpadder-import.md` lists what this
//! importer carries over, what it cannot, and every assumption it makes.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use zpadder_import_xpadder::{ButtonMap, import_controller, import_profile};
//!
//! // The controller file tells which ZPadder button each Xpadder button number is.
//! let controller = import_controller(&std::fs::read("Xbox 360.xpaddercontroller")?, "Xbox 360")?;
//! let profile = import_profile(&std::fs::read("p1.xpadderprofile")?, &controller.buttons)?;
//! for warning in &profile.warnings {
//!     eprintln!("warning: {warning}");
//! }
//! assert!(profile.profile.validate().is_ok());
//!
//! // Without a controller file, pick a preset instead.
//! let other = import_profile(&std::fs::read("p2.xpadderprofile")?, &ButtonMap::dualshock4())?;
//! # let _ = other;
//! # Ok(())
//! # }
//! ```
//!
//! `examples/xpadder-import.rs` converts files to ZPadder's TOML from the command line.
//!
//! Importing never panics and never rejects a file for a bad line: anything that cannot be
//! carried over becomes a warning. Only input that is not an Xpadder file at all, is the other
//! kind of Xpadder file, or is larger than [`MAX_FILE_SIZE`], is an error. At most
//! [`MAX_WARNINGS`] warnings are listed; the last one counts the rest.

mod buttons;
mod controller;
mod image;
mod keys;
mod profile;
mod text;

use std::fmt;

pub use buttons::ButtonMap;
pub use controller::{ImportedController, import_controller};
pub use profile::{ImportedProfile, import_profile};

/// The largest file the importer reads, in bytes. Real profiles are a few kilobytes and
/// controller files, with their picture, at most about one megabyte.
pub const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;

/// The most warnings an import lists one by one. Any more are counted in one last warning, so
/// a file full of junk cannot bury the useful ones (or fill memory).
pub const MAX_WARNINGS: usize = 50;

/// Warnings collected during an import: the first [`MAX_WARNINGS`], and how many more.
#[derive(Debug, Default)]
pub(crate) struct Warnings {
    listed: Vec<String>,
    more: usize,
}

impl Warnings {
    pub(crate) fn push(&mut self, warning: String) {
        if self.listed.len() < MAX_WARNINGS {
            self.listed.push(warning);
        } else {
            self.more += 1;
        }
    }

    /// The listed warnings, then a count of the others.
    pub(crate) fn into_vec(self) -> Vec<String> {
        let mut warnings = self.listed;
        match self.more {
            0 => {}
            1 => warnings.push("...and 1 more warning".to_string()),
            more => warnings.push(format!("...and {more} more warnings")),
        }
        warnings
    }

    /// How many warnings there are, listed or not.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.listed.len() + self.more
    }
}

impl Extend<String> for Warnings {
    fn extend<I: IntoIterator<Item = String>>(&mut self, warnings: I) {
        for warning in warnings {
            self.push(warning);
        }
    }
}

/// The two kinds of Xpadder file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// `.xpaddercontroller`: a controller picture and where each control sits on it.
    Controller,
    /// `.xpadderprofile`: what each control does.
    Profile,
}

impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FileKind::Controller => "an Xpadder controller file",
            FileKind::Profile => "an Xpadder profile",
        })
    }
}

/// Why a file could not be imported at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// Nothing in the input looks like an Xpadder file.
    NotXpadder,
    /// The input is the other kind of Xpadder file.
    WrongKind { expected: FileKind, found: FileKind },
    /// The input is larger than [`MAX_FILE_SIZE`]; `size` is its length in bytes.
    TooLarge { size: usize },
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotXpadder => f.write_str("this is not an Xpadder file"),
            Self::WrongKind { expected, found } => {
                write!(f, "expected {expected}, but this is {found}")
            }
            Self::TooLarge { size } => write!(
                f,
                "the file is {} MB, too large for an Xpadder file (the limit is {} MB)",
                size.div_ceil(1024 * 1024),
                MAX_FILE_SIZE / (1024 * 1024)
            ),
        }
    }
}

impl std::error::Error for ImportError {}

/// Tells which kind of Xpadder file `bytes` hold, for example to import a file or the clipboard
/// without asking. Files are recognised by their `DataType` line, or by the shape of their keys
/// for legacy files that have none; the comment banner is not trusted.
pub fn detect(bytes: &[u8]) -> Option<FileKind> {
    detect_document(&text::Document::parse(bytes))
}

fn detect_document(doc: &text::Document) -> Option<FileKind> {
    match doc.get("DataType").map(str::to_ascii_lowercase).as_deref() {
        Some("profile") => return Some(FileKind::Profile),
        Some("controller") => return Some(FileKind::Controller),
        _ => {}
    }
    let (mut profile, mut controller) = (0, 0);
    for entry in &doc.entries {
        let mut key = text::KeyReader::new(&entry.key);
        if key.eat("Set") && key.number().is_some() {
            profile += 1;
            continue;
        }
        let key = entry.key.to_ascii_lowercase();
        if key.starts_with("imageline") || key.ends_with("location") || key.ends_with("access") {
            controller += 1;
        }
    }
    match (profile, controller) {
        (0, 0) => None,
        (p, c) if p >= c => Some(FileKind::Profile),
        _ => Some(FileKind::Controller),
    }
}

/// Parses `bytes`, refusing input larger than [`MAX_FILE_SIZE`].
fn parse(bytes: &[u8]) -> Result<text::Document, ImportError> {
    if bytes.len() > MAX_FILE_SIZE {
        return Err(ImportError::TooLarge { size: bytes.len() });
    }
    Ok(text::Document::parse(bytes))
}

/// Checks that `doc` is the expected kind of file.
fn expect_kind(doc: &text::Document, expected: FileKind) -> Result<(), ImportError> {
    match detect_document(doc) {
        Some(found) if found == expected => Ok(()),
        Some(found) => Err(ImportError::WrongKind { expected, found }),
        None => Err(ImportError::NotXpadder),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_data_type_then_key_shape() {
        assert_eq!(detect(b"DataType=Profile\r\n"), Some(FileKind::Profile));
        assert_eq!(detect(b"datatype=controller"), Some(FileKind::Controller));
        // Legacy files have no DataType, and the banner varies, so it is ignored.
        assert_eq!(
            detect(b";--- Xpadder Controller File ---\r\nVersion=5.6\r\nSet1Button1Slots=A"),
            Some(FileKind::Profile)
        );
        assert_eq!(detect(b"Version=5.3\r\nButton1Location=1,2"), Some(FileKind::Controller));
        assert_eq!(detect(b"Version=5.3\r\n"), None);
        assert_eq!(detect(b""), None);
        assert_eq!(detect(&[0, 159, 146, 150]), None);
    }

    #[test]
    fn wrong_kind_is_an_error() {
        let controller = b"DataType=Controller\r\nButton1Location=1,2\r\n";
        assert_eq!(
            import_profile(controller, &ButtonMap::xbox()),
            Err(ImportError::WrongKind {
                expected: FileKind::Profile,
                found: FileKind::Controller
            })
        );
        assert_eq!(import_controller(b"hello", "x"), Err(ImportError::NotXpadder));
        assert_eq!(ImportError::NotXpadder.to_string(), "this is not an Xpadder file".to_string());
    }

    #[test]
    fn huge_files_are_refused() {
        let mut huge = b"DataType=Profile\r\n".to_vec();
        huge.resize(MAX_FILE_SIZE + 1, b' ');
        let size = huge.len();
        assert_eq!(import_profile(&huge, &ButtonMap::xbox()), Err(ImportError::TooLarge { size }));
        assert_eq!(import_controller(&huge, "x"), Err(ImportError::TooLarge { size }));
        assert!(ImportError::TooLarge { size }.to_string().contains("limit is 4 MB"));
        huge.truncate(MAX_FILE_SIZE);
        assert!(import_profile(&huge, &ButtonMap::xbox()).is_ok());
    }

    #[test]
    fn warnings_are_capped() {
        let mut warnings = Warnings::default();
        warnings.extend((0..MAX_WARNINGS + 7).map(|i| i.to_string()));
        let list = warnings.into_vec();
        assert_eq!(list.len(), MAX_WARNINGS + 1);
        assert_eq!(list[MAX_WARNINGS], "...and 7 more warnings");

        // A profile of junk lines lists the first ones and counts the rest.
        let junk = "DataType=Profile\r\n".to_string() + &"junk\r\n".repeat(10_000);
        let imported = import_profile(junk.as_bytes(), &ButtonMap::xbox()).unwrap();
        assert_eq!(imported.warnings.len(), MAX_WARNINGS + 1);
        assert_eq!(imported.warnings[MAX_WARNINGS], "...and 9950 more warnings");
    }
}
