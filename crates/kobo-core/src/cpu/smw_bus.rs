//! A bus that maps a SMW ROM plus work RAM, and captures what the game
//! uploads to video memory.
//!
//! Only the pieces of hardware the loaders touch are modelled: the VRAM
//! and CGRAM data ports, and general-purpose DMA to them. Everything
//! else reads as zero and ignores writes.

use crate::cpu::Bus;
use crate::rom::Rom;

pub const WRAM_LEN: usize = 0x2_0000;
pub const VRAM_LEN: usize = 0x1_0000;
pub const CGRAM_LEN: usize = 0x200;
const SRAM_LEN: usize = 0x8000;

#[derive(Clone, Copy, Default, Debug)]
struct DmaChannel {
    control: u8,
    dest: u8,
    src: u32,
    size: u16,
}

/// ROM, 128 KiB of WRAM, VRAM, CGRAM, a scratch SRAM, and enough register
/// state to follow uploads.
pub struct SmwBus<'a> {
    pub rom: &'a Rom,
    pub wram: Vec<u8>,
    pub sram: Vec<u8>,
    pub vram: Vec<u8>,
    /// Which VRAM bytes have been written since reset, so callers can
    /// tell uploaded data from the untouched zero fill.
    pub vram_written: Vec<bool>,
    pub cgram: Vec<u8>,
    vmain: u8,
    /// VRAM word address.
    vmadd: u16,
    /// CGRAM byte address (colour index * 2 + half).
    cgadd: u16,
    /// Last values written to `BG1SC`-`BG4SC` (`$2107`-`$210A`): tilemap
    /// VRAM base and size per layer. Lunar Magic moves the layer 1 and 2
    /// tilemaps, so these are how to find them.
    pub bg_sc: [u8; 4],
    pub bg_character_base: [u16; 4],
    pub bg_scroll: [[u16; 2]; 4],
    pub bg_mode: u8,
    pub object_select: u8,
    pub mode7: crate::video::Mode7,
    bg_scroll_latch: u8,
    mode7_latch: u8,
    pub irq_scanline: u16,
    pub interrupt_enable: u8,
    hblank: bool,
    dma: [DmaChannel; 8],
    /// Last values written to the APU I/O ports `$2140`-`$2143`.
    apu_ports: [u8; 4],
    /// Models the SPC700 boot ROM upload protocol: outside a transfer the
    /// ports read `$AA`/`$BB` while port 0 holds zero; during a transfer
    /// every write is echoed. A block header with port 1 zero ends the
    /// transfer (the boot ROM jumps to the uploaded code, and SMW's sound
    /// engine then presents `$AA`/`$BB` again for the sample upload).
    apu_in_transfer: bool,
    apu_expected: u8,
    /// The block-end header byte, echoed exactly once after a transfer
    /// ends before the ports go back to `$AA`/`$BB`.
    apu_jump_echo: Option<u8>,
    /// A block header was just written to port 0; a zero written to
    /// port 1 right after it ends the transfer (AddmusicK's ordering).
    apu_header_pending: bool,
    /// Number of reads that hit unmapped or unmodelled register space.
    pub unmapped_reads: u64,
    pub unmapped_writes: u64,
    /// When set, every ROM read address is appended here.
    pub trace_rom_reads: Option<Vec<u32>>,
}

