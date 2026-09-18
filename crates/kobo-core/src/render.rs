//! Drawing tiles into images.
//!
//! [`LayerTiles`] models the 8x8 tiles a level's layers can reference,
//! laid out as the game uploads them to VRAM: FG1, FG2, BG1, FG3 at tiles
//! `0x000`, `0x080`, `0x100`, `0x180`. Decoding captured VRAM also
//! includes animated tiles and other dynamic uploads.

use crate::gfx::{self, Bpp, GfxError, Tile8};
use crate::image::RgbImage;
use crate::map16::{Map16Table, Map16Tile, Tile8Ref};
use crate::palette::{Color15, Palette};
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

/// Stacking order of the layers in Mode 1 terms: layer 3 low priority
/// (1), layer 3 high (3), layer 2 low (5), layer 1 low (6), layer 2 high
/// (8), layer 1 high (9). Objects with priority 0-3 slot in at 2, 4, 7,
/// and 10. With the BG3 priority bit, high-priority layer 3 moves in
/// front of everything (11). Zero is transparent.
pub const LAYER3_LOW: u8 = 1;
pub const LAYER3_HIGH: u8 = 3;
pub const LAYER2_LOW: u8 = 5;
pub const LAYER1_LOW: u8 = 6;
pub const LAYER2_HIGH: u8 = 8;
pub const LAYER1_HIGH: u8 = 9;
pub const LAYER3_FRONT: u8 = 11;
pub const OBJECT_PRIORITIES: [u8; 4] = [2, 4, 7, 10];

/// One pixel of one layer: its CGRAM colour (0 is transparent, as on the
/// PPU) and its stacking priority.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct LayerPixel {
    pub color: u8,
    pub priority: u8,
}

/// Indices of the layers in [`LevelLayers`].
pub const BG1: usize = 0;
pub const BG2: usize = 1;
pub const BG3: usize = 2;
pub const OBJ: usize = 3;
/// Each layer's bit in `TM`, `TS`, and `CGADSUB`.
const LAYER_BITS: [u8; 4] = [0x01, 0x02, 0x04, 0x10];
/// The backdrop's bit in `CGADSUB`.
const BACKDROP_BIT: u8 = 0x20;
/// Objects using sprite palettes 0-3 never take part in colour math.
const FIRST_MATH_OBJECT_COLOR: u8 = 0xC0;

/// A level drawn layer by layer, before the PPU's screen designation and
/// colour math combine the layers into a picture. Layer 1, layer 2,
/// layer 3, and the objects each keep their own pixels, so the same
/// buffers serve as the main screen and the subscreen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LevelLayers {
    pub width: u32,
    pub height: u32,
    pub layers: [Vec<LayerPixel>; 4],
}

