//! Sprite lists must parse to exactly the bytes Lunar Magic stored. Lunar
//! Magic wraps relocated sprite data in a RATS block whose tag precedes the
//! header byte, so the block size is an independent statement of where the
//! list ends: a parser that stops early or overruns disagrees with it.
//! Runs on the vanilla ROM (no RATS tags, so only requiring clean parses)
//! and on every hack in `KOBO_LM_ROMS`.

mod common;

use kobo_core::expand::{self, ExpandError};
use kobo_core::{Rom, sprites};

/// The size of the RATS block whose data starts at `data`, if a valid tag
/// immediately precedes it.
fn rats_size(rom: &Rom, data: kobo_core::addr::SnesAddr) -> Option<usize> {
    let off = rom
        .mapping()
        .snes_to_pc(data)
        .ok()?
        .as_usize()
        .checked_sub(8)?;
    let tag = rom.data().get(off..off + 8)?;
    if &tag[..4] != b"STAR" {
        return None;
    }
    let size = u16::from_le_bytes([tag[4], tag[5]]);
    let complement = u16::from_le_bytes([tag[6], tag[7]]);
    (size == !complement).then_some(size as usize + 1)
}

/// (levels parsed, levels whose length a RATS tag confirmed).
fn check_rom(name: &str, rom: &Rom) -> (usize, usize) {
    let mut parsed = 0;
    let mut confirmed = 0;
    for level in 0..0x200u16 {
        let tiles = match expand::expand_level(rom, level) {
            Ok(t) => t,
            Err(ExpandError::MissingBackgroundTable(_)) => continue,
            Err(e) => panic!("{name} level {level:03X}: {e}"),
        };
        let start = tiles.sprite_data_ptr();
        let list = sprites::read_sprites_at(rom, start)
            .unwrap_or_else(|e| panic!("{name} level {level:03X}: {e}"));
        parsed += 1;
        if let Some(size) = rats_size(rom, start) {
            assert_eq!(
                list.len, size,
                "{name} level {level:03X}: parsed {} bytes at {start} but the RATS block holds {size}",
                list.len
            );
            confirmed += 1;
        }
    }
    (parsed, confirmed)
}

#[test]
fn sprite_lists_match_their_rats_blocks() {
    let Some(vanilla) = common::vanilla() else {
        return;
    };
    let (parsed, _) = check_rom("vanilla", &vanilla);
    assert_eq!(parsed, 0x200);
    for (path, rom) in common::lunar_magic_roms() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let (parsed, confirmed) = check_rom(&name, &rom);
        assert!(confirmed > 0, "{name}: no RATS-wrapped sprite lists found");
        eprintln!("{name}: {parsed} lists parsed, {confirmed} confirmed by RATS tags");
    }
}
