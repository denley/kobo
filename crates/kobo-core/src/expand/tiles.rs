//! A loaded level: the tile grid the game expanded its objects into, and
//! what level preparation left in video memory and RAM around it.

use std::collections::HashMap;

use super::Diagnostic;
use crate::level::PrimaryHeader;
use crate::map16::Map16Tile;
use crate::ram::{self, Ram};

/// Bytes per plane of the tile grid.
pub const GRID_LEN: usize = 0x3800;
pub const SCREEN_ROWS: usize = 27;
pub const SCREEN_COLS: usize = 16;
pub(super) const SCREEN_LEN: usize = SCREEN_ROWS * SCREEN_COLS;

/// Lunar Magic 3's level sizes (Vitor Vilela's dynamic level patch):
/// the height in pixels of each horizontal level mode and how many
/// screens of it fit the tile planes. Mode 0 is the vanilla layout.
pub const LEVEL_SIZES: [(u16, usize); 32] = [
    (0x01B0, 0x20),
    (0x01C0, 0x20),
    (0x01D0, 0x1E),
    (0x0200, 0x1C),
    (0x0220, 0x1A),
    (0x0250, 0x18),
    (0x0260, 0x17),
    (0x0280, 0x16),
    (0x02A0, 0x15),
    (0x02C0, 0x14),
    (0x02F0, 0x13),
    (0x0310, 0x12),
    (0x0340, 0x11),
    (0x0380, 0x10),
    (0x03B0, 0x0F),
    (0x0400, 0x0E),
    (0x0440, 0x0D),
    (0x04A0, 0x0C),
    (0x0510, 0x0B),
    (0x0590, 0x0A),
    (0x0630, 0x09),
    (0x0700, 0x08),
    (0x0800, 0x07),
    (0x0950, 0x06),
    (0x0B30, 0x05),
    (0x0E00, 0x04),
    (0x12A0, 0x03),
    (0x1C00, 0x02),
    (0x3800, 0x01),
    (0x0100, 0x38),
    (0x00F0, 0x3B),
    (0x00E0, 0x40),
];

/// Screens of a horizontal level of `rows` tile rows that fit the planes,
/// per [`LEVEL_SIZES`]; `None` for a height Lunar Magic does not define.
pub fn max_screens(rows: usize) -> Option<usize> {
    LEVEL_SIZES
        .iter()
        .find(|(height, _)| *height as usize == rows * 16)
        .map(|(_, screens)| *screens)
}

/// Where a level keeps its layer 2 objects in the tile grid, per the
/// game's layer 2 upload dispatch (`CODE_058883`) and the per-mode screen
/// pointer tables at `$00BB08` and `$00BC16`. The layout is independent
/// of layer 1's: modes 3 and 4 pair a vertical layer 1 with a horizontal
/// layer 2, and modes 5 and 6 the reverse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer2Objects {
    /// Horizontal screens of 16 by `rows` tiles from plane offset `base`.
    /// A level with layer 2 objects splits the screens its height allows
    /// between the layers: layer 1 takes the first half (rounded up) and
    /// layer 2 the rest, starting right after. Vanilla's 27-row levels
    /// give 16 screens each from `0x1B00`; Lunar Magic 3's expanded
    /// heights follow the same rule with their own screen counts (the
    /// dynamic tilemap upload at `$1F8000` reads layer 2 from there).
    Horizontal {
        base: usize,
        rows: usize,
        screens: usize,
    },
    /// 14 screens of 32x16 tiles (left and right halves) from `0x1C00`.
    Vertical,
}

impl Layer2Objects {
    /// The layout for a level mode and layer 1 row count, or `None` when
    /// the mode uploads no layer 2 objects (background tilemap modes and
    /// boss arenas) or the height is unknown.
    pub fn for_level(mode: u8, rows: usize) -> Option<Self> {
        match mode {
            0x01..=0x04 | 0x0F | 0x1F => {
                let total = max_screens(rows)?;
                let layer1 = total.div_ceil(2);
                Some(Self::Horizontal {
                    base: layer1 * rows * SCREEN_COLS,
                    rows,
                    screens: total - layer1,
                })
            }
            0x05..=0x08 => Some(Self::Vertical),
            _ => None,
        }
    }

