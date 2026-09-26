//! Map16 page files: one page of 256 foreground tiles past page 1, in
//! Lunar Magic's numbering, with what each tile acts like.
//!
//! ```toml
//! [tiles]
//! 0x200 = { acts = 0x130, gfx = ["0A0 2", "0A1 2", "0B0 2", "0B1 2 p"] }
//! ```
//!
//! One tile per line, keyed by its full number. `gfx` is its four 8x8
//! tiles in reading order (top left, top right, bottom left, bottom right,
//! which is not the game's storage order), each as the 8x8 tile number in
//! hex, the palette row, and any of `x` and `y` (flips) and `p` (priority).
//! `acts` is the tile it acts like. A tile the file does not list is empty:
//! four references to 8x8 tile 0 in palette 0, acting like `$130`, as a
//! fresh Lunar Magic install's acts-like table has every tile past page 1.
//! The manifest's `[map16]` table says which page a file is.
//!
//! Background pages, in the manifest's `[map16_bg]` table, are the same
//! without `acts`: page `P` is page `P % 16` of BG Map16 table `P / 16`,
//! and a tile's key is its number in the table plus `$1000` times the
//! table, so that the file names the table's pages as the table's number.

use std::collections::BTreeMap;

use toml_edit::{DocumentMut, Item, Table, Value};

use super::{SourceError, hex, invalid, own_line_comments};
use crate::map16::{Map16Tile, Tile8Ref};

/// Tiles in a page.
pub const PAGE_TILES: u16 = 0x100;
/// The pages a page file can be: 2 to `$7F`. Pages 0 and 1 are the game's
/// own tables.
pub const PAGES: std::ops::RangeInclusive<u8> = 0x02..=0x7F;
/// What a tile of a page acts like when its file does not say.
pub const DEFAULT_ACTS: u16 = 0x130;

/// Foreground pages, 2 to `$7F`, whose tiles act like others; or
/// background pages, `$00` to `$FF`, whose tiles do not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageKind {
    Foreground,
    Background,
}

/// One tile: its graphics, and the tile it acts like.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Map16Entry {
    pub gfx: Map16Tile,
    pub acts: u16,
}

impl Default for Map16Entry {
    fn default() -> Self {
        Self {
            gfx: Map16Tile::default(),
            acts: DEFAULT_ACTS,
        }
    }
}

/// A page file's tiles, by full tile number.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Map16Page {
    pub tiles: BTreeMap<u16, Map16Entry>,
}

/// The own-line comments of a page file, kept when Kobo writes it again:
/// those at the top and those before each tile.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct PageComments {
    pub top: Vec<String>,
    pub tiles: BTreeMap<u16, Vec<String>>,
}

impl Map16Page {
    /// The tile, or the empty one if the file does not list it.
    pub fn tile(&self, number: u16) -> Map16Entry {
        self.tiles.get(&number).copied().unwrap_or_default()
    }

    /// Writes the page in Kobo's format.
    pub fn to_toml(&self, kind: PageKind, comments: &PageComments) -> String {
        let mut out = String::new();
        for line in &comments.top {
            out += &format!("{line}\n");
        }
        if !comments.top.is_empty() {
            out.push('\n');
        }
        out += "[tiles]\n";
        for (&number, entry) in &self.tiles {
            for line in comments.tiles.get(&number).into_iter().flatten() {
                out += &format!("{line}\n");
            }
            let t = entry.gfx;
            let gfx: Vec<String> = [t.top_left, t.top_right, t.bottom_left, t.bottom_right]
                .iter()
                .map(|r| format!("\"{}\"", tile8_text(*r)))
                .collect();
            let acts = match kind {
                PageKind::Foreground => format!("acts = {}, ", hex(entry.acts as u32, 3)),
                PageKind::Background => String::new(),
            };
            out += &format!(
                "{} = {{ {acts}gfx = [{}] }}\n",
                hex(number as u32, 3),
                gfx.join(", ")
            );
        }
        out
    }

