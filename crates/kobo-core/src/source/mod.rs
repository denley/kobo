//! A project's source files: the manifest and the level files, in TOML.
//!
//! Kobo owns the formatting. Every file is written the same way from the
//! same content, one object or sprite per line in data order, numbers the
//! game uses in hex, positions in decimal, and bytes Kobo does not
//! interpret as a hex string. A user's comments on lines of their own
//! survive reformatting; a comment after an entry on its line is Kobo's,
//! and is rewritten.

use thiserror::Error;

pub mod level;
pub mod project;

#[derive(Debug, Error)]
pub enum SourceError {
    #[error(transparent)]
    Toml(#[from] toml_edit::TomlError),
    #[error("{at}: {message}")]
    Invalid { at: String, message: String },
}

pub(crate) fn invalid(at: impl Into<String>, message: impl Into<String>) -> SourceError {
    SourceError::Invalid {
        at: at.into(),
        message: message.into(),
    }
}

/// `0x05`, `0x105`: a number the game uses, in at least `digits` digits.
pub(crate) fn hex(value: u32, digits: usize) -> String {
    format!("0x{value:0digits$X}")
}

/// Bytes as `"11 5A"`.
pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    let parts: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!("\"{}\"", parts.join(" "))
}

pub(crate) fn parse_hex_bytes(at: &str, text: &str) -> Result<Vec<u8>, SourceError> {
    text.split_whitespace()
        .map(|part| {
            u8::from_str_radix(part, 16)
                .ok()
                .filter(|_| part.len() == 2)
                .ok_or_else(|| invalid(at, format!("{part:?} is not a hex byte")))
        })
        .collect()
}

/// The comment lines in a decor prefix that stand on lines of their own.
/// The text before the first line break follows the previous entry on its
/// line, so it is Kobo's and is dropped.
pub(crate) fn own_line_comments(prefix: &str, after_entry: bool) -> Vec<String> {
    let lines: Vec<&str> = prefix.split('\n').collect();
    let skip = usize::from(after_entry && lines.len() > 1);
    lines[skip..]
        .iter()
        .map(|line| line.trim())
        .filter(|line| line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}