    /// Plane offset of a level-wide tile position, or `None` outside the
    /// buffer.
    pub fn offset(self, x: usize, y: usize) -> Option<usize> {
        match self {
            Self::Horizontal {
                base,
                rows,
                screens,
            } => (x / SCREEN_COLS < screens && y < rows).then(|| {
                base + (x / SCREEN_COLS) * rows * SCREEN_COLS + y * SCREEN_COLS + x % SCREEN_COLS
            }),
            Self::Vertical => (x < 32 && y / 16 < 14)
                .then(|| 0x1C00 + (y / 16) * 0x200 + (x / 16) * 0x100 + (y % 16) * 16 + x % 16),
        }
    }
}

/// Bytes per plane of the layer 2 background tilemap buffer.
pub const LAYER2_TILEMAP_LEN: usize = 0x400;
/// The vertical pipe tiles whose definition the game picks by position,
/// and how many alternatives `MAP16AppTable` offers.
pub const PIPE_TILES: std::ops::RangeInclusive<u16> = 0x133..=0x13A;
pub const PIPE_TILE_COUNT: usize = 8;
pub const PIPE_VARIANTS: usize = 4;
/// Bytes per screen of a Lunar Magic 32-row background (16x32 tiles).
pub(super) const LM_TALL_SCREEN_LEN: usize = 0x200;

/// A level's expanded tile grid.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LevelTiles {
    pub level: u16,
    pub header: PrimaryHeader,
    /// Level mode as the game stored it.
    pub level_mode: u8,
    /// Object tileset as the game stored it. Tileset 3 shifts layer 2
    /// object palettes up by four rows on upload.
    pub object_tileset: u8,
    /// True for vertical levels.
    pub vertical: bool,
    pub screens: usize,
    /// Rows per screen of the layer 1 grid: 27 for horizontal levels, 16
    /// for vertical ones, or the height Lunar Magic 3's expanded level
    /// format gave a horizontal level (`$13D7 / 16`, up to 448). Screens
    /// follow one another in the planes with a stride of `rows * 16`.
    pub rows: usize,
    pub low: Vec<u8>,
    pub high: Vec<u8>,
    /// The game's RAM after level preparation: what the sprite passes
    /// start from, and open to inspection through [`crate::ram`].
    pub ram: Ram,
    /// Foreground Map16 definitions for tile numbers in the object grid.
    /// Lunar Magic pages 2 and 3 are distinct from the same-numbered BG
    /// tiles, which live in `bg_map16`. Prefer [`LevelTiles::map16_at`],
    /// which also knows the position-dependent pipe tiles.
    pub map16: HashMap<u16, Map16Tile>,
    /// Vanilla definitions of the vertical pipe tiles `133`-`13A` by
    /// position: the game re-points them for every column (row in vertical
    /// levels) it uploads, choosing variant `(column / 8) % 4` from
    /// `MAP16AppTable`, so a pipe's colour depends on where it stands.
    /// `None` for Lunar Magic ROMs, whose upload resolves tiles through
    /// Lunar Magic's own pointer routine and ignores the re-pointing.
    pub pipe_map16: Option<[[Map16Tile; PIPE_TILE_COUNT]; PIPE_VARIANTS]>,
    /// BG Map16 definitions, indexed by the raw background tile number.
    /// Vanilla has 0x200 definitions; Lunar Magic backgrounds can use
    /// higher indices. Empty when the level has no decoded background.
    pub bg_map16: Vec<Map16Tile>,
    /// VRAM as uploaded by level preparation: layer tiles at `$0000`,
    /// sprite tiles at `$C000`, tilemaps in between. Boss arenas also
    /// include the first drawing pass's player and boss graphics uploads.
    pub vram: Vec<u8>,
    /// Which VRAM bytes level preparation actually wrote.
    pub vram_written: Vec<bool>,
    /// CGRAM as uploaded by level preparation.
    pub cgram: Vec<u8>,
    /// `BG1SC`-`BG4SC` as level preparation set them: bits 7-2 are the
    /// tilemap's VRAM word address divided by `$400`, bit 1 selects 64
    /// tiles tall, bit 0 selects 64 tiles wide. Vanilla puts layer 1 at
    /// `$2000` and layer 2 at `$3000`, both 64x64; Lunar Magic uses
    /// `$3000` and `$3800`, 64x32.
    pub bg_sc: [u8; 4],
    /// `OBSEL`: object sizes and character base as level preparation set it.
    pub object_select: u8,
    /// The player's OAM objects at the level's entrance, in level
    /// coordinates, as the game draws him once any entrance action (pipe,
    /// cannon pipe, door) has finished. His per-frame tile and palette
    /// uploads are applied to `vram` and `cgram`. Empty for boss arenas,
    /// whose drawing pass already includes him.
    pub player: Vec<crate::video::SpriteObject>,
    /// Video-mode bands installed by the ROM's boss NMI/IRQ handlers.
    pub boss_scene: Option<crate::video::BossScene>,
    /// Layer 3 position and scroll behaviour, when the level shows
    /// layer 3 on either screen in Mode 1 (every ordinary level; boss
    /// arenas draw theirs into `boss_scene`).
    pub layer3: Option<crate::video::Layer3>,
    /// Main and sub screen designation and colour math, which decide how
    /// the layers combine into the picture.
    pub screen: crate::video::Screen,
    /// Layer 1 position the level was entered at (`$1A`/`$1C`) and the
    /// layer 2 position the first camera update derived for it (`$1E`/
    /// `$20`). The two differ when the level's layer 2 scroll settings
    /// offset or slow the layer (parallax); the renderer draws layer 2
    /// where this camera sees it.
    pub camera: [u16; 2],
    pub layer2_position: [u16; 2],
    /// Layer 2 background tilemap planes, when the level uses a
    /// pre-built background instead of layer 2 objects. Raw tile numbers
    /// index `bg_map16`; `layer2_bg_tile` adds the legacy 0x200 display base.
    pub layer2_tilemap: Option<(Vec<u8>, Vec<u8>)>,
    /// Bytes per screen of the background planes: `0x1B0` (16x27, vanilla)
    /// or `0x200` (16x32, Lunar Magic's taller backgrounds).
    pub layer2_screen_len: usize,
    /// What went wrong without stopping the level from loading: the
    /// player's entrance pass giving up leaves `player` empty.
    pub diagnostics: Vec<Diagnostic>,
}

