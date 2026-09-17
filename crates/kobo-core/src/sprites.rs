//! Level sprite lists.
//!
//! A level's sprite data is a header byte `SBNMMMMM` (buoyancy, buoyancy
//! without layer 2 interaction, Lunar Magic's "new sprite system" flag,
//! sprite memory) followed by entries of three bytes:
//! `yyyyEESY XXXXssss NNNNNNNN` (Y low nibble, extra bits, screen high
//! bit, Y high bit; X, screen low nibble; sprite number). In vertical
//! levels the game reads the `Y` field as the X position and `X` plus the
//! screen as the Y position.
//!
//! In the original format `$FF` ends the list. When the header's `N` bit is
//! set (Lunar Magic 3.00 and later; the flag is per level, not per ROM),
//! `$FF` introduces a command: `$00`-`$7F` sets the upper bits of the Y
//! position for every following sprite, `$FE` ends the list, and `$FF` is
//! an ordinary sprite whose first byte is `$FF`. Sprites inserted with PIXI
//! can carry extension bytes; their count comes from a size table PIXI
//! installs.

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::level::{self, LevelError};
use crate::rom::{Rom, RomError};

/// PIXI's marker byte and pointer to its per-sprite data size table.
pub const PIXI_SIZE_TABLE_MARKER: SnesAddr = SnesAddr::new(0x0EF30F);
pub const PIXI_SIZE_TABLE_PTR: SnesAddr = SnesAddr::new(0x0EF30C);
const PIXI_MARKER_VALUE: u8 = 0x42;

/// Header bit 5: the list uses Lunar Magic's command format.
const HEADER_NEW_SPRITE_SYSTEM: u8 = 0x20;
const CMD_END: u8 = 0xFE;
const CMD_LITERAL: u8 = 0xFF;