impl<'a> SmwBus<'a> {
    pub fn new(rom: &'a Rom) -> Self {
        Self {
            rom,
            wram: vec![0; WRAM_LEN],
            sram: vec![0; SRAM_LEN],
            vram: vec![0; VRAM_LEN],
            vram_written: vec![false; VRAM_LEN],
            cgram: vec![0; CGRAM_LEN],
            vmain: 0,
            vmadd: 0,
            cgadd: 0,
            bg_sc: [0; 4],
            bg_character_base: [0; 4],
            bg_scroll: [[0; 2]; 4],
            bg_mode: 0,
            object_select: 0,
            mode7: crate::video::Mode7::default(),
            bg_scroll_latch: 0,
            mode7_latch: 0,
            irq_scanline: 0,
            interrupt_enable: 0,
            hblank: false,
            dma: [DmaChannel::default(); 8],
            apu_ports: [0; 4],
            apu_in_transfer: false,
            apu_expected: 0,
            apu_jump_echo: None,
            apu_header_pending: false,
            unmapped_reads: 0,
            unmapped_writes: 0,
            trace_rom_reads: None,
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
        if let Some(trace) = &mut self.trace_rom_reads {
            trace.push(addr);
        }
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

    fn vram_step(&self) -> u16 {
        match self.vmain & 0x03 {
            0 => 1,
            1 => 32,
            _ => 128,
        }
    }

    /// Applies the VRAM address remapping selected by VMAIN bits 2-3.
    fn vram_remap(&self, addr: u16) -> u16 {
        match (self.vmain >> 2) & 0x03 {
            0 => addr,
            1 => (addr & 0xFF00) | ((addr & 0x001F) << 3) | ((addr & 0x00E0) >> 5),
            2 => (addr & 0xFE00) | ((addr & 0x003F) << 3) | ((addr & 0x01C0) >> 6),
            _ => (addr & 0xFC00) | ((addr & 0x007F) << 3) | ((addr & 0x0380) >> 7),
        }
    }

    fn write_register(&mut self, reg: u16, value: u8) {
        match reg {
            0x2101 => self.object_select = value,
            0x2105 => self.bg_mode = value,
            0x2107..=0x210A => self.bg_sc[(reg - 0x2107) as usize] = value,
            0x210B..=0x210C => {
                let layer = (reg - 0x210B) as usize * 2;
                self.bg_character_base[layer] = ((value & 0x0F) as u16) << 13;
                self.bg_character_base[layer + 1] = ((value >> 4) as u16) << 13;
            }
            0x210D..=0x2114 => {
                let layer = (reg - 0x210D) as usize / 2;
                let axis = (reg - 0x210D) as usize % 2;
                let low = if axis == 0 {
                    (self.bg_scroll_latch & 0xF8) | ((self.bg_scroll[layer][axis] >> 8) as u8 & 7)
                } else {
                    self.bg_scroll_latch
                };
                self.bg_scroll[layer][axis] = u16::from_le_bytes([low, value]);
                self.bg_scroll_latch = value;
                if layer == 0 {
                    self.mode7.scroll[axis] = u16::from_le_bytes([self.mode7_latch, value]);
                    self.mode7_latch = value;
                }
            }
            0x211A => self.mode7.control = value,
            0x211B..=0x2120 => {
                let word = u16::from_le_bytes([self.mode7_latch, value]);
                if reg <= 0x211E {
                    self.mode7.matrix[(reg - 0x211B) as usize] = word as i16;
                } else {
                    self.mode7.center[(reg - 0x211F) as usize] = word;
                }
                self.mode7_latch = value;
            }
            0x2115 => self.vmain = value,
            0x2116 => self.vmadd = (self.vmadd & 0xFF00) | value as u16,
            0x2117 => self.vmadd = (self.vmadd & 0x00FF) | ((value as u16) << 8),
            0x2118 => {
                let a = (self.vram_remap(self.vmadd) as usize * 2) % VRAM_LEN;
                self.vram[a] = value;
                self.vram_written[a] = true;
                if self.vmain & 0x80 == 0 {
                    self.vmadd = self.vmadd.wrapping_add(self.vram_step());
                }
            }
            0x2119 => {
                let a = (self.vram_remap(self.vmadd) as usize * 2 + 1) % VRAM_LEN;
                self.vram[a] = value;
                self.vram_written[a] = true;
                if self.vmain & 0x80 != 0 {
                    self.vmadd = self.vmadd.wrapping_add(self.vram_step());
                }
            }
            0x2121 => self.cgadd = value as u16 * 2,
            0x2122 => {
                self.cgram[self.cgadd as usize % CGRAM_LEN] = value;
                self.cgadd = self.cgadd.wrapping_add(1);
            }
            0x2140..=0x2143 => {
                let port = (reg & 3) as usize;
                self.apu_ports[port] = value;
                if port == 0 {
                    self.apu_header_pending = false;
                    if !self.apu_in_transfer {
                        if value == 0xCC && self.apu_ports[1] != 0 {
                            self.apu_in_transfer = true;
                            self.apu_expected = 0;
                        }
                    } else if value == self.apu_expected {
                        self.apu_expected = self.apu_expected.wrapping_add(1);
                    } else if self.apu_ports[1] == 0 {
                        self.apu_in_transfer = false;
                        self.apu_jump_echo = Some(value);
                    } else {
                        self.apu_expected = 0;
                        self.apu_header_pending = true;
                    }
                } else if port == 1 && value == 0 && self.apu_in_transfer && self.apu_header_pending
                {
                    self.apu_in_transfer = false;
                    self.apu_header_pending = false;
                    self.apu_jump_echo = Some(self.apu_ports[0]);
                }
            }
            0x420B => self.run_dma(value),
            0x4200 => self.interrupt_enable = value,
            0x4209 => self.irq_scanline = (self.irq_scanline & 0x100) | value as u16,
            0x420A => self.irq_scanline = (self.irq_scanline & 0xFF) | (((value & 1) as u16) << 8),
            0x4300..=0x437F => {
                let ch = &mut self.dma[((reg >> 4) & 7) as usize];
                match reg & 0x0F {
                    0x0 => ch.control = value,
                    0x1 => ch.dest = value,
                    0x2 => ch.src = (ch.src & 0xFF_FF00) | value as u32,
                    0x3 => ch.src = (ch.src & 0xFF_00FF) | ((value as u32) << 8),
                    0x4 => ch.src = (ch.src & 0x00_FFFF) | ((value as u32) << 16),
                    0x5 => ch.size = (ch.size & 0xFF00) | value as u16,
                    0x6 => ch.size = (ch.size & 0x00FF) | ((value as u16) << 8),
                    _ => {}
                }
            }
            _ => self.unmapped_writes += 1,
        }
    }

    /// Executes the general-purpose DMA channels enabled in `mask`,
    /// transferring from the A bus to the B bus registers.
    fn run_dma(&mut self, mask: u8) {
        for ch in 0..8 {
            if mask & (1 << ch) == 0 {
                continue;
            }
            let c = self.dma[ch];
            if c.control & 0x80 != 0 {
                // B to A transfers (VRAM reads) are not needed by the loaders.
                self.unmapped_writes += 1;
                continue;
            }
            let regs: [u16; 4] = {
                let base = 0x2100 | c.dest as u16;
                match c.control & 0x07 {
                    0 => [base; 4],
                    1 => [base, base + 1, base, base + 1],
                    2 | 6 => [base; 4],
                    3 | 7 => [base, base, base + 1, base + 1],
                    4 => [base, base + 1, base + 2, base + 3],
                    _ => [base, base + 1, base, base + 1],
                }
            };
            let fixed = c.control & 0x08 != 0;
            let step: i32 = if c.control & 0x10 != 0 { -1 } else { 1 };
            let mut src = c.src;
            let mut count = if c.size == 0 { 0x1_0000 } else { c.size as u32 };
            let mut i = 0;
            while count > 0 {
                let value = self.read(src);
                self.write_register(regs[i & 3], value);
                if !fixed {
                    let off = ((src as u16) as i32 + step) as u16;
                    src = (src & 0xFF_0000) | off as u32;
                }
                i += 1;
                count -= 1;
            }
            self.dma[ch].src = src;
            self.dma[ch].size = 0;
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
                0x2140..=0x2143 => {
                    let i = (off & 3) as usize;
                    if self.apu_in_transfer || i >= 2 {
                        self.apu_ports[i]
                    } else if i == 0 && self.apu_jump_echo.is_some() {
                        self.apu_jump_echo.take().unwrap()
                    } else {
                        [0xAA, 0xBB][i]
                    }
                }
                0x4212 => {
                    // Let the ROM's wait-for-HBlank handshake finish.
                    // This is a headless loader, not a cycle-timed PPU.
                    let value = if self.hblank { 0x40 } else { 0 };
                    self.hblank = !self.hblank;
                    value
                }
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
            0x00..=0x3F | 0x80..=0xBF if off < 0x8000 => self.write_register(off, value),
            0x70..=0x7D if off < 0x8000 => self.sram[(off as usize) % SRAM_LEN] = value,
            _ => self.unmapped_writes += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::Rom;

    fn rom() -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x7FC0 + 0x15] = 0x20;
        for (i, b) in data.iter_mut().enumerate().take(0x100) {
            *b = i as u8;
        }
        Rom::from_bytes(data).unwrap()
    }

    #[test]
    fn video_registers_keep_separate_shared_latches() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        bus.write(0x210B, 0x73);
        assert_eq!(&bus.bg_character_base[..2], &[0x6000, 0xE000]);
        bus.write(0x210D, 0xAB);
        bus.write(0x210D, 0x01);
        assert_eq!(bus.bg_scroll[0][0], 0x01AB);
        assert_eq!(bus.mode7.scroll[0], 0x01AB);
        bus.write(0x211B, 0xFE);
        bus.write(0x211C, 0xFF); // Matrix registers share a latch.
        assert_eq!(bus.mode7.matrix[1], -2);
        bus.write(0x210E, 0x02);
        assert_eq!(bus.bg_scroll[0][1], 0x0201);
        assert_eq!(bus.mode7.scroll[1], 0x02FF);
        bus.write(0x4209, 0xAE);
        bus.write(0x420A, 0xFF);
        assert_eq!(bus.irq_scanline, 0x1AE);
        assert_eq!(
            [bus.read(0x4212), bus.read(0x4212), bus.read(0x4212)],
            [0, 0x40, 0]
        );
    }

    #[test]
    fn vram_port_writes_words() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        bus.write(0x002115, 0x80); // increment after high byte, step 1 word
        bus.write(0x002116, 0x00);
        bus.write(0x002117, 0x10); // word address $1000
        bus.write(0x002118, 0xAA);
        bus.write(0x002119, 0xBB);
        bus.write(0x002118, 0xCC);
        bus.write(0x002119, 0xDD);
        assert_eq!(&bus.vram[0x2000..0x2004], &[0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn dma_to_cgram_and_vram() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        // CGRAM: colour index 4, 8 bytes from ROM $008010 via channel 0, mode 0 to $2122.
        bus.write(0x002121, 0x04);
        bus.write(0x004300, 0x00);
        bus.write(0x004301, 0x22);
        bus.write(0x004302, 0x10);
        bus.write(0x004303, 0x80);
        bus.write(0x004304, 0x00);
        bus.write(0x004305, 0x08);
        bus.write(0x004306, 0x00);
        bus.write(0x00420B, 0x01);
        assert_eq!(
            &bus.cgram[8..16],
            &[0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17]
        );
        // VRAM: mode 1 to $2118/$2119, 6 bytes from WRAM $7E1000 via channel 1.
        bus.wram[0x1000..0x1006].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        bus.write(0x002115, 0x80);
        bus.write(0x002116, 0x00);
        bus.write(0x002117, 0x20);
        bus.write(0x004310, 0x01);
        bus.write(0x004311, 0x18);
        bus.write(0x004312, 0x00);
        bus.write(0x004313, 0x10);
        bus.write(0x004314, 0x7E);
        bus.write(0x004315, 0x06);
        bus.write(0x004316, 0x00);
        bus.write(0x00420B, 0x02);
        assert_eq!(&bus.vram[0x4000..0x4006], &[1, 2, 3, 4, 5, 6]);
        // Fixed-source fill: 4 bytes of the same value.
        bus.write(0x002116, 0x00);
        bus.write(0x002117, 0x30);
        bus.wram[0x1100] = 0x25;
        bus.write(0x004310, 0x09);
        bus.write(0x004312, 0x00);
        bus.write(0x004313, 0x11);
        bus.write(0x004315, 0x04);
        bus.write(0x00420B, 0x02);
        assert_eq!(&bus.vram[0x6000..0x6004], &[0x25; 4]);
    }
}
