//! Map16 pages past 1 through a build: written in Lunar Magic's layout
//! with Kobo's code for it installed, read back by import as they were
//! written, and resolved by the ROM's own tilemap upload. Needs Asar's
//! library; the upload check needs the vanilla ROM too. That Lunar Magic
//! keeps a build's pages when it saves is checked by hand
//! (docs/testing.md).

mod common;

use std::path::PathBuf;

use kobo_core::build::{self, Project};
use kobo_core::map16::pages;
use kobo_core::map16::{Map16Tile, Tile8Ref};
use kobo_core::source::map16::{DEFAULT_ACTS, Map16Entry, Map16Page};
use kobo_core::{Rom, expand, import};

/// A tile made up from its number, to tell tiles apart.
fn entry(tile: u16) -> Map16Entry {
    let r = |i: u16| {
        Tile8Ref::new(
            tile.wrapping_add(i) & 0x3FF,
            (tile % 8) as u8,
            i == 3,
            i == 1,
            false,
        )
    };
    Map16Entry {
        gfx: Map16Tile {
            top_left: r(0),
            bottom_left: r(1),
            top_right: r(2),
            bottom_right: r(3),
        },
        acts: [0x025, 0x130, 0x12F, 0x200][tile as usize % 4],
    }
}

/// Pages in three groups, one past `$40`, with some tiles each.
fn project() -> Project {
    let mut map16 = Vec::new();
    for page in [0x02u8, 0x13, 0x45] {
        let first = page as u16 * 0x100;
        let mut tiles = Map16Page::default();
        for tile in [first, first + 1, first + 0x80, first + 0xFF] {
            tiles.tiles.insert(tile, entry(tile));
        }
        map16.push((page, tiles));
    }
    Project {
        root: PathBuf::from("."),
        manifest: Default::default(),
        levels: Vec::new(),
        map16,
    }
}

fn check_read_back(rom: &Rom, project: &Project) {
    let (read, notes) = import::read_map16(rom).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let listed: Vec<u8> = project.map16.iter().map(|(p, _)| *p).collect();
    let back: Vec<u8> = read.iter().map(|(p, _)| *p).collect();
    assert_eq!(
        back, listed,
        "the pages that read back are the ones written"
    );
    for ((_, written), (_, back)) in project.map16.iter().zip(&read) {
        for (&tile, entry) in &back.tiles {
            assert_eq!(*entry, written.tile(tile), "tile {tile:04X}");
        }
    }
    // A tile the page file does not list is empty.
    assert_eq!(pages::acts_like(rom, 0x0202).unwrap(), Some(DEFAULT_ACTS));
}

#[test]
fn pages_build_and_read_back() {
    if common::asar().is_none() {
        return;
    }
    let base = common::synthetic_base();
    let project = project();
    let built = build::build_on(&base, &project, None).unwrap();
    assert!(pages::installed(&built));
    check_read_back(&built, &project);
    assert_eq!(
        build::build_on(&base, &project, None).unwrap().data(),
        built.data(),
        "same inputs, same output"
    );
}

#[test]
fn the_tilemap_upload_finds_them() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let project = project();
    let built = build::build(&clean, &project).unwrap();
    check_read_back(&built, &project);
    let tiles: Vec<u16> = project
        .map16
        .iter()
        .flat_map(|(_, p)| p.tiles.keys().copied())
        .collect();
    let resolved = expand::resolve_map16(&built, 0x105, &tiles).unwrap();
    for (tile, got) in tiles.iter().zip(resolved) {
        assert_eq!(got, Some(entry(*tile).gfx), "tile {tile:04X}");
    }
    // The vanilla levels are untouched by the install.
    let options = kobo_core::render::RenderOptions::default();
    for level in [0x105, 0x0C7] {
        let a = kobo_core::render::render_level(&clean, level, options).unwrap();
        let b = kobo_core::render::render_level(&built, level, options).unwrap();
        assert!(a.image.pixels == b.image.pixels, "level {level:03X}");
    }
}
