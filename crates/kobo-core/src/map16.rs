//! Map16: 16x16 level tiles built from four 8x8 tiles.
//!
//! The game keeps a table of 0x200 pointers (`$7E0FBE`) to 8-byte tile
//! definitions. For layer 1 the pointers mix a common table with a
//! tileset-specific one, selected per tile by a bitmask. For layer 2 the
//! pointers all come from the BG table, which Lunar Magic numbers as tiles
//! `0x200` to `0x3FF`. This module reproduces that assembly for a vanilla
//! ROM. Lunar Magic's relocated pages are resolved by running its own
//! pointer routine during a level load: see `expand::LevelTiles::map16`.

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::rom::{Rom, RomError};

/// One 8x8 tile reference as stored in SNES tilemaps: `yxpccctt tttttttt`
/// (y flip, x flip, priority, palette, 10-bit tile number).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Tile8Ref(pub u16);

impl Tile8Ref {
    pub const fn new(tile: u16, palette: u8, priority: bool, flip_x: bool, flip_y: bool) -> Self {
        Self(
            (tile & 0x3FF)
                | (((palette & 7) as u16) << 10)
                | ((priority as u16) << 13)
                | ((flip_x as u16) << 14)
                | ((flip_y as u16) << 15),
        )
    }

    pub const fn tile(self) -> u16 {
        self.0 & 0x3FF
    }

    pub const fn palette(self) -> u8 {
        ((self.0 >> 10) & 7) as u8
    }

    pub const fn priority(self) -> bool {
        self.0 & 0x2000 != 0
    }

    pub const fn flip_x(self) -> bool {
        self.0 & 0x4000 != 0
    }

    pub const fn flip_y(self) -> bool {
        self.0 & 0x8000 != 0
    }
}

/// A 16x16 tile. Fields are in the game's storage order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Map16Tile {
    pub top_left: Tile8Ref,
    pub bottom_left: Tile8Ref,
    pub top_right: Tile8Ref,
    pub bottom_right: Tile8Ref,
}

impl Map16Tile {
    pub fn from_bytes(b: [u8; 8]) -> Self {
        let w = |i: usize| Tile8Ref(u16::from_le_bytes([b[i], b[i + 1]]));
        Self {
            top_left: w(0),
            bottom_left: w(2),
            top_right: w(4),
            bottom_right: w(6),
        }
    }

    pub fn to_bytes(self) -> [u8; 8] {
        let mut out = [0u8; 8];
        for (i, r) in self.refs().iter().enumerate() {
            out[2 * i..2 * i + 2].copy_from_slice(&r.0.to_le_bytes());
        }
        out
    }

    /// The four references in storage order.
    pub fn refs(&self) -> [Tile8Ref; 4] {
        [
            self.top_left,
            self.bottom_left,
            self.top_right,
            self.bottom_right,
        ]
    }

    /// The reference at a quadrant, `(x, y)` in 0..2.
    pub fn quadrant(&self, x: usize, y: usize) -> Tile8Ref {
        match (x, y) {
            (0, 0) => self.top_left,
            (0, _) => self.bottom_left,
            (_, 0) => self.top_right,
            _ => self.bottom_right,
        }
    }
}

/// Vanilla Map16 tables.
pub mod tables {
    use crate::addr::SnesAddr;

    /// 15 words: bank `$0D` offsets of each tileset's specific tile data.
    pub const TILESET_MAP16_LOC: SnesAddr = SnesAddr::new(0x058000);
    /// 64-byte bitmask over tiles 0x000 to 0x1FF, MSB first. A set bit means
    /// the tile comes from the common table, clear means tileset-specific.
    pub const TILESET_SPECIFIC_MASK: SnesAddr = SnesAddr::new(0x0581BB);
    /// Common layer 1 tile data.
    pub const MAP16_COMMON: SnesAddr = SnesAddr::new(0x0D8000);
    /// Diagonal pipe tiles patched over 1C4-1C7 and 1EC-1EF in tilesets 0 and 7.
    pub const DIAGONAL_PIPE_TILES: SnesAddr = SnesAddr::new(0x0D8A70);
    /// Layer 2 (BG) tile data, 0x200 tiles.
    pub const MAP16_BG_TILES: SnesAddr = SnesAddr::new(0x0D9100);
}

pub const TILESET_COUNT: u8 = 15;
pub const FG_TILE_COUNT: usize = 0x200;
pub const BG_TILE_COUNT: usize = 0x200;

