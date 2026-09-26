//! What each bit of a level's settings does to its entry, as a ROM's own
//! code has it (`expand::enter_by_exit`): for learning from memory effects
//! what Lunar Magic's entrance code does with its per-level tables, and
//! checking Kobo's against it.
//!
//! `entry_probe effects rom.sfc level` flips each bit of the level's eight
//! settings bytes (`$05F000`, `$05F200`, `$05F400`, `$05F600`, `$05DE00`,
//! `$06FA00`, `$06FC00`, `$06FE00`) in turn and prints the work RAM the
//! level's main entrance then leaves differently. `entry_probe compare
//! a.sfc b.sfc level` runs every value of every byte in both ROMs and
//! prints where their entries differ.

use kobo_core::{Rom, SnesAddr, expand, ram};

const TABLES: [(&str, u32); 8] = [
    ("05F000", 0x05F000),
    ("05F200", 0x05F200),
    ("05F400", 0x05F400),
    ("05F600", 0x05F600),
    ("05DE00", 0x05DE00),
    ("06FA00", 0x06FA00),
    ("06FC00", 0x06FC00),
    ("06FE00", 0x06FE00),
];

/// The work RAM an entry through the level's main entrance leaves, but the
/// stack and the scratch at `$00`-`$0F`.
fn entry(rom: &Rom, level: u16) -> Vec<u8> {
    let high = 0x04 | (level >> 8) as u8;
    let ram = expand::enter_by_exit(rom, level as u8, high, false, 0, |_| {}).unwrap();
    let mut bytes = ram.bytes(ram::RamAddr::new(0x7E_0000), 0x2000);
    bytes[0x100..0x200].fill(0);
    bytes[..0x10].fill(0);
    bytes
}

fn with(rom: &Rom, table: u32, level: u16, value: u8) -> Rom {
    let mut rom = Rom::from_bytes(rom.data().to_vec()).unwrap();
    rom.write_u8(SnesAddr::new(table + level as u32), value).unwrap();
    rom
}

fn diff(a: &[u8], b: &[u8]) -> String {
    let parts: Vec<String> = (0..a.len())
        .filter(|&i| a[i] != b[i])
        .map(|i| format!("{i:04X}:{:02X}>{:02X}", a[i], b[i]))
        .collect();
    parts.join(" ")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("effects") => {
            let rom = Rom::load(&args[1]).unwrap();
            let level = u16::from_str_radix(&args[2], 16).unwrap();
            let base = entry(&rom, level);
            for (name, table) in TABLES {
                let value = rom.read_u8(SnesAddr::new(table + level as u32)).unwrap();
                for bit in (0..8).rev() {
                    let flipped = value ^ (1 << bit);
                    let after = entry(&with(&rom, table, level, flipped), level);
                    println!("{name} {value:02X}>{flipped:02X} bit {bit}: {}", diff(&base, &after));
                }
            }
        }
        Some("compare") => {
            let a = Rom::load(&args[1]).unwrap();
            let b = Rom::load(&args[2]).unwrap();
            let level = u16::from_str_radix(&args[3], 16).unwrap();
            let mut same = 0;
            for (name, table) in TABLES {
                for value in 0..=255u8 {
                    let x = entry(&with(&a, table, level, value), level);
                    let y = entry(&with(&b, table, level, value), level);
                    if x == y {
                        same += 1;
                    } else {
                        println!("{name}={value:02X}: {}", diff(&x, &y));
                    }
                }
            }
            println!("{same} of {} the same", 256 * TABLES.len());
        }
        _ => eprintln!("usage: entry_probe effects rom level | compare a b level"),
    }
}
