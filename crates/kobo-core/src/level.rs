//! Level lookup and the primary level header.

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::palette::LevelPaletteSelect;
use crate::rom::{Rom, RomError};

pub const LEVEL_COUNT: u16 = 0x200;

/// Vanilla level pointer tables. Lunar Magic keeps these in place.
pub mod tables {
    use crate::addr::SnesAddr;

    /// 3-byte layer 1 pointers, one per level.
    pub const LAYER1_PTRS: SnesAddr = SnesAddr::new(0x05E000);
    /// 3-byte layer 2 pointers, one per level.
    pub const LAYER2_PTRS: SnesAddr = SnesAddr::new(0x05E600);
    /// 2-byte sprite pointers into bank `$07`, one per level.
    pub const SPRITE_PTRS: SnesAddr = SnesAddr::new(0x05EC00);
}

#[derive(Debug, Error)]
pub enum LevelError {
    #[error("level {0:03X} is out of range (000 to 1FF)")]
    BadLevel(u16),
    #[error(transparent)]
    Rom(#[from] RomError),
}

fn check(level: u16) -> Result<(), LevelError> {
    if level < LEVEL_COUNT {
        Ok(())
    } else {
        Err(LevelError::BadLevel(level))
    }
}

/// Where a level's layer 1 data (header plus objects) starts.
pub fn layer1_ptr(rom: &Rom, level: u16) -> Result<SnesAddr, LevelError> {
    check(level)?;
    Ok(rom.read_ptr(tables::LAYER1_PTRS.add(3 * level as u32))?)
}

/// What a level's layer 2 pointer refers to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer2Data {
    /// Object data with its own 5-byte header, like layer 1.
    Objects(SnesAddr),
    /// A pre-built background tilemap. The stored bank is `$FF`; the game
    /// substitutes bank `$0C`.
    Tilemap(SnesAddr),
}

pub fn layer2_ptr(rom: &Rom, level: u16) -> Result<Layer2Data, LevelError> {
    check(level)?;
    let raw = rom.read_ptr(tables::LAYER2_PTRS.add(3 * level as u32))?;
    Ok(if raw.bank() == 0xFF {
        Layer2Data::Tilemap(SnesAddr::from_bank_offset(0x0C, raw.offset()))
    } else {
        Layer2Data::Objects(raw)
    })
}

/// Where a level's sprite data (header plus sprites) starts.
pub fn sprite_ptr(rom: &Rom, level: u16) -> Result<SnesAddr, LevelError> {
    check(level)?;
    let offset = rom.read_u16(tables::SPRITE_PTRS.add(2 * level as u32))?;
    Ok(SnesAddr::from_bank_offset(0x07, offset))
}

/// The five bytes at the start of a level's layer 1 data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrimaryHeader {
    /// Background palette, 0 to 7.
    pub bg_palette: u8,
    /// Number of screens, 1 to 32.
    pub screens: u8,
    /// Back area colour, 0 to 7.
    pub back_area: u8,
    /// Level mode, 0 to 31.
    pub level_mode: u8,
    pub layer3_priority: bool,
    /// Music, 0 to 7.
    pub music: u8,
    /// Sprite GFX set, 0 to 15.
    pub sprite_tileset: u8,
    /// Timer setting, 0 to 3.
    pub time: u8,
    /// Sprite palette, 0 to 7.
    pub sprite_palette: u8,
    /// Foreground palette, 0 to 7.
    pub fg_palette: u8,
    /// Item memory setting, 0 to 3.
    pub item_memory: u8,
    /// Vertical scroll setting, 0 to 3.
    pub vertical_scroll: u8,
    /// Object (FG/BG) tileset, 0 to 15.
    pub object_tileset: u8,
}

impl PrimaryHeader {
    pub fn from_bytes(b: [u8; 5]) -> Self {
        Self {
            bg_palette: b[0] >> 5,
            screens: (b[0] & 0x1F) + 1,
            back_area: b[1] >> 5,
            level_mode: b[1] & 0x1F,
            layer3_priority: b[2] & 0x80 != 0,
            music: (b[2] >> 4) & 0x07,
            sprite_tileset: b[2] & 0x0F,
            time: b[3] >> 6,
            sprite_palette: (b[3] >> 3) & 0x07,
            fg_palette: b[3] & 0x07,
            item_memory: b[4] >> 6,
            vertical_scroll: (b[4] >> 4) & 0x03,
            object_tileset: b[4] & 0x0F,
        }
    }

    pub fn to_bytes(self) -> [u8; 5] {
        [
            (self.bg_palette << 5) | ((self.screens - 1) & 0x1F),
            (self.back_area << 5) | (self.level_mode & 0x1F),
            ((self.layer3_priority as u8) << 7) | (self.music << 4) | (self.sprite_tileset & 0x0F),
            (self.time << 6) | (self.sprite_palette << 3) | (self.fg_palette & 0x07),
            (self.item_memory << 6) | (self.vertical_scroll << 4) | (self.object_tileset & 0x0F),
        ]
    }

    pub fn palette_select(&self) -> LevelPaletteSelect {
        LevelPaletteSelect {
            fg: self.fg_palette,
            bg: self.bg_palette,
            sprite: self.sprite_palette,
            back_area: self.back_area,
        }
    }
}

pub fn read_primary_header(rom: &Rom, level: u16) -> Result<PrimaryHeader, LevelError> {
    let ptr = layer1_ptr(rom, level)?;
    let b = rom.read(ptr, 5)?;
    Ok(PrimaryHeader::from_bytes(b.try_into().expect("5 bytes")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip() {
        let h = PrimaryHeader {
            bg_palette: 5,
            screens: 20,
            back_area: 3,
            level_mode: 0x1E,
            layer3_priority: true,
            music: 6,
            sprite_tileset: 0xB,
            time: 2,
            sprite_palette: 7,
            fg_palette: 1,
            item_memory: 3,
            vertical_scroll: 1,
            object_tileset: 0xE,
        };
        let b = h.to_bytes();
        assert_eq!(b[0], 0b101_10011);
        assert_eq!(b[1], 0b011_11110);
        assert_eq!(b[2], 0b1110_1011);
        assert_eq!(b[3], 0b10_111_001);
        assert_eq!(b[4], 0b1101_1110);
        assert_eq!(PrimaryHeader::from_bytes(b), h);
    }
}
