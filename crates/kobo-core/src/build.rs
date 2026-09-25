//! Building a project into a ROM.
//!
//! A build starts from the clean ROM and writes what the project defines,
//! nothing else: a level the project does not list keeps the clean ROM's
//! data. The output depends on the clean ROM, the project files, and the
//! Kobo version alone.
//!
//! This is the build of step 2a, in the game's own formats, so it writes
//! nothing in Lunar Magic's layout: layer data goes in RATS blocks in the
//! expanded ROM, and sprite lists, which the game reads from bank `$07`
//! only, stay where the clean ROM has them when unchanged and go in bank
//! `$07`'s unused space otherwise.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::level::objects::{self, Jumps, Layout, Object, ObjectError};
use crate::level::{self, LevelError, tables};
use crate::rats::{Contents, FreeSpace, FreeSpaceError};
use crate::rom::{Rom, RomError, RomIdentity};
use crate::source::SourceError;
use crate::source::level::{Layer2, Level};
use crate::source::project::{MANIFEST, Manifest};
use crate::sprites::{self, SpriteEncodeError};

/// The size a project that writes levels and sets none is expanded to.
pub const DEFAULT_ROM_SIZE: usize = 0x10_0000;

/// Unused space in bank `$07`, all `$FF` in the US version (SMWDisX's
/// free space list), for sprite lists.
const BANK_07_FREE: [(u16, u16); 5] = [
    (0x80ED, 0x8100),
    (0xA179, 0xA600),
    (0xC226, 0xC300),
    (0xE76F, 0xF000),
    (0xFC90, 0x0000),
];

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Source {
        path: PathBuf,
        #[source]
        source: SourceError,
    },
    #[error("the clean ROM must be the vanilla USA image; this one's SHA-1 is {0}")]
    NotClean(String),
    #[error("level {level:03X}: {message}")]
    Level { level: u16, message: String },
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error(transparent)]
    ReadLevel(#[from] LevelError),
    #[error(transparent)]
    FreeSpace(#[from] FreeSpaceError),
}

fn level_error(level: u16, message: impl std::fmt::Display) -> BuildError {
    BuildError::Level {
        level,
        message: message.to_string(),
    }
}

/// A loaded project.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Project {
    pub manifest: Manifest,
    pub levels: Vec<(u16, Level)>,
}

impl Project {
    pub fn load(dir: &Path) -> Result<Self, BuildError> {
        let read = |path: PathBuf| {
            fs::read_to_string(&path).map_err(|source| BuildError::Io { path, source })
        };
        let manifest_path = dir.join(MANIFEST);
        let manifest = Manifest::from_toml(&read(manifest_path.clone())?).map_err(|source| {
            BuildError::Source {
                path: manifest_path,
                source,
            }
        })?;
        let mut levels = Vec::new();
        for (&number, file) in &manifest.levels {
            let path = dir.join(file);
            let (level, _) = Level::from_toml(&read(path.clone())?)
                .map_err(|source| BuildError::Source { path, source })?;
            levels.push((number, level));
        }
        Ok(Self { manifest, levels })
    }
}

/// Builds a project onto a copy of the clean ROM.
pub fn build(clean: &Rom, project: &Project) -> Result<Rom, BuildError> {
    if clean.identify() != RomIdentity::VanillaUsa {
        return Err(BuildError::NotClean(clean.sha1_hex()));
    }
    let mut rom = Rom::from_bytes(clean.data().to_vec())?;
    let size = project
        .manifest
        .rom_size
        .unwrap_or(if project.levels.is_empty() {
            clean.len()
        } else {
            DEFAULT_ROM_SIZE
        });
    rom.expand(size)?;
    let mut space = FreeSpace::scan(&rom);
    let mut bank07 = Bank07::new(clean);
    for (number, level) in &project.levels {
        write_level(&mut rom, clean, &mut space, &mut bank07, *number, level)?;
    }
    rom.fix_checksum()?;
    Ok(rom)
}

/// First-fit placement in [`BANK_07_FREE`].
struct Bank07 {
    free: Vec<(u32, u32)>,
}

impl Bank07 {
    fn new(clean: &Rom) -> Self {
        let free = BANK_07_FREE
            .iter()
            .map(|&(start, end)| {
                let end = if end == 0 { 0x1_0000 } else { end as u32 };
                (0x07_0000 + start as u32, 0x07_0000 + end)
            })
            // Only what the clean ROM really has free.
            .filter(|&(start, end)| {
                clean
                    .read(SnesAddr::new(start), (end - start) as usize)
                    .is_ok_and(|bytes| bytes.iter().all(|&b| b == 0xFF))
            })
            .collect();
        Self { free }
    }

