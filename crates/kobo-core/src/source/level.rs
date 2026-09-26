//! Level files.
//!
//! ```toml
//! [header]
//! screens = 9
//! mode = 0x00
//! # ...
//!
//! [entrance]
//! screen = 0
//! # ...
//!
//! [layer1]
//! objects = [
//!     { obj = 0x05, x = 3, y = 10, height = 1, width = 5 },
//!     { ext = 0x2D, x = 52, y = 5 },
//!     { exit = 1, dest = 0x13 },
//! ]
//!
//! [layer2]
//! background = 0x0CE8FE
//!
//! [sprites]
//! memory = 0
//! list = [
//!     { id = 0x0F, x = 20, y = 10 },
//! ]
//! ```
//!
//! Objects are in drawing order. Positions are absolute tiles. A standard
//! object's settings byte is written as its handler reads it
//! ([`Settings`]): `width` and `height` in tiles, `type`, `length`, or,
//! for the tileset-specific objects, `settings`. `lm` entries are Lunar
//! Magic's placed objects, `raw` its unplaced ones, with their bytes.
//! `[entrance]` holds the game's secondary header and Lunar Magic's
//! per-level settings ([`LevelSettings`]), `[midway]` a midway entrance
//! with settings of its own.

use std::collections::BTreeMap;

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

use super::{SourceError, hex, hex_bytes, invalid, own_line_comments, parse_hex_bytes};
use crate::addr::SnesAddr;
use crate::entrance::{
    Background, EntranceSettings, LevelSettings, MidwayEntrance, SeparateMidway, offset_bits,
    offset_rows,
};
use crate::level::objects::{Object, ScreenExit, Settings};
use crate::level::{LevelMode, PrimaryHeader, SecondaryHeader};
use crate::names;
use crate::palette::{Color15, CustomPalette, Palette};
use crate::sprites::{SpriteEntry, SpriteHeader};

/// A level as its source file has it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Level {
    pub header: PrimaryHeader,
    pub entrance: SecondaryHeader,
    /// Lunar Magic's settings for the main and midway entrances and the
    /// level's layers, which a build writes with Kobo's code for them.
    pub settings: LevelSettings,
    pub layer1: Vec<Object>,
    pub layer2: Layer2,
    pub sprites: Sprites,
    /// The secondary entrances that lead here.
    pub entrances: Vec<Entrance>,
    /// The level's own palette, in Lunar Magic's layout, if it has one.
    pub palette: Option<CustomPalette>,
}

/// A secondary entrance, in the level it leads to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entrance {
    /// Its number, which screen exits name.
    pub id: u16,
    /// Screen, 0 to 31.
    pub screen: u8,
    /// X position setting, 0 to 7.
    pub x: u8,
    /// Y position setting, 0 to 15.
    pub y: u8,
    /// Entrance action, 0 to 7.
    pub action: u8,
    /// Foreground and background initial positions, 0 to 3 each; with a
    /// relative camera, the offset's low bits.
    pub fg_position: u8,
    pub bg_position: u8,
    /// Lunar Magic's settings: bits 4-7 of its `$05FE00` byte and its two
    /// further tables. Bit 3, Lunar Magic's copy of the destination's bit
    /// 8, is the level's and is not kept; builds write it where Kobo's
    /// exit code reads it.
    pub settings: EntranceSettings,
}

impl Entrance {
    /// From its bytes in the tables (the first, the destination, aside),
    /// and its Lunar Magic settings, read from those and further tables.
    pub fn from_bytes(id: u16, bytes: [u8; 3], settings: EntranceSettings) -> Self {
        let [fa, fc, fe] = bytes;
        Self {
            id,
            screen: fc & 0x1F,
            x: fc >> 5,
            y: fa & 0x0F,
            action: fe & 0x07,
            fg_position: (fa >> 4) & 0x03,
            bg_position: fa >> 6,
            settings,
        }
    }

    /// Its bytes in the tables at `$05FA00`, `$05FC00`, and `$05FE00`, and
    /// in Lunar Magic's two further ones.
    pub fn to_bytes(self) -> ([u8; 3], [u8; 2]) {
        let (flags, extra) = self.settings.to_bytes();
        if let Some(bytes) = self.settings.overworld {
            return (bytes, extra);
        }
        (
            [
                self.bg_position << 6 | (self.fg_position & 0x03) << 4 | (self.y & 0x0F),
                self.x << 5 | (self.screen & 0x1F),
                flags | (self.action & 0x07),
            ],
            extra,
        )
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Layer2 {
    /// Nothing, as the boss arenas' modes have it.
    None,
    Objects(Vec<Object>),
    /// A background tilemap already in the clean ROM, by address.
    VanillaBackground(SnesAddr),
    /// A background of the level's own, in Lunar Magic's layout.
    Background(BackgroundTiles),
}

/// A background's tiles: two halves of 16 columns side by side, as
/// [`BACKGROUND_ROWS`] rows of 32 tiles, each the left half's row then the
/// right half's.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BackgroundTiles {
    /// The BG Map16 table its tile numbers are in, 0 to 15.
    pub table: u8,
    /// 32 rows, Lunar Magic's own format; or 27, the game's, whose tiles
    /// all share one high byte and whose table is 0.
    pub rows: usize,
    /// `BACKGROUND_ROWS * 32` tiles; past `rows` rows they are 0.
    pub tiles: Vec<u16>,
}

/// Rows a background's tiles are written as.
pub const BACKGROUND_ROWS: usize = 32;

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Sprites {
    /// Sprite memory setting, 0 to 31.
    pub memory: u8,
    pub buoyancy: bool,
    pub buoyancy_no_layer2: bool,
    pub list: Vec<Sprite>,
}

/// A sprite at absolute tile coordinates.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sprite {
    pub id: u8,
    pub x: u16,
    pub y: u16,
    /// Extra bits, 0 to 3.
    pub extra_bits: u8,
    /// Extension bytes, for sprites a tool gives some.
    pub extension: Vec<u8>,
}

impl Sprites {
    /// From a parsed list, placing each sprite as the game does on a
    /// level of that orientation.
    pub fn from_entries(header: SpriteHeader, entries: &[SpriteEntry], vertical: bool) -> Self {
        let list = entries
            .iter()
            .map(|entry| {
                let (x, y) = entry.tile_position(vertical);
                Sprite {
                    id: entry.id,
                    x: x as u16,
                    y: y as u16,
                    extra_bits: entry.extra_bits,
                    extension: entry.extension.clone(),
                }
            })
            .collect();
        Self {
            memory: header.memory,
            buoyancy: header.buoyancy,
            buoyancy_no_layer2: header.buoyancy_no_layer2,
            list,
        }
    }

