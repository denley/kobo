//! Mapping between the SNES CPU address space and offsets in the ROM file.
//!
//! Nothing outside this module may assume a particular cartridge mapping.
//! Every ROM read goes through a [`Mapping`], so LoROM and SA-1 ROMs are
//! handled by the same code paths.
//!
//! The conversions mirror the conventions used by Asar so that addresses
//! shown by Kobo agree with those shown by the rest of the toolchain.

use std::fmt;

use thiserror::Error;

/// A 24-bit address in the SNES CPU address space, such as `$05E000`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SnesAddr(u32);

impl SnesAddr {
    /// Wraps a raw address. Bits above the low 24 are discarded.
    pub const fn new(addr: u32) -> Self {
        Self(addr & 0x00FF_FFFF)
    }

    /// Builds an address from a bank byte and a 16-bit offset within it.
    pub const fn from_bank_offset(bank: u8, offset: u16) -> Self {
        Self(((bank as u32) << 16) | offset as u32)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn bank(self) -> u8 {
        (self.0 >> 16) as u8
    }

    /// The 16-bit offset within the bank.
    pub const fn offset(self) -> u16 {
        self.0 as u16
    }

    /// Adds a byte count, wrapping within the 24-bit space.
    pub const fn add(self, n: u32) -> Self {
        Self::new(self.0.wrapping_add(n))
    }
}

impl fmt::Debug for SnesAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${:06X}", self.0)
    }
}

impl fmt::Display for SnesAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// A byte offset into the headerless ROM image. Often called a "PC address"
/// in SMW hacking documentation.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PcAddr(u32);

impl PcAddr {
    pub const fn new(offset: u32) -> Self {
        Self(offset)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Debug for PcAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:06X}", self.0)
    }
}

impl fmt::Display for PcAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// Cartridge memory mapping.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mapping {
    /// Standard LoROM, as used by vanilla SMW and non-SA-1 hacks. Up to 4 MiB.
    LoRom,
    /// SA-1 cartridge mapping with the default Super MMC bank assignment
    /// (banks 0 to 3 mapped in order). Up to 4 MiB.
    Sa1Rom,
    /// An SA-1 cartridge of more than 4 MiB, as SA-1 Pack sets the Super
    /// MMC up for one (Asar's `bigsa1rom`): the first 4 MiB in the LoROM
    /// view alone, the rest in the HiROM view alone. Up to 8 MiB.
    BigSa1Rom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Error)]
pub enum MapError {
    #[error("{0} is not a ROM address under {1:?} mapping")]
    NotRom(SnesAddr, Mapping),
    #[error("{0} is outside the addressable ROM range under {1:?} mapping")]
    OutOfRange(PcAddr, Mapping),
}

/// SA-1 Super MMC slots. Index is the top three bits of the SNES bank, value
/// is the megabyte of ROM mapped there. `None` slots are not ROM.
const SA1_SLOTS: [Option<u32>; 8] = [
    Some(0x00_0000),
    Some(0x10_0000),
    None,
    None,
    Some(0x20_0000),
    Some(0x30_0000),
    None,
    None,
];

impl Mapping {
    /// Picks a mapping from the map-mode byte of the internal ROM header
    /// and the image's length: the header does not say how an SA-1
    /// cartridge's Super MMC is set up, but only an image over 4 MiB
    /// needs anything other than the default. Returns `None` for
    /// unsupported modes (HiROM, ExHiROM, and so on).
    pub const fn from_map_mode(map_mode: u8, rom_len: usize) -> Option<Self> {
        match map_mode & 0x0F {
            0x0 => Some(Self::LoRom),
            0x3 if rom_len > 0x40_0000 => Some(Self::BigSa1Rom),
            0x3 => Some(Self::Sa1Rom),
            _ => None,
        }
    }

    /// Whether the cartridge has an SA-1.
    pub const fn is_sa1(self) -> bool {
        matches!(self, Self::Sa1Rom | Self::BigSa1Rom)
    }

