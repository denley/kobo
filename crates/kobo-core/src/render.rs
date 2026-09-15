//! Drawing tiles into images.
//!
//! [`LayerTiles`] models the 8x8 tiles a level's layers can reference,
//! laid out as the game uploads them to VRAM: FG1, FG2, BG1, FG3 at tiles
//! `0x000`, `0x080`, `0x100`, `0x180`. Tiles `0x200` and up (animated
//! tiles and other dynamic uploads) are blank for now.

use crate::gfx::{self, Bpp, GfxError, Tile8};
use crate::image::RgbImage;
use crate::map16::{Map16Table, Map16Tile, Tile8Ref};
use crate::palette::Palette;
use crate::rom::Rom;

/// Number of 8x8 tiles addressable by a tilemap word.
pub const LAYER_TILE_COUNT: usize = 0x400;

/// VRAM tile index each layer GFX slot starts at.
pub const LAYER_SLOT_BASE: [usize; 4] = [0x000, 0x080, 0x100, 0x180];

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LayerTiles {
    pub tiles: Vec<Tile8>,
}

impl LayerTiles {
    pub fn blank() -> Self {
        Self {
            tiles: vec![Tile8::default(); LAYER_TILE_COUNT],
        }
    }

    /// Loads the four layer GFX files of an object tileset, applying the
    /// game's fourth-plane rule as it would on upload.
    pub fn for_object_tileset(rom: &Rom, tileset: u8) -> Result<Self, GfxError> {
        let files = gfx::object_tileset_files(rom, tileset)?;
        let mut out = Self::blank();
        for (slot, &file) in files.iter().enumerate() {
            let gfx = gfx::read_gfx_file(rom, file)?;
            let mut tiles = gfx.tiles();
            if gfx.bpp() == Some(Bpp::Three) {
                for t in gfx::vram_upper_palette_tiles(file, tileset, tiles.len()) {
                    for px in tiles[t].pixels.iter_mut().flatten() {
                        if *px != 0 {
                            *px |= 8;
                        }
                    }
                }
            }
            let base = LAYER_SLOT_BASE[slot];
            for (i, tile) in tiles.into_iter().enumerate().take(0x80) {
                out.tiles[base + i] = tile;
            }
        }
        Ok(out)
    }

    /// Decodes the layer tile area of VRAM (4bpp tiles from byte 0) as
    /// the game uploaded it, including animated tiles and ExGFX.
    pub fn from_vram(vram: &[u8]) -> Self {
        let mut out = Self::blank();
        for (i, tile) in out.tiles.iter_mut().enumerate() {
            let start = i * 32;
            if start + 32 <= vram.len() {
                *tile = Tile8::decode(Bpp::Four, &vram[start..start + 32]);
            }
        }
        out
    }

    pub fn get(&self, index: u16) -> &Tile8 {
        &self.tiles[index as usize % LAYER_TILE_COUNT]
    }
}

/// Draws one 8x8 tile. Colour 0 is transparent.
pub fn draw_tile8(
    img: &mut RgbImage,
    x: u32,
    y: u32,
    tile: &Tile8,
    row: &[[u8; 3]; 16],
    flip_x: bool,
    flip_y: bool,
) {
    for (ty, line) in tile.pixels.iter().enumerate() {
        for (tx, &px) in line.iter().enumerate() {
            if px == 0 {
                continue;
            }
            let dx = if flip_x { 7 - tx } else { tx } as u32;
            let dy = if flip_y { 7 - ty } else { ty } as u32;
            img.put(x + dx, y + dy, row[px as usize]);
        }
    }
}

/// Draws one 8x8 tile reference using the palette row it selects.
pub fn draw_tile_ref(
    img: &mut RgbImage,
    x: u32,
    y: u32,
    r: Tile8Ref,
    tiles: &LayerTiles,
    palette: &Palette,
) {
    let row = palette.row_rgb8(r.palette() as usize);
    draw_tile8(img, x, y, tiles.get(r.tile()), &row, r.flip_x(), r.flip_y());
}

/// Draws a 16x16 tile at pixel position (`x`, `y`).
pub fn draw_map16_tile(
    img: &mut RgbImage,
    x: u32,
    y: u32,
    tile: &Map16Tile,
    tiles: &LayerTiles,
    palette: &Palette,
) {
    for qy in 0..2 {
        for qx in 0..2 {
            let r = tile.quadrant(qx, qy);
            draw_tile_ref(img, x + 8 * qx as u32, y + 8 * qy as u32, r, tiles, palette);
        }
    }
}

/// Renders every tile of a Map16 table, `columns` per row, over a solid
/// background colour.
pub fn map16_sheet(
    table: &Map16Table,
    tiles: &LayerTiles,
    palette: &Palette,
    background: [u8; 3],
    columns: u32,
) -> RgbImage {
    let columns = columns.max(1);
    let rows = (table.tiles.len() as u32).div_ceil(columns);
    let mut img = RgbImage::new(columns * 16, rows * 16);
    img.pixels.fill(background);
    for (i, tile) in table.tiles.iter().enumerate() {
        let x = (i as u32 % columns) * 16;
        let y = (i as u32 / columns) * 16;
        draw_map16_tile(&mut img, x, y, tile, tiles, palette);
    }
    img
}

/// Renders a level's layer 1 tile grid (and layer 2 background tilemap,
/// if any) over the back area colour.
pub fn level_image(
    tiles: &crate::expand::LevelTiles,
    layer_tiles: &LayerTiles,
    palette: &Palette,
    background: [u8; 3],
) -> RgbImage {
    let map16 = &tiles.map16;
    let (w, h) = tiles.size();
    let mut img = RgbImage::new(w as u32 * 16, h as u32 * 16);
    img.pixels.fill(background);
    // Layer 2 background tilemap, repeated every two screens.
    if tiles.layer2_tilemap.is_some() && !tiles.vertical {
        for screen in 0..tiles.screens {
            for y in 0..crate::expand::SCREEN_ROWS {
                for x in 0..crate::expand::SCREEN_COLS {
                    let n = tiles.layer2_bg_tile(screen, x, y).unwrap();
                    if let Some(tile) = map16.get(&n) {
                        let px = ((screen * crate::expand::SCREEN_COLS + x) * 16) as u32;
                        let py = (y * 16) as u32;
                        draw_map16_tile(&mut img, px, py, tile, layer_tiles, palette);
                    }
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let n = tiles.tile_at(x, y);
            if let Some(tile) = map16.get(&n) {
                draw_map16_tile(
                    &mut img,
                    (x * 16) as u32,
                    (y * 16) as u32,
                    tile,
                    layer_tiles,
                    palette,
                );
            }
        }
    }
    img
}

/// Renders a palette as a 16x16 grid of `cell`-pixel swatches.
pub fn palette_swatch(palette: &Palette, cell: u32) -> RgbImage {
    let mut img = RgbImage::new(16 * cell, 16 * cell);
    for row in 0..16 {
        for col in 0..16 {
            let rgb = palette.get(row, col).to_rgb8();
            for dy in 0..cell {
                for dx in 0..cell {
                    img.put(col as u32 * cell + dx, row as u32 * cell + dy, rgb);
                }
            }
        }
    }
    img
}
