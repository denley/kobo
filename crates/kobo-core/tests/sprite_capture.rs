//! Sprite graphics come from running the ROM's own sprite loader and
//! engine, so the checks here are about the capture behaving: every sprite
//! entry either draws something or is one of the game's invisible sprites,
//! objects land near their entries, and a spread of levels captures
//! without the CPU tripping.

mod common;

use kobo_core::{expand, sprites};

/// Sprites that draw nothing on their first frame: invisible warp blocks,
/// invisible mushrooms, and the level-end "Yoshi egg in the ground" wing.
const INVISIBLE: [u8; 3] = [0x8E, 0xC7, 0xDB];

#[test]
fn yoshis_island_1_sprites_all_draw_near_their_entries() {
    let Some(rom) = common::vanilla() else { return };
    let tiles = expand::expand_level(&rom, 0x105).unwrap();
    let list = sprites::read_sprites_at(&rom, tiles.sprite_data_ptr()).unwrap();
    let scene = expand::capture_sprites(&rom, &tiles, &list).unwrap();
    assert!(
        scene.objects.len() >= 100,
        "{} objects",
        scene.objects.len()
    );
    for (x, y, id) in &scene.undrawn {
        assert!(
            INVISIBLE.contains(id),
            "sprite {id:02X} at ({x}, {y}) drew nothing"
        );
    }
    // Every visible entry has an object within a 48-pixel box of it.
    for e in &list.sprites {
        if INVISIBLE.contains(&e.id) {
            continue;
        }
        let (x, y) = e.tile_position(false);
        let (x, y) = (x as i32 * 16, y as i32 * 16);
        assert!(
            scene
                .objects
                .iter()
                .any(|o| (o.x - x).abs() <= 48 && (o.y - y).abs() <= 48),
            "sprite {:02X} at ({x}, {y}) has no object nearby",
            e.id
        );
    }
}

#[test]
fn vertical_level_sprites_draw_where_their_entries_are() {
    let Some(rom) = common::vanilla() else { return };
    let tiles = expand::expand_level(&rom, 0x0DB).unwrap();
    let list = sprites::read_sprites_at(&rom, tiles.sprite_data_ptr()).unwrap();
    let scene = expand::capture_sprites(&rom, &tiles, &list).unwrap();
    assert!(scene.undrawn.is_empty(), "{:?}", scene.undrawn);
    for e in &list.sprites {
        let (x, y) = e.tile_position(true);
        let (x, y) = (x as i32 * 16, y as i32 * 16);
        assert!(
            scene
                .objects
                .iter()
                .any(|o| (o.x - x).abs() <= 48 && (o.y - y).abs() <= 48),
            "sprite {:02X} at ({x}, {y}) has no object nearby",
            e.id
        );
    }
}

#[test]
fn a_spread_of_vanilla_levels_captures_cleanly() {
    let Some(rom) = common::vanilla() else { return };
    let mut captured = 0;
    for level in (0..0x200u16).step_by(7) {
        let tiles = expand::expand_level(&rom, level).unwrap();
        let list = sprites::read_sprites_at(&rom, tiles.sprite_data_ptr()).unwrap();
        let scene = expand::capture_sprites(&rom, &tiles, &list)
            .unwrap_or_else(|e| panic!("level {level:03X}: {e}"));
        captured += scene.objects.len();
    }
    assert!(captured > 0);
}

/// The player is captured by his own drawing pass: standing at the
/// entrance in Yoshi's Island 1, shot out of the cannon pipe that level
/// 0D0 starts with, and left to the arena's own pass in boss rooms.
#[test]
fn player_is_drawn_at_the_entrance() {
    let Some(rom) = common::vanilla() else { return };
    let tiles = expand::expand_level(&rom, 0x105).unwrap();
    let player = |tiles: &expand::LevelTiles| {
        let at = |i: usize| i32::from(u16::from_le_bytes([tiles.wram[i], tiles.wram[i + 1]]));
        (at(0x94), at(0x96))
    };
    let (x, y) = player(&tiles);
    assert_eq!((x, y), (0x10, 0x160));
    assert!(!tiles.player.is_empty());
    for o in &tiles.player {
        assert!(
            (o.x - x).abs() <= 16 && (o.y - y).abs() <= 32,
            "object at ({}, {}) is not the player at ({x}, {y})",
            o.x,
            o.y
        );
    }
    // Mario's palette (row 8, colours 6-F) is uploaded with his tiles.
    assert_ne!(tiles.palette().get(8, 8).0, 0);

    // The cannon pipe fires him up and to the right of where he entered,
    // and the pass follows him until the launch ends.
    let tiles = expand::expand_level(&rom, 0x0D0).unwrap();
    assert_eq!(tiles.wram[0x71], 7, "cannon pipe entrance");
    let (x, y) = player(&tiles);
    let (w, h) = tiles.size();
    assert!(!tiles.player.is_empty());
    for o in &tiles.player {
        assert!(
            o.x > x && o.y < y && o.x < w as i32 * 16 && o.y >= 0 && o.y < h as i32 * 16,
            "object at ({}, {}) after launching from ({x}, {y})",
            o.x,
            o.y
        );
    }

    let tiles = expand::expand_level(&rom, 0x1C7).unwrap();
    assert!(tiles.player.is_empty());
}
