//! Importing a ROM's levels into a project.
//!
//! Only levels that differ from the clean ROM are imported, unless all are
//! asked for, so a project holds no Nintendo level data its author did not
//! change. What cannot be carried over is reported, not dropped silently.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::addr::{PcAddr, SnesAddr};
use crate::level::objects::Object;
use crate::level::{self, LEVEL_COUNT, LevelError, LevelFormat, tables};
use crate::map16::Map16Tile;
use crate::map16::pages::{self as map16_pages, PAGE_GROUPS};
use crate::mwl::{self, Mwl, MwlFile};
use crate::palette::{self, CustomPalette};
use crate::rats::{self, RatsBlock};
use crate::rom::Rom;
use crate::source::level::{
    BACKGROUND_ROWS, BackgroundTiles, Comments, Entrance, Layer2, Level, Sprites,
};
use crate::source::map16::{Map16Entry, Map16Page, PAGE_TILES, PageComments, PageKind};
use crate::source::project::{MANIFEST, Manifest};
use crate::sprites::{self, SpriteError};

/// Lunar Magic's version string (see [`Rom::lunar_magic_version`]).
const LUNAR_MAGIC_MARKER: SnesAddr = SnesAddr::new(0x0FF0A0);

#[derive(Debug, Error)]
pub enum ImportError {
    #[error(transparent)]
    Level(#[from] LevelError),
    #[error(transparent)]
    Sprites(#[from] SpriteError),
    #[error(transparent)]
    Rom(#[from] crate::rom::RomError),
    #[error("failed to write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0} already has a {MANIFEST}")]
    Exists(PathBuf),
    #[error(transparent)]
    Mwl(#[from] crate::mwl::MwlError),
    #[error("{path}: {source}")]
    Manifest {
        path: PathBuf,
        #[source]
        source: crate::source::SourceError,
    },
}

/// A level read from a ROM, with notes on what it holds that a build
/// cannot write yet.
pub fn read_level(rom: &Rom, number: u16) -> Result<(Level, Vec<String>), ImportError> {
    let mut notes = Vec::new();
    let data = level::read_objects(rom, number)?;
    let header = data.header();
    let layer2 = match data.layer2 {
        level::Layer2::None => Layer2::None,
        level::Layer2::Objects(list) => Layer2::Objects(list.objects),
        level::Layer2::Background(addr) if addr.bank() == 0x0C => Layer2::VanillaBackground(addr),
        level::Layer2::Background(addr) => match level::read_background(rom, number)? {
            Some(bg) => match background_tiles(&bg) {
                Some(tiles) => Layer2::Background(tiles),
                None => {
                    notes.push(format!(
                        "its background at {addr} has flags {:02X?}, which Kobo does not read",
                        bg.flags
                    ));
                    Layer2::None
                }
            },
            None => Layer2::None,
        },
    };
    let list = sprites::read_sprites_at(rom, level::sprite_ptr(rom, number)?)?;
    let vertical = header.level_mode.layer1_vertical();
    let format = LevelFormat::of(rom);
    let entrances = level::read_entrances(rom)?
        .into_iter()
        .zip(0..)
        .filter(|(bytes, id)| bytes.in_use(format) && bytes.destination(*id, format) == number)
        .map(|(bytes, id)| {
            let [_, fa, fc, fe] = bytes.0;
            Entrance::from_bytes(id, [fa, fc, fe])
        })
        .collect();
    let level = Level {
        header,
        entrance: level::read_secondary_header(rom, number)?,
        layer1: data.layer1.objects,
        layer2,
        sprites: Sprites::from_entries(list.header, &list.sprites, vertical),
        entrances,
        palette: palette::lm_level_palette(rom, number)?,
    };
    if level.palette.as_ref().is_some_and(high_bits) {
        notes.push("its palette has colours with bit 15 set, which is not kept".into());
    }
    let lunar = level
        .layer1
        .iter()
        .chain(match &level.layer2 {
            Layer2::Objects(list) => list.as_slice(),
            _ => &[],
        })
        .filter(|o| matches!(o, Object::Lunar { .. } | Object::Unplaced(_)))
        .count();
    if lunar > 0 {
        notes.push(format!("{lunar} of Lunar Magic's objects, kept as bytes"));
    }
    if LevelFormat::of(rom).lunar_magic && list.header.new_sprite_system {
        notes.push("sprites in Lunar Magic's list format".into());
    }
    Ok((level, notes))
}

/// What an import did.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Report {
    /// Levels written.
    pub levels: Vec<u16>,
    /// Map16 pages written.
    pub map16: Vec<u8>,
    /// Notes, each naming its level.
    pub notes: Vec<String>,
    /// Ranges of the clean ROM's space the imported ROM changed, outside
    /// everything the import read: the ROM's changes Kobo does not model.
    pub unmodelled: Vec<(SnesAddr, usize)>,
    /// Tagged blocks past the clean ROM's end that nothing the import read
    /// lies in.
    pub unread_blocks: Vec<RatsBlock>,
}

/// Imports a ROM's levels into a new project in `dir`: every level that
/// differs from `base`, or every level with `all`. `base` is what the
/// project will build onto: the clean ROM, or for an SA-1 ROM the clean
/// ROM with SA-1 Pack ([`crate::build::base_image`]); what differs from it
/// outside the levels is reported.
pub fn import_rom(rom: &Rom, base: &Rom, dir: &Path, all: bool) -> Result<Report, ImportError> {
    let manifest_path = dir.join(MANIFEST);
    if manifest_path.exists() {
        return Err(ImportError::Exists(dir.to_path_buf()));
    }
    let write = |path: &Path, text: String| {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ImportError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(path, text).map_err(|source| ImportError::Io {
            path: path.to_path_buf(),
            source,
        })
    };
    let mut report = Report::default();
    // As large as the ROM, if that is more than a build's default.
    let mut manifest = Manifest {
        sa1: rom.mapping().is_sa1(),
        rom_size: (rom.len() > crate::build::DEFAULT_ROM_SIZE).then_some(rom.len()),
        ..Manifest::default()
    };
    for number in 0..LEVEL_COUNT {
        let (level, notes) = read_level(rom, number)?;
        if !all && read_level(base, number)?.0 == level {
            continue;
        }
        let file = PathBuf::from("levels").join(format!("{number:03X}.toml"));
        write(&dir.join(&file), level.to_toml(&Comments::default()))?;
        manifest.levels.insert(number, file);
        report.levels.push(number);
        report.notes.extend(
            notes
                .into_iter()
                .map(|n| format!("level {number:03X}: {n}")),
        );
    }
    let (pages, notes) = read_map16_bg(rom, base)?;
    report.notes.extend(notes);
    for (page, tiles) in pages {
        let file = PathBuf::from("map16").join(format!("bg-{page:02X}.toml"));
        write(
            &dir.join(&file),
            tiles.to_toml(PageKind::Background, &PageComments::default()),
        )?;
        manifest.map16_bg.insert(page, file);
    }
    let (pages, notes) = read_map16(rom)?;
    report.notes.extend(notes);
    for (page, tiles) in pages {
        let file = PathBuf::from("map16").join(format!("{page:02X}.toml"));
        write(
            &dir.join(&file),
            tiles.to_toml(PageKind::Foreground, &PageComments::default()),
        )?;
        manifest.map16.insert(page, file);
        report.map16.push(page);
    }
    let mut changed_acts = 0;
    if map16_pages::installed(rom) {
        for tile in 0..0x200u16 {
            changed_acts +=
                usize::from(map16_pages::acts_like(rom, tile)?.is_some_and(|a| a != tile));
        }
    }
    if changed_acts > 0 {
        report.notes.push(format!(
            "{changed_acts} tiles of pages 0 and 1 act like another tile; not imported"
        ));
    }
    write(&manifest_path, manifest.to_toml())?;
    let read = read_spans(rom)?;
    report.unmodelled = unmodelled(rom, base, &read);
    report.unread_blocks = rats::blocks(rom)
        .into_iter()
        .filter(|block| {
            let start = rom.pc(block.start).map_or(0, |pc| pc.as_usize());
            start >= base.len()
                && !read
                    .iter()
                    .any(|r| r.start < start + block.len && start < r.end)
        })
        .collect();
    Ok(report)
}

/// A background in Lunar Magic's layout as a level file has it: 32 rows
/// from its own format with high bytes (`C` and `F`), 27 from the game's
/// behind a full pointer (`V`), whose tiles all take the flags' high nibble
/// as their high byte. Its own format without high bytes (`C` alone, from
/// older versions) comes through as 32 rows, the last five empty.
fn background_tiles(bg: &level::Background) -> Option<BackgroundTiles> {
    let flags = bg.flags?;
    let byte = |i: usize| bg.data.get(i).copied().unwrap_or(0) as u16;
    let mut tiles = vec![0; BACKGROUND_ROWS * 32];
    let (table, rows) = match (flags & 0x02 != 0, flags & 0x04 != 0, flags & 0x08 != 0) {
        (true, true, _) => {
            for (i, tile) in tiles.iter_mut().enumerate() {
                let (row, half, col) = (i / 32, i % 32 / 16, i % 16);
                let at = half * 512 + row * 16 + col;
                *tile = byte(1024 + at) << 8 | byte(at);
            }
            (flags >> 4, 32)
        }
        (custom, false, vanilla) if custom || vanilla => {
            let high = (flags as u16 >> 4) << 8;
            for row in 0..27 {
                for half in 0..2 {
                    for col in 0..16 {
                        tiles[row * 32 + half * 16 + col] =
                            high | byte(half * 432 + row * 16 + col);
                    }
                }
            }
            if custom { (flags >> 4, 32) } else { (0, 27) }
        }
        _ => return None,
    };
    Some(BackgroundTiles { table, rows, tiles })
}

/// BG Map16 pages from Lunar Magic's tables: the pages of each table that
/// lie in the RATS block it starts in, the last as far as the block goes,
/// but empty ones; and of the game's
/// own table, when the first pointer is still it, the pages that differ
/// from `clean`'s.
pub fn read_map16_bg(rom: &Rom, clean: &Rom) -> Result<Map16Import, ImportError> {
    const GAME_TABLE: u32 = 0x0D9100;
    let (mut out, notes) = (Vec::new(), Vec::new());
    let blocks: Vec<Range<usize>> = rats::blocks(rom)
        .into_iter()
        .filter_map(|b| {
            rom.pc(b.start)
                .ok()
                .map(|s| s.as_usize()..s.as_usize() + b.len)
        })
        .collect();
    let page_len = PAGE_TILES as usize * 8;
    // A table ends at the tile after its last used one, so a page may end
    // early: its tiles past the block are empty.
    let read_page = |at: SnesAddr, number: u8, len: usize| -> Result<Map16Page, ImportError> {
        let bytes = rom.read(at, len - len % 8)?;
        let mut page = Map16Page::default();
        for (i, def) in bytes.chunks(8).enumerate() {
            let entry = Map16Entry {
                gfx: Map16Tile::from_bytes(def.try_into().expect("8 bytes")),
                ..Map16Entry::default()
            };
            if entry != Map16Entry::default() {
                page.tiles
                    .insert(number as u16 * PAGE_TILES + i as u16, entry);
            }
        }
        Ok(page)
    };
    for table in 0..16u8 {
        let Some(at) = map16_pages::bg_table(rom, table)? else {
            continue;
        };
        if table == 0 && at.raw() == GAME_TABLE {
            for page in 0..2u8 {
                let start = at.add(page as u32 * page_len as u32);
                if rom.read(start, page_len)? != clean.read(start, page_len)? {
                    out.push((page, read_page(start, page, page_len)?));
                }
            }
            continue;
        }
        let Some(block) = rom
            .pc(at)
            .ok()
            .and_then(|pc| blocks.iter().find(|b| b.contains(&pc.as_usize())).cloned())
        else {
            continue;
        };
        for page in 0..16u8 {
            let start = at.add(page as u32 * page_len as u32);
            let Ok(pc) = rom.pc(start) else { break };
            if pc.as_usize() >= block.end {
                break;
            }
            let number = table * 16 + page;
            let tiles = read_page(start, number, page_len.min(block.end - pc.as_usize()))?;
            if !tiles.tiles.is_empty() {
                out.push((number, tiles));
            }
        }
    }
    Ok((out, notes))
}

/// Map16 pages by number, and notes on what was left out.
pub type Map16Import = (Vec<(u8, Map16Page)>, Vec<String>);

/// Map16 pages 2 to `$7F` from Lunar Magic's tables: the pages of each
/// group of 16 that lie in the RATS block its table starts in (Lunar Magic
/// allocates a group up to the last tile it uses, so the last page may
/// stop early, its other tiles empty), with what their tiles
/// act like, but empty tiles ([`Map16Entry::default`]), which a page file
/// leaves out, and pages with nothing else, which a build writes for the
/// pages of a group a project does not list.
/// A group whose table is in no RATS block is in an older Lunar Magic's
/// layout (2.43 and before) and is left out, with a note.
pub fn read_map16(rom: &Rom) -> Result<Map16Import, ImportError> {
    let mut out = Vec::new();
    let mut notes = Vec::new();
    if !map16_pages::installed(rom) {
        return Ok((out, notes));
    }
    let blocks: Vec<Range<usize>> = rats::blocks(rom)
        .into_iter()
        .filter_map(|b| {
            rom.pc(b.start)
                .ok()
                .map(|s| s.as_usize()..s.as_usize() + b.len)
        })
        .collect();
    let page_span = |group: &map16_pages::PageGroup, page: u8| -> Option<Range<usize>> {
        let at = group.definition(rom, page as u16 * PAGE_TILES).ok()??;
        let pc = rom.pc(at).ok()?.as_usize();
        Some(pc..pc + PAGE_TILES as usize * 8)
    };
    for group in &PAGE_GROUPS {
        let first = *group.pages().start();
        let Some(start) = group.definition(rom, first as u16 * PAGE_TILES)? else {
            continue;
        };
        let block = page_span(group, first)
            .and_then(|span| blocks.iter().find(|b| b.contains(&span.start)).cloned());
        let Some(block) = block else {
            notes.push(format!(
                "Map16 pages {first:02X}-{:02X}: their table at {start} is in no RATS block, \
                 an older Lunar Magic's layout; not imported",
                group.pages().end()
            ));
            continue;
        };
        for page in group.pages() {
            let Some(span) = page_span(group, page) else {
                break;
            };
            if !(block.start <= span.start && span.start < block.end) {
                break;
            }
            let mut tiles = Map16Page::default();
            let first = page as u16 * PAGE_TILES;
            for tile in first..first + PAGE_TILES {
                let at = group.definition(rom, tile)?.expect("the group has a table");
                // Past the block's end, where a group's last page may stop.
                let inside = rom.pc(at).is_ok_and(|pc| pc.as_usize() + 8 <= block.end);
                let gfx = if inside {
                    Map16Tile::from_bytes(rom.read(at, 8)?.try_into().expect("8 bytes"))
                } else {
                    Map16Tile::default()
                };
                let entry = Map16Entry {
                    gfx,
                    acts: map16_pages::acts_like(rom, tile)?
                        .unwrap_or(crate::source::map16::DEFAULT_ACTS),
                };
                if entry != Map16Entry::default() {
                    tiles.tiles.insert(tile, entry);
                }
            }
            if !tiles.tiles.is_empty() {
                out.push((page, tiles));
            }
        }
    }
    Ok((out, notes))
}

/// The file ranges of everything [`read_level`] reads for every level,
/// the tables included, and the header bytes a build rewrites.
fn read_spans(rom: &Rom) -> Result<Vec<Range<usize>>, ImportError> {
    let mut spans = Vec::new();
    let mut add = |addr: SnesAddr, len: usize| {
        if let Ok(pc) = rom.pc(addr) {
            spans.push(pc.as_usize()..pc.as_usize() + len);
        }
    };
    let count = LEVEL_COUNT as usize;
    add(tables::LAYER1_PTRS, 3 * count);
    add(tables::LAYER2_PTRS, 3 * count);
    add(tables::SPRITE_PTRS, 2 * count);
    for table in tables::SECONDARY_HEADERS {
        add(table, count);
    }
    for table in tables::ENTRANCES {
        add(table, tables::ENTRANCE_COUNT as usize);
    }
    if LevelFormat::of(rom).lunar_magic {
        add(tables::SPRITE_BANKS, count);
        add(LUNAR_MAGIC_MARKER, 64);
    }
    if level::has_level_flags(rom) {
        add(tables::LEVEL_FLAGS, count);
    }
    if map16_pages::installed(rom) {
        for group in &PAGE_GROUPS {
            add(group.pointer, 2);
            add(group.bank, 1);
            if let Ok(Some(start)) = group.definition(rom, *group.pages().start() as u16 * 0x100) {
                add(start, group.pages().count() * 0x800);
            }
        }
        add(map16_pages::ACTS_LIKE, 3);
        add(map16_pages::ACTS_LIKE_UPPER, 3);
        if let Ok(table) = rom.read_u24(map16_pages::ACTS_LIKE) {
            add(SnesAddr::new(table), 0x8000);
        }
        let upper = rom.read_u24(map16_pages::ACTS_LIKE_UPPER)?;
        if upper >> 16 != 0xFF {
            add(SnesAddr::new(upper + 0x8000), 0x8000);
        }
    }
    // The ROM size code, and the checksum and its complement.
    add(SnesAddr::new(0x00FFD7), 1);
    add(SnesAddr::new(0x00FFDC), 4);
    for number in 0..LEVEL_COUNT {
        let data = level::read_objects(rom, number)?;
        add(level::layer1_ptr(rom, number)?, data.layer1.len);
        if let (level::Layer2::Objects(list), level::Layer2Data::Objects(addr)) =
            (&data.layer2, level::layer2_ptr(rom, number)?)
        {
            add(addr, list.len);
        }
        if let Some(bg) = level::read_background(rom, number)? {
            add(bg.address, bg.stream_len);
        }
        let at = level::sprite_ptr(rom, number)?;
        add(at, sprites::read_sprites_at(rom, at)?.len);
    }
    spans.sort_by_key(|r| r.start);
    Ok(spans)
}

/// Bytes that differ from the clean ROM within its length and fall in no
/// span, gathered into ranges; differences fewer than 16 bytes apart are
/// one range.
fn unmodelled(rom: &Rom, clean: &Rom, read: &[Range<usize>]) -> Vec<(SnesAddr, usize)> {
    let mut covered = vec![false; clean.len()];
    for span in read {
        for flag in covered
            .iter_mut()
            .take(span.end.min(clean.len()))
            .skip(span.start)
        {
            *flag = true;
        }
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let (a, b) = (rom.data(), clean.data());
    for i in (0..clean.len().min(a.len())).filter(|&i| a[i] != b[i] && !covered[i]) {
        match ranges.last_mut() {
            Some(last) if i - last.end < 16 => last.end = i + 1,
            _ => ranges.push(i..i + 1),
        }
    }
    ranges
        .into_iter()
        .filter_map(|r| {
            let at = rom.mapping().pc_to_snes(PcAddr::new(r.start as u32)).ok()?;
            Some((at, r.len()))
        })
        .collect()
}

/// Which parts of a level differ between two ROMs.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct LevelDiff {
    pub level: u16,
    /// `header`, `entrance`, `layer1`, `layer2`, `sprites`, `entrances`, or the error
    /// reading the level from one of them.
    pub parts: Vec<String>,
}

/// Compares every level of two ROMs as Kobo reads them, whatever their
/// layouts: the same level moved or re-encoded is no difference, and nor
/// is a screen exit in the game's format and in Lunar Magic's.
pub fn diff_levels(a: &Rom, b: &Rom) -> Vec<LevelDiff> {
    let mut diffs = Vec::new();
    for level in 0..LEVEL_COUNT {
        // Screen exits in one format, apart from the other objects, by
        // screen: the game keeps one per screen whatever their place among
        // the objects, and Lunar Magic's save moves and sorts them.
        let read = |rom| {
            read_level(rom, level).map(|(mut l, _)| {
                let (mut exits, others): (Vec<Object>, Vec<Object>) = l
                    .layer1
                    .into_iter()
                    .partition(|o| matches!(o, Object::ScreenExit(_)));
                for object in &mut exits {
                    if let Object::ScreenExit(exit) = object {
                        *exit = exit.in_lunar_magic_format(level);
                    }
                }
                exits.sort_by_key(|o| match o {
                    Object::ScreenExit(exit) => exit.screen,
                    _ => 0,
                });
                l.layer1 = others;
                l.layer1.extend(exits);
                l
            })
        };
        let parts: Vec<String> = match (read(a), read(b)) {
            (Ok(x), Ok(y)) => [
                ("header", x.header != y.header),
                ("entrance", x.entrance != y.entrance),
                ("layer1", x.layer1 != y.layer1),
                ("layer2", x.layer2 != y.layer2),
                ("sprites", x.sprites != y.sprites),
                ("entrances", x.entrances != y.entrances),
                ("palette", x.palette != y.palette),
            ]
            .into_iter()
            .filter(|(_, differs)| *differs)
            .map(|(part, _)| part.to_owned())
            .collect(),
            (x, y) => [x.err(), y.err()]
                .into_iter()
                .flatten()
                .map(|e| e.to_string())
                .collect(),
        };
        if !parts.is_empty() {
            diffs.push(LevelDiff { level, parts });
        }
    }
    diffs
}

/// A level from an MWL file, with notes on what it holds that a build
/// cannot write yet. A background comes through as the clean ROM's when
/// the file says it came from there and its tiles are that background's.
pub fn level_from_mwl(mwl: &Mwl, clean: &Rom) -> Result<(Level, Vec<String>), ImportError> {
    let mut notes = Vec::new();
    let header = mwl.layer1.primary_header();
    let mode = header.level_mode;
    let layer2 = match (&mwl.layer2.data, mode.layer2()) {
        (mwl::Layer2Data::Objects(data), level::Layer2Kind::HorizontalObjects)
        | (mwl::Layer2Data::Objects(data), level::Layer2Kind::VerticalObjects) => {
            Layer2::Objects(data.objects.clone())
        }
        (mwl::Layer2Data::Background(tiles), level::Layer2Kind::Background) => {
            let vanilla = mwl.layer2.header.source().and_then(|at| {
                let at = match at.bank() {
                    0xFF => SnesAddr::from_bank_offset(0x0C, at.offset()),
                    _ => at,
                };
                let pointer = level::Layer2Data::Tilemap(at);
                let bg = level::read_background_at(clean, mwl.info.level, pointer).ok()?;
                (at.bank() == 0x0C && bg.tiles() == *tiles).then_some(at)
            });
            match vanilla {
                Some(at) => Layer2::VanillaBackground(at),
                None => {
                    notes.push(
                        "its background is not one of the clean ROM's, and is not imported yet"
                            .into(),
                    );
                    Layer2::None
                }
            }
        }
        _ => Layer2::None,
    };
    let palette = mwl.layer1.custom_palette().then(|| CustomPalette {
        back_area: mwl.palette.back_area,
        palette: mwl.palette.colors.clone(),
    });
    if palette.as_ref().is_some_and(high_bits) {
        notes.push("its palette has colours with bit 15 set, which is not kept".into());
    }
    let lunar = mwl
        .entrances
        .entries
        .iter()
        .filter(|e| e.lm != [0, 0])
        .count();
    if lunar > 0 {
        notes.push(format!(
            "{lunar} secondary entrances use Lunar Magic 3's settings, which are not imported yet"
        ));
    }
    let list = &mwl.sprites.list;
    let level = Level {
        header,
        entrance: mwl.info.secondary,
        layer1: mwl.layer1.data.objects.clone(),
        layer2,
        sprites: Sprites::from_entries(list.header, &list.sprites, mode.layer1_vertical()),
        entrances: mwl
            .entrances
            .entries
            .iter()
            .map(|e| Entrance::from_bytes(e.id, e.tables))
            .collect(),
        palette,
    };
    Ok((level, notes))
}

/// Whether a palette has a colour with bit 15 set, which the SNES ignores
/// and a level file does not keep.
fn high_bits(p: &CustomPalette) -> bool {
    std::iter::once(p.back_area)
        .chain(p.palette.colors)
        .any(|c| c.0 & 0x8000 != 0)
}

/// Imports an MWL file into the project in `dir`, as `level` or the level
/// it was saved from, creating the project if there is none. The level's
/// file is `levels/NNN.toml`; the manifest is written again.
pub fn import_mwl(
    bytes: &[u8],
    clean: &Rom,
    dir: &Path,
    level: Option<u16>,
) -> Result<Report, ImportError> {
    let mwl = MwlFile::parse(bytes)?.decode(None)?;
    let number = level.unwrap_or(mwl.info.level);
    let (source, notes) = level_from_mwl(&mwl, clean)?;
    let manifest_path = dir.join(MANIFEST);
    let mut manifest = match fs::read_to_string(&manifest_path) {
        Ok(text) => Manifest::from_toml(&text).map_err(|source| ImportError::Manifest {
            path: manifest_path.clone(),
            source,
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Manifest::default(),
        Err(source) => {
            return Err(ImportError::Io {
                path: manifest_path,
                source,
            });
        }
    };
    let file = PathBuf::from("levels").join(format!("{number:03X}.toml"));
    let path = dir.join(&file);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ImportError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&path, source.to_toml(&Comments::default()))
        .map_err(|source| ImportError::Io { path, source })?;
    manifest.levels.insert(number, file);
    fs::write(&manifest_path, manifest.to_toml()).map_err(|source| ImportError::Io {
        path: manifest_path,
        source,
    })?;
    Ok(Report {
        levels: vec![number],
        notes: notes
            .into_iter()
            .map(|n| format!("level {number:03X}: {n}"))
            .collect(),
        ..Report::default()
    })
}
