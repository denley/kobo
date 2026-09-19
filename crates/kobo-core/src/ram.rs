//! The game's working memory, and where its variables live in it.
//!
//! A [`RamAddr`] names a variable by its address in the vanilla game
//! (`$7E0000`-`$7FFFFF`), the way the community's RAM maps do. A
//! [`RamMap`] turns it into the bus address the loaded ROM keeps that
//! variable at, and [`Ram`] holds the memory itself. This is the only
//! place that knows that mapping: SA-1 Pack moves most of the game's RAM
//! into the SA-1's I-RAM and BW-RAM and widens the sprite tables, so
//! nothing else may assume a variable sits in WRAM at its vanilla address.

use std::fmt;

pub const WRAM_LEN: usize = 0x2_0000;
pub const SRAM_LEN: usize = 0x8000;

/// A game variable, named by its vanilla address.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct RamAddr(u32);

impl RamAddr {
    /// `addr` is a vanilla work RAM address, `$7E0000`-`$7FFFFF`.
    pub const fn new(addr: u32) -> Self {
        assert!(addr >= 0x7E_0000 && addr <= 0x7F_FFFF);
        Self(addr)
    }

    /// Parses a work RAM address, or `None` outside `$7E0000`-`$7FFFFF`.
    pub fn checked(addr: u32) -> Option<Self> {
        (0x7E_0000..=0x7F_FFFF)
            .contains(&addr)
            .then_some(Self(addr))
    }

    /// The vanilla address.
    pub const fn vanilla(self) -> u32 {
        self.0
    }
}

impl fmt::Display for RamAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${:06X}", self.0)
    }
}

/// Where a ROM keeps the game's variables.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum RamMap {
    /// Everything in WRAM at its vanilla address. Lunar Magic and the
    /// usual tools leave this alone.
    #[default]
    Vanilla,
}

impl RamMap {
    /// Bus address of a variable. Tables are resolved by their first
    /// entry and indexed from there, since a map may widen them.
    pub fn resolve(self, addr: RamAddr) -> u32 {
        match self {
            Self::Vanilla => addr.0,
        }
    }

    /// Sprite slots, the length of the per-slot sprite tables.
    pub fn sprite_slots(self) -> u32 {
        match self {
            Self::Vanilla => 12,
        }
    }
}

/// Everything a routine can change apart from video memory: work RAM and
/// the cartridge's save RAM. Cloning it is a snapshot of the game's state,
/// and [`Clone::clone_from`] restores one.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Ram {
    map: RamMap,
    wram: Vec<u8>,
    sram: Vec<u8>,
}

impl fmt::Debug for Ram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ram").field("map", &self.map).finish()
    }
}

impl Ram {
    /// Zeroed memory laid out by `map`.
    pub fn new(map: RamMap) -> Self {
        Self {
            map,
            wram: vec![0; WRAM_LEN],
            sram: vec![0; SRAM_LEN],
        }
    }

    pub fn map(&self) -> RamMap {
        self.map
    }

    /// Reads a bus address, or `None` if it is not RAM: work RAM in banks
    /// `$7E`-`$7F` with its low 8 KiB mirrored in the system banks, and
    /// save RAM in banks `$70`-`$7D`. Every CPU access comes through here
    /// or [`Ram::write`], so the decode is kept to one bank match.
    #[inline(always)]
    pub fn read(&self, addr: u32) -> Option<u8> {
        let off = addr as u16;
        match (addr >> 16) as u8 {
            0x7E | 0x7F => Some(self.wram[(addr - 0x7E_0000) as usize]),
            0x00..=0x3F | 0x80..=0xBF if off < 0x2000 => Some(self.wram[off as usize]),
            0x70..=0x7D if off < 0x8000 => Some(self.sram[off as usize % SRAM_LEN]),
            _ => None,
        }
    }

