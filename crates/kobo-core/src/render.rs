//! Drawing tiles into images.
//!
//! [`LayerTiles`] models the 8x8 tiles a level's layers can reference,
//! laid out as the game uploads them to VRAM: FG1, FG2, BG1, FG3 at tiles
//! `0x000`, `0x080`, `0x100`, `0x180`. Decoding captured VRAM also
//! includes animated tiles and other dynamic uploads.

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
    draw_tile8_prio(img, None, x, y, tile, row, flip_x, flip_y);
}

/// Per-pixel stacking order of what the level image holds, in Mode 1
/// terms: layer 2 low priority (5), layer 1 low (6), layer 2 high (8),
/// layer 1 high (9). Objects with priority 0-3 slot in at 2, 4, 7, and 10.
pub type Priorities = Vec<u8>;
pub const LAYER2_LOW: u8 = 5;
pub const LAYER1_LOW: u8 = 6;
pub const LAYER2_HIGH: u8 = 8;
pub const LAYER1_HIGH: u8 = 9;
pub const OBJECT_PRIORITIES: [u8; 4] = [2, 4, 7, 10];

/// `draw_tile8`, also recording `value` in the priority buffer for every
/// pixel drawn.
#[allow(clippy::too_many_arguments)]
fn draw_tile8_prio(
    img: &mut RgbImage,
    prio: Option<(&mut [u8], u8)>,
    x: u32,
    y: u32,
    tile: &Tile8,
    row: &[[u8; 3]; 16],
    flip_x: bool,
    flip_y: bool,
) {
    let mut prio = prio;
    for (ty, line) in tile.pixels.iter().enumerate() {
        for (tx, &px) in line.iter().enumerate() {
            if px == 0 {
                continue;
            }
            let dx = if flip_x { 7 - tx } else { tx } as u32;
            let dy = if flip_y { 7 - ty } else { ty } as u32;
            let (px_x, px_y) = (x + dx, y + dy);
            if px_x < img.width && px_y < img.height {
                let at = (px_y * img.width + px_x) as usize;
                img.pixels[at] = row[px as usize];
                if let Some((buf, value)) = prio.as_mut() {
                    buf[at] = *value;
                }
            }
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

/// One drawing pass over a layer: which priority bit it draws, and the
/// palette bits the game's upload routine ORs into that layer's tilemap
/// words.
#[derive(Clone, Copy)]
struct LayerPass {
    priority: bool,
    palette_mask: u8,
}

/// Draws the quadrants of a 16x16 tile that belong to `pass`, recording
/// `prio` for their pixels.
#[allow(clippy::too_many_arguments)]
fn draw_map16_layer(
    img: &mut RgbImage,
    priorities: &mut [u8],
    prio: u8,
    x: u32,
    y: u32,
    tile: &Map16Tile,
    tiles: &LayerTiles,
    palette: &Palette,
    pass: LayerPass,
) {
    for qy in 0..2 {
        for qx in 0..2 {
            let r = tile.quadrant(qx, qy);
            if r.priority() != pass.priority {
                continue;
            }
            let row = palette.row_rgb8((r.palette() | pass.palette_mask) as usize & 7);
            let (px, py) = (x + 8 * qx as u32, y + 8 * qy as u32);
            draw_tile8_prio(
                img,
                Some((priorities, prio)),
                px,
                py,
                tiles.get(r.tile()),
                &row,
                r.flip_x(),
                r.flip_y(),
            );
        }
    }
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

/// Renders a level's layer 1 tile grid and its layer 2 (background
/// tilemap or objects) over the back area colour. Layers interleave the
/// way Mode 1 stacks them: layer 2 low priority, layer 1 low priority,
/// layer 2 high priority, layer 1 high priority.
pub fn level_image(
    tiles: &crate::expand::LevelTiles,
    layer_tiles: &LayerTiles,
    palette: &Palette,
    background: [u8; 3],
) -> RgbImage {
    level_render(tiles, layer_tiles, palette, background).0
}

/// `level_image` plus the stacking priority of every pixel, for drawing
/// sprites into the image afterwards. Boss arenas return no priorities:
/// their objects are already part of the image.
pub fn level_render(
    tiles: &crate::expand::LevelTiles,
    layer_tiles: &LayerTiles,
    palette: &Palette,
    background: [u8; 3],
) -> (RgbImage, Priorities) {
    if let Some(scene) = &tiles.boss_scene {
        return (
            boss_image(scene, &tiles.vram, palette, background),
            Vec::new(),
        );
    }
    let map16 = &tiles.map16;
    let (w, h) = tiles.size();
    let mut img = RgbImage::new(w as u32 * 16, h as u32 * 16);
    img.pixels.fill(background);
    let mut priorities = vec![0u8; img.pixels.len()];
    for priority in [false, true] {
        draw_layer2(
            &mut img,
            &mut priorities,
            tiles,
            layer_tiles,
            palette,
            priority,
        );
        let prio = if priority { LAYER1_HIGH } else { LAYER1_LOW };
        for y in 0..h {
            for x in 0..w {
                if let Some(tile) = map16.get(&tiles.tile_at(x, y)) {
                    let (px, py) = ((x * 16) as u32, (y * 16) as u32);
                    let pass = LayerPass {
                        priority,
                        palette_mask: 0,
                    };
                    draw_map16_layer(
                        &mut img,
                        &mut priorities,
                        prio,
                        px,
                        py,
                        tile,
                        layer_tiles,
                        palette,
                        pass,
                    );
                }
            }
        }
    }
    (img, priorities)
}

/// Draws captured sprite objects into a level image, front to back,
/// honouring their OAM priority against the layers and each other.
pub fn draw_sprite_scene(
    img: &mut RgbImage,
    priorities: &[u8],
    scene: &crate::video::SpriteScene,
    vram: &[u8],
    palette: &Palette,
) {
    let colors = palette.colors.map(|color| color.to_rgb8());
    let sizes = crate::expand::object_sizes(scene.object_select);
    let mut covered = vec![false; img.pixels.len()];
    for object in &scene.objects {
        let (width, height) = sizes[object.large as usize];
        let priority = OBJECT_PRIORITIES[(object.attr >> 4 & 3) as usize];
        for dy in 0..height {
            for dx in 0..width {
                let (x, y) = (object.x + dx, object.y + dy);
                if x < 0 || y < 0 || x >= img.width as i32 || y >= img.height as i32 {
                    continue;
                }
                let at = (y as u32 * img.width + x as u32) as usize;
                if covered[at] {
                    continue;
                }
                let Some(color) = object_pixel(
                    vram,
                    scene.object_select,
                    object.tile,
                    object.attr,
                    dx,
                    dy,
                    width,
                    height,
                ) else {
                    continue;
                };
                covered[at] = true;
                if priority > priorities[at] {
                    img.pixels[at] =
                        colors[128 + (object.attr as usize >> 1 & 7) * 16 + color as usize];
                }
            }
        }
    }
}

/// Colour index (1-15) of a pixel of an OAM object, or `None` where it is
/// transparent. `dx`/`dy` are unflipped offsets within the object.
#[allow(clippy::too_many_arguments)]
fn object_pixel(
    vram: &[u8],
    object_select: u8,
    tile: u8,
    attr: u8,
    dx: i32,
    dy: i32,
    width: i32,
    height: i32,
) -> Option<u8> {
    let tx = if attr & 0x40 != 0 { width - 1 - dx } else { dx } as usize;
    let ty = if attr & 0x80 != 0 {
        height - 1 - dy
    } else {
        dy
    } as usize;
    let number = (((tile as usize & 0xF0) + ty / 8 * 16) & 0xF0) | ((tile as usize + tx / 8) & 15);
    let base = ((object_select as usize & 7) << 14)
        + if attr & 1 != 0 {
            (((object_select as usize >> 3) & 3) + 1) * 0x2000
        } else {
            0
        };
    let start = base + number * 32 + (ty % 8) * 2;
    let mut color = 0;
    for plane in 0..4 {
        color |=
            ((video_byte(vram, start + plane / 2 * 16 + plane % 2) >> (7 - tx % 8)) & 1) << plane;
    }
    (color != 0).then_some(color)
}

/// Draws the quadrants of layer 2 with the given priority: the
/// background tilemap repeated every two screens, or the layer 2 objects
/// from their own region of the tile grid.
///
/// A vertical level's background is the same two-screen-wide tilemap
/// (mode `$0A` keeps layer 2 horizontal: `$5B` bit 1 clear) spanning the
/// level's full 32-tile width, and the game scrolls it slowly so the
/// 27 rows cover the whole descent. A static render cannot reproduce
/// that parallax, so the background is tiled down the level instead.
fn draw_layer2(
    img: &mut RgbImage,
    priorities: &mut [u8],
    tiles: &crate::expand::LevelTiles,
    layer_tiles: &LayerTiles,
    palette: &Palette,
    priority: bool,
) {
    let (w, h) = tiles.size();
    let prio = if priority { LAYER2_HIGH } else { LAYER2_LOW };
    if tiles.layer2_tilemap.is_some() {
        // Boss arenas and dark rooms sharing their tilemap do not display
        // the decoded background buffer.
        if matches!(tiles.level_mode, 0x09 | 0x0B | 0x0F | 0x10) {
            return;
        }
        let rows = tiles.layer2_bg_rows().min(crate::expand::SCREEN_ROWS);
        let pass = LayerPass {
            priority,
            palette_mask: 0,
        };
        for y in 0..h {
            for x in 0..w {
                let (screen, col) = (
                    x / crate::expand::SCREEN_COLS,
                    x % crate::expand::SCREEN_COLS,
                );
                let n = tiles.layer2_bg_tile(screen, col, y % rows).unwrap();
                if let Some(tile) = tiles.bg_map16.get(n as usize - 0x200) {
                    let (px, py) = ((x * 16) as u32, (y * 16) as u32);
                    draw_map16_layer(
                        img,
                        priorities,
                        prio,
                        px,
                        py,
                        tile,
                        layer_tiles,
                        palette,
                        pass,
                    );
                }
            }
        }
        return;
    }
    if tiles.layer2_objects().is_none() {
        return;
    }
    let pass = LayerPass {
        priority,
        palette_mask: tiles.layer2_palette_mask(),
    };
    for y in 0..h {
        for x in 0..w {
            let Some(n) = tiles.layer2_object_tile(x, y) else {
                continue;
            };
            if let Some(tile) = tiles.map16.get(&n) {
                let (px, py) = ((x * 16) as u32, (y * 16) as u32);
                draw_map16_layer(
                    img,
                    priorities,
                    prio,
                    px,
                    py,
                    tile,
                    layer_tiles,
                    palette,
                    pass,
                );
            }
        }
    }
}

/// Fixed-screen arenas use the ROM's video-mode bands, not its collision
/// Map16 grid. In particular, Mode 7 interleaves tile numbers in the low
/// VRAM bytes and packed 8bpp pixels in the high bytes.
fn boss_image(
    scene: &crate::video::BossScene,
    vram: &[u8],
    palette: &Palette,
    background: [u8; 3],
) -> RgbImage {
    let mut img = RgbImage::new(256, 224);
    img.pixels.fill(background);
    let colors = palette.colors.map(|color| color.to_rgb8());
    let mut priorities = vec![0; img.pixels.len()];
    for y in 0..224 {
        let Some(band) = scene.bands.iter().rev().find(|b| b.start <= y) else {
            continue;
        };
        for x in 0..256 {
            let [left, right] = scene.backdrop_window[y];
            if x >= left as usize && x <= right as usize {
                continue;
            }
            img.pixels[y * 256 + x] = [0; 3];
            let color = match band.layer.mode & 7 {
                7 => mode7_pixel(vram, &band.layer.mode7, x, y + 1).map(|color| (color, 3)),
                1 => tilemap_pixel(vram, &band.layer, x, y + 1),
                _ => None,
            };
            if let Some((color, priority)) = color {
                img.pixels[y * 256 + x] = colors[color as usize];
                priorities[y * 256 + x] = priority;
            }
        }
    }
    draw_boss_objects(&mut img, &priorities, scene, vram, &colors);
    img
}

fn draw_boss_objects(
    img: &mut RgbImage,
    bg_priorities: &[u8],
    scene: &crate::video::BossScene,
    vram: &[u8],
    colors: &[[u8; 3]; 256],
) {
    let sizes = crate::expand::object_sizes(scene.object_select);
    let mut covered = vec![false; img.pixels.len()];
    for offset in 0..128 {
        let object = (scene.first_object + offset) % 128;
        let Some(bytes) = scene.oam.get(object * 4..object * 4 + 4) else {
            continue;
        };
        let high = scene.oam.get(512 + object / 4).copied().unwrap_or(0) >> (2 * (object % 4));
        let x = bytes[0] as i32 - if high & 1 != 0 { 256 } else { 0 };
        let y = bytes[1] as i32;
        let attr = bytes[3];
        let (width, height) = sizes[(high & 2 != 0) as usize];
        for dy in 0..height {
            let sy = ((y + dy) & 255) as usize;
            if sy >= 224 {
                continue;
            }
            let Some(band) = scene.bands.iter().rev().find(|b| b.start <= sy) else {
                continue;
            };
            let [left, right] = scene.backdrop_window[sy];
            for dx in 0..width {
                let sx = x + dx;
                if !(0..256).contains(&sx) || (sx >= left as i32 && sx <= right as i32) {
                    continue;
                }
                let at = sy * 256 + sx as usize;
                if covered[at] {
                    continue;
                }
                let Some(color) = object_pixel(
                    vram,
                    scene.object_select,
                    bytes[2],
                    attr,
                    dx,
                    dy,
                    width,
                    height,
                ) else {
                    continue;
                };
                covered[at] = true;
                let priority = if band.layer.mode & 7 == 7 {
                    [2, 4, 6, 7]
                } else {
                    OBJECT_PRIORITIES
                }[(attr >> 4 & 3) as usize];
                if priority > bg_priorities[at] {
                    img.pixels[at] = colors[128 + (attr as usize >> 1 & 7) * 16 + color as usize];
                }
            }
        }
    }
}

fn video_byte(vram: &[u8], address: usize) -> u8 {
    vram.get(address & 0xFFFF).copied().unwrap_or(0)
}

fn tilemap_pixel(
    vram: &[u8],
    layer: &crate::video::Layer1,
    x: usize,
    y: usize,
) -> Option<(u8, u8)> {
    let side = if layer.mode & 0x10 != 0 { 16 } else { 8 };
    let x = x + layer.scroll[0] as usize;
    let y = y + layer.scroll[1] as usize;
    let wide = (layer.tilemap as usize & 1) + 1;
    let tall = ((layer.tilemap as usize >> 1) & 1) + 1;
    let col = (x / side) % (32 * wide);
    let row = (y / side) % (32 * tall);
    let at = ((layer.tilemap as usize >> 2) << 11)
        + (row / 32 * wide + col / 32) * 0x800
        + (row % 32 * 32 + col % 32) * 2;
    let tile = Tile8Ref(u16::from_le_bytes([
        video_byte(vram, at),
        video_byte(vram, at + 1),
    ]));
    let px = if tile.flip_x() {
        side - 1 - x % side
    } else {
        x % side
    };
    let py = if tile.flip_y() {
        side - 1 - y % side
    } else {
        y % side
    };
    let number = (tile.tile() as usize + px / 8 + py / 8 * 16) & 0x3FF;
    let start = layer.character_base as usize + number * 32 + (py % 8) * 2;
    let mut color = 0;
    for plane in 0..4 {
        let bits = video_byte(vram, start + (plane / 2) * 16 + plane % 2);
        color |= ((bits >> (7 - px % 8)) & 1) << plane;
    }
    (color != 0).then_some((
        color + tile.palette() * 16,
        if tile.priority() { 9 } else { 6 },
    ))
}

fn mode7_pixel(vram: &[u8], mode: &crate::video::Mode7, x: usize, y: usize) -> Option<u8> {
    let signed13 = |n: u16| ((n << 3) as i16 >> 3) as i32;
    let center = mode.center.map(signed13);
    let scroll = mode.scroll.map(signed13);
    // The PPU clips the scroll-to-center differences to ten magnitude
    // bits while retaining the sign from bit 13.
    let delta: [i32; 2] = std::array::from_fn(|axis| {
        let n = scroll[axis] - center[axis];
        if n & 0x2000 != 0 {
            n | !0x3FF
        } else {
            n & 0x3FF
        }
    });
    let x = if mode.control & 1 != 0 { 255 - x } else { x } as i32;
    let y = if mode.control & 2 != 0 { 255 - y } else { y } as i32;
    let position: [i32; 2] = std::array::from_fn(|axis| {
        let a = mode.matrix[axis * 2] as i32;
        let b = mode.matrix[axis * 2 + 1] as i32;
        let row = [a * delta[0], b * delta[1], b * y]
            .map(|n| n & !63)
            .iter()
            .sum::<i32>();
        (row + a * x + center[axis] * 256) >> 8
    });
    let outside = position.iter().any(|&n| !(0..1024).contains(&n));
    let tile = if outside && mode.control & 0x80 != 0 {
        if mode.control & 0x40 == 0 {
            return None;
        }
        0
    } else {
        let col = (position[0] & 1023) as usize / 8;
        let row = (position[1] & 1023) as usize / 8;
        video_byte(vram, 2 * (row * 128 + col)) as usize
    };
    let pixel = tile * 64 + (position[1] & 7) as usize * 8 + (position[0] & 7) as usize;
    let color = video_byte(vram, pixel * 2 + 1);
    (color != 0).then_some(color)
}

/// VRAM byte offset of the layer 3 font (GFX28 at word `$4000`): tiles
/// `0`-`9` then `A`-`Z`, 2bpp.
const FONT_VRAM_OFFSET: usize = 0x8000;

/// Draws a hex digit or letter from the layer 3 font in VRAM.
pub fn draw_font_glyph(img: &mut RgbImage, x: u32, y: u32, vram: &[u8], glyph: char, rgb: [u8; 3]) {
    let index = match glyph {
        '0'..='9' => glyph as usize - '0' as usize,
        'A'..='Z' => glyph as usize - 'A' as usize + 10,
        _ => return,
    };
    let start = FONT_VRAM_OFFSET + index * 16;
    if start + 16 > vram.len() {
        return;
    }
    let tile = Tile8::decode(Bpp::Two, &vram[start..start + 16]);
    let row = [[0, 0, 0], rgb, rgb, rgb];
    let mut palette = [[0u8; 3]; 16];
    palette[..4].copy_from_slice(&row);
    draw_tile8(img, x, y, &tile, &palette, false, false);
}

/// Draws a sprite marker: a box outline with the sprite number inside.
pub fn draw_sprite_marker(img: &mut RgbImage, x: u32, y: u32, id: u8, vram: &[u8]) {
    let outline = [255, 255, 255];
    let fill = [0, 0, 0];
    for i in 0..16 {
        img.put(x + i, y, outline);
        img.put(x + i, y + 15, outline);
        img.put(x, y + i, outline);
        img.put(x + 15, y + i, outline);
    }
    for dy in 4..12 {
        for dx in 0..16 {
            img.put(x + dx, y + dy, fill);
        }
    }
    let text = format!("{id:02X}");
    let mut chars = text.chars();
    draw_font_glyph(img, x, y + 4, vram, chars.next().unwrap(), outline);
    draw_font_glyph(img, x + 8, y + 4, vram, chars.next().unwrap(), outline);
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

#[cfg(test)]
mod video_tests {
    use super::*;
    use crate::video::{Band, BossScene, Layer1, Mode7};

    #[test]
    fn mode7_interleaving_transform_and_overflow() {
        let mut vram = vec![0; 0x10000];
        let mut mode = Mode7 {
            matrix: [256, 0, 0, 256],
            ..Mode7::default()
        };
        vram[2 * (128 + 2)] = 3; // Tile at world (16, 8).
        vram[2 * (3 * 64 + 2 * 8 + 1) + 1] = 42;
        assert_eq!(mode7_pixel(&vram, &mode, 17, 10), Some(42));
        assert_eq!(mode7_pixel(&vram, &mode, 18, 10), None);
        mode.matrix = [0, 256, -256, 0];
        mode.center = [16, 16];
        mode.scroll = [16, 16];
        assert_eq!(mode7_pixel(&vram, &mode, 6, 1), Some(42));

        mode = Mode7 {
            matrix: [256, 0, 0, 256],
            scroll: [0x1FFF, 0],
            ..Mode7::default()
        };
        vram[2 * 127] = 4;
        vram[2 * (4 * 64 + 7) + 1] = 25;
        vram[2 * 7 + 1] = 9;
        assert_eq!(mode7_pixel(&vram, &mode, 0, 0), Some(25)); // Wrap -1 to 1023.
        mode.control = 0x80;
        assert_eq!(mode7_pixel(&vram, &mode, 0, 0), None);
        mode.control = 0xC0;
        assert_eq!(mode7_pixel(&vram, &mode, 0, 0), Some(9)); // Outside uses tile 0.
    }

    #[test]
    fn tilemap_uses_character_base_second_screen_and_flips() {
        let mut vram = vec![0; 0x10000];
        let layer = Layer1 {
            mode: 1,
            tilemap: 0x59,
            character_base: 0xE000,
            ..Layer1::default()
        };
        // Second horizontal screen, tile 2, palette 3, high priority, both flips.
        vram[0xB800..0xB802].copy_from_slice(&0xEC02u16.to_le_bytes());
        vram[0xE000 + 2 * 32 + 14] = 1;
        assert_eq!(tilemap_pixel(&vram, &layer, 256, 0), Some((49, 9)));
        assert_eq!(tilemap_pixel(&vram, &layer, 257, 0), None);
        assert_eq!(tilemap_pixel(&vram, &layer, 768, 0), Some((49, 9)));
    }

    #[test]
    fn objects_respect_oam_order_signed_x_background_priority_and_window() {
        let mut scene = BossScene {
            bands: vec![Band {
                start: 0,
                layer: Layer1 {
                    mode: 1,
                    ..Layer1::default()
                },
            }],
            backdrop_window: vec![[1, 0]; 224],
            oam: vec![0; 544],
            object_select: 0,
            first_object: 0,
        };
        for object in scene.oam[..512].as_chunks_mut::<4>().0 {
            object[1] = 240;
        }
        scene.oam[..8].copy_from_slice(&[255, 0, 0, 0, 0, 0, 1, 0x30]);
        scene.oam[512] = 1; // First sprite at x=-1, second at x=0.
        let mut vram = vec![0; 0x10000];
        vram[0] = 0xFF;
        vram[32 + 1] = 0xFF;
        let mut colors = [[0; 3]; 256];
        colors[129] = [255, 0, 0];
        colors[130] = [0, 255, 0];
        let mut priorities = vec![0; 256 * 224];
        priorities[1] = 6;
        scene.backdrop_window[0] = [2, 2];
        let mut img = RgbImage::new(256, 224);
        draw_boss_objects(&mut img, &priorities, &scene, &vram, &colors);
        assert_eq!(img.pixels[0], colors[129]);
        assert_eq!(img.pixels[1], [0; 3]); // First OAM sprite blocks the higher-priority second.
        assert_eq!(img.pixels[2], [0; 3]); // Window masks both sprites.
        assert_eq!(img.pixels[7], colors[130]); // Signed X clips the first sprite.
        scene.first_object = 1;
        draw_boss_objects(&mut img, &priorities, &scene, &vram, &colors);
        assert_eq!(img.pixels[1], colors[130]);
    }
}
