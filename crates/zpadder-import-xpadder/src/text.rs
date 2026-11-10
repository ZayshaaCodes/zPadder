//! Tolerant reader for the INI-like text both Xpadder formats use.
//!
//! Real files break naive parsers in several ways, all handled here:
//! - they are Windows-1252, not UTF-8 (a BOM, or valid UTF-8, is honoured for re-saved files);
//! - line endings are CRLF, but LF or lone CR also occur after copy and paste;
//! - lines may be indented, so every line is trimmed;
//! - legacy 5.x files have no `[Section]` headers, and text can be glued after a header
//!   (`[Assignments];--- Xpadder Profile File ---`), so sections are skipped over rather than
//!   relied on;
//! - lines that are not `key=value` are kept so the importer can mention them.

use std::collections::{HashMap, HashSet};

use crate::Warnings;

/// One `key=value` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// 1-based line number, for messages.
    pub(crate) line: usize,
    pub(crate) key: String,
    pub(crate) value: String,
}

/// A parsed file: its `key=value` lines in order, and the lines that were neither.
#[derive(Debug, Default)]
pub(crate) struct Document {
    pub(crate) entries: Vec<Entry>,
    /// Lines that are not comments, section headers or `key=value` pairs.
    pub(crate) stray: Vec<(usize, String)>,
    /// Keys (as written first) that appear more than once; the last value wins.
    pub(crate) duplicates: Vec<String>,
    /// Lower-case key → index of its last entry.
    index: HashMap<String, usize>,
}

impl Document {
    /// Decodes and parses a whole file. Never fails: anything unreadable ends up in `stray`.
    pub(crate) fn parse(bytes: &[u8]) -> Self {
        let text = decode(bytes);
        let mut doc = Document::default();
        for (number, line) in text.lines().enumerate() {
            // `lines` splits on LF and strips a trailing CR; split again on lone CRs.
            for line in line.split('\r') {
                doc.parse_line(number + 1, line);
            }
        }
        let mut reported = HashSet::new();
        for (i, entry) in doc.entries.iter().enumerate() {
            let lower = entry.key.to_ascii_lowercase();
            if let Some(previous) = doc.index.insert(lower.clone(), i)
                && reported.insert(lower)
            {
                doc.duplicates.push(doc.entries[previous].key.clone());
            }
        }
        doc
    }

    fn parse_line(&mut self, number: usize, line: &str) {
        let mut line = line.trim();
        // A header can have more text glued after it, which is read as a line of its own.
        // Loop rather than recurse so that `[a][b][c]...` cannot exhaust the stack.
        while let Some(rest) = line.strip_prefix('[') {
            let Some(end) = rest.find(']') else { break };
            line = rest[end + 1..].trim();
        }
        if line.is_empty() || line.starts_with(';') {
            return;
        }
        match line.split_once('=') {
            Some((key, value)) if !key.trim().is_empty() => self.entries.push(Entry {
                line: number,
                key: key.trim().to_string(),
                value: value.trim().to_string(),
            }),
            _ => self.stray.push((number, line.to_string())),
        }
    }

    /// The value of `key` (compared ignoring ASCII case); the last one if it repeats.
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        let i = *self.index.get(&key.to_ascii_lowercase())?;
        Some(&self.entries[i].value)
    }

    /// Each key once, at its last (winning) occurrence, in file order.
    pub(crate) fn settings(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().enumerate().filter_map(|(i, entry)| {
            (self.index.get(&entry.key.to_ascii_lowercase()) == Some(&i)).then_some(entry)
        })
    }

    /// Adds readable notes about stray lines and repeated keys to `warnings`.
    pub(crate) fn notes(&self, warnings: &mut Warnings) {
        for (line, text) in &self.stray {
            warnings.push(format!("line {line}: `{}` is not a setting; ignored", shorten(text)));
        }
        for key in &self.duplicates {
            warnings.push(format!("`{key}` appears more than once; the last value is used"));
        }
    }
}