    /// Writes a bus address; false if it is not RAM.
    #[inline(always)]
    pub fn write(&mut self, addr: u32, value: u8) -> bool {
        let off = addr as u16;
        let cell = match (addr >> 16) as u8 {
            0x7E | 0x7F => &mut self.wram[(addr - 0x7E_0000) as usize],
            0x00..=0x3F | 0x80..=0xBF if off < 0x2000 => &mut self.wram[off as usize],
            0x70..=0x7D if off < 0x8000 => &mut self.sram[off as usize % SRAM_LEN],
            _ => return false,
        };
        *cell = value;
        true
    }

    /// Reads RAM at a bus address the ROM's own code names (a table a
    /// patch added, say). Panics if the address is not RAM.
    pub fn peek(&self, addr: u32) -> u8 {
        self.read(addr)
            .unwrap_or_else(|| panic!("${addr:06X} is not RAM"))
    }

    /// Writes RAM at a bus address. Panics if the address is not RAM.
    pub fn poke(&mut self, addr: u32, value: u8) {
        assert!(self.write(addr, value), "${addr:06X} is not RAM");
    }

    pub fn u8(&self, addr: RamAddr) -> u8 {
        self.peek(self.map.resolve(addr))
    }

    /// Entry `index` of the byte table starting at `table`.
    pub fn u8_at(&self, table: RamAddr, index: u32) -> u8 {
        self.peek(self.map.resolve(table) + index)
    }

    pub fn u16(&self, addr: RamAddr) -> u16 {
        u16::from_le_bytes([self.u8_at(addr, 0), self.u8_at(addr, 1)])
    }

    /// Entry `index` of the table of 16-bit words starting at `table`.
    pub fn u16_at(&self, table: RamAddr, index: u32) -> u16 {
        u16::from_le_bytes([
            self.u8_at(table, 2 * index),
            self.u8_at(table, 2 * index + 1),
        ])
    }

    pub fn u24(&self, addr: RamAddr) -> u32 {
        u32::from_le_bytes([
            self.u8_at(addr, 0),
            self.u8_at(addr, 1),
            self.u8_at(addr, 2),
            0,
        ])
    }

    /// `len` bytes starting at `addr`.
    pub fn bytes(&self, addr: RamAddr, len: usize) -> Vec<u8> {
        (0..len as u32).map(|i| self.u8_at(addr, i)).collect()
    }

    pub fn set_u8(&mut self, addr: RamAddr, value: u8) {
        self.poke(self.map.resolve(addr), value);
    }

    pub fn set_u8_at(&mut self, table: RamAddr, index: u32, value: u8) {
        self.poke(self.map.resolve(table) + index, value);
    }

    pub fn set_u16(&mut self, addr: RamAddr, value: u16) {
        let [low, high] = value.to_le_bytes();
        self.set_u8_at(addr, 0, low);
        self.set_u8_at(addr, 1, high);
    }

    /// Sets `len` bytes starting at `addr` to `value`.
    pub fn fill(&mut self, addr: RamAddr, len: u32, value: u8) {
        for i in 0..len {
            self.set_u8_at(addr, i, value);
        }
    }
}

const fn ram(addr: u32) -> RamAddr {
    RamAddr::new(addr)
}

/// `$0100`: the game mode. Lunar Magic's tilemap upload checks it.
pub const GAME_MODE: RamAddr = ram(0x7E_0100);
/// `$0101`-`$0108`: the GFX files currently in VRAM. `$FF` forces uploads.
pub const LOADED_GFX_FILES: RamAddr = ram(0x7E_0101);
pub const LOADED_GFX_FILES_LEN: u32 = 8;
/// `$13`: the frame counter, including paused frames.
pub const TRUE_FRAME: RamAddr = ram(0x7E_0013);

