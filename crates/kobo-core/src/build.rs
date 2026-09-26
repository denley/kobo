//! Building a project into a ROM.
//!
//! A build starts from the clean ROM and writes what the project defines,
//! nothing else: a level the project does not list keeps the clean ROM's
//! data. The output depends on the clean ROM, the project files, and the
//! Kobo version alone.
//!
//! A build runs fixed [`Stage`]s in order, each on the image the one
//! before it left, and can keep a snapshot of the image after each in a
//! [`Cache`], keyed by a hash chained through the stages: the previous
//! key, the stage and its version, Kobo's version, and the stage's inputs.
//! A build starts again from the last stage whose key has a snapshot.
//!
//! Levels are in the game's own formats: layer data goes in RATS blocks in
//! the expanded ROM, and sprite lists, which the game reads from bank `$07`
//! only, stay where the clean ROM has them when unchanged and go in bank
//! `$07`'s unused space otherwise. What only Lunar Magic's layout has, Map16
//! pages past 1, is written in that layout, with Kobo's own code for it
//! ([`crate::install`]) installed first.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::asar::{Asar, AsarError, Patch};
use crate::compress::rle1;
use crate::config::{self, ConfigError};
use crate::install;
use crate::level::objects::{self, Jumps, Layout, Object, ObjectError};
use crate::level::{self, LevelError, tables};
use crate::map16::pages as map16_pages;
use crate::rats::{Contents, FreeSpace, FreeSpaceError};
use crate::rom::{Rom, RomError, RomIdentity};
use crate::source::SourceError;
use crate::source::level::{BACKGROUND_ROWS, BackgroundTiles, Layer2, Level};
use crate::source::map16::{self as page_source, Map16Page, PageKind};
use crate::source::project::{MANIFEST, Manifest};
use crate::sprites::{self, SpriteEncodeError};
use crate::tools::{self, ToolError};
use sha1::{Digest, Sha1};

/// The size a project that writes anything and sets none is expanded to.
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
    #[error("patch {path}: {source}")]
    Patch {
        path: PathBuf,
        #[source]
        source: Box<AsarError>,
    },
    #[error(transparent)]
    Asar(Box<AsarError>),
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("the build cache at {path}: {source}")]
    Cache {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
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
    /// The folder the manifest's paths are relative to.
    pub root: PathBuf,
    pub manifest: Manifest,
    pub levels: Vec<(u16, Level)>,
    /// Map16 pages 2 to `$7F`.
    pub map16: Vec<(u8, Map16Page)>,
    /// BG Map16 pages, `$00` to `$FF` (table * 16 + page).
    pub map16_bg: Vec<(u8, Map16Page)>,
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
        let pages = |list: &std::collections::BTreeMap<u8, PathBuf>, kind| {
            let mut out = Vec::new();
            for (&page, file) in list {
                let path = dir.join(file);
                let (tiles, _) = Map16Page::from_toml(kind, page, &read(path.clone())?)
                    .map_err(|source| BuildError::Source { path, source })?;
                out.push((page, tiles));
            }
            Ok::<_, BuildError>(out)
        };
        let map16 = pages(&manifest.map16, PageKind::Foreground)?;
        let map16_bg = pages(&manifest.map16_bg, PageKind::Background)?;
        Ok(Self {
            root: dir.to_path_buf(),
            manifest,
            levels,
            map16,
            map16_bg,
        })
    }

    /// Whether the project has anything only Lunar Magic's layout holds,
    /// which needs Kobo's code for it installed: Map16 pages past 1, or
    /// Lunar Magic's objects or a background of its own in a level.
    pub fn lunar_magic_layout(&self) -> bool {
        !self.map16.is_empty()
            || !self.map16_bg.is_empty()
            || self.levels.iter().any(|(_, level)| {
                level.layer1.iter().any(handled)
                    || matches!(&level.layer2, Layer2::Background(_))
                    || matches!(&level.layer2, Layer2::Objects(list) if list.iter().any(handled))
            })
    }
}

