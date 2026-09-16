//! Shared helpers for ROM-backed integration tests.

use kobo_core::{Rom, config};

/// The configured vanilla ROM, or `None` (after printing why) so the
/// calling test can return early and pass.
#[allow(dead_code)]
pub fn vanilla() -> Option<Rom> {
    match config::vanilla_rom_path() {
        Ok(path) => Some(Rom::load(&path).expect("configured vanilla ROM must load")),
        Err(e) => {
            eprintln!("skipping: {e}");
            None
        }
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
    std::env::split_paths(&list)
        .map(|p| {
            let rom = Rom::load(&p).expect("listed Lunar Magic ROM must load");
            (p, rom)
        })
        .collect()
}
