//! Loading and identifying SMW ROM images.
//!
//! A [`Rom`] holds the headerless image. Copier headers (the 512-byte prefix
//! some dumps carry) are stripped on load and remembered so they can be
//! written back out. All reads take SNES addresses and go through the ROM's
//! [`Mapping`].

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};
use thiserror::Error;

use crate::addr::{MapError, Mapping, PcAddr, SnesAddr};

/// Size of the optional copier header some dumps carry.
pub const COPIER_HEADER_LEN: usize = 512;

/// ROM bank size. Every valid image is a multiple of this.
const BANK_LEN: usize = 0x8000;

/// Offset of the internal header for LoROM and SA-1 images.
const INTERNAL_HEADER: PcAddr = PcAddr::new(0x7FC0);

/// SHA-1 of the headerless No-Intro "Super Mario World (USA)" image.
pub const VANILLA_USA_SHA1: [u8; 20] = [
    0x6b, 0x47, 0xbb, 0x75, 0xd1, 0x65, 0x14, 0xb6, 0xa4, 0x76, 0xaa, 0x0c, 0x73, 0xa6, 0x83, 0xa2,
    0xa4, 0xc1, 0x87, 0x65,
];

#[derive(Debug, Error)]
pub enum RomError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("ROM size {0} is not a multiple of 32 KiB, with or without a copier header")]
    BadSize(usize),
    #[error("ROM is too small to contain an internal header")]
    TooSmall,
    #[error("unsupported cartridge mapping (map mode ${0:02X})")]
    UnsupportedMapping(u8),
    #[error(transparent)]
    Map(#[from] MapError),
    #[error("read of {len} bytes at {addr} ({pc}) runs past the end of the ROM")]
    OutOfBounds {
        addr: SnesAddr,
        pc: PcAddr,
        len: usize,
    },
}

/// What a loaded ROM was recognised as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RomIdentity {
    /// Byte-identical to the vanilla USA release.
    VanillaUsa,
    /// Not a known vanilla image. Most likely a hack or an expanded ROM.
    Unknown,
}

/// The internal header at `$00FFC0`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InternalHeader {
    /// 21-character title, trailing spaces trimmed.
    pub title: String,
    pub map_mode: u8,
    pub cartridge_type: u8,
    pub rom_size_code: u8,
    pub sram_size_code: u8,
    pub region: u8,
    pub developer_id: u8,
    pub version: u8,
    pub checksum_complement: u16,
    pub checksum: u16,
}

impl InternalHeader {
    fn parse(bytes: &[u8]) -> Self {
        let title = String::from_utf8_lossy(&bytes[0x00..0x15])
            .trim_end()
            .to_string();
        Self {
            title,
            map_mode: bytes[0x15],
            cartridge_type: bytes[0x16],
            rom_size_code: bytes[0x17],
            sram_size_code: bytes[0x18],
            region: bytes[0x19],
            developer_id: bytes[0x1A],
            version: bytes[0x1B],
            checksum_complement: u16::from_le_bytes([bytes[0x1C], bytes[0x1D]]),
            checksum: u16::from_le_bytes([bytes[0x1E], bytes[0x1F]]),
        }
    }

    /// Declared ROM size in bytes.
    pub fn rom_size(&self) -> usize {
        1usize << (self.rom_size_code as u32 + 10)
    }

    /// Declared SRAM size in bytes.
    pub fn sram_size(&self) -> usize {
        if self.sram_size_code == 0 {
            0
        } else {
            1usize << (self.sram_size_code as u32 + 10)
        }
    }

    /// Whether the checksum and its complement agree with each other.
    pub fn checksum_pair_valid(&self) -> bool {
        self.checksum ^ self.checksum_complement == 0xFFFF
    }
}

pub struct Rom {
    data: Vec<u8>,
    copier_header: Option<Vec<u8>>,
    mapping: Mapping,
    source: Option<PathBuf>,
}