#[derive(Debug, Error)]
pub enum SpriteError {
    #[error(transparent)]
    Level(#[from] LevelError),
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error("sprite data at {0} runs past the end of the ROM")]
    Truncated(SnesAddr),
    #[error("sprite data at {0} uses the unknown command $FF ${1:02X} at offset {2}")]
    UnknownCommand(SnesAddr, u8, usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpriteHeader {
    /// Sprite memory setting, 0 to 31.
    pub memory: u8,
    /// Sprite buoyancy enabled.
    pub buoyancy: bool,
    /// Buoyancy without layer 2 interaction.
    pub buoyancy_no_layer2: bool,
    /// Lunar Magic's "new sprite system" flag: `$FF` starts a command
    /// rather than ending the list.
    pub new_sprite_system: bool,
}

impl SpriteHeader {
    pub fn from_byte(h: u8) -> Self {
        SpriteHeader {
            memory: h & 0x1F,
            buoyancy: h & 0x80 != 0,
            buoyancy_no_layer2: h & 0x40 != 0,
            new_sprite_system: h & HEADER_NEW_SPRITE_SYSTEM != 0,
        }
    }
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
    /// Y, 0 to 31 in the original format; the Y position jump in effect
    /// supplies bits 5 and up in Lunar Magic's format.
    pub y: u16,
    /// Extension bytes following the entry, if the ROM defines any.
    pub extension: Vec<u8>,
}

impl SpriteEntry {
    /// Level-wide tile coordinates. Horizontal levels place the sprite at
    /// (`screen * 16 + x`, `y`); vertical levels read the fields the other
    /// way round, at (`y`, `screen * 16 + x`), as `LoadSprFromLevel` does.
    pub fn tile_position(&self, vertical: bool) -> (usize, usize) {
        let along = self.screen as usize * 16 + self.x as usize;
        if vertical {
            (self.y as usize, along)
        } else {
            (along, self.y as usize)
        }
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

/// Parses a level's sprite list from the vanilla pointer table. Lunar
/// Magic ROMs relocate sprite data; prefer [`read_sprites_at`] with the
/// pointer the game resolved (see `LevelTiles::sprite_data_ptr`).
pub fn read_sprites(rom: &Rom, level: u16) -> Result<SpriteList, SpriteError> {
    read_sprites_at(rom, level::sprite_ptr(rom, level)?)
}

/// Parses a sprite list starting at `start` (the header byte).
pub fn read_sprites_at(rom: &Rom, start: SnesAddr) -> Result<SpriteList, SpriteError> {
    let sizes = pixi_size_table(rom);
    let byte = |i: usize| {
        rom.read_u8(start.add(i as u32))
            .map_err(|_| SpriteError::Truncated(start))
    };
    let mut data = Vec::new();
    let fill_to = |data: &mut Vec<u8>, n: usize| -> Result<(), SpriteError> {
        while data.len() < n {
            data.push(byte(data.len())?);
        }
        Ok(())
    };
    let mut list = None;
    let mut want = 1;
    while list.is_none() {
        fill_to(&mut data, want)?;
        match parse_sprites(&data, sizes.as_deref()) {
            Ok(l) => list = Some(l),
            Err(ParseError::Need(n)) => want = n,
            Err(ParseError::UnknownCommand(b, at)) => {
                return Err(SpriteError::UnknownCommand(start, b, at));
            }
        }
    }
    Ok(list.unwrap())
}

enum ParseError {
    /// The data must be at least this long to continue.
    Need(usize),
    UnknownCommand(u8, usize),
}

/// Parses the sprite list at the start of `data`, asking for more bytes
/// as it goes so a caller reading from a ROM never has to guess the
/// length up front.
fn parse_sprites(data: &[u8], sizes: Option<&[u8]>) -> Result<SpriteList, ParseError> {
    let get = |i: usize| data.get(i).copied().ok_or(ParseError::Need(i + 1));
    let header = SpriteHeader::from_byte(get(0)?);
    let mut sprites = Vec::new();
    let mut i = 1;
    let mut y_high = 0u16;
    loop {
        let mut b0 = get(i)?;
        if b0 == 0xFF {
            if !header.new_sprite_system {
                i += 1;
                break;
            }
            let cmd = get(i + 1)?;
            i += 2;
            match cmd {
                CMD_END => break,
                CMD_LITERAL => b0 = 0xFF,
                0x00..=0x7F => {
                    y_high = (cmd as u16) << 5;
                    continue;
                }
                _ => return Err(ParseError::UnknownCommand(cmd, i - 2)),
            }
        } else {
            i += 1;
        }
        // `i` now points at the second byte of the entry.
        let b1 = get(i)?;
        let id = get(i + 1)?;
        let extra_bits = (b0 >> 2) & 0x03;
        let size = sizes
            .map(|t| t[(extra_bits as usize) << 8 | id as usize] as usize)
            .unwrap_or(3)
            .max(3);
        let extension = (2..size - 1)
            .map(|k| get(i + k))
            .collect::<Result<_, _>>()?;
        sprites.push(SpriteEntry {
            id,
            extra_bits,
            screen: ((b0 & 0x02) << 3) | (b1 & 0x0F),
            x: b1 >> 4,
            y: y_high | ((b0 & 0x01) << 4 | (b0 >> 4)) as u16,
            extension,
        });
        i += size - 1;
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

    fn parse(data: &[u8]) -> SpriteList {
        match parse_sprites(data, None) {
            Ok(l) => l,
            Err(ParseError::Need(n)) => panic!("parser wanted {n} bytes of {}", data.len()),
            Err(ParseError::UnknownCommand(b, at)) => panic!("unknown command {b:02X} at {at}"),
        }
    }

    #[test]
    fn entry_fields() {
        // Y = 0x15 (high bit set, low nibble 5), extra bits 2, screen 0x13,
        // X = 7, sprite 0x2A.
        let b0 = (0x5 << 4) | (2 << 2) | 0x02 | 0x01;
        let b1 = (7 << 4) | 0x3;
        let list = parse(&[0x00, b0, b1, 0x2A, 0xFF]);
        let entry = &list.sprites[0];
        assert_eq!(entry.extra_bits, 2);
        assert_eq!(entry.screen, 0x13);
        assert_eq!(entry.x, 7);
        assert_eq!(entry.y, 0x15);
        assert_eq!(entry.tile_position(false), (0x137, 0x15));
        assert_eq!(entry.tile_position(true), (0x15, 0x137));
        assert_eq!(list.len, 5);
    }

    #[test]
    fn original_format_ends_at_ff_even_with_fe_following() {
        // Kaizo Kindergarten (Lunar Magic 3.03) level 105: an empty list
        // followed by the next RATS block.
        let list = parse(&[0x00, 0xFF, b'S', b'T', b'A', b'R', 0x0D, 0x00, 0xF2, 0xFF]);
        assert!(list.sprites.is_empty());
        assert_eq!(list.len, 2);
        assert!(!list.header.new_sprite_system);
        let list = parse(&[0x80, 0x31, 0x61, 0x35, 0xFF, 0xFE]);
        assert_eq!(list.sprites.len(), 1);
        assert_eq!(list.len, 5);
    }

    #[test]
    fn new_format_commands() {
        // Level 106 of a Lunar Magic 3.31 hack, abridged: two sprites on
        // screen 1, a Y jump to rows 32+, two more, a jump back, one on
        // screen 2, then the end marker.
        let data = [
            0x20, 0xF1, 0x21, 0x72, 0xC0, 0x61, 0x98, 0xFF, 0x01, 0x40, 0x81, 0x99, 0x00, 0x71,
            0x72, 0xFF, 0x00, 0xF1, 0x32, 0x98, 0xFF, 0xFE,
        ];
        let list = parse(&data);
        assert!(list.header.new_sprite_system);
        assert_eq!(list.len, data.len());
        let ys: Vec<u16> = list.sprites.iter().map(|s| s.y).collect();
        assert_eq!(ys, [31, 12, 36, 32, 31]);
        let xs: Vec<usize> = list
            .sprites
            .iter()
            .map(|s| s.tile_position(false).0)
            .collect();
        assert_eq!(xs, [0x12, 0x16, 0x18, 0x17, 0x23]);
    }

    #[test]
    fn new_format_literal_ff_and_unknown_command() {
        let list = parse(&[0x20, 0xFF, 0xFF, 0x45, 0x0A, 0xFF, 0xFE]);
        assert_eq!(list.sprites.len(), 1);
        let s = &list.sprites[0];
        assert_eq!(
            (s.y, s.extra_bits, s.screen, s.x, s.id),
            (0x1F, 3, 0x15, 4, 0x0A)
        );
        assert_eq!(list.len, 7);
        assert!(matches!(
            parse_sprites(&[0x20, 0xFF, 0x80, 0xFF, 0xFE], None),
            Err(ParseError::UnknownCommand(0x80, 1))
        ));
    }

    #[test]
    fn extension_bytes_from_size_table() {
        let mut sizes = vec![0u8; 0x400];
        sizes[2 << 8 | 0x2A] = 5;
        let data = [0x00, 0x08, 0x00, 0x2A, 0xAA, 0xBB, 0x00, 0x00, 0x01, 0xFF];
        let list = parse_sprites(&data, Some(&sizes)).ok().unwrap();
        assert_eq!(list.sprites[0].extension, [0xAA, 0xBB]);
        assert_eq!(list.sprites[1].id, 1);
        assert!(list.sprites[1].extension.is_empty());
        assert_eq!(list.len, data.len());
    }

    #[test]
    fn asks_for_more_bytes() {
        assert!(matches!(parse_sprites(&[], None), Err(ParseError::Need(1))));
        assert!(matches!(
            parse_sprites(&[0x20, 0xFF], None),
            Err(ParseError::Need(3))
        ));
        assert!(matches!(
            parse_sprites(&[0x00, 0x08], None),
            Err(ParseError::Need(3))
        ));
    }
}