    /// The header and entries to encode, with `new_sprite_system` for the
    /// list format. Positions a list cannot hold are left for the encoder
    /// to refuse.
    pub fn to_entries(
        &self,
        vertical: bool,
        new_sprite_system: bool,
    ) -> (SpriteHeader, Vec<SpriteEntry>) {
        let header = SpriteHeader {
            memory: self.memory,
            buoyancy: self.buoyancy,
            buoyancy_no_layer2: self.buoyancy_no_layer2,
            new_sprite_system,
        };
        let entries = self
            .list
            .iter()
            .map(|sprite| {
                let (along, across) = if vertical {
                    (sprite.y, sprite.x)
                } else {
                    (sprite.x, sprite.y)
                };
                SpriteEntry {
                    id: sprite.id,
                    extra_bits: sprite.extra_bits,
                    screen: (along / 16).min(0xFF) as u8,
                    x: (along % 16) as u8,
                    y: across,
                    extension: sprite.extension.clone(),
                }
            })
            .collect();
        (header, entries)
    }
}

/// Where a user's own-line comments go when a file is written again: before
/// the entry of a list they preceded, keyed by list and index, or before
/// the list's closing bracket, at the index past its last entry.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Comments {
    /// Comments at the top of the file.
    pub top: Vec<String>,
    pub entries: BTreeMap<(&'static str, usize), Vec<String>>,
}

impl Level {
    /// Writes the level in Kobo's format.
    pub fn to_toml(&self, comments: &Comments) -> String {
        let mut out = String::new();
        for line in &comments.top {
            out += &format!("{line}\n");
        }
        if !comments.top.is_empty() {
            out.push('\n');
        }
        let h = &self.header;
        out += "[header]\n";
        out += &format!("screens = {}\n", h.screens);
        out += &named("mode", hex(h.level_mode.0 as u32, 2), h.level_mode.name());
        out += &named(
            "tileset",
            hex(h.object_tileset as u32, 1),
            names::object_tileset(h.object_tileset),
        );
        out += &named(
            "sprite_tileset",
            hex(h.sprite_tileset as u32, 1),
            names::sprite_tileset(h.sprite_tileset),
        );
        out += &named("music", h.music.to_string(), names::music(h.music));
        out += &format!("time = {}\n", h.time);
        out += &format!("bg_palette = {}\n", h.bg_palette);
        out += &format!("fg_palette = {}\n", h.fg_palette);
        out += &format!("sprite_palette = {}\n", h.sprite_palette);
        out += &format!("back_area = {}\n", h.back_area);
        out += &format!("item_memory = {}\n", h.item_memory);
        out += &format!("vertical_scroll = {}\n", h.vertical_scroll);
        out += &format!("layer3_priority = {}\n", h.layer3_priority);

        let e = &self.entrance;
        let lm = &self.settings;
        out += "\n[entrance]\n";
        out += &format!("screen = {}\n", e.entrance_screen);
        let (x_high, y_high) = lm.tile_position.unwrap_or((0, 0));
        out += &format!("x = {}\n", x_high << 3 | e.entrance_x);
        out += &format!("y = {}\n", (y_high as u16) << 4 | e.entrance_y as u16);
        if lm.tile_position.is_some() {
            out += "method = 2\n";
        }
        out += &format!("action = {}\n", e.entrance_action);
        out += &format!(
            "midway_screen = {}\n",
            (lm.midway.screen_high as u8) << 4 | e.midway_screen
        );
        out += &camera_lines(e.fg_position, e.bg_position, lm.relative);
        out += &format!("layer2_scroll = {}\n", e.layer2_scroll);
        if let Some(v) = lm.layer2_vertical_scroll {
            out += &format!("layer2_vertical_scroll = {v}\n");
        }
        out += &format!("layer3 = {}\n", e.layer3);
        out += &format!("no_yoshi_intro = {}\n", e.no_yoshi_intro);
        out += &format!("vertical_position = {}\n", e.vertical_position);
        if e.unknown {
            out += "unknown = true\n";
        }
        for (key, value) in [
            ("slippery", lm.slippery),
            ("water", lm.water),
            ("face_left", lm.face_left),
            ("smart_spawn", lm.smart_spawn),
        ] {
            if value {
                out += &format!("{key} = true\n");
            }
        }
        if lm.spawn_range != 0 {
            out += &format!("spawn_range = {}\n", lm.spawn_range);
        }
        if !lm.auto_screens {
            out += "auto_screens = false\n";
        }
        match lm.background {
            Background::Height(27) => {}
            Background::Height(rows) => out += &format!("bg_height = {rows}\n"),
            Background::Offset(rows) => out += &format!("bg_offset = {rows}\n"),
            Background::Absolute => out += "bg_offset = \"absolute\"\n",
        }
        match lm.midway.separate {
            None => {}
            Some(SeparateMidway::Redirect(level)) => {
                out += "\n[midway]\n";
                out += &format!("redirect = {}\n", hex(level as u32, 3));
            }
            Some(SeparateMidway::Entrance(m)) => {
                out += "\n[midway]\n";
                out += &format!("x = {}\n", m.x);
                out += &format!("y = {}\n", m.y);
                out += &format!("action = {}\n", m.action);
                out += &camera_lines(m.fg_position, m.bg_position, m.relative);
                for (key, value) in [
                    ("slippery", m.slippery),
                    ("water", m.water),
                    ("face_left", m.face_left),
                ] {
                    if value {
                        out += &format!("{key} = true\n");
                    }
                }
            }
        }

        let tileset = h.object_tileset;
        out += "\n[layer1]\n";
        out += &list("objects", "layer1", &self.layer1, comments, |o| {
            object_line(o, tileset)
        });
        match &self.layer2 {
            Layer2::None => {}
            Layer2::Objects(objects) => {
                out += "\n[layer2]\n";
                out += &list("objects", "layer2", objects, comments, |o| {
                    object_line(o, tileset)
                });
            }
            Layer2::VanillaBackground(addr) => {
                out += "\n[layer2]\n";
                out += &format!("background = {}\n", hex(addr.raw(), 6));
            }
            Layer2::Background(bg) => {
                out += "\n[layer2]\n";
                out += &format!("table = {}\n", hex(bg.table as u32, 1));
                out += &format!("rows = {}\n", bg.rows);
                let digits = if bg.tiles.iter().any(|&t| t > 0xFFF) {
                    4
                } else {
                    3
                };
                out += "tiles = \"\"\"\n";
                for row in bg.tiles.chunks(32).take(bg.rows) {
                    let cells: Vec<String> = row.iter().map(|t| format!("{t:0digits$X}")).collect();
                    out += &cells.join(" ");
                    out += "\n";
                }
                out += "\"\"\"\n";
            }
        }

        let s = &self.sprites;
        out += "\n[sprites]\n";
        out += &format!("memory = {}\n", s.memory);
        if s.buoyancy {
            out += "buoyancy = true\n";
        }
        if s.buoyancy_no_layer2 {
            out += "buoyancy_no_layer2 = true\n";
        }
        out += &list("list", "sprites", &s.list, comments, sprite_line);
        if !self.entrances.is_empty() {
            out += "\n[entrances]\n";
            out += &list(
                "list",
                "entrances",
                &self.entrances,
                comments,
                entrance_line,
            );
        }
        if let Some(palette) = &self.palette {
            out += "\n[palette]\n";
            out += &format!("back_area = \"{}\"\n", color_text(palette.back_area));
            out += "colors = [\n";
            for (row, colors) in palette.palette.colors.chunks(16).enumerate() {
                let cells: Vec<String> = colors.iter().map(|&c| color_text(c)).collect();
                out += &format!("    \"{}\",  # row {row:X}\n", cells.join(" "));
            }
            out += "]\n";
        }
        out
    }

