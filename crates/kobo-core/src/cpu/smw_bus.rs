//! A bus that maps a SMW ROM plus work RAM, with hardware registers
//! stubbed out.

use crate::cpu::Bus;
use crate::rom::Rom;

pub const WRAM_LEN: usize = 0x2_0000;
const SRAM_LEN: usize = 0x8000;

/// ROM, 128 KiB of WRAM, and a scratch SRAM. Hardware registers read as
/// zero and ignore writes.
pub struct SmwBus<'a> {
    pub rom: &'a Rom,
    pub wram: Vec<u8>,
    pub sram: Vec<u8>,
    /// Number of reads that hit unmapped or register space.
    pub unmapped_reads: u64,
    pub unmapped_writes: u64,
}

impl<'a> SmwBus<'a> {
    pub fn new(rom: &'a Rom) -> Self {
        Self {
            rom,
            wram: vec![0; WRAM_LEN],
            sram: vec![0; SRAM_LEN],
            unmapped_reads: 0,
            unmapped_writes: 0,
        }
    }

    /// Reads a slice of WRAM by its `$7Exxxx` / `$7Fxxxx` address.
    pub fn wram_slice(&self, addr: u32, len: usize) -> &[u8] {
        let start = (addr - 0x7E_0000) as usize;
        &self.wram[start..start + len]
    }

    pub fn wram_u8(&self, addr: u32) -> u8 {
        self.wram[(addr - 0x7E_0000) as usize]
    }

    pub fn set_wram_u8(&mut self, addr: u32, value: u8) {
        self.wram[(addr - 0x7E_0000) as usize] = value;
    }

    fn rom_read(&mut self, addr: u32) -> u8 {
        match self
            .rom
            .mapping()
            .snes_to_pc(crate::addr::SnesAddr::new(addr))
        {
            Ok(pc) => self
                .rom
                .data()
                .get(pc.as_usize())
                .copied()
                .unwrap_or_else(|| {
                    self.unmapped_reads += 1;
                    0
                }),
            Err(_) => {
                self.unmapped_reads += 1;
                0
            }
        }
    }
}

impl Bus for SmwBus<'_> {
    fn read(&mut self, addr: u32) -> u8 {
        let bank = (addr >> 16) as u8;
        let off = addr as u16;
        match bank {
            0x7E | 0x7F => self.wram[(addr - 0x7E_0000) as usize],
            0x00..=0x3F | 0x80..=0xBF => match off {
                0x0000..=0x1FFF => self.wram[off as usize],
                0x2000..=0x7FFF => {
                    self.unmapped_reads += 1;
                    0
                }
                _ => self.rom_read(addr),
            },
            0x70..=0x7D if off < 0x8000 => self.sram[(off as usize) % SRAM_LEN],
            _ => self.rom_read(addr),
        }
    }

    fn write(&mut self, addr: u32, value: u8) {
        let bank = (addr >> 16) as u8;
        let off = addr as u16;
        match bank {
            0x7E | 0x7F => self.wram[(addr - 0x7E_0000) as usize] = value,
            0x00..=0x3F | 0x80..=0xBF if off < 0x2000 => self.wram[off as usize] = value,
            0x70..=0x7D if off < 0x8000 => self.sram[(off as usize) % SRAM_LEN] = value,
            _ => self.unmapped_writes += 1,
        }
    }
}
