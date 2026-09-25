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
    /// Whether the ROM runs on the SA-1, through SA-1 Pack.
    pub sa1: bool,
    /// Asar patches applied before AddmusicK and the tools, in order.
    pub early_patches: Vec<PathBuf>,
    /// Asar patches applied after the tools, before the levels, in order.
    pub late_patches: Vec<PathBuf>,
    /// A folder of UberASM Tool's input files (`list.txt`, `level/`, ...),
    /// laid over the user's UberASM Tool folder.
    pub uberasm: Option<PathBuf>,
    /// A folder of AddmusicK's input files (`Addmusic_list.txt`, `music/`,
    /// `samples/`, ...), laid over the user's AddmusicK folder.
    pub music: Option<PathBuf>,
    /// Level number to file, relative to the project directory.
    pub levels: BTreeMap<u16, PathBuf>,
}

impl Manifest {
    pub fn to_toml(&self) -> String {
        let mut out = format!("format = {FORMAT}\n");
        if self.rom_size.is_some() || self.sa1 {
            out += "\n[rom]\n";
        }
        if let Some(size) = self.rom_size {
            out += &format!("size = \"{}\"\n", size_text(size));
        }
        if self.sa1 {
            out += "sa1 = true\n";
        }
        let paths = |list: &[PathBuf]| {
            let quoted: Vec<String> = list.iter().map(|p| format!("\"{}\"", slashes(p))).collect();
            format!("[{}]", quoted.join(", "))
        };
        if !self.early_patches.is_empty() || !self.late_patches.is_empty() {
            out += "\n[patches]\n";
            if !self.early_patches.is_empty() {
                out += &format!("early = {}\n", paths(&self.early_patches));
            }
            if !self.late_patches.is_empty() {
                out += &format!("late = {}\n", paths(&self.late_patches));
            }
        }
        if let Some(music) = &self.music {
            out += &format!("\n[music]\ndir = \"{}\"\n", slashes(music));
        }
        if let Some(uberasm) = &self.uberasm {
            out += &format!("\n[uberasm]\ndir = \"{}\"\n", slashes(uberasm));
        }
        out += "\n[levels]\n";
        for (level, path) in &self.levels {
            out += &format!("0x{level:03X} = \"{}\"\n", slashes(path));
        }
        out
    }

    pub fn from_toml(text: &str) -> Result<Self, SourceError> {
        let doc: DocumentMut = text.parse()?;
        for (key, _) in doc.iter() {
            if !["format", "rom", "patches", "music", "uberasm", "levels"].contains(&key) {
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
                    "sa1" => {
                        manifest.sa1 = item
                            .as_bool()
                            .ok_or_else(|| invalid("rom.sa1", "must be true or false"))?;
                    }
                    _ => return Err(invalid("rom", format!("unknown key `{key}`"))),
                }
            }
        }
        if let Some(patches) = doc.get("patches") {
            let patches = patches
                .as_table()
                .ok_or_else(|| invalid("patches", "must be a table"))?;
            for (key, item) in patches.iter() {
                let at = format!("patches.{key}");
                let list = item
                    .as_array()
                    .ok_or_else(|| invalid(&at, "must be a list of patch files"))?
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(PathBuf::from)
                            .ok_or_else(|| invalid(&at, "must be a list of patch files"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                match key {
                    "early" => manifest.early_patches = list,
                    "late" => manifest.late_patches = list,
                    _ => return Err(invalid("patches", format!("unknown key `{key}`"))),
                }
            }
        }
        if let Some(music) = doc.get("music") {
            let music = music
                .as_table()
                .ok_or_else(|| invalid("music", "must be a table"))?;
            for (key, item) in music.iter() {
                match key {
                    "dir" => {
                        let dir = item
                            .as_str()
                            .ok_or_else(|| invalid("music.dir", "must be a folder path"))?;
                        manifest.music = Some(PathBuf::from(dir));
                    }
                    _ => return Err(invalid("music", format!("unknown key `{key}`"))),
                }
            }
        }
        if let Some(uberasm) = doc.get("uberasm") {
            let uberasm = uberasm
                .as_table()
                .ok_or_else(|| invalid("uberasm", "must be a table"))?;
            for (key, item) in uberasm.iter() {
                match key {
                    "dir" => {
                        let dir = item
                            .as_str()
                            .ok_or_else(|| invalid("uberasm.dir", "must be a folder path"))?;
                        manifest.uberasm = Some(PathBuf::from(dir));
                    }
                    _ => return Err(invalid("uberasm", format!("unknown key `{key}`"))),
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

/// A path as the manifest writes it, with forward slashes on every
/// platform.
fn slashes(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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
            sa1: true,
            early_patches: vec![PathBuf::from("asm/fastrom.asm")],
            late_patches: vec![PathBuf::from("asm/a.asm"), PathBuf::from("asm/b.asm")],
            music: Some(PathBuf::from("music")),
            uberasm: Some(PathBuf::from("uberasm")),
            levels: BTreeMap::from([
                (0x105, PathBuf::from("world1/yoshis-island-1.toml")),
                (0x0C7, PathBuf::from("title.toml")),
            ]),
        };
        let text = manifest.to_toml();
        assert_eq!(
            text,
            "format = 1\n\n[rom]\nsize = \"1536K\"\nsa1 = true\n\n[patches]\n\
             early = [\"asm/fastrom.asm\"]\nlate = [\"asm/a.asm\", \"asm/b.asm\"]\n\n\
             [music]\ndir = \"music\"\n\n[uberasm]\ndir = \"uberasm\"\n\n[levels]\n\
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
