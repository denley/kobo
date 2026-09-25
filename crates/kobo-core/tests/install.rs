//! Kobo's clean-room code for Lunar Magic's layout (`kobo_core::install`),
//! checked by running it: the ROM's own loader, Map16 routine, and block
//! interaction on the vanilla ROM with the patches applied. Needs Asar's
//! library and the vanilla ROM. That custom blocks run as under Lunar
//! Magic's code is checked by hand with `examples/contact_probe.rs`
//! (docs/testing.md).

mod common;

use kobo_core::map16::Map16Tile;
use kobo_core::rats::{Contents, FreeSpace};
use kobo_core::render::{self, RenderOptions, Sprites};
use kobo_core::{Rom, SnesAddr, expand, install, level};

fn installed(clean: &Rom) -> Option<Rom> {
    let asar = common::asar()?;
    let mut rom = Rom::from_bytes(clean.data().to_vec()).unwrap();
    rom.expand(0x10_0000).unwrap();
    Some(install::apply_lunar_magic(&asar, &rom).unwrap())
}

/// A definition made up from its tile number, to tell tiles apart.
fn definition(tile: u16) -> [u8; 8] {
    let [lo, hi] = tile.to_le_bytes();
    [lo, hi, 0x11, 0x22, lo ^ 0xFF, hi, 0x33, 0x44]
}

#[test]
fn pages_0_and_1_are_the_games() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(rom) = installed(&clean) else { return };
    let tiles: Vec<u16> = (0..0x200).collect();
    for level in [0x105, 0x0D3, 0x0C7] {
        assert_eq!(
            expand::resolve_map16(&rom, level, &tiles).unwrap(),
            expand::resolve_map16(&clean, level, &tiles).unwrap(),
            "level {level:03X}"
        );
    }
}

#[test]
fn vanilla_levels_draw_the_same() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(rom) = installed(&clean) else { return };
    // The row and column uploads of both layers, horizontal and vertical,
    // a background, and layer 2 objects, all through the routine.
    for level in [0x105, 0x0D3, 0x01C, 0x0CB, 0x0E5, 0x0C7] {
        let options = RenderOptions {
            sprites: Sprites::Markers,
            player: false,
        };
        let a = render::render_level(&clean, level, options).unwrap().image;
        let b = render::render_level(&rom, level, options).unwrap().image;
        assert!(a.pixels == b.pixels, "level {level:03X}");
    }
    // Sprites that change tiles while they run, through the tile change
    // and tile generation code; boss arenas, whose floors the player samples
    // with high bytes past 1, which the acts-like chain must keep solid.
    for level in [0x006, 0x0C3, 0x095, 0x1C7] {
        let options = RenderOptions::default();
        let a = render::render_level(&clean, level, options).unwrap().image;
        let b = render::render_level(&rom, level, options).unwrap().image;
        assert!(a.pixels == b.pixels, "level {level:03X} with sprites");
    }
}

#[test]
fn pages_past_1_come_from_the_page_tables() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(mut rom) = installed(&clean) else {
        return;
    };
    let mut space = FreeSpace::scan(&rom);
    // Each group's table pointer and bank, where Lunar Magic's layout has
    // them, and whether the pointer is kept less one.
    let groups: [(u16, u32, u32, bool); 5] = [
        (0x0200, 0x06F553, 0x06F557, false),
        (0x1000, 0x06F55C, 0x06F560, false),
        (0x2000, 0x06F567, 0x06F56B, true),
        (0x4000, 0x06F594, 0x06F598, false),
        (0x7000, 0x06F5B1, 0x06F5B5, true),
    ];
    let mut expected = Vec::new();
    for (first, pointer, bank, less_one) in groups {
        let tiles = [first, first + 1, first + 0x1FF];
        let len = (tiles[2] - first + 1) as usize * 8;
        let table = space.alloc(&mut rom, len, Contents::Data).unwrap();
        for &tile in &tiles {
            let at = table.add((tile - first) as u32 * 8);
            rom.write(at, &definition(tile)).unwrap();
            expected.push((tile, definition(tile)));
        }
        let stored = table.offset().wrapping_sub(less_one as u16);
        rom.write_u16(SnesAddr::new(pointer), stored).unwrap();
        rom.write_u8(SnesAddr::new(bank), table.bank()).unwrap();
    }
    let tiles: Vec<u16> = expected.iter().map(|(t, _)| *t).collect();
    let resolved = expand::resolve_map16(&rom, 0x105, &tiles).unwrap();
    for ((tile, bytes), got) in expected.iter().zip(resolved) {
        assert_eq!(got, Some(Map16Tile::from_bytes(*bytes)), "tile {tile:04X}");
    }

    // Page 2 per object tileset: the table at the pointer plus $1000,
    // $800 bytes a tileset.
    let tileset = level::read_primary_header(&rom, 0x105)
        .unwrap()
        .object_tileset as usize;
    let table = space
        .alloc(&mut rom, 0x1000 + 0x800 * (tileset + 1), Contents::Data)
        .unwrap();
    let tile = 0x2A5;
    let at = table.add((0x1000 + 0x800 * tileset + 0xA5 * 8) as u32);
    rom.write(at, &definition(0x1234)).unwrap();
    rom.write_u16(SnesAddr::new(0x06F586), table.offset())
        .unwrap();
    rom.write_u8(SnesAddr::new(0x06F58A), table.bank()).unwrap();
    rom.write_u8(SnesAddr::new(0x06F547), 1).unwrap();
    let resolved = expand::resolve_map16(&rom, 0x105, &[tile, 0x3FF]).unwrap();
    assert_eq!(resolved[0], Some(Map16Tile::from_bytes(definition(0x1234))));
    // Page 3 still comes from the group's table.
    assert_eq!(resolved[1], Some(Map16Tile::from_bytes(definition(0x3FF))));
}