/// `$141A`: non-zero while inside a level, so the header pointer
/// routine takes the screen-exit path instead of the overworld one.
pub const SUBLEVEL_COUNT: RamAddr = ram(0x7E_141A);
/// `$19B8`: screen exit table, level number low byte per screen.
pub const EXIT_TABLE_LOW: RamAddr = ram(0x7E_19B8);
/// `$19D8`: screen exit table, flags per screen. Vanilla stores the
/// exit's water bit here and never reads it back. Lunar Magic's
/// replacement for the high byte lookup (`JSL $05DC50` from
/// `CODE_05D796`) treats an entry with bit 2 set as its own format:
/// bit 0 is the level number's high byte, bit 1 selects a secondary
/// exit, and bit 3 is copied to `$192A`.
pub const EXIT_TABLE_HIGH: RamAddr = ram(0x7E_19D8);
/// `$1F11`: the player's submap, which vanilla turns into the level
/// number's high byte.
pub const OW_PLAYER_SUBMAP: RamAddr = ram(0x7E_1F11);
pub const LAST_SCREEN_HORIZ: RamAddr = ram(0x7E_005E);
pub const SCREEN_MODE: RamAddr = ram(0x7E_005B);
pub const LEVEL_MODE: RamAddr = ram(0x7E_1925);
pub const SCREENS: RamAddr = ram(0x7E_005D);
/// `$13D7`: the level height in pixels. Vanilla leaves it zero;
/// Lunar Magic 3's loader hook (`JSL` at `$05D9A1`) stores the
/// height of the level's horizontal level mode here.
pub const LEVEL_HEIGHT: RamAddr = ram(0x7E_13D7);
/// `$1931`: the object tileset, as the loader stored it.
pub const OBJECT_TILESET: RamAddr = ram(0x7E_1931);
/// The Map16 tile grid: low bytes and high bytes, `0x3800` each.
pub const TILES_LOW: RamAddr = ram(0x7E_C800);
pub const TILES_HIGH: RamAddr = ram(0x7F_C800);
pub const LAYER2_TILEMAP_LOW: RamAddr = ram(0x7E_B900);
pub const LAYER2_TILEMAP_HIGH: RamAddr = ram(0x7E_BD00);
pub const BACKGROUND_COLOR: RamAddr = ram(0x7E_0701);
/// 0x200 two-byte pointers into bank `$0D`, built by the level loader.
pub const MAP16_POINTERS: RamAddr = ram(0x7E_0FBE);
/// Direct page `$0C`: bank byte of the pointer Lunar Magic's routine returns.
pub const LM_MAP16_BANK: RamAddr = ram(0x7E_000C);
/// Direct page `$0A`-`$0C`: the BG Map16 table pointer the initial
/// layer 2 tilemap upload reads tile definitions through.
pub const BG_MAP16_BASE: RamAddr = ram(0x7E_000A);
/// Direct page `$05`-`$06`: bytes per screen of the background buffer,
/// as set by Lunar Magic's hook (vanilla hard-codes `$1B0`).
pub const BG_SCREEN_LEN: RamAddr = ram(0x7E_0005);
/// Direct page `$CE`-`$D0`: the level's sprite data pointer.
pub const SPRITE_DATA_PTR: RamAddr = ram(0x7E_00CE);

