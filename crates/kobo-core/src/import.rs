//! Importing a ROM's levels into a project.
//!
//! Only levels that differ from the clean ROM are imported, unless all are
//! asked for, so a project holds no Nintendo level data its author did not
//! change. What cannot be carried over is reported, not dropped silently.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::level::{self, LEVEL_COUNT, LevelError, LevelFormat};
use crate::rom::Rom;
use crate::source::level::{Comments, Layer2, Level, Sprites};
use crate::source::project::{MANIFEST, Manifest};
use crate::sprites::{self, SpriteError};

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
    let level = Level {
        header,
        entrance: level::read_secondary_header(rom, number)?,
        layer1: data.layer1.objects,
        layer2,
        sprites: Sprites::from_entries(list.header, &list.sprites, vertical),
    };
    let lunar = level
        .layer1
        .iter()
        .chain(match &level.layer2 {
            Layer2::Objects(list) => list.as_slice(),
            _ => &[],
        })
        .filter(|o| {
            matches!(
                o,
                level::objects::Object::Lunar { .. } | level::objects::Object::Unplaced(_)
            )
        })
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
}

/// Imports a ROM's levels into a new project in `dir`: every level that
/// differs from the clean ROM, or every level with `all`.
pub fn import_rom(rom: &Rom, clean: &Rom, dir: &Path, all: bool) -> Result<Report, ImportError> {
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
    let mut manifest = Manifest::default();
    for number in 0..LEVEL_COUNT {
        let (level, notes) = read_level(rom, number)?;
        if !all && read_level(clean, number)?.0 == level {
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
    Ok(report)
}
