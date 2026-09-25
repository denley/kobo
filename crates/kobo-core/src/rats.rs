//! RATS tags and the free space between them.
//!
//! A RATS tag is the eight bytes before a block: `STAR`, then the block's
//! length minus one and that value's complement, both 16-bit little-endian.
//! Asar and every tool built on it take any run of `$00` for free space
//! unless a valid tag covers it, and never let a block's contents cross a
//! bank. Kobo tags every block it places the same way, so the tools that
//! run after it leave its blocks alone, and places them first-fit in a
//! fixed order, so the same image and the same requests give the same
//! addresses. The placement follows Asar's `freecode` and `freedata`: on
//! a LoROM image, a sequence of requests lands byte for byte where Asar
//! 1.91 puts the same sequence.

use thiserror::Error;

use crate::addr::{Mapping, PcAddr, SnesAddr};
use crate::rom::{Rom, RomError};

/// Length of a RATS tag.
pub const TAG_LEN: usize = 8;

/// The longest block a tag can protect.
pub const MAX_BLOCK_LEN: usize = 0x1_0000;

const MAGIC: &[u8; 4] = b"STAR";

/// Where the searches start: `$108000` under every mapping, the first byte
/// past the vanilla image.
const SEARCH_START: usize = 0x08_0000;

/// The length of the block protected by a valid tag at file offset `pc`.
pub fn tag_at(data: &[u8], pc: usize) -> Option<usize> {
    let tag = data.get(pc..pc.checked_add(TAG_LEN)?)?;
    let valid = &tag[..4] == MAGIC && tag[4] ^ tag[6] == 0xFF && tag[5] ^ tag[7] == 0xFF;
    valid.then(|| u16::from_le_bytes([tag[4], tag[5]]) as usize + 1)
}

/// The tag for a block of `len` bytes, 1 to [`MAX_BLOCK_LEN`].
fn tag(len: usize) -> [u8; TAG_LEN] {
    let [lo, hi] = ((len - 1) as u16).to_le_bytes();
    [b'S', b'T', b'A', b'R', lo, hi, !lo, !hi]
}

/// A tagged block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RatsBlock {
    /// The first byte after the tag.
    pub start: SnesAddr,
    pub len: usize,
}

/// The tagged blocks from `$108000` on, in file order. As in Asar's walk,
/// a block is skipped whole, so a tag inside one is not another.
pub fn blocks(rom: &Rom) -> Vec<RatsBlock> {
    let data = rom.data();
    let mut found = Vec::new();
    let mut pc = SEARCH_START;
    while pc < data.len() {
        let Some(len) = tag_at(data, pc) else {
            pc += 1;
            continue;
        };
        if let Ok(start) = rom.mapping().pc_to_snes(PcAddr::new((pc + TAG_LEN) as u32)) {
            found.push(RatsBlock { start, len });
        }
        pc += TAG_LEN + len;
    }
    found
}

/// What a block holds, which decides where it may go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Contents {
    /// Code, which goes in a system bank (see [`SnesAddr::in_system_bank`]).
    Code,
    /// Data, which goes outside the system banks first, to save them for
    /// code, as Asar's `freedata` does.
    Data,
}

#[derive(Debug, Error)]
pub enum FreeSpaceError {
    #[error("a RATS block holds 1 to 65536 bytes, not {0}")]
    BadLength(usize),
    #[error("no free space for {len} bytes of {contents:?} within one bank")]
    Full { len: usize, contents: Contents },
    #[error(transparent)]
    Rom(#[from] RomError),
}

/// A run of free bytes, as file offsets.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Run {
    start: usize,
    end: usize,
}

/// An image's free space: the runs of `$00` from `$108000` on that no tag
/// covers. Scan it once a stage's writes to fixed addresses are done and
/// place everything else the stage writes through it: it does not see
/// what is written into free space behind its back.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreeSpace {
    mapping: Mapping,
    runs: Vec<Run>,
}

impl FreeSpace {
    pub fn scan(rom: &Rom) -> Self {
        let mapping = rom.mapping();
        let data = &rom.data()[..rom.len().min(mapping.max_rom_len())];
        let mut runs = Vec::new();
        let mut pc = SEARCH_START;
        while pc < data.len() {
            if let Some(len) = tag_at(data, pc) {
                pc += TAG_LEN + len;
                continue;
            }
            if data[pc] != 0 {
                pc += 1;
                continue;
            }
            let start = pc;
            while pc < data.len() && data[pc] == 0 {
                pc += 1;
            }
            runs.push(Run { start, end: pc });
        }
        Self { mapping, runs }
    }

    /// The free bytes left.
    pub fn free_bytes(&self) -> usize {
        self.runs.iter().map(|run| run.end - run.start).sum()
    }