impl fmt::Debug for Rom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rom")
            .field("len", &self.data.len())
            .field("copier_header", &self.copier_header.is_some())
            .field("mapping", &self.mapping)
            .field("source", &self.source)
            .finish()
    }
}

impl Rom {
    /// Loads a ROM image from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, RomError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| RomError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut rom = Self::from_bytes(bytes)?;
        rom.source = Some(path.to_path_buf());
        Ok(rom)
    }

    /// Builds a ROM from raw file contents, stripping a copier header if
    /// one is present.
    pub fn from_bytes(mut bytes: Vec<u8>) -> Result<Self, RomError> {
        let copier_header = match bytes.len() % BANK_LEN {
            0 => None,
            COPIER_HEADER_LEN => {
                let header = bytes.drain(..COPIER_HEADER_LEN).collect();
                Some(header)
            }
            _ => return Err(RomError::BadSize(bytes.len())),
        };
        if bytes.len() < INTERNAL_HEADER.as_usize() + 0x20 {
            return Err(RomError::TooSmall);
        }
        let map_mode = bytes[INTERNAL_HEADER.as_usize() + 0x15];
        let mapping =
            Mapping::from_map_mode(map_mode).ok_or(RomError::UnsupportedMapping(map_mode))?;
        Ok(Self {
            data: bytes,
            copier_header,
            mapping,
            source: None,
        })
    }

    /// The headerless image.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Size of the headerless image in bytes.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn has_copier_header(&self) -> bool {
        self.copier_header.is_some()
    }

    pub fn mapping(&self) -> Mapping {
        self.mapping
    }

    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    pub fn internal_header(&self) -> InternalHeader {
        let start = INTERNAL_HEADER.as_usize();
        InternalHeader::parse(&self.data[start..start + 0x20])
    }

    /// Computes the checksum the internal header should hold for this image.
    ///
    /// For sizes that are not a power of two, the tail past the largest
    /// power of two is counted repeatedly, as if mirrored up to that size.
    pub fn compute_checksum(&self) -> u16 {
        let len = self.data.len();
        let sum = |slice: &[u8]| slice.iter().map(|&b| b as u32).sum::<u32>();
        let head_len = if len.is_power_of_two() {
            len
        } else {
            1usize << (usize::BITS - 1 - len.leading_zeros())
        };
        let (head, tail) = self.data.split_at(head_len);
        let mut total = sum(head);
        if !tail.is_empty() {
            let repeats = (head_len / tail.len()) as u32;
            total = total.wrapping_add(sum(tail).wrapping_mul(repeats));
        }
        total as u16
    }

    pub fn sha1(&self) -> [u8; 20] {
        Sha1::digest(&self.data).into()
    }

    pub fn sha1_hex(&self) -> String {
        self.sha1().iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn identify(&self) -> RomIdentity {
        if self.sha1() == VANILLA_USA_SHA1 {
            RomIdentity::VanillaUsa
        } else {
            RomIdentity::Unknown
        }
    }

    pub fn pc(&self, addr: SnesAddr) -> Result<PcAddr, MapError> {
        self.mapping.snes_to_pc(addr)
    }

    /// Reads `len` bytes starting at a SNES address.
    pub fn read(&self, addr: SnesAddr, len: usize) -> Result<&[u8], RomError> {
        let pc = self.pc(addr)?;
        let start = pc.as_usize();
        self.data
            .get(start..start + len)
            .ok_or(RomError::OutOfBounds { addr, pc, len })
    }

    pub fn read_u8(&self, addr: SnesAddr) -> Result<u8, RomError> {
        Ok(self.read(addr, 1)?[0])
    }

    pub fn read_u16(&self, addr: SnesAddr) -> Result<u16, RomError> {
        let b = self.read(addr, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    /// Reads a 24-bit little-endian value, typically a long pointer.
    pub fn read_u24(&self, addr: SnesAddr) -> Result<u32, RomError> {
        let b = self.read(addr, 3)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], 0]))
    }

    /// Reads a 24-bit pointer and wraps it as an address.
    pub fn read_ptr(&self, addr: SnesAddr) -> Result<SnesAddr, RomError> {
        Ok(SnesAddr::new(self.read_u24(addr)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal 32 KiB image with a plausible LoROM header.
    fn fake_rom(map_mode: u8) -> Vec<u8> {
        let mut data = vec![0u8; BANK_LEN];
        let h = INTERNAL_HEADER.as_usize();
        data[h..h + 0x15].copy_from_slice(b"FAKE ROM             ");
        data[h + 0x15] = map_mode;
        data[h + 0x17] = 0x05;
        data
    }

    #[test]
    fn strips_copier_header() {
        let mut bytes = vec![0xAAu8; COPIER_HEADER_LEN];
        bytes.extend(fake_rom(0x20));
        let rom = Rom::from_bytes(bytes).unwrap();
        assert!(rom.has_copier_header());
        assert_eq!(rom.len(), BANK_LEN);
        assert_eq!(rom.internal_header().title, "FAKE ROM");
    }

    #[test]
    fn accepts_headerless() {
        let rom = Rom::from_bytes(fake_rom(0x20)).unwrap();
        assert!(!rom.has_copier_header());
        assert_eq!(rom.mapping(), Mapping::LoRom);
    }

    #[test]
    fn detects_sa1() {
        let rom = Rom::from_bytes(fake_rom(0x23)).unwrap();
        assert_eq!(rom.mapping(), Mapping::Sa1Rom);
    }

    #[test]
    fn rejects_odd_sizes() {
        assert!(matches!(
            Rom::from_bytes(vec![0; 1000]),
            Err(RomError::BadSize(1000))
        ));
    }

    #[test]
    fn rejects_hirom() {
        assert!(matches!(
            Rom::from_bytes(fake_rom(0x21)),
            Err(RomError::UnsupportedMapping(0x21))
        ));
    }

    #[test]
    fn reads_go_through_mapping() {
        let mut data = fake_rom(0x20);
        data[0x1234] = 0x42;
        data[0x1235] = 0x43;
        data[0x1236] = 0x44;
        let rom = Rom::from_bytes(data).unwrap();
        let addr = SnesAddr::new(0x009234);
        assert_eq!(rom.read_u8(addr).unwrap(), 0x42);
        assert_eq!(rom.read_u16(addr).unwrap(), 0x4342);
        assert_eq!(rom.read_u24(addr).unwrap(), 0x444342);
        assert!(matches!(
            rom.read_u8(SnesAddr::new(0x018000)),
            Err(RomError::OutOfBounds { .. })
        ));
        assert!(matches!(
            rom.read_u8(SnesAddr::new(0x000000)),
            Err(RomError::Map(_))
        ));
    }

    #[test]
    fn checksum_power_of_two() {
        let mut data = fake_rom(0x20);
        data[0] = 0xFF;
        data[1] = 0x02;
        let rom = Rom::from_bytes(data).unwrap();
        let expected: u32 = rom.data().iter().map(|&b| b as u32).sum();
        assert_eq!(rom.compute_checksum(), expected as u16);
    }

    #[test]
    fn checksum_mirrors_tail() {
        // 3 banks: 2 banks of head, 1 bank of tail counted twice.
        let mut data = fake_rom(0x20);
        data.extend(vec![0u8; BANK_LEN]);
        data.extend(vec![1u8; BANK_LEN]);
        let rom = Rom::from_bytes(data).unwrap();
        let head: u32 = rom.data()[..2 * BANK_LEN].iter().map(|&b| b as u32).sum();
        let expected = head + 2 * BANK_LEN as u32;
        assert_eq!(rom.compute_checksum(), expected as u16);
    }
}
