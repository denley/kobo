//! Cases for Lunar Magic's placed objects (`22`, `23`, `27`, `29`), for
//! learning from memory effects what its object code writes into a level's
//! grid and checking Kobo's against it (docs/lunar-magic-install.md).
//!
//! `lm_objects make in.mwl out.mwl group` replaces the layer 1 objects of
//! a level's MWL export with a group of 16 cases, each on every other
//! screen so it has the next one to cross into; import the result with
//! Lunar Magic into a copy of vanilla, and `lm_objects grid rom.sfc level
//! group` prints, per case, the tiles the level's load left on its two
//! screens, as `x,y=tile`, relative to the first. Groups 0 and 1 are for a
//! horizontal level (`105`), group 2 for a vertical one (`1CE`), whose
//! screens are 16 rows of two 16-column halves. `lm_objects add in.mwl
//! out.mwl "40 60 0B"` adds one of Lunar Magic's settings objects, as its
//! bytes, to the end of layer 1.

use kobo_core::level::objects::Object;
use kobo_core::mwl::MwlFile;
use kobo_core::{Rom, expand};

/// Object number, position within its screen, and the bytes after the
/// second, as each form is documented (smwspeedruns' level data format).
fn cases() -> Vec<(&'static str, u8, u16, u16, Vec<u8>)> {
    let form = |f: u8, t: u16| [(f << 6) | (t >> 8) as u8 & 0x3F, t as u8];
    let mut out = vec![
        ("22 1x1", 0x22, 2, 5, vec![0x00, 0xAB]),
        ("22 4x3", 0x22, 2, 5, vec![0x23, 0xAB]),
        ("23 16x1 across", 0x23, 4, 5, vec![0x0F, 0xCD]),
        ("22 1x4 at the bottom", 0x22, 2, 24, vec![0x30, 0x30]),
    ];
    let mut push = |name, x, y, data: Vec<u8>| out.push((name, 0x27u8, x, y, data));
    push(
        "27 single 5x3",
        2,
        5,
        [&[0x24][..], &form(0, 0x2A5)].concat(),
    );
    push(
        "27 single across",
        14,
        5,
        [&[0x04][..], &form(0, 0x2A5)].concat(),
    );
    push(
        "27 block 4x3",
        2,
        5,
        [&[0x23][..], &form(1, 0x300)].concat(),
    );
    push(
        "27 block 16x1",
        0,
        5,
        [&[0x0F][..], &form(1, 0x3F0)].concat(),
    );
    push(
        "27 block across a page",
        2,
        5,
        [&[0x13][..], &form(1, 0x2FE)].concat(),
    );
    push(
        "27 stretch 7x6 of 3x2",
        2,
        5,
        [&[0x56][..], &form(2, 0x300), &[0x12]].concat(),
    );
    push(
        "27 stretch 2x2 of 4x3",
        2,
        5,
        [&[0x11][..], &form(2, 0x300), &[0x23]].concat(),
    );
    push(
        "27 stretch 7x7 of 3x3",
        2,
        5,
        [&[0x66][..], &form(2, 0x300), &[0x22]].concat(),
    );
    push(
        "27 stretch 4x4 of 4x4",
        2,
        5,
        [&[0x33][..], &form(2, 0x300), &[0x33]].concat(),
    );
    push(
        "27 stretch 5x1 of 1x1",
        2,
        5,
        [&[0x04][..], &form(2, 0x300), &[0x00]].concat(),
    );
    push(
        "27 wide 20x3 of 3x2",
        2,
        5,
        [&[0x13][..], &form(3, 0x300), &[0x12, 0x02]].concat(),
    );
    push(
        "27 wide 3x20 of 2x3",
        2,
        2,
        [&[0x02][..], &form(3, 0x300), &[0x21, 0x13]].concat(),
    );
    push(
        "27 flag 4x3 of 1x1, show",
        2,
        5,
        [&[0x83][..], &form(3, 0x300), &[0x00, 0x02, 0x05]].concat(),
    );
    push(
        "27 flag 4x3 of 1x1, +100",
        2,
        5,
        [&[0x83][..], &form(3, 0x300), &[0x00, 0x02, 0x86]].concat(),
    );
    out.push((
        "29 single 2x2",
        0x29,
        2,
        5,
        [&[0x11][..], &form(0, 0x0123)].concat(),
    ));
    out.push((
        "29 block 2x2",
        0x29,
        2,
        5,
        [&[0x11][..], &form(1, 0x0123)].concat(),
    ));
    out
}