    /// Places a block of `len` bytes, first-fit in file order, and writes
    /// its tag; the contents stay `$00` for the caller to write. Returns
    /// where they start. As in Asar, the contents stay in one bank, and
    /// when they would cross into the next, the tag goes in the last eight
    /// bytes of the bank and the contents start at the next one.
    pub fn alloc(
        &mut self,
        rom: &mut Rom,
        len: usize,
        contents: Contents,
    ) -> Result<SnesAddr, FreeSpaceError> {
        if !(1..=MAX_BLOCK_LEN).contains(&len) {
            return Err(FreeSpaceError::BadLength(len));
        }
        let first = |system: Option<bool>| {
            self.runs
                .iter()
                .enumerate()
                .find_map(|(index, run)| self.place(run, len, system).map(|tag| (index, tag)))
        };
        let (index, tag_pc) = match contents {
            Contents::Code => first(Some(true)),
            Contents::Data => first(Some(false)).or_else(|| first(None)),
        }
        .ok_or(FreeSpaceError::Full { len, contents })?;
        let at = self.snes(tag_pc);
        rom.write(at, &tag(len))?;
        let run = self.runs.remove(index);
        let after = Run {
            start: tag_pc + TAG_LEN + len,
            end: run.end,
        };
        let before = Run {
            start: run.start,
            end: tag_pc,
        };
        for part in [after, before] {
            if part.start < part.end {
                self.runs.insert(index, part);
            }
        }
        Ok(self.snes(tag_pc + TAG_LEN))
    }

    /// Where in a run a block's tag goes, if the block fits there with
    /// its contents in one bank, in a system bank or not if `system` says.
    fn place(&self, run: &Run, len: usize, system: Option<bool>) -> Option<usize> {
        let mut tag_pc = run.start;
        while tag_pc + TAG_LEN + len <= run.end {
            let contents = PcAddr::new((tag_pc + TAG_LEN) as u32);
            let bank_end = self
                .mapping
                .bank_end(contents)
                .expect("the image is mapped")
                .as_usize();
            let wanted =
                system.is_none_or(|s| self.snes(contents.as_usize()).in_system_bank() == s);
            if wanted && contents.as_usize() + len <= bank_end {
                return Some(tag_pc);
            }
            tag_pc = bank_end - TAG_LEN;
        }
        None
    }