/// `$1A`/`$1C`: layer 1 position, and `$1462`/`$1464`: the position
/// the camera update copies from at the start of each frame.
pub const LAYER1_X: RamAddr = ram(0x7E_001A);
pub const LAYER1_Y: RamAddr = ram(0x7E_001C);
/// `$1E`/`$20`: layer 2 position. Game mode `$11` copies all eight
/// bytes of `$1A`-`$21` to `$1462`-`$1469` after resolving the level
/// header, seeding the camera update.
pub const LAYER2_X: RamAddr = ram(0x7E_001E);
pub const LAYER2_Y: RamAddr = ram(0x7E_0020);
pub const LAYER_POSITIONS_LEN: u32 = 8;
pub const NEXT_LAYER1_X: RamAddr = ram(0x7E_1462);
pub const NEXT_LAYER1_Y: RamAddr = ram(0x7E_1464);
/// `$22`/`$24`: layer 3 position, as the IRQ handler writes it to
/// `BG3HOFS`/`BG3VOFS` below the status bar.
pub const LAYER3_X: RamAddr = ram(0x7E_0022);
pub const LAYER3_Y: RamAddr = ram(0x7E_0024);
/// `$17BD`/`$17BC`: how far layer 1 moved this frame, as the camera
/// update leaves it for the layer scroll routine.
pub const LAYER1_DX: RamAddr = ram(0x7E_17BD);
pub const LAYER1_DY: RamAddr = ram(0x7E_17BC);
/// `$55`: layer 1 scroll direction, which the sprite loader turns into
/// an offset from the camera to the column it loads (1: none).
pub const LAYER1_SCROLL_DIR: RamAddr = ram(0x7E_0055);
/// `$1411`/`$1412`: horizontal and vertical camera scroll settings;
/// zero freezes the camera.
pub const HORIZ_SCROLL_SETTING: RamAddr = ram(0x7E_1411);
pub const VERT_SCROLL_SETTING: RamAddr = ram(0x7E_1412);
/// `$143E`/`$143F`: the layer 1 and layer 2 scroll commands a scroll
/// sprite (`E7`-`F5`) installed; zero when the level has none.
/// Autoscroll commands drive the camera every frame.
pub const LAYER1_SCROLL_CMD: RamAddr = ram(0x7E_143E);
pub const LAYER2_SCROLL_CMD: RamAddr = ram(0x7E_143F);

/// `$94`/`$96`: the player's position for the next frame.
pub const PLAYER_X: RamAddr = ram(0x7E_0094);
pub const PLAYER_Y: RamAddr = ram(0x7E_0096);
/// `$7B`/`$7D`: the player's speed.
pub const PLAYER_X_SPEED: RamAddr = ram(0x7E_007B);
pub const PLAYER_Y_SPEED: RamAddr = ram(0x7E_007D);
/// `$71`: the player's animation state; non-zero while an entrance
/// action (pipe, cannon pipe, door) is still playing.
pub const PLAYER_ANIMATION: RamAddr = ram(0x7E_0071);
/// `$185C`: non-zero skips the player's interaction with tiles.
pub const PLAYER_NO_TILE_INTERACTION: RamAddr = ram(0x7E_185C);
/// `$9D`: sprite lock, which also pauses layer 3 autoscroll.
pub const SPRITE_LOCK: RamAddr = ram(0x7E_009D);

/// `$0200`-`$03FF`: the OAM image, four bytes per object, followed at
/// `$0400`-`$041F` by the size and X-high bits packed four objects to a
/// byte. `$3F` is the OAM address the first object was written at.
pub const OAM: RamAddr = ram(0x7E_0200);
pub const OAM_ADDRESS: RamAddr = ram(0x7E_003F);
/// `$14C8`: sprite slot status (0 = free), one byte per slot.
pub const SPRITE_STATUS: RamAddr = ram(0x7E_14C8);
/// `$9E`, `$E4`/`$14E0`, `$D8`/`$14D4`: sprite number and position
/// tables, one byte per slot.
pub const SPRITE_NUMBER: RamAddr = ram(0x7E_009E);
pub const SPRITE_X_LOW: RamAddr = ram(0x7E_00E4);
pub const SPRITE_X_HIGH: RamAddr = ram(0x7E_14E0);
pub const SPRITE_Y_LOW: RamAddr = ram(0x7E_00D8);
pub const SPRITE_Y_HIGH: RamAddr = ram(0x7E_14D4);
/// `$1938`: the per-entry "already loaded" flags the vanilla level
/// sprite loader keeps, 128 of them.
pub const SPRITE_LOAD_STATUS: RamAddr = ram(0x7E_1938);
pub const SPRITE_LOAD_STATUS_LEN: u32 = 0x80;
/// `$18B9`: the active sprite generator, which keeps spawning sprites
/// after its own sprite slot is cleared.
pub const SPRITE_GENERATOR: RamAddr = ram(0x7E_18B9);
/// `$1892`: cluster sprite numbers (0 = free), `$1E16`/`$1E02`: the
/// low bytes of their positions. 20 slots.
pub const CLUSTER_NUMBER: RamAddr = ram(0x7E_1892);
pub const CLUSTER_X_LOW: RamAddr = ram(0x7E_1E16);
pub const CLUSTER_Y_LOW: RamAddr = ram(0x7E_1E02);
pub const CLUSTER_SLOTS: u32 = 20;