impl LevelTiles {
    /// Map16 tile number at a horizontal-level position.
    pub fn tile(&self, screen: usize, x: usize, y: usize) -> u16 {
        let i = screen * self.screen_len() + y * SCREEN_COLS + x;
        self.low[i] as u16 | ((self.high[i] as u16) << 8)
    }

    /// Bytes per screen in the layer 1 planes.
    pub fn screen_len(&self) -> usize {
        if self.vertical {
            0x200
        } else {
            self.rows * SCREEN_COLS
        }
    }

    /// Buffer offset of a level-wide tile position, for either orientation.
    pub fn offset(&self, x: usize, y: usize) -> usize {
        if self.vertical {
            (y / 16) * 0x200 + (x / 16) * 0x100 + (y % 16) * 16 + (x % 16)
        } else {
            (x / SCREEN_COLS) * self.screen_len() + y * SCREEN_COLS + (x % SCREEN_COLS)
        }
    }

    /// Map16 tile number at a level-wide position.
    pub fn tile_at(&self, x: usize, y: usize) -> u16 {
        let i = self.offset(x, y);
        self.low[i] as u16 | ((self.high[i] as u16) << 8)
    }

    /// The foreground definition of tile number `n` standing at level
    /// tile position (`x`, `y`): the pipe tiles `133`-`13A` take the
    /// variant the game's upload picked for that column (row in a vertical
    /// level); everything else comes from `map16`.
    pub fn map16_at(&self, n: u16, x: usize, y: usize) -> Option<&Map16Tile> {
        if let Some(variants) = &self.pipe_map16
            && PIPE_TILES.contains(&n)
        {
            let along = if self.vertical { y } else { x };
            return Some(&variants[(along / 8) % PIPE_VARIANTS][(n - PIPE_TILES.start()) as usize]);
        }
        self.map16.get(&n)
    }

    /// How this level's layer 2 objects are laid out, if it has any.
    pub fn layer2_objects(&self) -> Option<Layer2Objects> {
        if self.layer2_tilemap.is_some() {
            return None;
        }
        // Modes 3 and 4 pair a vertical layer 1 with a vanilla horizontal
        // layer 2.
        let rows = if self.vertical {
            SCREEN_ROWS
        } else {
            self.rows
        };
        Layer2Objects::for_level(self.level_mode, rows)
    }

