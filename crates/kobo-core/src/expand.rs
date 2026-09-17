//! Expanding a level's objects into the Map16 tile grid by running the
//! ROM's own level loader on the headless CPU.
//!
//! The game keeps the grid in two byte planes: low bytes at `$7EC800` and
//! high bytes at `$7FC800`, `0x3800` bytes each. Horizontal levels store
//! each screen as 16 columns by 27 rows, screen after screen. Vertical
//! levels are 32 tiles wide; each screen is 16 rows stored as a left
//! half and a right half of 16 by 16 tiles.

use thiserror::Error;

use std::collections::HashMap;

use crate::cpu::smw_bus::SmwBus;
use crate::cpu::{Bus, Cpu, CpuError};
use crate::level::{self, LevelError, PrimaryHeader};
use crate::map16::{self, Map16Tile};
use crate::rom::Rom;

/// Bytes per plane of the tile grid.
pub const GRID_LEN: usize = 0x3800;
pub const SCREEN_ROWS: usize = 27;
pub const SCREEN_COLS: usize = 16;
const SCREEN_LEN: usize = SCREEN_ROWS * SCREEN_COLS;

/// RAM addresses the loader reads.
mod ram {
    /// `$141A`: non-zero while inside a level, so the header pointer
    /// routine takes the screen-exit path instead of the overworld one.
    pub const SUBLEVEL_COUNT: u32 = 0x7E_141A;
    /// `$19B8`: screen exit table, level number low byte per screen.
    pub const EXIT_TABLE_LOW: u32 = 0x7E_19B8;
    /// `$19D8`: screen exit table, flags per screen. Vanilla stores the
    /// exit's water bit here and never reads it back. Lunar Magic's
    /// replacement for the high byte lookup (`JSL $05DC50` from
    /// `CODE_05D796`) treats an entry with bit 2 set as its own format:
    /// bit 0 is the level number's high byte, bit 1 selects a secondary
    /// exit, and bit 3 is copied to `$192A`.
    pub const EXIT_TABLE_HIGH: u32 = 0x7E_19D8;
    /// `$1F11`: the player's submap, which vanilla turns into the level
    /// number's high byte.
    pub const OW_PLAYER_SUBMAP: u32 = 0x7E_1F11;
    pub const LAST_SCREEN_HORIZ: u32 = 0x7E_005E;
    pub const SCREEN_MODE: u32 = 0x7E_005B;
    pub const LEVEL_MODE: u32 = 0x7E_1925;
    pub const SCREENS: u32 = 0x7E_005D;
    /// `$13D7`: the level height in pixels. Vanilla leaves it zero;
    /// Lunar Magic 3's loader hook (`JSL` at `$05D9A1`) stores the
    /// height of the level's horizontal level mode here.
    pub const LEVEL_HEIGHT: u32 = 0x7E_13D7;
    /// `$1931`: the object tileset, as the loader stored it.
    pub const OBJECT_TILESET: u32 = 0x7E_1931;
    pub const TILES_LOW: u32 = 0x7E_C800;
    pub const TILES_HIGH: u32 = 0x7F_C800;
    pub const LAYER2_TILEMAP_LOW: u32 = 0x7E_B900;
    pub const LAYER2_TILEMAP_HIGH: u32 = 0x7E_BD00;
    pub const BACKGROUND_COLOR: u32 = 0x7E_0701;
    /// `$0100`: the game mode. Lunar Magic's tilemap upload checks it.
    pub const GAME_MODE: u32 = 0x7E_0100;
    /// 0x200 two-byte pointers into bank `$0D`, built by the level loader.
    pub const MAP16_POINTERS: u32 = 0x7E_0FBE;
    /// Direct page `$0C`: bank byte of the pointer Lunar Magic's routine returns.
    pub const LM_MAP16_BANK: u32 = 0x7E_000C;
    /// Direct page `$0A`-`$0C`: the BG Map16 table pointer the initial
    /// layer 2 tilemap upload reads tile definitions through.
    pub const BG_MAP16_BASE: u32 = 0x7E_000A;
    /// Direct page `$05`-`$06`: bytes per screen of the background buffer,
    /// as set by Lunar Magic's hook (vanilla hard-codes `$1B0`).
    pub const BG_SCREEN_LEN: u32 = 0x7E_0005;
    /// Direct page `$CE`-`$D0`: the level's sprite data pointer.
    pub const SPRITE_DATA_PTR: u32 = 0x7E_00CE;
    /// `$1A`/`$1C`: layer 1 position, and `$1462`/`$1464`: the position
    /// the camera update copies from at the start of each frame.
    pub const LAYER1_X: u32 = 0x7E_001A;
    pub const LAYER1_Y: u32 = 0x7E_001C;
    /// `$1E`/`$20`: layer 2 position. Game mode `$11` copies all eight
    /// bytes of `$1A`-`$21` to `$1462`-`$1469` after resolving the level
    /// header, seeding the camera update.
    pub const LAYER2_X: u32 = 0x7E_001E;
    pub const LAYER2_Y: u32 = 0x7E_0020;
    pub const LAYER_POSITIONS_LEN: u32 = 8;
    pub const NEXT_LAYER1_X: u32 = 0x7E_1462;
    pub const NEXT_LAYER1_Y: u32 = 0x7E_1464;
    /// `$55`: layer 1 scroll direction, which the sprite loader turns into
    /// an offset from the camera to the column it loads (1: none).
    pub const LAYER1_SCROLL_DIR: u32 = 0x7E_0055;
    /// `$1411`/`$1412`: horizontal and vertical camera scroll settings;
    /// zero freezes the camera.
    pub const HORIZ_SCROLL_SETTING: u32 = 0x7E_1411;
    pub const VERT_SCROLL_SETTING: u32 = 0x7E_1412;
    /// `$94`/`$96`: the player's position for the next frame.
    pub const PLAYER_X: u32 = 0x7E_0094;
    pub const PLAYER_Y: u32 = 0x7E_0096;
    /// `$0200`-`$041F` packed OAM, `$0420`-`$043F` the size/X-high bits,
    /// and `$3F` the OAM address the first object was written at.
    pub const OAM: u32 = 0x7E_0200;
    pub const OAM_ADDRESS: u32 = 0x7E_003F;
    /// `$14C8`: sprite slot status (0 = free), and `$1938`: per-entry
    /// "already loaded" flags the level sprite loader keeps.
    pub const SPRITE_STATUS: u32 = 0x7E_14C8;
    pub const SPRITE_LOAD_STATUS: u32 = 0x7E_1938;
    /// `$22`/`$24`: layer 3 position, as the IRQ handler writes it to
    /// `BG3HOFS`/`BG3VOFS` below the status bar.
    pub const LAYER3_X: u32 = 0x7E_0022;
    pub const LAYER3_Y: u32 = 0x7E_0024;
    /// `$17BD`/`$17BC`: how far layer 1 moved this frame, as the camera
    /// update leaves it for the layer scroll routine.
    pub const LAYER1_DX: u32 = 0x7E_17BD;
    pub const LAYER1_DY: u32 = 0x7E_17BC;
    /// `$9D`: sprite lock, which also pauses layer 3 autoscroll.
    pub const SPRITE_LOCK: u32 = 0x7E_009D;
    /// `$3E`: `BGMODE` mirror; `$40`: `CGADSUB` mirror; `$44`: `CGWSEL`
    /// mirror; `$0D9D`/`$0D9E`: main and sub screen designation mirrors.
    pub const BG_MODE: u32 = 0x7E_003E;
    pub const COLOR_MATH: u32 = 0x7E_0040;
    pub const COLOR_MATH_SELECT: u32 = 0x7E_0044;
    pub const MAIN_SCREEN: u32 = 0x7E_0D9D;
    pub const SUB_SCREEN: u32 = 0x7E_0D9E;
}

