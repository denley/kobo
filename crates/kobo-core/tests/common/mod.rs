//! Shared helpers for ROM-backed integration tests.

use kobo_core::{Rom, config};

/// The configured vanilla ROM, or `None` (after printing why) so the
/// calling test can return early and pass.
pub fn vanilla() -> Option<Rom> {
    match config::vanilla_rom_path() {
        Ok(path) => Some(Rom::load(&path).expect("configured vanilla ROM must load")),
        Err(e) => {
            eprintln!("skipping: {e}");
            None
        }
    }
}
