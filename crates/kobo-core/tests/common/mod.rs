//! Shared helpers for ROM-backed integration tests.

use kobo_core::{Rom, RomIdentity, SnesAddr, config};

/// The configured vanilla ROM, or `None` (after printing why) so the
/// calling test can return early and pass.
#[allow(dead_code)]
pub fn vanilla() -> Option<Rom> {
    match config::vanilla_rom_path() {
        Ok(path) => {
            let rom = Rom::load(&path).expect("configured vanilla ROM must load");
            assert_eq!(
                rom.identify(),
                RomIdentity::VanillaUsa,
                "configured vanilla ROM has the wrong headerless SHA-1: {}",
                path.display()
            );
            Some(rom)
        }
        Err(config::ConfigError::NoVanillaRom) => {
            assert!(
                std::env::var_os("KOBO_REQUIRE_ROM").is_none(),
                "strict validation requires a vanilla ROM"
            );
            eprintln!("skipping: no vanilla ROM configured");
            None
        }
        Err(e) => panic!("invalid ROM configuration: {e}"),
    }
}

/// The ROM the emulator dumps being compared against were made from:
/// `KOBO_ORACLE_ROM`, or else the vanilla ROM.
#[allow(dead_code)]
pub fn oracle_rom() -> Option<Rom> {
    match std::env::var_os("KOBO_ORACLE_ROM") {
        Some(path) => Some(Rom::load(path).expect("KOBO_ORACLE_ROM must load")),
        None => vanilla(),
    }
}

/// Lunar Magic hack ROMs to exercise, from the `:`-separated paths in
/// `KOBO_LM_ROMS`. Empty (after printing why) when the variable is unset.
#[allow(dead_code)]
pub fn lunar_magic_roms() -> Vec<(std::path::PathBuf, Rom)> {
    let Some(list) = std::env::var_os("KOBO_LM_ROMS") else {
        eprintln!("skipping Lunar Magic ROMs: KOBO_LM_ROMS is not set");
        return Vec::new();
    };
    assert!(!list.is_empty(), "KOBO_LM_ROMS is set but empty");
    std::env::split_paths(&list)
        .map(|p| {
            let rom = Rom::load(&p).expect("listed Lunar Magic ROM must load");
            (p, rom)
        })
        .collect()
}

/// A 512 KiB LoROM image laid out as the vanilla ROM is where a build
/// reads it, with no game data: every level's layer data is an empty list
/// and every sprite list empty, the unused space in bank `$07` is `$FF`,
/// and Lunar Magic's gate is clear.
#[allow(dead_code)]
pub fn synthetic_base() -> Rom {
    use kobo_core::level::{LEVEL_COUNT, tables};
    let mut data = vec![0xFF; 0x8_0000];
    data[0x7FC0..0x7FD5].copy_from_slice(b"KOBO SYNTHETIC BASE  ");
    data[0x7FD5] = 0x20;
    data[0x7FD7] = 0x09;
    let mut rom = Rom::from_bytes(data).unwrap();
    let empty_objects = SnesAddr::new(0x068000);
    rom.write(empty_objects, &[0, 0, 0, 0, 0, 0xFF]).unwrap();
    let empty_sprites = SnesAddr::new(0x07C000);
    rom.write(empty_sprites, &[0x00, 0xFF]).unwrap();
    for n in 0..LEVEL_COUNT as u32 {
        rom.write_ptr(tables::LAYER1_PTRS.add(3 * n), empty_objects)
            .unwrap();
        rom.write_ptr(tables::LAYER2_PTRS.add(3 * n), empty_objects)
            .unwrap();
        rom.write_u16(tables::SPRITE_PTRS.add(2 * n), empty_sprites.offset())
            .unwrap();
        for table in tables::SECONDARY_HEADERS {
            rom.write_u8(table.add(n), 0).unwrap();
        }
    }
    rom.fix_checksum().unwrap();
    rom
}