/// Vanilla sprite slots and level sprite entries the loader tracks.
const SPRITE_SLOTS: u32 = 12;
const SPRITE_LOAD_FLAGS: u32 = 0x80;
/// Most frames a sprite pass runs waiting for the sprites to draw.
const SPRITE_FRAMES: usize = 8;

/// Where a level mode keeps its layer 2 objects in the tile grid, per the
/// game's layer 2 upload dispatch (`CODE_058883`) and the per-mode screen
/// pointer tables at `$00BB08` and `$00BC16`. The layout is independent
/// of layer 1's: modes 3 and 4 pair a vertical layer 1 with a horizontal
/// layer 2, and modes 5 and 6 the reverse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer2Objects {
    /// 16 screens of 16x27 tiles from plane offset `0x1B00`.
    Horizontal,
    /// 14 screens of 32x16 tiles (left and right halves) from `0x1C00`.
    Vertical,
}

impl Layer2Objects {
    /// The layout for a level mode, or `None` when the mode uploads no
    /// layer 2 objects (background tilemap modes and boss arenas).
    pub fn for_level_mode(mode: u8) -> Option<Self> {
        match mode {
            0x01..=0x04 | 0x0F | 0x1F => Some(Self::Horizontal),
            0x05..=0x08 => Some(Self::Vertical),
            _ => None,
        }
    }