    /// Reads the file of page `page`.
    pub fn from_toml(
        kind: PageKind,
        page: u8,
        text: &str,
    ) -> Result<(Self, PageComments), SourceError> {
        let doc: DocumentMut = text.parse()?;
        let mut comments = PageComments::default();
        for (key, _) in doc.iter() {
            if key != "tiles" {
                return Err(invalid("file", format!("unknown key `{key}`")));
            }
        }
        let Some(item) = doc.get("tiles") else {
            return Ok((Self::default(), comments));
        };
        let table: &Table = item
            .as_table()
            .ok_or_else(|| invalid("tiles", "must be a table"))?;
        comments.top = own_line_comments(
            table
                .decor()
                .prefix()
                .and_then(|p| p.as_str())
                .unwrap_or(""),
            false,
        );
        let first = page as u16 * PAGE_TILES;
        let mut tiles = BTreeMap::new();
        for (key, item) in table.iter() {
            let at = format!("tiles.{key}");
            let number = key
                .strip_prefix("0x")
                .and_then(|h| u16::from_str_radix(h, 16).ok())
                .filter(|n| (first..first + PAGE_TILES).contains(n))
                .ok_or_else(|| {
                    invalid(
                        &at,
                        format!(
                            "a tile of page {} is {} to {}",
                            hex(page as u32, 2),
                            hex(first as u32, 3),
                            hex((first + PAGE_TILES - 1) as u32, 3)
                        ),
                    )
                })?;
            if let Some(prefix) = table
                .key(key)
                .and_then(|k| k.leaf_decor().prefix())
                .and_then(|p| p.as_str())
            {
                // A key's prefix starts on its own line: the comment after
                // the entry before it is that entry's suffix.
                let own = own_line_comments(prefix, false);
                if !own.is_empty() {
                    comments.tiles.insert(number, own);
                }
            }
            let entry = read_entry(item, &at, kind)?;
            if tiles.insert(number, entry).is_some() {
                return Err(invalid(&at, "is listed twice"));
            }
        }
        Ok((Self { tiles }, comments))
    }
}

fn read_entry(item: &Item, at: &str, kind: PageKind) -> Result<Map16Entry, SourceError> {
    let t = item
        .as_inline_table()
        .ok_or_else(|| invalid(at, "must be an inline table { acts, gfx }"))?;
    for (key, _) in t.iter() {
        if !(key == "gfx" || key == "acts" && kind == PageKind::Foreground) {
            return Err(invalid(at, format!("unknown key `{key}`")));
        }
    }
    let acts = match t.get("acts") {
        None => DEFAULT_ACTS,
        Some(v) => v
            .as_integer()
            .filter(|n| (0..=0x7FFF).contains(n))
            .ok_or_else(|| invalid(format!("{at}.acts"), "a tile number is 0x000 to 0x7FFF"))?
            as u16,
    };
    let gfx = match t.get("gfx") {
        None => Map16Tile::default(),
        Some(v) => {
            let at = format!("{at}.gfx");
            let refs: Vec<Tile8Ref> = v
                .as_array()
                .filter(|a| a.len() == 4)
                .ok_or_else(|| invalid(&at, "must be four 8x8 tiles"))?
                .iter()
                .map(|v: &Value| {
                    v.as_str().and_then(parse_tile8).ok_or_else(|| {
                        invalid(&at, "an 8x8 tile is \"TTT P\" with optional x, y, p")
                    })
                })
                .collect::<Result<_, _>>()?;
            Map16Tile {
                top_left: refs[0],
                top_right: refs[1],
                bottom_left: refs[2],
                bottom_right: refs[3],
            }
        }
    };
    Ok(Map16Entry { gfx, acts })
}

/// `0A0 2`, `1F3 4 xp`.
fn tile8_text(r: Tile8Ref) -> String {
    let mut flags = String::new();
    for (set, c) in [(r.flip_x(), 'x'), (r.flip_y(), 'y'), (r.priority(), 'p')] {
        if set {
            flags.push(c);
        }
    }
    let base = format!("{:03X} {}", r.tile(), r.palette());
    if flags.is_empty() {
        base
    } else {
        format!("{base} {flags}")
    }
}

