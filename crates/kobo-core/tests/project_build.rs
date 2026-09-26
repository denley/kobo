//! The step 2a round trip: every vanilla level imported as text and built
//! back into a ROM, with its layer data in the expanded ROM, reads back as
//! the same level and renders as vanilla does. The full picture check of
//! all 512 levels is `render_hashes` (docs/testing.md); this renders a
//! sample of level kinds.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use kobo_core::bps;
use kobo_core::build::{self, Project};
use kobo_core::import;
use kobo_core::level::{self, tables};
use kobo_core::render::{self, RenderOptions};
use kobo_core::source::level::{Comments, Layer2, Level};
use kobo_core::{Rom, SnesAddr};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kobo-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn level_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(dir.join("levels"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    files
}

#[test]
fn vanilla_imports_and_builds_back() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = temp_dir("import-all");
    let report = import::import_rom(&clean, &clean, &dir, true).unwrap();
    assert_eq!(report.levels.len(), 512);
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    assert!(report.unmodelled.is_empty() && report.unread_blocks.is_empty());

    // Kobo's formatting is a fixed point.
    for file in level_files(&dir) {
        let text = fs::read_to_string(&file).unwrap();
        let (level, comments) = Level::from_toml(&text).unwrap();
        assert_eq!(level.to_toml(&comments), text, "{}", file.display());
    }

    let project = Project::load(&dir).unwrap();
    let built = build::build(&clean, &project).unwrap();
    assert_eq!(
        build::build(&clean, &project).unwrap().data(),
        built.data(),
        "same inputs, same output"
    );
    assert!(built.internal_header().checksum_pair_valid());
    assert_eq!(built.internal_header().checksum, built.compute_checksum());

    for number in 0..level::LEVEL_COUNT {
        let (read, _) = import::read_level(&built, number).unwrap();
        let (vanilla, _) = import::read_level(&clean, number).unwrap();
        assert_eq!(read, vanilla, "level {number:03X}");
        let layer1 = level::layer1_ptr(&built, number).unwrap();
        assert!(layer1.bank() >= 0x10, "level {number:03X} at {layer1}");
    }
    // A build of the imported vanilla ROM imports as no changes at all.
    let again = temp_dir("reimport");
    let built_path = again.with_extension("sfc");
    built.save(&built_path).unwrap();
    let rebuilt = Rom::load(&built_path).unwrap();
    let report = import::import_rom(&rebuilt, &clean, &again, false).unwrap();
    assert!(report.levels.is_empty());
    // What the build changed is all level data and tables the import
    // reads, so nothing is left unaccounted for.
    assert_eq!(report.unmodelled, []);
    assert_eq!(report.unread_blocks, []);

    // Level kinds: horizontal with a background, vertical, layer 2
    // objects (horizontal and vertical), a boss arena, the title screen.
    for number in [0x105, 0x0D3, 0x01C, 0x0CB, 0x0E5, 0x1C7, 0x0C7] {
        for options in [
            RenderOptions::default(),
            RenderOptions {
                sprites: render::Sprites::Markers,
                player: false,
            },
        ] {
            let a = render::render_level(&clean, number, options).unwrap().image;
            let b = render::render_level(&built, number, options).unwrap().image;
            assert!(
                a.pixels == b.pixels,
                "level {number:03X} renders differently"
            );
        }
    }
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&again);
    let _ = fs::remove_file(&built_path);
}

#[test]
fn an_empty_project_builds_the_clean_rom() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let project = Project {
        root: std::path::PathBuf::from("."),
        manifest: Default::default(),
        levels: Vec::new(),
        map16: Vec::new(),
        map16_bg: Vec::new(),
    };
    assert_eq!(build::build(&clean, &project).unwrap().data(), clean.data());
}