/// Shortens text for a message, at a character boundary.
pub(crate) fn shorten(text: &str) -> String {
    const MAX: usize = 60;
    match text.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}...", &text[..cut]),
        None => text.to_string(),
    }
}

/// Decodes a file to text. Xpadder writes Windows-1252 without a BOM. A UTF-8 or UTF-16 BOM, or
/// bytes that are valid UTF-8, mean the file was re-saved by another editor, so they are
/// honoured; genuine Windows-1252 text with accented letters is never valid UTF-8.
pub(crate) fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        return decode_utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        return decode_utf16(rest, u16::from_be_bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| windows_1252(b)).collect(),
    }
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|&pair| unit(pair));
    char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
}

/// One Windows-1252 byte as a character. Only 0x80–0x9F differ from Latin-1; the five bytes
/// Windows-1252 leaves undefined map to the C1 controls, as in the WHATWG encoding standard.
pub(crate) fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}',
        '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}',
        '\u{2014}', '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}', '\u{17E}',
        '\u{178}',
    ];
    match byte {
        0x80..=0x9F => HIGH[usize::from(byte - 0x80)],
        _ => char::from(byte),
    }
}

/// Reads keys such as `Set2Stick1UpSlots` piece by piece, ignoring ASCII case.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KeyReader<'a> {
    rest: &'a str,
}

impl<'a> KeyReader<'a> {
    pub(crate) fn new(key: &'a str) -> Self {
        Self { rest: key }
    }

    /// Consumes `word` if the key continues with it.
    pub(crate) fn eat(&mut self, word: &str) -> bool {
        let matches =
            self.rest.get(..word.len()).is_some_and(|head| head.eq_ignore_ascii_case(word));
        if matches {
            self.rest = &self.rest[word.len()..];
        }
        matches
    }

    /// Consumes a decimal number from 1 to 255.
    pub(crate) fn number(&mut self) -> Option<u8> {
        let digits = self.rest.bytes().take_while(u8::is_ascii_digit).count();
        let number = self.rest[..digits].parse::<u8>().ok().filter(|&n| n > 0)?;
        self.rest = &self.rest[digits..];
        Some(number)
    }

    /// Consumes the first of `words` the key continues with. List longer words first when one
    /// is a prefix of another.
    pub(crate) fn one_of<T: Copy>(&mut self, words: &[(&str, T)]) -> Option<T> {
        words.iter().find(|(word, _)| self.eat(word)).map(|&(_, value)| value)
    }

    /// The part of the key not read yet.
    pub(crate) fn rest(&self) -> &'a str {
        self.rest
    }

    /// Whether the rest of the key is exactly `word`.
    pub(crate) fn is(&self, word: &str) -> bool {
        self.rest.eq_ignore_ascii_case(word)
    }

    /// Matches the whole rest of the key against `words`.
    pub(crate) fn rest_is<T: Copy>(&self, words: &[(&str, T)]) -> Option<T> {
        words.iter().find(|(word, _)| self.is(word)).map(|&(_, value)| value)
    }
}

/// Parses `a,b` integer pairs such as `Set1StickDeadZones=64,53`. Missing or unreadable halves
/// are `None`.
pub(crate) fn pair(value: &str) -> [Option<i64>; 2] {
    let mut parts = value.split(',').map(|part| part.trim().parse::<i64>().ok());
    [parts.next().flatten(), parts.next().flatten()]
}

/// Parses an integer, tolerating surrounding spaces.
pub(crate) fn int(value: &str) -> Option<i64> {
    value.trim().parse().ok()
}

