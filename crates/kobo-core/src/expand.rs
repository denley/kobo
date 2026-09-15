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
    pub const TILES_LOW: u32 = 0x7E_C800;
    pub const TILES_HIGH: u32 = 0x7F_C800;
    pub const LAYER2_TILEMAP_LOW: u32 = 0x7E_B900;
    pub const LAYER2_TILEMAP_HIGH: u32 = 0x7E_BD00;
    pub const BACKGROUND_COLOR: u32 = 0x7E_0701;
    /// 0x200 two-byte pointers into bank `$0D`, built by the level loader.
    pub const MAP16_POINTERS: u32 = 0x7E_0FBE;
    /// Direct page `$0C`: bank byte of the pointer Lunar Magic's routine returns.
    pub const LM_MAP16_BANK: u32 = 0x7E_000C;
    /// Direct page `$CE`-`$D0`: the level's sprite data pointer.
    pub const SPRITE_DATA_PTR: u32 = 0x7E_00CE;
}

/// Bytes per plane of the layer 2 background tilemap buffer.
pub const LAYER2_TILEMAP_LEN: usize = 0x400;

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
    /// True for vertical levels.
    pub vertical: bool,
    pub screens: usize,
    pub low: Vec<u8>,
    pub high: Vec<u8>,
    /// All of work RAM after the loader ran, for inspection.
    pub wram: Vec<u8>,
    /// Map16 definitions for every tile number the level uses.
    pub map16: HashMap<u16, Map16Tile>,
    /// VRAM as uploaded by level preparation: layer tiles at `$0000`,
    /// sprite tiles at `$C000`, tilemaps in between.
    pub vram: Vec<u8>,
    /// CGRAM as uploaded by level preparation.
    pub cgram: Vec<u8>,
    /// Layer 2 background tilemap planes, when the level uses a
    /// pre-built background instead of layer 2 objects. Tile numbers
    /// index the BG half of the Map16 table (`0x200` upwards).
    pub layer2_tilemap: Option<(Vec<u8>, Vec<u8>)>,
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

    /// Map16 tile number (BG numbering, `0x200` upwards) of a layer 2
    /// object at a horizontal-level position. Layer 2 objects occupy
    /// screens `0x10` and up of the buffer, so at most 16 screens exist.
    pub fn layer2_object_tile(&self, x: usize, y: usize) -> Option<u16> {
        if self.vertical || self.layer2_tilemap.is_some() || x / SCREEN_COLS >= 16 {
            return None;
        }
        let i = (x / SCREEN_COLS + 16) * SCREEN_LEN + y * SCREEN_COLS + (x % SCREEN_COLS);
        Some(0x200 | self.low[i] as u16 | ((self.high[i] as u16) << 8))
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

    /// Width and height of the level in tiles.
    pub fn size(&self) -> (usize, usize) {
        if self.vertical {
            (32, self.screens * 16)
        } else {
            (self.screens * SCREEN_COLS, SCREEN_ROWS)
        }
    }

    /// Map16 tile number (BG numbering, `0x200` upwards) at a position in
    /// the layer 2 background tilemap, which is two screens of 16 by 27
    /// tiles laid out like the main buffer. Returns `None` for levels
    /// whose layer 2 is objects.
    pub fn layer2_bg_tile(&self, screen: usize, x: usize, y: usize) -> Option<u16> {
        let (lo, hi) = self.layer2_tilemap.as_ref()?;
        let i = (screen % 2) * SCREEN_LEN + y * SCREEN_COLS + x;
        Some(0x200 | lo[i] as u16 | ((hi[i] as u16) << 8))
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
    if trace {
        cpu.trace_data_reads = Some(Vec::new());
    }
    let run = |cpu: &mut Cpu, bus: &mut SmwBus, addr: u32| {
        cpu.call(bus, addr, STEP_LIMIT)
            .map_err(|source| ExpandError::Cpu { level, source })
    };
    bus.set_wram_u8(ram::OVERWORLD_OVERRIDE, ovr);
    bus.set_wram_u8(ram::OW_PLAYER_SUBMAP, hi);
    run(&mut cpu, &mut bus, routines::LOAD_HEADER_POINTERS)?;
    // Game mode $11 sets the maximum screen count before loading.
    bus.set_wram_u8(ram::LAST_SCREEN_HORIZ, 0x20);
    run(&mut cpu, &mut bus, routines::LOAD_LEVEL_DATA)?;
    // The rest of game mode $11, then all of game mode $12: this is what
    // draws boss arenas, sets up layer 3, and uploads GFX and palettes.
    let run_jsr = |cpu: &mut Cpu, bus: &mut SmwBus, addr: u32| {
        cpu.p |= crate::cpu::Flags::M | crate::cpu::Flags::X;
        cpu.db = 0;
        cpu.call_jsr(bus, addr, STEP_LIMIT)
            .map_err(|source| ExpandError::Cpu { level, source })
    };
    for i in 0..8 {
        bus.set_wram_u8(LOADED_GFX_FILES + i, 0xFF);
    }
    run_jsr(&mut cpu, &mut bus, routines::DECOMPRESS_PLAYER_GFX)?;
    run_jsr(&mut cpu, &mut bus, routines::INIT_LEVEL_RAM)?;
    run_jsr(&mut cpu, &mut bus, routines::INIT_LAYER2_SCROLL)?;
    run_jsr(&mut cpu, &mut bus, routines::PREPARE_LEVEL)?;
    let layer2_tilemap = match level::layer2_ptr(rom, level)? {
        level::Layer2Data::Tilemap(_) => Some((
            bus.wram_slice(ram::LAYER2_TILEMAP_LOW, LAYER2_TILEMAP_LEN)
                .to_vec(),
            bus.wram_slice(ram::LAYER2_TILEMAP_HIGH, LAYER2_TILEMAP_LEN)
                .to_vec(),
        )),
        level::Layer2Data::Objects(_) => None,
    };
    let trace = cpu.trace_data_reads.take();
    let lunar_magic = rom.lunar_magic_version().is_some();
    let map16 = lookup_map16(&mut cpu, &mut bus, level, lunar_magic)?;
    let tiles = LevelTiles {
        level,
        header,
        level_mode: bus.wram_u8(ram::LEVEL_MODE),
        vertical: bus.wram_u8(ram::SCREEN_MODE) & 0x01 != 0,
        screens: bus.wram_u8(ram::SCREENS) as usize,
        low: bus.wram_slice(ram::TILES_LOW, GRID_LEN).to_vec(),
        high: bus.wram_slice(ram::TILES_HIGH, GRID_LEN).to_vec(),
        layer2_tilemap,
        vram: bus.vram,
        cgram: bus.cgram,
        wram: bus.wram,
        map16,
    };
    Ok((tiles, trace))
}

/// Resolves the Map16 definition of every tile number present in the
/// loaded level's buffers. Pages 0 and 1 come from the pointer table the
/// loader built in RAM, pages 2 and 3 from the vanilla BG table, and higher
/// pages from Lunar Magic's pointer routine when the ROM was saved by it.
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
    for i in 0..LAYER2_TILEMAP_LEN {
        let lo = bus.wram[(ram::LAYER2_TILEMAP_LOW - 0x7E_0000) as usize + i] as u16;
        let hi = bus.wram[(ram::LAYER2_TILEMAP_HIGH - 0x7E_0000) as usize + i] as u16;
        numbers.push(0x200 | lo | (hi << 8));
    }
    // Layer 2 objects index the BG table.
    numbers.extend((16 * SCREEN_LEN..GRID_LEN).map(|i| {
        0x200
            | bus.wram[(ram::TILES_LOW - 0x7E_0000) as usize + i] as u16
            | ((bus.wram[(ram::TILES_HIGH - 0x7E_0000) as usize + i] as u16) << 8)
    }));
    numbers.sort_unstable();
    numbers.dedup();
    let mut out = HashMap::with_capacity(numbers.len());
    for n in numbers {
        let ptr: Option<u32> = if n < 0x200 {
            let i = (ram::MAP16_POINTERS - 0x7E_0000) as usize + 2 * n as usize;
            Some(0x0D_0000 | bus.wram[i] as u32 | ((bus.wram[i + 1] as u32) << 8))
        } else if n < 0x400 && !lunar_magic {
            Some(
                map16::tables::MAP16_BG_TILES
                    .add(8 * (n as u32 - 0x200))
                    .raw(),
            )
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
