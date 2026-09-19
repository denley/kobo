//! The SA-1: a second 65816 on the cartridge, and the registers at
//! `$2200`-`$23FF` the two processors talk through.
//!
//! What is modelled is what a game needs to hand work across: the
//! control registers with their message nibbles and IRQ flags, the
//! vectors, the BW-RAM windows, and the arithmetic unit. Timers, DMA with
//! character conversion, the variable-length bit reader, and write
//! protection are not; the Super MMC stays at the bank assignment
//! [`crate::addr::Mapping::Sa1Rom`] describes.

use super::{Cpu, CpuError};

/// Which of the two processors is on the bus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Processor {
    /// The console's CPU.
    Main,
    Sa1,
}

/// How banks `$60`-`$6F` slice BW-RAM into cells (`BBF`, `$223F`).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Bitmap {
    #[default]
    FourBits,
    TwoBits,
}

impl Bitmap {
    /// BW-RAM byte, shift, and mask of cell `at`.
    pub fn cell(self, at: usize) -> (usize, u32, u8) {
        match self {
            Self::FourBits => (at / 2, (at as u32 & 1) * 4, 0x0F),
            Self::TwoBits => (at / 4, (at as u32 & 3) * 2, 0x03),
        }
    }
}

/// One direction of the link between the processors: a four-bit message
/// and an IRQ the receiver can mask and has to acknowledge.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Link {
    message: u8,
    irq: bool,
    irq_enabled: bool,
}

impl Link {
    fn send(&mut self, control: u8) {
        self.message = control & 0x0F;
        self.irq |= control & 0x80 != 0;
    }

    fn line(self) -> bool {
        self.irq && self.irq_enabled
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sa1 {
    /// The SA-1's CPU. While it runs it is out on the bus and this is a
    /// placeholder.
    pub cpu: Cpu,
    /// Held in reset (`CCNT` bit 5), as it is from power-on, or stopped
    /// (`CCNT` bit 6).
    held: bool,
    /// What stopped the CPU for good, if anything has.
    pub fault: Option<CpuError>,
    to_sa1: Link,
    to_main: Link,
    /// `CRV`, `CNV`, `CIV`: the SA-1 takes its vectors from registers.
    reset_vector: u16,
    irq_vector: u16,
    /// `SIV`, which replaces the S-CPU's IRQ vector when `SCNT` bit 6 is
    /// set.
    main_irq_vector: u16,
    main_irq_vector_selected: bool,
    /// `BMAPS` and `BMAP`: the BW-RAM block at `$6000`-`$7FFF` for the
    /// S-CPU and for the SA-1. Bit 7 of the SA-1's selects the bitmap
    /// view instead.
    pub bwram_window: [u8; 2],
    pub bitmap: Bitmap,
    arithmetic: Arithmetic,
}

impl Default for Sa1 {
    fn default() -> Self {
        Self {
            cpu: Cpu::new(),
            held: true,
            fault: None,
            to_sa1: Link::default(),
            to_main: Link::default(),
            reset_vector: 0,
            irq_vector: 0,
            main_irq_vector: 0,
            main_irq_vector_selected: false,
            bwram_window: [0; 2],
            bitmap: Bitmap::default(),
            arithmetic: Arithmetic::default(),
        }
    }
}

/// `MCNT`, `MA`, `MB`, and the 40-bit result `MR`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Arithmetic {
    divide: bool,
    cumulative: bool,
    a: u16,
    b: u16,
    result: u64,
    overflow: bool,
}

impl Arithmetic {
    const RESULT_MASK: u64 = (1 << 40) - 1;

    fn control(&mut self, value: u8) {
        self.divide = value & 1 != 0;
        self.cumulative = value & 2 != 0;
        if self.cumulative {
            self.result = 0;
        }
    }