#[derive(Debug, Error)]
pub enum Map16Error {
    #[error("tileset {0} is out of range (0 to 14)")]
    BadTileset(u8),
    #[error(transparent)]
    Rom(#[from] RomError),
}

/// The 0x400 Map16 tiles visible to a level: 0x000-0x1FF for layer 1 in
/// the given tileset, 0x200-0x3FF for layer 2.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Map16Table {
    pub tileset: u8,
    pub tiles: Vec<Map16Tile>,
}

impl Map16Table {
    pub fn get(&self, index: u16) -> Option<&Map16Tile> {
        self.tiles.get(index as usize)
    }

    /// Raw bytes of a tile range, as Lunar Magic exports them.
    pub fn bytes(&self, range: std::ops::Range<usize>) -> Vec<u8> {
        self.tiles[range]
            .iter()
            .flat_map(|t| t.to_bytes())
            .collect()
    }
}

fn read_tile(rom: &Rom, addr: SnesAddr) -> Result<Map16Tile, RomError> {
    let b = rom.read(addr, 8)?;
    Ok(Map16Tile::from_bytes(b.try_into().expect("8 bytes")))
}

/// Builds the vanilla Map16 table for a tileset, mirroring the game's
/// pointer setup at level load. `apply_pipe_override` reproduces the
/// diagonal pipe patch the game applies for tilesets 0 and 7; Lunar
/// Magic's export leaves those eight tiles as the tileset data has them.
pub fn vanilla_map16(
    rom: &Rom,
    tileset: u8,
    apply_pipe_override: bool,
) -> Result<Map16Table, Map16Error> {
    use tables::*;
    if tileset >= TILESET_COUNT {
        return Err(Map16Error::BadTileset(tileset));
    }
    let mut specific = SnesAddr::from_bank_offset(
        0x0D,
        rom.read_u16(TILESET_MAP16_LOC.add(2 * tileset as u32))?,
    );
    let mut common = MAP16_COMMON;
    let mask = rom.read(TILESET_SPECIFIC_MASK, FG_TILE_COUNT / 8)?;
    let mut tiles = Vec::with_capacity(FG_TILE_COUNT + BG_TILE_COUNT);
    for t in 0..FG_TILE_COUNT {
        let from_common = mask[t / 8] & (0x80 >> (t % 8)) != 0;
        let src = if from_common {
            &mut common
        } else {
            &mut specific
        };
        tiles.push(read_tile(rom, *src)?);
        *src = src.add(8);
    }
    if apply_pipe_override && (tileset == 0 || tileset == 7) {
        let mut addr = DIAGONAL_PIPE_TILES;
        for t in (0x1C4..=0x1C7).chain(0x1EC..=0x1EF) {
            tiles[t] = read_tile(rom, addr)?;
            addr = addr.add(8);
        }
    }
    for i in 0..BG_TILE_COUNT {
        tiles.push(read_tile(rom, MAP16_BG_TILES.add(8 * i as u32))?);
    }
    Ok(Map16Table { tileset, tiles })
}

/// Lunar Magic's tables for foreground pages 2 to `$7F` and for what every
/// tile acts like, at the fixed addresses its layout keeps their pointers
/// (docs/lunar-magic-install.md). Kobo's Map16 routine and acts-like chain
/// (`asm/lunar-magic/`) read the same tables.
pub mod pages {
    use crate::addr::SnesAddr;
    use crate::rom::{Rom, RomError};

    /// Not `$FF` once the Map16 routine and the acts-like chain are
    /// installed, Lunar Magic's or Kobo's.
    pub const INSTALLED: SnesAddr = SnesAddr::new(0x06F600);
    /// 24-bit pointer to what tiles `$0000`-`$3FFF` act like, 2 bytes a tile.
    pub const ACTS_LIKE: SnesAddr = SnesAddr::new(0x06F624);
    /// 24-bit pointer, less `$8000`, to what tiles `$4000`-`$7FFF` act like;
    /// bank `$FF` for none.
    pub const ACTS_LIKE_UPPER: SnesAddr = SnesAddr::new(0x06F63A);

