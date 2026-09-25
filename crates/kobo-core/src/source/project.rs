//! The project manifest, `kobo.toml`.
//!
//! ```toml
//! format = 1
//!
//! [rom]
//! size = "1M"
//!
//! [levels]
//! 0x105 = "levels/yoshis-island-1.toml"
//! ```
//!
//! The level table is the only place level numbers live; file names and
//! folders are free. A level the table does not list keeps the clean
//! ROM's content.

use std::collections::BTreeMap;
use std::path::PathBuf;

use toml_edit::DocumentMut;

use super::{SourceError, invalid};
use crate::level::LEVEL_COUNT;

/// The manifest format this version of Kobo writes. A newer one is
/// refused; older ones are migrated when there are any.
pub const FORMAT: u32 = 1;

pub const MANIFEST: &str = "kobo.toml";

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Manifest {
    /// The size to expand the ROM to, if the project sets one.
    pub rom_size: Option<usize>,
    /// Level number to file, relative to the project directory.
    pub levels: BTreeMap<u16, PathBuf>,
}

impl Manifest {
    pub fn to_toml(&self) -> String {
        let mut out = format!("format = {FORMAT}\n");
        if let Some(size) = self.rom_size {
            out += &format!("\n[rom]\nsize = \"{}\"\n", size_text(size));
        }
        out += "\n[levels]\n";
        for (level, path) in &self.levels {
            let path = path.to_string_lossy().replace('\\', "/");
            out += &format!("0x{level:03X} = \"{path}\"\n");
        }
        out
    }

    pub fn from_toml(text: &str) -> Result<Self, SourceError> {
        let doc: DocumentMut = text.parse()?;
        for (key, _) in doc.iter() {
            if !["format", "rom", "levels"].contains(&key) {
                return Err(invalid(MANIFEST, format!("unknown key `{key}`")));
            }
        }
        let format = doc
            .get("format")
            .and_then(|f| f.as_integer())
            .ok_or_else(|| invalid("format", "must be an integer"))?;
        if format != FORMAT as i64 {
            return Err(invalid(
                "format",
                format!("this Kobo reads format {FORMAT}, not {format}"),
            ));
        }
        let mut manifest = Self::default();
        if let Some(rom) = doc.get("rom") {
            let rom = rom
                .as_table()
                .ok_or_else(|| invalid("rom", "must be a table"))?;
            for (key, item) in rom.iter() {
                match key {
                    "size" => {
                        let text = item.as_str().ok_or_else(|| {
                            invalid("rom.size", "must be a string such as \"1M\"")
                        })?;
                        manifest.rom_size = Some(parse_size(text).ok_or_else(|| {
                            invalid("rom.size", format!("{text:?} is not a size such as \"1M\""))
                        })?);
                    }
                    _ => return Err(invalid("rom", format!("unknown key `{key}`"))),
                }
            }
        }
        if let Some(levels) = doc.get("levels") {
            let levels = levels
                .as_table()
                .ok_or_else(|| invalid("levels", "must be a table"))?;
            for (key, item) in levels.iter() {
                let at = format!("levels.{key}");
                let level = key
                    .strip_prefix("0x")
                    .and_then(|hex| u16::from_str_radix(hex, 16).ok())
                    .filter(|&n| n < LEVEL_COUNT)
                    .ok_or_else(|| invalid(&at, "a level number is 0x000 to 0x1FF"))?;
                let path = item
                    .as_str()
                    .ok_or_else(|| invalid(&at, "must be a file path"))?;
                if manifest.levels.insert(level, PathBuf::from(path)).is_some() {
                    return Err(invalid(&at, "is listed twice"));
                }
            }
        }
        Ok(manifest)
    }
}

/// `512K`, `1M`, `3M`, or a byte count.
pub fn parse_size(text: &str) -> Option<usize> {
    let (digits, unit) = match text.char_indices().last()? {
        (i, 'K' | 'k') => (&text[..i], 1 << 10),
        (i, 'M' | 'm') => (&text[..i], 1 << 20),
        _ => (text, 1),
    };
    digits.parse::<usize>().ok()?.checked_mul(unit)
}

fn size_text(size: usize) -> String {
    match size {
        s if s % (1 << 20) == 0 => format!("{}M", s >> 20),
        s if s % (1 << 10) == 0 => format!("{}K", s >> 10),
        s => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let manifest = Manifest {
            rom_size: Some(0x18_0000),
            levels: BTreeMap::from([
                (0x105, PathBuf::from("world1/yoshis-island-1.toml")),
                (0x0C7, PathBuf::from("title.toml")),
            ]),
        };
        let text = manifest.to_toml();
        assert_eq!(
            text,
            "format = 1\n\n[rom]\nsize = \"1536K\"\n\n[levels]\n\
             0x0C7 = \"title.toml\"\n0x105 = \"world1/yoshis-island-1.toml\"\n"
        );
        assert_eq!(Manifest::from_toml(&text).unwrap(), manifest);
    }

    #[test]
    fn refusals() {
        let bad = |text: &str| Manifest::from_toml(text).is_err();
        assert!(bad("format = 2\n"));
        assert!(bad("[levels]\n"));
        assert!(bad("format = 1\n[levels]\n0x200 = \"a.toml\"\n"));
        assert!(bad("format = 1\n[levels]\n105 = \"a.toml\"\n"));
        assert!(bad("format = 1\nextra = 1\n"));
        assert!(bad("format = 1\n[rom]\nsize = \"big\"\n"));
        assert!(!bad("format = 1\n"));
    }
}