    /// Reads a level file, with the own-line comments to keep when it is
    /// written again.
    pub fn from_toml(text: &str) -> Result<(Self, Comments), SourceError> {
        let doc: DocumentMut = text.parse()?;
        let mut comments = Comments::default();
        if let Some((_, item)) = doc.as_table().iter().next()
            && let Some(table) = item.as_table()
        {
            let prefix = table
                .decor()
                .prefix()
                .and_then(|p| p.as_str())
                .unwrap_or("");
            comments.top = own_line_comments(prefix, false);
        }
        check_keys(
            doc.as_table(),
            "file",
            &[
                "header",
                "entrance",
                "midway",
                "layer1",
                "layer2",
                "sprites",
                "entrances",
                "palette",
            ],
        )?;

        let header = read_header(table(&doc, "header")?)?;
        let midway = match doc.get("midway") {
            None => None,
            Some(item) => Some(
                item.as_table()
                    .ok_or_else(|| invalid("midway", "must be a table"))?,
            ),
        };
        let (entrance, settings) = read_entrance(table(&doc, "entrance")?, midway)?;
        let layer1 = table(&doc, "layer1")?;
        check_keys(layer1, "layer1", &["objects"])?;
        let layer1 = read_list(layer1, "objects", "layer1", &mut comments, read_object)?;
        let layer2 = match doc.get("layer2") {
            None => Layer2::None,
            Some(item) => {
                let t = item
                    .as_table()
                    .ok_or_else(|| invalid("layer2", "must be a table"))?;
                check_keys(
                    t,
                    "layer2",
                    &["objects", "background", "table", "rows", "tiles"],
                )?;
                if t.contains_key("tiles") {
                    Layer2::Background(read_background(t)?)
                } else {
                    match (t.get("objects"), t.get("background")) {
                        (Some(_), None) => Layer2::Objects(read_list(
                            t,
                            "objects",
                            "layer2",
                            &mut comments,
                            read_object,
                        )?),
                        (None, Some(bg)) => {
                            let addr = int(bg, "layer2.background", 0xFF_FFFF)?;
                            Layer2::VanillaBackground(SnesAddr::new(addr))
                        }
                        _ => {
                            return Err(invalid(
                                "layer2",
                                "needs one of `objects`, `background`, and `tiles`",
                            ));
                        }
                    }
                }
            }
        };
        let sprites = table(&doc, "sprites")?;
        check_keys(
            sprites,
            "sprites",
            &["memory", "buoyancy", "buoyancy_no_layer2", "list"],
        )?;
        let sprites = Sprites {
            memory: field(sprites, "sprites", "memory", 0x1F)? as u8,
            buoyancy: flag(sprites, "sprites", "buoyancy")?,
            buoyancy_no_layer2: flag(sprites, "sprites", "buoyancy_no_layer2")?,
            list: read_list(sprites, "list", "sprites", &mut comments, read_sprite)?,
        };
        let entrances = match doc.get("entrances") {
            None => Vec::new(),
            Some(item) => {
                let t = item
                    .as_table()
                    .ok_or_else(|| invalid("entrances", "must be a table"))?;
                check_keys(t, "entrances", &["list"])?;
                read_list(t, "list", "entrances", &mut comments, read_entrance_entry)?
            }
        };
        let palette = match doc.get("palette") {
            None => None,
            Some(item) => Some(read_palette(
                item.as_table()
                    .ok_or_else(|| invalid("palette", "must be a table"))?,
            )?),
        };
        Ok((
            Self {
                header,
                entrance,
                settings,
                layer1,
                layer2,
                sprites,
                entrances,
                palette,
            },
            comments,
        ))
    }
}

/// `#RRGGBB`, each channel the SNES 5-bit value times 8.
fn color_text(c: Color15) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r() * 8, c.g() * 8, c.b() * 8)
}

fn parse_color(at: &str, text: &str) -> Result<Color15, SourceError> {
    let bad = || {
        invalid(
            at,
            format!("{text:?} is not a colour #RRGGBB of multiples of 8"),
        )
    };
    let hex = text
        .strip_prefix('#')
        .filter(|h| h.len() == 6)
        .ok_or_else(bad)?;
    let channel = |i: usize| {
        u8::from_str_radix(&hex[i..i + 2], 16)
            .ok()
            .filter(|v| v % 8 == 0)
            .map(|v| v / 8)
            .ok_or_else(bad)
    };
    Ok(Color15::from_rgb5(channel(0)?, channel(2)?, channel(4)?))
}

fn read_palette(t: &Table) -> Result<CustomPalette, SourceError> {
    check_keys(t, "palette", &["back_area", "colors"])?;
    let back_area = t
        .get("back_area")
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid("palette.back_area", "must be a colour \"#RRGGBB\""))?;
    let back_area = parse_color("palette.back_area", back_area)?;
    let rows = t
        .get("colors")
        .and_then(|v| v.as_array())
        .filter(|a| a.len() == 16)
        .ok_or_else(|| invalid("palette.colors", "must be 16 rows of 16 colours"))?;
    let mut palette = Palette::default();
    for (row, value) in rows.iter().enumerate() {
        let at = format!("palette.colors[{row}]");
        let text = value
            .as_str()
            .ok_or_else(|| invalid(&at, "must be a string of 16 colours"))?;
        let cells: Vec<&str> = text.split_whitespace().collect();
        if cells.len() != 16 {
            return Err(invalid(&at, format!("has {} colours, not 16", cells.len())));
        }
        for (i, cell) in cells.iter().enumerate() {
            palette.colors[row * 16 + i] = parse_color(&at, cell)?;
        }
    }
    Ok(CustomPalette { back_area, palette })
}