fn parse_tile8(text: &str) -> Option<Tile8Ref> {
    let mut parts = text.split_whitespace();
    let tile = u16::from_str_radix(parts.next()?, 16)
        .ok()
        .filter(|&t| t < 0x400)?;
    let palette = parts.next()?.parse::<u8>().ok().filter(|&p| p < 8)?;
    let flags = parts.next().unwrap_or("");
    if parts.next().is_some() || !flags.chars().all(|c| "xyp".contains(c)) {
        return None;
    }
    Some(Tile8Ref::new(
        tile,
        palette,
        flags.contains('p'),
        flags.contains('x'),
        flags.contains('y'),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(bytes: [u8; 8], acts: u16) -> Map16Entry {
        Map16Entry {
            gfx: Map16Tile::from_bytes(bytes),
            acts,
        }
    }

    #[test]
    fn a_page_round_trips_with_its_comments() {
        let mut page = Map16Page::default();
        page.tiles.insert(
            0x200,
            entry([0xA0, 0x08, 0xB0, 0x08, 0xA1, 0x08, 0xB1, 0x28], 0x130),
        );
        page.tiles.insert(
            0x2FF,
            entry([0xFF, 0xC2, 0xFF, 0xC2, 0xFF, 0x82, 0xFF, 0x82], 0x025),
        );
        let mut comments = PageComments::default();
        comments.top.push("# Castle tiles".into());
        comments.tiles.insert(0x2FF, vec!["# unused".into()]);
        let text = page.to_toml(PageKind::Foreground, &comments);
        assert_eq!(
            text,
            "# Castle tiles\n\n[tiles]\n\
             0x200 = { acts = 0x130, gfx = [\"0A0 2\", \"0A1 2\", \"0B0 2\", \"0B1 2 p\"] }\n\
             # unused\n\
             0x2FF = { acts = 0x025, gfx = [\"2FF 0 xy\", \"2FF 0 y\", \"2FF 0 xy\", \"2FF 0 y\"] }\n"
        );
        let (back, back_comments) =
            Map16Page::from_toml(PageKind::Foreground, 0x02, &text).unwrap();
        assert_eq!(back, page);
        assert_eq!(back_comments, comments);
        assert_eq!(back.to_toml(PageKind::Foreground, &back_comments), text);
    }

    #[test]
    fn a_missing_tile_is_empty() {
        let (page, _) = Map16Page::from_toml(
            PageKind::Foreground,
            0x02,
            "[tiles]\n0x201 = { gfx = [\"001 1\", \"002 1\", \"003 1\", \"004 1\"] }\n",
        )
        .unwrap();
        assert_eq!(page.tile(0x200), Map16Entry::default());
        assert_eq!(page.tile(0x201).acts, DEFAULT_ACTS);
        assert_eq!(page.tile(0x201).gfx.top_right.tile(), 2);
    }

    #[test]
    fn bad_input_is_refused() {
        for (text, what) in [
            ("[tiles]\n0x300 = { acts = 0x25 }\n", "tile of another page"),
            ("[tiles]\n0x200 = { acts = 0x8000 }\n", "acts out of range"),
            (
                "[tiles]\n0x200 = { gfx = [\"400 0\", \"0 0\", \"0 0\", \"0 0\"] }\n",
                "8x8 tile past 3FF",
            ),
            (
                "[tiles]\n0x200 = { gfx = [\"000 8\", \"0 0\", \"0 0\", \"0 0\"] }\n",
                "palette 8",
            ),
            (
                "[tiles]\n0x200 = { gfx = [\"000 0 z\", \"0 0\", \"0 0\", \"0 0\"] }\n",
                "unknown flag",
            ),
            ("[tiles]\n0x200 = { gfx = [\"000 0\"] }\n", "one 8x8 tile"),
            ("[tiles]\n0x200 = { act = 0x25 }\n", "unknown key"),
            ("[pages]\n", "unknown table"),
        ] {
            assert!(
                Map16Page::from_toml(PageKind::Foreground, 0x02, text).is_err(),
                "{what}"
            );
        }
    }
}