    fn alloc(&mut self, len: usize) -> Option<SnesAddr> {
        let run = self
            .free
            .iter_mut()
            .find(|(start, end)| (end - start) as usize >= len)?;
        let at = SnesAddr::new(run.0);
        run.0 += len as u32;
        Some(at)
    }
}

/// Writes one level: layer data in new RATS blocks, and its pointers and
/// secondary header over the clean ROM's.
fn write_level(
    rom: &mut Rom,
    clean: &Rom,
    space: &mut FreeSpace,
    bank07: &mut Bank07,
    number: u16,
    level: &Level,
) -> Result<(), BuildError> {
    let err = |message: &dyn std::fmt::Display| level_error(number, message);
    let mode = level.header.level_mode;
    let vertical = |v: bool| {
        if v {
            Layout::Vertical
        } else {
            Layout::Horizontal
        }
    };
    let layout1 = vertical(mode.layer1_vertical());
    check_vanilla(number, &level.layer1)?;
    let layer1 = objects::encode(
        level.header.to_bytes(),
        &level.layer1,
        layout1,
        Jumps::Vanilla,
    )
    .map_err(|e: ObjectError| err(&format_args!("layer 1: {e}")))?;
    let at = place(rom, space, &layer1)?;
    rom.write_ptr(tables::LAYER1_PTRS.add(3 * number as u32), at)?;

    let layer2_ptr = tables::LAYER2_PTRS.add(3 * number as u32);
    match &level.layer2 {
        Layer2::None => {}
        Layer2::Objects(list) => {
            check_vanilla(number, list)?;
            let layout = vertical(mode.layer2() == level::Layer2Kind::VerticalObjects);
            let bytes = objects::encode(level.header.to_bytes(), list, layout, Jumps::Vanilla)
                .map_err(|e| err(&format_args!("layer 2: {e}")))?;
            let at = place(rom, space, &bytes)?;
            rom.write_ptr(layer2_ptr, at)?;
        }
        Layer2::VanillaBackground(addr) => {
            if addr.bank() != 0x0C {
                return Err(err(&format_args!(
                    "background {addr} is not in bank $0C, where the game reads backgrounds"
                )));
            }
            rom.write_ptr(layer2_ptr, SnesAddr::from_bank_offset(0xFF, addr.offset()))?;
        }
    }

    for (table, byte) in tables::SECONDARY_HEADERS
        .iter()
        .zip(level.entrance.to_bytes())
    {
        rom.write_u8(table.add(number as u32), byte)?;
    }

    let (header, entries) = level.sprites.to_entries(mode.layer1_vertical(), false);
    let list = sprites::encode(header, &entries, None)
        .map_err(|e: SpriteEncodeError| err(&format_args!("sprites: {e}")))?;
    let clean_at = level::sprite_ptr(clean, number)?;
    let clean_len = sprites::read_sprites_at(clean, clean_at)
        .map_err(|e| err(&e))?
        .len;
    let at = if clean.read(clean_at, clean_len)? == &list[..] {
        clean_at
    } else {
        let at = bank07.alloc(list.len()).ok_or_else(|| {
            err(&format_args!(
                "its {}-byte sprite list does not fit in bank $07's unused space",
                list.len()
            ))
        })?;
        rom.write(at, &list)?;
        at
    };
    rom.write_u16(tables::SPRITE_PTRS.add(2 * number as u32), at.offset())?;
    Ok(())
}

/// Lunar Magic's objects need its code in the ROM, which step 2a's builds
/// do not install.
fn check_vanilla(number: u16, list: &[Object]) -> Result<(), BuildError> {
    match list
        .iter()
        .position(|o| matches!(o, Object::Lunar { .. } | Object::Unplaced(_)))
    {
        Some(i) => Err(level_error(
            number,
            format!("object {i} is one of Lunar Magic's, which this build cannot write yet"),
        )),
        None => Ok(()),
    }
}

fn place(rom: &mut Rom, space: &mut FreeSpace, bytes: &[u8]) -> Result<SnesAddr, BuildError> {
    let at = space.alloc(rom, bytes.len(), Contents::Data)?;
    rom.write(at, bytes)?;
    Ok(at)
}
