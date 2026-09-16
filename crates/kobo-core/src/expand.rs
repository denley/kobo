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
    pub const OVERWORLD_OVERRIDE: u32 = 0x7E_0109;
    pub const OW_PLAYER_SUBMAP: u32 = 0x7E_1F11;
    pub const LAST_SCREEN_HORIZ: u32 = 0x7E_005E;
    pub const SCREEN_MODE: u32 = 0x7E_005B;
    pub const LEVEL_MODE: u32 = 0x7E_1925;
    pub const SCREENS: u32 = 0x7E_005D;
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
}

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
    /// `CODE_0098A9`: uploads the boss's graphics to VRAM.
    pub const UPLOAD_BOSS_TILES: u32 = 0x00_98A9;
    /// `CODE_00A300`: uploads the player's graphics to VRAM.
    pub const UPLOAD_PLAYER_TILES: u32 = 0x00_A300;
}

/// `$0101`-`$0108`: the GFX files currently in VRAM. `$FF` forces uploads.
const LOADED_GFX_FILES: u32 = 0x7E_0101;

const STEP_LIMIT: u64 = 200_000_000;

#[derive(Debug, Error)]
pub enum ExpandError {
    #[error(transparent)]
    Level(#[from] LevelError),
    #[error("level {0:03X} cannot be selected through the overworld override")]
    Unreachable(u16),
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
    pub low: Vec<u8>,
    pub high: Vec<u8>,
    /// All of work RAM after the loader ran, for inspection.
    pub wram: Vec<u8>,
    /// Foreground Map16 definitions for tile numbers in the object grid.
    /// Lunar Magic pages 2 and 3 are distinct from the same-numbered BG
    /// tiles, which live in `bg_map16`.
    pub map16: HashMap<u16, Map16Tile>,
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
    /// Video-mode bands installed by the ROM's boss NMI/IRQ handlers.
    pub boss_scene: Option<crate::video::BossScene>,
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
        let i = screen * SCREEN_LEN + y * SCREEN_COLS + x;
        self.low[i] as u16 | ((self.high[i] as u16) << 8)
    }

    /// Buffer offset of a level-wide tile position, for either orientation.
    pub fn offset(&self, x: usize, y: usize) -> usize {
        if self.vertical {
            (y / 16) * 0x200 + (x / 16) * 0x100 + (y % 16) * 16 + (x % 16)
        } else {
            (x / SCREEN_COLS) * SCREEN_LEN + y * SCREEN_COLS + (x % SCREEN_COLS)
        }
    }

    /// Map16 tile number at a level-wide position.
    pub fn tile_at(&self, x: usize, y: usize) -> u16 {
        let i = self.offset(x, y);
        self.low[i] as u16 | ((self.high[i] as u16) << 8)
    }

    /// How this level's layer 2 objects are laid out, if it has any.
    pub fn layer2_objects(&self) -> Option<Layer2Objects> {
        if self.layer2_tilemap.is_some() {
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
                self.screens.min(len / SCREEN_LEN) * SCREEN_COLS,
                SCREEN_ROWS,
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

/// The `$0109` value that selects `level`, and the high-byte flag.
///
/// Zero means "no override", so levels `000` and `100` cannot be selected
/// this way, nor can low bytes `$DC` and above.
pub fn override_for(level: u16) -> Option<(u8, u8)> {
    let lo = level & 0xFF;
    let hi = (level >> 8) as u8;
    let v = if lo < 0x25 { lo } else { lo + 0x24 };
    (lo != 0 && v <= 0xFF).then_some((v as u8, hi))
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
    let (ovr, hi) = override_for(level).ok_or(ExpandError::Unreachable(level))?;
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
    bus.set_wram_u8(ram::OVERWORLD_OVERRIDE, ovr);
    bus.set_wram_u8(ram::OW_PLAYER_SUBMAP, hi);
    // Run each phase with the game mode the real machine would be in.
    bus.set_wram_u8(ram::GAME_MODE, 0x11);
    run(&mut cpu, &mut bus, routines::LOAD_HEADER_POINTERS)?;
    // Game mode $11 sets the maximum screen count before loading.
    bus.set_wram_u8(ram::LAST_SCREEN_HORIZ, 0x20);
    run(&mut cpu, &mut bus, routines::LOAD_LEVEL_DATA)?;
    // Boss preparation reuses the screen-count byte (level $1C7 ends
    // with $FF). Preserve the length while it still describes the grid.
    let screens = bus.wram_u8(ram::SCREENS) as usize;
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
    bus.set_wram_u8(ram::GAME_MODE, 0x12);
    run_jsr(&mut cpu, &mut bus, routines::PREPARE_LEVEL)?;
    let boss_scene = capture_boss_scene(&mut bus, level)?;
    let trace = cpu.trace_data_reads.take();
    let lunar_magic = rom.lunar_magic_version().is_some();
    let (bg_map16, layer2_screen_len) = match &layer2_tilemap {
        Some(planes) => read_bg_map16(&mut cpu, &mut bus, level, planes)?,
        None => (Vec::new(), SCREEN_LEN),
    };
    let map16 = lookup_map16(&mut cpu, &mut bus, level, lunar_magic)?;
    let tiles = LevelTiles {
        level,
        header,
        level_mode: bus.wram_u8(ram::LEVEL_MODE),
        object_tileset: bus.wram_u8(ram::OBJECT_TILESET),
        vertical: bus.wram_u8(ram::SCREEN_MODE) & 0x01 != 0,
        screens,
        low: bus.wram_slice(ram::TILES_LOW, GRID_LEN).to_vec(),
        high: bus.wram_slice(ram::TILES_HIGH, GRID_LEN).to_vec(),
        layer2_tilemap,
        layer2_screen_len,
        vram: bus.vram,
        vram_written: bus.vram_written,
        cgram: bus.cgram,
        bg_sc: bus.bg_sc,
        boss_scene,
        wram: bus.wram,
        map16,
        bg_map16,
    };
    Ok((tiles, trace))
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

/// Resolves the Map16 definition of every tile number present in the
/// loaded level's object grid. Pages 0 and 1 come from the pointer table
/// the loader built in RAM, and all higher pages from Lunar Magic's
/// foreground pointer routine. Background definitions stay separate:
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
        let ptr: Option<u32> = if n < 0x200 {
            let i = (ram::MAP16_POINTERS - 0x7E_0000) as usize + 2 * n as usize;
            Some(0x0D_0000 | bus.wram[i] as u32 | ((bus.wram[i + 1] as u32) << 8))
        } else if lunar_magic {
            cpu.p &= !(crate::cpu::Flags::M | crate::cpu::Flags::X);
            cpu.a = n.wrapping_mul(2);
            cpu.db = 0;
            cpu.dp = 0;
            cpu.call(bus, routines::LM_MAP16_POINTER, 100_000)
                .map_err(|source| ExpandError::Cpu { level, source })?;
            let bank = bus.wram_u8(ram::LM_MAP16_BANK) as u32;
            Some((bank << 16) | cpu.a as u32)
        } else {
            None
        };
        if let Some(ptr) = ptr {
            let mut b = [0u8; 8];
            for (i, byte) in b.iter_mut().enumerate() {
                *byte = bus.read(ptr.wrapping_add(i as u32));
            }
            out.insert(n, Map16Tile::from_bytes(b));
        }
    }
    Ok(out)
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
