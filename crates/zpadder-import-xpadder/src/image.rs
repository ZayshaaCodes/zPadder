//! Decoder for the run-length-encoded picture in a controller file's `[Image]` section.
//!
//! Each `ImageLine{y}=` value (y = 1..=256) is one row of a 512x256 image. The first token is an
//! escape value. After it, a token equal to the escape value starts a run `ESC,RRGGBB,COUNT` with
//! COUNT in hex; any other token is one pixel in hex RGB with leading zeros dropped (`0` is
//! black, `10101` is #010101). Rows may be missing, and magenta is the transparent background.

use std::fmt;

/// Width of an Xpadder controller image, in pixels.
pub(crate) const WIDTH: usize = 512;
/// Height of an Xpadder controller image, in pixels.
pub(crate) const HEIGHT: usize = 256;
/// Magenta, which Xpadder treats as transparent.
pub(crate) const TRANSPARENT: u32 = 0xFF00FF;

/// Why an `ImageLine` value could not be decoded. `index` counts comma-separated tokens from 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DecodeError {
    /// The token is not a hex colour (at most `FFFFFF`) or hex run length.
    BadToken { index: usize },
    /// The run starting at this token is missing its colour or count.
    TruncatedRun { index: usize },
    /// The row decodes to more than [`WIDTH`] pixels.
    TooWide,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadToken { index } => write!(f, "token {index} is not a valid hex value"),
            Self::TruncatedRun { index } => write!(f, "run at token {index} is incomplete"),
            Self::TooWide => write!(f, "row is wider than {WIDTH} pixels"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decodes one `ImageLine` value into `0xRRGGBB` pixels, at most [`WIDTH`] of them.
pub(crate) fn decode_line(value: &str) -> Result<Vec<u32>, DecodeError> {
    let mut tokens = value.split(',').map(str::trim).enumerate();
    let Some((_, escape)) = tokens.next() else {
        return Ok(Vec::new());
    };

    let mut pixels = Vec::with_capacity(WIDTH);
    while let Some((index, token)) = tokens.next() {
        let (color, count) = if token == escape {
            let (Some(color), Some(count)) = (tokens.next(), tokens.next()) else {
                return Err(DecodeError::TruncatedRun { index });
            };
            (parse_color(color)?, parse_count(count)?)
        } else {
            (parse_color((index, token))?, 1)
        };
        if count > WIDTH - pixels.len() {
            return Err(DecodeError::TooWide);
        }
        pixels.extend(std::iter::repeat_n(color, count));
    }
    Ok(pixels)
}

fn parse_color((index, token): (usize, &str)) -> Result<u32, DecodeError> {
    u32::from_str_radix(token, 16)
        .ok()
        .filter(|&c| c <= 0xFF_FFFF)
        .ok_or(DecodeError::BadToken { index })
}

fn parse_count((index, token): (usize, &str)) -> Result<usize, DecodeError> {
    usize::from_str_radix(token, 16).map_err(|_| DecodeError::BadToken { index })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_runs_and_literals() {
        // 0x1F0 magenta + black + #010101 + 0xE white = 496 + 1 + 1 + 14 = 512.
        let row = decode_line("1,1,FF00FF,1F0,0,10101,1,FFFFFF,E").unwrap();
        assert_eq!(row.len(), WIDTH);
        assert!(row[..0x1F0].iter().all(|&p| p == TRANSPARENT));
        assert_eq!(row[0x1F0], 0x000000);
        assert_eq!(row[0x1F1], 0x010101);
        assert!(row[0x1F2..].iter().all(|&p| p == 0xFFFFFF));
    }

    #[test]
    fn empty_value_is_an_empty_row() {
        assert_eq!(decode_line(""), Ok(Vec::new()));
    }

    #[test]
    fn rejects_truncated_run() {
        assert_eq!(decode_line("2,2,FF00FF"), Err(DecodeError::TruncatedRun { index: 1 }));
    }

    #[test]
    fn rejects_bad_tokens() {
        assert_eq!(decode_line("1,ZZ"), Err(DecodeError::BadToken { index: 1 }));
        assert_eq!(decode_line("1,1000000"), Err(DecodeError::BadToken { index: 1 }));
        assert_eq!(decode_line("1,1,0,"), Err(DecodeError::BadToken { index: 3 }));
    }

    #[test]
    fn rejects_rows_wider_than_the_image() {
        assert_eq!(decode_line("1,1,0,201"), Err(DecodeError::TooWide));
    }
}