/// The build's stages, in the order they run (docs/step-2.md). The ones
/// of the plan still to come take their places between these.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// The clean ROM, expanded to the project's size.
    Base,
    /// Kobo's code for Lunar Magic's layout, if the project uses it.
    Install,
    /// The project's early Asar patches.
    EarlyPatches,
    /// AddmusicK, with the project's music.
    Music,
    /// Map16 pages past 1 and the acts-like tables. GPS, which will run
    /// after it, rewrites the acts-like table.
    Map16,
    /// UberASM Tool, with the project's UberASM files. PIXI and GPS will
    /// run before it, as it reads what PIXI leaves (docs/toolchain.md).
    UberAsm,
    /// The project's late Asar patches.
    LatePatches,
    /// The levels the project defines.
    Levels,
}

impl Stage {
    pub const ALL: [Stage; 8] = [
        Stage::Base,
        Stage::Install,
        Stage::EarlyPatches,
        Stage::Music,
        Stage::Map16,
        Stage::UberAsm,
        Stage::LatePatches,
        Stage::Levels,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stage::Base => "base",
            Stage::Install => "install",
            Stage::EarlyPatches => "early patches",
            Stage::Music => "music",
            Stage::Map16 => "map16",
            Stage::UberAsm => "uberasm",
            Stage::LatePatches => "late patches",
            Stage::Levels => "levels",
        }
    }

    fn patches(self, project: &Project) -> &[PathBuf] {
        match self {
            Stage::EarlyPatches => &project.manifest.early_patches,
            Stage::LatePatches => &project.manifest.late_patches,
            _ => &[],
        }
    }

    /// Changes whenever what the stage writes for the same inputs does, so
    /// no snapshot of an older version is reused.
    fn version(self) -> u32 {
        1
    }

    /// The stage's inputs, as bytes that differ whenever its output would.
    /// A patch's inputs are every file in its folder and below, since a
    /// patch can include any of them; a tool's are its whole folder.
    fn inputs(self, clean: &Rom, project: &Project) -> Result<Vec<u8>, BuildError> {
        Ok(match self {
            Stage::Base => {
                let mut hash = Sha1::new();
                hash.update(clean.sha1());
                hash.update((rom_size(clean, project) as u64).to_le_bytes());
                if project.manifest.sa1 {
                    tools::hash_tree(&mut hash, &config::sa1pack_path()?)?;
                    tools::hash_tree(&mut hash, &config::asar_library_path()?)?;
                }
                hash.finalize().to_vec()
            }
            Stage::Install => {
                if !project.lunar_magic_layout() {
                    return Ok(Vec::new());
                }
                let mut hash = Sha1::new();
                tools::hash_tree(&mut hash, &config::asar_library_path()?)?;
                for (name, source) in install::LUNAR_MAGIC {
                    hash.update(name.as_bytes());
                    hash.update([0]);
                    hash.update(source.as_bytes());
                }
                hash.finalize().to_vec()
            }
            Stage::Map16 => {
                let mut bytes = Vec::new();
                for (kind, page, tiles) in project
                    .map16
                    .iter()
                    .map(|(p, t)| (PageKind::Foreground, p, t))
                    .chain(
                        project
                            .map16_bg
                            .iter()
                            .map(|(p, t)| (PageKind::Background, p, t)),
                    )
                {
                    bytes.push(*page);
                    bytes.push(kind as u8);
                    let text = tiles.to_toml(kind, &Default::default());
                    bytes.extend((text.len() as u64).to_le_bytes());
                    bytes.extend(text.as_bytes());
                }
                bytes
            }
            Stage::EarlyPatches | Stage::LatePatches => {
                let patches = self.patches(project);
                if patches.is_empty() {
                    return Ok(Vec::new());
                }
                let mut hash = Sha1::new();
                tools::hash_tree(&mut hash, &config::asar_library_path()?)?;
                for patch in patches {
                    hash.update(patch.to_string_lossy().as_bytes());
                    hash.update([0]);
                    let folder = project.root.join(patch);
                    let folder = folder.parent().unwrap_or(&project.root);
                    tools::hash_tree(&mut hash, folder)?;
                }
                hash.finalize().to_vec()
            }
            Stage::Music => {
                let Some(music) = &project.manifest.music else {
                    return Ok(Vec::new());
                };
                let mut hash = Sha1::new();
                tools::hash_tree(&mut hash, &config::addmusick_path()?)?;
                tools::hash_tree(&mut hash, &config::asar_library_path()?)?;
                tools::hash_tree(&mut hash, &project.root.join(music))?;
                hash.finalize().to_vec()
            }
            Stage::UberAsm => {
                let Some(files) = &project.manifest.uberasm else {
                    return Ok(Vec::new());
                };
                let mut hash = Sha1::new();
                tools::hash_tree(&mut hash, &config::uberasm_path()?)?;
                tools::hash_tree(&mut hash, &config::asar_library_path()?)?;
                tools::hash_tree(&mut hash, &project.root.join(files))?;
                hash.finalize().to_vec()
            }
            Stage::Levels => {
                let mut bytes = Vec::new();
                for (number, level) in &project.levels {
                    bytes.extend(number.to_le_bytes());
                    let text = level.to_toml(&Default::default());
                    bytes.extend((text.len() as u64).to_le_bytes());
                    bytes.extend(text.as_bytes());
                }
                bytes
            }
        })
    }

    fn run(self, rom: &mut Rom, clean: &Rom, project: &Project) -> Result<(), BuildError> {
        match self {
            Stage::Base => {
                if project.manifest.sa1 {
                    *rom = apply_sa1pack(rom, rom_size(clean, project))?;
                }
                rom.expand(rom_size(clean, project))?;
            }
            Stage::Install => {
                if project.lunar_magic_layout() {
                    let asar = Asar::configured().map_err(|e| BuildError::Asar(Box::new(e)))?;
                    *rom = install::apply_lunar_magic(&asar, rom)
                        .map_err(|e| BuildError::Asar(Box::new(e)))?;
                }
            }
            Stage::Map16 => {
                write_map16(rom, project)?;
                write_map16_bg(rom, clean, project)?;
            }
            Stage::EarlyPatches | Stage::LatePatches => {
                let patches = self.patches(project);
                if patches.is_empty() {
                    return Ok(());
                }
                let asar = Asar::configured().map_err(|e| BuildError::Asar(Box::new(e)))?;
                for patch in patches {
                    let path = project.root.join(patch);
                    let spec = Patch::new(&path).include_path(&project.root);
                    let patched = asar.patch(rom, &spec).map_err(|source| BuildError::Patch {
                        path: path.clone(),
                        source: Box::new(source),
                    })?;
                    *rom = patched.rom;
                }
            }
            Stage::Music => {
                if let Some(music) = &project.manifest.music {
                    let tool = config::addmusick_path()?;
                    let asar = config::asar_library_path()?;
                    *rom = tools::addmusick(rom, &tool, &project.root.join(music), &asar)?;
                }
            }
            Stage::UberAsm => {
                if let Some(files) = &project.manifest.uberasm {
                    let tool = config::uberasm_path()?;
                    let asar = config::asar_library_path()?;
                    *rom = tools::uberasm(rom, &tool, &project.root.join(files), &asar)?;
                }
            }
            Stage::Levels => {
                // Sprite lists are compared with, and bank $07's space
                // taken from, the image the stage starts from: SA-1 Pack
                // has changed the clean ROM's by then.
                let before = Rom::from_bytes(rom.data().to_vec())?;
                let mut space = FreeSpace::scan(rom);
                let mut bank07 = Bank07::new(&before);
                for (number, level) in &project.levels {
                    write_level(rom, &before, &mut space, &mut bank07, *number, level)?;
                }
                write_entrances(rom, project)?;
            }
        }
        Ok(())
    }
}