#[test]
fn edits_reach_the_rom() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let (mut level, _) = import::read_level(&clean, 0x105).unwrap();
    level.layer1.truncate(10);
    level.sprites.list[0].x += 1;
    let text = level.to_toml(&Comments::default());
    let (level, _) = Level::from_toml(&text).unwrap();
    let project = Project {
        root: std::path::PathBuf::from("."),
        manifest: Default::default(),
        levels: vec![(0x105, level.clone())],
        map16: Vec::new(),
        map16_bg: Vec::new(),
    };
    let built = build::build(&clean, &project).unwrap();
    assert_eq!(import::read_level(&built, 0x105).unwrap().0, level);
    // The changed sprite list went to bank $07's unused space; the other
    // levels kept theirs.
    assert_eq!(
        level::sprite_ptr(&built, 0x105).unwrap(),
        SnesAddr::new(0x07A179)
    );
    assert_eq!(
        built.read_u16(tables::SPRITE_PTRS.add(2 * 0x106)).unwrap(),
        clean.read_u16(tables::SPRITE_PTRS.add(2 * 0x106)).unwrap()
    );
    assert_eq!(
        level::layer1_ptr(&built, 0x106).unwrap(),
        level::layer1_ptr(&clean, 0x106).unwrap()
    );
    assert!(built.read(SnesAddr::new(0x108000), 4).unwrap() == b"STAR");
}

#[test]
fn lunar_magic_objects_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    use kobo_core::level::objects::Object;
    let (mut level, _) = import::read_level(&clean, 0x105).unwrap();
    // A 2x1 of tile $0AB, and a 2x2 block of Map16 from $1A0.
    level.layer1.push(Object::Lunar {
        number: 0x22,
        x: 1,
        y: 1,
        data: vec![0x01, 0xAB],
    });
    level.layer1.push(Object::Lunar {
        number: 0x27,
        x: 4,
        y: 1,
        data: vec![0x11, 0x41, 0xA0],
    });
    let project = Project {
        root: std::path::PathBuf::from("."),
        manifest: Default::default(),
        levels: vec![(0x105, level.clone())],
        map16: Vec::new(),
        map16_bg: Vec::new(),
    };
    let built = build::build(&clean, &project).unwrap();
    assert_eq!(import::read_level(&built, 0x105).unwrap().0, level);
    let tiles = kobo_core::expand::expand_level(&built, 0x105)
        .unwrap()
        .tiles;
    let at = |x, y| tiles.tile_at(x, y);
    assert_eq!([at(1, 1), at(2, 1)], [0x0AB, 0x0AB]);
    assert_eq!(
        [at(4, 1), at(5, 1), at(4, 2), at(5, 2)],
        [0x1A0, 0x1A1, 0x1B0, 0x1B1]
    );

    // The music bypass, song $0A.
    let mut with_music = level.clone();
    with_music
        .layer1
        .push(Object::Unplaced(vec![0x40, 0x60, 0x0B]));
    let project = Project {
        levels: vec![(0x105, with_music)],
        ..project
    };
    let built = build::build(&clean, &project).unwrap();
    let ram = kobo_core::expand::expand_level(&built, 0x105).unwrap().ram;
    assert_eq!(ram.u8(kobo_core::ram::RamAddr::new(0x7E_0DDA)), 0x0A);

    // Its graphics and time limit bypasses are not in yet.
    level.layer1.push(Object::Unplaced(vec![0x40, 0x80, 0x00]));
    let project = Project {
        levels: vec![(0x105, level)],
        ..project
    };
    let error = build::build(&clean, &project).unwrap_err().to_string();
    assert!(error.contains("Lunar Magic"), "{error}");
}