/// `$3E`: `BGMODE` mirror; `$40`: `CGADSUB` mirror; `$44`: `CGWSEL`
/// mirror; `$0D9D`/`$0D9E`: main and sub screen designation mirrors.
pub const BG_MODE: RamAddr = ram(0x7E_003E);
pub const COLOR_MATH: RamAddr = ram(0x7E_0040);
pub const COLOR_MATH_SELECT: RamAddr = ram(0x7E_0044);
pub const MAIN_SCREEN: RamAddr = ram(0x7E_0D9D);
pub const SUB_SCREEN: RamAddr = ram(0x7E_0D9E);
/// `$0D9B`: which NMI and IRQ code runs. Bit 7 marks a Mode 7 boss
/// arena, bit 6 one that uploads boss tiles, bit 0 one without the
/// ceiling and floor IRQs.
pub const IRQ_NMI_COMMAND: RamAddr = ram(0x7E_0D9B);
/// `$11`: which of an arena's IRQs comes next.
pub const IRQ_TYPE: RamAddr = ram(0x7E_0011);
/// `$04A0`: window 1's left and right edges per scanline, as the HDMA
/// feeds them to the PPU.
pub const WINDOW_TABLE: RamAddr = ram(0x7E_04A0);
/// `$41`-`$43`: `W12SEL`, `W34SEL`, and `WOBJSEL` mirrors.
pub const WINDOW_SELECT: RamAddr = ram(0x7E_0041);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_resolve_through_the_map() {
        let mut ram = Ram::new(RamMap::Vanilla);
        ram.set_u16(LAYER1_X, 0x1234);
        assert_eq!(ram.peek(0x7E_001A), 0x34);
        assert_eq!(ram.peek(0x00_001B), 0x12); // low WRAM mirror
        assert_eq!(ram.u16(LAYER1_X), 0x1234);
        ram.set_u8_at(SPRITE_STATUS, 11, 8);
        assert_eq!(ram.u8(RamAddr::new(0x7E_14D3)), 8);
        ram.fill(TILES_HIGH, 3, 0xAB);
        assert_eq!(ram.bytes(TILES_HIGH, 4), [0xAB, 0xAB, 0xAB, 0]);
        assert_eq!(ram.u24(TILES_HIGH), 0xAB_ABAB);
    }

    #[test]
    fn only_ram_answers_as_ram() {
        let mut ram = Ram::new(RamMap::Vanilla);
        assert_eq!(ram.read(0x00_8000), None);
        assert_eq!(ram.read(0x00_2100), None);
        assert!(!ram.write(0x05_8000, 1));
        assert!(ram.write(0x70_0010, 7));
        assert_eq!(ram.read(0x71_0010), Some(7)); // one SRAM chip, mirrored
        assert_eq!(RamAddr::checked(0x80_0000), None);
        assert_eq!(RamAddr::checked(0x7F_FFFF).unwrap().to_string(), "$7FFFFF");
    }

    #[test]
    fn a_clone_is_a_snapshot() {
        let mut ram = Ram::new(RamMap::Vanilla);
        ram.set_u8(GAME_MODE, 0x11);
        let saved = ram.clone();
        ram.set_u8(GAME_MODE, 0x14);
        ram.clone_from(&saved);
        assert_eq!(ram.u8(GAME_MODE), 0x11);
    }
}
