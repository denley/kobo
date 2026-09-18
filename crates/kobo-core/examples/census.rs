use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let rom = kobo_core::rom::Rom::load(&path).unwrap();
    let mut undrawn: BTreeMap<u8, Vec<u16>> = BTreeMap::new();
    for level in 0..0x200u16 {
        let Ok(loaded) = kobo_core::expand::expand_level(&rom, level) else {
            continue;
        };
        let Ok(list) = kobo_core::sprites::read_sprites_at(&rom, loaded.sprite_data_ptr()) else {
            continue;
        };
        match kobo_core::expand::capture_sprites(&rom, &loaded, &list) {
            Ok(scene) => {
                for (_, _, id) in scene.undrawn {
                    undrawn.entry(id).or_default().push(level);
                }
            }
            Err(e) => eprintln!("{level:03X}: {e}"),
        }
    }
    for (id, levels) in undrawn {
        let mut l = levels.clone();
        l.dedup();
        println!(
            "{id:02X}: {} entries, levels {:03X?}",
            levels.len(),
            &l[..l.len().min(12)]
        );
    }
}