/// Group 2, vertical: across into a screen's right half, and down into
/// the next screen.
fn vertical_cases() -> Vec<(&'static str, u8, u16, u16, Vec<u8>)> {
    let form = |f: u8, t: u16| [(f << 6) | (t >> 8) as u8 & 0x3F, t as u8];
    vec![
        (
            "v 27 block 4x3 across",
            0x27,
            14,
            5,
            [&[0x23][..], &form(1, 0x300)].concat(),
        ),
        (
            "v 27 single 1x20 down",
            0x27,
            2,
            5,
            [&[0xF0][..], &form(0, 0x2A5)].concat(),
        ),
        (
            "v 27 wide 21x2 of 3x2",
            0x27,
            2,
            5,
            [&[0x14][..], &form(3, 0x300), &[0x12, 0x01]].concat(),
        ),
        ("v 22 4x4 across and down", 0x22, 14, 14, vec![0x33, 0xAB]),
        (
            "v 27 wide 2x40 of 1x3",
            0x27,
            30,
            3,
            [&[0x01][..], &form(3, 0x300), &[0x20, 0x27]].concat(),
        ),
    ]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("make") => make(&args[1], &args[2], args[3].parse().unwrap()),
        Some("add") => add(&args[1], &args[2], &args[3]),
        Some("grid") => grid(
            &args[1],
            u16::from_str_radix(&args[2], 16).unwrap(),
            args[3].parse().unwrap(),
        ),
        _ => eprintln!("usage: lm_objects make in.mwl out.mwl group | grid rom.sfc level group"),
    }
}

fn group(group: usize) -> impl Iterator<Item = (&'static str, u8, u16, u16, Vec<u8>)> {
    let cases = if group == 2 {
        vertical_cases()
    } else {
        cases()
    };
    cases.into_iter().skip(16 * (group % 2)).take(16)
}

fn make(input: &str, output: &str, group_index: usize) {
    let bytes = std::fs::read(input).unwrap();
    let mut mwl = MwlFile::parse(&bytes).unwrap().decode(None).unwrap();
    let data = &mut mwl.layer1.data;
    let vertical = group_index == 2;
    // 32 screens, or 12 of a vertical level's two halves.
    data.header[0] = data.header[0] & 0xE0 | if vertical { 0x0B } else { 0x1F };
    data.objects = group(group_index)
        .enumerate()
        .map(|(i, (_, number, x, y, data))| {
            let step = i as u16 * 32;
            let (x, y) = if vertical {
                (x, step + y)
            } else {
                (step + x, y)
            };
            Object::Lunar { number, x, y, data }
        })
        .collect();
    std::fs::write(output, mwl.to_file(None).unwrap().to_bytes()).unwrap();
}

fn add(input: &str, output: &str, bytes: &str) {
    let mut mwl = MwlFile::parse(&std::fs::read(input).unwrap())
        .unwrap()
        .decode(None)
        .unwrap();
    let bytes = bytes
        .split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).unwrap())
        .collect();
    mwl.layer1.data.objects.push(Object::Unplaced(bytes));
    std::fs::write(output, mwl.to_file(None).unwrap().to_bytes()).unwrap();
}

fn grid(path: &str, level: u16, group_index: usize) {
    let rom = Rom::load(path).unwrap();
    let loaded = expand::expand_level(&rom, level).unwrap();
    let tiles = &loaded.tiles;
    for (i, (name, ..)) in group(group_index).enumerate() {
        let mut cells = Vec::new();
        let (rows, step) = if tiles.vertical {
            (32, (0, i * 32))
        } else {
            (tiles.rows, (i * 32, 0))
        };
        for dy in 0..rows {
            for dx in 0..32 {
                let tile = tiles.tile_at(step.0 + dx, step.1 + dy);
                if tile != 0x25 {
                    cells.push(format!("{dx},{dy}={tile:X}"));
                }
            }
        }
        println!("{name}: {}", cells.join(" "));
    }
}