/// SA-1 Pack on the clean ROM, and its 6 or 8 MiB patch for a larger
/// image, run through Asar from the configured SA-1 Pack folder. SA-1 Pack
/// applies to a clean ROM only, before anything else (docs/toolchain.md).
fn apply_sa1pack(rom: &Rom, size: usize) -> Result<Rom, BuildError> {
    let dir = config::sa1pack_path()?.join("asm");
    let asar = Asar::configured().map_err(|e| BuildError::Asar(Box::new(e)))?;
    let mut rom = Rom::from_bytes(rom.data().to_vec())?;
    let mut patches = vec![dir.join("sa1.asm")];
    match size {
        s if s > 0x60_0000 => patches.push(dir.join("8mb.asm")),
        s if s > 0x40_0000 => patches.push(dir.join("6mb.asm")),
        _ => {}
    }
    for path in patches {
        let patched = asar
            .patch(&rom, &Patch::new(&path))
            .map_err(|source| BuildError::Patch {
                path: path.clone(),
                source: Box::new(source),
            })?;
        rom = patched.rom;
    }
    Ok(rom)
}

/// The size the project expands the ROM to: its own, or
/// [`DEFAULT_ROM_SIZE`] if it writes anything, for the free space its
/// levels take and the space AddmusicK requires past 512 KiB.
fn rom_size(clean: &Rom, project: &Project) -> usize {
    let m = &project.manifest;
    let writes = !project.levels.is_empty()
        || project.lunar_magic_layout()
        || m.sa1
        || !m.early_patches.is_empty()
        || !m.late_patches.is_empty()
        || m.music.is_some()
        || m.uberasm.is_some();
    m.rom_size.unwrap_or(if writes {
        DEFAULT_ROM_SIZE
    } else {
        clean.len()
    })
}