fn list<T>(
    key: &str,
    name: &'static str,
    items: &[T],
    comments: &Comments,
    line: impl Fn(&T) -> (String, Option<String>),
) -> String {
    let mut out = format!("{key} = [\n");
    for (i, item) in items.iter().enumerate() {
        for comment in comments.entries.get(&(name, i)).into_iter().flatten() {
            out += &format!("    {comment}\n");
        }
        let (text, note) = line(item);
        out += &format!("    {text},");
        if let Some(note) = note {
            out += &format!("  # {note}");
        }
        out.push('\n');
    }
    for comment in comments
        .entries
        .get(&(name, items.len()))
        .into_iter()
        .flatten()
    {
        out += &format!("    {comment}\n");
    }
    out += "]\n";
    out
}

fn object_line(object: &Object, tileset: u8) -> (String, Option<String>) {
    let text = match object {
        Object::Standard {
            number,
            x,
            y,
            settings,
        } => {
            let (hi, lo) = (settings >> 4, settings & 0x0F);
            let fields = match Settings::of(*number) {
                Settings::HeightWidth => format!("height = {}, width = {}", hi + 1, lo + 1),
                Settings::HeightType => format!("height = {}, type = {lo}", hi + 1),
                Settings::TypeWidth => format!("type = {hi}, width = {}", lo + 1),
                Settings::Height if lo == 0 => format!("height = {}", hi + 1),
                Settings::Height => format!("height = {}, unused = {lo}", hi + 1),
                Settings::Width if hi == 0 => format!("width = {}", lo + 1),
                Settings::Width => format!("unused = {hi}, width = {}", lo + 1),
                Settings::Length => format!("length = {}", *settings as u16 + 1),
                Settings::Raw => format!("settings = {}", hex(*settings as u32, 2)),
            };
            format!(
                "{{ obj = {}, x = {x}, y = {y}, {fields} }}",
                hex(*number as u32, 2)
            )
        }
        Object::Extended { number, x, y } => {
            format!("{{ ext = {}, x = {x}, y = {y} }}", hex(*number as u32, 2))
        }
        Object::ScreenExit(exit) => {
            let mut text = format!(
                "{{ exit = {}, dest = {}",
                exit.screen,
                hex(exit.destination as u32, 2)
            );
            let secondary = exit.flags & 0x02 != 0;
            for (bit, name) in [
                (0x08, if secondary { "water" } else { "midway" }),
                (0x04, "lm_format"),
                (0x02, "secondary"),
                (0x01, "high"),
            ] {
                if exit.flags & bit != 0 {
                    text += &format!(", {name} = true");
                }
            }
            text + " }"
        }
        Object::Lunar { number, x, y, data } => format!(
            "{{ lm = {}, x = {x}, y = {y}, data = {} }}",
            hex(*number as u32, 2),
            hex_bytes(data)
        ),
        Object::Unplaced(bytes) => format!("{{ raw = {} }}", hex_bytes(bytes)),
    };
    (text, names::object(object, tileset).map(str::to_owned))
}

/// `fg_position` and `bg_position`, or with a relative camera the offset
/// in rows they make with `F` (`Fffbb`).
fn camera_lines(fg: u8, bg: u8, relative: Option<bool>) -> String {
    match relative {
        None => format!("fg_position = {fg}\nbg_position = {bg}\n"),
        Some(high) => format!("relative = {}\n", offset_rows(high, fg << 2 | bg)),
    }
}

fn entrance_line(e: &Entrance) -> (String, Option<String>) {
    let s = &e.settings;
    if let Some(bytes) = s.overworld {
        let text = format!(
            "{{ id = {}, overworld = {} }}",
            hex(e.id as u32, 3),
            hex_bytes(&bytes)
        );
        return (text, None);
    }
    let (x_high, y_high) = s.tile_position.unwrap_or((0, 0));
    let mut text = format!(
        "{{ id = {}, screen = {}, x = {}, y = {}",
        hex(e.id as u32, 3),
        e.screen,
        x_high << 3 | e.x,
        (y_high as u16) << 4 | e.y as u16,
    );
    if s.tile_position.is_some() {
        text += ", method = 2";
    }
    text += &format!(", action = {}", e.action);
    // A secondary entrance's offset is `Fbbff`.
    match s.relative {
        None => {
            text += &format!(
                ", fg_position = {}, bg_position = {}",
                e.fg_position, e.bg_position
            )
        }
        Some(high) => {
            text += &format!(
                ", relative = {}",
                offset_rows(high, e.bg_position << 2 | e.fg_position)
            )
        }
    }
    for (key, value) in [
        ("slippery", s.slippery),
        ("water", s.water),
        ("face_left", s.face_left),
    ] {
        if value {
            text += &format!(", {key} = true");
        }
    }
    (text + " }", None)
}

fn sprite_line(sprite: &Sprite) -> (String, Option<String>) {
    let mut text = format!(
        "{{ id = {}, x = {}, y = {}",
        hex(sprite.id as u32, 2),
        sprite.x,
        sprite.y
    );
    if sprite.extra_bits != 0 {
        text += &format!(", extra = {}", sprite.extra_bits);
    }
    if !sprite.extension.is_empty() {
        text += &format!(", data = {}", hex_bytes(&sprite.extension));
    }
    (text + " }", Some(names::sprite(sprite.id).to_owned()))
}

/// `key = value`, and Kobo's name for the value after it if it has one.
fn named(key: &str, value: String, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("{key} = {value}  # {name}\n"),
        None => format!("{key} = {value}\n"),
    }
}

fn table<'a>(doc: &'a DocumentMut, key: &str) -> Result<&'a Table, SourceError> {
    doc.get(key)
        .ok_or_else(|| invalid(key, "is missing"))?
        .as_table()
        .ok_or_else(|| invalid(key, "must be a table"))
}

