//! Which of a block's actions fire as the player meets it in different
//! ways, observed by running the ROM (`expand::play_level`): for learning,
//! from memory effects alone, when Lunar Magic's acts-like chain runs each
//! custom block action, and for checking Kobo's against it.
//!
//! `contact_probe place in.mwl out.mwl x y` adds tile `$200` at (x, y) to an
//! MWL file (Lunar Magic's object `27`). `contact_probe run rom.sfc [x y]`
//! plays level `105` with the player put next to the tile at (x, y),
//! default (6, 20), which it writes into the level's grid first, in each
//! scenario, for small and big Mario, and prints
//! the actions the probe block (`tools/lunar-magic/block-probe/probe.asm`)
//! logged, with where the game sampled, relative to the player.
//! `contact_probe stand rom.sfc level x y tile...` drops the player onto
//! each tile in turn, written into the grid at (x, y) of a horizontal level
//! (or of a vertical level's first screen), and prints whether they landed
//! and the low byte of the tile the game was told it acts like (`$1693`),
//! and, on a probe ROM, the actions the probe block logged. An argument
//! `addr=value` (hex) instead of a tile writes that RAM byte on every frame.

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
        Some("stand") => {
            let n = |i: usize| u16::from_str_radix(&args[i], 16).unwrap();
            let (pokes, tiles): (Vec<_>, Vec<_>) = args[5..].iter().partition(|a| a.contains('='));
            let tiles: Vec<u16> = tiles
                .iter()
                .map(|t| u16::from_str_radix(t, 16).unwrap())
                .collect();
            let pokes: Vec<(u32, u8)> = pokes
                .iter()
                .map(|p| {
                    let (a, v) = p.split_once('=').unwrap();
                    (
                        u32::from_str_radix(a, 16).unwrap(),
                        u8::from_str_radix(v, 16).unwrap(),
                    )
                })
                .collect();
            let (x, y) = (args[3].parse().unwrap(), args[4].parse().unwrap());
            stand(&args[1], n(2), x, y, &tiles, &pokes)
        }
        _ => eprintln!(
            "usage: contact_probe place in.mwl out.mwl x y | run rom.sfc [x y] \
             | stand rom.sfc level x y tile..."
        ),
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
    // Tile $200 in the grid (horizontal, 27 rows a screen), as Lunar
    // Magic's object puts it and a ROM without that object code cannot.
    let index = (tile_x / 16 * 0x1B0 + tile_y * 16 + tile_x % 16) as u32;
    let place = |ram: &mut ram::Ram| {
        ram.set_u8(ram::RamAddr::new(0x7E_C800 + index), 0x00);
        ram.set_u8(ram::RamAddr::new(0x7F_C800 + index), 0x02);
    };
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
                    place(ram);
                    ram.set_u8(ram::RamAddr::new(0x7E_0019), powerup);
                    ram.set_u16(ram::PLAYER_X, x as u16);
                    ram.set_u16(ram::PLAYER_Y, y as u16);
                    ram.set_u8(ram::PLAYER_X_SPEED, x_speed as u8);
                    ram.set_u8(ram::PLAYER_Y_SPEED, y_speed as u8);
                    ram.set_u8(ram::RamAddr::new(LOG), 0);
                }
            })
            .unwrap();
            println!("{size:5} {name:22} {}", log(&ram).join(" "));
        }
    }
    // Sprites: slot 0 alone, the player out of the way. Number, status
    // (1 to start, $0A a kicked shell), position, speeds.
    let sprites: [(&str, u8, u8, i32, i32, i8, i8); 4] = [
        ("goomba dropped on", 0x0F, 0x01, bx, by - 20, 0, 0x10),
        ("shell into left side", 0x04, 0x0A, bx - 20, by, 0x30, 0),
        ("shell into right side", 0x04, 0x0A, bx + 20, by, -0x30, 0),
        ("shell up into", 0x04, 0x0A, bx, by + 20, 0, -0x40),
    ];
    for (name, number, status, x, y, x_speed, y_speed) in sprites {
        let ram = expand::play_level(&rom, 0x105, 16, |frame, ram| {
            if frame == 0 {
                place(ram);
                ram.fill(ram::SPRITE_STATUS, ram.map().sprite_slots(), 0);
                ram.set_u16(ram::PLAYER_X, (bx + 0x80) as u16);
                ram.set_u8_at(ram::SPRITE_NUMBER, 0, number);
                ram.set_u8_at(ram::SPRITE_STATUS, 0, status);
                ram.set_u8_at(ram::SPRITE_X_LOW, 0, x as u8);
                ram.set_u8_at(ram::SPRITE_X_HIGH, 0, (x >> 8) as u8);
                ram.set_u8_at(ram::SPRITE_Y_LOW, 0, y as u8);
                ram.set_u8_at(ram::SPRITE_Y_HIGH, 0, (y >> 8) as u8);
                ram.set_u8_at(ram::RamAddr::new(0x7E_00B6), 0, x_speed as u8);
                ram.set_u8_at(ram::RamAddr::new(0x7E_00AA), 0, y_speed as u8);
                ram.set_u8(ram::RamAddr::new(LOG), 0);
            }
        })
        .unwrap();
        println!("sprite {name:22} {}", log(&ram).join(" "));
    }
    // The cape, spinning beside the block, and a fireball thrown into it
    // (extended sprite 5, in one of the player's two slots).
    for (name, side) in [
        ("cape spin, left of it", -12i32),
        ("cape spin, right of it", 12),
    ] {
        let ram = expand::play_level(&rom, 0x105, 12, |frame, ram| {
            if frame == 0 {
                place(ram);
                ram.set_u8(ram::RamAddr::new(0x7E_0019), 2);
                ram.set_u16(ram::PLAYER_X, (bx + side) as u16);
                ram.set_u16(ram::PLAYER_Y, (by - 16) as u16);
                ram.set_u8(ram::RamAddr::new(LOG), 0);
            }
            ram.set_u8(ram::RamAddr::new(0x7E_14A6), 0x12);
        })
        .unwrap();
        println!("cape   {name:22} {}", log(&ram).join(" "));
    }
    for (name, x, y, speed, fall) in [
        ("fireball from the left", bx - 24, by + 4, 0x30i8, 0i8),
        ("fireball from the right", bx + 24, by + 4, -0x30, 0),
        ("fireball in it", bx + 4, by + 4, 0x30, 0),
        ("fireball onto it", bx + 4, by - 12, 0x10, 0x30),
    ] {
        let ram = expand::play_level(&rom, 0x105, 12, |frame, ram| {
            if frame == 0 {
                place(ram);
                ram.set_u16(ram::PLAYER_X, (bx + 0x80) as u16);
                ram.set_u8(ram::RamAddr::new(0x7E_170B + 8), 5);
                ram.set_u8(ram::RamAddr::new(0x7E_171F + 8), x as u8);
                ram.set_u8(ram::RamAddr::new(0x7E_1733 + 8), (x >> 8) as u8);
                ram.set_u8(ram::RamAddr::new(0x7E_1715 + 8), y as u8);
                ram.set_u8(ram::RamAddr::new(0x7E_1729 + 8), (y >> 8) as u8);
                ram.set_u8(ram::RamAddr::new(0x7E_1747 + 8), speed as u8);
                ram.set_u8(ram::RamAddr::new(0x7E_173D + 8), fall as u8);
                ram.set_u8(ram::RamAddr::new(LOG), 0);
            }
        })
        .unwrap();
        println!("fire   {name:22} {}", log(&ram).join(" "));
    }
}

