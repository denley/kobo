//! Compares the headless loader's tile grid against dumps taken from a
//! real emulator (see `tools/oracle/`). Runs only when `KOBO_ORACLE_DIR`
//! points at a directory of `level_XXX.l1lo.bin` / `.l1hi.bin` files.

mod common;

use kobo_core::expand;
use std::fs;
use std::path::PathBuf;

fn oracle_dir() -> Option<PathBuf> {
    match std::env::var_os("KOBO_ORACLE_DIR") {
        Some(d) => Some(PathBuf::from(d)),
        None => {
            eprintln!("skipping: KOBO_ORACLE_DIR is not set");
            None
        }
    }
}

#[test]
fn tile_grids_match_emulator_dumps() {
    let Some(rom) = common::vanilla() else { return };
    let Some(dir) = oracle_dir() else { return };
    let mut checked = 0;
    let mut failures = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".l1lo.bin"))
        .collect();
    entries.sort();
    for lo_path in entries {
        let name = lo_path.file_name().unwrap().to_string_lossy().to_string();
        let level = u16::from_str_radix(&name[6..9], 16).unwrap();
        let hi_path = dir.join(format!("level_{level:03X}.l1hi.bin"));
        let want_lo = fs::read(&lo_path).unwrap();
        let want_hi = fs::read(&hi_path).unwrap();
        match expand::expand_level(&rom, level) {
            Ok(tiles) => {
                let bad = (0..expand::GRID_LEN)
                    .filter(|&i| tiles.low[i] != want_lo[i] || tiles.high[i] != want_hi[i])
                    .count();
                if bad != 0 {
                    failures.push(format!("level {level:03X}: {bad} bytes differ"));
                }
            }
            Err(e) => failures.push(format!("level {level:03X}: {e}")),
        }
        checked += 1;
    }
    eprintln!("checked {checked} levels");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Capture with KOBO_ORACLE_VIDEO=1 after the title-screen loader guard.
/// Compare stable arena graphics, excluding animated characters and tilemaps.
#[test]
fn boss_graphics_match_emulator_dumps() {
    let Some(rom) = common::vanilla() else { return };
    let Some(dir) = std::env::var_os("KOBO_BOSS_ORACLE_DIR").map(PathBuf::from) else {
        eprintln!("skipping: KOBO_BOSS_ORACLE_DIR is not set");
        return;
    };
    for level in [0x096, 0x0CC, 0x0D9, 0x1C7] {
        let want = fs::read(dir.join(format!("level_{level:03X}.vram.bin"))).unwrap();
        assert_eq!(want.len(), 0x10000);
        let tiles = expand::expand_level(&rom, level).unwrap();
        for (name, addresses) in [
            (
                "Mode 7 characters",
                (1..0x8000).step_by(2).collect::<Vec<_>>(),
            ),
            ("layer 3 GFX", (0x8000..0xA000).collect()),
            ("arena tilemap", (0xB000..0xC000).collect()),
            ("SP3 characters", (0xE000..0xF000).collect()),
        ] {
            for address in addresses {
                assert_eq!(
                    tiles.vram[address], want[address],
                    "level {level:03X} {name} at {address:04X}"
                );
            }
        }
    }
}

/// The layer 3 tilemap the game uploaded during level preparation, below
/// the status bar rows the NMI handler rewrites every frame.
#[test]
fn layer3_tilemaps_match_emulator_dumps() {
    let Some(rom) = common::vanilla() else { return };
    let Some(dir) = oracle_dir() else { return };
    let mut failures = Vec::new();
    let mut checked = 0;
    for level in 0..0x200u16 {
        let Ok(want) = fs::read(dir.join(format!("level_{level:03X}.vram.bin"))) else {
            continue;
        };
        let tiles = expand::expand_level(&rom, level).unwrap();
        if tiles.boss_scene.is_some() {
            continue;
        }
        checked += 1;
        let region = 0xA140..0xC000;
        let bad = region.clone().filter(|&i| tiles.vram[i] != want[i]).count();
        if bad != 0 {
            let first = region.clone().find(|&i| tiles.vram[i] != want[i]).unwrap();
            failures.push(format!(
                "level {level:03X}: {bad} bytes differ, first at {first:04X}: {:02X} vs {:02X}",
                tiles.vram[first], want[first]
            ));
        }
    }
    eprintln!("checked {checked} levels");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