/// Each stage's key, chained from the one before.
pub fn stage_keys(clean: &Rom, project: &Project) -> Result<Vec<[u8; 20]>, BuildError> {
    let mut key = [0u8; 20];
    Stage::ALL
        .iter()
        .map(|&stage| {
            let mut hash = Sha1::new();
            hash.update(key);
            hash.update(stage.name());
            hash.update(stage.version().to_le_bytes());
            hash.update(env!("CARGO_PKG_VERSION"));
            hash.update(stage.inputs(clean, project)?);
            key = hash.finalize().into();
            Ok(key)
        })
        .collect()
}

/// Snapshots of the image after each stage, one file per key.
#[derive(Clone, Debug)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `KOBO_CACHE_DIR`, or `kobo/stages` in the user's cache directory.
    pub fn user() -> Option<Self> {
        match std::env::var_os("KOBO_CACHE_DIR").filter(|d| !d.is_empty()) {
            Some(dir) => Some(Self::new(dir)),
            None => dirs::cache_dir().map(|d| Self::new(d.join("kobo").join("stages"))),
        }
    }

    fn path(&self, key: &[u8; 20]) -> PathBuf {
        let name: String = key.iter().map(|b| format!("{b:02x}")).collect();
        self.dir.join(format!("{name}.bin"))
    }

    fn get(&self, key: &[u8; 20]) -> Option<Vec<u8>> {
        fs::read(self.path(key)).ok()
    }

    fn put(&self, key: &[u8; 20], data: &[u8]) -> Result<(), BuildError> {
        let path = self.path(key);
        let fail = |source| BuildError::Cache {
            path: path.clone(),
            source,
        };
        fs::create_dir_all(&self.dir).map_err(fail)?;
        // Written whole under another name first, so a reader never sees
        // half a snapshot.
        let partial = path.with_extension(format!("{}.part", std::process::id()));
        fs::write(&partial, data).map_err(fail)?;
        fs::rename(&partial, &path).map_err(fail)
    }
}

/// The image a project with this manifest builds onto: the clean ROM
/// after the base stage (SA-1 Pack if the manifest says so, and the
/// expansion). Import compares a ROM with it to find what changed.
pub fn base_image(clean: &Rom, manifest: &Manifest) -> Result<Rom, BuildError> {
    let project = Project {
        root: PathBuf::from("."),
        manifest: manifest.clone(),
        levels: Vec::new(),
        map16: Vec::new(),
        map16_bg: Vec::new(),
    };
    let mut rom = Rom::from_bytes(clean.data().to_vec())?;
    Stage::Base.run(&mut rom, clean, &project)?;
    rom.fix_checksum()?;
    Ok(rom)
}