fn read_background(t: &Table) -> Result<BackgroundTiles, SourceError> {
    let table = t
        .get("table")
        .map(|v| int(v, "layer2.table", 15))
        .transpose()?
        .unwrap_or(0) as u8;
    let rows = t
        .get("rows")
        .map(|v| int(v, "layer2.rows", 32))
        .transpose()?
        .unwrap_or(32) as usize;
    if rows != 27 && rows != 32 {
        return Err(invalid("layer2.rows", "a background has 27 rows or 32"));
    }
    let text = t
        .get("tiles")
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid("layer2.tiles", "must be a string of rows of tiles"))?;
    let mut tiles = Vec::with_capacity(BACKGROUND_ROWS * 32);
    for (i, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let at = format!("layer2.tiles row {i}");
        let row: Vec<u16> = line
            .split_whitespace()
            .map(|cell| {
                u16::from_str_radix(cell, 16)
                    .map_err(|_| invalid(&at, format!("{cell:?} is not a tile number")))
            })
            .collect::<Result<_, _>>()?;
        if row.len() != 32 {
            return Err(invalid(&at, format!("has {} tiles, not 32", row.len())));
        }
        tiles.extend(row);
    }
    if tiles.len() != rows * 32 {
        return Err(invalid(
            "layer2.tiles",
            format!("has {} rows, not {rows}", tiles.len() / 32),
        ));
    }
    tiles.resize(BACKGROUND_ROWS * 32, 0);
    Ok(BackgroundTiles { table, rows, tiles })
}

fn check_keys<'a>(
    keys: impl IntoIterator<Item = (&'a str, impl Sized)>,
    at: &str,
    allowed: &[&str],
) -> Result<(), SourceError> {
    for (key, _) in keys {
        if !allowed.contains(&key) {
            return Err(invalid(at, format!("unknown key `{key}`")));
        }
    }
    Ok(())
}

fn int_value(value: &Value, at: &str, max: u32) -> Result<u32, SourceError> {
    value
        .as_integer()
        .ok_or_else(|| invalid(at, "must be an integer"))
        .and_then(|n| {
            u32::try_from(n)
                .ok()
                .filter(|&n| n <= max)
                .ok_or_else(|| invalid(at, format!("{n} is out of range (0 to {max})")))
        })
}

fn int(item: &Item, at: &str, max: u32) -> Result<u32, SourceError> {
    int_value(
        item.as_value()
            .ok_or_else(|| invalid(at, "must be a value"))?,
        at,
        max,
    )
}

fn field(table: &Table, at: &str, key: &str, max: u32) -> Result<u32, SourceError> {
    let at = format!("{at}.{key}");
    int(
        table.get(key).ok_or_else(|| invalid(&at, "is missing"))?,
        &at,
        max,
    )
}

fn flag(table: &Table, at: &str, key: &str) -> Result<bool, SourceError> {
    match table.get(key) {
        None => Ok(false),
        Some(item) => item
            .as_bool()
            .ok_or_else(|| invalid(format!("{at}.{key}"), "must be true or false")),
    }
}

fn read_header(t: &Table) -> Result<PrimaryHeader, SourceError> {
    check_keys(
        t,
        "header",
        &[
            "screens",
            "mode",
            "tileset",
            "sprite_tileset",
            "music",
            "time",
            "bg_palette",
            "fg_palette",
            "sprite_palette",
            "back_area",
            "item_memory",
            "vertical_scroll",
            "layer3_priority",
        ],
    )?;
    let f = |key, max| field(t, "header", key, max).map(|n| n as u8);
    let screens = f("screens", 32)?;
    if screens == 0 {
        return Err(invalid("header.screens", "must be 1 to 32"));
    }
    Ok(PrimaryHeader {
        bg_palette: f("bg_palette", 7)?,
        screens,
        back_area: f("back_area", 7)?,
        level_mode: LevelMode(f("mode", 0x1F)?),
        layer3_priority: flag(t, "header", "layer3_priority")?,
        music: f("music", 7)?,
        sprite_tileset: f("sprite_tileset", 0x0F)?,
        time: f("time", 3)?,
        sprite_palette: f("sprite_palette", 7)?,
        fg_palette: f("fg_palette", 7)?,
        item_memory: f("item_memory", 3)?,
        vertical_scroll: f("vertical_scroll", 3)?,
        object_tileset: f("tileset", 0x0F)?,
    })
}

/// An entrance's X and Y settings' low bits, and with position method 2
/// the high ones.
type Position = (u8, u16, Option<(u8, u8)>);

/// A table's values by key, for readers that inline tables and tables
/// share.
struct Fields<'a> {
    at: &'a str,
    get: &'a dyn Fn(&str) -> Option<&'a Value>,
}

impl Fields<'_> {
    fn path(&self, key: &str) -> String {
        format!("{}.{key}", self.at)
    }

    fn uint(&self, key: &str, max: u32) -> Result<Option<u32>, SourceError> {
        (self.get)(key)
            .map(|v| int_value(v, &self.path(key), max))
            .transpose()
    }

    fn required(&self, key: &str, max: u32) -> Result<u32, SourceError> {
        self.uint(key, max)?
            .ok_or_else(|| invalid(self.path(key), "is missing"))
    }

    fn int(&self, key: &str, min: i64, max: i64) -> Result<Option<i64>, SourceError> {
        (self.get)(key)
            .map(|v| {
                v.as_integer()
                    .filter(|n| (min..=max).contains(n))
                    .ok_or_else(|| {
                        invalid(
                            self.path(key),
                            format!("must be an integer from {min} to {max}"),
                        )
                    })
            })
            .transpose()
    }

    fn flag(&self, key: &str) -> Result<bool, SourceError> {
        (self.get)(key)
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| invalid(self.path(key), "must be true or false"))
            })
            .transpose()
            .map(|b| b.unwrap_or(false))
    }

    /// An entrance's position: `x` and `y`, which position method 2
    /// (`method = 2`) lets go past the game's settings to a tile, 0 to 31
    /// and 0 to 1023. Returns the low bits and, with method 2, the high.
    fn position(&self) -> Result<Position, SourceError> {
        match self.uint("method", 2)?.unwrap_or(1) {
            1 => Ok((
                self.required("x", 7)? as u8,
                self.required("y", 15)? as u16,
                None,
            )),
            2 => {
                let (x, y) = (self.required("x", 31)?, self.required("y", 1023)?);
                Ok((
                    (x & 7) as u8,
                    (y & 15) as u16,
                    Some(((x >> 3) as u8, (y >> 4) as u8)),
                ))
            }
            _ => Err(invalid(self.path("method"), "must be 1 or 2")),
        }
    }

    /// `fg_position` and `bg_position`, or `relative`: the offset in rows
    /// of layer 1 from the player, whose low four bits the two positions
    /// hold, `ff` above `bb` (or, `bb_above`, `bb` above `ff`). Returns
    /// the positions and `F` for a relative camera.
    fn camera(&self, bb_above: bool) -> Result<(u8, u8, Option<bool>), SourceError> {
        match self.int("relative", -16, 15)? {
            Some(rows) => {
                if (self.get)("fg_position").is_some() || (self.get)("bg_position").is_some() {
                    return Err(invalid(
                        self.path("relative"),
                        "replaces `fg_position` and `bg_position`",
                    ));
                }
                let (high, low) = offset_bits(rows as i8);
                let (upper, lower) = (low >> 2, low & 3);
                Ok(if bb_above {
                    (lower, upper, Some(high))
                } else {
                    (upper, lower, Some(high))
                })
            }
            None => Ok((
                self.required("fg_position", 3)? as u8,
                self.required("bg_position", 3)? as u8,
                None,
            )),
        }
    }
}

