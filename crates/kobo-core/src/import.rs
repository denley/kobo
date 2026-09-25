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
use crate::mwl::{self, Mwl, MwlFile};
use crate::rats::{self, RatsBlock};
use crate::rom::Rom;
use crate::source::level::{Comments, Entrance, Layer2, Level, Sprites};
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
        level::Layer2::Background(addr) => {
            notes.push(format!(
                "its background at {addr} is Lunar Magic's own format, which is not imported yet"
            ));
            Layer2::None
        }
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
    };
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
    let mut manifest = Manifest {
        sa1: rom.mapping().is_sa1(),
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
        add(tables::LEVEL_FLAGS, count);
        add(LUNAR_MAGIC_MARKER, 64);
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
        let read = |rom| {
            read_level(rom, level).map(|(mut l, _)| {
                for object in &mut l.layer1 {
                    if let Object::ScreenExit(exit) = object {
                        *exit = exit.in_lunar_magic_format(level);
                    }
                }
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
    if mwl.layer1.custom_palette() {
        notes.push("its custom palette is not imported yet".into());
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
    };
    Ok((level, notes))
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
