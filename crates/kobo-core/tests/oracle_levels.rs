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