/// The `[entrance]` table and the `[midway]` one, if the level has it.
fn read_entrance(
    t: &Table,
    midway: Option<&Table>,
) -> Result<(SecondaryHeader, LevelSettings), SourceError> {
    check_keys(
        t,
        "entrance",
        &[
            "screen",
            "x",
            "y",
            "method",
            "action",
            "midway_screen",
            "fg_position",
            "bg_position",
            "relative",
            "layer2_scroll",
            "layer2_vertical_scroll",
            "layer3",
            "no_yoshi_intro",
            "vertical_position",
            "unknown",
            "slippery",
            "water",
            "face_left",
            "smart_spawn",
            "spawn_range",
            "auto_screens",
            "bg_height",
            "bg_offset",
        ],
    )?;
    let get = |key: &str| t.get(key).and_then(Item::as_value);
    let f = Fields {
        at: "entrance",
        get: &get,
    };
    let (x, y, tile_position) = f.position()?;
    let (fg_position, bg_position, relative) = f.camera(false)?;
    let midway_screen = f.required("midway_screen", 31)? as u8;
    let header = SecondaryHeader {
        layer2_scroll: f.required("layer2_scroll", 15)? as u8,
        entrance_y: y as u8,
        layer3: f.required("layer3", 3)? as u8,
        entrance_action: f.required("action", 7)? as u8,
        entrance_x: x,
        midway_screen: midway_screen & 0x0F,
        fg_position,
        bg_position,
        no_yoshi_intro: f.flag("no_yoshi_intro")?,
        unknown: f.flag("unknown")?,
        vertical_position: f.flag("vertical_position")?,
        entrance_screen: f.required("screen", 31)? as u8,
    };
    let background = match (t.get("bg_height"), t.get("bg_offset")) {
        (Some(_), Some(_)) => {
            return Err(invalid("entrance.bg_offset", "replaces `bg_height`"));
        }
        (None, Some(item)) if item.as_str() == Some("absolute") => Background::Absolute,
        (None, Some(_)) => Background::Offset(f.int("bg_offset", -15, 15)?.unwrap_or(0) as i8),
        (_, None) => match f.int("bg_height", 1, 32)? {
            Some(rows) => Background::Height(rows as u8),
            None => Background::Height(27),
        },
    };
    let settings = LevelSettings {
        slippery: f.flag("slippery")?,
        water: f.flag("water")?,
        tile_position,
        smart_spawn: f.flag("smart_spawn")?,
        spawn_range: f.uint("spawn_range", 3)?.unwrap_or(0) as u8,
        layer2_vertical_scroll: f.uint("layer2_vertical_scroll", 31)?.map(|v| v as u8),
        auto_screens: match t.get("auto_screens") {
            None => true,
            Some(_) => f.flag("auto_screens")?,
        },
        relative,
        face_left: f.flag("face_left")?,
        background,
        midway: crate::entrance::Midway {
            screen_high: midway_screen & 0x10 != 0,
            separate: midway.map(read_midway).transpose()?,
        },
    };
    Ok((header, settings))
}

fn read_midway(t: &Table) -> Result<SeparateMidway, SourceError> {
    let get = |key: &str| t.get(key).and_then(Item::as_value);
    let f = Fields {
        at: "midway",
        get: &get,
    };
    if t.contains_key("redirect") {
        check_keys(t, "midway", &["redirect"])?;
        return Ok(SeparateMidway::Redirect(
            f.required("redirect", 0x1FF)? as u16
        ));
    }
    check_keys(
        t,
        "midway",
        &[
            "x",
            "y",
            "action",
            "fg_position",
            "bg_position",
            "relative",
            "slippery",
            "water",
            "face_left",
        ],
    )?;
    let (fg_position, bg_position, relative) = f.camera(false)?;
    Ok(SeparateMidway::Entrance(MidwayEntrance {
        slippery: f.flag("slippery")?,
        water: f.flag("water")?,
        action: f.required("action", 7)? as u8,
        x: f.required("x", 31)? as u8,
        y: f.required("y", 1023)? as u16,
        fg_position,
        bg_position,
        relative,
        face_left: f.flag("face_left")?,
    }))
}

fn read_list<T>(
    t: &Table,
    key: &str,
    name: &'static str,
    comments: &mut Comments,
    read: impl Fn(&InlineTable, &str) -> Result<T, SourceError>,
) -> Result<Vec<T>, SourceError> {
    let at = format!("{name}.{key}");
    let array: &Array = t
        .get(key)
        .ok_or_else(|| invalid(&at, "is missing"))?
        .as_array()
        .ok_or_else(|| invalid(&at, "must be an array"))?;
    let mut items = Vec::with_capacity(array.len());
    for (i, value) in array.iter().enumerate() {
        let at = format!("{at}[{i}]");
        let prefix = value
            .decor()
            .prefix()
            .and_then(|p| p.as_str())
            .unwrap_or("");
        let own = own_line_comments(prefix, i > 0);
        if !own.is_empty() {
            comments.entries.insert((name, i), own);
        }
        let entry = value
            .as_inline_table()
            .ok_or_else(|| invalid(&at, "must be an inline table"))?;
        items.push(read(entry, &at)?);
    }
    // Comments before the closing bracket stay there.
    let trailing = own_line_comments(array.trailing().as_str().unwrap_or(""), !array.is_empty());
    if !trailing.is_empty() {
        comments.entries.insert((name, array.len()), trailing);
    }
    Ok(items)
}

/// An inline table's integer, if present.
fn opt(t: &InlineTable, at: &str, key: &str, max: u32) -> Result<Option<u32>, SourceError> {
    t.get(key)
        .map(|v| int_value(v, &format!("{at}.{key}"), max))
        .transpose()
}

