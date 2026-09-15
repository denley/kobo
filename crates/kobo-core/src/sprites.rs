//! Level sprite lists.
//!
//! A level's sprite data is a header byte followed by entries of three
//! bytes: `yyyyEESY XXXXssss NNNNNNNN` (Y low nibble, extra bits, screen
//! high bit, Y high bit; X, screen low nibble; sprite number). Vanilla
//! data ends with `$FF`. Lunar Magic 3.00 and later ends with `$FF $FE`
//! and may use `$FF` followed by a byte below `$80` as a command. Sprites
//! inserted with PIXI can carry extension bytes; their count comes from a
//! size table PIXI installs.

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::level::{self, LevelError};
use crate::rom::{Rom, RomError};

/// PIXI's marker byte and pointer to its per-sprite data size table.
pub const PIXI_SIZE_TABLE_MARKER: SnesAddr = SnesAddr::new(0x0EF30F);
pub const PIXI_SIZE_TABLE_PTR: SnesAddr = SnesAddr::new(0x0EF30C);
const PIXI_MARKER_VALUE: u8 = 0x42;

#[derive(Debug, Error)]
pub enum SpriteError {
    #[error(transparent)]
    Level(#[from] LevelError),
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error("sprite data at {0} runs past the end of the ROM")]
    Truncated(SnesAddr),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpriteHeader {
    /// Sprite memory setting, 0 to 31.
    pub memory: u8,
    /// Sprite buoyancy enabled.
    pub buoyancy: bool,
    /// Buoyancy without layer 2 interaction.
    pub buoyancy_no_layer2: bool,
    /// Lunar Magic's "new sprite system" flag.
    pub new_sprite_system: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SpriteEntry {
    /// Sprite number, 0 to 255.
    pub id: u8,
    /// Extra bits, 0 to 3.
    pub extra_bits: u8,
    /// Screen number, 0 to 31.
    pub screen: u8,
    /// X within the screen, 0 to 15.
    pub x: u8,
    /// Y, 0 to 31 (plus any Lunar Magic Y offset command in effect).
    pub y: u8,
    /// Extension bytes following the entry, if the ROM defines any.
    pub extension: Vec<u8>,
}

impl SpriteEntry {
    /// Level-wide tile coordinates for a horizontal level.
    pub fn level_x(&self) -> usize {
        self.screen as usize * 16 + self.x as usize
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SpriteList {
    pub header: SpriteHeader,
    pub sprites: Vec<SpriteEntry>,
    /// Bytes of sprite data consumed, including the terminator.
    pub len: usize,
}

/// PIXI's data-size table, if installed: one byte per (extra bits, sprite
/// number), giving the total entry size.
fn pixi_size_table(rom: &Rom) -> Option<Vec<u8>> {
    if rom.read_u8(PIXI_SIZE_TABLE_MARKER).ok()? != PIXI_MARKER_VALUE {
        return None;
    }
    let ptr = rom.read_ptr(PIXI_SIZE_TABLE_PTR).ok()?;
    rom.read(ptr, 0x400).ok().map(|t| t.to_vec())
}

/// Whether the ROM uses the Lunar Magic 3 sprite data terminator.
fn lm3_sprite_format(rom: &Rom) -> bool {
    rom.lunar_magic_version()
        .and_then(|v| v.split('.').next()?.parse::<u32>().ok())
        .is_some_and(|major| major >= 3)
}

/// Parses a level's sprite list from the vanilla pointer table. Lunar
/// Magic ROMs relocate sprite data; prefer [`read_sprites_at`] with the
/// pointer the game resolved (see `LevelTiles::sprite_data_ptr`).
pub fn read_sprites(rom: &Rom, level: u16) -> Result<SpriteList, SpriteError> {
    read_sprites_at(rom, level::sprite_ptr(rom, level)?)
}

/// Parses a sprite list starting at `start` (the header byte).
pub fn read_sprites_at(rom: &Rom, start: SnesAddr) -> Result<SpriteList, SpriteError> {
    let sizes = pixi_size_table(rom);
    let lm3 = lm3_sprite_format(rom);
    let byte = |i: usize| {
        rom.read_u8(start.add(i as u32))
            .map_err(|_| SpriteError::Truncated(start))
    };
    let h = byte(0)?;
    let header = SpriteHeader {
        memory: h & 0x1F,
        buoyancy: h & 0x80 != 0,
        buoyancy_no_layer2: h & 0x40 != 0,
        new_sprite_system: h & 0x20 != 0,
    };
    let mut sprites = Vec::new();
    let mut i = 1;
    let mut y_base = 0u8;
    loop {
        let b0 = byte(i)?;
        if b0 == 0xFF {
            if !lm3 {
                i += 1;
                break;
            }
            let cmd = byte(i + 1)?;
            i += 2;
            if cmd == 0xFE {
                break;
            }
            // Other commands adjust the Y base for following sprites.
            y_base = cmd;
            continue;
        }
        let b1 = byte(i + 1)?;
        let id = byte(i + 2)?;
        let extra_bits = (b0 >> 2) & 0x03;
        let size = sizes
            .as_ref()
            .map(|t| t[(extra_bits as usize) << 8 | id as usize] as usize)
            .unwrap_or(3)
            .max(3);
        let mut extension = Vec::with_capacity(size - 3);
        for k in 3..size {
            extension.push(byte(i + k)?);
        }
        sprites.push(SpriteEntry {
            id,
            extra_bits,
            screen: ((b0 & 0x02) << 3) | (b1 & 0x0F),
            x: b1 >> 4,
            y: ((b0 & 0x01) << 4 | (b0 >> 4)).wrapping_add(y_base),
            extension,
        });
        i += size;
    }
    Ok(SpriteList {
        header,
        sprites,
        len: i,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_fields() {
        // Y = 0x15 (high bit set, low nibble 5), extra bits 2, screen 0x13,
        // X = 7, sprite 0x2A.
        let b0 = (0x5 << 4) | (2 << 2) | 0x02 | 0x01;
        let b1 = (7 << 4) | 0x3;
        let entry = SpriteEntry {
            id: 0x2A,
            extra_bits: (b0 >> 2) & 3,
            screen: ((b0 & 0x02) << 3) | (b1 & 0x0F),
            x: b1 >> 4,
            y: (b0 & 0x01) << 4 | (b0 >> 4),
            extension: vec![],
        };
        assert_eq!(entry.extra_bits, 2);
        assert_eq!(entry.screen, 0x13);
        assert_eq!(entry.x, 7);
        assert_eq!(entry.y, 0x15);
        assert_eq!(entry.level_x(), 0x137);
    }
}
