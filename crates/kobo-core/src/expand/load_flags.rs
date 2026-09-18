//! The sprite loader's per-entry "already loaded" flags.

use crate::cpu::Bus;
use crate::cpu::smw_bus::SmwBus;
use crate::ram::{self, Ram};

/// Where the ROM's sprite loader keeps its per-entry "already loaded"
/// flags: `$1938` (128 entries) in vanilla, or `$7FAF00` (256 entries)
/// when Lunar Magic 3's 255-sprites-per-level patch has replaced the
/// loader's flag check at `$02A856` with a jump to its own code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct LoadFlags {
    /// Bus address of the first flag.
    base: u32,
    count: u32,
}

impl LoadFlags {
    pub fn detect(bus: &mut SmwBus) -> Self {
        const FLAG_CHECK: u32 = 0x02_A856;
        const JML: u8 = 0x5C;
        const LM_FLAGS: u32 = 0x7F_AF00;
        if bus.read(FLAG_CHECK) == JML {
            let target = bus.read_u24(FLAG_CHECK + 1);
            let code: Vec<u8> = (0..0x60).map(|i| bus.read(target + i)).collect();
            if code.windows(3).any(|w| *w == LM_FLAGS.to_le_bytes()[..3]) {
                return Self {
                    base: LM_FLAGS,
                    count: 0x100,
                };
            }
        }
        Self {
            base: bus.ram.map().resolve(ram::SPRITE_LOAD_STATUS),
            count: ram::SPRITE_LOAD_STATUS_LEN,
        }
    }

    pub fn fill(self, ram: &mut Ram, value: u8) {
        for i in 0..self.count {
            ram.poke(self.base + i, value);
        }
    }

    pub fn read(self, ram: &Ram) -> Vec<u8> {
        (0..self.count).map(|i| ram.peek(self.base + i)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr::{Mapping, SnesAddr};
    use crate::rom::Rom;

    #[test]
    fn load_flags_follow_lunar_magics_255_sprite_patch() {
        let mut bytes = vec![0; 0x10_0000];
        let pc = |addr: u32| {
            Mapping::LoRom
                .snes_to_pc(SnesAddr::new(addr))
                .unwrap()
                .as_usize()
        };
        let vanilla = LoadFlags {
            base: 0x7E_1938,
            count: 0x80,
        };
        bytes[0x7FD5] = 0x20; // LoROM
        // Vanilla: `LDA $1938,X`.
        bytes[pc(0x02_A856)..][..3].copy_from_slice(&[0xBD, 0x38, 0x19]);
        let rom = Rom::from_bytes(bytes.clone()).unwrap();
        assert_eq!(LoadFlags::detect(&mut SmwBus::new(&rom)), vanilla);
        // Patched: `JML $128000`, which does `LDA $7FAF00,X`.
        bytes[pc(0x02_A856)..][..4].copy_from_slice(&[0x5C, 0x00, 0x80, 0x12]);
        bytes[pc(0x12_8000)..][..4].copy_from_slice(&[0xBF, 0x00, 0xAF, 0x7F]);
        let rom = Rom::from_bytes(bytes.clone()).unwrap();
        assert_eq!(
            LoadFlags::detect(&mut SmwBus::new(&rom)),
            LoadFlags {
                base: 0x7F_AF00,
                count: 0x100
            }
        );
        // Some other patch at the same place keeps the vanilla table.
        bytes[pc(0x12_8000)..][..4].copy_from_slice(&[0xBD, 0x38, 0x19, 0x6B]);
        let rom = Rom::from_bytes(bytes).unwrap();
        assert_eq!(LoadFlags::detect(&mut SmwBus::new(&rom)), vanilla);
    }
}