/// Builds a project onto a copy of the clean ROM, which must be the
/// vanilla image.
pub fn build(clean: &Rom, project: &Project) -> Result<Rom, BuildError> {
    build_cached(clean, project, None)
}

/// [`build`], reusing and keeping snapshots in `cache`.
pub fn build_cached(
    clean: &Rom,
    project: &Project,
    cache: Option<&Cache>,
) -> Result<Rom, BuildError> {
    if clean.identify() != RomIdentity::VanillaUsa {
        return Err(BuildError::NotClean(clean.sha1_hex()));
    }
    build_on(clean, project, cache)
}

/// [`build_cached`] on any base image laid out as the vanilla ROM is, for
/// synthetic images in tests.
pub fn build_on(base: &Rom, project: &Project, cache: Option<&Cache>) -> Result<Rom, BuildError> {
    let keys = stage_keys(base, project)?;
    // The last stage with a snapshot, and the image it holds.
    let resumed = cache.and_then(|cache| {
        (0..keys.len())
            .rev()
            .find_map(|i| Some((i + 1, cache.get(&keys[i])?)))
    });
    let (first, mut rom) = match resumed {
        Some((next, data)) => (next, Rom::from_bytes(data)?),
        None => (0, Rom::from_bytes(base.data().to_vec())?),
    };
    for (i, stage) in Stage::ALL.iter().enumerate().skip(first) {
        stage.run(&mut rom, base, project)?;
        if let Some(cache) = cache {
            cache.put(&keys[i], rom.data())?;
        }
    }
    rom.fix_checksum()?;
    Ok(rom)
}

/// Writes the project's Map16 pages into tables of their own, one for each
/// group of 16 pages the project uses, and what their tiles act like into the acts-like tables: the
/// one Kobo's install made for tiles below `$4000`, and one made here for
/// the rest when a page needs it.
fn write_map16(rom: &mut Rom, project: &Project) -> Result<(), BuildError> {
    if project.map16.is_empty() {
        return Ok(());
    }
    let mut space = FreeSpace::scan(rom);
    let pages: std::collections::BTreeMap<u8, &Map16Page> =
        project.map16.iter().map(|(n, p)| (*n, p)).collect();
    let tiles_of = |page: u8| {
        let first = page as u16 * page_source::PAGE_TILES;
        first..first + page_source::PAGE_TILES
    };
    let lower = SnesAddr::new(rom.read_u24(map16_pages::ACTS_LIKE)?);
    let mut upper = None;
    for (&page, tiles) in &pages {
        for tile in tiles_of(page) {
            let acts = tiles.tile(tile).acts;
            let at = if tile < 0x4000 {
                lower.add(2 * tile as u32)
            } else {
                let table = match upper {
                    Some(table) => table,
                    None => {
                        let table = space.alloc(rom, 0x8000, Contents::Data)?;
                        let default = page_source::DEFAULT_ACTS.to_le_bytes();
                        rom.write(table, &default.repeat(0x4000))?;
                        rom.write_u24(map16_pages::ACTS_LIKE_UPPER, table.raw() - 0x8000)?;
                        upper = Some(table);
                        table
                    }
                };
                table.add(2 * (tile - 0x4000) as u32)
            };
            rom.write_u16(at, acts)?;
        }
    }
    // A group's table is whole, as Lunar Magic allocates it: its editor
    // shows every page of a group with a table, and reads them all.
    for group in &map16_pages::PAGE_GROUPS {
        if !group.pages().any(|p| pages.contains_key(&p)) {
            continue;
        }
        let mut bytes = Vec::with_capacity(group.pages().count() * 0x800);
        for page in group.pages() {
            for tile in tiles_of(page) {
                let entry = pages.get(&page).map(|p| p.tile(tile)).unwrap_or_default();
                bytes.extend(entry.gfx.to_bytes());
            }
        }
        let table = place(rom, &mut space, &bytes)?;
        let (pointer, bank) = group.stored_for(tiles_of(*group.pages().start()).start, table);
        rom.write_u16(group.pointer, pointer)?;
        rom.write_u8(group.bank, bank)?;
    }
    Ok(())
}