    /// Writing the high byte of `MB` starts the operation.
    fn run(&mut self) {
        let (a, b) = (self.a as i16 as i64, self.b);
        if self.cumulative {
            let sum = self.result.wrapping_add((a * b as i16 as i64) as u64);
            self.overflow = sum >> 40 != 0;
            self.result = sum & Self::RESULT_MASK;
        } else if self.divide {
            // Signed dividend, unsigned divisor, remainder never negative.
            self.result = match b as i64 {
                0 => 0,
                b => (a.rem_euclid(b) as u64) << 16 | (a.div_euclid(b) as u64 & 0xFFFF),
            };
            self.a = 0;
        } else {
            self.result = (a * b as i16 as i64) as u64 & 0xFFFF_FFFF;
        }
        self.b = 0;
    }
}

fn set_low(word: &mut u16, value: u8) {
    *word = (*word & 0xFF00) | value as u16;
}

fn set_high(word: &mut u16, value: u8) {
    *word = (*word & 0x00FF) | (value as u16) << 8;
}

impl Sa1 {
    /// Whether the CPU has anything to run on.
    pub fn runnable(&self) -> bool {
        !self.held && self.fault.is_none()
    }

    /// The IRQ line into `processor`.
    pub fn irq(&self, processor: Processor) -> bool {
        match processor {
            Processor::Main => self.to_main.line(),
            Processor::Sa1 => self.to_sa1.line(),
        }
    }

    pub fn irq_vector(&self) -> u16 {
        self.irq_vector
    }

    /// The S-CPU's IRQ vector, if the SA-1 has replaced the cartridge's.
    pub fn main_irq_vector(&self) -> Option<u16> {
        self.main_irq_vector_selected
            .then_some(self.main_irq_vector)
    }

    /// Reads a register, or `None` if `processor` has nothing there.
    pub fn read(&self, processor: Processor, reg: u16) -> Option<u8> {
        let flags = |link: Link| (link.irq as u8) << 7 | link.message;
        Some(match (processor, reg) {
            (Processor::Main, 0x2300) => {
                flags(self.to_main) | (self.main_irq_vector_selected as u8) << 6
            }
            (Processor::Sa1, 0x2301) => flags(self.to_sa1),
            (Processor::Sa1, 0x2306..=0x230A) => {
                (self.arithmetic.result >> (8 * (reg - 0x2306))) as u8
            }
            (Processor::Sa1, 0x230B) => (self.arithmetic.overflow as u8) << 7,
            _ => return None,
        })
    }