impl LevelLayers {
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width * height) as usize;
        Self {
            width,
            height,
            layers: std::array::from_fn(|_| vec![LayerPixel::default(); len]),
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32)
            .then(|| (y as u32 * self.width + x as u32) as usize)
    }

    /// Sets a background layer pixel unless a higher-priority one is there.
    fn put(&mut self, layer: usize, at: usize, color: u8, priority: u8) {
        let p = &mut self.layers[layer][at];
        if color != 0 && priority > p.priority {
            *p = LayerPixel { color, priority };
        }
    }

    /// Sets an object pixel: the first opaque object in OAM order wins,
    /// whatever its priority.
    fn put_object(&mut self, at: usize, color: u8, priority: u8) {
        let p = &mut self.layers[OBJ][at];
        if color != 0 && p.color == 0 {
            *p = LayerPixel { color, priority };
        }
    }

    /// The topmost opaque pixel among the layers enabled in `mask` (a `TM`
    /// or `TS` value), with its layer index.
    fn pick(&self, mask: u8, at: usize) -> Option<(usize, LayerPixel)> {
        let mut best: Option<(usize, LayerPixel)> = None;
        for (layer, bit) in LAYER_BITS.iter().enumerate() {
            if mask & bit == 0 {
                continue;
            }
            let p = self.layers[layer][at];
            if p.color != 0 && best.is_none_or(|(_, b)| p.priority > b.priority) {
                best = Some((layer, p));
            }
        }
        best
    }

    /// Combines the layers the way the PPU does: the main screen shows the
    /// topmost enabled layer or the backdrop (CGRAM colour 0), and pixels
    /// whose layer is enabled in `CGADSUB` are added to or subtracted from
    /// the subscreen's topmost pixel, or the fixed colour where the
    /// subscreen is transparent (in which case the result is not halved).
    /// Objects on sprite palettes 0-3 are exempt. No colour window is
    /// modelled.
    pub fn compose(&self, palette: &Palette, screen: &crate::video::Screen) -> RgbImage {
        let mut img = RgbImage::new(self.width, self.height);
        let prevented = screen.prevents_math();
        let clipped = screen.clips_to_black();
        let add_subscreen = screen.math_select & 0x02 != 0;
        let halve = screen.color_math & 0x40 != 0;
        let subtract = screen.color_math & 0x80 != 0;
        for (at, out) in img.pixels.iter_mut().enumerate() {
            let (color, bit, exempt) = match self.pick(screen.main, at) {
                Some((layer, p)) => (
                    palette.colors[p.color as usize],
                    LAYER_BITS[layer],
                    layer == OBJ && p.color < FIRST_MATH_OBJECT_COLOR,
                ),
                None => (palette.colors[0], BACKDROP_BIT, false),
            };
            let main = if clipped { Color15(0) } else { color };
            let math = screen.color_math & bit != 0 && !exempt && !prevented;
            let result = if math {
                let (operand, halve) = match (add_subscreen, self.pick(screen.sub, at)) {
                    (true, Some((_, p))) => (palette.colors[p.color as usize], halve && !clipped),
                    (true, None) => (screen.fixed_color, false),
                    (false, _) => (screen.fixed_color, halve && !clipped),
                };
                color_math(main, operand, subtract, halve)
            } else {
                main
            };
            *out = result.to_rgb8();
        }
        img
    }
}

/// Adds or subtracts two colours per channel in the PPU's five bits,
/// optionally halving the result, clamped to the channel range.
fn color_math(main: Color15, operand: Color15, subtract: bool, halve: bool) -> Color15 {
    let channel = |m: u8, o: u8| -> u8 {
        let (m, o) = (m as i32, o as i32);
        let v = if subtract { (m - o).max(0) } else { m + o };
        let v = if halve { v / 2 } else { v };
        v.min(31) as u8
    };
    Color15::from_rgb5(
        channel(main.r(), operand.r()),
        channel(main.g(), operand.g()),
        channel(main.b(), operand.b()),
    )
}