fn req(t: &InlineTable, at: &str, key: &str, max: u32) -> Result<u32, SourceError> {
    opt(t, at, key, max)?.ok_or_else(|| invalid(format!("{at}.{key}"), "is missing"))
}

fn inline_flag(t: &InlineTable, at: &str, key: &str) -> Result<bool, SourceError> {
    match t.get(key) {
        None => Ok(false),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| invalid(format!("{at}.{key}"), "must be true or false")),
    }
}

fn bytes(t: &InlineTable, at: &str, key: &str) -> Result<Vec<u8>, SourceError> {
    let at = format!("{at}.{key}");
    let text = t
        .get(key)
        .ok_or_else(|| invalid(&at, "is missing"))?
        .as_str()
        .ok_or_else(|| invalid(&at, "must be a string of hex bytes"))?;
    parse_hex_bytes(&at, text)
}

fn keys_of(t: &InlineTable, at: &str, allowed: &[&str]) -> Result<(), SourceError> {
    check_keys(t.iter(), at, allowed)
}

fn read_object(t: &InlineTable, at: &str) -> Result<Object, SourceError> {
    let kinds: Vec<&str> = ["obj", "ext", "exit", "lm", "raw"]
        .into_iter()
        .filter(|k| t.contains_key(k))
        .collect();
    let [kind] = kinds[..] else {
        return Err(invalid(
            at,
            "needs exactly one of `obj`, `ext`, `exit`, `lm`, `raw`",
        ));
    };
    let pos = |t: &InlineTable| -> Result<(u16, u16), SourceError> {
        Ok((
            req(t, at, "x", 0xFFFF)? as u16,
            req(t, at, "y", 0xFFFF)? as u16,
        ))
    };
    Ok(match kind {
        "obj" => {
            let number = req(t, at, "obj", 0x3F)? as u8;
            let (x, y) = pos(t)?;
            let size = |key| {
                opt(t, at, key, 16)?
                    .filter(|&n| n > 0)
                    .map(|n| n as u8 - 1)
                    .ok_or_else(|| invalid(format!("{at}.{key}"), "must be 1 to 16"))
            };
            let nibble = |key| opt(t, at, key, 15).map(|n| n.unwrap_or(0) as u8);
            let layout = Settings::of(number);
            let allowed: &[&str] = match layout {
                Settings::HeightWidth => &["height", "width"],
                Settings::HeightType => &["height", "type"],
                Settings::TypeWidth => &["type", "width"],
                Settings::Height => &["height", "unused"],
                Settings::Width => &["unused", "width"],
                Settings::Length => &["length"],
                Settings::Raw => &["settings"],
            };
            let mut all = vec!["obj", "x", "y"];
            all.extend_from_slice(allowed);
            keys_of(t, at, &all)?;
            let settings = match layout {
                Settings::HeightWidth => size("height")? << 4 | size("width")?,
                Settings::HeightType => size("height")? << 4 | nibble("type")?,
                Settings::TypeWidth => nibble("type")? << 4 | size("width")?,
                Settings::Height => size("height")? << 4 | nibble("unused")?,
                Settings::Width => nibble("unused")? << 4 | size("width")?,
                Settings::Length => {
                    let length = req(t, at, "length", 256)?;
                    if length == 0 {
                        return Err(invalid(format!("{at}.length"), "must be 1 to 256"));
                    }
                    (length - 1) as u8
                }
                Settings::Raw => req(t, at, "settings", 0xFF)? as u8,
            };
            Object::Standard {
                number,
                x,
                y,
                settings,
            }
        }
        "ext" => {
            keys_of(t, at, &["ext", "x", "y"])?;
            let number = req(t, at, "ext", 0xFF)? as u8;
            let (x, y) = pos(t)?;
            Object::Extended { number, x, y }
        }
        "exit" => {
            keys_of(
                t,
                at,
                &[
                    "exit",
                    "dest",
                    "midway",
                    "water",
                    "lm_format",
                    "secondary",
                    "high",
                ],
            )?;
            let secondary = inline_flag(t, at, "secondary")?;
            let (w, wrong) = if secondary {
                ("water", "midway")
            } else {
                ("midway", "water")
            };
            if t.contains_key(wrong) {
                return Err(invalid(
                    at,
                    format!(
                        "`{wrong}` needs `secondary` {}",
                        if secondary { "unset" } else { "set" }
                    ),
                ));
            }
            let flags = (inline_flag(t, at, w)? as u8) << 3
                | (inline_flag(t, at, "lm_format")? as u8) << 2
                | (secondary as u8) << 1
                | inline_flag(t, at, "high")? as u8;
            Object::ScreenExit(ScreenExit {
                screen: req(t, at, "exit", 0x1F)? as u8,
                flags,
                destination: req(t, at, "dest", 0xFF)? as u8,
            })
        }
        "lm" => {
            keys_of(t, at, &["lm", "x", "y", "data"])?;
            let number = req(t, at, "lm", 0x3F)? as u8;
            let (x, y) = pos(t)?;
            Object::Lunar {
                number,
                x,
                y,
                data: bytes(t, at, "data")?,
            }
        }
        _ => {
            keys_of(t, at, &["raw"])?;
            Object::Unplaced(bytes(t, at, "raw")?)
        }
    })
}

fn read_entrance_entry(t: &InlineTable, at: &str) -> Result<Entrance, SourceError> {
    let id = req(t, at, "id", 0x1FF)? as u16;
    if t.contains_key("overworld") {
        keys_of(t, at, &["id", "overworld"])?;
        let raw = bytes(t, at, "overworld")?;
        let bytes: [u8; 3] = raw
            .try_into()
            .map_err(|_| invalid(format!("{at}.overworld"), "must be three bytes"))?;
        let settings = EntranceSettings {
            overworld: Some(bytes),
            ..EntranceSettings::default()
        };
        return Ok(Entrance::from_bytes(id, bytes, settings));
    }
    keys_of(
        t,
        at,
        &[
            "id",
            "screen",
            "x",
            "y",
            "method",
            "action",
            "fg_position",
            "bg_position",
            "relative",
            "slippery",
            "water",
            "face_left",
        ],
    )?;
    let get = |key: &str| t.get(key);
    let f = Fields { at, get: &get };
    let (x, y, tile_position) = f.position()?;
    let (fg_position, bg_position, relative) = f.camera(true)?;
    Ok(Entrance {
        id,
        screen: req(t, at, "screen", 0x1F)? as u8,
        x,
        y: y as u8,
        action: req(t, at, "action", 7)? as u8,
        fg_position,
        bg_position,
        settings: EntranceSettings {
            slippery: f.flag("slippery")?,
            tile_position,
            relative,
            face_left: f.flag("face_left")?,
            water: f.flag("water")?,
            overworld: None,
        },
    })
}