    /// Converts a SNES address to a ROM file offset.
    pub fn snes_to_pc(self, addr: SnesAddr) -> Result<PcAddr, MapError> {
        let a = addr.raw();
        let err = MapError::NotRom(addr, self);
        match self {
            Self::LoRom => {
                let is_wram = (a & 0xFE_0000) == 0x7E_0000;
                let is_low_half = (a & 0x40_8000) == 0;
                let is_sram = (a & 0x70_8000) == 0x70_0000;
                if is_wram || is_low_half || is_sram {
                    return Err(err);
                }
                Ok(PcAddr(((a & 0x7F_0000) >> 1) | (a & 0x7FFF)))
            }
            Self::Sa1Rom => {
                if (a & 0x40_8000) == 0x00_8000 {
                    // LoROM-style view: banks 00-3F and 80-BF, upper halves.
                    let slot = SA1_SLOTS[((a & 0xE0_0000) >> 21) as usize].ok_or(err)?;
                    Ok(PcAddr(slot | ((a & 0x1F_0000) >> 1) | (a & 0x7FFF)))
                } else if (a & 0xC0_0000) == 0xC0_0000 {
                    // HiROM-style view: banks C0-FF, full banks.
                    let idx = ((a & 0x10_0000) >> 20) | ((a & 0x20_0000) >> 19);
                    let slot = SA1_SLOTS[idx as usize].ok_or(err)?;
                    Ok(PcAddr(slot | (a & 0x0F_FFFF)))
                } else {
                    Err(err)
                }
            }
            Self::BigSa1Rom => {
                if (a & 0x40_8000) == 0x00_8000 {
                    Ok(PcAddr(
                        ((a & 0x80_0000) >> 2) | ((a & 0x3F_0000) >> 1) | (a & 0x7FFF),
                    ))
                } else if (a & 0xC0_0000) == 0xC0_0000 {
                    Ok(PcAddr(0x40_0000 | (a & 0x3F_FFFF)))
                } else {
                    Err(err)
                }
            }
        }
    }