/// Draws one 8x8 tile into a background layer: pixel value `p` becomes
/// CGRAM colour `color_base + p`.
#[allow(clippy::too_many_arguments)]
fn draw_tile8_layer(
    layers: &mut LevelLayers,
    layer: usize,
    x: i32,
    y: i32,
    tile: &Tile8,
    color_base: u8,
    priority: u8,
    flip_x: bool,
    flip_y: bool,
) {
    for (ty, line) in tile.pixels.iter().enumerate() {
        for (tx, &px) in line.iter().enumerate() {
            if px == 0 {
                continue;
            }
            let dx = if flip_x { 7 - tx } else { tx } as i32;
            let dy = if flip_y { 7 - ty } else { ty } as i32;
            if let Some(at) = layers.index(x + dx, y + dy) {
                layers.put(layer, at, color_base + px, priority);
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

/// Draws a 16x16 tile into a layer at pixel position (`x`, `y`), each
/// quadrant at the low or high priority its priority bit selects.
/// `palette_mask` is ORed into the palette row, as the game's upload
/// routine does for layer 2 objects in object tileset 3.
#[allow(clippy::too_many_arguments)]
fn draw_map16_layer(
    layers: &mut LevelLayers,
    layer: usize,
    priorities: [u8; 2],
    x: i32,
    y: i32,
    tile: &Map16Tile,
    tiles: &LayerTiles,
    palette_mask: u8,
) {
    for qy in 0..2 {
        for qx in 0..2 {
            let r = tile.quadrant(qx, qy);
            let color_base = ((r.palette() | palette_mask) & 7) * 16;
            draw_tile8_layer(
                layers,
                layer,
                x + 8 * qx as i32,
                y + 8 * qy as i32,
                tiles.get(r.tile()),
                color_base,
                priorities[r.priority() as usize],
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

/// Renders a level: its layer 1 tile grid, its layer 2 (background
/// tilemap or objects), and its layer 3, combined by the screen
/// designation and colour math the level set up. Sprites are left out;
/// see [`level_layers`] and [`draw_sprite_scene`] to include them.
pub fn level_image(
    tiles: &crate::expand::LevelTiles,
    layer_tiles: &LayerTiles,
    palette: &Palette,
) -> RgbImage {
    compose_level(tiles, &level_layers(tiles, layer_tiles), palette)
}

/// Draws a level's background layers, ready for sprites to be added
/// before [`compose_level`] turns them into a picture. Boss arenas have
/// their own drawing path and get empty layers.
pub fn level_layers(tiles: &crate::expand::LevelTiles, layer_tiles: &LayerTiles) -> LevelLayers {
    if tiles.boss_scene.is_some() {
        return LevelLayers::new(256, 224);
    }
    let (w, h) = tiles.size();
    let mut layers = LevelLayers::new(w as u32 * 16, h as u32 * 16);
    draw_layer1(&mut layers, tiles, layer_tiles);
    draw_layer2(&mut layers, tiles, layer_tiles);
    if let Some(layer3) = &tiles.layer3 {
        draw_layer3(&mut layers, layer3, &tiles.vram);
    }
    layers
}

/// Turns drawn layers into the picture the level shows. Boss arenas are
/// rendered from their captured video-mode bands instead.
pub fn compose_level(
    tiles: &crate::expand::LevelTiles,
    layers: &LevelLayers,
    palette: &Palette,
) -> RgbImage {
    match &tiles.boss_scene {
        Some(scene) => boss_image(
            scene,
            &tiles.vram,
            palette,
            tiles.screen.fixed_color.to_rgb8(),
        ),
        None => layers.compose(palette, &tiles.screen),
    }
}

/// Draws the layer 1 tile grid.
fn draw_layer1(
    layers: &mut LevelLayers,
    tiles: &crate::expand::LevelTiles,
    layer_tiles: &LayerTiles,
) {
    let (w, h) = tiles.size();
    for y in 0..h {
        for x in 0..w {
            if let Some(tile) = tiles.map16_at(tiles.tile_at(x, y), x, y) {
                draw_map16_layer(
                    layers,
                    BG1,
                    [LAYER1_LOW, LAYER1_HIGH],
                    (x * 16) as i32,
                    (y * 16) as i32,
                    tile,
                    layer_tiles,
                    0,
                );
            }
        }
    }
}

/// Draws layer 3 from the captured tilemap: where the game showed it on
/// the entry screen, continued unstretched across the level along the
/// axes it scrolls on (a parallax layer keeps the entry screen's phase).
/// Along an axis the layer does not scroll it keeps its screen position:
/// horizontally the entry view repeats every 256 pixels, since every
/// screen of the level shows the same fixed backdrop; vertically it stays
/// inside the entry screen's 224-pixel band, where a tide sits while the
/// camera rests.
fn draw_layer3(layers: &mut LevelLayers, layer3: &crate::video::Layer3, vram: &[u8]) {
    let priority_bit = layer3.high_priority_in_front();
    for ly in 0..layers.height {
        let Some(ty) = layer3_axis(layer3, 1, ly as i32) else {
            continue;
        };
        for lx in 0..layers.width {
            let Some(tx) = layer3_axis(layer3, 0, lx as i32) else {
                continue;
            };
            let Some((color, high)) = layer3_pixel(vram, layer3, tx, ty) else {
                continue;
            };
            let priority = match (high, priority_bit) {
                (true, true) => LAYER3_FRONT,
                (true, false) => LAYER3_HIGH,
                (false, _) => LAYER3_LOW,
            };
            let at = (ly * layers.width + lx) as usize;
            layers.put(BG3, at, color, priority);
        }
    }
}

/// Layer 3 tilemap coordinate shown at a level coordinate along one axis
/// (0 horizontal, 1 vertical), or `None` where the layer is not drawn.
fn layer3_axis(layer3: &crate::video::Layer3, axis: usize, at: i32) -> Option<i32> {
    let position = layer3.position[axis] as i32;
    let offset = at - layer3.camera[axis] as i32;
    if layer3.scroll_per_16[axis] != 0 {
        Some(position + offset)
    } else if axis == 0 {
        Some(position + offset.rem_euclid(256))
    } else {
        (0..224).contains(&offset).then_some(position + offset)
    }
}

/// Colour index (1-31) and priority of a layer 3 tilemap pixel, or `None`
/// where it is transparent. Mode 1's layer 3 is 2bpp with 8x8 tiles.
fn layer3_pixel(vram: &[u8], layer3: &crate::video::Layer3, x: i32, y: i32) -> Option<(u8, bool)> {
    let (x, y) = (x.rem_euclid(512) as usize, y.rem_euclid(512) as usize);
    let wide = (layer3.tilemap as usize & 1) + 1;
    let tall = ((layer3.tilemap as usize >> 1) & 1) + 1;
    let col = (x / 8) % (32 * wide);
    let row = (y / 8) % (32 * tall);
    let at = ((layer3.tilemap as usize >> 2) << 11)
        + (row / 32 * wide + col / 32) * 0x800
        + (row % 32 * 32 + col % 32) * 2;
    let tile = Tile8Ref(u16::from_le_bytes([
        video_byte(vram, at),
        video_byte(vram, at + 1),
    ]));
    let px = if tile.flip_x() { 7 - x % 8 } else { x % 8 };
    let py = if tile.flip_y() { 7 - y % 8 } else { y % 8 };
    let start = layer3.character_base as usize + tile.tile() as usize * 16 + py * 2;
    let mut color = 0;
    for plane in 0..2 {
        color |= ((video_byte(vram, start + plane) >> (7 - px)) & 1) << plane;
    }
    (color != 0).then_some((color + tile.palette() * 4, tile.priority()))
}

/// Draws captured sprite objects into the object layer, front to back:
/// the first opaque object at a pixel wins, and carries its OAM priority
/// against the background layers.
pub fn draw_sprite_scene(layers: &mut LevelLayers, scene: &crate::video::SpriteScene, vram: &[u8]) {
    draw_objects(layers, &scene.objects, scene.object_select, vram);
}

/// Draws OAM objects (in level coordinates) into the object layer, front
/// to back, with the sizes and character base `object_select` selects.
/// Objects already in the layer stay in front of these.
pub fn draw_objects(
    layers: &mut LevelLayers,
    objects: &[crate::video::SpriteObject],
    object_select: u8,
    vram: &[u8],
) {
    let sizes = crate::expand::object_sizes(object_select);
    for object in objects {
        let (width, height) = sizes[object.large as usize];
        let priority = OBJECT_PRIORITIES[(object.attr >> 4 & 3) as usize];
        let color_base = 128 + (object.attr >> 1 & 7) * 16;
        for dy in 0..height {
            for dx in 0..width {
                let Some(at) = layers.index(object.x + dx, object.y + dy) else {
                    continue;
                };
                let Some(color) = object_pixel(
                    vram,
                    object_select,
                    object.tile,
                    object.attr,
                    dx,
                    dy,
                    width,
                    height,
                ) else {
                    continue;
                };
                layers.put_object(at, color_base + color, priority);
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

/// Draws layer 2 where the entry camera sees it: the background tilemap
/// repeated every two screens, or the layer 2 objects from their own
/// region of the tile grid, displaced from the layer 1 grid by the
/// difference between the layer 2 and layer 1 positions at entry (layer 2
/// scroll settings offset the layer or move it at another rate). Beyond
/// the entry screen the layer continues unstretched.
///
/// A vertical level's background is the same two-screen-wide tilemap
/// (mode `$0A` keeps layer 2 horizontal: `$5B` bit 1 clear) spanning the
/// level's full 32-tile width, and the game scrolls it slowly so the
/// 27 rows cover the whole descent. A static render cannot reproduce
/// that parallax, so the background is tiled down the level instead.
fn draw_layer2(
    layers: &mut LevelLayers,
    tiles: &crate::expand::LevelTiles,
    layer_tiles: &LayerTiles,
) {
    let priorities = [LAYER2_LOW, LAYER2_HIGH];
    // Boss arenas and dark rooms sharing their tilemap do not display the
    // decoded background buffer.
    let background = tiles.layer2_tilemap.is_some();
    if background && matches!(tiles.level_mode, 0x09 | 0x0B | 0x0F | 0x10) {
        return;
    }
    if !background && tiles.layer2_objects().is_none() {
        return;
    }
    let palette_mask = if background {
        0
    } else {
        tiles.layer2_palette_mask()
    };
    let rows = tiles.layer2_bg_rows().min(crate::expand::SCREEN_ROWS) as i32;
    let [dx, dy] = tiles.layer2_offset();
    // Layer 2 tile (tx, ty) covers level pixels from (tx * 16 + dx, ty * 16 + dy).
    let first = |d: i32| (-d).div_euclid(16);
    let last = |d: i32, extent: u32| (extent as i32 - d).div_euclid(16);
    for ty in first(dy)..=last(dy, layers.height) {
        for tx in first(dx)..=last(dx, layers.width) {
            let tile = if background {
                let (col, row) = (tx.rem_euclid(32) as usize, ty.rem_euclid(rows) as usize);
                let n = tiles.layer2_bg_tile(col / 16, col % 16, row).unwrap();
                tiles.bg_map16.get(n as usize - 0x200)
            } else {
                let (Ok(x), Ok(y)) = (usize::try_from(tx), usize::try_from(ty)) else {
                    continue;
                };
                tiles
                    .layer2_object_tile(x, y)
                    .and_then(|n| tiles.map16_at(n, x, y))
            };
            if let Some(tile) = tile {
                draw_map16_layer(
                    layers,
                    BG2,
                    priorities,
                    tx * 16 + dx,
                    ty * 16 + dy,
                    tile,
                    layer_tiles,
                    palette_mask,
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
    fn layer3_axes_continue_the_entry_view_or_keep_the_screen() {
        let mut layer3 = crate::video::Layer3 {
            position: [100, 64],
            camera: [32, 192],
            scroll_per_16: [8, 0],
            ..Default::default()
        };
        assert_eq!(layer3_axis(&layer3, 0, 32), Some(100));
        assert_eq!(layer3_axis(&layer3, 0, 48), Some(116)); // unstretched
        assert_eq!(layer3_axis(&layer3, 0, 31), Some(99));
        // A vertically fixed layer stays inside the entry screen's band.
        assert_eq!(layer3_axis(&layer3, 1, 192), Some(64));
        assert_eq!(layer3_axis(&layer3, 1, 415), Some(287));
        assert_eq!(layer3_axis(&layer3, 1, 416), None);
        assert_eq!(layer3_axis(&layer3, 1, 191), None);
        // A horizontally fixed layer repeats the entry screen.
        layer3.scroll_per_16 = [0, 16];
        assert_eq!(layer3_axis(&layer3, 0, 32), Some(100));
        assert_eq!(layer3_axis(&layer3, 0, 288), Some(100));
        assert_eq!(layer3_axis(&layer3, 0, 31), Some(355));
        assert_eq!(layer3_axis(&layer3, 1, 0), Some(64 - 192));
    }

    #[test]
    fn color_math_adds_subtracts_and_halves_in_five_bits() {
        let main = Color15::from_rgb5(31, 0, 20);
        let sub = Color15::from_rgb5(0, 31, 20);
        assert_eq!(
            color_math(main, sub, false, false),
            Color15::from_rgb5(31, 31, 31)
        );
        assert_eq!(
            color_math(main, sub, true, false),
            Color15::from_rgb5(31, 0, 0)
        );
        assert_eq!(
            color_math(main, sub, false, true),
            Color15::from_rgb5(15, 15, 20)
        );
        assert_eq!(
            color_math(main, sub, true, true),
            Color15::from_rgb5(15, 0, 0)
        );
    }

    /// One pixel per column: layer 1 (colour 1, red) at column 1, layer 2
    /// (colour 2, blue) at columns 1 and 2, an object on sprite palette 0
    /// (colour 129, green) at column 3 and one on palette 4 (colour 193,
    /// also green) at column 4, all low priority; nothing at column 0.
    fn layers() -> (LevelLayers, Palette) {
        let mut layers = LevelLayers::new(5, 1);
        layers.put(BG1, 1, 1, LAYER1_LOW);
        layers.put(BG2, 1, 2, LAYER2_LOW);
        layers.put(BG2, 2, 2, LAYER2_LOW);
        layers.put_object(3, 129, OBJECT_PRIORITIES[0]);
        layers.put_object(4, 193, OBJECT_PRIORITIES[0]);
        let mut palette = Palette::default();
        palette.colors[1] = Color15::from_rgb5(31, 0, 0);
        palette.colors[2] = Color15::from_rgb5(0, 0, 31);
        palette.colors[129] = Color15::from_rgb5(0, 31, 0);
        palette.colors[193] = Color15::from_rgb5(0, 31, 0);
        (layers, palette)
    }

    fn rgb5(image: &RgbImage) -> Vec<[u8; 3]> {
        image.pixels.iter().map(|p| p.map(|c| c >> 3)).collect()
    }

    #[test]
    fn compose_follows_screen_designation_and_color_math() {
        use crate::video::Screen;
        let (layers, palette) = layers();
        let fixed = Color15::from_rgb5(8, 8, 8);
        // Vanilla: layer 2 on the subscreen shows through the backdrop only.
        let screen = Screen::vanilla(fixed);
        assert_eq!(
            rgb5(&layers.compose(&palette, &screen)),
            [[8, 8, 8], [31, 0, 0], [0, 0, 31], [0, 31, 0], [0, 31, 0]]
        );
        // Half-brightness modes: objects and the backdrop halve with layer
        // 2, except objects on palettes 0-3 and pixels over a transparent
        // subscreen, which take the fixed colour unhalved.
        let half = Screen {
            color_math: 0x70,
            ..screen
        };
        assert_eq!(
            rgb5(&layers.compose(&palette, &half)),
            [[8, 8, 8], [31, 0, 0], [0, 0, 15], [0, 31, 0], [8, 31, 8]]
        );
        // The spotlight rooms subtract the fixed colour from everything and
        // halve, with the fixed colour as the operand; "prevent inside the
        // colour window" prevents nothing without a window.
        let dark = Screen {
            main: 0x17,
            sub: 0x00,
            color_math: 0xFF,
            math_select: 0x20,
            fixed_color: fixed,
        };
        assert_eq!(
            rgb5(&layers.compose(&palette, &dark)),
            [[0, 0, 0], [11, 0, 0], [0, 0, 11], [0, 31, 0], [0, 11, 0]]
        );
        // Clipping to black and preventing math everywhere.
        let clipped = Screen {
            math_select: 0xF2,
            ..screen
        };
        assert_eq!(rgb5(&layers.compose(&palette, &clipped)), [[0; 3]; 5]);
        // Level mode $1E: only layer 1 on the main screen, added to the
        // objects and layer 2 beneath it.
        let translucent = Screen {
            main: 0x01,
            sub: 0x16,
            color_math: 0x21,
            math_select: 0x02,
            fixed_color: fixed,
        };
        assert_eq!(
            rgb5(&layers.compose(&palette, &translucent)),
            [[8, 8, 8], [31, 0, 31], [0, 0, 31], [0, 31, 0], [0, 31, 0]]
        );
    }

    #[test]
    fn layer3_pixels_come_from_the_2bpp_tilemap_with_flips() {
        let mut vram = vec![0; 0x10000];
        let layer3 = crate::video::Layer3 {
            tilemap: 0x53,
            character_base: 0x8000,
            ..Default::default()
        };
        // Tile 2, palette 3, high priority, X flip, at column 1 of the
        // bottom-left 32x32 screen (row 32).
        let word: u16 = 0x2002 | (3 << 10) | 0x4000;
        vram[0xB000 + 2..0xB000 + 4].copy_from_slice(&word.to_le_bytes());
        vram[0x8000 + 2 * 16] = 0x80; // plane 0, row 0, leftmost pixel
        vram[0x8000 + 2 * 16 + 1] = 0x80; // plane 1
        assert_eq!(layer3_pixel(&vram, &layer3, 15, 256), Some((15, true)));
        assert_eq!(layer3_pixel(&vram, &layer3, 8, 256), None);
        assert_eq!(
            layer3_pixel(&vram, &layer3, 15 + 512, 256 + 512),
            Some((15, true))
        );
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