/// Reads a checkbox flag: `1` (or any non-zero number) is on.
pub(crate) fn flag(value: &str) -> bool {
    int(value).is_some_and(|n| n != 0) || value.trim().eq_ignore_ascii_case("true")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(doc: &Document) -> Vec<(&str, &str)> {
        doc.settings().map(|e| (e.key.as_str(), e.value.as_str())).collect()
    }

    #[test]
    fn windows_1252_is_decoded() {
        // "Botón" as Xpadder writes it, plus the euro sign and curly quotes from 0x80–0x9F.
        let text = decode(b"Button1Name=Bot\xF3n 1 \x80 \x93q\x94");
        assert_eq!(text, "Button1Name=Botón 1 € \u{201C}q\u{201D}");
        assert_eq!(windows_1252(0x81), '\u{81}');
        assert_eq!(windows_1252(0x9F), 'Ÿ');
        assert_eq!(windows_1252(b'A'), 'A');
    }

    #[test]
    fn boms_and_utf8_are_honoured() {
        assert_eq!(decode(b"\xEF\xBB\xBFName=Bot\xC3\xB3n"), "Name=Botón");
        assert_eq!(decode("Name=Botón".as_bytes()), "Name=Botón");
        assert_eq!(decode(b"\xFF\xFEN\0=\0x\0"), "N=x");
        assert_eq!(decode(b"\xFE\xFF\0N\0=\0x"), "N=x");
    }

    #[test]
    fn line_endings_and_indentation() {
        let doc = Document::parse(b"    A=1\r\n\tB = 2 \nC=3\rD=4\r\n\r\n");
        assert_eq!(keys(&doc), [("A", "1"), ("B", "2"), ("C", "3"), ("D", "4")]);
        assert_eq!(doc.entries[3].line, 3);
    }

    #[test]
    fn sections_comments_and_glued_headers() {
        let doc = Document::parse(
            b";--- Xpadder Profile File ---\r\nDataType=Profile\r\n[Set Settings]\r\n\
              [Assignments];--- Xpadder Profile File ---\r\n[Image]ImageLine1=0\r\n[][x]Y=1\r\n",
        );
        assert_eq!(keys(&doc), [("DataType", "Profile"), ("ImageLine1", "0"), ("Y", "1")]);
        assert!(doc.stray.is_empty());
    }

    #[test]
    fn stray_lines_and_duplicates_are_reported() {
        let doc = Document::parse(b"junk line\r\nA=1\r\na=2\r\n=3\r\n[unclosed\r\n");
        assert_eq!(doc.get("A"), Some("2"));
        assert_eq!(keys(&doc), [("a", "2")]);
        assert_eq!(doc.stray.len(), 3);
        assert_eq!(doc.duplicates, ["A"]);
        let mut notes = Warnings::default();
        doc.notes(&mut notes);
        assert_eq!(notes.len(), 4);
    }

    #[test]
    fn hostile_input_does_not_panic() {
        let brackets = "[]".repeat(100_000);
        let doc = Document::parse(brackets.as_bytes());
        assert!(doc.entries.is_empty());
        let _ = Document::parse(&[0xFF, 0xFE, 0x00]);
        let _ = Document::parse(&(0..=255).collect::<Vec<u8>>());
        assert_eq!(shorten(&"é".repeat(100)).chars().count(), 63);
    }

    #[test]
    fn key_reader() {
        let mut key = KeyReader::new("set12STICK1UpRightSlots");
        assert!(key.eat("Set"));
        assert_eq!(key.number(), Some(12));
        assert!(!key.eat("Button"));
        assert!(key.eat("Stick"));
        assert_eq!(key.number(), Some(1));
        assert_eq!(key.one_of(&[("UpRight", 1), ("Up", 2)]), Some(1));
        assert!(key.is("slots"));
        assert_eq!(KeyReader::new("Set0").number(), None);
        assert_eq!(KeyReader::new("x").number(), None);
        let mut big = KeyReader::new("Set999");
        big.eat("Set");
        assert_eq!(big.number(), None);
        // Multi-byte text right where a word is expected.
        assert!(!KeyReader::new("é").eat("Se"));
    }

    #[test]
    fn values() {
        assert_eq!(pair("64, 53"), [Some(64), Some(53)]);
        assert_eq!(pair("64"), [Some(64), None]);
        assert_eq!(pair("x,7"), [None, Some(7)]);
        assert!(flag("1") && flag(" 1 ") && !flag("0") && !flag("") && !flag("yes"));
    }
}
