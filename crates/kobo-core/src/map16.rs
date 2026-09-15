//! Map16: 16x16 level tiles built from four 8x8 tiles.
//!
//! The game keeps a table of 0x200 pointers (`$7E0FBE`) to 8-byte tile
//! definitions. For layer 1 the pointers mix a common table with a
//! tileset-specific one, selected per tile by a bitmask. For layer 2 the
//! pointers all come from the BG table, which Lunar Magic numbers as tiles
//! `0x200` to `0x3FF`. This module reproduces that assembly for a vanilla
//! ROM; Lunar Magic's relocated Map16 is not handled yet.

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
