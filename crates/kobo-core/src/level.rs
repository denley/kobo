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

/// What a level mode puts on layer 2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer2Kind {
    /// A pre-built background tilemap, which the loader decodes into its
    /// own buffer and the game uploads whole.
    Background,
    /// Objects in the upper part of the tile grid, in horizontal screens.
    /// Modes `$03` and `$04` pair these with a vertical layer 1.
    HorizontalObjects,
    /// Objects in the upper part of the tile grid, in vertical screens.
    VerticalObjects,
    /// Nothing the loader builds: boss arenas draw their own layers, and
    /// the modes the game does not define load nothing.
    None,
}

/// A level mode (`$1925`, five bits of the primary header): the one
/// number that chooses a level's layer 2 and, through the ROM's per-mode
/// tables, its orientation, screen designation, and colour math. Those
/// tables are read by the game's own code; what is stated here is what
/// this library has to know without running it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct LevelMode(pub u8);

impl LevelMode {
    /// What the loader builds for layer 2. Background modes decode the
    /// tilemap (`LoadLevel`); object modes follow the layer 2 upload
    /// dispatch (`CODE_058883`) and the screen pointer tables at `$00BB08`
    /// and `$00BC16`, which choose the layout independently of layer 1's.
    /// Mode `$0F` (the dark rooms sharing the boss arenas' tilemap) has
    /// objects; the arenas themselves (`$09`, `$0B`, `$10`) have neither.
    pub fn layer2(self) -> Layer2Kind {
        match self.0 {
            0x00 | 0x0A | 0x0C | 0x0D | 0x0E | 0x11 | 0x1E => Layer2Kind::Background,
            0x01..=0x04 | 0x0F | 0x1F => Layer2Kind::HorizontalObjects,
            0x05..=0x08 => Layer2Kind::VerticalObjects,
            _ => Layer2Kind::None,
        }
    }
}

impl std::fmt::Display for LevelMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "${:02X}", self.0)
    }
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
    pub level_mode: LevelMode,
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
            level_mode: LevelMode(b[1] & 0x1F),
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
            (self.back_area << 5) | (self.level_mode.0 & 0x1F),
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
            level_mode: LevelMode(0x1E),
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

    #[test]
    fn level_modes_choose_layer_2() {
        let kind = |mode| LevelMode(mode).layer2();
        assert_eq!(kind(0x00), Layer2Kind::Background);
        assert_eq!(kind(0x0A), Layer2Kind::Background);
        assert_eq!(kind(0x01), Layer2Kind::HorizontalObjects);
        assert_eq!(kind(0x03), Layer2Kind::HorizontalObjects); // vertical layer 1
        assert_eq!(kind(0x0F), Layer2Kind::HorizontalObjects);
        assert_eq!(kind(0x07), Layer2Kind::VerticalObjects);
        for boss in [0x09, 0x0B, 0x10] {
            assert_eq!(kind(boss), Layer2Kind::None);
        }
        assert_eq!(LevelMode(0x0C).to_string(), "$0C");
    }
}