fn read_sprite(t: &InlineTable, at: &str) -> Result<Sprite, SourceError> {
    keys_of(t, at, &["id", "x", "y", "extra", "data"])?;
    Ok(Sprite {
        id: req(t, at, "id", 0xFF)? as u8,
        x: req(t, at, "x", 0xFFFF)? as u16,
        y: req(t, at, "y", 0xFFFF)? as u16,
        extra_bits: opt(t, at, "extra", 3)?.unwrap_or(0) as u8,
        extension: if t.contains_key("data") {
            bytes(t, at, "data")?
        } else {
            Vec::new()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "\
# Yoshi's Island 2, shortened.

[header]
screens = 2
mode = 0x00  # Horizontal, background
tileset = 0x7  # Normal 2
sprite_tileset = 0x8  # Banzai Bill
music = 0  # Overworld
time = 2
bg_palette = 1
fg_palette = 0
sprite_palette = 0
back_area = 2
item_memory = 0
vertical_scroll = 2
layer3_priority = false

[entrance]
screen = 0
x = 0
y = 11
action = 0
midway_screen = 9
fg_position = 2
bg_position = 2
layer2_scroll = 5
layer3 = 0
no_yoshi_intro = false
vertical_position = false

[layer1]
objects = [
    { obj = 0x21, x = 0, y = 24, length = 192 },  # Long ground ledge
    # The first pipe.
    { obj = 0x0F, x = 113, y = 21, height = 3, type = 0 },  # Vertical pipe
    { ext = 0x41, x = 17, y = 16 },  # Dragon coin
    { exit = 1, dest = 0x13, water = true, secondary = true },  # Screen exit
    { lm = 0x22, x = 1, y = 2, data = \"11 55\" },  # Direct Map16, page 0
    { raw = \"45 60 21\" },  # Music bypass
    # Last.
]

[layer2]
background = 0x0CD900

[sprites]
memory = 0
list = [
    { id = 0x0F, x = 20, y = 10 },  # Goomba
    { id = 0x35, x = 21, y = 10, extra = 2, data = \"01 02\" },  # Yoshi
]
";

    #[test]
    fn a_file_reads_and_writes_back_the_same() {
        let (level, comments) = Level::from_toml(TEXT).unwrap();
        assert_eq!(level.to_toml(&comments), TEXT);
        assert_eq!(comments.top, ["# Yoshi's Island 2, shortened."]);
        assert_eq!(comments.entries[&("layer1", 1)], ["# The first pipe."]);
        assert_eq!(comments.entries[&("layer1", 6)], ["# Last."]);
        assert_eq!(
            level.layer1[0],
            Object::Standard {
                number: 0x21,
                x: 0,
                y: 24,
                settings: 191
            }
        );
        assert_eq!(
            level.layer1[3],
            Object::ScreenExit(ScreenExit {
                screen: 1,
                flags: 0b1010,
                destination: 0x13
            })
        );
        assert_eq!(level.sprites.list[1].extension, [1, 2]);
        assert_eq!(
            level.layer2,
            Layer2::VanillaBackground(SnesAddr::new(0x0CD900))
        );
    }

    #[test]
    fn kobo_comments_are_its_own() {
        let text = TEXT.replace("  # Dragon coin", "  # a stale name");
        assert_ne!(text, TEXT);
        let (level, comments) = Level::from_toml(&text).unwrap();
        assert_eq!(level.to_toml(&comments), TEXT);
    }

    #[test]
    fn entrance_bytes() {
        let bytes = [0xAA, 0x24, 0xDB];
        let e = Entrance::from_bytes(0x1BC, bytes, EntranceSettings::from_bytes(bytes, [0, 0]));
        assert_eq!(
            (e.y, e.fg_position, e.bg_position, e.screen, e.x, e.action),
            (10, 2, 2, 4, 1, 3)
        );
        assert!(e.settings.slippery);
        assert_eq!(e.settings.tile_position, Some((1, 0)));
        // Bit 3 of the last byte, Lunar Magic's copy of the destination's
        // bit 8, is not kept.
        assert_eq!(e.to_bytes(), ([0xAA, 0x24, 0xD3], [0, 0]));
    }

    #[test]
    fn lunar_magic_entrance_settings() {
        let text = TEXT
            .replace("x = 0\ny = 11\n", "x = 9\ny = 22\nmethod = 2\n")
            .replace("fg_position = 2\nbg_position = 2\n", "relative = -10\n")
            .replace(
                "vertical_position = false\n",
                "vertical_position = false\nslippery = true\nbg_offset = \"absolute\"\n\n\
                 [midway]\nx = 17\ny = 22\naction = 0\nfg_position = 1\nbg_position = 2\n",
            );
        let (level, comments) = Level::from_toml(&text).unwrap();
        assert_eq!(level.settings.tile_position, Some((1, 1)));
        assert_eq!(level.settings.relative, Some(true));
        assert_eq!(
            (level.entrance.fg_position, level.entrance.bg_position),
            (1, 2)
        );
        assert_eq!(level.settings.background, Background::Absolute);
        let Some(SeparateMidway::Entrance(midway)) = level.settings.midway.separate else {
            panic!("{:?}", level.settings.midway)
        };
        assert_eq!((midway.x, midway.y), (17, 22));
        assert_eq!(level.to_toml(&comments), text);
    }

    #[test]
    fn mistakes_are_refused() {
        let bad = |from: &str, to: &str| {
            let text = TEXT.replace(from, to);
            assert_ne!(text, TEXT, "{from}");
            Level::from_toml(&text).unwrap_err().to_string()
        };
        assert!(bad("height = 3, type = 0", "width = 3, type = 0").contains("unknown key `width`"));
        assert!(bad("length = 192", "length = 0").contains("1 to 256"));
        assert!(bad("water = true, secondary", "midway = true, secondary").contains("midway"));
        assert!(bad("{ obj = 0x21,", "{ obj = 0x21, ext = 1,").contains("exactly one"));
        assert!(bad("screens = 2", "screens = 33").contains("out of range"));
        assert!(bad("data = \"11 55\"", "data = \"1 55\"").contains("hex byte"));
        assert!(bad("[sprites]", "[sprite]").contains("unknown key"));
    }
}
