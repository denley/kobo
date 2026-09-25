//! Dependency-free mutation smoke fuzzing shared by CI and the longer driver.
//! Reproduce a case with its seed; this is not coverage-guided fuzzing.

use kobo_core::level::objects::{self, Jumps, Layout};
use kobo_core::{Rom, SnesAddr, compress, gfx, sprites};

pub fn case(mut seed: u64) {
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let len = next() as usize % 2048;
    let input: Vec<u8> = (0..len).map(|_| next() as u8).collect();
    for decode in [
        compress::lz2::decompress,
        compress::lz3::decompress,
        compress::rle1::decompress,
    ] {
        for n in [0, input.len() / 2, input.len()] {
            if let Ok(result) = decode(&input[..n]) {
                assert!(result.consumed <= n);
                assert!(result.data.len() <= compress::MAX_OUTPUT);
            }
        }
    }
    // What decodes encodes again to the same thing, where it can be placed.
    for layout in [Layout::Horizontal, Layout::Vertical] {
        for jumps in [Jumps::Vanilla, Jumps::Tall] {
            if let Ok(data) = objects::decode(&input, layout, jumps) {
                assert!(data.len <= input.len());
                if let Ok(bytes) = objects::encode(data.header, &data.objects, layout, jumps) {
                    let again = objects::decode(&bytes, layout, jumps).unwrap();
                    assert_eq!(again.objects, data.objects);
                }
            }
        }
    }
    if let Ok(result) = compress::rle1::decompress(&input)
        && (1..=4096).contains(&result.data.len())
    {
        let packed = compress::rle1::compress(&result.data).unwrap();
        assert_eq!(
            compress::rle1::decompress(&packed).unwrap().data,
            result.data
        );
    }
    // Valid container shape, arbitrary headers and pointer operands. Without
    // a structured seed, almost every ROM mutation is rejected at BadSize.
    let mut bytes = vec![0; 0x8000];
    bytes[..input.len()].copy_from_slice(&input);
    for byte in &mut bytes[0x3800..0x3A30] {
        *byte = next() as u8;
    }
    for byte in &mut bytes[0x7FC0..] {
        *byte = next() as u8;
    }
    bytes[0x7FD5] = if next() & 1 == 0 { 0x20 } else { 0x23 };
    let rom = Rom::from_bytes(bytes).unwrap();
    let header = rom.internal_header();
    let _ = header.rom_size();
    let _ = header.sram_size();
    let _ = rom.compute_checksum();
    let addr = SnesAddr::new(next() as u32);
    let _ = rom.read(addr, usize::MAX);
    let _ = rom.read_tail(addr);
    let _ = gfx::read_gfx_file(&rom, next() as u8);
    let _ = sprites::read_sprites_at(&rom, SnesAddr::new(0x008000));
    let _ = sprites::read_sprites_at(&rom, addr);
}
