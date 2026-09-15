//! Expanding a level's objects into the Map16 tile grid by running the
//! ROM's own level loader on the headless CPU.
//!
//! The game keeps the grid in two byte planes: low bytes at `$7EC800` and
//! high bytes at `$7FC800`, `0x3800` bytes each. Horizontal levels store
//! each screen as 16 columns by 27 rows, screen after screen.

use thiserror::Error;

use crate::cpu::smw_bus::SmwBus;
use crate::cpu::{Cpu, CpuError};
use crate::level::{self, LevelError, PrimaryHeader};
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
}

/// ROM routines the loader entry points call, from the vanilla layout.
/// Lunar Magic keeps these entry points in place.
mod routines {
    /// `CODE_05D796`: resolves the level number and header pointers.
    pub const LOAD_HEADER_POINTERS: u32 = 0x05_D796;
    /// `CODE_05801E`: clears the buffers and runs `LoadLevel`.
    pub const LOAD_LEVEL_DATA: u32 = 0x05_801E;
}

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
}

impl LevelTiles {
    /// Map16 tile number at a horizontal-level position.
    pub fn tile(&self, screen: usize, x: usize, y: usize) -> u16 {
        let i = screen * SCREEN_LEN + y * SCREEN_COLS + x;
        self.low[i] as u16 | ((self.high[i] as u16) << 8)
    }

    /// Width and height in tiles for rendering a horizontal level.
    pub fn size(&self) -> (usize, usize) {
        (self.screens * SCREEN_COLS, SCREEN_ROWS)
    }
}

/// The `$0109` value that selects `level`, and the high-byte flag.
pub fn override_for(level: u16) -> Option<(u8, u8)> {
    let lo = level & 0xFF;
    let hi = (level >> 8) as u8;
    let v = if lo < 0x25 { lo } else { lo + 0x24 };
    (v <= 0xFF).then_some((v as u8, hi))
}

/// Runs the ROM's level loader for `level` and returns the tile grid.
pub fn expand_level(rom: &Rom, level: u16) -> Result<LevelTiles, ExpandError> {
    let header = level::read_primary_header(rom, level)?;
    let (ovr, hi) = override_for(level).ok_or(ExpandError::Unreachable(level))?;
    let mut bus = SmwBus::new(rom);
    let mut cpu = Cpu::new();
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
    Ok(LevelTiles {
        level,
        header,
        level_mode: bus.wram_u8(ram::LEVEL_MODE),
        vertical: bus.wram_u8(ram::SCREEN_MODE) & 0x01 != 0,
        screens: bus.wram_u8(ram::SCREENS) as usize,
        low: bus.wram_slice(ram::TILES_LOW, GRID_LEN).to_vec(),
        high: bus.wram_slice(ram::TILES_HIGH, GRID_LEN).to_vec(),
    })
}