/// Writes the project's BG Map16 pages into a table of their own, all 16
/// pages, for each BG Map16 table they are in, and points `$0EFD50` at
/// them. Pages 0 and 1 of the first table keep the game's tiles unless the
/// project lists them; any other page it does not list is empty, as Lunar
/// Magic leaves a table's unused pages.
fn write_map16_bg(rom: &mut Rom, clean: &Rom, project: &Project) -> Result<(), BuildError> {
    if project.map16_bg.is_empty() {
        return Ok(());
    }
    let mut space = FreeSpace::scan(rom);
    let pages: std::collections::BTreeMap<u8, &Map16Page> =
        project.map16_bg.iter().map(|(n, p)| (*n, p)).collect();
    for table in 0..16u8 {
        if !pages.keys().any(|&p| p >> 4 == table) {
            continue;
        }
        let mut bytes = Vec::with_capacity(16 * 0x800);
        for page in 0..16 {
            let number = table * 16 + page;
            match pages.get(&number) {
                Some(tiles) => {
                    let first = number as u16 * page_source::PAGE_TILES;
                    for tile in first..first + page_source::PAGE_TILES {
                        bytes.extend(tiles.tile(tile).gfx.to_bytes());
                    }
                }
                None if table == 0 && page < 2 => {
                    let at = crate::map16::tables::MAP16_BG_TILES.add(page as u32 * 0x800);
                    bytes.extend_from_slice(clean.read(at, 0x800)?);
                }
                None => bytes.extend([0; 0x800]),
            }
        }
        let at = place(rom, &mut space, &bytes)?;
        rom.write_u24(map16_pages::BG_TABLES.add(3 * table as u32), at.raw())?;
    }
    Ok(())
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
/// secondary header over the base's. A sprite list the base already has
/// for the level keeps its place.
fn write_level(
    rom: &mut Rom,
    base: &Rom,
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
    check_objects(number, &level.layer1)?;
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
            check_objects(number, list)?;
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
        Layer2::Background(bg) => {
            let (data, flags) = background_stream(bg).map_err(|e| err(&format_args!("{e}")))?;
            let stream = rle1::compress(&data).map_err(|e| err(&format_args!("{e}")))?;
            let at = place(rom, space, &stream)?;
            rom.write_ptr(layer2_ptr, at)?;
            rom.write_u8(tables::LEVEL_FLAGS.add(number as u32), flags)?;
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
    let base_at = level::sprite_ptr(base, number)?;
    let base_len = sprites::read_sprites_at(base, base_at)
        .map_err(|e| err(&e))?
        .len;
    let at = if base.read(base_at, base_len)? == &list[..] {
        base_at
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

/// Writes every project level's secondary entrances. A level's list is all
/// of them: an entrance the base ROM had leading to it that the list does
/// not name is cleared. In the game's format an entrance's number gives its
/// destination's bit 8, so it must match the level's.
fn write_entrances(rom: &mut Rom, project: &Project) -> Result<(), BuildError> {
    let format = level::LevelFormat::of(rom);
    let mut entrances = level::read_entrances(rom)?;
    let defined: Vec<u16> = project.levels.iter().map(|(n, _)| *n).collect();
    for (id, bytes) in (0..).zip(entrances.iter_mut()) {
        if bytes.in_use(format) && defined.contains(&bytes.destination(id, format)) {
            *bytes = level::EntranceBytes::default();
        }
    }
    let mut owner: Vec<Option<u16>> = vec![None; entrances.len()];
    for (number, level) in &project.levels {
        for entrance in &level.entrances {
            let id = entrance.id as usize;
            if let Some(other) = owner[id].replace(*number) {
                return Err(level_error(
                    *number,
                    format!("entrance {:03X} is also level {other:03X}'s", entrance.id),
                ));
            }
            if entrance.id >> 8 != number >> 8 {
                return Err(level_error(
                    *number,
                    format!(
                        "entrance {:03X} cannot lead here in the game's format, where its number gives the level's bit 8",
                        entrance.id
                    ),
                ));
            }
            let [fa, fc, fe] = entrance.to_bytes();
            entrances[id] = level::EntranceBytes([*number as u8, fa, fc, fe]);
        }
    }
    for (i, table) in tables::ENTRANCES.iter().enumerate() {
        let column: Vec<u8> = entrances.iter().map(|e| e.0[i]).collect();
        rom.write(*table, &column)?;
    }
    Ok(())
}

/// A background as its stream holds it, and the level flags that say
/// how: 32 rows in Lunar Magic's own format (`C` and `F`, the table in the
/// high nibble), low bytes then high bytes, each two halves of 32 rows of
/// 16; 27 rows in the game's format behind a full pointer (`V`, the tiles'
/// one high byte in the high nibble), low bytes, two halves of 27 rows.
fn background_stream(bg: &BackgroundTiles) -> Result<(Vec<u8>, u8), String> {
    let rows = bg.rows;
    let tile = |half: usize, row: usize, col: usize| bg.tiles[row * 32 + half * 16 + col];
    let cells = || {
        (0..2).flat_map(move |half| {
            (0..rows).flat_map(move |row| (0..16).map(move |col| (half, row, col)))
        })
    };
    if rows == BACKGROUND_ROWS {
        let low = cells().map(|(h, r, c)| tile(h, r, c) as u8);
        let high = cells().map(|(h, r, c)| (tile(h, r, c) >> 8) as u8);
        return Ok((low.chain(high).collect(), bg.table << 4 | 0x06));
    }
    let high = tile(0, 0, 0) >> 8;
    if bg.table != 0 || high > 0xF || cells().any(|(h, r, c)| tile(h, r, c) >> 8 != high) {
        return Err(format!(
            "a {rows}-row background has table 0 and one high byte, 0 to F, for all its tiles"
        ));
    }
    let low = cells().map(|(h, r, c)| tile(h, r, c) as u8).collect();
    Ok((low, 0x08 | (high as u8) << 4))
}

/// Lunar Magic's objects that place tiles (`22`, `23`, `27`, `29`), which
/// Kobo's code ([`crate::install`]) handles.
fn places_tiles(object: &Object) -> bool {
    matches!(
        object,
        Object::Lunar {
            number: 0x22 | 0x23 | 0x27 | 0x29,
            ..
        }
    )
}

/// Lunar Magic's objects Kobo's code handles besides those: its music
/// bypass (`26`) and its user object (`2D`).
fn handled(object: &Object) -> bool {
    places_tiles(object)
        || matches!(object, Object::Lunar { number: 0x2D, .. })
        || matches!(object, Object::Unplaced(b) if b.len() == 3 && b[0] & 0x60 == 0x40 && b[1] >> 4 == 6)
}

/// Lunar Magic's other objects need code of its own Kobo does not install
/// yet: its graphics and time limit bypasses (`24`, `25`, `28`) and its
/// long screen exits.
fn check_objects(number: u16, list: &[Object]) -> Result<(), BuildError> {
    match list
        .iter()
        .position(|o| matches!(o, Object::Lunar { .. } | Object::Unplaced(_)) && !handled(o))
    {
        Some(i) => Err(level_error(
            number,
            format!("object {i} is one of Lunar Magic's that this build cannot write yet"),
        )),
        None => Ok(()),
    }
}

fn place(rom: &mut Rom, space: &mut FreeSpace, bytes: &[u8]) -> Result<SnesAddr, BuildError> {
    let at = space.alloc(rom, bytes.len(), Contents::Data)?;
    rom.write(at, bytes)?;
    Ok(at)
}