    /// 16 pages that share a table: tile `n`'s definition is at the table's
    /// pointer (plus 1 if kept less one) plus `n * 8` in 16 bits, in the
    /// pointer's bank.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct PageGroup {
        pub first_page: u8,
        pub pointer: SnesAddr,
        pub bank: SnesAddr,
        pub less_one: bool,
    }

    pub const PAGE_GROUPS: [PageGroup; 8] = {
        const fn g(first_page: u8, pointer: u32, bank: u32, less_one: bool) -> PageGroup {
            PageGroup {
                first_page,
                pointer: SnesAddr::new(pointer),
                bank: SnesAddr::new(bank),
                less_one,
            }
        }
        [
            g(0x00, 0x06F553, 0x06F557, false),
            g(0x10, 0x06F55C, 0x06F560, false),
            g(0x20, 0x06F567, 0x06F56B, true),
            g(0x30, 0x06F570, 0x06F574, true),
            g(0x40, 0x06F594, 0x06F598, false),
            g(0x50, 0x06F59D, 0x06F5A1, false),
            g(0x60, 0x06F5A8, 0x06F5AC, true),
            g(0x70, 0x06F5B1, 0x06F5B5, true),
        ]
    };

    impl PageGroup {
        /// The group page `page` (below `$80`) is in.
        pub fn of(page: u8) -> &'static PageGroup {
            &PAGE_GROUPS[(page as usize >> 4) & 7]
        }

        /// The group's pages past 1.
        pub fn pages(&self) -> std::ops::RangeInclusive<u8> {
            self.first_page.max(2)..=self.first_page + 15
        }

        /// Where the group's table has tile `first`, given the table's
        /// address, as the pointer and bank to store.
        pub fn stored_for(&self, first: u16, at: SnesAddr) -> (u16, u8) {
            let pointer = at
                .offset()
                .wrapping_sub(first.wrapping_mul(8))
                .wrapping_sub(self.less_one as u16);
            (pointer, at.bank())
        }

        /// The address of tile `tile`'s definition, or `None` if the
        /// group has no table (bank `$00`, as a fresh install leaves it).
        pub fn definition(&self, rom: &Rom, tile: u16) -> Result<Option<SnesAddr>, RomError> {
            let bank = rom.read_u8(self.bank)?;
            if bank == 0 {
                return Ok(None);
            }
            let pointer = rom.read_u16(self.pointer)?;
            let offset = pointer
                .wrapping_add(self.less_one as u16)
                .wrapping_add(tile.wrapping_mul(8));
            Ok(Some(SnesAddr::from_bank_offset(bank, offset)))
        }
    }

    /// Whether the ROM has Lunar Magic's layout for pages past 1.
    pub fn installed(rom: &Rom) -> bool {
        rom.read_u8(INSTALLED).is_ok_and(|b| b != 0xFF)
    }

    /// What tile `tile` acts like, from the tables, or `None` where there
    /// is no table.
    pub fn acts_like(rom: &Rom, tile: u16) -> Result<Option<u16>, RomError> {
        let at = if tile < 0x4000 {
            SnesAddr::new(rom.read_u24(ACTS_LIKE)?)
        } else {
            let pointer = rom.read_u24(ACTS_LIKE_UPPER)?;
            if pointer >> 16 == 0xFF {
                return Ok(None);
            }
            SnesAddr::new(pointer)
        };
        Ok(Some(rom.read_u16(SnesAddr::new(
            (at.raw() + 2 * tile as u32) & 0xFF_FFFF,
        ))?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_ref_fields() {
        let r = Tile8Ref(0x1C70);
        assert_eq!(r.tile(), 0x070);
        assert_eq!(r.palette(), 7);
        assert!(!r.priority());
        assert!(!r.flip_x());
        assert!(!r.flip_y());
        let r = Tile8Ref::new(0x3FF, 5, true, true, true);
        assert_eq!(r.0, 0xF7FF);
        assert_eq!(
            Tile8Ref::new(0x1234, 0xF, false, false, false).0,
            0x1E34 & 0x1FFF
        );
    }

    #[test]
    fn map16_tile_round_trip() {
        let b = [0x70, 0x1C, 0x72, 0x1C, 0x71, 0x1C, 0x73, 0x1C];
        let t = Map16Tile::from_bytes(b);
        assert_eq!(t.top_left.tile(), 0x70);
        assert_eq!(t.bottom_left.tile(), 0x72);
        assert_eq!(t.top_right.tile(), 0x71);
        assert_eq!(t.bottom_right.tile(), 0x73);
        assert_eq!(t.quadrant(1, 0), t.top_right);
        assert_eq!(t.quadrant(0, 1), t.bottom_left);
        assert_eq!(t.to_bytes(), b);
    }
}