fn stand(path: &str, level: u16, tile_x: i32, tile_y: i32, tiles: &[u16], pokes: &[(u32, u8)]) {
    let rom = Rom::load(path).unwrap();
    let vertical = kobo_core::level::read_primary_header(&rom, level)
        .unwrap()
        .level_mode
        .layer1_vertical();
    let index = if vertical {
        (tile_y * 16 + tile_x) as u32
    } else {
        (tile_x / 16 * 0x1B0 + tile_y * 16 + tile_x % 16) as u32
    };
    for &tile in tiles {
        let ram = expand::play_level(&rom, level, 16, |frame, ram| {
            if frame == 0 {
                let [low, high] = tile.to_le_bytes();
                ram.set_u8(ram::RamAddr::new(0x7E_C800 + index), low);
                ram.set_u8(ram::RamAddr::new(0x7F_C800 + index), high);
                ram.set_u16(ram::PLAYER_X, (tile_x * 16) as u16);
                ram.set_u16(ram::PLAYER_Y, (tile_y * 16 - 40) as u16);
                ram.set_u8(ram::PLAYER_Y_SPEED, 0x30);
                ram.set_u8(ram::RamAddr::new(LOG), 0);
            }
            for &(addr, value) in pokes {
                ram.set_u8(ram::RamAddr::new(addr), value);
            }
        })
        .unwrap();
        let on_ground = ram.u8(ram::RamAddr::new(0x7E_13EF)) != 0;
        let y = ram.u16(ram::PLAYER_Y) as i32 - (tile_y * 16 - 32);
        println!(
            "{tile:04X}: {} (y {y:+}), $1693 = {:02X} {}",
            if on_ground { "landed" } else { "in the air" },
            ram.u8(ram::RamAddr::new(0x7E_1693)),
            log(&ram).join(" ")
        );
    }
}

/// The probe block's log: each action once, with where the game sampled
/// relative to the player (or, for a sprite, where it sampled).
fn log(ram: &ram::Ram) -> Vec<String> {
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
    seen
}