    /// Plane offset of a level-wide tile position, or `None` outside the
    /// buffer.
    pub fn offset(self, x: usize, y: usize) -> Option<usize> {
        match self {
            Self::Horizontal => (x / SCREEN_COLS < 16 && y < SCREEN_ROWS).then(|| {
                0x1B00 + (x / SCREEN_COLS) * SCREEN_LEN + y * SCREEN_COLS + x % SCREEN_COLS
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
const LM_TALL_SCREEN_LEN: usize = 0x200;

/// ROM routines the loader entry points call, from the vanilla layout.
/// Lunar Magic keeps these entry points in place.
mod routines {
    /// `CODE_05D796`: resolves the level number and header pointers.
    pub const LOAD_HEADER_POINTERS: u32 = 0x05_D796;
    /// `CODE_05801E`: clears the buffers and runs `LoadLevel`.
    pub const LOAD_LEVEL_DATA: u32 = 0x05_801E;
    /// `CODE_00B888`: decompresses GFX32/GFX33 into RAM. The game runs it
    /// once during the "Nintendo Presents" screen; the animated tile
    /// uploads read from that RAM.
    pub const DECOMPRESS_PLAYER_GFX: u32 = 0x00_B888;
    /// `ClearOutLayer3`: DMA-fills the layer 3 tilemap. Its side effect of
    /// leaving the VRAM port in two-byte mode is what the upload below
    /// relies on.
    pub const CLEAR_LAYER3: u32 = 0x00_85FA;
    /// `CODE_00A993`: uploads GFX28-GFX2B (layer 3 tiles and the status bar
    /// font) to VRAM word `$4000` through the port. The game runs it once
    /// during the "Nintendo Presents" screen and level loads leave that
    /// region alone.
    pub const UPLOAD_LAYER3_GFX: u32 = 0x00_A993;
    /// `CODE_00A635`: initialises level RAM and the player's entrance.
    pub const INIT_LEVEL_RAM: u32 = 0x00_A635;
    /// `CODE_00A796`: initial layer 2 scroll positions.
    pub const INIT_LAYER2_SCROLL: u32 = 0x00_A796;
    /// `GM12PrepLevel`: game mode $12. Uploads GFX, palettes, and the
    /// initial tilemaps; draws boss arenas; sets up layer 3. Ends with RTS.
    pub const PREPARE_LEVEL: u32 = 0x00_A59C;
    /// Lunar Magic's Map16 tile pointer routine. Called with a 16-bit
    /// accumulator holding the tile number times two; returns the pointer's
    /// low word in A and its bank in direct page `$0C`. Its body encodes
    /// the Map16 page layout, which differs between Lunar Magic versions.
    pub const LM_MAP16_POINTER: u32 = 0x06_F540;
    /// Inside `CODE_058D7A` (initial layer 2 tilemap upload), where vanilla
    /// stores `#Map16BGTiles` to `$0A`. Lunar Magic 2.3 and later replace
    /// the store with a `JSL` to a routine that leaves the level's BG
    /// Map16 table pointer in `$0A`-`$0C`; the BG pages live in a separate
    /// block from the layer 1 pages, so `LM_MAP16_POINTER` cannot find
    /// them. Older versions keep the vanilla table.
    pub const BG_MAP16_BASE_HOOK: u32 = 0x05_8DA4;
    /// Inside the NMI handler: writes the Mode 7 boss arena's video
    /// registers (mode, tilemap, character base, scroll, matrix).
    pub const MODE7_NMI_REGISTERS: u32 = 0x00_82F7;
    /// Where the NMI handler arms the status bar IRQ; Y holds its scanline.
    pub const SET_STATUS_BAR_IRQ: u32 = 0x00_8294;
    /// The IRQ handler's boss-arena branch, which switches video modes at
    /// the ceiling and floor lines.
    pub const BOSS_IRQ: u32 = 0x00_83BA;
    /// Common exit of the IRQ handler.
    pub const EXIT_IRQ: u32 = 0x00_83B2;
    /// `CODE_00A1DA`: one game-mode `$14` drawing pass, which fills OAM
    /// with the player, boss, and sprite-based arena walls and floor.
    pub const DRAW_LEVEL_FRAME: u32 = 0x00_A1DA;
    /// `CODE_02A802`: the body of `LoadSprFromLevel`, after its
    /// every-other-frame check. Spawns the level sprites at the column the
    /// camera position and scroll direction select. Lunar Magic reroutes
    /// its inner loop but keeps this entry.
    pub const SPAWN_SPRITES: u32 = 0x02_A802;
    /// `CODE_0098A9`: uploads the boss's graphics to VRAM.
    pub const UPLOAD_BOSS_TILES: u32 = 0x00_98A9;
    /// `CODE_00A300`: uploads the player's graphics to VRAM.
    pub const UPLOAD_PLAYER_TILES: u32 = 0x00_A300;
    /// `MAP16AppTable`: four pointers into bank `$0D`, one per 8-column
    /// stretch of the level, to alternative definitions of the vertical
    /// pipe tiles `133`-`13A`. The initial tilemap upload (`CODE_0580BD`)
    /// and the scroll setup (`CODE_05877E`) re-point those tiles from it
    /// as each column goes up, so a pipe's colour follows its position.
    pub const PIPE_POINTER_TABLE: u32 = 0x05_8776;
    /// `UpdateScreenPosition`: the level loop's per-frame camera update.
    /// It follows the player with layer 1 and derives the layer 2 position
    /// from it and the level's layer 2 scroll settings (`$1413`/`$1414`):
    /// the same position, half of it, or a fraction plus the offset
    /// `CODE_00A796` worked out at load time.
    pub const UPDATE_CAMERA: u32 = 0x00_F6DB;
    /// `ProcScreenScrollCmds`: the level loop's per-frame layer scroll
    /// update, run right after the camera update. It moves layer 2 and
    /// layer 3 according to the level's scroll settings (tides, parallax
    /// backgrounds, autoscroll) from the camera delta `$17BC`-`$17BD`.
    pub const SCROLL_LAYERS: u32 = 0x05_BC00;
}

/// `$0101`-`$0108`: the GFX files currently in VRAM. `$FF` forces uploads.
const LOADED_GFX_FILES: u32 = 0x7E_0101;

const STEP_LIMIT: u64 = 200_000_000;

#[derive(Debug, Error)]
pub enum ExpandError {
    #[error(transparent)]
    Level(#[from] LevelError),
    #[error("level {level:03X}: {source}")]
    Cpu {
        level: u16,
        #[source]
        source: CpuError,
    },
    #[error("level {level:03X}: unknown background layout, {len:#x} bytes per screen")]
    BackgroundLayout { level: u16, len: usize },
    #[error("level {0:03X}: Lunar Magic background Map16 table pointer is null")]
    MissingBackgroundTable(u16),
}

/// The reset vector and the main game loop it ends in.
const RESET: u32 = 0x00_8000;
const GAME_LOOP: u32 = 0x00_806B;

/// Runs the reset code up to the main loop: builds the RAM-resident OAM
/// reset routine, uploads the SPC engine (against a stub that echoes the
/// handshake), and clears memory. Patches hooked into the reset code run
/// as well.
fn run_reset(cpu: &mut Cpu, bus: &mut SmwBus, level: u16) -> Result<(), ExpandError> {
    cpu.run_until(bus, RESET, GAME_LOOP, STEP_LIMIT)
        .map_err(|source| ExpandError::Cpu { level, source })?;
    *cpu = Cpu::new();
    Ok(())
}

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
    /// All of work RAM after the loader ran, for inspection.
    pub wram: Vec<u8>,
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
    /// Unknown for horizontal levels with an expanded height: their
    /// layer 1 screens fill the planes, so the layer 2 objects live
    /// somewhere this crate does not know yet.
    pub fn layer2_objects(&self) -> Option<Layer2Objects> {
        if self.layer2_tilemap.is_some() || (!self.vertical && self.rows != SCREEN_ROWS) {
            return None;
        }
        Layer2Objects::for_level_mode(self.level_mode)
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
        let i = (ram::SPRITE_DATA_PTR - 0x7E_0000) as usize;
        crate::addr::SnesAddr::new(u32::from_le_bytes([
            self.wram[i],
            self.wram[i + 1],
            self.wram[i + 2],
            0,
        ]))
    }

    /// The back area colour the game settled on.
    pub fn back_area_color(&self) -> crate::palette::Color15 {
        let i = (ram::BACKGROUND_COLOR - 0x7E_0000) as usize;
        crate::palette::Color15(u16::from_le_bytes([self.wram[i], self.wram[i + 1]]))
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

/// Runs the ROM's level loader for `level` and returns the tile grid.
pub fn expand_level(rom: &Rom, level: u16) -> Result<LevelTiles, ExpandError> {
    expand_level_traced(rom, level, false).map(|(t, _)| t)
}

/// A data read: (address of the reading instruction, address read).
pub type ReadTrace = Vec<(u32, u32)>;

/// Like [`expand_level`], optionally recording every data read made
/// after reset.
pub fn expand_level_traced(
    rom: &Rom,
    level: u16,
    trace: bool,
) -> Result<(LevelTiles, Option<ReadTrace>), ExpandError> {
    let header = level::read_primary_header(rom, level)?;
    let mut bus = SmwBus::new(rom);
    let mut cpu = Cpu::new();
    run_reset(&mut cpu, &mut bus, level)?;
    let run_jsr = |cpu: &mut Cpu, bus: &mut SmwBus, addr: u32| {
        cpu.p |= crate::cpu::Flags::M | crate::cpu::Flags::X;
        cpu.db = 0;
        cpu.call_jsr(bus, addr, STEP_LIMIT)
            .map_err(|source| ExpandError::Cpu { level, source })
    };
    // Boot-time VRAM state: the layer 3 tiles survive from the
    // "Nintendo Presents" screen through every level load.
    run_jsr(&mut cpu, &mut bus, routines::CLEAR_LAYER3)?;
    run_jsr(&mut cpu, &mut bus, routines::UPLOAD_LAYER3_GFX)?;
    if trace {
        cpu.trace_data_reads = Some(Vec::new());
    }
    let run = |cpu: &mut Cpu, bus: &mut SmwBus, addr: u32| {
        cpu.call(bus, addr, STEP_LIMIT)
            .map_err(|source| ExpandError::Cpu { level, source })
    };
    // Enter the level the way a screen exit on screen 0 would. The
    // overworld path cannot express every level number through `$0109`,
    // loads the "No Yoshi" entrance intro room for castle and ghost house
    // tilesets, and is rerouted by some Lunar Magic versions. The high
    // byte is given in both the vanilla form (the player's submap) and
    // Lunar Magic's exit table form.
    let (lo, hi) = (level as u8, (level >> 8) as u8);
    bus.set_wram_u8(ram::SUBLEVEL_COUNT, 1);
    bus.set_wram_u8(ram::EXIT_TABLE_LOW, lo);
    bus.set_wram_u8(ram::EXIT_TABLE_HIGH, 0x04 | hi);
    bus.set_wram_u8(ram::OW_PLAYER_SUBMAP, hi);
    // Run each phase with the game mode the real machine would be in.
    bus.set_wram_u8(ram::GAME_MODE, 0x11);
    run(&mut cpu, &mut bus, routines::LOAD_HEADER_POINTERS)?;
    // Game mode $11 seeds the camera update's previous positions from the
    // entrance and sets the maximum screen count before loading.
    for i in 0..ram::LAYER_POSITIONS_LEN {
        let value = bus.wram_u8(ram::LAYER1_X + i);
        bus.set_wram_u8(ram::NEXT_LAYER1_X + i, value);
    }
    bus.set_wram_u8(ram::LAST_SCREEN_HORIZ, 0x20);
    run(&mut cpu, &mut bus, routines::LOAD_LEVEL_DATA)?;
    // Boss preparation reuses the screen-count byte (level $1C7 ends
    // with $FF). Preserve the length while it still describes the grid.
    let screens = bus.wram_u8(ram::SCREENS) as usize;
    let vertical = bus.wram_u8(ram::SCREEN_MODE) & 0x01 != 0;
    let rows = level_rows(&bus, vertical);
    // Capture the background tilemap now: level preparation decompresses
    // GFX files into `$7EAD00`, and Lunar Magic's 4bpp files overrun the
    // vanilla 3bpp buffer into `$7EB900`. The game has already uploaded
    // the tilemap to VRAM by then, so it does not care; we do.
    let layer2_tilemap = match bus.wram_u8(ram::LEVEL_MODE) {
        0x00 | 0x0A | 0x0C | 0x0D | 0x0E | 0x11 | 0x1E => Some((
            bus.wram_slice(ram::LAYER2_TILEMAP_LOW, LAYER2_TILEMAP_LEN)
                .to_vec(),
            bus.wram_slice(ram::LAYER2_TILEMAP_HIGH, LAYER2_TILEMAP_LEN)
                .to_vec(),
        )),
        _ => None,
    };
    // The rest of game mode $11, then all of game mode $12: this is what
    // draws boss arenas, sets up layer 3, and uploads GFX and palettes.
    for i in 0..8 {
        bus.set_wram_u8(LOADED_GFX_FILES + i, 0xFF);
    }
    run_jsr(&mut cpu, &mut bus, routines::DECOMPRESS_PLAYER_GFX)?;
    run_jsr(&mut cpu, &mut bus, routines::INIT_LEVEL_RAM)?;
    run_jsr(&mut cpu, &mut bus, routines::INIT_LAYER2_SCROLL)?;
    // Game mode $11 then runs the camera update once, which places layer 2
    // for the entry camera. The game enables vertical scrolling at will
    // first; that is left off so the camera stays at the entrance instead
    // of starting to drift towards the player.
    run(&mut cpu, &mut bus, routines::UPDATE_CAMERA)?;
    bus.set_wram_u8(ram::GAME_MODE, 0x12);
    run_jsr(&mut cpu, &mut bus, routines::PREPARE_LEVEL)?;
    let boss_scene = capture_boss_scene(&mut bus, level)?;
    let trace = cpu.trace_data_reads.take();
    let read16 =
        |bus: &SmwBus, addr: u32| u16::from_le_bytes([bus.wram_u8(addr), bus.wram_u8(addr + 1)]);
    let camera = [read16(&bus, ram::LAYER1_X), read16(&bus, ram::LAYER1_Y)];
    let layer2_position = [read16(&bus, ram::LAYER2_X), read16(&bus, ram::LAYER2_Y)];
    let screen = crate::video::Screen {
        main: bus.wram_u8(ram::MAIN_SCREEN),
        sub: bus.wram_u8(ram::SUB_SCREEN),
        color_math: bus.wram_u8(ram::COLOR_MATH),
        math_select: bus.wram_u8(ram::COLOR_MATH_SELECT),
        fixed_color: crate::palette::Color15(u16::from_le_bytes([
            bus.wram_u8(ram::BACKGROUND_COLOR),
            bus.wram_u8(ram::BACKGROUND_COLOR + 1),
        ])),
    };
    let layer3 = if boss_scene.is_some() {
        None
    } else {
        capture_layer3(&mut cpu, &mut bus, level)?
    };
    let lunar_magic = rom.lunar_magic_version().is_some();
    let (bg_map16, layer2_screen_len) = match &layer2_tilemap {
        Some(planes) => read_bg_map16(&mut cpu, &mut bus, level, planes)?,
        None => (Vec::new(), SCREEN_LEN),
    };
    let map16 = lookup_map16(&mut cpu, &mut bus, level, lunar_magic)?;
    let pipe_map16 = (!lunar_magic).then(|| read_pipe_map16(&mut bus));
    let tiles = LevelTiles {
        level,
        header,
        level_mode: bus.wram_u8(ram::LEVEL_MODE),
        object_tileset: bus.wram_u8(ram::OBJECT_TILESET),
        vertical,
        screens,
        rows,
        low: bus.wram_slice(ram::TILES_LOW, GRID_LEN).to_vec(),
        high: bus.wram_slice(ram::TILES_HIGH, GRID_LEN).to_vec(),
        layer2_tilemap,
        layer2_screen_len,
        vram: bus.vram,
        vram_written: bus.vram_written,
        cgram: bus.cgram,
        bg_sc: bus.bg_sc,
        object_select: bus.object_select,
        boss_scene,
        layer3,
        screen,
        camera,
        layer2_position,
        wram: bus.wram,
        map16,
        pipe_map16,
        bg_map16,
    };
    Ok((tiles, trace))
}

/// Rows per screen of the loaded level. Lunar Magic 3's expanded level
/// format stores a horizontal level's height in `$13D7`; vanilla and
/// older Lunar Magic ROMs leave it zero. Anything that does not describe
/// whole rows fitting the planes is treated as the vanilla 27.
fn level_rows(bus: &SmwBus, vertical: bool) -> usize {
    if vertical {
        return 16;
    }
    let height = bus.wram_slice(ram::LEVEL_HEIGHT, 2);
    let height = height[0] as usize | ((height[1] as usize) << 8);
    match height / 16 {
        rows if height.is_multiple_of(16) && rows > 0 && rows * SCREEN_COLS <= GRID_LEN => rows,
        _ => SCREEN_ROWS,
    }
}

/// Screen size the sprite engine draws within.
const SCREEN_W: i32 = 256;
const SCREEN_H: i32 = 224;

/// Draws the level's sprites the way the game does: for each camera
/// position that puts a sprite entry's column at the screen edge, it
/// restores the loaded level, runs the ROM's own sprite loader (so custom
/// sprite tools' loaders and extension bytes apply), runs two frames of
/// the level loop (sprite initialisation, then the first drawing frame),
/// and reads OAM back in level coordinates. Mario is parked in the middle
/// of the screen with scrolling disabled, and his objects (the same on
/// every pass) are subtracted using a pass without sprites.
///
/// Boss arenas draw their sprites in `boss_scene` instead and get an empty
/// scene here.
pub fn capture_sprites(
    rom: &Rom,
    tiles: &LevelTiles,
    list: &crate::sprites::SpriteList,
) -> Result<crate::video::SpriteScene, ExpandError> {
    use crate::video::SpriteScene;
    use std::collections::{BTreeMap, HashSet};
    let level = tiles.level;
    if tiles.boss_scene.is_some() {
        return Ok(SpriteScene {
            object_select: tiles.object_select,
            ..Default::default()
        });
    }
    let mut bus = SmwBus::new(rom);
    bus.wram = tiles.wram.clone();
    bus.vram = tiles.vram.clone();
    bus.cgram = tiles.cgram.clone();
    bus.bg_sc = tiles.bg_sc;
    bus.object_select = tiles.object_select;
    // Start from an empty sprite table: the entrance screen's sprites were
    // spawned during level preparation, and each pass respawns what it
    // needs from the level data.
    for slot in 0..SPRITE_SLOTS {
        bus.set_wram_u8(ram::SPRITE_STATUS + slot, 0);
    }
    for i in 0..SPRITE_LOAD_FLAGS {
        bus.set_wram_u8(ram::SPRITE_LOAD_STATUS + i, 0);
    }
    let saved = bus.wram.clone();
    let (w, h) = tiles.size();
    let (level_w, level_h) = (w as i32 * 16, h as i32 * 16);
    // Camera position whose loading column is the entry's, keeping the
    // sprite inside the screen on the other axis.
    let camera = |e: &crate::sprites::SpriteEntry| -> (i32, i32) {
        let (x, y) = e.tile_position(tiles.vertical);
        let (x, y) = (x as i32 * 16, y as i32 * 16);
        if tiles.vertical {
            ((x - SCREEN_W / 2).clamp(0, (level_w - SCREEN_W).max(0)), y)
        } else {
            (x, (y - SCREEN_H / 2).clamp(0, (level_h - SCREEN_H).max(0)))
        }
    };
    let mut groups: BTreeMap<(i32, i32), Vec<&crate::sprites::SpriteEntry>> = BTreeMap::new();
    for e in &list.sprites {
        groups.entry(camera(e)).or_default().push(e);
    }
    let sizes = object_sizes(tiles.object_select);
    // Runs one pass: restore the level, place the camera and player, spawn
    // the column's sprites (unless `frames` fixes the frame count for a
    // player-only baseline), and run frames until every spawned slot has
    // left its initialisation state and drawn once. Returns the final OAM
    // and the number of frames run.
    let run_pass = |bus: &mut SmwBus,
                    cam: (i32, i32),
                    frames: Option<usize>|
     -> Result<(Vec<u8>, usize), ExpandError> {
        bus.wram.copy_from_slice(&saved);
        let set16 = |bus: &mut SmwBus, addr: u32, v: i32| {
            bus.set_wram_u8(addr, v as u8);
            bus.set_wram_u8(addr + 1, (v >> 8) as u8);
        };
        for addr in [ram::LAYER1_X, ram::NEXT_LAYER1_X] {
            set16(bus, addr, cam.0);
        }
        for addr in [ram::LAYER1_Y, ram::NEXT_LAYER1_Y] {
            set16(bus, addr, cam.1);
        }
        // Mario waits just off the left edge, where most sprites expect
        // to meet him (a Banzai Bill erases itself if he is to its right)
        // and where his own objects stay out of OAM.
        set16(bus, ram::PLAYER_X, cam.0 - 64);
        set16(bus, ram::PLAYER_Y, cam.1 + SCREEN_H / 2 - 16);
        bus.set_wram_u8(ram::LAYER1_SCROLL_DIR, 1);
        bus.set_wram_u8(ram::HORIZ_SCROLL_SETTING, 0);
        bus.set_wram_u8(ram::VERT_SCROLL_SETTING, 0);
        bus.set_wram_u8(ram::GAME_MODE, 0x14);
        let status = |bus: &SmwBus| {
            bus.wram_slice(ram::SPRITE_STATUS, SPRITE_SLOTS as usize)
                .to_vec()
        };
        let mut spawned = Vec::new();
        if frames.is_none() {
            // The loader reads its slot tables through the data bank its
            // bank 2 callers set.
            let mut cpu = Cpu::new();
            cpu.db = 0x02;
            cpu.call_jsr(bus, routines::SPAWN_SPRITES, STEP_LIMIT)
                .map_err(|source| ExpandError::Cpu { level, source })?;
            spawned = status(bus)
                .iter()
                .enumerate()
                .filter(|(_, s)| **s != 0)
                .map(|(slot, _)| slot)
                .collect();
        }
        // Initialisation takes the first frame and drawing the second;
        // some sprites wait a few more frames before they first appear.
        let mut oam = Vec::new();
        let mut run = 0;
        for frame in 0..frames.unwrap_or(SPRITE_FRAMES) {
            let mut cpu = Cpu::new();
            cpu.call_jsr(bus, routines::DRAW_LEVEL_FRAME, STEP_LIMIT)
                .map_err(|source| ExpandError::Cpu { level, source })?;
            let first = bus.wram_u8(ram::OAM_ADDRESS) as usize / 2;
            oam = bus.wram_slice(ram::OAM, 0x240).to_vec();
            oam.push(first as u8);
            run = frame + 1;
            let status = status(bus);
            if frames.is_none() && frame >= 1 && spawned.iter().all(|&slot| status[slot] != 1) {
                break;
            }
        }
        Ok((oam, run))
    };
    let mut seen = HashSet::new();
    let mut scene = SpriteScene {
        object_select: tiles.object_select,
        ..Default::default()
    };
    for (cam, entries) in &groups {
        let (oam, frames) = run_pass(&mut bus, *cam, None)?;
        // The player's objects after the same frames from the same spot.
        let (baseline, _) = run_pass(&mut bus, *cam, Some(frames))?;
        let player: HashSet<_> = screen_objects(&baseline, sizes).into_iter().collect();
        let mut drew = false;
        for (sx, sy, tile, attr, large) in screen_objects(&oam, sizes) {
            if player.contains(&(sx, sy, tile, attr, large)) {
                continue;
            }
            drew = true;
            let object = crate::video::SpriteObject {
                x: cam.0 + sx,
                y: cam.1 + sy,
                tile,
                attr,
                large,
            };
            if seen.insert(object) {
                scene.objects.push(object);
            }
        }
        if !drew {
            for e in entries {
                let (x, y) = e.tile_position(tiles.vertical);
                scene.undrawn.push((x, y, e.id));
            }
        }
    }
    Ok(scene)
}

/// Small and large object dimensions for an `OBSEL` value.
pub fn object_sizes(object_select: u8) -> [(i32, i32); 2] {
    [
        [(8, 8), (16, 16)],
        [(8, 8), (32, 32)],
        [(8, 8), (64, 64)],
        [(16, 16), (32, 32)],
        [(16, 16), (64, 64)],
        [(32, 32), (64, 64)],
        [(16, 32), (32, 64)],
        [(16, 32), (32, 32)],
    ][(object_select >> 5) as usize]
}

/// Visible objects in a packed OAM image (512 bytes of objects, 32 bytes
/// of size and X bits, then the index of the first object), front to
/// back, as (x, y, tile, attribute, large) in screen coordinates. Y `$F0`
/// is the game's hidden marker; objects entirely off the screen are
/// dropped, and those wrapped past its bottom are read as negative.
fn screen_objects(oam: &[u8], sizes: [(i32, i32); 2]) -> Vec<(i32, i32, u8, u8, bool)> {
    let first = oam[0x240] as usize;
    let mut out = Vec::new();
    for offset in 0..128 {
        let object = (first + offset) % 128;
        let bytes = &oam[object * 4..object * 4 + 4];
        let high = oam[512 + object / 4] >> (2 * (object % 4));
        let large = high & 2 != 0;
        let (width, height) = sizes[large as usize];
        let x = bytes[0] as i32 - if high & 1 != 0 { 256 } else { 0 };
        let y = bytes[1];
        if y == 0xF0 {
            continue;
        }
        let y = if y as i32 >= SCREEN_H {
            y as i32 - 256
        } else {
            y as i32
        };
        if x + width <= 0 || x >= SCREEN_W || y + height <= 0 || y >= SCREEN_H {
            continue;
        }
        out.push((x, y, bytes[2], bytes[3], large));
    }
    out
}

/// Run just the video-register portions of the boss interrupt handlers.
/// The ROM chooses its tilemap, graphics base, Mode 7 transform, and IRQ
/// scanlines. One drawing pass populates the sprite-based arena artwork;
/// its RAM changes are isolated from the captured collision grid.
fn capture_boss_scene(
    bus: &mut SmwBus,
    level: u16,
) -> Result<Option<crate::video::BossScene>, ExpandError> {
    use crate::video::{Band, BossScene, Layer1};
    let command = bus.wram[0x0D9B];
    if command & 0x80 == 0 {
        return Ok(None);
    }
    let saved_ram = bus.wram.clone();
    let state = |bus: &SmwBus| Layer1 {
        mode: bus.bg_mode,
        tilemap: bus.bg_sc[0],
        character_base: bus.bg_character_base[0],
        scroll: bus.bg_scroll[0],
        mode7: bus.mode7,
    };
    let mut cpu = Cpu::new();
    bus.wram[0x0100] = 0x14;
    cpu.call_jsr(bus, routines::DRAW_LEVEL_FRAME, STEP_LIMIT)
        .map_err(|source| ExpandError::Cpu { level, source })?;
    let oam = bus.wram[0x0200..0x0420].to_vec();
    let first_object = bus.wram[0x3F] as usize / 2;
    for routine in [routines::UPLOAD_PLAYER_TILES, routines::UPLOAD_BOSS_TILES] {
        if routine == routines::UPLOAD_BOSS_TILES && command & 0x40 == 0 {
            continue;
        }
        cpu = Cpu::new();
        cpu.call_jsr(bus, routine, STEP_LIMIT)
            .map_err(|source| ExpandError::Cpu { level, source })?;
    }
    cpu = Cpu::new();
    let run = |cpu: &mut Cpu, bus: &mut SmwBus, start, stop| {
        cpu.run_until(bus, start, stop, 100_000)
            .map_err(|source| ExpandError::Cpu { level, source })
    };
    let stop = if command & 1 != 0 {
        routines::EXIT_IRQ
    } else {
        routines::SET_STATUS_BAR_IRQ
    };
    run(&mut cpu, bus, routines::MODE7_NMI_REGISTERS, stop)?;
    let mut bands = vec![Band {
        start: 0,
        layer: state(bus),
    }];
    if command & 1 == 0 {
        // At the first stop Y holds the status-bar/ceiling IRQ line.
        let first_line = cpu.y as usize;
        bus.wram[0x11] = 0;
        cpu.a = 0x81;
        run(&mut cpu, bus, routines::BOSS_IRQ, routines::EXIT_IRQ)?;
        bands.push(Band {
            start: first_line,
            layer: state(bus),
        });
        if bus.interrupt_enable & 0x20 != 0 {
            let floor_line = bus.irq_scanline as usize;
            cpu.a = 0x81;
            run(&mut cpu, bus, routines::BOSS_IRQ, routines::EXIT_IRQ)?;
            bands.push(Band {
                start: floor_line,
                layer: state(bus),
            });
        }
    }
    let backdrop_window = bus.wram[0x04A0..0x04A0 + 224 * 2]
        .as_chunks::<2>()
        .0
        .to_vec();
    bus.wram = saved_ram;
    Ok(Some(BossScene {
        bands,
        backdrop_window,
        oam,
        object_select: bus.object_select,
        first_object,
    }))
}

/// Layer 3 as level preparation left it, plus how it follows the camera.
/// The latter is measured rather than decoded: the ROM's per-frame layer
/// scroll routine runs three times from the prepared state, with the
/// camera where it is and then moved 16 pixels along each axis, the way
/// the camera update hands it a frame's movement. Tides, the tileset
/// backgrounds' half-speed parallax, autoscrolling fish, sprite-driven
/// layers, and custom scroll code hooked into the routine all come out
/// of the same measurement. `None` when layer 3 is not shown on either
/// screen in Mode 1.
fn capture_layer3(
    cpu: &mut Cpu,
    bus: &mut SmwBus,
    level: u16,
) -> Result<Option<crate::video::Layer3>, ExpandError> {
    let bg_mode = bus.wram_u8(ram::BG_MODE);
    let shown = bus.wram_u8(ram::MAIN_SCREEN) | bus.wram_u8(ram::SUB_SCREEN);
    if bg_mode & 0x07 != 1 || shown & 0x04 == 0 {
        return Ok(None);
    }
    let read16 =
        |bus: &SmwBus, addr: u32| u16::from_le_bytes([bus.wram_u8(addr), bus.wram_u8(addr + 1)]);
    let position = [read16(bus, ram::LAYER3_X), read16(bus, ram::LAYER3_Y)];
    let camera = [read16(bus, ram::LAYER1_X), read16(bus, ram::LAYER1_Y)];
    let saved = bus.wram.clone();
    let scrolled = |cpu: &mut Cpu, bus: &mut SmwBus, delta: [u16; 2]| {
        bus.wram.copy_from_slice(&saved);
        // The entrance locks sprites, which also pauses autoscroll.
        bus.set_wram_u8(ram::SPRITE_LOCK, 0);
        for axis in 0..2 {
            let moved = camera[axis].wrapping_add(delta[axis]).to_le_bytes();
            for addr in [
                [ram::LAYER1_X, ram::LAYER1_Y][axis],
                [ram::NEXT_LAYER1_X, ram::NEXT_LAYER1_Y][axis],
            ] {
                bus.set_wram_u8(addr, moved[0]);
                bus.set_wram_u8(addr + 1, moved[1]);
            }
            bus.set_wram_u8([ram::LAYER1_DX, ram::LAYER1_DY][axis], delta[axis] as u8);
        }
        cpu.p |= crate::cpu::Flags::M | crate::cpu::Flags::X;
        cpu.db = 0;
        cpu.dp = 0;
        cpu.call(bus, routines::SCROLL_LAYERS, STEP_LIMIT)
            .map_err(|source| ExpandError::Cpu { level, source })?;
        Ok::<_, ExpandError>([read16(bus, ram::LAYER3_X), read16(bus, ram::LAYER3_Y)])
    };
    let still = scrolled(cpu, bus, [0, 0])?;
    let moved_x = scrolled(cpu, bus, [16, 0])?;
    let moved_y = scrolled(cpu, bus, [0, 16])?;
    bus.wram = saved;
    Ok(Some(crate::video::Layer3 {
        position,
        camera,
        scroll_per_16: [
            moved_x[0].wrapping_sub(still[0]) as i16 as i32,
            moved_y[1].wrapping_sub(still[1]) as i16 as i32,
        ],
        tilemap: bus.bg_sc[2],
        character_base: bus.bg_character_base[2],
        bg_mode,
    }))
}

/// Where the game reads BG Map16 tile definitions from for the loaded
/// level, and the background's bytes per screen: the vanilla table and
/// `$1B0`, or whatever the routine Lunar Magic hooked into the layer 2
/// tilemap upload leaves in `$0A`-`$0C` and `$05`-`$06`. Runs after the
/// loader so the routine sees the level's Lunar Magic flags.
fn bg_map16_base(cpu: &mut Cpu, bus: &mut SmwBus, level: u16) -> Result<(u32, usize), ExpandError> {
    let hook: Vec<u8> = (0..4)
        .map(|i| bus.read(routines::BG_MAP16_BASE_HOOK + i))
        .collect();
    let [0x22, lo, hi, bank] = hook[..] else {
        return Ok((map16::tables::MAP16_BG_TILES.raw(), SCREEN_LEN));
    };
    let target = lo as u32 | ((hi as u32) << 8) | ((bank as u32) << 16);
    // The caller has a 16-bit accumulator and 8-bit index registers.
    cpu.p &= !crate::cpu::Flags::M;
    cpu.p |= crate::cpu::Flags::X;
    cpu.db = 0;
    cpu.dp = 0;
    cpu.call(bus, target, 100_000)
        .map_err(|source| ExpandError::Cpu { level, source })?;
    let base = bus.wram_slice(ram::BG_MAP16_BASE, 3);
    let base = base[0] as u32 | ((base[1] as u32) << 8) | ((base[2] as u32) << 16);
    if base == 0 || base == 0xFF_FFFF {
        return Err(ExpandError::MissingBackgroundTable(level));
    }
    let len = bus.wram_slice(ram::BG_SCREEN_LEN, 2);
    let len = len[0] as usize | ((len[1] as usize) << 8);
    if len != SCREEN_LEN && len != LM_TALL_SCREEN_LEN {
        return Err(ExpandError::BackgroundLayout { level, len });
    }
    Ok((base, len))
}

/// Background Map16 definitions, including any Lunar Magic indices above
/// the vanilla 0x200 tiles, and the background's bytes per screen.
fn read_bg_map16(
    cpu: &mut Cpu,
    bus: &mut SmwBus,
    level: u16,
    planes: &(Vec<u8>, Vec<u8>),
) -> Result<(Vec<Map16Tile>, usize), ExpandError> {
    let (base, screen_len) = bg_map16_base(cpu, bus, level)?;
    let tile_count = planes
        .0
        .iter()
        .zip(&planes.1)
        .take(2 * screen_len)
        .map(|(&lo, &hi)| lo as usize + ((hi as usize) << 8) + 1)
        .max()
        .unwrap_or(0)
        .max(map16::BG_TILE_COUNT);
    let tiles = (0..tile_count as u32)
        .map(|n| {
            let mut b = [0u8; 8];
            for (i, byte) in b.iter_mut().enumerate() {
                *byte = bus.read(base.wrapping_add(8 * n + i as u32));
            }
            Map16Tile::from_bytes(b)
        })
        .collect();
    Ok((tiles, screen_len))
}

/// Reads a Map16 definition from ROM.
fn read_map16(bus: &mut SmwBus, ptr: u32) -> Map16Tile {
    let mut b = [0u8; 8];
    for (i, byte) in b.iter_mut().enumerate() {
        *byte = bus.read(ptr.wrapping_add(i as u32));
    }
    Map16Tile::from_bytes(b)
}

/// Resolves the Map16 definition of every tile number present in the
/// loaded level's object grid, the way the level's tilemap upload does.
/// Vanilla reads pages 0 and 1 through the pointer table the loader built
/// in RAM; Lunar Magic replaces that lookup (`$058A65`) with a call to its
/// foreground pointer routine for every tile number, which is also the only
/// way to reach pages 2 and up. Background definitions stay separate:
/// their tile numbers overlap foreground pages 2 and 3.
fn lookup_map16(
    cpu: &mut Cpu,
    bus: &mut SmwBus,
    level: u16,
    lunar_magic: bool,
) -> Result<HashMap<u16, Map16Tile>, ExpandError> {
    let mut numbers: Vec<u16> = (0..GRID_LEN)
        .map(|i| {
            bus.wram[(ram::TILES_LOW - 0x7E_0000) as usize + i] as u16
                | ((bus.wram[(ram::TILES_HIGH - 0x7E_0000) as usize + i] as u16) << 8)
        })
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    let mut out = HashMap::with_capacity(numbers.len());
    for n in numbers {
        let ptr: Option<u32> = if lunar_magic {
            cpu.p &= !(crate::cpu::Flags::M | crate::cpu::Flags::X);
            cpu.a = n.wrapping_mul(2);
            cpu.db = 0;
            cpu.dp = 0;
            cpu.call(bus, routines::LM_MAP16_POINTER, 100_000)
                .map_err(|source| ExpandError::Cpu { level, source })?;
            let bank = bus.wram_u8(ram::LM_MAP16_BANK) as u32;
            Some((bank << 16) | cpu.a as u32)
        } else if n < 0x200 {
            let i = (ram::MAP16_POINTERS - 0x7E_0000) as usize + 2 * n as usize;
            Some(0x0D_0000 | bus.wram[i] as u32 | ((bus.wram[i + 1] as u32) << 8))
        } else {
            None
        };
        if let Some(ptr) = ptr {
            out.insert(n, read_map16(bus, ptr));
        }
    }
    Ok(out)
}

/// The four position-dependent definitions of each vertical pipe tile,
/// from `MAP16AppTable`.
fn read_pipe_map16(bus: &mut SmwBus) -> [[Map16Tile; PIPE_TILE_COUNT]; PIPE_VARIANTS] {
    std::array::from_fn(|variant| {
        let entry = routines::PIPE_POINTER_TABLE + 2 * variant as u32;
        let base = 0x0D_0000 | bus.read(entry) as u32 | ((bus.read(entry + 1) as u32) << 8);
        std::array::from_fn(|tile| read_map16(bus, base + 8 * tile as u32))
    })
}

#[cfg(test)]
mod sprite_capture_tests {
    use super::*;

    fn oam_with(objects: &[(usize, u8, u8, u8, u8, u8)], first: u8) -> Vec<u8> {
        let mut oam = vec![0u8; 0x240];
        for i in 0..128 {
            oam[i * 4 + 1] = 0xF0;
        }
        for &(i, x, y, tile, attr, high) in objects {
            oam[i * 4..i * 4 + 4].copy_from_slice(&[x, y, tile, attr]);
            oam[512 + i / 4] |= high << (2 * (i % 4));
        }
        oam.push(first);
        oam
    }

    #[test]
    fn hidden_and_offscreen_objects_are_dropped() {
        let sizes = object_sizes(0x60); // 16x16 and 32x32
        let oam = oam_with(
            &[
                (0, 10, 20, 0x40, 0x30, 2),   // large, visible
                (1, 10, 0xF0, 0x40, 0x30, 2), // hidden marker
                (2, 0xF8, 30, 0x41, 0x00, 1), // x = -8, small: 8 px visible
                (3, 0xF0, 30, 0x41, 0x00, 1), // x = -16, small: gone
                (4, 0, 0xF8, 0x42, 0x00, 2),  // y = -8, large: visible
                (5, 0, 0xE0, 0x42, 0x00, 0),  // y = 224: below the screen
            ],
            0,
        );
        let got = screen_objects(&oam, sizes);
        assert_eq!(
            got,
            [
                (10, 20, 0x40, 0x30, true),
                (-8, 30, 0x41, 0x00, false),
                (0, -8, 0x42, 0x00, true),
            ]
        );
    }

    #[test]
    fn objects_start_from_the_first_written_one() {
        let sizes = object_sizes(0x03); // 8x8 and 16x16, what SMW uses
        assert_eq!(sizes, [(8, 8), (16, 16)]);
        let oam = oam_with(&[(0, 1, 1, 1, 0, 0), (100, 2, 2, 2, 0, 0)], 100);
        let got: Vec<u8> = screen_objects(&oam, sizes).iter().map(|o| o.2).collect();
        assert_eq!(got, [2, 1]);
    }

    #[test]
    fn level_rows_come_from_the_reported_height() {
        let mut bytes = vec![0; 0x8000];
        bytes[0x7FD5] = 0x20; // LoROM
        let rom = crate::Rom::from_bytes(bytes).unwrap();
        let mut bus = SmwBus::new(&rom);
        assert_eq!(level_rows(&bus, false), SCREEN_ROWS);
        assert_eq!(level_rows(&bus, true), 16);
        bus.set_wram_u8(ram::LEVEL_HEIGHT, 0x80);
        bus.set_wram_u8(ram::LEVEL_HEIGHT + 1, 0x02);
        assert_eq!(level_rows(&bus, false), 40);
        bus.set_wram_u8(ram::LEVEL_HEIGHT, 0x88); // not whole rows
        assert_eq!(level_rows(&bus, false), SCREEN_ROWS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr::{Mapping, SnesAddr};

    #[test]
    fn lunar_magic_foreground_pages_two_and_three_use_pointer_routine() {
        let mut bytes = vec![0; 0x10_0000];
        let mut put = |addr: u32, data: &[u8]| {
            let at = Mapping::LoRom
                .snes_to_pc(SnesAddr::new(addr))
                .unwrap()
                .as_usize();
            bytes[at..at + data.len()].copy_from_slice(data);
        };
        put(0x00_FFD5, &[0x20]);
        // Synthetic pointer routine: input A = tile * 2, return
        // $10:(tile * 8 + $8000), with the bank in direct page $0C.
        put(
            routines::LM_MAP16_POINTER,
            &[
                0x0A, 0x0A, // ASL : ASL
                0x18, 0x69, 0x00, 0x80, // CLC : ADC #$8000
                0x48, 0xA9, 0x10, 0x00, // PHA : LDA #$0010
                0x85, 0x0C, 0x68, 0x6B, // STA $0C : PLA : RTL
            ],
        );
        let page_two = Map16Tile::from_bytes([1, 0, 2, 0, 3, 0, 4, 0]);
        let page_three = Map16Tile::from_bytes([5, 0, 6, 0, 7, 0, 8, 0]);
        put(0x10_9000, &page_two.to_bytes());
        put(0x10_9800, &page_three.to_bytes());
        let rom = Rom::from_bytes(bytes).unwrap();
        let mut bus = SmwBus::new(&rom);
        bus.set_wram_u8(ram::TILES_HIGH, 2);
        bus.set_wram_u8(ram::TILES_HIGH + 1, 3);
        let result = lookup_map16(&mut Cpu::new(), &mut bus, 0x105, true).unwrap();
        assert_eq!(result[&0x200], page_two);
        assert_eq!(result[&0x300], page_three);
    }
}