    fn snes(&self, pc: usize) -> SnesAddr {
        self.mapping
            .pc_to_snes(PcAddr::new(pc as u32))
            .expect("the image is mapped")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: usize = 0x10_0000;
    const BANK: usize = 0x8000;

    /// An image of `len` bytes, all `$00` past the vanilla-sized head,
    /// which is `$FF`.
    fn image(map_mode: u8, len: usize) -> Rom {
        let mut data = vec![0xFF; len];
        data[SEARCH_START..].fill(0);
        data[0x7FD5] = map_mode;
        Rom::from_bytes(data).unwrap()
    }

    fn pc(rom: &Rom, addr: SnesAddr) -> usize {
        rom.pc(addr).unwrap().as_usize()
    }

    #[test]
    fn tags_are_checked_against_their_complement() {
        let mut data = vec![0; 16];
        data[2..10].copy_from_slice(&tag(0x1234));
        assert_eq!(&data[2..10], b"STAR\x33\x12\xCC\xED");
        assert_eq!(tag_at(&data, 2), Some(0x1234));
        assert_eq!(tag_at(&data, 3), None);
        assert_eq!(tag_at(&data, 9), None);
        data[9] ^= 1;
        assert_eq!(tag_at(&data, 2), None);
        assert_eq!(tag(MAX_BLOCK_LEN)[4..], [0xFF, 0xFF, 0x00, 0x00]);
    }

    #[test]
    fn walk_skips_whole_blocks() {
        let mut rom = image(0x20, MIB);
        let inner = SnesAddr::new(0x108010);
        rom.write(SnesAddr::new(0x108000), &tag(0x40)).unwrap();
        rom.write(inner, &tag(4)).unwrap();
        rom.write(SnesAddr::new(0x118000), &tag(2)).unwrap();
        assert_eq!(
            blocks(&rom),
            [
                RatsBlock {
                    start: SnesAddr::new(0x108008),
                    len: 0x40
                },
                RatsBlock {
                    start: SnesAddr::new(0x118008),
                    len: 2
                },
            ]
        );
    }

    #[test]
    fn free_space_is_untagged_zeros_within_a_bank() {
        let mut rom = image(0x20, MIB);
        // A tagged block of zeros is not free, and a stray byte splits a run.
        rom.write(SnesAddr::new(0x108000), &tag(0x100)).unwrap();
        rom.write_u8(SnesAddr::new(0x108200), 0x42).unwrap();
        let space = FreeSpace::scan(&rom);
        let runs: Vec<_> = space.runs.iter().map(|r| (r.start, r.end)).collect();
        assert_eq!(runs, [(0x80108, 0x80200), (0x80201, MIB)]);
        assert_eq!(space.free_bytes(), MIB / 2 - 0x109);
    }

    #[test]
    fn allocation_is_first_fit_and_tagged() {
        let mut rom = image(0x20, MIB);
        rom.write_u8(SnesAddr::new(0x108020), 0x42).unwrap();
        let mut space = FreeSpace::scan(&rom);
        let a = space.alloc(&mut rom, 0x10, Contents::Code).unwrap();
        let b = space.alloc(&mut rom, 0x20, Contents::Code).unwrap();
        let c = space.alloc(&mut rom, 0x08, Contents::Data).unwrap();
        assert_eq!(a, SnesAddr::new(0x108008));
        assert_eq!(b, SnesAddr::new(0x108029));
        assert_eq!(c, SnesAddr::new(0x108051));
        assert_eq!(tag_at(rom.data(), pc(&rom, a) - TAG_LEN), Some(0x10));
        let found: Vec<_> = blocks(&rom).iter().map(|b| b.start).collect();
        assert_eq!(found, [a, b, c]);
        // A rescan of the result carries on where the allocator is.
        let mut copy = Rom::from_bytes(rom.data().to_vec()).unwrap();
        let mut rescanned = FreeSpace::scan(&copy);
        assert_eq!(rescanned, space);
        assert_eq!(
            rescanned.alloc(&mut copy, 0x300, Contents::Code).unwrap(),
            space.alloc(&mut rom, 0x300, Contents::Code).unwrap()
        );
        assert_eq!(copy.data(), rom.data());
    }

    #[test]
    fn contents_never_cross_a_bank() {
        let mut rom = image(0x20, MIB);
        let mut space = FreeSpace::scan(&rom);
        let a = space.alloc(&mut rom, 0x10, Contents::Code).unwrap();
        // The rest of bank $10 is too short: the tag goes at its end.
        let b = space.alloc(&mut rom, BANK - 0x10, Contents::Code).unwrap();
        let c = space.alloc(&mut rom, 0x20, Contents::Code).unwrap();
        let d = space.alloc(&mut rom, BANK, Contents::Code).unwrap();
        assert_eq!(a, SnesAddr::new(0x108008));
        assert_eq!(b, SnesAddr::new(0x118000));
        assert_eq!(tag_at(rom.data(), 0x87FF8), Some(BANK - 0x10));
        // What the move left behind is still free.
        assert_eq!(c, SnesAddr::new(0x108020));
        assert_eq!(d, SnesAddr::new(0x128000));
        assert!(matches!(
            space.alloc(&mut rom, BANK + 1, Contents::Data),
            Err(FreeSpaceError::Full { .. })
        ));
        assert!(matches!(
            space.alloc(&mut rom, 0, Contents::Data),
            Err(FreeSpaceError::BadLength(0))
        ));
    }

    #[test]
    fn lorom_data_goes_past_the_system_banks_first() {
        let mut rom = image(0x20, 4 * MIB);
        let mut space = FreeSpace::scan(&rom);
        let data = space.alloc(&mut rom, 0x100, Contents::Data).unwrap();
        let code = space.alloc(&mut rom, 0x100, Contents::Code).unwrap();
        assert_eq!(data, SnesAddr::new(0x408000));
        assert_eq!(tag_at(rom.data(), 0x1FFFF8), Some(0x100));
        assert_eq!(code, SnesAddr::new(0x108008));
        // Code never goes there, and data falls back to the system banks.
        let mut rom = image(0x20, MIB);
        let mut space = FreeSpace::scan(&rom);
        let data = space.alloc(&mut rom, 0x100, Contents::Data).unwrap();
        assert_eq!(data, SnesAddr::new(0x108008));
    }

    #[test]
    fn sa1_images() {
        // Up to 4 MiB, every bank is a system bank, to the last.
        let mut rom = image(0x23, 4 * MIB);
        let mut space = FreeSpace::scan(&rom);
        let data = space.alloc(&mut rom, 0x100, Contents::Data).unwrap();
        assert_eq!(data, SnesAddr::new(0x108008));
        let mut rom = image(0x23, 4 * MIB);
        let last = pc(&rom, SnesAddr::new(0xBF8000));
        rom.write(SnesAddr::new(0x108000), &vec![1; last - SEARCH_START])
            .unwrap();
        let mut space = FreeSpace::scan(&rom);
        let code = space.alloc(&mut rom, 0x100, Contents::Code).unwrap();
        assert_eq!(code, SnesAddr::new(0xBF8008));

        // Past 4 MiB, data goes in the HiROM view's 64 KiB banks first.
        let mut rom = image(0x23, 8 * MIB);
        let mut space = FreeSpace::scan(&rom);
        let data = space.alloc(&mut rom, 0xC000, Contents::Data).unwrap();
        let more = space.alloc(&mut rom, 0x8000, Contents::Data).unwrap();
        let code = space.alloc(&mut rom, 0x100, Contents::Code).unwrap();
        assert_eq!(data, SnesAddr::new(0xC00000));
        assert_eq!(more, SnesAddr::new(0xC10000));
        assert_eq!(code, SnesAddr::new(0x108008));
        assert_eq!(pc(&rom, more), 0x410000);
        assert!(matches!(
            space.alloc(&mut rom, MAX_BLOCK_LEN + 1, Contents::Data),
            Err(FreeSpaceError::BadLength(_))
        ));
    }
}