    /// Map16 tile number of the layer 2 object at a level-wide position.
    /// The game resolves these through the same Map16 pointer table as
    /// layer 1, so they index `map16`, not `bg_map16`. `None` when the
    /// level's layer 2 is not objects or the position is outside the
    /// layer 2 buffer.
    pub fn layer2_object_tile(&self, x: usize, y: usize) -> Option<u16> {
        let i = self.layer2_objects()?.offset(x, y)?;
        Some(self.low[i] as u16 | ((self.high[i] as u16) << 8))
    }

    /// How far layer 2 content is displaced from the layer 1 grid, in
    /// pixels: a layer 2 tile at column `c` shows at level x
    /// `c * 16 + offset[0]`. Zero when both layers scroll together.
    pub fn layer2_offset(&self) -> [i32; 2] {
        std::array::from_fn(|axis| {
            self.camera[axis].wrapping_sub(self.layer2_position[axis]) as i16 as i32
        })
    }

    /// Palette bits the layer 2 object upload ORs into every tile: bit 2
    /// (rows 4-7) in object tileset 3, where `CODE_058B8D` ORs `$1000`
    /// into the tilemap words; nothing otherwise.
    pub fn layer2_palette_mask(&self) -> u8 {
        if self.object_tileset == 3 { 4 } else { 0 }
    }

    /// Where the game found the level's sprite data, honouring any Lunar
    /// Magic relocation.
    pub fn sprite_data_ptr(&self) -> crate::addr::SnesAddr {
        crate::addr::SnesAddr::new(self.ram.u24(ram::SPRITE_DATA_PTR))
    }

    /// The back area colour the game settled on.
    pub fn back_area_color(&self) -> crate::palette::Color15 {
        crate::palette::Color15(self.ram.u16(ram::BACKGROUND_COLOR))
    }

    /// The palette as uploaded to CGRAM.
    pub fn palette(&self) -> crate::palette::Palette {
        crate::palette::Palette::from_cgram(&self.cgram)
    }

    /// Width and height of the captured level in tiles. Some headers
    /// declare more screens than fit in the object buffer (notably
    /// unused vertical levels); only complete captured screens count.
    pub fn size(&self) -> (usize, usize) {
        let len = self.low.len().min(self.high.len());
        if self.vertical {
            (32, self.screens.min(len / 0x200) * 16)
        } else {
            (
                self.screens.min(len / self.screen_len()) * SCREEN_COLS,
                self.rows,
            )
        }
    }

    /// Map16 tile number (BG numbering, `0x200` upwards) at a position in
    /// the layer 2 background tilemap, which is two screens of 16 by 27
    /// tiles laid out like the main buffer. Returns `None` for levels
    /// whose layer 2 is objects.
    pub fn layer2_bg_tile(&self, screen: usize, x: usize, y: usize) -> Option<u16> {
        let (lo, hi) = self.layer2_tilemap.as_ref()?;
        let i = (screen % 2) * self.layer2_screen_len + y * SCREEN_COLS + x;
        Some(0x200 + lo[i] as u16 + ((hi[i] as u16) << 8))
    }

    /// Rows in the layer 2 background: 27, or 32 for Lunar Magic's taller
    /// backgrounds.
    pub fn layer2_bg_rows(&self) -> usize {
        self.layer2_screen_len / SCREEN_COLS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer2_objects_start_after_layer_1s_share_of_the_screens() {
        let horizontal = |rows: usize| match Layer2Objects::for_level(0x02, rows) {
            Some(Layer2Objects::Horizontal {
                base,
                rows,
                screens,
            }) => (base, rows, screens),
            other => panic!("{other:?}"),
        };
        assert_eq!(horizontal(27), (0x1B00, 27, 16));
        assert_eq!(horizontal(47), (0x1D60, 47, 9)); // 19 screens: 10 + 9
        assert_eq!(horizontal(74), (0x1BC0, 74, 6));
        assert_eq!(horizontal(298), (0x2540, 298, 1));
        assert_eq!(horizontal(448), (0x1C00, 448, 1));
        assert_eq!(Layer2Objects::for_level(0x02, 50), None);
        assert_eq!(Layer2Objects::for_level(0x00, 27), None);
        let layout = Layer2Objects::for_level(0x01, 47).unwrap();
        assert_eq!(layout.offset(17, 3), Some(0x1D60 + 0x2F0 + 3 * 16 + 1));
        assert_eq!(layout.offset(9 * 16, 0), None);
        assert_eq!(layout.offset(0, 47), None);
    }
}
