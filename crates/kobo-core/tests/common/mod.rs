//! Shared helpers for ROM-backed integration tests.

use kobo_core::{Rom, RomIdentity, config};

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
