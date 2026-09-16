//! Pixel-level regressions using synthetic tiles, plus an optional vanilla
//! ROM check for dimensions overwritten during boss preparation.

mod common;

use kobo_core::expand::{GRID_LEN, LAYER2_TILEMAP_LEN, LevelTiles, SCREEN_COLS, SCREEN_ROWS};
use kobo_core::gfx::Tile8;
use kobo_core::level::PrimaryHeader;
use kobo_core::map16::{BG_TILE_COUNT, Map16Tile, Tile8Ref};
use kobo_core::palette::{Color15, Palette};
use kobo_core::render::{self, LayerTiles};
use std::collections::HashMap;

fn solid_tile(index: u16) -> Map16Tile {
    let r = Tile8Ref(index);
    Map16Tile {
        top_left: r,
        bottom_left: r,
        top_right: r,
        bottom_right: r,
    }
}

fn scene() -> (LevelTiles, LayerTiles, Palette) {
    let tiles = LevelTiles {
        level: 0x105,
        header: PrimaryHeader::from_bytes([0; 5]),
        level_mode: 0,
        vertical: false,
        screens: 3,
        low: vec![0; GRID_LEN],
        high: vec![0; GRID_LEN],
        wram: vec![],
        map16: HashMap::from([(0, solid_tile(0)), (0x200, solid_tile(1))]),
        bg_map16: vec![solid_tile(2); BG_TILE_COUNT],
        vram: vec![],
        vram_written: vec![],
        cgram: vec![],
        bg_sc: [0; 4],
        boss_scene: None,
        layer2_tilemap: Some((vec![0; LAYER2_TILEMAP_LEN], vec![0; LAYER2_TILEMAP_LEN])),
        layer2_screen_len: SCREEN_COLS * SCREEN_ROWS,
    };
    let mut gfx = LayerTiles::blank();
    gfx.tiles[1] = Tile8 {
        pixels: [[1; 8]; 8],
    };
    gfx.tiles[2] = Tile8 {
        pixels: [[2; 8]; 8],
    };
    let mut palette = Palette::default();
    palette.set(0, 1, Color15::from_rgb5(31, 0, 0));
    palette.set(0, 2, Color15::from_rgb5(0, 0, 31));
    (tiles, gfx, palette)
}

#[test]
fn overlapping_map16_numbers_keep_foreground_and_background_art_separate() {
    let (mut tiles, mut gfx, palette) = scene();
    tiles.high[0] = 2; // Foreground tile $200 over background tile $200.
    gfx.tiles[1].pixels[0][0] = 0; // Transparent pixels reveal the background.
    let image = render::level_image(&tiles, &gfx, &palette, [0, 255, 0]);
    assert_eq!(image.pixels[0], [0, 0, 255]);
    assert_eq!(image.pixels[1], [255, 0, 0]);
    assert_eq!(image.pixels[16], [0, 0, 255]);
    assert_eq!(image.pixels[512], [0, 0, 255]); // Background repeats.
    assert_eq!((image.width, image.height), (768, 432));
}

#[test]
fn special_modes_do_not_draw_unused_background_buffers() {
    let (mut tiles, gfx, palette) = scene();
    let back = [0, 255, 0];
    for mode in [0x09, 0x0B, 0x0F, 0x10] {
        tiles.level_mode = mode;
        let image = render::level_image(&tiles, &gfx, &palette, back);
        assert!(image.pixels.iter().all(|&p| p == back), "mode {mode:02X}");
        // Suppressing the background must still leave foreground art visible.
        tiles.high[0] = 2;
        let image = render::level_image(&tiles, &gfx, &palette, back);
        assert_eq!(image.pixels[0], [255, 0, 0], "mode {mode:02X}");
        assert_eq!(image.pixels[16], back, "mode {mode:02X}");
        tiles.high[0] = 0;
    }
}

#[test]
fn tall_background_uses_its_own_screen_stride() {
    let (mut tiles, gfx, palette) = scene();
    tiles.layer2_screen_len = 0x200;
    tiles.bg_map16[1] = solid_tile(1);
    let (low, _) = tiles.layer2_tilemap.as_mut().unwrap();
    low[0x200] = 1;
    let image = render::level_image(&tiles, &gfx, &palette, [0, 255, 0]);
    assert_eq!(image.pixels[0], [0, 0, 255]);
    assert_eq!(image.pixels[256], [255, 0, 0]);
    assert_eq!(image.pixels[512], [0, 0, 255]);
}

