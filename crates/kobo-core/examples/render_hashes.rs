//! Prints a SHA-1 of every level's rendered picture, with sprites drawn
//! and again as markers without the player: diff the output of two builds
//! to check that a change to `expand` or `render` left every picture
//! byte-identical. Levels that fail to render print their error instead.
//!
//! `cargo run --release --example render_hashes -- rom.smc > before.txt`

use std::thread;

use kobo_core::Rom;
use kobo_core::render::{self, RenderOptions, Sprites};
use sha1::{Digest, Sha1};

fn picture_hash(rom: &Rom, level: u16, options: RenderOptions) -> String {
    match render::render_level(rom, level, options) {
        Ok(rendered) => {
            let image = rendered.image;
            let mut hasher = Sha1::new();
            hasher.update(image.width.to_le_bytes());
            hasher.update(image.height.to_le_bytes());
            hasher.update(image.pixels.as_flattened());
            let hash = hasher.finalize();
            hash.iter().map(|b| format!("{b:02x}")).collect()
        }
        Err(e) => format!("error: {e}"),
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: render_hashes <rom>");
    let rom = Rom::load(&path).expect("ROM must load");
    let workers = thread::available_parallelism().map_or(1, |n| n.get());
    let levels: Vec<u16> = (0..0x200).collect();
    let lines: Vec<String> = thread::scope(|scope| {
        let handles: Vec<_> = levels
            .chunks(levels.len().div_ceil(workers))
            .map(|chunk| {
                let rom = &rom;
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|&level| {
                            let drawn = picture_hash(rom, level, RenderOptions::default());
                            let markers = picture_hash(
                                rom,
                                level,
                                RenderOptions {
                                    sprites: Sprites::Markers,
                                    player: false,
                                },
                            );
                            format!("{level:03X} {drawn} {markers}")
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("worker must not panic"))
            .collect()
    });
    for line in lines {
        println!("{line}");
    }
}