#[test]
fn lunar_magic_backgrounds_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    use kobo_core::source::level::{BACKGROUND_ROWS, BackgroundTiles};
    let (level, _) = import::read_level(&clean, 0x105).unwrap();
    // Tiles from the game's BG Map16, made up from their place.
    let tile = |i: usize| ((i * 7) % 0x200) as u16;
    for rows in [32, 27] {
        let high = |i: usize| {
            if rows == 27 {
                0x100 | tile(i) & 0xFF
            } else {
                tile(i)
            }
        };
        let tiles: Vec<u16> = (0..BACKGROUND_ROWS * 32)
            .map(|i| if i / 32 < rows { high(i) } else { 0 })
            .collect();
        let mut level = level.clone();
        level.layer2 = Layer2::Background(BackgroundTiles {
            table: 0,
            rows,
            tiles: tiles.clone(),
        });
        let project = Project {
            root: std::path::PathBuf::from("."),
            manifest: Default::default(),
            levels: vec![(0x105, level.clone())],
            map16: Vec::new(),
            map16_bg: Vec::new(),
        };
        let built = build::build(&clean, &project).unwrap();
        assert_eq!(
            import::read_level(&built, 0x105).unwrap().0,
            level,
            "{rows} rows"
        );
        let loaded = kobo_core::expand::expand_level(&built, 0x105)
            .unwrap()
            .tiles;
        let (low, high_plane) = loaded.layer2_tilemap.clone().unwrap();
        let screen = loaded.layer2_screen_len;
        assert_eq!(screen, if rows == 32 { 0x200 } else { 0x1B0 });
        for row in 0..rows {
            for half in 0..2 {
                for col in 0..16 {
                    let at = half * screen + row * 16 + col;
                    let got = u16::from_le_bytes([low[at], high_plane[at]]);
                    assert_eq!(
                        got,
                        tiles[row * 32 + half * 16 + col],
                        "{rows} rows: {row},{half},{col}"
                    );
                }
            }
        }
    }
}

#[test]
fn cached_builds_equal_clean_ones() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let cache_dir = temp_dir("cache");
    let cache = build::Cache::new(&cache_dir);
    let (level, _) = import::read_level(&clean, 0x105).unwrap();
    let mut project = Project {
        root: std::path::PathBuf::from("."),
        manifest: Default::default(),
        levels: vec![(0x105, level)],
        map16: Vec::new(),
        map16_bg: Vec::new(),
    };
    let uncached = build::build(&clean, &project).unwrap();
    let cold = build::build_cached(&clean, &project, Some(&cache)).unwrap();
    let warm = build::build_cached(&clean, &project, Some(&cache)).unwrap();
    assert_eq!(cold.data(), uncached.data());
    assert_eq!(warm.data(), uncached.data());
    assert_eq!(
        fs::read_dir(&cache_dir).unwrap().count(),
        build::Stage::ALL.len()
    );

    // An edit reruns the level stage from the cached base.
    project.levels[0].1.layer1.pop();
    let edited = build::build_cached(&clean, &project, Some(&cache)).unwrap();
    assert_eq!(
        edited.data(),
        build::build(&clean, &project).unwrap().data()
    );
    assert_eq!(
        fs::read_dir(&cache_dir).unwrap().count(),
        build::Stage::ALL.len() + 1
    );
    let _ = fs::remove_dir_all(&cache_dir);
}

/// A build needs no ROM data to check that its output is the same on every
/// platform: this one runs on a synthetic base in CI.
#[test]
fn a_synthetic_build_is_the_same_everywhere() {
    let base = common::synthetic_base();
    let text = include_str!("fixtures/synthetic_level.toml");
    let (level, comments) = Level::from_toml(text).unwrap();
    assert_eq!(level.to_toml(&comments), text);
    let project = Project {
        root: std::path::PathBuf::from("."),
        manifest: Default::default(),
        levels: vec![(0x105, level.clone()), (0x0C7, level)],
        map16: Vec::new(),
        map16_bg: Vec::new(),
    };
    let built = build::build_on(&base, &project, None).unwrap();
    assert_eq!(
        import::read_level(&built, 0x105).unwrap().0,
        project.levels[0].1
    );
    assert_eq!(
        built.sha1_hex(),
        "5f308245ae892ca5e8b2778540c220a03cf6b847",
        "the synthetic build's output changed"
    );
    // Distributed as a patch (`kobo build --bps`), it gives the build back.
    let patch = bps::create(base.data(), built.data());
    assert_eq!(bps::apply(&patch, base.data()).unwrap(), built.data());
}