#[test]
fn background_indices_above_511_do_not_alias_lower_tiles() {
    let (mut tiles, gfx, palette) = scene();
    tiles.bg_map16.resize(0x400, solid_tile(2));
    tiles.bg_map16[0x201] = solid_tile(1);
    let (lo, hi) = tiles.layer2_tilemap.as_mut().unwrap();
    lo[0] = 1;
    hi[0] = 2;
    assert_eq!(tiles.layer2_bg_tile(0, 0, 0), Some(0x401));
    let image = render::level_image(&tiles, &gfx, &palette, [0; 3]);
    assert_eq!(image.pixels[0], [255, 0, 0]);
    assert_eq!(image.pixels[16], [0, 0, 255]);
}

#[test]
fn oversized_vertical_level_renders_only_the_captured_grid() {
    let (mut tiles, gfx, palette) = scene();
    tiles.vertical = true;
    tiles.screens = 32; // 32 * $200 exceeds the $3800-byte grid planes.
    let last = GRID_LEN - 1;
    tiles.high[last] = 2;
    let back = [0, 255, 0];
    let image = render::level_image(&tiles, &gfx, &palette, back);
    assert_eq!((image.width, image.height), (512, 7168));
    assert_eq!(image.pixels[0], back);
    assert_eq!(image.pixels.last(), Some(&[255, 0, 0]));
    assert_eq!(tiles.screens, 32); // Retain the loader's value for inspection.
}

#[test]
fn boss_preparation_does_not_replace_the_level_dimensions() {
    let Some(rom) = common::vanilla() else { return };
    let tiles = kobo_core::expand::expand_level(&rom, 0x1C7).unwrap();
    assert_eq!(tiles.screens, 1);
    assert_eq!(tiles.size(), (16, 27));
    // The raw dump still includes the boss routine's overwritten byte.
    assert_eq!(tiles.wram[0x5D], 0xFF);
    let image = render::level_image(
        &tiles,
        &LayerTiles::from_vram(&tiles.vram),
        &tiles.palette(),
        tiles.back_area_color().to_rgb8(),
    );
    assert_eq!((image.width, image.height), (256, 224));
    assert_eq!(image.pixels[0], [0; 3]);
    assert!(image.pixels[192 * 256..].iter().any(|&p| p != [0; 3]));
}

#[test]
fn grand_poo_world_background_validation() {
    for (_, rom) in common::lunar_magic_roms() {
        if rom.sha1_hex() != "390583d5faa0cc02e0c4f414f7638228661b2dc9" {
            continue;
        }
        assert!(matches!(
            kobo_core::expand::expand_level(&rom, 0x09F),
            Err(kobo_core::expand::ExpandError::MissingBackgroundTable(
                0x09F
            ))
        ));
        let objects = kobo_core::expand::expand_level(&rom, 0x00E).unwrap();
        assert!(objects.layer2_tilemap.is_none());
        let background = kobo_core::expand::expand_level(&rom, 0x046).unwrap();
        assert_eq!(background.layer2_bg_rows(), 32);
        assert!(background.bg_map16.len() > 0x350);
    }
}

#[test]
fn boss_arenas_capture_mode_switches_and_object_art() {
    let Some(rom) = common::vanilla() else { return };
    for (level, starts) in [
        (0x096, vec![0, 36]),
        (0x0CC, vec![0, 45, 174]),
        (0x0D9, vec![0, 36, 174]),
        (0x1C7, vec![0]),
    ] {
        let tiles = kobo_core::expand::expand_level(&rom, level).unwrap();
        let scene = tiles.boss_scene.as_ref().unwrap();
        assert_eq!(
            scene.bands.iter().map(|b| b.start).collect::<Vec<_>>(),
            starts,
            "level {level:03X}"
        );
        assert!(scene.bands.iter().any(|b| b.layer.mode & 7 == 7));
        assert_eq!(scene.oam.len(), 544);
        assert!(
            scene.oam[..512]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|o| o[1] < 224)
        );
        assert_eq!(tiles.wram[0x13], 0); // Keep the frame counter from before the drawing pass.
    }
}