    /// Writes a register; false if `processor` has nothing there.
    pub fn write(&mut self, processor: Processor, reg: u16, value: u8) -> bool {
        match (processor, reg) {
            (Processor::Main, 0x2200) => {
                self.to_sa1.send(value);
                let held = value & 0x60 != 0;
                if self.held && value & 0x20 == 0 {
                    // Out of reset: a 65816 starts in emulation mode.
                    self.cpu = Cpu::new();
                    self.cpu.emulation = true;
                    self.cpu.pc = self.reset_vector;
                    self.fault = None;
                }
                self.held = held;
            }
            (Processor::Main, 0x2201) => self.to_main.irq_enabled = value & 0x80 != 0,
            (Processor::Main, 0x2202) => self.to_main.irq &= value & 0x80 == 0,
            (Processor::Main, 0x2203) => set_low(&mut self.reset_vector, value),
            (Processor::Main, 0x2204) => set_high(&mut self.reset_vector, value),
            (Processor::Main, 0x2207) => set_low(&mut self.irq_vector, value),
            (Processor::Main, 0x2208) => set_high(&mut self.irq_vector, value),
            (Processor::Sa1, 0x2209) => {
                self.to_main.send(value);
                self.main_irq_vector_selected = value & 0x40 != 0;
            }
            (Processor::Sa1, 0x220A) => self.to_sa1.irq_enabled = value & 0x80 != 0,
            (Processor::Sa1, 0x220B) => self.to_sa1.irq &= value & 0x80 == 0,
            (Processor::Sa1, 0x220E) => set_low(&mut self.main_irq_vector, value),
            (Processor::Sa1, 0x220F) => set_high(&mut self.main_irq_vector, value),
            (Processor::Main, 0x2224) => self.bwram_window[0] = value & 0x1F,
            (Processor::Sa1, 0x2225) => self.bwram_window[1] = value,
            (Processor::Sa1, 0x223F) => {
                self.bitmap = if value & 0x80 != 0 {
                    Bitmap::TwoBits
                } else {
                    Bitmap::FourBits
                };
            }
            (Processor::Sa1, 0x2250) => self.arithmetic.control(value),
            (Processor::Sa1, 0x2251) => set_low(&mut self.arithmetic.a, value),
            (Processor::Sa1, 0x2252) => set_high(&mut self.arithmetic.a, value),
            (Processor::Sa1, 0x2253) => set_low(&mut self.arithmetic.b, value),
            (Processor::Sa1, 0x2254) => {
                set_high(&mut self.arithmetic.b, value);
                self.arithmetic.run();
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arithmetic(sa1: &mut Sa1, control: u8, a: u16, b: u16) -> u64 {
        let [a_low, a_high] = a.to_le_bytes();
        let [b_low, b_high] = b.to_le_bytes();
        for (reg, value) in [
            (0x2250, control),
            (0x2251, a_low),
            (0x2252, a_high),
            (0x2253, b_low),
            (0x2254, b_high),
        ] {
            assert!(sa1.write(Processor::Sa1, reg, value));
        }
        (0..5).fold(0, |result, i| {
            result | (sa1.read(Processor::Sa1, 0x2306 + i).unwrap() as u64) << (8 * i)
        })
    }

    #[test]
    fn arithmetic_is_signed_with_an_unsigned_divisor() {
        let mut sa1 = Sa1::default();
        assert_eq!(arithmetic(&mut sa1, 0, 0xFFFE, 3), 0xFFFF_FFFA); // -2 * 3
        assert_eq!(arithmetic(&mut sa1, 0, 300, 300), 90_000);
        // -7 / 2 = -4 remainder 1; the remainder is in the upper word.
        assert_eq!(arithmetic(&mut sa1, 1, 0xFFF9, 2), 0x0001_FFFC);
        assert_eq!(arithmetic(&mut sa1, 1, 100, 0), 0);
        // Selecting the cumulative sum clears it; each run adds a product.
        assert_eq!(arithmetic(&mut sa1, 2, 1000, 1000), 1_000_000);
        for (reg, value) in [(0x2251, 0xFF), (0x2252, 0xFF), (0x2253, 1), (0x2254, 0)] {
            sa1.write(Processor::Sa1, reg, value);
        }
        assert_eq!(sa1.arithmetic.result, 999_999);
    }

    #[test]
    fn each_processor_raises_and_the_other_acknowledges() {
        let mut sa1 = Sa1::default();
        assert!(!sa1.runnable());
        sa1.write(Processor::Main, 0x2203, 0x34);
        sa1.write(Processor::Main, 0x2204, 0x12);
        sa1.write(Processor::Main, 0x2200, 0x00);
        assert!(sa1.runnable());
        assert_eq!((sa1.cpu.pc, sa1.cpu.emulation), (0x1234, true));

        sa1.write(Processor::Main, 0x2200, 0x83);
        assert_eq!(sa1.read(Processor::Sa1, 0x2301), Some(0x83));
        assert!(!sa1.irq(Processor::Sa1)); // masked until `CIE` allows it
        sa1.write(Processor::Sa1, 0x220A, 0x80);
        assert!(sa1.irq(Processor::Sa1));
        sa1.write(Processor::Sa1, 0x220B, 0x80);
        assert!(!sa1.irq(Processor::Sa1));
        // The S-CPU cannot write the SA-1's side, nor read its flags.
        assert!(!sa1.write(Processor::Main, 0x2209, 0x80));
        assert_eq!(sa1.read(Processor::Main, 0x2301), None);

        sa1.write(Processor::Sa1, 0x220E, 0x00);
        sa1.write(Processor::Sa1, 0x220F, 0x1D);
        sa1.write(Processor::Sa1, 0x2209, 0xC5);
        sa1.write(Processor::Main, 0x2201, 0x80);
        assert!(sa1.irq(Processor::Main));
        assert_eq!(sa1.read(Processor::Main, 0x2300), Some(0xC5));
        assert_eq!(sa1.main_irq_vector(), Some(0x1D00));
    }
}
