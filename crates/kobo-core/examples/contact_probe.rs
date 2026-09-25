//! Which of a block's actions fire as the player meets it in different
//! ways, observed by running the ROM (`expand::play_level`): for learning,
//! from memory effects alone, when Lunar Magic's acts-like chain runs each
//! custom block action, and for checking Kobo's against it.
//!
//! `contact_probe place in.mwl out.mwl x y` adds tile `$200` at (x, y) to an
//! MWL file (Lunar Magic's object `27`). `contact_probe run rom.sfc [x y]`
//! plays level `105` with the player put next to the tile at (x, y),
//! default (6, 20), in each scenario, for small and big Mario, and prints
//! the actions the probe block (`tools/lunar-magic/block-probe/probe.asm`)
//! logged, with where the game sampled, relative to the player.

use kobo_core::level::objects::Object;
use kobo_core::mwl::MwlFile;
use kobo_core::{Rom, expand, ram};

const ACTIONS: [&str; 12] = [
    "below",
    "above",
    "side",
    "sprite v",
    "sprite h",
    "cape",
    "fireball",
    "top corner",
    "body",
    "head",
    "wall feet",
    "wall body",
];
const LOG: u32 = 0x7F_B40F;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("place") => place(
            &args[1],
            &args[2],
            args[3].parse().unwrap(),
            args[4].parse().unwrap(),
        ),
        Some("run") => {
            let at = |i: usize, default| args.get(i).map_or(default, |s| s.parse().unwrap());
            run(&args[1], at(2, 6), at(3, 20))
        }
        _ => eprintln!("usage: contact_probe place in.mwl out.mwl x y | run rom.sfc [x y]"),
    }
}

fn place(input: &str, output: &str, x: u16, y: u16) {
    let bytes = std::fs::read(input).unwrap();
    let mut mwl = MwlFile::parse(&bytes).unwrap().decode(None).unwrap();
    // One tile: HHHHWWWW, 00BBBBBB, bbbbbbbb.
    mwl.layer1.data.objects.push(Object::Lunar {
        number: 0x27,
        x,
        y,
        data: vec![0x00, 0x02, 0x00],
    });
    std::fs::write(output, mwl.to_file(None).unwrap().to_bytes()).unwrap();
}

fn run(path: &str, tile_x: i32, tile_y: i32) {
    let rom = Rom::load(path).unwrap();
    let (bx, by) = (tile_x * 16, tile_y * 16);
    // Name, and the player's position and speeds on the first frame.
    let scenarios: [(&str, i32, i32, i8, i8); 12] = [
        ("fall onto", bx, by - 40, 0, 0x30),
        ("jump into from below", bx, by + 20, 0, -0x50),
        ("run into left side", bx - 20, by - 12, 0x30, 0),
        ("run into right side", bx + 20, by - 12, -0x30, 0),
        ("one foot on, right", bx + 8, by - 34, 0, 0x10),
        ("one foot on, left", bx - 8, by - 34, 0, 0x10),
        ("walk off right", bx + 2, by - 32, 0x18, 0),
        ("walk off left", bx - 2, by - 32, -0x18, 0),
        ("stand on right edge", bx + 13, by - 32, 0, 0x10),
        ("stand on left edge", bx - 13, by - 32, 0, 0x10),
        ("overlap", bx, by - 16, 0, 0),
        ("overlap low", bx, by - 8, 0, 0),
    ];
    for (powerup, size) in [(0u8, "small"), (1, "big")] {
        for (name, x, y, x_speed, y_speed) in scenarios {
            let ram = expand::play_level(&rom, 0x105, 12, |frame, ram| {
                if frame == 0 {
                    ram.set_u8(ram::RamAddr::new(0x7E_0019), powerup);
                    ram.set_u16(ram::PLAYER_X, x as u16);
                    ram.set_u16(ram::PLAYER_Y, y as u16);
                    ram.set_u8(ram::PLAYER_X_SPEED, x_speed as u8);
                    ram.set_u8(ram::PLAYER_Y_SPEED, y_speed as u8);
                    ram.set_u8(ram::RamAddr::new(LOG), 0);
                }
            })
            .unwrap();
            let mut seen = Vec::new();
            for entry in 0..ram.u8(ram::RamAddr::new(LOG)) as u32 {
                let byte = |k: u32| ram.u8(ram::RamAddr::new(LOG + 1 + 16 * entry + k)) as i32;
                let word = |k: u32| byte(k) | byte(k + 1) << 8;
                let (touch_x, touch_y) = (word(3), word(1));
                let (player_x, player_y) = (word(5), word(7));
                let text = format!(
                    "{}@({:+},{:+})",
                    ACTIONS[byte(0) as usize],
                    touch_x - player_x,
                    touch_y - player_y
                );
                if !seen.contains(&text) {
                    seen.push(text);
                }
            }
            println!("{size:5} {name:22} {}", seen.join(" "));
        }
    }
}