    /// Converts a ROM file offset to the canonical SNES address for it.
    pub fn pc_to_snes(self, pc: PcAddr) -> Result<SnesAddr, MapError> {
        let p = pc.raw();
        let err = MapError::OutOfRange(pc, self);
        match (self, p) {
            (Self::BigSa1Rom, 0x40_0000..0x80_0000) => return Ok(SnesAddr::new(0xC0_0000 | p)),
            (_, 0x40_0000..) => return Err(err),
            _ => {}
        }
        match self {
            Self::LoRom => {
                let mut bank = p >> 15;
                // Banks 7E and 7F are WRAM; the ROM there is only visible
                // through the FE/FF mirrors.
                if bank >= 0x7E {
                    bank |= 0x80;
                }
                Ok(SnesAddr::new((bank << 16) | 0x8000 | (p & 0x7FFF)))
            }
            Self::Sa1Rom | Self::BigSa1Rom => {
                let mb = p & 0x70_0000;
                let slot = SA1_SLOTS.iter().position(|s| *s == Some(mb)).ok_or(err)? as u32;
                Ok(SnesAddr::new(
                    0x8000 | (slot << 21) | ((p & 0x0F_8000) << 1) | (p & 0x7FFF),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(a: u32) -> SnesAddr {
        SnesAddr::new(a)
    }

    fn pc(p: u32) -> PcAddr {
        PcAddr::new(p)
    }

    #[test]
    fn snes_addr_parts() {
        let a = SnesAddr::from_bank_offset(0x05, 0xE000);
        assert_eq!(a.raw(), 0x05E000);
        assert_eq!(a.bank(), 0x05);
        assert_eq!(a.offset(), 0xE000);
        assert_eq!(format!("{a}"), "$05E000");
        assert_eq!(s(0xFF05E000), s(0x05E000));
    }

    #[test]
    fn lorom_known_addresses() {
        let m = Mapping::LoRom;
        assert_eq!(m.snes_to_pc(s(0x008000)), Ok(pc(0x000000)));
        assert_eq!(m.snes_to_pc(s(0x00FFFF)), Ok(pc(0x007FFF)));
        assert_eq!(m.snes_to_pc(s(0x018000)), Ok(pc(0x008000)));
        // SMW layer 1 level pointer table.
        assert_eq!(m.snes_to_pc(s(0x05E000)), Ok(pc(0x02E000)));
        // Internal header location.
        assert_eq!(m.snes_to_pc(s(0x00FFC0)), Ok(pc(0x007FC0)));
        // Mirrors in the upper half of the address space.
        assert_eq!(m.snes_to_pc(s(0x808000)), Ok(pc(0x000000)));
        assert_eq!(m.snes_to_pc(s(0x85E000)), Ok(pc(0x02E000)));
        // End of a 4 MiB ROM.
        assert_eq!(m.snes_to_pc(s(0xFFFFFF)), Ok(pc(0x3FFFFF)));
    }

    #[test]
    fn lorom_rejects_non_rom() {
        let m = Mapping::LoRom;
        for a in [0x000000, 0x007FFF, 0x7E0000, 0x7F1234, 0x700000, 0xF00000] {
            assert_eq!(
                m.snes_to_pc(s(a)),
                Err(MapError::NotRom(s(a), m)),
                "{:06X}",
                a
            );
        }
    }

    #[test]
    fn lorom_pc_to_snes() {
        let m = Mapping::LoRom;
        assert_eq!(m.pc_to_snes(pc(0x000000)), Ok(s(0x008000)));
        assert_eq!(m.pc_to_snes(pc(0x007FFF)), Ok(s(0x00FFFF)));
        assert_eq!(m.pc_to_snes(pc(0x008000)), Ok(s(0x018000)));
        assert_eq!(m.pc_to_snes(pc(0x02E000)), Ok(s(0x05E000)));
        assert_eq!(m.pc_to_snes(pc(0x3F0000)), Ok(s(0xFE8000)));
        assert_eq!(m.pc_to_snes(pc(0x3FFFFF)), Ok(s(0xFFFFFF)));
        assert_eq!(
            m.pc_to_snes(pc(0x400000)),
            Err(MapError::OutOfRange(pc(0x400000), m))
        );
    }

    #[test]
    fn lorom_round_trips() {
        let m = Mapping::LoRom;
        for p in (0..0x40_0000u32).step_by(0x1234) {
            let a = m.pc_to_snes(pc(p)).unwrap();
            assert_eq!(m.snes_to_pc(a), Ok(pc(p)), "{a}");
        }
    }

    #[test]
    fn sa1_known_addresses() {
        let m = Mapping::Sa1Rom;
        // LoROM-style banks.
        assert_eq!(m.snes_to_pc(s(0x008000)), Ok(pc(0x000000)));
        assert_eq!(m.snes_to_pc(s(0x05E000)), Ok(pc(0x02E000)));
        assert_eq!(m.snes_to_pc(s(0x1FFFFF)), Ok(pc(0x0FFFFF)));
        assert_eq!(m.snes_to_pc(s(0x208000)), Ok(pc(0x100000)));
        assert_eq!(m.snes_to_pc(s(0x3FFFFF)), Ok(pc(0x1FFFFF)));
        assert_eq!(m.snes_to_pc(s(0x808000)), Ok(pc(0x200000)));
        assert_eq!(m.snes_to_pc(s(0xA08000)), Ok(pc(0x300000)));
        assert_eq!(m.snes_to_pc(s(0xBFFFFF)), Ok(pc(0x3FFFFF)));
        // HiROM-style banks.
        assert_eq!(m.snes_to_pc(s(0xC00000)), Ok(pc(0x000000)));
        assert_eq!(m.snes_to_pc(s(0xC08000)), Ok(pc(0x008000)));
        assert_eq!(m.snes_to_pc(s(0xD00000)), Ok(pc(0x100000)));
        assert_eq!(m.snes_to_pc(s(0xE00000)), Ok(pc(0x200000)));
        assert_eq!(m.snes_to_pc(s(0xF00000)), Ok(pc(0x300000)));
        assert_eq!(m.snes_to_pc(s(0xFFFFFF)), Ok(pc(0x3FFFFF)));
    }

    #[test]
    fn sa1_rejects_non_rom() {
        let m = Mapping::Sa1Rom;
        for a in [0x000000, 0x007FFF, 0x400000, 0x408000, 0x7E0000, 0x7F8000] {
            assert_eq!(
                m.snes_to_pc(s(a)),
                Err(MapError::NotRom(s(a), m)),
                "{:06X}",
                a
            );
        }
    }

    #[test]
    fn sa1_pc_to_snes() {
        let m = Mapping::Sa1Rom;
        assert_eq!(m.pc_to_snes(pc(0x000000)), Ok(s(0x008000)));
        assert_eq!(m.pc_to_snes(pc(0x008000)), Ok(s(0x018000)));
        assert_eq!(m.pc_to_snes(pc(0x100000)), Ok(s(0x208000)));
        assert_eq!(m.pc_to_snes(pc(0x200000)), Ok(s(0x808000)));
        assert_eq!(m.pc_to_snes(pc(0x2FFFFF)), Ok(s(0x9FFFFF)));
        assert_eq!(m.pc_to_snes(pc(0x300000)), Ok(s(0xA08000)));
        assert_eq!(m.pc_to_snes(pc(0x3FFFFF)), Ok(s(0xBFFFFF)));
        assert_eq!(
            m.pc_to_snes(pc(0x400000)),
            Err(MapError::OutOfRange(pc(0x400000), m))
        );
    }

    #[test]
    fn sa1_round_trips() {
        let m = Mapping::Sa1Rom;
        for p in (0..0x40_0000u32).step_by(0x1234) {
            let a = m.pc_to_snes(pc(p)).unwrap();
            assert_eq!(m.snes_to_pc(a), Ok(pc(p)), "{a}");
        }
    }

    #[test]
    fn map_mode_detection() {
        const MIB: usize = 0x10_0000;
        assert_eq!(Mapping::from_map_mode(0x20, MIB), Some(Mapping::LoRom));
        assert_eq!(Mapping::from_map_mode(0x30, MIB), Some(Mapping::LoRom));
        assert_eq!(Mapping::from_map_mode(0x23, 4 * MIB), Some(Mapping::Sa1Rom));
        assert_eq!(
            Mapping::from_map_mode(0x23, 6 * MIB),
            Some(Mapping::BigSa1Rom)
        );
        assert_eq!(Mapping::from_map_mode(0x21, MIB), None);
        assert_eq!(Mapping::from_map_mode(0x25, MIB), None);
    }

    #[test]
    fn big_sa1_splits_the_image_between_the_views() {
        let m = Mapping::BigSa1Rom;
        assert!(m.is_sa1());
        // The first 4 MiB as the default assignment has them, LoROM only.
        for a in [0x008000, 0x05E000, 0x3FFFFF, 0x808000, 0xBFFFFF] {
            assert_eq!(m.snes_to_pc(s(a)), Mapping::Sa1Rom.snes_to_pc(s(a)));
        }
        // The rest in whole banks.
        assert_eq!(m.snes_to_pc(s(0xC00000)), Ok(pc(0x400000)));
        assert_eq!(m.snes_to_pc(s(0xFFFFFF)), Ok(pc(0x7FFFFF)));
        assert!(m.snes_to_pc(s(0x400000)).is_err());
        assert!(m.snes_to_pc(s(0x001234)).is_err());
        assert_eq!(m.pc_to_snes(pc(0x02E000)), Ok(s(0x05E000)));
        assert_eq!(m.pc_to_snes(pc(0x3FFFFF)), Ok(s(0xBFFFFF)));
        assert_eq!(m.pc_to_snes(pc(0x400000)), Ok(s(0xC00000)));
        assert_eq!(m.pc_to_snes(pc(0x7FFFFF)), Ok(s(0xFFFFFF)));
        assert!(m.pc_to_snes(pc(0x800000)).is_err());
        assert!(Mapping::Sa1Rom.pc_to_snes(pc(0x400000)).is_err());
    }
}
